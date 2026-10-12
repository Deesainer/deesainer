use deesainer_doc::{Document, NodeId, TransitionKind};
use deesainer_geom::Point;
use deesainer_render::{render_node, Options};
use deesainer_tools::proto::{Event, Player};
use egui::{pos2, vec2, Color32, Rect as ERect};

use crate::App;

pub struct PlayerWindow {
    pub player: Player,
    textures: Vec<egui::TextureHandle>,
    page: NodeId,
    start: Option<NodeId>,
    scroll: deesainer_tools::proto::ScrollOffsets,
    scrolled_frame: Option<NodeId>,
}

impl PlayerWindow {
    pub fn start(doc: &Document, page: NodeId, frame: Option<NodeId>) -> Option<Self> {
        Some(Self {
            player: Player::start(doc, page, frame)?,
            textures: Vec::new(),
            page,
            start: frame,
            scroll: Default::default(),
            scrolled_frame: None,
        })
    }
}

fn texture(
    ctx: &egui::Context,
    doc: &Document,
    id: NodeId,
    scale: f64,
    name: &str,
) -> Option<(egui::TextureHandle, egui::Vec2)> {
    let opts = Options {
        background: false,
        text: Some(deesainer_engine::text_painter()),
        hidden: &[],
    };
    let (pix, bounds) = render_node(doc, id, scale, opts).ok()?;
    let img = egui::ColorImage::from_rgba_premultiplied(
        [pix.width() as usize, pix.height() as usize],
        pix.data(),
    );
    let tex = ctx.load_texture(name, img, egui::TextureOptions::LINEAR);
    Some((
        tex,
        vec2(
            (bounds.width * scale) as f32,
            (bounds.height * scale) as f32,
        ),
    ))
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut pw) = app.player.take() else {
        return;
    };
    let mut open = true;
    let doc = app.session.snapshot();
    let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
    let mut url_to_open = None;
    for e in pw.player.tick(&doc, dt) {
        if let Event::OpenUrl(u) = e {
            url_to_open = Some(u);
        }
    }
    let (esc, restart) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::R),
        )
    });
    if esc {
        open = false;
    }
    if restart {
        if let Some(fresh) = PlayerWindow::start(&doc, pw.page, pw.start) {
            pw = fresh;
        }
    }
    let screen = ctx.content_rect();
    let file_name = app.file_name.clone();
    egui::Area::new(egui::Id::new("present"))
        .fixed_pos(screen.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let painter = ui.painter_at(screen);
            painter.rect_filled(screen, 0.0, Color32::BLACK);
            let bar = ERect::from_min_size(screen.min, vec2(screen.width(), 44.0));
            painter.rect_filled(bar, 0.0, Color32::from_rgb(44, 44, 44));
            painter.text(
                bar.center(),
                egui::Align2::CENTER_CENTER,
                &file_name,
                crate::theme::strong(),
                Color32::WHITE,
            );
            let close =
                ERect::from_center_size(bar.right_center() - vec2(28.0, 0.0), vec2(28.0, 28.0));
            let close_resp =
                ui.interact(close, egui::Id::new("present-close"), egui::Sense::click());
            if close_resp.hovered() {
                painter.rect_filled(close, 6.0, Color32::from_rgb(70, 70, 70));
            }
            crate::icons::paint(
                &painter,
                close.shrink(6.0),
                crate::icons::Icon::Close,
                Color32::WHITE,
            );
            if close_resp.clicked() {
                open = false;
            }
            let frames = deesainer_tools::proto::frames(&doc, pw.page);
            let idx = frames
                .iter()
                .position(|f| *f == pw.player.current)
                .map_or(1, |i| i + 1);
            let counter = format!("{idx} / {}", frames.len().max(1));
            let pill =
                ERect::from_center_size(screen.center_bottom() - vec2(0.0, 28.0), vec2(56.0, 28.0));
            painter.rect_filled(pill, 8.0, Color32::from_rgb(44, 44, 44));
            painter.text(
                pill.center(),
                egui::Align2::CENTER_CENTER,
                counter,
                crate::theme::strong(),
                Color32::WHITE,
            );
            let rs =
                ERect::from_min_size(screen.right_bottom() - vec2(104.0, 42.0), vec2(92.0, 28.0));
            let rs_resp = ui.interact(rs, egui::Id::new("present-restart"), egui::Sense::click());
            painter.rect_filled(rs, 8.0, Color32::from_rgb(44, 44, 44));
            painter.text(
                rs.center(),
                egui::Align2::CENTER_CENTER,
                "⟲ Restart  R",
                crate::theme::body(),
                Color32::WHITE,
            );
            if rs_resp.clicked() {
                if let Some(fresh) = PlayerWindow::start(&doc, pw.page, pw.start) {
                    pw = fresh;
                }
            }
            let stage = ERect::from_min_max(
                pos2(screen.min.x + 24.0, bar.max.y + 24.0),
                pos2(screen.max.x - 24.0, screen.max.y - 64.0),
            );
            let response = ui.interact(stage, egui::Id::new("present-stage"), egui::Sense::click());
            let device = doc.get(pw.page).map(|p| p.device).unwrap_or_default();
            let (side, top, bottom) = bezel_insets(device);
            let rect =
                ERect::from_min_max(stage.min + vec2(side, top), stage.max - vec2(side, bottom));
            let top = pw.player.top();
            let Some(fb) = doc.world_bounds(pw.player.current) else {
                return;
            };
            if pw.scrolled_frame != Some(pw.player.current) {
                pw.scroll.clear();
                pw.scrolled_frame = Some(pw.player.current);
            }
            let shown = (!pw.player.variables.is_empty()).then(|| {
                deesainer_engine::preview::with_variable_values(&doc, &pw.player.variables)
            });
            let view =
                deesainer_tools::proto::scrolled_view(shown.as_ref().unwrap_or(&doc), &pw.scroll);
            let ppp = f64::from(ctx.pixels_per_point());
            let fit = (f64::from(rect.width()) / fb.width.max(1.0))
                .min(f64::from(rect.height()) / fb.height.max(1.0))
                .min(2.0);
            let scale = fit * ppp;
            let size = vec2((fb.width * fit) as f32, (fb.height * fit) as f32);
            let frame_rect = ERect::from_center_size(rect.center(), size);
            draw_bezel(&ui.painter_at(stage), device, frame_rect);
            pw.textures.clear();
            let uv = ERect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let painter = ui.painter_at(rect);
            match pw.player.transition {
                Some(t) => {
                    let p = t.progress() as f32;
                    match t.kind {
                        TransitionKind::SmartAnimate => {
                            let mid =
                                deesainer_algo::smart_animate(&doc, t.from, t.to, f64::from(p));
                            if let Some((tex, _)) = texture(ctx, &mid, t.to, scale, "proto-mid") {
                                painter.image(tex.id(), frame_rect, uv, Color32::WHITE);
                                pw.textures.push(tex);
                            }
                        }
                        TransitionKind::Dissolve => {
                            for (id, alpha) in [(t.from, 1.0 - p), (t.to, p)] {
                                if let Some((tex, _)) = texture(ctx, &doc, id, scale, "proto-d") {
                                    painter.image(
                                        tex.id(),
                                        frame_rect,
                                        uv,
                                        Color32::from_white_alpha((alpha * 255.0) as u8),
                                    );
                                    pw.textures.push(tex);
                                }
                            }
                        }
                        _ => {
                            let (incoming, outgoing) = t.offsets();
                            for (id, off) in [(t.from, outgoing), (t.to, incoming)] {
                                if let Some((tex, _)) = texture(ctx, &doc, id, scale, "proto-m") {
                                    let shift = vec2(off.0 as f32 * size.x, off.1 as f32 * size.y);
                                    painter.image(
                                        tex.id(),
                                        frame_rect.translate(shift),
                                        uv,
                                        Color32::WHITE,
                                    );
                                    pw.textures.push(tex);
                                }
                            }
                        }
                    }
                    ctx.request_repaint();
                }
                None => {
                    if let Some((tex, _)) =
                        texture(ctx, &view, pw.player.current, scale, "proto-frame")
                    {
                        painter.image(tex.id(), frame_rect, uv, Color32::WHITE);
                        pw.textures.push(tex);
                    }
                    for o in pw.player.overlays.clone() {
                        if let (Some(ob), Some((tex, osize))) =
                            (doc.world_bounds(o), texture(ctx, &doc, o, scale, "proto-o"))
                        {
                            let _ = ob;
                            let r =
                                ERect::from_center_size(frame_rect.center(), osize / ppp as f32);
                            painter.rect_filled(frame_rect, 0.0, Color32::from_black_alpha(90));
                            painter.image(tex.id(), r, uv, Color32::WHITE);
                            pw.textures.push(tex);
                        }
                    }
                }
            }
            let top_rect = if top == pw.player.current {
                frame_rect
            } else {
                doc.world_bounds(top)
                    .map(|b| {
                        ERect::from_center_size(
                            frame_rect.center(),
                            vec2((b.width * fit) as f32, (b.height * fit) as f32),
                        )
                    })
                    .unwrap_or(frame_rect)
            };
            let to_frame = |pos: egui::Pos2| {
                Point::new(
                    f64::from(pos.x - top_rect.min.x) / fit,
                    f64::from(pos.y - top_rect.min.y) / fit,
                )
            };
            if let Some(pos) = response.hover_pos() {
                if let Some(Event::OpenUrl(u)) = pw.player.hover(&view, to_frame(pos)) {
                    url_to_open = Some(u);
                }
                let wheel = ctx.input(|i| i.smooth_scroll_delta);
                if wheel != egui::Vec2::ZERO && pw.player.transition.is_none() {
                    if let Some(f) =
                        deesainer_tools::proto::scroll_target(&doc, pw.page, top, to_frame(pos))
                    {
                        let o = pw.scroll.entry(f).or_insert((0.0, 0.0));
                        o.0 -= f64::from(wheel.x) / fit;
                        o.1 -= f64::from(wheel.y) / fit;
                        deesainer_tools::proto::clamp_scroll(&doc, &mut pw.scroll);
                        ctx.request_repaint();
                    }
                }
            }
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if let Some(Event::OpenUrl(u)) = pw.player.click(&view, to_frame(pos)) {
                        url_to_open = Some(u);
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        });
    if let Some(url) = url_to_open {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    if open {
        app.player = Some(pw);
    }
}

fn bezel_insets(device: deesainer_doc::Device) -> (f32, f32, f32) {
    use deesainer_doc::Device;
    match device {
        Device::None => (0.0, 0.0, 0.0),
        Device::Phone => (16.0, 40.0, 40.0),
        Device::Tablet => (28.0, 28.0, 28.0),
        Device::Browser => (2.0, 44.0, 2.0),
    }
}

fn draw_bezel(painter: &egui::Painter, device: deesainer_doc::Device, screen: ERect) {
    use deesainer_doc::Device;
    let body = Color32::from_rgb(28, 28, 30);
    let edge = egui::Stroke::new(1.5, Color32::from_rgb(80, 80, 84));
    match device {
        Device::None => {}
        Device::Phone => {
            let outer =
                ERect::from_min_max(screen.min - vec2(14.0, 34.0), screen.max + vec2(14.0, 34.0));
            painter.rect(outer, 36.0, body, edge, egui::StrokeKind::Outside);
            painter.rect_filled(
                ERect::from_center_size(
                    pos2(outer.center().x, outer.min.y + 17.0),
                    vec2(56.0, 6.0),
                ),
                3.0,
                Color32::from_rgb(60, 60, 64),
            );
            painter.rect_filled(
                ERect::from_center_size(
                    pos2(outer.center().x, outer.max.y - 17.0),
                    vec2(96.0, 5.0),
                ),
                2.5,
                Color32::from_rgb(90, 90, 94),
            );
        }
        Device::Tablet => {
            let outer = screen.expand(24.0);
            painter.rect(outer, 28.0, body, edge, egui::StrokeKind::Outside);
            painter.circle_filled(
                pos2(outer.center().x, outer.min.y + 12.0),
                3.0,
                Color32::from_rgb(60, 60, 64),
            );
        }
        Device::Browser => {
            let bar = ERect::from_min_max(
                pos2(screen.min.x - 1.0, screen.min.y - 40.0),
                pos2(screen.max.x + 1.0, screen.min.y),
            );
            painter.rect_filled(
                bar,
                egui::CornerRadius {
                    nw: 10,
                    ne: 10,
                    sw: 0,
                    se: 0,
                },
                Color32::from_rgb(232, 232, 236),
            );
            for (i, c) in [(255, 95, 86), (255, 189, 46), (39, 201, 63)]
                .iter()
                .enumerate()
            {
                painter.circle_filled(
                    pos2(bar.min.x + 18.0 + i as f32 * 18.0, bar.center().y),
                    6.0,
                    Color32::from_rgb(c.0, c.1, c.2),
                );
            }
            let field = ERect::from_min_max(
                pos2(bar.min.x + 84.0, bar.min.y + 9.0),
                pos2(bar.max.x - 16.0, bar.max.y - 9.0),
            );
            if field.width() > 40.0 {
                painter.rect_filled(field, 6.0, Color32::WHITE);
            }
        }
    }
}

pub fn present(app: &mut App) {
    let doc = app.session.snapshot();
    let page = app.session.current_page();
    let start = app.session.selection().first().and_then(|id| {
        std::iter::once(*id).chain(doc.ancestors(*id)).find(|a| {
            doc.get(*a)
                .is_some_and(|n| n.parent == Some(page) && n.kind.is_frame_like())
        })
    });
    match PlayerWindow::start(&doc, page, start) {
        Some(pw) => app.player = Some(pw),
        None => app.info("Nothing to present: add a frame first."),
    }
}

pub fn draw_connections(app: &App, painter: &egui::Painter, origin: egui::Pos2) {
    let doc = app.session.doc();
    let page = app.session.current_page();
    let color = crate::theme::TOKENS.selection;
    for f in deesainer_tools::proto::frames(doc, page) {
        for (src, to) in Player::targets(doc, f) {
            let (Some(a), Some(b)) = (doc.world_bounds(src), doc.world_bounds(to)) else {
                continue;
            };
            let start = app
                .camera
                .to_screen(origin, Point::new(a.max_x(), a.center().y));
            let end = app.camera.to_screen(origin, Point::new(b.x, b.y + 20.0));
            let mid = pos2((start.x + end.x) / 2.0, start.y);
            let pts = vec![start, mid, pos2(mid.x, end.y), end];
            painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, color)));
            painter.circle_filled(start, 3.5, color);
            let head = [end, end + vec2(-7.0, -4.0), end + vec2(-7.0, 4.0)];
            painter.add(egui::Shape::convex_polygon(
                head.to_vec(),
                color,
                egui::Stroke::NONE,
            ));
        }
    }
    for s in deesainer_tools::proto::flow_starts(doc, page) {
        if let Some(b) = doc.world_bounds(s) {
            let p = app.camera.to_screen(origin, b.origin()) + vec2(0.0, -18.0);
            painter.text(
                p,
                egui::Align2::LEFT_BOTTOM,
                "▶ Start",
                egui::FontId::proportional(11.0),
                color,
            );
        }
    }
}
