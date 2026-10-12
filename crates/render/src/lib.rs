pub mod blur;
mod convert;
pub mod images;
mod nonseparable;

use std::borrow::Cow;
use std::collections::BTreeMap;

use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{
    Document, Effect, ImageScaleMode, Node, NodeId, NodeKind, Paint, PaintKind, PropValue,
    StrokeAlign, VectorPath,
};
use deesainer_geom::{Affine, Point, Rect};
use tiny_skia as sk;

pub use tiny_skia::Pixmap;

pub const MAX_PIXELS: u64 = 100_000_000;
pub const MAX_INSTANCE_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RenderError {
    #[error("render size {0}x{1} is empty or too large")]
    Size(u32, u32),
    #[error("no node with id {0}")]
    UnknownNode(NodeId),
    #[error("png encoding failed: {0}")]
    Encode(String),
}

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub transform: Affine,
    pub width: u32,
    pub height: u32,
}

pub trait TextPainter {
    fn outlines(&self, node: &Node) -> Vec<VectorPath>;

    fn parts(&self, node: &Node) -> (Vec<VectorPath>, Vec<(Paint, VectorPath)>) {
        (self.outlines(node), Vec::new())
    }
}

#[derive(Clone, Copy, Default)]
pub struct Options<'a> {
    pub background: bool,
    pub text: Option<&'a dyn TextPainter>,
    pub hidden: &'a [NodeId],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub nodes_drawn: u64,
    pub layers: u64,
}

struct Surface {
    pix: Pixmap,
    ox: i32,
    oy: i32,
}

#[derive(Clone, Copy, PartialEq)]
struct IRect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl IRect {
    fn from_rect(r: &Rect) -> Option<IRect> {
        if !r.is_valid() {
            return None;
        }
        let clamp = |v: f64| v.clamp(-1.0e7, 1.0e7);
        Some(IRect {
            x0: clamp(r.x.floor()) as i32,
            y0: clamp(r.y.floor()) as i32,
            x1: clamp(r.max_x().ceil()) as i32,
            y1: clamp(r.max_y().ceil()) as i32,
        })
    }

    fn intersect(self, o: IRect) -> Option<IRect> {
        let r = IRect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        };
        (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
    }
}

impl Surface {
    fn new(r: IRect) -> Option<Surface> {
        let w = u32::try_from(r.x1 - r.x0).ok()?;
        let h = u32::try_from(r.y1 - r.y0).ok()?;
        if u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        Some(Surface {
            pix: Pixmap::new(w, h)?,
            ox: r.x0,
            oy: r.y0,
        })
    }

    fn bounds(&self) -> IRect {
        IRect {
            x0: self.ox,
            y0: self.oy,
            x1: self.ox + self.pix.width() as i32,
            y1: self.oy + self.pix.height() as i32,
        }
    }

    fn local(&self, t: &Affine) -> sk::Transform {
        convert::transform(&Affine::translate(-f64::from(self.ox), -f64::from(self.oy)).then(*t))
    }

    fn composite(&mut self, layer: &Surface, opacity: f32, blend: sk::BlendMode) {
        if let Some(mode) = nonseparable::mode(blend) {
            let offset = (layer.ox - self.ox, layer.oy - self.oy);
            nonseparable::composite(&mut self.pix, &layer.pix, offset, opacity, mode);
            return;
        }
        let paint = sk::PixmapPaint {
            opacity: opacity.clamp(0.0, 1.0),
            blend_mode: blend,
            quality: sk::FilterQuality::Nearest,
        };
        self.pix.draw_pixmap(
            layer.ox - self.ox,
            layer.oy - self.oy,
            layer.pix.as_ref(),
            &paint,
            sk::Transform::identity(),
            None,
        );
    }

    fn mask_for(&self, path: &sk::Path, rule: sk::FillRule, t: &Affine) -> Option<sk::Mask> {
        let mut mask = sk::Mask::new(self.pix.width(), self.pix.height())?;
        mask.fill_path(path, rule, true, self.local(t));
        Some(mask)
    }
}

fn scale_of(t: &Affine) -> f64 {
    t.determinant().abs().sqrt().max(1e-6)
}

pub fn overridden<'a>(
    doc: &'a Document,
    overrides: &[BTreeMap<NodeId, Vec<PropValue>>],
    id: NodeId,
) -> Option<Cow<'a, Node>> {
    let base = doc.get(id)?;
    let mut node: Option<Node> = None;
    for map in overrides.iter().rev() {
        if let Some(values) = map.get(&id) {
            let n = node.get_or_insert_with(|| base.clone());
            for v in values {
                n.set(v.clone());
            }
        }
    }
    Some(node.map_or(Cow::Borrowed(base), Cow::Owned))
}

struct Renderer<'a> {
    doc: &'a Document,
    opts: Options<'a>,
    overrides: Vec<BTreeMap<NodeId, Vec<PropValue>>>,
    slots: Vec<BTreeMap<NodeId, NodeId>>,
    stats: Stats,
}

impl<'a> Renderer<'a> {
    fn new(doc: &'a Document, opts: Options<'a>) -> Self {
        Self {
            doc,
            opts,
            overrides: Vec::new(),
            slots: Vec::new(),
            stats: Stats::default(),
        }
    }

    fn node(&self, id: NodeId) -> Option<Cow<'a, Node>> {
        overridden(self.doc, &self.overrides, id)
    }

    fn outset(node: &Node) -> f64 {
        let stroke = if node.strokes.iter().any(|s| s.visible) {
            match node.stroke.align {
                StrokeAlign::Inside => 0.0,
                StrokeAlign::Center => node.stroke.weight / 2.0,
                StrokeAlign::Outside => node.stroke.weight,
            }
        } else {
            0.0
        };
        let effects = node.effects.iter().map(Effect::outset).fold(0.0, f64::max);
        let stroked = node.strokes.iter().any(|s| s.visible);
        let join = if stroked
            && matches!(
                node.kind,
                NodeKind::Line | NodeKind::Vector { .. } | NodeKind::Star { .. }
            ) {
            node.stroke.weight * 2.0
        } else {
            0.0
        };
        stroke + effects + join + 1.0
    }

    fn pixel_bounds(node: &Node, t: &Affine, outset: f64) -> Option<IRect> {
        let local = Rect::new(0.0, 0.0, node.rect.width, node.rect.height).inflate(outset);
        IRect::from_rect(&t.transform_rect_bounds(&local))
    }

    fn draw_node(&mut self, s: &mut Surface, id: NodeId, parent_t: &Affine, depth: usize) {
        if depth > deesainer_doc::MAX_DEPTH {
            return;
        }
        let Some(node) = self.node(id) else { return };
        if !node.visible || self.opts.hidden.contains(&id) {
            return;
        }
        let t = parent_t.then(Document::local_transform(&node));
        let is_container =
            node.kind.is_container() || matches!(node.kind, NodeKind::Instance { .. });
        let own = Self::pixel_bounds(&node, &t, Self::outset(&node));
        let clips = node.clips_content && node.kind.is_frame_like();
        if !is_container || clips {
            match own.and_then(|b| b.intersect(s.bounds())) {
                Some(_) => {}
                None => return,
            }
        }
        self.stats.nodes_drawn += 1;
        let layer_blur = node.effects.iter().find_map(|e| match e {
            Effect::LayerBlur {
                radius,
                visible: true,
            } if *radius > 0.0 => Some(*radius),
            _ => None,
        });
        let needs_layer = node.opacity < 1.0
            || !matches!(node.blend, BlendMode::PassThrough | BlendMode::Normal)
            || layer_blur.is_some();
        if needs_layer {
            let region = if is_container && !clips {
                Some(s.bounds())
            } else {
                own.and_then(|b| b.intersect(s.bounds()))
            };
            let Some(mut layer) = region.and_then(Surface::new) else {
                return;
            };
            self.stats.layers += 1;
            self.draw_content(&mut layer, &node, &t, true, depth);
            if let Some(r) = layer_blur {
                blur::blur(&mut layer.pix, r * scale_of(&t));
            }
            s.composite(&layer, node.opacity, convert::blend(node.blend));
        } else {
            self.draw_content(s, &node, &t, true, depth);
        }
    }

    fn outline(&self, node: &Node) -> Option<VectorPath> {
        deesainer_vector::resolved_outline(self.doc, node)
    }

    fn draw_content(
        &mut self,
        s: &mut Surface,
        node: &Node,
        t: &Affine,
        effects: bool,
        depth: usize,
    ) {
        let outline = self.outline(node);
        let sk_path = outline.as_ref().and_then(convert::path);
        let rule = outline
            .as_ref()
            .map_or(sk::FillRule::Winding, |o| convert::fill_rule(o.fill_rule));
        if effects {
            for e in &node.effects {
                if let Effect::DropShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    visible: true,
                } = e
                {
                    self.drop_shadow(s, node, t, *color, *offset, *blur, *spread, depth);
                }
            }
            for e in &node.effects {
                if let (
                    Effect::BackgroundBlur {
                        radius,
                        visible: true,
                    },
                    Some(path),
                ) = (e, &sk_path)
                {
                    self.background_blur(s, path, rule, t, *radius);
                }
            }
        }
        match (&sk_path, &node.kind) {
            (Some(path), _) => match (&outline, node.network.is_some()) {
                (Some(o), true) => {
                    for (part, paints) in node.fill_parts(o) {
                        let Some(part_path) = convert::path(&part) else {
                            continue;
                        };
                        for paint in paints.iter().filter(|p| p.visible) {
                            self.fill(s, &part_path, rule, t, paint, node);
                        }
                    }
                }
                _ => {
                    for paint in node.fills.iter().filter(|p| p.visible) {
                        self.fill(s, path, rule, t, paint, node);
                    }
                }
            },
            (None, NodeKind::Text(_)) => self.draw_text(s, node, t),
            _ => {}
        }
        if effects {
            if let Some(path) = &sk_path {
                for e in &node.effects {
                    if let Effect::InnerShadow {
                        color,
                        offset,
                        blur,
                        spread,
                        visible: true,
                    } = e
                    {
                        self.inner_shadow(s, path, rule, node, t, *color, *offset, *blur, *spread);
                    }
                }
            }
        }
        let filled = self.slots.last().and_then(|m| m.get(&node.id)).copied();
        let children: Vec<NodeId> = match &node.kind {
            _ if filled.is_some() => filled
                .and_then(|c| self.doc.get(c))
                .map(|c| c.children.clone())
                .unwrap_or_default(),
            NodeKind::BooleanOp { .. } => Vec::new(),
            NodeKind::Instance {
                component,
                overrides,
            } => {
                if self.overrides.len() >= MAX_INSTANCE_DEPTH {
                    Vec::new()
                } else if let Some(main) = self.doc.get(*component) {
                    let mut map: BTreeMap<NodeId, Vec<PropValue>> = BTreeMap::new();
                    for (cid, rect) in
                        instance_child_rects(self.doc, main, node.rect.width, node.rect.height)
                    {
                        map.entry(cid).or_default().push(PropValue::Rect(rect));
                    }
                    for o in overrides {
                        map.entry(o.target).or_default().push(o.value.clone());
                    }
                    self.overrides.push(map);
                    self.slots
                        .push(self.doc.slot_fills_from(node.id, *component));
                    let kids = main.children.clone();
                    self.draw_children(s, node, t, &kids, sk_path.as_ref(), rule, depth);
                    self.slots.pop();
                    self.overrides.pop();
                    Vec::new()
                } else {
                    Vec::new()
                }
            }
            _ => node.children.clone(),
        };
        if !children.is_empty() {
            self.draw_children(s, node, t, &children, sk_path.as_ref(), rule, depth);
        }
        if let Some(path) = &sk_path {
            self.strokes(s, path, rule, node, t);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_children(
        &mut self,
        s: &mut Surface,
        node: &Node,
        t: &Affine,
        children: &[NodeId],
        outline: Option<&sk::Path>,
        rule: sk::FillRule,
        depth: usize,
    ) {
        let clips = node.clips_content && node.kind.is_frame_like();
        if clips {
            let Some(region) =
                Self::pixel_bounds(node, t, 0.0).and_then(|b| b.intersect(s.bounds()))
            else {
                return;
            };
            let Some(mut layer) = Surface::new(region) else {
                return;
            };
            self.stats.layers += 1;
            self.draw_child_list(&mut layer, children, t, depth);
            if let Some(path) = outline {
                if let Some(mask) = layer.mask_for(path, rule, t) {
                    layer.pix.apply_mask(&mask);
                }
            }
            s.composite(&layer, 1.0, sk::BlendMode::SourceOver);
        } else {
            self.draw_child_list(s, children, t, depth);
        }
    }

    fn draw_child_list(&mut self, s: &mut Surface, children: &[NodeId], t: &Affine, depth: usize) {
        let mut i = 0;
        while i < children.len() {
            let id = children[i];
            let is_mask = self
                .node(id)
                .is_some_and(|n| n.is_mask && n.visible && !self.opts.hidden.contains(&id));
            if !is_mask {
                self.draw_node(s, id, t, depth + 1);
                i += 1;
                continue;
            }
            let Some(mut mask_surface) = Surface::new(s.bounds()) else {
                return;
            };
            self.draw_node(&mut mask_surface, id, t, depth + 1);
            let Some(mut content) = Surface::new(s.bounds()) else {
                return;
            };
            self.stats.layers += 2;
            for sibling in &children[i + 1..] {
                self.draw_node(&mut content, *sibling, t, depth + 1);
            }
            let mask = sk::Mask::from_pixmap(mask_surface.pix.as_ref(), sk::MaskType::Alpha);
            content.pix.apply_mask(&mask);
            s.composite(&content, 1.0, sk::BlendMode::SourceOver);
            break;
        }
    }

    fn fill(
        &mut self,
        s: &mut Surface,
        path: &sk::Path,
        rule: sk::FillRule,
        t: &Affine,
        paint: &Paint,
        node: &Node,
    ) {
        let (w, h) = (node.rect.width, node.rect.height);
        let blend = convert::blend(paint.blend);
        if nonseparable::mode(blend).is_some() {
            let Some(region) =
                Self::pixel_bounds(node, t, 1.0).and_then(|b| b.intersect(s.bounds()))
            else {
                return;
            };
            let Some(mut layer) = Surface::new(region) else {
                return;
            };
            let normal = Paint {
                blend: BlendMode::Normal,
                ..paint.clone()
            };
            self.fill(&mut layer, path, rule, t, &normal, node);
            s.composite(&layer, 1.0, blend);
            return;
        }
        let stops = |stops: &[deesainer_doc::ColorStop]| -> Vec<sk::GradientStop> {
            stops
                .iter()
                .map(|st| {
                    sk::GradientStop::new(
                        st.position.clamp(0.0, 1.0),
                        convert::color(st.color, paint.opacity),
                    )
                })
                .collect()
        };
        if let PaintKind::DiamondGradient {
            center,
            radius,
            stops: st,
        } = &paint.kind
        {
            let (cx, cy) = (center.x * w, center.y * h);
            let (rx, ry) = (
                (radius.x * w).abs().max(1e-3),
                (radius.y * h).abs().max(1e-3),
            );
            const FAR: f64 = 1e7;
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                let (gx, gy) = (sx / rx, sy / ry);
                let g2 = gx * gx + gy * gy;
                let end = (cx + gx / g2, cy + gy / g2);
                let Some(shader) = sk::LinearGradient::new(
                    sk::Point::from_xy(cx as f32, cy as f32),
                    sk::Point::from_xy(end.0 as f32, end.1 as f32),
                    stops(st),
                    sk::SpreadMode::Pad,
                    sk::Transform::identity(),
                ) else {
                    continue;
                };
                let (x0, x1) = if sx > 0.0 {
                    (cx, cx + FAR)
                } else {
                    (cx - FAR, cx)
                };
                let (y0, y1) = if sy > 0.0 {
                    (cy, cy + FAR)
                } else {
                    (cy - FAR, cy)
                };
                let Some(quadrant) =
                    sk::Rect::from_ltrb(x0 as f32, y0 as f32, x1 as f32, y1 as f32)
                else {
                    continue;
                };
                let Some(mut mask) = s.mask_for(path, rule, t) else {
                    return;
                };
                mask.intersect_path(
                    &sk::PathBuilder::from_rect(quadrant),
                    sk::FillRule::Winding,
                    false,
                    s.local(t),
                );
                let p = sk::Paint {
                    shader,
                    blend_mode: blend,
                    anti_alias: true,
                    ..sk::Paint::default()
                };
                s.pix.fill_path(path, &p, rule, s.local(t), Some(&mask));
            }
            return;
        }
        let shader = match &paint.kind {
            PaintKind::AngularGradient {
                center,
                angle,
                stops: st,
            } => {
                let (cx, cy) = ((center.x * w) as f32, (center.y * h) as f32);
                sk::SweepGradient::new(
                    sk::Point::from_xy(cx, cy),
                    0.0,
                    360.0,
                    stops(st),
                    sk::SpreadMode::Pad,
                    sk::Transform::from_rotate_at(*angle as f32, cx, cy),
                )
            }
            PaintKind::DiamondGradient { .. } => None,
            PaintKind::Solid { color } => Some(sk::Shader::SolidColor(convert::color(
                *color,
                paint.opacity,
            ))),
            PaintKind::LinearGradient {
                start,
                end,
                stops: st,
            } => sk::LinearGradient::new(
                sk::Point::from_xy((start.x * w) as f32, (start.y * h) as f32),
                sk::Point::from_xy((end.x * w) as f32, (end.y * h) as f32),
                stops(st),
                sk::SpreadMode::Pad,
                sk::Transform::identity(),
            ),
            PaintKind::RadialGradient {
                center,
                radius,
                stops: st,
            } => sk::RadialGradient::new(
                sk::Point::from_xy(0.0, 0.0),
                0.0,
                sk::Point::from_xy(0.0, 0.0),
                1.0,
                stops(st),
                sk::SpreadMode::Pad,
                sk::Transform::from_row(
                    (radius.x * w).max(1e-3) as f32,
                    0.0,
                    0.0,
                    (radius.y * h).max(1e-3) as f32,
                    (center.x * w) as f32,
                    (center.y * h) as f32,
                ),
            ),
            PaintKind::Image {
                asset,
                scale_mode,
                adjust,
            } => {
                let Some(img) = images::get_adjusted(self.doc, asset, adjust) else {
                    return;
                };
                let (iw, ih) = (f64::from(img.width()), f64::from(img.height()));
                let (sx, sy, tx, ty, spread) = match scale_mode {
                    ImageScaleMode::Stretch => (w / iw, h / ih, 0.0, 0.0, sk::SpreadMode::Pad),
                    ImageScaleMode::Fill | ImageScaleMode::Fit => {
                        let k = if matches!(scale_mode, ImageScaleMode::Fill) {
                            (w / iw).max(h / ih)
                        } else {
                            (w / iw).min(h / ih)
                        };
                        (
                            k,
                            k,
                            (w - iw * k) / 2.0,
                            (h - ih * k) / 2.0,
                            sk::SpreadMode::Pad,
                        )
                    }
                    ImageScaleMode::Tile => (1.0, 1.0, 0.0, 0.0, sk::SpreadMode::Repeat),
                };
                let shader = sk::Pattern::new(
                    img.as_ref().as_ref(),
                    spread,
                    sk::FilterQuality::Bilinear,
                    paint.opacity.clamp(0.0, 1.0),
                    sk::Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, tx as f32, ty as f32),
                );
                let mut p = sk::Paint {
                    shader,
                    blend_mode: blend,
                    anti_alias: true,
                    ..sk::Paint::default()
                };
                p.anti_alias = true;
                if matches!(scale_mode, ImageScaleMode::Fit) {
                    let area = Rect::new(tx, ty, iw * sx, ih * sy);
                    if let Some(r) = sk::Rect::from_xywh(
                        area.x as f32,
                        area.y as f32,
                        area.width as f32,
                        area.height as f32,
                    ) {
                        let mut mask = match s.mask_for(path, rule, t) {
                            Some(m) => m,
                            None => return,
                        };
                        let clip = sk::PathBuilder::from_rect(r);
                        mask.intersect_path(&clip, sk::FillRule::Winding, true, s.local(t));
                        s.pix.fill_path(path, &p, rule, s.local(t), Some(&mask));
                        return;
                    }
                }
                s.pix.fill_path(path, &p, rule, s.local(t), None);
                return;
            }
        };
        let Some(shader) = shader else { return };
        let p = sk::Paint {
            shader,
            blend_mode: blend,
            anti_alias: true,
            ..sk::Paint::default()
        };
        s.pix.fill_path(path, &p, rule, s.local(t), None);
    }

    fn strokes(
        &mut self,
        s: &mut Surface,
        path: &sk::Path,
        rule: sk::FillRule,
        node: &Node,
        t: &Affine,
    ) {
        let visible: Vec<&Paint> = node.strokes.iter().filter(|p| p.visible).collect();
        let weight = node.stroke.weight;
        let sides = side_weights(node);
        if visible.is_empty() || (weight <= 0.0 && sides.is_none()) {
            return;
        }
        let open = matches!(node.kind, NodeKind::Line)
            || matches!(&node.kind, NodeKind::Vector { path } if !path.cmds.iter().any(|c| matches!(c, deesainer_doc::PathCmd::Close)));
        let align = if open {
            StrokeAlign::Center
        } else {
            node.stroke.align
        };
        let width = match align {
            StrokeAlign::Center => weight,
            StrokeAlign::Inside | StrokeAlign::Outside => weight * 2.0,
        };
        let stroke = convert::stroke(&node.stroke, width);
        let paints: Vec<sk::Paint> = visible
            .iter()
            .filter_map(|p| match &p.kind {
                PaintKind::Solid { color } => Some(sk::Paint {
                    shader: sk::Shader::SolidColor(convert::color(*color, p.opacity)),
                    blend_mode: convert::blend(p.blend),
                    anti_alias: true,
                    ..sk::Paint::default()
                }),
                PaintKind::LinearGradient { stops, .. }
                | PaintKind::RadialGradient { stops, .. }
                | PaintKind::AngularGradient { stops, .. }
                | PaintKind::DiamondGradient { stops, .. } => stops.first().map(|st| sk::Paint {
                    shader: sk::Shader::SolidColor(convert::color(st.color, p.opacity)),
                    anti_alias: true,
                    ..sk::Paint::default()
                }),
                PaintKind::Image { .. } => None,
            })
            .collect();
        if let Some(sides) = sides {
            let ring = deesainer_vector::side_stroke_ring(
                node.rect.width,
                node.rect.height,
                &node.corner_radius,
                sides,
                node.stroke.align,
            );
            if let Some(ring) = convert::path(&ring) {
                for p in &paints {
                    s.pix
                        .fill_path(&ring, p, sk::FillRule::EvenOdd, s.local(t), None);
                }
            }
            return;
        }
        if matches!(align, StrokeAlign::Center) {
            for p in &paints {
                s.pix.stroke_path(path, p, &stroke, s.local(t), None);
            }
            return;
        }
        let Some(region) =
            Self::pixel_bounds(node, t, weight + 2.0).and_then(|b| b.intersect(s.bounds()))
        else {
            return;
        };
        let Some(mut layer) = Surface::new(region) else {
            return;
        };
        self.stats.layers += 1;
        for p in &paints {
            layer
                .pix
                .stroke_path(path, p, &stroke, layer.local(t), None);
        }
        if let Some(mut mask) = layer.mask_for(path, rule, t) {
            if matches!(align, StrokeAlign::Outside) {
                mask.invert();
            }
            layer.pix.apply_mask(&mask);
        }
        s.composite(&layer, 1.0, sk::BlendMode::SourceOver);
    }

    #[allow(clippy::too_many_arguments)]
    fn drop_shadow(
        &mut self,
        s: &mut Surface,
        node: &Node,
        t: &Affine,
        color: Rgba,
        offset: Point,
        blur_radius: f64,
        spread: f64,
        depth: usize,
    ) {
        let k = scale_of(t);
        let pad = blur_radius * 2.0 + spread.max(0.0) + offset.x.abs().max(offset.y.abs()) + 2.0;
        let Some(region) = Self::pixel_bounds(node, t, pad).and_then(|b| b.intersect(s.bounds()))
        else {
            return;
        };
        let Some(mut layer) = Surface::new(region) else {
            return;
        };
        self.stats.layers += 1;
        let shifted = t.then(Affine::translate(offset.x, offset.y));
        self.draw_content(&mut layer, node, &shifted, false, depth);
        if spread > 0.0 {
            if let Some(path) = self.outline(node).as_ref().and_then(convert::path) {
                let st = sk::Stroke {
                    width: (spread * 2.0) as f32,
                    ..sk::Stroke::default()
                };
                let p = sk::Paint::default();
                layer
                    .pix
                    .stroke_path(&path, &p, &st, layer.local(&shifted), None);
            }
        }
        tint(&mut layer.pix, color);
        blur::blur(&mut layer.pix, blur_radius * k);
        s.composite(&layer, 1.0, sk::BlendMode::SourceOver);
    }

    #[allow(clippy::too_many_arguments)]
    fn inner_shadow(
        &mut self,
        s: &mut Surface,
        path: &sk::Path,
        rule: sk::FillRule,
        node: &Node,
        t: &Affine,
        color: Rgba,
        offset: Point,
        blur_radius: f64,
        spread: f64,
    ) {
        let k = scale_of(t);
        let Some(region) = Self::pixel_bounds(node, t, 1.0).and_then(|b| b.intersect(s.bounds()))
        else {
            return;
        };
        let Some(mut layer) = Surface::new(region) else {
            return;
        };
        self.stats.layers += 1;
        layer.pix.fill(sk::Color::BLACK);
        let shifted = t.then(Affine::translate(offset.x, offset.y));
        let cut = sk::Paint {
            blend_mode: sk::BlendMode::DestinationOut,
            anti_alias: true,
            ..sk::Paint::default()
        };
        layer
            .pix
            .fill_path(path, &cut, rule, layer.local(&shifted), None);
        if spread > 0.0 {
            let st = sk::Stroke {
                width: (spread * 2.0) as f32,
                ..sk::Stroke::default()
            };
            let p = sk::Paint {
                anti_alias: true,
                ..sk::Paint::default()
            };
            layer
                .pix
                .stroke_path(path, &p, &st, layer.local(&shifted), None);
        }
        tint(&mut layer.pix, color);
        blur::blur(&mut layer.pix, blur_radius * k);
        if let Some(mask) = layer.mask_for(path, rule, t) {
            layer.pix.apply_mask(&mask);
        }
        s.composite(&layer, 1.0, sk::BlendMode::SourceOver);
    }

    fn background_blur(
        &mut self,
        s: &mut Surface,
        path: &sk::Path,
        rule: sk::FillRule,
        t: &Affine,
        radius: f64,
    ) {
        let bounds = path.bounds();
        let local = Rect::new(
            f64::from(bounds.x()),
            f64::from(bounds.y()),
            f64::from(bounds.width()),
            f64::from(bounds.height()),
        );
        let Some(region) = IRect::from_rect(&t.transform_rect_bounds(&local))
            .and_then(|b| b.intersect(s.bounds()))
        else {
            return;
        };
        let rect = sk::IntRect::from_ltrb(
            region.x0 - s.ox,
            region.y0 - s.oy,
            region.x1 - s.ox,
            region.y1 - s.oy,
        );
        let Some(copy) = rect.and_then(|r| s.pix.clone_rect(r)) else {
            return;
        };
        let mut layer = Surface {
            pix: copy,
            ox: region.x0,
            oy: region.y0,
        };
        blur::blur(&mut layer.pix, radius * scale_of(t));
        if let Some(mask) = layer.mask_for(path, rule, t) {
            layer.pix.apply_mask(&mask);
        }
        self.stats.layers += 1;
        s.composite(&layer, 1.0, sk::BlendMode::SourceOver);
    }

    fn draw_text(&mut self, s: &mut Surface, node: &Node, t: &Affine) {
        let Some(painter) = self.opts.text else {
            return;
        };
        let (glyphs, colored) = painter.parts(node);
        let mut combined = VectorPath::default();
        for g in glyphs {
            combined.cmds.extend(g.cmds);
        }
        let mut whole = combined.clone();
        for (_, p) in &colored {
            whole.cmds.extend(p.cmds.iter().copied());
        }
        let Some(path) = convert::path(&whole) else {
            return;
        };
        if let Some(base) = convert::path(&combined) {
            for paint in node.fills.iter().filter(|p| p.visible) {
                self.fill(s, &base, sk::FillRule::Winding, t, paint, node);
            }
        }
        for (paint, p) in colored.iter().filter(|(paint, _)| paint.visible) {
            let framed = match (&paint.kind, p.bounds()) {
                (PaintKind::Image { .. }, Some(b)) if b.width > 0.0 && b.height > 0.0 => Some(b),
                _ => None,
            };
            if let Some(b) = framed {
                let local = p.map_points(|q| Point::new(q.x - b.x, q.y - b.y));
                if let Some(lp) = convert::path(&local) {
                    let mut frame = node.clone();
                    frame.rect.width = b.width;
                    frame.rect.height = b.height;
                    let moved = t.then(Affine::translate(b.x, b.y));
                    self.fill(s, &lp, sk::FillRule::Winding, &moved, paint, &frame);
                }
            } else if let Some(cp) = convert::path(p) {
                self.fill(s, &cp, sk::FillRule::Winding, t, paint, node);
            }
        }
        if !node.strokes.is_empty() {
            let quarter_px = 0.25 / t.determinant().abs().sqrt().max(1e-9);
            let merged = deesainer_vector::boolean::merged_outline(&whole, quarter_px);
            let path = convert::path(&merged).unwrap_or(path);
            let st = convert::stroke(&node.stroke, node.stroke.weight);
            for p in node.strokes.iter().filter(|p| p.visible) {
                if let Some(c) = p.primary_color() {
                    let paint = sk::Paint {
                        shader: sk::Shader::SolidColor(convert::color(c, p.opacity)),
                        anti_alias: true,
                        ..sk::Paint::default()
                    };
                    s.pix.stroke_path(&path, &paint, &st, s.local(t), None);
                }
            }
        }
    }
}

pub fn side_weights(node: &Node) -> Option<[f64; 4]> {
    let boxy = matches!(
        node.kind,
        NodeKind::Rectangle
            | NodeKind::Frame
            | NodeKind::Component { .. }
            | NodeKind::Instance { .. }
    );
    node.stroke.sides.filter(|_| boxy)
}

pub fn instance_child_rects(doc: &Document, main: &Node, w: f64, h: f64) -> Vec<(NodeId, Rect)> {
    let (mw, mh) = (main.rect.width, main.rect.height);
    if (mw - w).abs() < 1e-9 && (mh - h).abs() < 1e-9 {
        return Vec::new();
    }
    let kids: Vec<&Node> = main.children.iter().filter_map(|c| doc.get(*c)).collect();
    if let Some(layout) = main.layout {
        let mut fixed = layout;
        fixed.primary_sizing = deesainer_doc::Sizing::Fixed;
        fixed.counter_sizing = deesainer_doc::Sizing::Fixed;
        let children: Vec<deesainer_algo::Child> = kids
            .iter()
            .map(|c| deesainer_algo::Child {
                width: c.rect.width,
                height: c.rect.height,
                horizontal: c.layout_child.horizontal,
                vertical: c.layout_child.vertical,
                hug_width: c.rect.width,
                hug_height: c.rect.height,
                skip: !c.visible || c.layout_child.absolute,
                limits: c.size_limits,
            })
            .collect();
        let sol = deesainer_algo::solve(&fixed, w, h, &children, &|i, _| {
            kids.get(i).map_or(0.0, |c| c.rect.height)
        });
        return kids
            .iter()
            .zip(sol.rects)
            .filter_map(|(c, r)| r.map(|r| (c.id, r)))
            .collect();
    }
    kids.iter()
        .map(|c| {
            let (x, cw) = c
                .constraints
                .horizontal
                .solve(c.rect.x, c.rect.width, mw, w);
            let (y, ch) = c.constraints.vertical.solve(c.rect.y, c.rect.height, mh, h);
            (c.id, Rect::new(x, y, cw, ch))
        })
        .collect()
}

fn tint(pix: &mut Pixmap, color: Rgba) {
    let [r, g, b, a] = color.to_rgba8();
    for px in pix.data_mut().as_chunks_mut::<4>().0 {
        let alpha = u32::from(px[3]) * u32::from(a) / 255;
        px[0] = (u32::from(r) * alpha / 255) as u8;
        px[1] = (u32::from(g) * alpha / 255) as u8;
        px[2] = (u32::from(b) * alpha / 255) as u8;
        px[3] = alpha as u8;
    }
}

fn check_size(w: u32, h: u32) -> Result<(), RenderError> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(RenderError::Size(w, h));
    }
    Ok(())
}

pub fn page_background(doc: &Document, page: NodeId) -> Rgba {
    doc.get(page)
        .and_then(|p| {
            p.fills
                .iter()
                .find(|f| f.visible)
                .and_then(Paint::primary_color)
        })
        .unwrap_or(Rgba::WHITE)
}

pub fn paint_pixmap(paint: &Paint, w: f64, h: f64, scale: f64) -> Option<Pixmap> {
    let (pw, ph) = ((w * scale).ceil(), (h * scale).ceil());
    if !(pw >= 1.0 && ph >= 1.0) || pw * ph > MAX_PIXELS as f64 {
        return None;
    }
    let mut s = Surface::new(IRect {
        x0: 0,
        y0: 0,
        x1: pw as i32,
        y1: ph as i32,
    })?;
    let doc = Document::new();
    let node = Node::new(
        NodeId(0),
        "",
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, w, h),
    );
    let path = sk::PathBuilder::from_rect(sk::Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?);
    let mut r = Renderer::new(&doc, Options::default());
    r.fill(
        &mut s,
        &path,
        sk::FillRule::Winding,
        &Affine::scale(scale, scale),
        paint,
        &node,
    );
    Some(s.pix)
}

pub fn render_page(
    doc: &Document,
    page: NodeId,
    vp: &Viewport,
    opts: Options<'_>,
) -> Result<(Pixmap, Stats), RenderError> {
    check_size(vp.width, vp.height)?;
    let page_node = doc.get(page).ok_or(RenderError::UnknownNode(page))?;
    let mut s = Surface::new(IRect {
        x0: 0,
        y0: 0,
        x1: vp.width as i32,
        y1: vp.height as i32,
    })
    .ok_or(RenderError::Size(vp.width, vp.height))?;
    if opts.background {
        s.pix.fill(convert::color(page_background(doc, page), 1.0));
    }
    let mut r = Renderer::new(doc, opts);
    let children = page_node.children.clone();
    r.draw_child_list(&mut s, &children, &vp.transform, 0);
    Ok((s.pix, r.stats))
}

pub fn render_bounds(doc: &Document, id: NodeId) -> Option<Rect> {
    let node = doc.get(id)?;
    let t = doc.world_transform(id);
    let local = Rect::new(0.0, 0.0, node.rect.width, node.rect.height)
        .inflate(Renderer::outset(node) - 1.0);
    let mut b = t.transform_rect_bounds(&local);
    let clips = node.clips_content && node.kind.is_frame_like();
    if !clips && !matches!(node.kind, NodeKind::BooleanOp { .. }) {
        for c in &node.children {
            if doc.get(*c).is_some_and(|n| n.visible) {
                if let Some(cb) = render_bounds(doc, *c) {
                    b = b.union(cb);
                }
            }
        }
    }
    Some(b)
}

pub fn render_node(
    doc: &Document,
    id: NodeId,
    scale: f64,
    opts: Options<'_>,
) -> Result<(Pixmap, Rect), RenderError> {
    let node = doc.get(id).ok_or(RenderError::UnknownNode(id))?;
    let bounds = render_bounds(doc, id).ok_or(RenderError::UnknownNode(id))?;
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let w = (bounds.width * scale).ceil().max(1.0);
    let h = (bounds.height * scale).ceil().max(1.0);
    if !(w.is_finite() && h.is_finite()) || w > 1.0e6 || h > 1.0e6 {
        return Err(RenderError::Size(u32::MAX, u32::MAX));
    }
    let (w, h) = (w as u32, h as u32);
    check_size(w, h)?;
    let mut s = Surface::new(IRect {
        x0: 0,
        y0: 0,
        x1: w as i32,
        y1: h as i32,
    })
    .ok_or(RenderError::Size(w, h))?;
    let parent_t = node
        .parent
        .map_or(Affine::IDENTITY, |p| doc.world_transform(p));
    let to_pixels = Affine::scale(scale, scale).then(Affine::translate(-bounds.x, -bounds.y));
    let mut r = Renderer::new(doc, opts);
    if matches!(node.kind, NodeKind::Page) {
        if opts.background {
            s.pix.fill(convert::color(page_background(doc, id), 1.0));
        }
        let children = node.children.clone();
        r.draw_child_list(&mut s, &children, &to_pixels, 0);
    } else {
        r.draw_node(&mut s, id, &to_pixels.then(parent_t), 1);
    }
    Ok((s.pix, bounds))
}

pub fn encode_png(pix: &Pixmap) -> Result<Vec<u8>, RenderError> {
    pix.encode_png()
        .map_err(|e| RenderError::Encode(e.to_string()))
}

pub fn pixel_rgba(pix: &Pixmap, x: u32, y: u32) -> Option<[u8; 4]> {
    let p = pix.pixel(x, y)?.demultiply();
    Some([p.red(), p.green(), p.blue(), p.alpha()])
}

pub fn stroke_outline(
    path: &VectorPath,
    style: &deesainer_doc::StrokeStyle,
    width: f64,
) -> Option<VectorPath> {
    let sk_path = convert::path(path)?;
    let stroked = sk_path.stroke(&convert::stroke(style, width), 1.0)?;
    Some(from_sk_path(&stroked))
}

pub fn from_sk_path(p: &sk::Path) -> VectorPath {
    use deesainer_doc::PathCmd;
    let pt = |q: sk::Point| Point::new(f64::from(q.x), f64::from(q.y));
    let mut cmds = Vec::new();
    for seg in p.segments() {
        cmds.push(match seg {
            sk::PathSegment::MoveTo(a) => PathCmd::MoveTo { to: pt(a) },
            sk::PathSegment::LineTo(a) => PathCmd::LineTo { to: pt(a) },
            sk::PathSegment::QuadTo(c, a) => PathCmd::QuadTo {
                ctrl: pt(c),
                to: pt(a),
            },
            sk::PathSegment::CubicTo(c1, c2, a) => PathCmd::CubicTo {
                ctrl1: pt(c1),
                ctrl2: pt(c2),
                to: pt(a),
            },
            sk::PathSegment::Close => PathCmd::Close,
        });
    }
    VectorPath {
        cmds,
        fill_rule: deesainer_doc::FillRule::NonZero,
    }
}
