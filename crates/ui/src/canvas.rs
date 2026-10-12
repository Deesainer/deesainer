use deesainer_doc::{Document, NodeId, NodeKind};
use deesainer_geom::{Affine, Point, Rect};
use deesainer_render::{render_page, Options, Viewport};
use deesainer_text::{EditAction, TextMotion};
use deesainer_tools::{
    selection_bounds, Cursor, Handle, Input, Mods, Overlay, Pointer, ToolKind, Tools,
};
use egui::{pos2, vec2, Color32, Pos2, Rect as ERect, Sense, Stroke, StrokeKind, Vec2};
use serde_json::json;
use std::hash::{Hash, Hasher};

use crate::theme::TOKENS;
use crate::App;

pub const MIN_ZOOM: f64 = 0.02;
pub const MAX_ZOOM: f64 = 256.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub zoom: f64,
    pub pan: Vec2,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: vec2(100.0, 100.0),
        }
    }
}

impl Camera {
    pub fn to_screen(&self, origin: Pos2, p: Point) -> Pos2 {
        pos2(
            origin.x + self.pan.x + (p.x * self.zoom) as f32,
            origin.y + self.pan.y + (p.y * self.zoom) as f32,
        )
    }

    pub fn to_page(&self, origin: Pos2, s: Pos2) -> Point {
        Point::new(
            f64::from(s.x - origin.x - self.pan.x) / self.zoom,
            f64::from(s.y - origin.y - self.pan.y) / self.zoom,
        )
    }

    pub fn zoom_at(&mut self, anchor: Vec2, factor: f64) {
        let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let page = (anchor - self.pan) / self.zoom as f32;
        self.zoom = new_zoom;
        self.pan = anchor - page * new_zoom as f32;
    }

    pub fn fit(&mut self, area: Rect, size: Vec2, max_zoom: f64) {
        if area.width <= 0.0 && area.height <= 0.0 {
            return;
        }
        let margin = 0.1;
        let zx = f64::from(size.x) * (1.0 - margin) / area.width.max(1.0);
        let zy = f64::from(size.y) * (1.0 - margin) / area.height.max(1.0);
        self.zoom = zx.min(zy).clamp(MIN_ZOOM, max_zoom);
        let c = area.center();
        self.pan = size / 2.0 - vec2((c.x * self.zoom) as f32, (c.y * self.zoom) as f32);
    }
}

type CacheKey = (
    u64,
    NodeId,
    u64,
    (i32, i32),
    (u32, u32),
    Option<(NodeId, u64)>,
);

struct MeshCache {
    revision: u64,
    page: NodeId,
    bucket: i32,
    mesh: std::sync::Arc<deesainer_mesh::SceneMesh>,
    id: u64,
    pixels: Option<deesainer_gpu::TexturePixels>,
}

struct CanvasCallback {
    frame: deesainer_gpu::Frame,
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &egui_wgpu::wgpu::Device,
        queue: &egui_wgpu::wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut egui_wgpu::wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        if let Some(c) = resources.get_mut::<deesainer_gpu::Compositor>() {
            let _ = c.prepare(device, queue, encoder, &self.frame);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut egui_wgpu::wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(c) = resources.get::<deesainer_gpu::Compositor>() {
            c.paint(pass);
        }
    }
}

pub fn attach_wgpu(app: &mut App, state: &egui_wgpu::RenderState, samples: u32) {
    if state.adapter.get_info().device_type == egui_wgpu::wgpu::DeviceType::Cpu {
        return;
    }
    let compositor =
        deesainer_gpu::Compositor::new(&state.device, &state.queue, state.target_format, samples);
    app.gpu_health = Some(compositor.health());
    state.renderer.write().callback_resources.insert(compositor);
    app.gpu_compositor = true;
}

#[derive(Default)]
pub struct CanvasCache {
    key: Option<CacheKey>,
    texture: Option<egui::TextureHandle>,
    mesh: Option<MeshCache>,
    mesh_failed: Option<(u64, NodeId, String)>,
    pub(crate) drew_mesh: bool,
    mesh_serial: u64,
    mesh_textures: std::collections::HashMap<deesainer_mesh::TextureRef, egui::TextureHandle>,
    pub last_size: Vec2,
    pub last_origin: Pos2,
}

impl CanvasCache {
    pub fn mesh_fallback(&self) -> Option<String> {
        self.mesh_failed.as_ref().map(|(_, _, why)| why.clone())
    }
}

fn mods(i: &egui::InputState) -> Mods {
    Mods {
        shift: i.modifiers.shift,
        alt: i.modifiers.alt,
        command: i.modifiers.command,
    }
}

pub fn content_bounds(doc: &Document, page: NodeId) -> Option<Rect> {
    doc.get(page)?
        .children
        .iter()
        .filter_map(|c| deesainer_render::render_bounds(doc, *c))
        .reduce(Rect::union)
}

pub fn canvas(app: &mut App, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let origin = rect.min;
    app.canvas.last_size = rect.size();
    app.canvas.last_origin = origin;
    let page = app.session.current_page();

    if !app.fitted {
        if let Some(b) = content_bounds(app.session.doc(), page) {
            app.camera.fit(b, rect.size(), 1.0);
        }
        app.fitted = true;
    }

    handle_view_input(app, ui, &response, origin);
    let on_handle = gradient_handles(app, ui, origin)
        || crate::connect::connect_handle(app, ui, origin)
        || network_handles(app, ui, origin)
        || ruler_guides(app, ui, rect);
    if !on_handle {
        handle_pointer(app, ui, &response, origin);
    }
    text_input(app, ui, origin);
    if let Some(drop) = response.dnd_release_payload::<crate::assets::AssetDrag>() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            let at = app.camera.to_page(origin, pos);
            crate::assets::place(app, drop.0, at);
        }
    }
    if response.secondary_clicked() {
        if let Some(h) = app.tools.hover() {
            if !app.session.selection().contains(&h) {
                app.dispatch("selection.set", json!({ "ids": [h] }));
            }
        }
    }
    crate::widgets::context_menu(&response, |ui| {
        if app.session.selection().is_empty() {
            crate::menu::canvas_context(app, ui);
        } else {
            crate::menu::object_context(app, ui, false);
        }
    });
    app.canvas.drew_mesh = draw_mesh(app, ui, rect);
    if !app.canvas.drew_mesh {
        draw_render(app, ui, rect);
    }
    if app.show_layout_guides {
        draw_layout_grids(app, ui, rect);
    }
    draw_overlays(app, ui, rect);
    if app.show_rulers {
        draw_rulers(app, ui, rect);
    }
    text_caret(app, ui, origin);
    set_cursor(app, ui, &response, origin);
}

fn handle_view_input(app: &mut App, ui: &mut egui::Ui, response: &egui::Response, origin: Pos2) {
    let (scroll, zoom_delta, pointer, middle_down, space) = ui.input(|i| {
        (
            i.smooth_scroll_delta,
            i.zoom_delta(),
            i.pointer.hover_pos(),
            i.pointer.middle_down(),
            i.key_down(egui::Key::Space),
        )
    });
    if !response.hovered() {
        return;
    }
    let anchor = pointer.map_or(app.canvas.last_size / 2.0, |p| p - origin);
    if (zoom_delta - 1.0).abs() > 1e-4 {
        app.camera.zoom_at(anchor, f64::from(zoom_delta));
    } else if scroll != Vec2::ZERO {
        app.camera.pan += scroll;
    }
    if middle_down || (space && response.dragged()) {
        app.camera.pan += response.drag_delta();
    }
}

fn handle_pointer(app: &mut App, ui: &mut egui::Ui, response: &egui::Response, origin: Pos2) {
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    if space
        || app.editing.is_some() && response.clicked() && !editing_rect_contains(app, ui, origin)
    {
        if app.editing.is_some() && response.clicked() {
            app.commit_editing();
        }
        if space {
            return;
        }
    }
    let (pressed, released, pos, m) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.interact_pos().or(i.pointer.hover_pos()),
            mods(i),
        )
    });
    let Some(screen) = pos else {
        app.pointer_page = None;
        return;
    };
    let p = app.camera.to_page(origin, screen);
    app.pointer_page = response.hovered().then_some(p);
    if app.editing.is_some() && editing_rect_contains(app, ui, origin) {
        return;
    }
    if app.eyedropper {
        if pressed && response.hovered() {
            app.eyedropper = false;
            if let Some(color) = crate::eyedropper::sample(app, p) {
                let ids = app.session.selection().to_vec();
                crate::eyedropper::apply(app, &ids, color);
            }
        }
        return;
    }
    if app.placing_comment && pressed && response.hovered() {
        app.comment_draft = Some((p, String::new()));
        return;
    }
    let busy = app.tools.is_busy();
    if !response.hovered() && !busy && !response.dragged() {
        return;
    }
    let doc = app.session.snapshot();
    let selection = app.session.selection().to_vec();
    let input = Input {
        doc: &doc,
        page: app.session.current_page(),
        selection: &selection,
        zoom: app.camera.zoom,
        pixel_grid: app.snap_pixel_grid,
    };
    let mut outs = Vec::new();
    if crate::widgets::double_or_more(response) {
        let slop = 3.0 / app.camera.zoom;
        let hit = deesainer_vector::hit_test(&doc, input.page, p, slop, false);
        let path = deesainer_vector::hit_path(&doc, input.page, p, slop);
        let selected_inst = path.iter().copied().find(|id| {
            selection.contains(id)
                && doc
                    .get(*id)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Instance { .. }))
        });
        let inst = selected_inst
            .or(hit)
            .and_then(|h| doc.get(h))
            .filter(|n| matches!(n.kind, NodeKind::Instance { .. }));
        if let Some(inst) = inst {
            if let Some(target) = deesainer_engine::slots::layer_at(&doc, inst, p) {
                let id = inst.id;
                let is_text = doc.get(target).is_some_and(|n| n.text().is_some());
                if is_text && app.instance_layer == Some((id, target)) {
                    drop(doc);
                    app.start_instance_editing(id, target);
                    return;
                }
                drop(doc);
                app.dispatch("selection.set", json!({ "ids": [id] }));
                app.instance_layer = Some((id, target));
                return;
            }
        }
    }
    if crate::widgets::double_or_more(response) {
        outs.extend(
            app.tools
                .pointer(&input, Pointer::DoubleClick { pos: p, mods: m }),
        );
    } else if pressed && response.hovered() {
        if app.editing.is_some() {
            app.commit_editing();
            return;
        }
        outs.extend(app.tools.pointer(&input, Pointer::Down { pos: p, mods: m }));
    } else if released {
        outs.extend(app.tools.pointer(&input, Pointer::Up { pos: p, mods: m }));
    } else {
        outs.extend(app.tools.pointer(&input, Pointer::Move { pos: p, mods: m }));
    }
    drop(doc);
    app.run_outputs(outs);
}

fn editing_rect_contains(app: &App, ui: &egui::Ui, origin: Pos2) -> bool {
    ui.input(|i| i.pointer.interact_pos())
        .is_some_and(|p| editing_screen_rect(app, origin).is_some_and(|r| r.contains(p)))
}

fn screen_rect(cam: &Camera, origin: Pos2, r: &Rect) -> ERect {
    ERect::from_min_max(
        cam.to_screen(origin, r.origin()),
        cam.to_screen(origin, Point::new(r.max_x(), r.max_y())),
    )
}

const MESH_AUTO_NODES: usize = 1500;

const MAX_IMAGE_TEXTURE_SIDE: usize = 4096;

fn draw_mesh(app: &mut App, ui: &mut egui::Ui, rect: ERect) -> bool {
    use crate::RenderMode;
    let wanted = match app.render_mode {
        RenderMode::Cpu => false,
        RenderMode::Mesh => true,
        RenderMode::Auto => app.session.doc().len() > MESH_AUTO_NODES,
    };
    if !wanted || app.editing.is_some() {
        return false;
    }
    let page = app.session.current_page();
    let revision = app.session.revision();
    if app
        .canvas
        .mesh_failed
        .as_ref()
        .is_some_and(|(r, p, _)| *r == revision && *p == page)
    {
        return false;
    }
    let origin = rect.min;
    let scale = app.camera.zoom * f64::from(ui.ctx().pixels_per_point());
    let bucket = (scale.max(1e-6).log2() * 2.0).floor() as i32;
    let (a, b) = (
        app.camera.to_page(origin, rect.min),
        app.camera.to_page(origin, rect.max),
    );
    let view = Rect::new(
        a.x.min(b.x),
        a.y.min(b.y),
        (b.x - a.x).abs(),
        (b.y - a.y).abs(),
    );
    let fresh = app.canvas.mesh.as_ref().is_some_and(|m| {
        m.revision == revision
            && m.page == page
            && m.bucket == bucket
            && m.mesh.region.contains_rect(&view)
    });
    if !fresh {
        let region = view.inflate(view.width.max(view.height));
        let build_scale = 2f64.powf((f64::from(bucket) + 1.0) / 2.0);
        match deesainer_mesh::build(
            app.session.doc(),
            page,
            region,
            build_scale,
            Some(deesainer_engine::text_painter()),
        ) {
            Ok(mesh) if mesh.has_layers() && !app.gpu_compositor => {
                let why = "layers (group opacity, blend modes, masks or effects), which need GPU compositing (not available on the OpenGL backend)".to_owned();
                app.canvas.mesh = None;
                if app.render_mode == RenderMode::Mesh {
                    app.status =
                        Some((format!("Mesh renderer cannot draw this page: {why}"), false));
                }
                app.canvas.mesh_failed = Some((revision, page, why));
                return false;
            }
            Ok(mesh) => {
                app.canvas.mesh_serial += 1;
                app.canvas.mesh = Some(MeshCache {
                    revision,
                    page,
                    bucket,
                    mesh: std::sync::Arc::new(mesh),
                    id: app.canvas.mesh_serial,
                    pixels: None,
                });
                app.canvas.mesh_failed = None;
            }
            Err(e) => {
                app.canvas.mesh = None;
                if app.render_mode == RenderMode::Mesh {
                    app.status = Some((format!("Mesh renderer cannot draw this page: {e}"), false));
                }
                app.canvas.mesh_failed = Some((revision, page, e.0));
                return false;
            }
        }
    }
    if app.gpu_compositor {
        if let Some(e) = app
            .gpu_health
            .as_ref()
            .and_then(deesainer_gpu::Health::failure)
        {
            app.gpu_compositor = false;
            app.canvas.mesh = None;
            app.canvas.mesh_failed =
                Some((revision, page, format!("the GPU reported an error: {e}")));
            app.status = Some((
                format!("GPU compositing turned off after an error: {e}"),
                false,
            ));
            return false;
        }
        return draw_mesh_gpu(app, ui, rect);
    }
    let Some(cache) = &app.canvas.mesh else {
        return false;
    };
    let textures = &mut app.canvas.mesh_textures;
    textures.retain(|t, _| cache.mesh.textures.contains(t));
    let max_side = ui
        .ctx()
        .input(|i| i.max_texture_side)
        .min(MAX_IMAGE_TEXTURE_SIDE);
    for t in &cache.mesh.textures {
        if textures.contains_key(t) {
            continue;
        }
        let (name, pixels, options) = match t {
            deesainer_mesh::TextureRef::Image {
                asset,
                repeat,
                adjust,
            } => (
                format!("mesh-image-{asset}-{adjust}"),
                deesainer_mesh::texture_pixels_adjusted(
                    app.session.doc(),
                    asset,
                    &cache
                        .mesh
                        .adjustments
                        .get(adjust)
                        .copied()
                        .unwrap_or_default(),
                    max_side,
                ),
                if *repeat {
                    egui::TextureOptions::LINEAR_REPEAT
                } else {
                    egui::TextureOptions::LINEAR
                },
            ),
            deesainer_mesh::TextureRef::Gradient(key) => (
                format!("mesh-gradient-{key:x}"),
                cache
                    .mesh
                    .gradients
                    .get(key)
                    .map(|p| (p.size, p.rgba.clone())),
                egui::TextureOptions::LINEAR,
            ),
        };
        let Some((size, px)) = pixels else {
            continue;
        };
        let image = egui::ColorImage::from_rgba_premultiplied(size, &px);
        let handle = ui.ctx().load_texture(name, image, options);
        textures.insert(t.clone(), handle);
    }
    let cam = app.camera;
    let screen = |x: f32, y: f32| cam.to_screen(origin, Point::new(f64::from(x), f64::from(y)));
    let painter = ui.painter_at(rect);
    if let Some(bg) = cache.mesh.background {
        painter.rect_filled(
            rect,
            0.0,
            Color32::from_rgba_premultiplied(bg[0], bg[1], bg[2], bg[3]),
        );
    }
    for batch in &cache.mesh.batches {
        let (lo, hi) = (
            screen(batch.bounds[0], batch.bounds[1]),
            screen(batch.bounds[2], batch.bounds[3]),
        );
        if lo.x > rect.max.x || lo.y > rect.max.y || hi.x < rect.min.x || hi.y < rect.min.y {
            continue;
        }
        let clip = match batch.clip {
            Some(c) => rect.intersect(ERect::from_min_max(screen(c[0], c[1]), screen(c[2], c[3]))),
            None => rect,
        };
        if clip.width() <= 0.0 || clip.height() <= 0.0 {
            continue;
        }
        let texture_id = match &batch.texture {
            None => egui::TextureId::default(),
            Some(t) => match textures.get(t) {
                Some(handle) => handle.id(),
                None => continue,
            },
        };
        let textured = batch.texture.is_some();
        let vertices = batch
            .vertices
            .iter()
            .map(|v| egui::epaint::Vertex {
                pos: screen(v.pos[0], v.pos[1]),
                uv: if textured {
                    egui::pos2(v.uv[0], v.uv[1])
                } else {
                    egui::epaint::WHITE_UV
                },
                color: Color32::from_rgba_premultiplied(
                    v.color[0], v.color[1], v.color[2], v.color[3],
                ),
            })
            .collect();
        let mesh = egui::Mesh {
            indices: batch.indices.clone(),
            vertices,
            texture_id,
        };
        painter.with_clip_rect(clip).add(egui::Shape::mesh(mesh));
    }
    true
}

fn draw_mesh_gpu(app: &mut App, ui: &mut egui::Ui, rect: ERect) -> bool {
    let ppp = ui.ctx().pixels_per_point();
    let max_side = ui
        .ctx()
        .input(|i| i.max_texture_side)
        .min(MAX_IMAGE_TEXTURE_SIDE);
    let doc = app.session.doc();
    let Some(cache) = app.canvas.mesh.as_mut() else {
        return false;
    };
    let pixels = cache.pixels.get_or_insert_with(|| {
        let mut map = std::collections::BTreeMap::new();
        for t in &cache.mesh.textures {
            let pixels = match t {
                deesainer_mesh::TextureRef::Image { asset, adjust, .. } => {
                    let a = cache
                        .mesh
                        .adjustments
                        .get(adjust)
                        .copied()
                        .unwrap_or_default();
                    deesainer_mesh::texture_pixels_adjusted(doc, asset, &a, max_side)
                        .map(|(size, rgba)| deesainer_mesh::Pixels { size, rgba })
                }
                deesainer_mesh::TextureRef::Gradient(key) => cache.mesh.gradients.get(key).cloned(),
            };
            if let Some(p) = pixels {
                map.insert(t.clone(), p);
            }
        }
        std::sync::Arc::new(map)
    });
    let left = (rect.min.x * ppp).round();
    let top = (rect.min.y * ppp).round();
    let w = ((rect.max.x * ppp).round() - left).max(1.0);
    let h = ((rect.max.y * ppp).round() - top).max(1.0);
    let cam = app.camera;
    let frame = deesainer_gpu::Frame {
        mesh: std::sync::Arc::clone(&cache.mesh),
        mesh_id: cache.id,
        textures: std::sync::Arc::clone(pixels),
        view: deesainer_gpu::View {
            scale: (cam.zoom * f64::from(ppp)) as f32,
            offset: [
                (rect.min.x + cam.pan.x) * ppp - left,
                (rect.min.y + cam.pan.y) * ppp - top,
            ],
            size: [w as u32, h as u32],
        },
        background: cache.mesh.background.unwrap_or([0; 4]),
    };
    ui.painter_at(rect)
        .add(egui_wgpu::Callback::new_paint_callback(
            rect,
            CanvasCallback { frame },
        ));
    true
}

fn draw_render(app: &mut App, ui: &mut egui::Ui, rect: ERect) {
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let w = (f64::from(rect.width()) * ppp).round().max(1.0) as u32;
    let h = (f64::from(rect.height()) * ppp).round().max(1.0) as u32;
    let page = app.session.current_page();
    let edit_key = app.editing.as_ref().map(|e| {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        e.editor.data().content.hash(&mut hash);
        format!("{:?}", e.editor.data().runs).hash(&mut hash);
        e.preedit.hash(&mut hash);
        format!("{:?}", e.preedit_active).hash(&mut hash);
        (e.id, hash.finish())
    });
    let key = (
        app.session.revision(),
        page,
        app.camera.zoom.to_bits(),
        (
            app.camera.pan.x.round() as i32,
            app.camera.pan.y.round() as i32,
        ),
        (w, h),
        edit_key,
    );
    if app.canvas.key != Some(key) || app.canvas.texture.is_none() {
        let vp = Viewport {
            transform: Affine::scale(ppp, ppp)
                .then(Affine::translate(
                    f64::from(app.camera.pan.x.round()),
                    f64::from(app.camera.pan.y.round()),
                ))
                .then(Affine::scale(app.camera.zoom, app.camera.zoom)),
            width: w,
            height: h,
        };
        let preview = app.editing.as_ref().and_then(|edit| {
            let data = if edit.preedit.is_empty() {
                edit.editor.data().clone()
            } else {
                edit.editor
                    .composed(
                        &edit.preedit,
                        edit.preedit_active.clone(),
                        crate::theme::rgba(TOKENS.selection),
                    )
                    .ok()?
            };
            match edit.instance {
                Some(inst) => {
                    deesainer_engine::instance_text_preview(app.session.doc(), inst, edit.id, &data)
                        .ok()
                }
                None => deesainer_engine::text_preview(app.session.doc(), edit.id, &data).ok(),
            }
        });
        let opts = Options {
            background: true,
            text: Some(deesainer_engine::text_painter()),
            hidden: &[],
        };
        match render_page(
            preview.as_ref().unwrap_or(app.session.doc()),
            page,
            &vp,
            opts,
        ) {
            Ok((pix, _)) => {
                let image =
                    egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pix.data());
                match &mut app.canvas.texture {
                    Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                    None => {
                        app.canvas.texture = Some(ui.ctx().load_texture(
                            "canvas",
                            image,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                }
                app.canvas.key = Some(key);
            }
            Err(e) => app.status = Some((format!("render failed: {e}"), true)),
        }
    }
    if let Some(tex) = &app.canvas.texture {
        let painter = ui.painter_at(rect);
        let shift = vec2(
            app.camera.pan.x.round() - app.camera.pan.x,
            app.camera.pan.y.round() - app.camera.pan.y,
        );
        painter.image(
            tex.id(),
            rect.translate(-shift * 0.0),
            ERect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

fn node_outline(app: &App, origin: Pos2, id: NodeId) -> Option<Vec<Pos2>> {
    let doc = app.session.doc();
    let node = doc.get(id)?;
    let t = doc.world_transform(id);
    let local = Rect::new(0.0, 0.0, node.rect.width, node.rect.height);
    Some(
        local
            .corners()
            .iter()
            .map(|c| app.camera.to_screen(origin, t.apply(*c)))
            .collect(),
    )
}

fn draw_overlays(app: &mut App, ui: &mut egui::Ui, rect: ERect) {
    let painter = ui.painter_at(rect);
    let origin = rect.min;
    let doc = app.session.snapshot();
    let page = app.session.current_page();
    let sel_stroke = Stroke::new(1.0, TOKENS.selection);

    if let Some(p) = doc.get(page) {
        for c in &p.children {
            let Some(n) = doc.get(*c) else { continue };
            if !n.visible || !n.kind.is_frame_like() {
                continue;
            }
            let Some(b) = doc.world_bounds(*c) else {
                continue;
            };
            let pos = app.camera.to_screen(origin, b.origin()) - vec2(0.0, 4.0);
            let selected = app.session.selection().contains(c);
            let color = if matches!(
                n.kind,
                NodeKind::Component { .. } | NodeKind::ComponentSet | NodeKind::Instance { .. }
            ) {
                TOKENS.component
            } else if selected {
                TOKENS.selection
            } else {
                TOKENS.canvas_label
            };
            painter.text(
                pos,
                egui::Align2::LEFT_BOTTOM,
                &n.name,
                egui::FontId::proportional(11.0),
                color,
            );
        }
    }

    if let Some(h) = app.tools.hover() {
        if !app.session.selection().contains(&h) && !app.tools.is_busy() {
            if let Some(pts) = node_outline(app, origin, h) {
                painter.add(egui::Shape::closed_line(
                    pts,
                    Stroke::new(1.5, TOKENS.hover),
                ));
            }
        }
    }

    let selection = app.session.selection().to_vec();
    for id in &selection {
        if let Some(pts) = node_outline(app, origin, *id) {
            painter.add(egui::Shape::closed_line(pts, sel_stroke));
        }
    }
    if let Some(b) = selection_bounds(&doc, &selection) {
        let r = screen_rect(&app.camera, origin, &b);
        if selection.len() > 1 {
            painter.rect_stroke(r, 0.0, sel_stroke, StrokeKind::Middle);
        }
        if selection.len() == 1
            && app.editing.is_none()
            && !app.placing_comment
            && app.tools.editing_path().is_none()
            && app.gradient_edit.is_none()
        {
            for h in Handle::ALL {
                let c = app.camera.to_screen(origin, h.at(&b));
                let hr = ERect::from_center_size(c, vec2(7.0, 7.0));
                painter.rect(hr, 1.0, TOKENS.handle_fill, sel_stroke, StrokeKind::Middle);
            }
        }
        let label = format!("{} × {}", fmt(b.width), fmt(b.height));
        let pos = pos2(r.center().x, r.max.y + 6.0);
        let galley =
            painter.layout_no_wrap(label, egui::FontId::proportional(11.0), Color32::WHITE);
        let bg = ERect::from_center_size(
            pos + vec2(0.0, galley.size().y / 2.0 + 2.0),
            galley.size() + vec2(8.0, 4.0),
        );
        painter.rect_filled(bg, 3.0, TOKENS.selection);
        painter.galley(bg.min + vec2(4.0, 2.0), galley, Color32::WHITE);
    }

    if ui.input(|i| i.modifiers.alt) && !app.tools.is_busy() {
        if let (Some(a), Some(h)) = (selection_bounds(&doc, &selection), app.tools.hover()) {
            if !selection.contains(&h) {
                if let Some(b) = doc.world_bounds(h) {
                    draw_measurements(app, &painter, origin, &a, &b);
                }
            }
        }
    }
    if app.show_prototype {
        crate::player::draw_connections(app, &painter, origin);
    }
    if app.right_tab == crate::RightTab::Inspect {
        draw_annotations(app, &painter, origin);
    }
    if let Some((inst, target)) = app.instance_layer {
        let doc = app.session.doc();
        if let Some(corners) = doc
            .get(inst)
            .and_then(|n| deesainer_engine::slots::layer_outline(doc, n, target))
        {
            let mut pts: Vec<Pos2> = corners
                .iter()
                .map(|c| app.camera.to_screen(origin, *c))
                .collect();
            pts.push(pts[0]);
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, TOKENS.component)));
        }
    }
    crate::comments::draw_pins(app, &painter, origin);
    if app.eyedropper {
        if let Some(p) = app.pointer_page {
            if let Some(c) = crate::eyedropper::sample(app, p) {
                let [r, g, b, _] = c.to_rgba8();
                let at = app.camera.to_screen(origin, p) + vec2(22.0, 22.0);
                painter.circle(
                    at,
                    13.0,
                    Color32::from_rgb(r, g, b),
                    Stroke::new(3.0, Color32::WHITE),
                );
                painter.circle_stroke(at, 14.5, Stroke::new(1.0, Color32::from_black_alpha(60)));
            }
        }
        ui.ctx().request_repaint();
    }
    crate::comments::draw_peers(app, &painter, origin);
    crate::chat::show(app, ui, origin);

    let mut overlays = app.tools.overlays();
    overlays.extend(app.tools.doc_overlays(&doc));
    for o in overlays {
        match o {
            Overlay::Marquee(r) => {
                let r = screen_rect(&app.camera, origin, &r);
                painter.rect(r, 0.0, TOKENS.marquee_fill, sel_stroke, StrokeKind::Middle);
            }
            Overlay::Preview(r) => {
                let r = screen_rect(&app.camera, origin, &r);
                painter.rect_stroke(r, 0.0, sel_stroke, StrokeKind::Middle);
            }
            Overlay::Insert(a, b) => {
                painter.line_segment(
                    [
                        app.camera.to_screen(origin, a),
                        app.camera.to_screen(origin, b),
                    ],
                    Stroke::new(2.0, TOKENS.selection),
                );
            }
            Overlay::Ghost(r) => {
                let r = screen_rect(&app.camera, origin, &r);
                let corners = [
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                    r.left_top(),
                ];
                painter.extend(egui::Shape::dashed_line(&corners, sel_stroke, 4.0, 3.0));
            }
            Overlay::DropTarget(r) => {
                let r = screen_rect(&app.camera, origin, &r);
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(2.0, TOKENS.selection),
                    StrokeKind::Inside,
                );
            }
            Overlay::PreviewLine(a, b) => {
                painter.line_segment(
                    [
                        app.camera.to_screen(origin, a),
                        app.camera.to_screen(origin, b),
                    ],
                    sel_stroke,
                );
            }
            Overlay::Guide(a, b) => {
                painter.line_segment(
                    [
                        app.camera.to_screen(origin, a),
                        app.camera.to_screen(origin, b),
                    ],
                    Stroke::new(1.0, TOKENS.guide),
                );
            }
            Overlay::Gap(a, b) => draw_gap(app, &painter, origin, a, b),
            Overlay::Anchors(pts, active) => {
                for (i, p) in pts.iter().enumerate() {
                    let c = app.camera.to_screen(origin, *p);
                    let fill = if Some(i) == active {
                        TOKENS.selection
                    } else {
                        TOKENS.handle_fill
                    };
                    painter.circle(c, 4.0, fill, sel_stroke);
                }
            }
            Overlay::PenPath(pts, _) => {
                let screen: Vec<Pos2> = pts
                    .iter()
                    .map(|p| app.camera.to_screen(origin, *p))
                    .collect();
                painter.add(egui::Shape::line(screen.clone(), sel_stroke));
                for s in screen {
                    painter.circle(s, 3.5, TOKENS.handle_fill, sel_stroke);
                }
            }
        }
    }
}

fn draw_annotations(app: &App, painter: &egui::Painter, origin: Pos2) {
    let doc = app.session.doc();
    let Ok(ids) = doc.subtree_pre_order(app.session.current_page()) else {
        return;
    };
    let mut placed: Vec<ERect> = Vec::new();
    for id in ids {
        let Some(n) = doc
            .get(id)
            .filter(|n| n.visible && !n.annotations.is_empty())
        else {
            continue;
        };
        let Some(b) = doc.world_bounds(id) else {
            continue;
        };
        let anchor = app.camera.to_screen(origin, Point::new(b.max_x(), b.y));
        let first = &n.annotations[0].text;
        let mut text: String = first.chars().take(40).collect();
        if first.chars().count() > 40 {
            text.push('…');
        }
        if n.annotations.len() > 1 {
            text = format!("{text}  +{}", n.annotations.len() - 1);
        }
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), Color32::WHITE);
        let at = anchor + vec2(10.0, -galley.size().y - 6.0);
        let mut bg = ERect::from_min_size(at, galley.size() + vec2(12.0, 6.0));
        for _ in 0..64 {
            match placed.iter().find(|r| r.expand(2.0).intersects(bg)) {
                Some(r) => bg = bg.translate(vec2(0.0, r.min.y - bg.max.y - 4.0)),
                None => break,
            }
        }
        placed.push(bg);
        painter.line_segment(
            [anchor, bg.left_bottom()],
            Stroke::new(1.0, TOKENS.annotation),
        );
        painter.rect_filled(bg, 4.0, TOKENS.annotation);
        painter.galley(bg.min + vec2(6.0, 3.0), galley, Color32::WHITE);
    }
}

fn draw_gap(app: &App, painter: &egui::Painter, origin: Pos2, a: Point, b: Point) {
    let (sa, sb) = (
        app.camera.to_screen(origin, a),
        app.camera.to_screen(origin, b),
    );
    let stroke = Stroke::new(1.0, TOKENS.guide);
    painter.line_segment([sa, sb], stroke);
    let horizontal = (a.y - b.y).abs() < 1e-9;
    let tick = if horizontal {
        vec2(0.0, 4.0)
    } else {
        vec2(4.0, 0.0)
    };
    for p in [sa, sb] {
        painter.line_segment([p - tick, p + tick], stroke);
    }
    let len = if horizontal {
        (b.x - a.x).abs()
    } else {
        (b.y - a.y).abs()
    };
    let galley = painter.layout_no_wrap(fmt(len), egui::FontId::proportional(11.0), Color32::WHITE);
    let mid = sa + (sb - sa) / 2.0;
    let at = if horizontal {
        mid + vec2(0.0, galley.size().y / 2.0 + 6.0)
    } else {
        mid + vec2(galley.size().x / 2.0 + 8.0, 0.0)
    };
    let bg = ERect::from_center_size(at, galley.size() + vec2(8.0, 4.0));
    painter.rect_filled(bg, 3.0, TOKENS.guide);
    painter.galley(bg.min + vec2(4.0, 2.0), galley, Color32::WHITE);
}

fn text_key_actions(
    key: egui::Key,
    m: egui::Modifiers,
    has_selection: bool,
    mac: bool,
) -> Option<Vec<EditAction>> {
    use egui::Key;
    let word = if mac { m.alt } else { m.ctrl };
    let line = mac && m.mac_cmd;
    let motion = match key {
        Key::ArrowLeft if line => TextMotion::Home,
        Key::ArrowRight if line => TextMotion::End,
        Key::ArrowUp if line => TextMotion::Start,
        Key::ArrowDown if line => TextMotion::Finish,
        Key::ArrowLeft if word => TextMotion::PreviousWord,
        Key::ArrowRight if word => TextMotion::NextWord,
        Key::ArrowLeft => TextMotion::Left,
        Key::ArrowRight => TextMotion::Right,
        Key::ArrowUp => TextMotion::Up,
        Key::ArrowDown => TextMotion::Down,
        Key::Home if !mac && m.ctrl => TextMotion::Start,
        Key::End if !mac && m.ctrl => TextMotion::Finish,
        Key::Home => TextMotion::Home,
        Key::End => TextMotion::End,
        Key::Backspace | Key::Delete => {
            let forward = key == Key::Delete;
            let erase = if forward {
                EditAction::Delete
            } else {
                EditAction::Backspace
            };
            let reach = if line {
                Some(if forward {
                    TextMotion::End
                } else {
                    TextMotion::Home
                })
            } else if word {
                Some(if forward {
                    TextMotion::NextWord
                } else {
                    TextMotion::PreviousWord
                })
            } else {
                None
            };
            return Some(match reach {
                Some(motion) if !has_selection => vec![EditAction::Move(motion, true), erase],
                _ => vec![erase],
            });
        }
        _ => return None,
    };
    Some(vec![EditAction::Move(motion, m.shift)])
}

fn fmt(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}", r.round() as i64)
    } else {
        format!("{r:.2}")
    }
}

fn editing_screen_rect(app: &App, origin: Pos2) -> Option<ERect> {
    let edit = app.editing.as_ref()?;
    let (node, transform) = app.editing_frame()?;
    let data = edit.editor.data();
    let width = match data.auto_resize {
        deesainer_doc::TextAutoResize::WidthAndHeight => None,
        _ => Some(node.rect.width),
    };
    let (measured_width, measured_height) = edit.editor.visual_size();
    let w = if width.is_none() {
        measured_width
    } else {
        node.rect.width
    };
    let h = if data.auto_resize == deesainer_doc::TextAutoResize::None {
        node.rect.height
    } else {
        measured_height
    };
    let b = transform.transform_rect_bounds(&Rect::new(0.0, 0.0, w, h));
    Some(screen_rect(&app.camera, origin, &b).expand(4.0))
}

fn text_input(app: &mut App, ui: &mut egui::Ui, origin: Pos2) {
    let Some(rect) = editing_screen_rect(app, origin) else {
        app.editing = None;
        return;
    };
    if app
        .editing
        .as_ref()
        .is_some_and(|editing| app.session.selection() != [editing.instance.unwrap_or(editing.id)])
    {
        app.commit_editing();
        return;
    }
    let id = ui.make_persistent_id("canvas_text_editor");
    let response = ui.interact(rect, id, Sense::click_and_drag());
    if app.editing.as_ref().is_some_and(|e| !e.focus_requested) {
        ui.ctx().memory_mut(|m| m.request_focus(id));
        if let Some(edit) = &mut app.editing {
            edit.focus_requested = true;
        }
    }
    ui.ctx().memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
                tab: true,
            },
        )
    });
    let (events, pointer, pressed, down, shift) = ui.input(|i| {
        (
            i.events.clone(),
            i.pointer.interact_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
        )
    });
    let mut commit = false;
    let mut action_error = None;
    let frame = app.editing_frame();
    if let Some(edit) = &mut app.editing {
        if edit.focus_requested
            && !ui.ctx().memory(|m| m.has_focus(id))
            && edit.editor.selected_range().is_none()
        {
            commit = true;
        }
        let node = frame.as_ref().map(|(n, _)| n);
        let local = pointer.and_then(|screen| {
            let world = app.camera.to_page(origin, screen);
            frame
                .as_ref()
                .and_then(|(_, t)| t.invert())
                .map(|inv| inv.apply(world))
        });
        if let (Some(local), Some(node)) = (local, node) {
            let clicks = if response.triple_clicked() {
                3
            } else if response.double_clicked() {
                2
            } else {
                1
            };
            if response.hovered() && (pressed || down || clicks > 1) {
                let extend = clicks == 1 && (down && !pressed || shift);
                if let Err(e) = edit.editor.pointer(local, node.rect.height, extend, clicks) {
                    action_error = Some(e);
                }
                ui.ctx().memory_mut(|m| m.request_focus(id));
            }
        }
        if ui.ctx().memory(|m| m.has_focus(id)) {
            for event in events {
                let action = match event {
                    egui::Event::Text(s) | egui::Event::Paste(s) => Some(EditAction::Insert(s)),
                    egui::Event::Ime(egui::ImeEvent::Preedit {
                        text,
                        active_range_chars,
                    }) => {
                        edit.preedit = text;
                        edit.preedit_active = active_range_chars;
                        None
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(s)) => {
                        edit.preedit.clear();
                        edit.preedit_active = None;
                        Some(EditAction::Insert(s))
                    }
                    egui::Event::Ime(_) => {
                        edit.preedit.clear();
                        edit.preedit_active = None;
                        None
                    }
                    egui::Event::Copy => {
                        if let Some(text) = edit.editor.selected_text() {
                            ui.ctx().copy_text(text);
                        }
                        None
                    }
                    egui::Event::Cut => {
                        if let Some(text) = edit.editor.selected_text() {
                            ui.ctx().copy_text(text);
                            Some(EditAction::Insert(String::new()))
                        } else {
                            None
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let keyed = text_key_actions(
                            key,
                            modifiers,
                            edit.editor.selected_range().is_some(),
                            cfg!(target_os = "macos"),
                        );
                        if let Some(actions) = keyed {
                            let (last, before) = actions
                                .split_last()
                                .map_or((None, &[][..]), |(l, b)| (Some(l.clone()), b));
                            for a in before {
                                if let Err(e) = edit.editor.action(a.clone()) {
                                    action_error = Some(e);
                                }
                            }
                            last
                        } else if modifiers.command && key == egui::Key::A {
                            Some(EditAction::SelectAll)
                        } else if modifiers.command && key == egui::Key::B {
                            Some(EditAction::ToggleBold)
                        } else if modifiers.command && key == egui::Key::I {
                            Some(EditAction::ToggleItalic)
                        } else if (modifiers.command && !modifiers.shift && key == egui::Key::U)
                            || (modifiers.command && modifiers.shift && key == egui::Key::X)
                        {
                            let want = if key == egui::Key::U {
                                deesainer_doc::TextDecoration::Underline
                            } else {
                                deesainer_doc::TextDecoration::Strikethrough
                            };
                            let now = edit.editor.selected_style().map(|s| s.decoration);
                            let next = if now == Some(want) {
                                deesainer_doc::TextDecoration::None
                            } else {
                                want
                            };
                            if let Err(e) =
                                edit.editor.modify_selected_style(|s| s.decoration = next)
                            {
                                action_error = Some(e);
                            }
                            None
                        } else if modifiers.command && key == egui::Key::Z {
                            Some(if modifiers.shift {
                                EditAction::Redo
                            } else {
                                EditAction::Undo
                            })
                        } else if key == egui::Key::Enter {
                            Some(EditAction::Insert("\n".into()))
                        } else if key == egui::Key::Tab {
                            Some(EditAction::Insert("\t".into()))
                        } else if key == egui::Key::Escape {
                            commit = true;
                            None
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    if let Err(e) = edit.editor.action(action) {
                        action_error = Some(e);
                    }
                }
            }
        }
        let geometry = edit.editor.geometry(node.map_or(0.0, |n| n.rect.height));
        if let Some(caret) = geometry
            .caret
            .filter(|_| ui.ctx().memory(|m| m.has_focus(id)))
        {
            let world = frame
                .as_ref()
                .map_or(Affine::IDENTITY, |(_, t)| *t)
                .apply(Point::new(caret.x, caret.y));
            let p = app.camera.to_screen(origin, world);
            ui.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect,
                    cursor_rect: ERect::from_min_size(
                        p,
                        vec2(2.0, (caret.height * app.camera.zoom) as f32),
                    ),
                    should_interrupt_composition: false,
                })
            });
        }
    }
    if let Some(message) = action_error {
        app.status = Some((message, true));
    }
    if commit {
        app.commit_editing();
    }
}

fn text_caret(app: &App, ui: &egui::Ui, origin: Pos2) {
    let Some(edit) = &app.editing else {
        return;
    };
    let Some((node, transform)) = app.editing_frame() else {
        return;
    };
    let geometry = edit.editor.geometry(node.rect.height);
    let painter = ui.painter();
    for r in geometry.selection {
        let b = transform.transform_rect_bounds(&r);
        painter.rect_filled(
            screen_rect(&app.camera, origin, &b),
            0.0,
            TOKENS.selection.gamma_multiply(0.35),
        );
    }
    if let Some(caret) = geometry.caret {
        let a = app
            .camera
            .to_screen(origin, transform.apply(Point::new(caret.x, caret.y)));
        let b = app.camera.to_screen(
            origin,
            transform.apply(Point::new(caret.x, caret.y + caret.height)),
        );
        painter.line_segment([a, b], Stroke::new(1.5, TOKENS.selection));
    }
}

fn set_cursor(app: &mut App, ui: &mut egui::Ui, response: &egui::Response, origin: Pos2) {
    if !response.hovered() {
        return;
    }
    let Some(screen) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let p = app.camera.to_page(origin, screen);
    let doc = app.session.snapshot();
    let selection = app.session.selection().to_vec();
    let input = Input {
        doc: &doc,
        page: app.session.current_page(),
        selection: &selection,
        zoom: app.camera.zoom,
        pixel_grid: app.snap_pixel_grid,
    };
    let icon = if ui.input(|i| i.key_down(egui::Key::Space)) {
        egui::CursorIcon::Grab
    } else if app.eyedropper {
        egui::CursorIcon::Crosshair
    } else {
        match app.tools.cursor(&input, p) {
            Cursor::Default => egui::CursorIcon::Default,
            Cursor::Crosshair => egui::CursorIcon::Crosshair,
            Cursor::Grab => egui::CursorIcon::Grab,
            Cursor::Grabbing => egui::CursorIcon::Grabbing,
            Cursor::Text => egui::CursorIcon::Text,
            Cursor::Rotate => egui::CursorIcon::Alias,
            Cursor::Resize(h) => match h {
                Handle::Top | Handle::Bottom => egui::CursorIcon::ResizeVertical,
                Handle::Left | Handle::Right => egui::CursorIcon::ResizeHorizontal,
                Handle::TopLeft | Handle::BottomRight => egui::CursorIcon::ResizeNwSe,
                Handle::TopRight | Handle::BottomLeft => egui::CursorIcon::ResizeNeSw,
            },
        }
    };
    ui.ctx().set_cursor_icon(icon);
}

pub fn zoom_to(app: &mut App, what: &str) {
    let size = app.canvas.last_size;
    let center = size / 2.0;
    match what {
        "in" => app.camera.zoom_at(center, 2.0),
        "out" => app.camera.zoom_at(center, 0.5),
        "100" => app.camera.zoom_at(center, 1.0 / app.camera.zoom),
        "fit" => {
            if let Some(b) = content_bounds(app.session.doc(), app.session.current_page()) {
                app.camera.fit(b, size, MAX_ZOOM);
            }
        }
        "selection" => {
            if let Some(b) = selection_bounds(app.session.doc(), app.session.selection()) {
                app.camera.fit(b, size, MAX_ZOOM);
            }
        }
        _ => {}
    }
}

pub fn nudge(app: &mut App, dx: f64, dy: f64) {
    if !app.session.selection().is_empty() {
        app.dispatch("node.move", json!({ "dx": dx, "dy": dy }));
    }
}

pub fn tool_for_key(key: egui::Key) -> Option<ToolKind> {
    use egui::Key;
    Some(match key {
        Key::V => ToolKind::Move,
        Key::F | Key::A => ToolKind::Frame,
        Key::R => ToolKind::Rectangle,
        Key::O => ToolKind::Ellipse,
        Key::L => ToolKind::Line,
        Key::T => ToolKind::Text,
        Key::P => ToolKind::Pen,
        Key::H => ToolKind::Hand,
        _ => return None,
    })
}

pub fn cancel_tool(tools: &mut Tools) -> Vec<deesainer_tools::Out> {
    let mut out = tools.exit_path_edit();
    out.extend(tools.finish_pen());
    out.extend(tools.cancel());
    out
}

fn draw_measurements(app: &App, painter: &egui::Painter, origin: Pos2, a: &Rect, b: &Rect) {
    let color = TOKENS.guide;
    let screen = |x: f64, y: f64| app.camera.to_screen(origin, Point::new(x, y));
    let label = |at: Pos2, value: f64| {
        let text = fmt(value);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(10.0), Color32::WHITE);
        let bg = ERect::from_center_size(at, galley.size() + vec2(8.0, 4.0));
        painter.rect_filled(bg, 3.0, color);
        painter.galley(bg.min + vec2(4.0, 2.0), galley, Color32::WHITE);
    };
    let line = |p: Pos2, q: Pos2, value: f64| {
        painter.line_segment([p, q], Stroke::new(1.0, color));
        for end in [p, q] {
            let d = (q - p).normalized();
            let n = vec2(-d.y, d.x) * 4.0;
            painter.line_segment([end - n, end + n], Stroke::new(1.0, color));
        }
        label(p + (q - p) * 0.5, value);
    };
    painter.rect_stroke(
        screen_rect(&app.camera, origin, b),
        0.0,
        Stroke::new(1.0, color),
        StrokeKind::Middle,
    );
    let overlap_x = (a.x.max(b.x), a.max_x().min(b.max_x()));
    let overlap_y = (a.y.max(b.y), a.max_y().min(b.max_y()));
    let mid_x = if overlap_x.0 < overlap_x.1 {
        (overlap_x.0 + overlap_x.1) / 2.0
    } else {
        a.center().x
    };
    let mid_y = if overlap_y.0 < overlap_y.1 {
        (overlap_y.0 + overlap_y.1) / 2.0
    } else {
        a.center().y
    };
    let contained = b.x <= a.x && b.y <= a.y && b.max_x() >= a.max_x() && b.max_y() >= a.max_y();
    let contains = a.x <= b.x && a.y <= b.y && a.max_x() >= b.max_x() && a.max_y() >= b.max_y();
    let (outer, inner) = if contained { (b, a) } else { (a, b) };
    if contained || contains {
        let (m_x, m_y) = (inner.center().x, inner.center().y);
        line(
            screen(outer.x, m_y),
            screen(inner.x, m_y),
            inner.x - outer.x,
        );
        line(
            screen(inner.max_x(), m_y),
            screen(outer.max_x(), m_y),
            outer.max_x() - inner.max_x(),
        );
        line(
            screen(m_x, outer.y),
            screen(m_x, inner.y),
            inner.y - outer.y,
        );
        line(
            screen(m_x, inner.max_y()),
            screen(m_x, outer.max_y()),
            outer.max_y() - inner.max_y(),
        );
        return;
    }
    if a.max_x() <= b.x {
        line(
            screen(a.max_x(), mid_y),
            screen(b.x, mid_y),
            b.x - a.max_x(),
        );
    } else if b.max_x() <= a.x {
        line(
            screen(b.max_x(), mid_y),
            screen(a.x, mid_y),
            a.x - b.max_x(),
        );
    }
    if a.max_y() <= b.y {
        line(
            screen(mid_x, a.max_y()),
            screen(mid_x, b.y),
            b.y - a.max_y(),
        );
    } else if b.max_y() <= a.y {
        line(
            screen(mid_x, b.max_y()),
            screen(mid_x, a.y),
            a.y - b.max_y(),
        );
    }
}

fn draw_layout_grids(app: &App, ui: &egui::Ui, rect: ERect) {
    use deesainer_doc::GridPattern;
    let painter = ui.painter_at(rect);
    let origin = rect.min;
    let doc = app.session.doc();
    let page = app.session.current_page();
    let Ok(ids) = doc.subtree_pre_order(page) else {
        return;
    };
    for id in ids {
        let Some(node) = doc.get(id) else { continue };
        if node.layout_grids.is_empty() || !node.visible || !node.kind.is_frame_like() {
            continue;
        }
        let t = doc.world_transform(id);
        let (w, h) = (node.rect.width, node.rect.height);
        let scr = |x: f64, y: f64| app.camera.to_screen(origin, t.apply(Point::new(x, y)));
        for g in node.layout_grids.iter().filter(|g| g.visible) {
            let c = crate::theme::color32(g.color);
            match g.pattern {
                GridPattern::Grid => {
                    let step = g.size.max(1.0);
                    if step * app.camera.zoom < 3.0 {
                        continue;
                    }
                    let stroke = Stroke::new(1.0, c);
                    let mut x = step;
                    while x < w {
                        painter.line_segment([scr(x, 0.0), scr(x, h)], stroke);
                        x += step;
                    }
                    let mut y = step;
                    while y < h {
                        painter.line_segment([scr(0.0, y), scr(w, y)], stroke);
                        y += step;
                    }
                }
                GridPattern::Columns => {
                    for (x0, bw) in g.bands(w) {
                        let pts =
                            vec![scr(x0, 0.0), scr(x0 + bw, 0.0), scr(x0 + bw, h), scr(x0, h)];
                        painter.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
                    }
                }
                GridPattern::Rows => {
                    for (y0, bh) in g.bands(h) {
                        let pts =
                            vec![scr(0.0, y0), scr(w, y0), scr(w, y0 + bh), scr(0.0, y0 + bh)];
                        painter.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
                    }
                }
            }
        }
    }
}

fn ruler_guides(app: &mut App, ui: &mut egui::Ui, rect: ERect) -> bool {
    use deesainer_doc::RulerGuide;
    const BAND: f32 = 18.0;
    if !app.show_rulers {
        app.guide_drag = None;
        return false;
    }
    let origin = rect.min;
    let page = app.session.current_page();
    let mut guides: Vec<RulerGuide> = app
        .session
        .doc()
        .get(page)
        .map(|p| p.guides.clone())
        .unwrap_or_default();
    let (pos, pressed, down, released) = ui.input(|i| {
        (
            i.pointer.interact_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
        )
    });
    let Some(pos) = pos else {
        return app.guide_drag.is_some();
    };
    let in_top = rect.contains(pos) && pos.y < rect.min.y + BAND && pos.x >= rect.min.x + BAND;
    let in_left = rect.contains(pos) && pos.x < rect.min.x + BAND && pos.y >= rect.min.y + BAND;
    let page_pt = app.camera.to_page(origin, pos);
    let near = |g: &RulerGuide, app: &App| {
        let s = app.camera.to_screen(origin, Point::new(g.at, g.at));
        (if g.vertical { s.x - pos.x } else { s.y - pos.y }).abs() <= 4.0
    };
    if pressed && app.guide_drag.is_none() && rect.contains(pos) {
        if in_top {
            app.guide_drag = Some((false, page_pt.y));
        } else if in_left {
            app.guide_drag = Some((true, page_pt.x));
        } else if let Some(i) = guides.iter().position(|g| near(g, app)) {
            let g = guides.remove(i);
            app.guide_drag = Some((g.vertical, g.at));
            app.session.begin_group("Move guide");
            app.dispatch("page.setGuides", json!({ "id": page, "guides": guides }));
            app.session.end_group();
        }
    }
    let Some((vertical, _)) = app.guide_drag else {
        if rect.contains(pos) && !in_top && !in_left && guides.iter().any(|g| near(g, app)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        return false;
    };
    ui.ctx().set_cursor_icon(if vertical {
        egui::CursorIcon::ResizeHorizontal
    } else {
        egui::CursorIcon::ResizeVertical
    });
    if down {
        app.guide_drag = Some((vertical, if vertical { page_pt.x } else { page_pt.y }));
        ui.ctx().request_repaint();
    }
    if released {
        let over_ruler = in_top || in_left || !rect.contains(pos);
        if !over_ruler {
            let at = if vertical { page_pt.x } else { page_pt.y }.round();
            guides.push(RulerGuide { vertical, at });
        }
        app.guide_drag = None;
        app.session.begin_group("Guides");
        app.dispatch("page.setGuides", json!({ "id": page, "guides": guides }));
        app.session.end_group();
    }
    true
}

fn ruler_step(zoom: f64, min_px: f64) -> f64 {
    let raw = min_px / zoom.max(1e-9);
    let pow = 10f64.powf(raw.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * pow >= raw {
            return m * pow;
        }
    }
    10.0 * pow
}

fn draw_rulers(app: &App, ui: &egui::Ui, rect: ERect) {
    const BAND: f32 = 18.0;
    let painter = ui.painter_at(rect);
    let origin = rect.min;
    let bg = Color32::WHITE;
    let line = Color32::from_gray(225);
    let text = Color32::from_gray(140);
    let font = egui::FontId::proportional(9.0);
    let top = ERect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + BAND));
    let left = ERect::from_min_max(rect.min, pos2(rect.min.x + BAND, rect.max.y));
    painter.rect_filled(top, 0.0, bg);
    painter.rect_filled(left, 0.0, bg);
    let doc = app.session.doc();
    if let Some(b) = selection_bounds(doc, app.session.selection()) {
        let r = screen_rect(&app.camera, origin, &b);
        let tint = TOKENS.selection.gamma_multiply(0.18);
        painter.rect_filled(
            ERect::from_min_max(pos2(r.min.x, top.min.y), pos2(r.max.x, top.max.y)),
            0.0,
            tint,
        );
        painter.rect_filled(
            ERect::from_min_max(pos2(left.min.x, r.min.y), pos2(left.max.x, r.max.y)),
            0.0,
            tint,
        );
    }
    let step = ruler_step(app.camera.zoom, 60.0);
    let page_min = app.camera.to_page(origin, rect.min);
    let page_max = app.camera.to_page(origin, rect.max);
    let fmt_tick = |v: f64| {
        if (v - v.round()).abs() < 1e-6 {
            format!("{}", v.round() as i64)
        } else {
            format!("{v:.1}")
        }
    };
    let mut v = (page_min.x / step).floor() * step;
    while v <= page_max.x {
        let x = app.camera.to_screen(origin, Point::new(v, 0.0)).x;
        if x > left.max.x {
            painter.line_segment(
                [pos2(x, top.max.y - 6.0), pos2(x, top.max.y)],
                Stroke::new(1.0, line),
            );
            painter.text(
                pos2(x + 3.0, top.min.y + 2.0),
                egui::Align2::LEFT_TOP,
                fmt_tick(v),
                font.clone(),
                text,
            );
        }
        v += step;
    }
    let mut v = (page_min.y / step).floor() * step;
    while v <= page_max.y {
        let y = app.camera.to_screen(origin, Point::new(0.0, v)).y;
        if y > top.max.y {
            painter.line_segment(
                [pos2(left.max.x - 6.0, y), pos2(left.max.x, y)],
                Stroke::new(1.0, line),
            );
            painter.text(
                pos2(left.min.x + 2.0, y + 2.0),
                egui::Align2::LEFT_TOP,
                fmt_tick(v),
                font.clone(),
                text,
            );
        }
        v += step;
    }
    if let Some(b) = selection_bounds(doc, app.session.selection()) {
        let r = screen_rect(&app.camera, origin, &b);
        for (x, val) in [(r.min.x, b.x), (r.max.x, b.x + b.width)] {
            painter.text(
                pos2(x, top.center().y),
                egui::Align2::CENTER_CENTER,
                fmt_tick(val),
                egui::FontId::proportional(9.0),
                TOKENS.selection,
            );
        }
        for (y, val) in [(r.min.y, b.y), (r.max.y, b.y + b.height)] {
            painter.text(
                pos2(left.center().x, y),
                egui::Align2::CENTER_CENTER,
                fmt_tick(val),
                egui::FontId::proportional(9.0),
                TOKENS.selection,
            );
        }
    }
    let page = app.session.current_page();
    let guide_color = TOKENS.guide;
    let draw_guide = |vertical: bool, at: f64, label: bool| {
        let s = app.camera.to_screen(origin, Point::new(at, at));
        if vertical {
            if s.x > left.max.x && s.x < rect.max.x {
                painter.line_segment(
                    [pos2(s.x, top.max.y), pos2(s.x, rect.max.y)],
                    Stroke::new(1.0, guide_color),
                );
                if label {
                    painter.text(
                        pos2(s.x + 4.0, top.max.y + 4.0),
                        egui::Align2::LEFT_TOP,
                        fmt_tick(at.round()),
                        font.clone(),
                        guide_color,
                    );
                }
            }
        } else if s.y > top.max.y && s.y < rect.max.y {
            painter.line_segment(
                [pos2(left.max.x, s.y), pos2(rect.max.x, s.y)],
                Stroke::new(1.0, guide_color),
            );
            if label {
                painter.text(
                    pos2(left.max.x + 4.0, s.y - 12.0),
                    egui::Align2::LEFT_TOP,
                    fmt_tick(at.round()),
                    font.clone(),
                    guide_color,
                );
            }
        }
    };
    for g in doc.get(page).map(|p| p.guides.as_slice()).unwrap_or(&[]) {
        draw_guide(g.vertical, g.at, false);
    }
    if let Some((vertical, at)) = app.guide_drag {
        draw_guide(vertical, at, true);
    }
    painter.line_segment(
        [pos2(rect.min.x, top.max.y), pos2(rect.max.x, top.max.y)],
        Stroke::new(1.0, line),
    );
    painter.line_segment(
        [pos2(left.max.x, rect.min.y), pos2(left.max.x, rect.max.y)],
        Stroke::new(1.0, line),
    );
    painter.rect_filled(ERect::from_min_size(rect.min, vec2(BAND, BAND)), 0.0, bg);
}

fn sample_stops(stops: &[deesainer_doc::ColorStop], t: f32) -> deesainer_color::Rgba {
    use deesainer_color::Rgba;
    let mut sorted: Vec<&deesainer_doc::ColorStop> = stops.iter().collect();
    sorted.sort_by(|a, b| a.position.total_cmp(&b.position));
    let (Some(first), Some(last)) = (sorted.first(), sorted.last()) else {
        return Rgba::BLACK;
    };
    if t <= first.position {
        return first.color;
    }
    if t >= last.position {
        return last.color;
    }
    for pair in sorted.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t >= a.position && t <= b.position {
            let u = (t - a.position) / (b.position - a.position).max(1e-6);
            let l = |x: f32, y: f32| x + (y - x) * u;
            return Rgba::new(
                l(a.color.r, b.color.r),
                l(a.color.g, b.color.g),
                l(a.color.b, b.color.b),
                l(a.color.a, b.color.a),
            );
        }
    }
    last.color
}

fn gradient_handles(app: &mut App, ui: &mut egui::Ui, origin: Pos2) -> bool {
    use deesainer_doc::PaintKind;
    let Some((id, index)) = app.gradient_edit else {
        return false;
    };
    let doc = app.session.snapshot();
    let Some(node) = doc.get(id) else {
        return false;
    };
    let Some(paint) = node.fills.get(index) else {
        return false;
    };
    let (w, h) = (node.rect.width, node.rect.height);
    let t = doc.world_transform(id);
    let to_screen = |p: Point| {
        app.camera
            .to_screen(origin, t.apply(Point::new(p.x * w, p.y * h)))
    };
    let points: Vec<(u8, Point)> = match &paint.kind {
        PaintKind::LinearGradient { start, end, .. } => vec![(0, *start), (1, *end)],
        PaintKind::RadialGradient { center, radius, .. }
        | PaintKind::DiamondGradient { center, radius, .. } => vec![
            (0, *center),
            (1, Point::new(center.x + radius.x, center.y)),
            (2, Point::new(center.x, center.y + radius.y)),
        ],
        PaintKind::AngularGradient { center, angle, .. } => {
            let reach = w.min(h).max(1e-9) / 3.0;
            let (s, c) = angle.to_radians().sin_cos();
            vec![
                (0, *center),
                (
                    1,
                    Point::new(
                        center.x + c * reach / w.max(1e-9),
                        center.y + s * reach / h.max(1e-9),
                    ),
                ),
            ]
        }
        _ => return false,
    };
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("gradient-handles"),
        ))
        .with_clip_rect(ui.max_rect());
    let screens: Vec<Pos2> = points.iter().map(|(_, p)| to_screen(*p)).collect();
    let line = Stroke::new(1.5, Color32::WHITE);
    for s in screens.iter().skip(1) {
        painter.line_segment(
            [screens[0], *s],
            Stroke::new(3.0, Color32::from_black_alpha(60)),
        );
        painter.line_segment([screens[0], *s], line);
    }
    let mut busy = false;
    let mut moved: Option<(u8, Point)> = None;
    let mut started = false;
    let mut stopped = false;
    for ((k, _), s) in points.iter().zip(&screens) {
        let r = ERect::from_center_size(*s, vec2(14.0, 14.0));
        let resp = ui.interact(
            r,
            ui.id().with(("gradient-handle", id.0, index, *k)),
            Sense::drag(),
        );
        let fill = if resp.hovered() || resp.dragged() {
            TOKENS.selection
        } else {
            Color32::WHITE
        };
        painter.circle(*s, 5.0, fill, Stroke::new(1.5, TOKENS.selection));
        busy |= resp.hovered() || resp.dragged();
        started |= resp.drag_started();
        stopped |= resp.drag_stopped();
        if resp.dragged() {
            if let (Some(p), Some(inv)) = (resp.interact_pointer_pos(), t.invert()) {
                let local = inv.apply(app.camera.to_page(origin, p));
                moved = Some((*k, Point::new(local.x / w.max(1e-9), local.y / h.max(1e-9))));
            }
        }
    }
    let stops_now: Vec<deesainer_doc::ColorStop> =
        paint.stops().map(<[_]>::to_vec).unwrap_or_default();
    let (a, b) = (screens[0], screens[1]);
    let axis = b - a;
    let len2 = (axis.x * axis.x + axis.y * axis.y).max(1e-6);
    let mut stop_edit: Option<Vec<deesainer_doc::ColorStop>> = None;
    let mut stop_drag = false;
    for (i, st) in stops_now.iter().enumerate() {
        let sp = a + axis * st.position.clamp(0.0, 1.0);
        let r = ERect::from_center_size(sp, vec2(16.0, 16.0));
        let resp = ui.interact(
            r,
            ui.id().with(("gradient-stop", id.0, index, i)),
            Sense::click_and_drag(),
        );
        let c = crate::theme::color32(st.color);
        painter.rect(
            ERect::from_center_size(sp, vec2(12.0, 12.0)),
            3.0,
            c,
            Stroke::new(
                2.0,
                if resp.hovered() || resp.dragged() {
                    TOKENS.selection
                } else {
                    Color32::WHITE
                },
            ),
            StrokeKind::Middle,
        );
        busy |= resp.hovered() || resp.dragged();
        if crate::widgets::double_or_more(&resp) && stops_now.len() > 2 {
            let mut v = stops_now.clone();
            v.remove(i);
            stop_edit = Some(v);
        } else if resp.dragged() {
            started |= resp.drag_started();
            stop_drag = true;
            if let Some(p) = resp.interact_pointer_pos() {
                let pos = (((p - a).x * axis.x + (p - a).y * axis.y) / len2).clamp(0.0, 1.0);
                let mut v = stops_now.clone();
                v[i].position = pos;
                stop_edit = Some(v);
            }
        }
        stopped |= resp.drag_stopped();
    }
    if !busy {
        let (pos, clicked) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed()));
        if let (Some(p), true) = (pos, clicked) {
            let t_on = (((p - a).x * axis.x + (p - a).y * axis.y) / len2).clamp(0.0, 1.0);
            let on_axis = a + axis * t_on;
            if (p - on_axis).length() <= 5.0 && ui.max_rect().contains(p) {
                let mut v = stops_now.clone();
                let color = sample_stops(&stops_now, t_on);
                v.push(deesainer_doc::ColorStop {
                    position: t_on,
                    color,
                });
                v.sort_by(|x, y| x.position.total_cmp(&y.position));
                stop_edit = Some(v);
                busy = true;
            }
        }
    }
    let _ = stop_drag;
    if !busy && stop_edit.is_none() {
        let (pos, pressed) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed()));
        if pressed && pos.is_some_and(|p| ui.max_rect().contains(p)) {
            app.gradient_sticky = None;
        }
    }
    if let Some(v) = stop_edit {
        let mut fills = node.fills.clone();
        if let Some(f) = fills.get_mut(index) {
            match &mut f.kind {
                PaintKind::LinearGradient { stops, .. }
                | PaintKind::RadialGradient { stops, .. }
                | PaintKind::AngularGradient { stops, .. }
                | PaintKind::DiamondGradient { stops, .. } => {
                    *stops = v;
                }
                PaintKind::Solid { .. } | PaintKind::Image { .. } => {}
            }
        }
        if started && !app.drag_group_open {
            app.session.begin_group("Gradient");
            app.drag_group_open = true;
        }
        app.dispatch("node.setFills", json!({ "ids": [id], "fills": fills }));
    }
    if started && !app.drag_group_open {
        app.session.begin_group("Gradient");
        app.drag_group_open = true;
    }
    if let Some((k, p)) = moved {
        let mut fills = node.fills.clone();
        if let Some(f) = fills.get_mut(index) {
            match &mut f.kind {
                PaintKind::LinearGradient { start, end, .. } => {
                    if k == 0 {
                        *start = p;
                    } else {
                        *end = p;
                    }
                }
                PaintKind::RadialGradient { center, radius, .. }
                | PaintKind::DiamondGradient { center, radius, .. } => match k {
                    0 => *center = p,
                    1 => radius.x = (p.x - center.x).abs(),
                    _ => radius.y = (p.y - center.y).abs(),
                },
                PaintKind::AngularGradient { center, angle, .. } => {
                    if k == 0 {
                        *center = p;
                    } else {
                        let (dx, dy) = ((p.x - center.x) * w, (p.y - center.y) * h);
                        if dx.hypot(dy) > 1e-9 {
                            *angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
                        }
                    }
                }
                _ => {}
            }
        }
        app.dispatch("node.setFills", json!({ "ids": [id], "fills": fills }));
    }
    if stopped && app.drag_group_open {
        app.session.end_group();
        app.drag_group_open = false;
    }
    busy
}

fn network_handles(app: &mut App, ui: &mut egui::Ui, origin: Pos2) -> bool {
    let id = match app.session.selection() {
        [id] => *id,
        _ => {
            app.net_vertex = None;
            return false;
        }
    };
    if app.tools.editing_path().is_some() {
        return false;
    }
    let doc = app.session.snapshot();
    let Some(node) = doc.get(id) else {
        return false;
    };
    let deesainer_doc::NodeKind::Vector { path } = &node.kind else {
        return false;
    };
    let Some(net) = node.network.as_ref().filter(|n| n.matches(path)) else {
        app.net_vertex = None;
        return false;
    };
    if net.vertices.len() > 400 {
        return false;
    }
    let t = doc.world_transform(id);
    let selected = app.net_vertex.filter(|(n, _)| *n == id).map(|(_, v)| v);
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("network-handles"),
        ))
        .with_clip_rect(ui.max_rect());
    let screens: Vec<Pos2> = net
        .vertices
        .iter()
        .map(|v| app.camera.to_screen(origin, t.apply(*v)))
        .collect();
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let pen = app.tools.active == ToolKind::Pen;
    let mut busy = false;
    let mut connect: Option<(usize, usize)> = None;
    let mut select: Option<usize> = None;
    let mut moved: Option<(usize, Point)> = None;
    let (mut started, mut stopped) = (false, false);
    for (i, s) in screens.iter().enumerate() {
        let resp = ui.interact(
            ERect::from_center_size(*s, vec2(14.0, 14.0)),
            ui.id().with(("net-vertex", id.0, i)),
            Sense::click_and_drag(),
        );
        let is_sel = selected == Some(i);
        let fill = if is_sel || resp.hovered() || resp.dragged() {
            TOKENS.selection
        } else {
            Color32::WHITE
        };
        painter.rect(
            ERect::from_center_size(*s, vec2(9.0, 9.0)),
            2.0,
            fill,
            Stroke::new(1.5, TOKENS.selection),
            StrokeKind::Middle,
        );
        busy |= resp.hovered() || resp.dragged();
        if resp.clicked() {
            match selected {
                Some(a) if (shift || pen) && a != i => connect = Some((a, i)),
                _ => select = Some(i),
            }
        }
        started |= resp.drag_started();
        stopped |= resp.drag_stopped();
        if resp.dragged() {
            select = Some(i);
            if let (Some(p), Some(inv)) = (resp.interact_pointer_pos(), t.invert()) {
                moved = Some((i, inv.apply(app.camera.to_page(origin, p))));
            }
        }
    }
    let mut bent: Option<(usize, bool, Point)> = None;
    let mut straighten: Option<(usize, bool)> = None;
    if let Some(v) = selected.filter(|v| *v < net.vertices.len()) {
        let at = net.vertices[v];
        for (si, seg) in net.segments.iter().enumerate() {
            for (is_b, end, tangent) in [(false, seg.a, seg.ta), (true, seg.b, seg.tb)] {
                if end != v {
                    continue;
                }
                let handle = Point::new(at.x + tangent.x, at.y + tangent.y);
                let (from, to) = (
                    app.camera.to_screen(origin, t.apply(at)),
                    app.camera.to_screen(origin, t.apply(handle)),
                );
                painter.line_segment([from, to], Stroke::new(1.0, TOKENS.selection));
                let resp = ui.interact(
                    ERect::from_center_size(to, vec2(14.0, 14.0)),
                    ui.id().with(("net-tangent", id.0, si, is_b)),
                    Sense::click_and_drag(),
                );
                painter.circle(
                    to,
                    4.0,
                    if resp.hovered() || resp.dragged() {
                        TOKENS.selection
                    } else {
                        Color32::WHITE
                    },
                    Stroke::new(1.5, TOKENS.selection),
                );
                busy |= resp.hovered() || resp.dragged();
                started |= resp.drag_started();
                stopped |= resp.drag_stopped();
                if crate::widgets::double_or_more(&resp) {
                    straighten = Some((si, is_b));
                } else if resp.dragged() {
                    if let (Some(p), Some(inv)) = (resp.interact_pointer_pos(), t.invert()) {
                        let local = inv.apply(app.camera.to_page(origin, p));
                        bent = Some((si, is_b, Point::new(local.x - at.x, local.y - at.y)));
                    }
                }
            }
        }
    }
    if started && !app.drag_group_open {
        app.session.begin_group("Move vertex");
        app.drag_group_open = true;
    }
    if let Some((segment, is_b, offset)) = bent {
        let key = if is_b { "tb" } else { "ta" };
        app.dispatch(
            "vector.setTangents",
            json!({ "id": id, "segment": segment, key: { "x": offset.x, "y": offset.y } }),
        );
    }
    if let Some((segment, is_b)) = straighten {
        let key = if is_b { "tb" } else { "ta" };
        app.dispatch(
            "vector.setTangents",
            json!({ "id": id, "segment": segment, key: { "x": 0.0, "y": 0.0 } }),
        );
    }
    if let Some((vertex, at)) = moved {
        app.dispatch(
            "vector.moveVertex",
            json!({ "id": id, "vertex": vertex, "x": at.x, "y": at.y }),
        );
    }
    if stopped && app.drag_group_open {
        app.session.end_group();
        app.drag_group_open = false;
    }
    if let Some((a, b)) = connect {
        app.dispatch("vector.connect", json!({ "id": id, "a": a, "b": b }));
        select = Some(b);
        busy = true;
    }
    if !busy && ui.input(|i| i.modifiers.command) {
        let (pos, pressed) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed()));
        if let (Some(p), true, Some(inv)) = (pos, pressed, t.invert()) {
            if ui.max_rect().contains(p) {
                let local = inv.apply(app.camera.to_page(origin, p));
                app.net_region = net.region_at(local).map(|r| (id, r));
                busy = true;
            }
        }
    }
    if let Some((rid, r)) = app.net_region {
        if rid == id {
            if let Some(path) = net.region_path(r, deesainer_doc::FillRule::NonZero) {
                let mut pts: Vec<Pos2> = Vec::new();
                let mut start = Pos2::ZERO;
                for cmd in &path.cmds {
                    let to_screen = |p: Point| app.camera.to_screen(origin, t.apply(p));
                    match *cmd {
                        deesainer_doc::PathCmd::MoveTo { to } => {
                            start = to_screen(to);
                            pts.push(start);
                        }
                        deesainer_doc::PathCmd::LineTo { to }
                        | deesainer_doc::PathCmd::QuadTo { to, .. }
                        | deesainer_doc::PathCmd::CubicTo { to, .. } => pts.push(to_screen(to)),
                        deesainer_doc::PathCmd::Close => pts.push(start),
                    }
                }
                painter.add(egui::Shape::line(
                    pts,
                    Stroke::new(2.0, TOKENS.selection.gamma_multiply(0.6)),
                ));
            }
        }
    }
    if let Some((pid, seg, v)) = app.net_pen.filter(|(pid, ..)| *pid == id) {
        let (pos, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        if down {
            if let (Some(p), Some(inv), Some(at)) = (pos, t.invert(), net.vertices.get(v)) {
                let drag = inv.apply(app.camera.to_page(origin, p));
                if (drag.x - at.x).abs() + (drag.y - at.y).abs() > 2.0 / app.camera.zoom {
                    app.dispatch(
                        "vector.setTangents",
                        json!({ "id": pid, "segment": seg, "tb": { "x": at.x - drag.x, "y": at.y - drag.y } }),
                    );
                }
            }
            busy = true;
        } else {
            app.net_pen = None;
            if app.drag_group_open {
                app.session.end_group();
                app.drag_group_open = false;
            }
        }
    }
    if !busy && (alt || pen) {
        let (pos, pressed) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed()));
        if let (Some(p), true, Some(inv)) = (pos, pressed, t.invert()) {
            if ui.max_rect().contains(p) {
                let at = inv.apply(app.camera.to_page(origin, p));
                if pen && !app.drag_group_open {
                    app.session.begin_group("Pen");
                    app.drag_group_open = true;
                }
                let reply = app.dispatch(
                    "vector.addVertex",
                    json!({ "id": id, "x": at.x, "y": at.y }),
                );
                if let Some(v) = reply.and_then(|r| r["vertex"].as_u64()) {
                    if let Some(a) = selected {
                        let seg = app
                            .dispatch("vector.connect", json!({ "id": id, "a": a, "b": v }))
                            .and_then(|r| r["segment"].as_u64());
                        if let (true, Some(seg)) = (pen, seg) {
                            app.net_pen = Some((id, seg as usize, v as usize));
                        }
                    }
                    select = Some(v as usize);
                }
                if app.net_pen.is_none() && app.drag_group_open && pen {
                    app.session.end_group();
                    app.drag_group_open = false;
                }
                busy = true;
            }
        }
    }
    if let Some(v) = select {
        app.net_vertex = Some((id, v));
    }
    busy
}

#[cfg(test)]
mod editing_tests {
    use super::*;

    #[test]
    fn text_keys_follow_the_platform() {
        use egui::{Key, Modifiers};
        let cmd = Modifiers {
            command: true,
            mac_cmd: true,
            ..Modifiers::default()
        };
        let alt = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::default()
        };
        let mv = |m, e| Some(vec![EditAction::Move(m, e)]);
        assert_eq!(
            text_key_actions(Key::ArrowLeft, cmd, false, true),
            mv(TextMotion::Home, false)
        );
        assert_eq!(
            text_key_actions(Key::ArrowDown, cmd, false, true),
            mv(TextMotion::Finish, false)
        );
        assert_eq!(
            text_key_actions(Key::ArrowRight, alt, false, true),
            mv(TextMotion::NextWord, false)
        );
        assert_eq!(
            text_key_actions(Key::Backspace, alt, false, true),
            Some(vec![
                EditAction::Move(TextMotion::PreviousWord, true),
                EditAction::Backspace
            ])
        );
        assert_eq!(
            text_key_actions(Key::Backspace, cmd, false, true),
            Some(vec![
                EditAction::Move(TextMotion::Home, true),
                EditAction::Backspace
            ])
        );
        assert_eq!(
            text_key_actions(Key::Backspace, alt, true, true),
            Some(vec![EditAction::Backspace])
        );
        assert_eq!(
            text_key_actions(Key::ArrowLeft, ctrl, false, false),
            mv(TextMotion::PreviousWord, false)
        );
        assert_eq!(
            text_key_actions(Key::End, ctrl, false, false),
            mv(TextMotion::Finish, false)
        );
        assert_eq!(
            text_key_actions(Key::Delete, ctrl, false, false),
            Some(vec![
                EditAction::Move(TextMotion::NextWord, true),
                EditAction::Delete
            ])
        );
        assert_eq!(text_key_actions(Key::A, ctrl, false, false), None);
    }

    #[test]
    fn text_inside_an_instance_is_edited_on_canvas_as_an_override() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let mut run = |cmd: &str, p: serde_json::Value| app.session.dispatch(cmd, &p).unwrap();
        let label = run(
            "node.create.text",
            json!({"x": 0, "y": 0, "content": "Buy", "fontFamily": "sans-serif"}),
        )["id"]
            .as_u64()
            .unwrap();
        let main = run("component.create", json!({"ids": [label]}))["id"]
            .as_u64()
            .unwrap();
        let inst = run(
            "component.instantiate",
            json!({"component": main, "x": 300, "y": 100}),
        )["id"]
            .as_u64()
            .unwrap();
        let (inst, label) = (NodeId(inst), NodeId(label));
        app.start_instance_editing(inst, label);
        let (_, t) = app.editing_frame().unwrap();
        assert!(t.apply(Point::new(0.0, 0.0)).x >= 300.0);
        let edit = app.editing.as_mut().unwrap();
        edit.editor.action(EditAction::SelectAll).unwrap();
        edit.editor
            .action(EditAction::Insert("Subscribe".into()))
            .unwrap();
        app.commit_editing();
        let doc = app.session.doc();
        let NodeKind::Instance { overrides, .. } = &doc.get(inst).unwrap().kind else {
            panic!("not an instance");
        };
        assert!(overrides.iter().any(|o| o.target == label
            && matches!(&o.value, deesainer_doc::PropValue::Kind(NodeKind::Text(t)) if t.content == "Subscribe")));
        assert_eq!(doc.get(label).unwrap().text().unwrap().content, "Buy");
    }

    #[test]
    fn selected_text_survives_panel_focus_then_commits_on_selection_change() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let id = app
            .session
            .dispatch(
                "node.create.text",
                &json!({"x": 0, "y": 0, "content": "Hello", "fontFamily": "sans-serif"}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        app.start_editing(NodeId(id));
        app.editing
            .as_mut()
            .unwrap()
            .editor
            .action(EditAction::SelectAll)
            .unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut App| {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                text_input(app, ui, pos2(0.0, 0.0));
            })
            .drop_without_applying_deltas();
        };
        frame(&mut app);
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("font-search")));
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            text_input(&mut app, ui, pos2(0.0, 0.0));
        });
        assert!(app.editing.is_some());
        assert!(output.platform_output.ime.is_none());
        app.session
            .dispatch("selection.set", &json!({"ids": []}))
            .unwrap();
        frame(&mut app);
        assert!(app.editing.is_none());
    }

    #[test]
    fn typing_is_previewed_then_committed_as_one_history_step() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let id = app
            .session
            .dispatch(
                "node.create.text",
                &json!({
                    "x": 0, "y": 0, "content": "Hello", "fontFamily": "sans-serif"
                }),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let id = NodeId(id);
        app.start_editing(id);
        let ctx = egui::Context::default();
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                text_input(app, ui, pos2(0.0, 0.0));
            })
            .drop_without_applying_deltas();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![egui::Event::Text("!".into())]);
        assert_eq!(
            app.session.doc().get(id).unwrap().text().unwrap().content,
            "Hello"
        );
        assert_eq!(
            app.editing.as_ref().unwrap().editor.data().content,
            "Hello!"
        );
        frame(
            &mut app,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "ni".into(),
                active_range_chars: None,
            })],
        );
        assert_eq!(app.editing.as_ref().unwrap().preedit, "ni");
        assert_eq!(
            app.editing.as_ref().unwrap().editor.data().content,
            "Hello!"
        );
        frame(
            &mut app,
            vec![egui::Event::Ime(egui::ImeEvent::Commit("你".into()))],
        );
        assert!(app.editing.as_ref().unwrap().preedit.is_empty());
        assert_eq!(
            app.editing.as_ref().unwrap().editor.data().content,
            "Hello!你"
        );
        app.editing
            .as_mut()
            .unwrap()
            .editor
            .action(EditAction::SelectAll)
            .unwrap();
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::B,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
        );
        assert_eq!(
            app.editing.as_ref().unwrap().editor.data().runs[0]
                .style
                .font_weight,
            700
        );
        assert!(app
            .session
            .doc()
            .get(id)
            .unwrap()
            .text()
            .unwrap()
            .runs
            .is_empty());
        app.commit_editing();
        assert_eq!(
            app.session.doc().get(id).unwrap().text().unwrap().content,
            "Hello!你"
        );
        assert_eq!(
            app.session.doc().get(id).unwrap().text().unwrap().runs[0]
                .style
                .font_weight,
            700
        );
        app.session.dispatch("edit.undo", &json!({})).unwrap();
        assert_eq!(
            app.session.doc().get(id).unwrap().text().unwrap().content,
            "Hello"
        );
        assert!(app
            .session
            .doc()
            .get(id)
            .unwrap()
            .text()
            .unwrap()
            .runs
            .is_empty());
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    use deesainer_doc::VectorNetwork;

    fn setup() -> (App, NodeId) {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let id = app
            .session
            .dispatch(
                "node.create.vector",
                &json!({"cmds": [
                    {"cmd": "moveTo", "to": {"x": 100, "y": 100}},
                    {"cmd": "lineTo", "to": {"x": 300, "y": 100}},
                    {"cmd": "lineTo", "to": {"x": 300, "y": 260}},
                    {"cmd": "lineTo", "to": {"x": 100, "y": 260}},
                    {"cmd": "close"},
                ]}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let id = NodeId(id);
        app.session
            .dispatch("vector.toNetwork", &json!({"id": id}))
            .unwrap();
        app.session
            .dispatch("selection.set", &json!({"ids": [id]}))
            .unwrap();
        app.camera = Camera {
            pan: vec2(0.0, 0.0),
            zoom: 1.0,
        };
        (app, id)
    }

    fn network(app: &App, id: NodeId) -> VectorNetwork {
        app.session.doc().get(id).unwrap().network.clone().unwrap()
    }

    fn vertex_at(app: &App, id: NodeId, i: usize) -> Pos2 {
        let doc = app.session.doc();
        let t = doc.world_transform(id);
        let v = network(app, id).vertices[i];
        app.camera.to_screen(pos2(0.0, 0.0), t.apply(v))
    }

    struct Driver {
        ctx: egui::Context,
        clock: std::cell::Cell<f64>,
    }

    impl Driver {
        fn new() -> Self {
            Self {
                ctx: egui::Context::default(),
                clock: std::cell::Cell::new(0.0),
            }
        }

        fn frame(&self, app: &mut App, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
            let mut all = vec![egui::Event::ModifiersChanged(modifiers)];
            all.extend(events);
            self.clock.set(self.clock.get() + 0.05);
            let input = egui::RawInput {
                events: all,
                time: Some(self.clock.get()),
                screen_rect: Some(ERect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 700.0))),
                ..Default::default()
            };
            self.ctx
                .run_ui(input, |ui| {
                    network_handles(app, ui, pos2(0.0, 0.0));
                })
                .drop_without_applying_deltas();
        }

        fn press(&self, app: &mut App, at: Pos2, modifiers: egui::Modifiers) {
            self.frame(app, vec![egui::Event::PointerMoved(at)], modifiers);
            self.frame(
                app,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                }],
                modifiers,
            );
        }

        fn release(&self, app: &mut App, at: Pos2, modifiers: egui::Modifiers) {
            self.frame(
                app,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                }],
                modifiers,
            );
            self.frame(app, vec![], modifiers);
        }

        fn click(&self, app: &mut App, at: Pos2, modifiers: egui::Modifiers) {
            self.press(app, at, modifiers);
            self.release(app, at, modifiers);
        }
    }

    const NONE: egui::Modifiers = egui::Modifiers::NONE;

    #[test]
    fn clicking_selects_and_shift_clicking_joins_two_vertices() {
        let (mut app, id) = setup();
        let d = Driver::new();
        d.frame(&mut app, vec![], NONE);
        let at = vertex_at(&app, id, 0);
        d.click(&mut app, at, NONE);
        assert_eq!(app.net_vertex, Some((id, 0)));
        assert_eq!(network(&app, id).segments.len(), 4);
        let shift = egui::Modifiers {
            shift: true,
            ..NONE
        };
        let at = vertex_at(&app, id, 2);
        d.click(&mut app, at, shift);
        let net = network(&app, id);
        assert_eq!(net.segments.len(), 5, "a diagonal joined the corners");
        assert_eq!(net.regions.len(), 2);
        assert_eq!(app.net_vertex, Some((id, 2)));
    }

    #[test]
    fn dragging_a_vertex_moves_it_as_one_undo_step() {
        let (mut app, id) = setup();
        let d = Driver::new();
        d.frame(&mut app, vec![], NONE);
        let from = vertex_at(&app, id, 2);
        let revision = app.session.revision();
        d.press(&mut app, from, NONE);
        let to = from + vec2(-40.0, -30.0);
        for step in 1..=4 {
            let at = from + (to - from) * (step as f32 / 4.0);
            d.frame(&mut app, vec![egui::Event::PointerMoved(at)], NONE);
        }
        d.release(&mut app, to, NONE);
        let moved = vertex_at(&app, id, 2);
        assert!(
            (moved.x - to.x).abs() < 1.5 && (moved.y - to.y).abs() < 1.5,
            "{moved:?} vs {to:?}"
        );
        assert!(app.session.revision() > revision);
        app.session.dispatch("edit.undo", &json!({})).unwrap();
        let back = vertex_at(&app, id, 2);
        assert!((back.x - from.x).abs() < 1.5 && (back.y - from.y).abs() < 1.5);
    }

    #[test]
    fn alt_click_on_empty_canvas_adds_a_vertex_joined_to_the_selected_one() {
        let (mut app, id) = setup();
        let d = Driver::new();
        d.frame(&mut app, vec![], NONE);
        let at = vertex_at(&app, id, 1);
        d.click(&mut app, at, NONE);
        let alt = egui::Modifiers { alt: true, ..NONE };
        let before = network(&app, id);
        d.click(&mut app, pos2(450.0, 60.0), alt);
        let after = network(&app, id);
        assert_eq!(after.vertices.len(), before.vertices.len() + 1);
        assert_eq!(after.segments.len(), before.segments.len() + 1);
        let last = after.segments.last().unwrap();
        assert_eq!(last.a.min(last.b), 1);
        assert_eq!(app.net_vertex, Some((id, after.vertices.len() - 1)));
    }

    #[test]
    fn cmd_click_picks_the_region_under_the_pointer() {
        let (mut app, id) = setup();
        app.session
            .dispatch("vector.connect", &json!({"id": id, "a": 0, "b": 2}))
            .unwrap();
        let d = Driver::new();
        d.frame(&mut app, vec![], NONE);
        let cmd = egui::Modifiers {
            command: true,
            ..NONE
        };
        d.click(&mut app, pos2(270.0, 130.0), cmd);
        let first = app.net_region;
        assert!(matches!(first, Some((i, _)) if i == id));
        d.click(&mut app, pos2(130.0, 230.0), cmd);
        let second = app.net_region;
        assert!(second.is_some() && second != first);
        d.click(&mut app, pos2(600.0, 500.0), cmd);
        assert!(app.net_region.is_none());
    }

    #[test]
    fn delete_removes_the_selected_vertex() {
        let (mut app, id) = setup();
        let d = Driver::new();
        d.frame(&mut app, vec![], NONE);
        let at = vertex_at(&app, id, 3);
        d.click(&mut app, at, NONE);
        assert_eq!(app.net_vertex, Some((id, 3)));
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: NONE,
            }],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| crate::menu::handle_shortcuts(&mut app, ui))
            .drop_without_applying_deltas();
        let net = network(&app, id);
        assert_eq!(net.vertices.len(), 3);
        assert!(app.net_vertex.is_none());
        assert!(app.session.doc().contains(id), "the layer itself stays");
    }

    #[test]
    fn the_pen_draws_branches_into_a_network() {
        let (mut app, id) = setup();
        app.tools.active = ToolKind::Pen;
        let d = Driver::new();
        d.frame(&mut app, vec![], NONE);
        let before = network(&app, id);
        let v0 = vertex_at(&app, id, 0);
        d.click(&mut app, v0, NONE);
        d.frame(&mut app, vec![], NONE);
        let empty = pos2(40.0, 40.0);
        d.press(&mut app, empty, NONE);
        d.frame(
            &mut app,
            vec![egui::Event::PointerMoved(empty + vec2(0.0, 30.0))],
            NONE,
        );
        d.release(&mut app, empty + vec2(0.0, 30.0), NONE);
        d.frame(&mut app, vec![], NONE);
        let net = network(&app, id);
        assert_eq!(net.vertices.len(), before.vertices.len() + 1);
        let fresh = net.vertices.len() - 1;
        let seg = net
            .segments
            .iter()
            .find(|s| s.a == 0 && s.b == fresh)
            .expect("a segment from vertex 0 to the new vertex");
        assert!(seg.tb.y.abs() > 10.0, "{seg:?}");
        let at_zero = net.segments.iter().filter(|s| s.a == 0 || s.b == 0).count();
        assert_eq!(at_zero, 3);
        let v2 = vertex_at(&app, id, 2);
        d.click(&mut app, v2, NONE);
        let net2 = network(&app, id);
        assert!(net2
            .segments
            .iter()
            .any(|s| (s.a, s.b) == (fresh, 2) || (s.a, s.b) == (2, fresh)));
        app.session.dispatch("edit.undo", &json!({})).unwrap();
        assert_eq!(network(&app, id).segments.len(), net.segments.len());
        app.session.dispatch("edit.undo", &json!({})).unwrap();
        assert_eq!(network(&app, id).vertices.len(), before.vertices.len());
        assert_eq!(
            app.session
                .doc()
                .get(app.session.current_page())
                .unwrap()
                .children
                .len(),
            1
        );
    }

    #[test]
    fn dragging_a_tangent_handle_bends_a_segment_and_double_click_straightens_it() {
        let (mut app, id) = setup();
        let d = Driver::new();
        d.frame(&mut app, vec![], NONE);
        let at = vertex_at(&app, id, 0);
        d.click(&mut app, at, NONE);
        d.frame(&mut app, vec![], NONE);
        let net = network(&app, id);
        let (si, is_b) = net
            .segments
            .iter()
            .enumerate()
            .find_map(|(i, s)| {
                (s.a == 0)
                    .then_some((i, false))
                    .or((s.b == 0).then_some((i, true)))
            })
            .unwrap();
        app.session
            .dispatch(
                "vector.setTangents",
                &json!({"id": id, "segment": si, if is_b { "tb" } else { "ta" }: {"x": 0.0, "y": -50.0}}),
            )
            .unwrap();
        let bent = network(&app, id).segments[si];
        let t = if is_b { bent.tb } else { bent.ta };
        assert_eq!(t.y, -50.0);
        d.frame(&mut app, vec![], NONE);
        let doc = app.session.doc();
        let tr = doc.world_transform(id);
        let v0 = network(&app, id).vertices[0];
        let grip = app
            .camera
            .to_screen(pos2(0.0, 0.0), tr.apply(Point::new(v0.x + t.x, v0.y + t.y)));
        let target = grip + vec2(30.0, 0.0);
        d.press(&mut app, grip, NONE);
        d.frame(&mut app, vec![egui::Event::PointerMoved(target)], NONE);
        d.release(&mut app, target, NONE);
        let dragged = network(&app, id).segments[si];
        let t2 = if is_b { dragged.tb } else { dragged.ta };
        assert!(
            (t2.x - 30.0).abs() < 2.0 && (t2.y + 50.0).abs() < 2.0,
            "{t2:?}"
        );
        let grip2 = app.camera.to_screen(
            pos2(0.0, 0.0),
            tr.apply(Point::new(v0.x + t2.x, v0.y + t2.y)),
        );
        d.click(&mut app, grip2, NONE);
        d.click(&mut app, grip2, NONE);
        let flat = network(&app, id).segments[si];
        let t3 = if is_b { flat.tb } else { flat.ta };
        assert!(t3.x.abs() < 1e-9 && t3.y.abs() < 1e-9, "{t3:?}");
    }
}

#[cfg(test)]
mod mesh_timing {
    use super::*;

    #[test]
    #[ignore = "timing only"]
    fn mesh_frame_cost() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        app.session.begin_group("bench");
        for f in 0..100 {
            let frame = app
                .session
                .dispatch(
                    "node.create.frame",
                    &json!({"x": (f % 10) as f64 * 1200.0, "y": (f / 10) as f64 * 900.0, "width": 1100, "height": 800}),
                )
                .unwrap()["id"]
                .clone();
            for i in 0..100 {
                let cmd = if i % 7 == 0 {
                    "node.create.ellipse"
                } else {
                    "node.create.rectangle"
                };
                app.session
                    .dispatch(
                        cmd,
                        &json!({"parent": frame, "x": (i % 20) as f64 * 54.0 + 10.0, "y": (i / 20) as f64 * 40.0 + 10.0, "width": 44, "height": 30, "fill": "#4F7CFF"}),
                    )
                    .unwrap();
            }
        }
        app.session.end_group();
        app.render_mode = crate::RenderMode::Mesh;
        let ctx = egui::Context::default();
        for zoom in [1.0, 0.15] {
            app.camera = Camera {
                pan: vec2(0.0, 0.0),
                zoom,
            };
            let mut times = Vec::new();
            for frame in 0..12 {
                app.camera.pan.x -= 7.0;
                let input = egui::RawInput {
                    screen_rect: Some(ERect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0))),
                    ..Default::default()
                };
                let started = std::time::Instant::now();
                let mut drew = false;
                let output = ctx.run_ui(input, |ui| {
                    let rect = ui.max_rect();
                    drew = draw_mesh(&mut app, ui, rect);
                });
                if frame == 5 {
                    let vertices: usize = output
                        .shapes
                        .iter()
                        .map(|s| match &s.shape {
                            egui::Shape::Mesh(m) => m.vertices.len(),
                            _ => 0,
                        })
                        .sum();
                    println!("zoom {zoom}: {vertices} vertices handed to egui");
                }
                output.drop_without_applying_deltas();
                assert!(drew, "the benchmark page must use the mesh path");
                if frame > 0 {
                    times.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(f64::total_cmp);
            println!(
                "mesh frame at zoom {zoom}: median {:.2} ms, worst {:.2} ms",
                times[times.len() / 2],
                times[times.len() - 1]
            );
        }
    }
}
#[cfg(test)]
mod mesh_tests {
    use super::*;

    fn draw_frame(mut app: App) -> App {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            canvas(&mut app, ui);
        })
        .drop_without_applying_deltas();
        app
    }

    #[test]
    fn instances_mesh_without_falling_back() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        app.render_mode = crate::RenderMode::Mesh;
        let rect = app
            .dispatch(
                "node.create.rectangle",
                json!({"x": 0, "y": 0, "width": 100, "height": 40, "fill": "#1E90FF"}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let comp = app
            .dispatch("component.create", json!({"ids": [rect]}))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        for x in [140, 280] {
            app.dispatch(
                "component.instantiate",
                json!({"component": comp, "x": x, "y": 0}),
            )
            .unwrap();
        }
        app = draw_frame(app);
        assert!(
            app.canvas.mesh.is_some(),
            "instances fell back to the CPU renderer: {:?}",
            app.canvas.mesh_failed.as_ref().map(|(_, _, why)| why)
        );
        assert!(app.status.is_none(), "{:?}", app.status);
        app.dispatch(
            "node.setEffects",
            json!({"ids": [rect], "effects": [{
                "type": "dropShadow",
                "color": {"r": 0, "g": 0, "b": 0, "a": 0.2},
                "offset": {"x": 0, "y": 8},
                "blur": 16
            }]}),
        )
        .unwrap();
        app = draw_frame(app);
        assert!(app.canvas.mesh.is_none());
        let why = app
            .canvas
            .mesh_failed
            .as_ref()
            .map(|(_, _, why)| why.clone());
        assert!(
            why.clone().is_some_and(|w| w.contains("effects")),
            "{why:?}"
        );
    }

    const PIXEL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    #[test]
    fn images_mesh_with_one_shared_texture() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        app.render_mode = crate::RenderMode::Mesh;
        let mut ids = Vec::new();
        for x in [0, 150] {
            let placed = app
                .dispatch(
                    "image.place",
                    json!({"data": PIXEL_PNG, "x": x, "y": 0, "width": 100, "height": 100}),
                )
                .unwrap();
            ids.push(placed["id"].as_u64().unwrap());
        }
        app = draw_frame(app);
        assert!(
            app.canvas.mesh.is_some(),
            "images fell back to the CPU renderer: {:?}",
            app.canvas.mesh_failed.as_ref().map(|(_, _, why)| why)
        );
        assert_eq!(app.canvas.mesh_textures.len(), 1);
        app.dispatch(
            "image.setFill",
            json!({"ids": ids, "data": PIXEL_PNG, "scaleMode": "tile"}),
        )
        .unwrap();
        app = draw_frame(app);
        assert_eq!(app.canvas.mesh_textures.len(), 1);
        assert!(app
            .canvas
            .mesh_textures
            .keys()
            .all(|t| matches!(t, deesainer_mesh::TextureRef::Image { repeat: true, .. })));
        app.dispatch("edit.delete", json!({"ids": ids})).unwrap();
        app = draw_frame(app);
        assert!(app.canvas.mesh_textures.is_empty());
    }
}
