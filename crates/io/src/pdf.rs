use std::collections::BTreeMap;

use deesainer_color::BlendMode;
use deesainer_doc::{
    Document, FillRule, Node, NodeId, NodeKind, Paint, PaintKind, PathCmd, StrokeAlign, VectorPath,
};
use deesainer_geom::{Affine, Rect};
use miniz_oxide::deflate::{compress_to_vec_zlib, CompressionLevel};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Ref};

use crate::{IoError, Resolver};

const RASTER_SCALE: f64 = 3.0;

struct ImageEntry {
    name: String,
    r: Ref,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
    w: u32,
    h: u32,
}

struct Images {
    list: Vec<ImageEntry>,
}

struct Page<'a> {
    res: Resolver<'a>,
    content: Content,
    alphas: BTreeMap<u32, String>,
    images: &'a mut Images,
    next_ref: &'a mut i32,
}

fn path_ops(c: &mut Content, path: &VectorPath, t: &Affine) {
    let p = |q: deesainer_geom::Point| {
        let r = t.apply(q);
        (r.x as f32, r.y as f32)
    };
    let mut last = (0.0f32, 0.0f32);
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo { to } => {
                let (x, y) = p(to);
                c.move_to(x, y);
                last = (x, y);
            }
            PathCmd::LineTo { to } => {
                let (x, y) = p(to);
                c.line_to(x, y);
                last = (x, y);
            }
            PathCmd::QuadTo { ctrl, to } => {
                let (qx, qy) = p(ctrl);
                let (x, y) = p(to);
                let c1 = (
                    last.0 + 2.0 / 3.0 * (qx - last.0),
                    last.1 + 2.0 / 3.0 * (qy - last.1),
                );
                let c2 = (x + 2.0 / 3.0 * (qx - x), y + 2.0 / 3.0 * (qy - y));
                c.cubic_to(c1.0, c1.1, c2.0, c2.1, x, y);
                last = (x, y);
            }
            PathCmd::CubicTo { ctrl1, ctrl2, to } => {
                let (x1, y1) = p(ctrl1);
                let (x2, y2) = p(ctrl2);
                let (x, y) = p(to);
                c.cubic_to(x1, y1, x2, y2, x, y);
                last = (x, y);
            }
            PathCmd::Close => {
                c.close_path();
            }
        }
    }
}

impl Page<'_> {
    fn alpha(&mut self, a: f64) -> Option<String> {
        if a >= 0.999 {
            return None;
        }
        let key = (a.clamp(0.0, 1.0) * 1000.0).round() as u32;
        let len = self.alphas.len();
        Some(
            self.alphas
                .entry(key)
                .or_insert_with(|| format!("A{len}"))
                .clone(),
        )
    }

    fn embed_render(&mut self, id: NodeId, t: &Affine) {
        let doc = self.res.doc;
        let opts = deesainer_render::Options {
            background: false,
            text: Some(&crate::TEXT),
            hidden: &[],
        };
        let Ok((pix, bounds)) = deesainer_render::render_node(doc, id, RASTER_SCALE, opts) else {
            return;
        };
        let parent_t = doc
            .get(id)
            .and_then(|n| n.parent)
            .map_or(Affine::IDENTITY, |p| doc.world_transform(p));
        let to_page = parent_t.invert().unwrap_or(Affine::IDENTITY);
        let placed = t
            .then(to_page)
            .then(Affine::translate(bounds.x, bounds.y))
            .then(Affine::scale(bounds.width, bounds.height));
        self.place_image(&pix, &placed);
    }

    fn place_image(&mut self, pix: &deesainer_render::Pixmap, t: &Affine) {
        let (w, h) = (pix.width(), pix.height());
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut alpha = Vec::with_capacity((w * h) as usize);
        let mut has_alpha = false;
        for px in pix.pixels() {
            let c = px.demultiply();
            rgb.extend_from_slice(&[c.red(), c.green(), c.blue()]);
            alpha.push(c.alpha());
            has_alpha |= c.alpha() < 255;
        }
        let name = format!("I{}", self.images.list.len());
        *self.next_ref += 2;
        let r = Ref::new(*self.next_ref - 1);
        self.images.list.push(ImageEntry {
            name: name.clone(),
            r,
            rgb,
            alpha: has_alpha.then_some(alpha),
            w,
            h,
        });
        let m = t
            .then(Affine::translate(0.0, 1.0))
            .then(Affine::scale(1.0, -1.0));
        self.content.save_state();
        self.content.transform(m.as_array().map(|v| v as f32));
        self.content.x_object(Name(name.as_bytes()));
        self.content.restore_state();
    }

    fn needs_raster(node: &Node) -> bool {
        node.effects.iter().any(|e| e.is_visible())
            || !matches!(node.blend, BlendMode::PassThrough | BlendMode::Normal)
            || node
                .fills
                .iter()
                .chain(node.strokes.iter())
                .any(|p| p.visible && p.stops().is_some())
            || node.is_mask
            || node.network.as_ref().is_some_and(|n| {
                n.region_paints
                    .iter()
                    .flat_map(|r| &r.fills)
                    .any(|p| p.visible && p.stops().is_some())
            })
    }

    fn node(&mut self, id: NodeId, parent_t: &Affine, opacity: f64, depth: usize) {
        if depth > deesainer_doc::MAX_DEPTH {
            return;
        }
        let Some(node) = self.res.node(id) else {
            return;
        };
        if !node.visible {
            return;
        }
        let range_image = self
            .res
            .text_colored(&node)
            .iter()
            .any(|(p, _)| p.visible && matches!(p.kind, PaintKind::Image { .. }));
        if Self::needs_raster(&node) || range_image {
            self.embed_render(id, parent_t);
            return;
        }
        let t = parent_t.then(Document::local_transform(&node));
        let opacity = opacity * f64::from(node.opacity);
        let outline = self.res.outline(&node);
        let even_odd = outline
            .as_ref()
            .is_some_and(|o| o.fill_rule == FillRule::EvenOdd);
        if let Some(path) = &outline {
            for (part, paints) in node.fill_parts(path) {
                for paint in paints.iter().filter(|p| p.visible) {
                    self.fill(&node, &part, &t, paint, opacity, even_odd);
                }
            }
        }
        for (paint, path) in self.res.text_colored(&node) {
            if paint.visible {
                self.fill(&node, &path, &t, &paint, opacity, false);
            }
        }
        let (children, pushed) = self.res.children(&node);
        if !children.is_empty() {
            let clips = node.clips_content && node.kind.is_frame_like();
            self.content.save_state();
            if clips {
                if let Some(path) = &outline {
                    path_ops(&mut self.content, path, &t);
                    self.content.clip_nonzero();
                    self.content.end_path();
                }
            }
            for c in children {
                self.node(c, &t, opacity, depth + 1);
            }
            self.content.restore_state();
        }
        if pushed {
            self.res.pop();
        }
        if let Some(path) = &outline {
            if node.stroke.weight > 0.0 && !matches!(node.kind, NodeKind::Text(_)) {
                for paint in node.strokes.iter().filter(|p| p.visible) {
                    let PaintKind::Solid { color } = paint.kind else {
                        continue;
                    };
                    self.content.save_state();
                    let width = match node.stroke.align {
                        StrokeAlign::Inside => {
                            path_ops(&mut self.content, path, &t);
                            self.content.clip_nonzero();
                            self.content.end_path();
                            node.stroke.weight * 2.0
                        }
                        _ => node.stroke.weight,
                    };
                    let scale = t.determinant().abs().sqrt();
                    if let Some(gs) = self.alpha(f64::from(color.a * paint.opacity) * opacity) {
                        self.content.set_parameters(Name(gs.as_bytes()));
                    }
                    self.content.set_stroke_rgb(color.r, color.g, color.b);
                    self.content.set_line_width((width * scale) as f32);
                    if node.stroke.dash.len() >= 2 {
                        self.content.set_dash_pattern(
                            node.stroke.dash.iter().map(|d| (d * scale) as f32),
                            0.0,
                        );
                    }
                    path_ops(&mut self.content, path, &t);
                    self.content.stroke();
                    self.content.restore_state();
                }
            }
        }
    }

    fn fill(
        &mut self,
        node: &Node,
        path: &VectorPath,
        t: &Affine,
        paint: &Paint,
        opacity: f64,
        even_odd: bool,
    ) {
        match &paint.kind {
            PaintKind::Solid { color } => {
                self.content.save_state();
                if let Some(gs) = self.alpha(f64::from(color.a * paint.opacity) * opacity) {
                    self.content.set_parameters(Name(gs.as_bytes()));
                }
                self.content.set_fill_rgb(color.r, color.g, color.b);
                path_ops(&mut self.content, path, t);
                if even_odd {
                    self.content.fill_even_odd();
                } else {
                    self.content.fill_nonzero();
                }
                self.content.restore_state();
            }
            PaintKind::Image { asset, .. } => {
                let Some(bytes) = self.res.doc.asset(asset) else {
                    return;
                };
                let Some(pix) = deesainer_render::images::decode(bytes) else {
                    return;
                };
                self.content.save_state();
                path_ops(&mut self.content, path, t);
                self.content.clip_nonzero();
                self.content.end_path();
                let (w, h) = (node.rect.width, node.rect.height);
                let (iw, ih) = (f64::from(pix.width()), f64::from(pix.height()));
                let k = (w / iw).max(h / ih);
                let r = Rect::new((w - iw * k) / 2.0, (h - ih * k) / 2.0, iw * k, ih * k);
                let placed = t
                    .then(Affine::translate(r.x, r.y))
                    .then(Affine::scale(r.width, r.height));
                self.place_image(&pix, &placed);
                self.content.restore_state();
            }
            _ => {}
        }
    }
}

pub fn export_pdf(doc: &Document, ids: &[NodeId]) -> Result<Vec<u8>, IoError> {
    if ids.is_empty() {
        return Err(IoError::Other("nothing to export".into()));
    }
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let mut next_ref = 2;
    let mut page_refs = Vec::new();
    let mut pages_data = Vec::new();
    for id in ids {
        let node = doc.get(*id).ok_or(IoError::UnknownNode(*id))?;
        let bounds = deesainer_render::render_bounds(doc, *id).ok_or(IoError::UnknownNode(*id))?;
        let mut images = Images { list: Vec::new() };
        let mut page = Page {
            res: Resolver::new(doc),
            content: Content::new(),
            alphas: BTreeMap::new(),
            images: &mut images,
            next_ref: &mut next_ref,
        };
        let flip = Affine::translate(0.0, bounds.height).then(Affine::scale(1.0, -1.0));
        let origin = flip.then(Affine::translate(-bounds.x, -bounds.y));
        let parent_t = node
            .parent
            .map_or(Affine::IDENTITY, |p| doc.world_transform(p));
        page.node(*id, &origin.then(parent_t), 1.0, 0);
        let alphas = std::mem::take(&mut page.alphas);
        let content = std::mem::replace(&mut page.content, Content::new()).finish();
        next_ref += 2;
        let page_ref = Ref::new(next_ref - 1);
        let content_ref = Ref::new(next_ref);
        let mut gs_refs = Vec::new();
        for (key, name) in &alphas {
            next_ref += 1;
            gs_refs.push((name.clone(), Ref::new(next_ref), f64::from(*key) / 1000.0));
        }
        page_refs.push(page_ref);
        pages_data.push((page_ref, content_ref, content, bounds, gs_refs, images));
    }
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree)
        .kids(page_refs.iter().copied())
        .count(page_refs.len() as i32);
    for (page_ref, content_ref, content, bounds, gs_refs, images) in pages_data {
        {
            let mut page = pdf.page(page_ref);
            page.media_box(pdf_writer::Rect::new(
                0.0,
                0.0,
                bounds.width as f32,
                bounds.height as f32,
            ));
            page.parent(tree);
            page.contents(content_ref);
            let mut resources = page.resources();
            {
                let mut gs = resources.ext_g_states();
                for (name, r, _) in &gs_refs {
                    gs.pair(Name(name.as_bytes()), *r);
                }
            }
            {
                let mut xo = resources.x_objects();
                for img in &images.list {
                    xo.pair(Name(img.name.as_bytes()), img.r);
                }
            }
            resources.finish();
            page.finish();
        }
        pdf.stream(content_ref, &content);
        for (_, r, a) in gs_refs {
            pdf.ext_graphics(r)
                .non_stroking_alpha(a as f32)
                .stroking_alpha(a as f32);
        }
        for ImageEntry {
            r,
            rgb,
            alpha,
            w,
            h,
            ..
        } in images.list
        {
            let level = CompressionLevel::DefaultLevel as u8;
            let encoded = compress_to_vec_zlib(&rgb, level);
            let mask_ref = Ref::new(r.get() + 1);
            {
                let mut image = pdf.image_xobject(r, &encoded);
                image.filter(Filter::FlateDecode);
                image.width(w as i32);
                image.height(h as i32);
                image.color_space().device_rgb();
                image.bits_per_component(8);
                if alpha.is_some() {
                    image.s_mask(mask_ref);
                }
                image.finish();
            }
            if let Some(a) = alpha {
                let encoded = compress_to_vec_zlib(&a, level);
                let mut mask = pdf.image_xobject(mask_ref, &encoded);
                mask.filter(Filter::FlateDecode);
                mask.width(w as i32);
                mask.height(h as i32);
                mask.color_space().device_gray();
                mask.bits_per_component(8);
                mask.finish();
            }
        }
    }
    Ok(pdf.finish())
}
