use std::fmt::Write as _;

use base64::Engine as _;
use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{
    Document, Effect, FillRule, ImageScaleMode, Node, NodeId, NodeKind, Paint, PaintKind, PathCmd,
    StrokeAlign, StrokeCap, StrokeJoin, VectorPath,
};
use deesainer_geom::Affine;

use crate::{num, IoError, Resolver};

struct Writer<'a> {
    res: Resolver<'a>,
    defs: String,
    body: String,
    next: usize,
}

fn d(path: &VectorPath) -> String {
    let mut s = String::new();
    for c in &path.cmds {
        match *c {
            PathCmd::MoveTo { to } => {
                let _ = write!(s, "M{} {}", num(to.x), num(to.y));
            }
            PathCmd::LineTo { to } => {
                let _ = write!(s, "L{} {}", num(to.x), num(to.y));
            }
            PathCmd::QuadTo { ctrl, to } => {
                let _ = write!(
                    s,
                    "Q{} {} {} {}",
                    num(ctrl.x),
                    num(ctrl.y),
                    num(to.x),
                    num(to.y)
                );
            }
            PathCmd::CubicTo { ctrl1, ctrl2, to } => {
                let _ = write!(
                    s,
                    "C{} {} {} {} {} {}",
                    num(ctrl1.x),
                    num(ctrl1.y),
                    num(ctrl2.x),
                    num(ctrl2.y),
                    num(to.x),
                    num(to.y)
                );
            }
            PathCmd::Close => s.push('Z'),
        }
    }
    s
}

fn color(c: Rgba) -> String {
    let [r, g, b, _] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn matrix(t: &Affine) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        num(t.a),
        num(t.b),
        num(t.c),
        num(t.d),
        num(t.e),
        num(t.f)
    )
}

fn blend_css(b: BlendMode) -> Option<&'static str> {
    Some(match b {
        BlendMode::PassThrough | BlendMode::Normal => return None,
        BlendMode::Darken => "darken",
        BlendMode::Multiply => "multiply",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::Lighten => "lighten",
        BlendMode::Screen => "screen",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::Overlay => "overlay",
        BlendMode::SoftLight => "soft-light",
        BlendMode::HardLight => "hard-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
    })
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

impl Writer<'_> {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    fn paint_ref(&mut self, paint: &Paint, w: f64, h: f64) -> Option<String> {
        let stops = |stops: &[deesainer_doc::ColorStop]| -> String {
            stops
                .iter()
                .map(|s| {
                    format!(
                        r#"<stop offset="{}" stop-color="{}" stop-opacity="{}"/>"#,
                        num(f64::from(s.position)),
                        color(s.color),
                        num(f64::from(s.color.a))
                    )
                })
                .collect()
        };
        match &paint.kind {
            PaintKind::Solid { color: c } => Some(color(*c)),
            PaintKind::LinearGradient {
                start,
                end,
                stops: st,
            } => {
                let id = self.id("g");
                let _ = write!(
                    self.defs,
                    r#"<linearGradient id="{id}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}">{}</linearGradient>"#,
                    num(start.x * w),
                    num(start.y * h),
                    num(end.x * w),
                    num(end.y * h),
                    stops(st)
                );
                Some(format!("url(#{id})"))
            }
            PaintKind::RadialGradient {
                center,
                radius,
                stops: st,
            } => {
                let id = self.id("g");
                let t = Affine::translate(center.x * w, center.y * h)
                    .then(Affine::scale(radius.x * w, radius.y * h));
                let _ = write!(
                    self.defs,
                    r#"<radialGradient id="{id}" gradientUnits="userSpaceOnUse" cx="0" cy="0" r="1" gradientTransform="{}">{}</radialGradient>"#,
                    matrix(&t),
                    stops(st)
                );
                Some(format!("url(#{id})"))
            }
            PaintKind::AngularGradient { .. } | PaintKind::DiamondGradient { .. } => {
                let opaque = Paint {
                    opacity: 1.0,
                    ..paint.clone()
                };
                let pix = deesainer_render::paint_pixmap(&opaque, w, h, 2.0)?;
                let png = deesainer_render::encode_png(&pix).ok()?;
                let id = self.id("p");
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                let _ = write!(
                    self.defs,
                    r#"<pattern id="{id}" patternUnits="userSpaceOnUse" x="0" y="0" width="{}" height="{}"><image width="{}" height="{}" preserveAspectRatio="none" href="data:image/png;base64,{data}"/></pattern>"#,
                    num(w),
                    num(h),
                    num(w),
                    num(h)
                );
                Some(format!("url(#{id})"))
            }
            PaintKind::Image { .. } => None,
        }
    }

    fn solid_opacity(paint: &Paint) -> f64 {
        let base = match &paint.kind {
            PaintKind::Solid { color } => f64::from(color.a),
            _ => 1.0,
        };
        base * f64::from(paint.opacity)
    }

    fn image(&mut self, paint: &Paint, clip_path: &str, w: f64, h: f64) {
        self.image_in(paint, clip_path, (0.0, 0.0, w, h));
    }

    fn image_in(&mut self, paint: &Paint, clip_path: &str, frame: (f64, f64, f64, f64)) {
        let PaintKind::Image {
            asset,
            scale_mode,
            adjust,
        } = &paint.kind
        else {
            return;
        };
        let Some(bytes) = self.res.doc.asset(asset) else {
            return;
        };
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let href = if adjust.is_identity() {
            let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                "image/png"
            } else {
                "image/jpeg"
            };
            format!("data:{mime};base64,{}", b64(bytes.as_slice()))
        } else {
            let Some(png) = deesainer_render::images::get_adjusted(self.res.doc, asset, adjust)
                .and_then(|img| deesainer_render::encode_png(&img).ok())
            else {
                return;
            };
            format!("data:image/png;base64,{}", b64(&png))
        };
        let aspect = match scale_mode {
            ImageScaleMode::Fill => "xMidYMid slice",
            ImageScaleMode::Fit => "xMidYMid meet",
            ImageScaleMode::Stretch | ImageScaleMode::Tile => "none",
        };
        let clip = self.id("c");
        let _ = write!(
            self.defs,
            r#"<clipPath id="{clip}"><path d="{clip_path}"/></clipPath>"#
        );
        let _ = write!(
            self.body,
            r#"<image href="{href}" x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="{aspect}" opacity="{}" clip-path="url(#{clip})"/>"#,
            num(frame.0),
            num(frame.1),
            num(frame.2),
            num(frame.3),
            num(f64::from(paint.opacity))
        );
    }

    fn filter(&mut self, effects: &[Effect]) -> Option<String> {
        let mut prims = String::new();
        let mut merge = Vec::new();
        for e in effects.iter().filter(|e| e.is_visible()) {
            match e {
                Effect::DropShadow {
                    color: c,
                    offset,
                    blur,
                    ..
                } => {
                    let r = self.id("r");
                    let _ = write!(
                        prims,
                        r#"<feDropShadow in="SourceGraphic" dx="{}" dy="{}" stdDeviation="{}" flood-color="{}" flood-opacity="{}" result="{r}"/>"#,
                        num(offset.x),
                        num(offset.y),
                        num(blur / 2.0),
                        color(*c),
                        num(f64::from(c.a))
                    );
                    merge.push(r);
                }
                Effect::InnerShadow {
                    color: c,
                    offset,
                    blur,
                    ..
                } => {
                    let r = self.id("r");
                    let _ = write!(
                        prims,
                        r#"<feOffset in="SourceAlpha" dx="{}" dy="{}" result="{r}o"/><feGaussianBlur in="{r}o" stdDeviation="{}" result="{r}b"/><feComposite in="{r}b" in2="SourceAlpha" operator="arithmetic" k2="-1" k3="1" result="{r}k"/><feFlood flood-color="{}" flood-opacity="{}"/><feComposite in2="{r}k" operator="in" result="{r}"/>"#,
                        num(offset.x),
                        num(offset.y),
                        num(blur / 2.0),
                        color(*c),
                        num(f64::from(c.a))
                    );
                    merge.push(format!("inner:{r}"));
                }
                Effect::LayerBlur { radius, .. } => {
                    let r = self.id("r");
                    let _ = write!(
                        prims,
                        r#"<feGaussianBlur in="SourceGraphic" stdDeviation="{}" result="{r}"/>"#,
                        num(radius / 2.0)
                    );
                    merge.push(format!("blur:{r}"));
                }
                Effect::BackgroundBlur { .. } => {}
            }
        }
        if merge.is_empty() {
            return None;
        }
        let id = self.id("f");
        let mut nodes = String::new();
        let mut has_source = true;
        for m in &merge {
            if let Some(b) = m.strip_prefix("blur:") {
                has_source = false;
                let _ = write!(nodes, r#"<feMergeNode in="{b}"/>"#);
            } else if m.starts_with("inner:") {
                continue;
            } else {
                let _ = write!(nodes, r#"<feMergeNode in="{m}"/>"#);
            }
        }
        if has_source {
            nodes.push_str(r#"<feMergeNode in="SourceGraphic"/>"#);
        }
        for m in &merge {
            if let Some(r) = m.strip_prefix("inner:") {
                let _ = write!(nodes, r#"<feMergeNode in="{r}"/>"#);
            }
        }
        let _ = write!(
            self.defs,
            r#"<filter id="{id}" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB">{prims}<feMerge>{nodes}</feMerge></filter>"#
        );
        Some(id)
    }

    fn node(&mut self, id: NodeId, transform: &Affine, depth: usize) {
        if depth > deesainer_doc::MAX_DEPTH {
            return;
        }
        let Some(node) = self.res.node(id) else {
            return;
        };
        if !node.visible {
            return;
        }
        let t = transform.then(Document::local_transform(&node));
        let mut attrs = format!(r#" transform="{}""#, matrix(&t));
        let _ = write!(attrs, r#" data-name="{}""#, escape(&node.name));
        if node.opacity < 1.0 {
            let _ = write!(attrs, r#" opacity="{}""#, num(f64::from(node.opacity)));
        }
        if let Some(b) = blend_css(node.blend) {
            let _ = write!(attrs, r#" style="mix-blend-mode:{b}""#);
        }
        if let Some(f) = self.filter(&node.effects) {
            let _ = write!(attrs, r#" filter="url(#{f})""#);
        }
        let _ = write!(self.body, "<g{attrs}>");
        let (w, h) = (node.rect.width, node.rect.height);
        let outline = self.res.outline(&node);
        let path_d = outline.as_ref().map(d).unwrap_or_default();
        let rule = match outline.as_ref().map(|o| o.fill_rule) {
            Some(FillRule::EvenOdd) => "evenodd",
            _ => "nonzero",
        };
        if let Some(outline) = &outline {
            for (part, paints) in node.fill_parts(outline) {
                let part_d = d(&part);
                for paint in paints.iter().filter(|p| p.visible) {
                    if matches!(paint.kind, PaintKind::Image { .. }) {
                        self.image(paint, &part_d, w, h);
                    } else if let Some(fill) = self.paint_ref(paint, w, h) {
                        let _ = write!(
                            self.body,
                            r#"<path d="{part_d}" fill="{fill}" fill-rule="{rule}" fill-opacity="{}"/>"#,
                            num(Self::solid_opacity(paint))
                        );
                    }
                }
            }
        }
        for (paint, path) in self.res.text_colored(&node) {
            if !paint.visible {
                continue;
            }
            let part_d = d(&path);
            if matches!(paint.kind, PaintKind::Image { .. }) {
                match path.bounds() {
                    Some(b) => self.image_in(&paint, &part_d, (b.x, b.y, b.width, b.height)),
                    None => self.image(&paint, &part_d, w, h),
                }
            } else if let Some(fill) = self.paint_ref(&paint, w, h) {
                let _ = write!(
                    self.body,
                    r#"<path d="{part_d}" fill="{fill}" fill-opacity="{}"/>"#,
                    num(Self::solid_opacity(&paint))
                );
            }
        }
        let (children, pushed) = self.res.children(&node);
        if !children.is_empty() {
            let clips = node.clips_content && node.kind.is_frame_like();
            if clips {
                let c = self.id("c");
                let _ = write!(
                    self.defs,
                    r#"<clipPath id="{c}"><path d="{path_d}"/></clipPath>"#
                );
                let _ = write!(self.body, r#"<g clip-path="url(#{c})">"#);
            } else {
                self.body.push_str("<g>");
            }
            self.children(&children, depth);
            self.body.push_str("</g>");
        }
        if pushed {
            self.res.pop();
        }
        if let Some(sides) = deesainer_render::side_weights(&node) {
            let ring = deesainer_vector::side_stroke_ring(
                w,
                h,
                &node.corner_radius,
                sides,
                node.stroke.align,
            );
            let ring_d = d(&ring);
            for paint in node.strokes.iter().filter(|p| p.visible) {
                if let Some(fill) = self.paint_ref(paint, w, h) {
                    let _ = write!(
                        self.body,
                        r#"<path d="{ring_d}" fill="{fill}" fill-rule="evenodd" fill-opacity="{}"/>"#,
                        num(Self::solid_opacity(paint))
                    );
                }
            }
        } else if outline.is_some()
            && node.stroke.weight > 0.0
            && !matches!(node.kind, NodeKind::Text(_))
        {
            let open = matches!(node.kind, NodeKind::Line)
                || outline
                    .as_ref()
                    .is_some_and(|o| !o.cmds.iter().any(|c| matches!(c, PathCmd::Close)));
            let align = if open {
                StrokeAlign::Center
            } else {
                node.stroke.align
            };
            let width = if align == StrokeAlign::Center {
                node.stroke.weight
            } else {
                node.stroke.weight * 2.0
            };
            let mut extra = String::new();
            let cap = match node.stroke.cap {
                StrokeCap::None => "butt",
                StrokeCap::Round => "round",
                StrokeCap::Square => "square",
            };
            let join = match node.stroke.join {
                StrokeJoin::Miter => "miter",
                StrokeJoin::Round => "round",
                StrokeJoin::Bevel => "bevel",
            };
            let _ = write!(extra, r#" stroke-linecap="{cap}" stroke-linejoin="{join}""#);
            if node.stroke.dash.len() >= 2 {
                let dash: Vec<String> = node.stroke.dash.iter().map(|v| num(*v)).collect();
                let _ = write!(extra, r#" stroke-dasharray="{}""#, dash.join(" "));
            }
            match align {
                StrokeAlign::Inside => {
                    let c = self.id("c");
                    let _ = write!(
                        self.defs,
                        r#"<clipPath id="{c}"><path d="{path_d}" clip-rule="{rule}"/></clipPath>"#
                    );
                    let _ = write!(extra, r#" clip-path="url(#{c})""#);
                }
                StrokeAlign::Outside => {
                    let m = self.id("m");
                    let _ = write!(
                        self.defs,
                        r#"<mask id="{m}" maskUnits="userSpaceOnUse" x="-100000" y="-100000" width="200000" height="200000"><rect x="-100000" y="-100000" width="200000" height="200000" fill="white"/><path d="{path_d}" fill="black" fill-rule="{rule}"/></mask>"#
                    );
                    let _ = write!(extra, r#" mask="url(#{m})""#);
                }
                StrokeAlign::Center => {}
            }
            for paint in node.strokes.iter().filter(|p| p.visible) {
                if let Some(stroke) = self.paint_ref(paint, w, h) {
                    let _ = write!(
                        self.body,
                        r#"<path d="{path_d}" fill="none" stroke="{stroke}" stroke-width="{}" stroke-opacity="{}"{extra}/>"#,
                        num(width),
                        num(Self::solid_opacity(paint))
                    );
                }
            }
        }
        self.body.push_str("</g>");
    }

    fn children(&mut self, ids: &[NodeId], depth: usize) {
        let mut i = 0;
        while i < ids.len() {
            let is_mask = self
                .res
                .node(ids[i])
                .is_some_and(|n| n.is_mask && n.visible);
            if !is_mask {
                self.node(ids[i], &Affine::IDENTITY, depth + 1);
                i += 1;
                continue;
            }
            let saved = std::mem::take(&mut self.body);
            self.node(ids[i], &Affine::IDENTITY, depth + 1);
            let mask_body = std::mem::replace(&mut self.body, saved);
            let m = self.id("m");
            let _ = write!(
                self.defs,
                r#"<mask id="{m}" maskUnits="userSpaceOnUse" x="-100000" y="-100000" width="200000" height="200000" style="mask-type:alpha">{mask_body}</mask>"#
            );
            let _ = write!(self.body, r#"<g mask="url(#{m})">"#);
            for id in &ids[i + 1..] {
                self.node(*id, &Affine::IDENTITY, depth + 1);
            }
            self.body.push_str("</g>");
            break;
        }
    }
}

pub fn export_svg(doc: &Document, id: NodeId) -> Result<String, IoError> {
    let node = doc.get(id).ok_or(IoError::UnknownNode(id))?;
    let bounds = if matches!(node.kind, NodeKind::Page) {
        node.children
            .iter()
            .filter_map(|c| deesainer_render::render_bounds(doc, *c))
            .reduce(deesainer_geom::Rect::union)
            .ok_or_else(|| IoError::Other("the page is empty".into()))?
    } else {
        deesainer_render::render_bounds(doc, id).ok_or(IoError::UnknownNode(id))?
    };
    let mut w = Writer {
        res: Resolver::new(doc),
        defs: String::new(),
        body: String::new(),
        next: 0,
    };
    let origin = Affine::translate(-bounds.x, -bounds.y);
    if matches!(node.kind, NodeKind::Page) {
        let children = node.children.clone();
        w.children_with(&children, &origin);
    } else {
        let parent_t = node
            .parent
            .map_or(Affine::IDENTITY, |p| doc.world_transform(p));
        w.node(id, &origin.then(parent_t), 0);
    }
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" fill="none"><defs>{}</defs>{}</svg>"#,
        num(bounds.width),
        num(bounds.height),
        num(bounds.width),
        num(bounds.height),
        w.defs,
        w.body
    ))
}

impl Writer<'_> {
    fn children_with(&mut self, ids: &[NodeId], t: &Affine) {
        let _ = write!(self.body, r#"<g transform="{}">"#, matrix(t));
        self.children(ids, 0);
        self.body.push_str("</g>");
    }
}

#[allow(dead_code)]
fn _node_used(_: &Node) {}
