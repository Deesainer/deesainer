use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{
    AssetId, BoolOp, ColorStop, Document, Effect, FillRule, ImageScaleMode, Node, NodeId, NodeKind,
    Paint, PaintKind, PathCmd, PropValue, StrokeAlign, StrokeCap, StrokeJoin, VectorPath,
};
use deesainer_geom::{Affine, Point, Rect};
use deesainer_render::{instance_child_rects, overridden, TextPainter, MAX_INSTANCE_DEPTH};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};

const BATCH_VERTICES: usize = 60_000;
const TOLERANCE_PX: f64 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextureRef {
    Image {
        asset: AssetId,
        repeat: bool,
        adjust: u64,
    },
    Gradient(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pixels {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

const GRADIENT_TEXELS: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub clip: Option<[f32; 4]>,
    pub texture: Option<TextureRef>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub bounds: [f32; 4],
}

impl Default for Batch {
    fn default() -> Self {
        Self {
            clip: None,
            texture: None,
            vertices: Vec::new(),
            indices: Vec::new(),
            bounds: [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub opacity: f32,
    pub blend: BlendMode,
    pub bounds: Option<[f32; 4]>,
    pub masked: bool,
    pub blur: f32,
    pub backdrop: bool,
    pub tint: Option<[f32; 4]>,
    pub invert: bool,
    pub mask_outside: bool,
}

impl Layer {
    pub fn plain(opacity: f32, blend: BlendMode, bounds: Option<[f32; 4]>) -> Self {
        Self {
            opacity,
            blend,
            bounds,
            masked: false,
            blur: 0.0,
            backdrop: false,
            tint: None,
            invert: false,
            mask_outside: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Batch(usize),
    Begin(Layer),
    MaskEnd,
    End,
}

pub const MAX_LAYER_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMesh {
    pub batches: Vec<Batch>,
    pub items: Vec<Item>,
    pub region: Rect,
    pub scale: f64,
    pub background: Option<[u8; 4]>,
    pub triangles: usize,
    pub textures: BTreeSet<TextureRef>,
    pub gradients: BTreeMap<u64, Pixels>,
    pub adjustments: BTreeMap<u64, deesainer_doc::ImageAdjust>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported(pub String);

impl SceneMesh {
    pub fn has_layers(&self) -> bool {
        self.items.iter().any(|i| matches!(i, Item::Begin(_)))
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

type Result<T> = std::result::Result<T, Unsupported>;

fn no<T>(why: impl Into<String>) -> Result<T> {
    Err(Unsupported(why.into()))
}

enum Colorer {
    Solid([f32; 4]),
    Linear {
        start: (f64, f64),
        axis: (f64, f64),
        len2: f64,
        from: [f32; 4],
        to: [f32; 4],
    },
    Ramp {
        texture: TextureRef,
        tint: [f32; 4],
        start: (f64, f64),
        axis: (f64, f64),
        len2: f64,
    },
    Radial {
        texture: TextureRef,
        tint: [f32; 4],
        center: (f64, f64),
        radius: (f64, f64),
    },
    BoxMap {
        texture: TextureRef,
        tint: [f32; 4],
        size: (f64, f64),
    },
    Image {
        texture: TextureRef,
        tint: [f32; 4],
        origin: (f64, f64),
        size: (f64, f64),
    },
}

fn premul(c: Rgba, alpha: f32) -> [f32; 4] {
    let a = (c.a * alpha).clamp(0.0, 1.0);
    [c.r, c.g, c.b, a]
}

fn lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn stop_color(stops: &[ColorStop], t: f64) -> [f32; 4] {
    let t = t as f32;
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return [0.0; 4];
    };
    if t <= first.position {
        return premul(first.color, 1.0);
    }
    for pair in stops.windows(2) {
        let [a, b] = pair else { continue };
        if t <= b.position {
            let span = b.position - a.position;
            let k = if span > 1e-9 {
                (t - a.position) / span
            } else {
                1.0
            };
            return lerp(premul(a.color, 1.0), premul(b.color, 1.0), k);
        }
    }
    premul(last.color, 1.0)
}

fn sorted_stops(stops: &[ColorStop]) -> Vec<ColorStop> {
    let mut v: Vec<ColorStop> = stops
        .iter()
        .filter(|s| s.position.is_finite())
        .map(|s| ColorStop {
            position: s.position.clamp(0.0, 1.0),
            color: s.color,
        })
        .collect();
    v.sort_by(|a, b| a.position.total_cmp(&b.position));
    v
}

fn gradient_key(kind: u8, extra: &[f64], stops: &[ColorStop]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut h);
    for v in extra {
        v.to_bits().hash(&mut h);
    }
    for s in stops {
        s.position.to_bits().hash(&mut h);
        for c in [s.color.r, s.color.g, s.color.b, s.color.a] {
            c.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

fn ramp_pixels(stops: &[ColorStop]) -> Pixels {
    let n = GRADIENT_TEXELS;
    let rgba = (0..n)
        .flat_map(|i| pack(stop_color(stops, i as f64 / (n - 1) as f64)))
        .collect();
    Pixels { size: [n, 1], rgba }
}

fn radial_pixels(stops: &[ColorStop]) -> Pixels {
    let n = GRADIENT_TEXELS;
    let at = |i: usize| -1.0 + (i as f64 + 0.5) * 2.0 / n as f64;
    let mut rgba = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            rgba.extend(pack(stop_color(stops, at(x).hypot(at(y)))));
        }
    }
    Pixels { size: [n, n], rgba }
}

fn diamond_pixels(stops: &[ColorStop]) -> Pixels {
    let n = GRADIENT_TEXELS;
    let at = |i: usize| -1.0 + (i as f64 + 0.5) * 2.0 / n as f64;
    let mut rgba = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            rgba.extend(pack(stop_color(stops, at(x).abs() + at(y).abs())));
        }
    }
    Pixels { size: [n, n], rgba }
}

fn angular_pixels(stops: &[ColorStop], center: (f64, f64), angle: f64, size: (f64, f64)) -> Pixels {
    let n = GRADIENT_TEXELS;
    let mut rgba = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let px = (x as f64 + 0.5) / n as f64 * size.0 - center.0;
            let py = (y as f64 + 0.5) / n as f64 * size.1 - center.1;
            let turn = (py.atan2(px).to_degrees() - angle).rem_euclid(360.0) / 360.0;
            rgba.extend(pack(stop_color(stops, turn)));
        }
    }
    Pixels { size: [n, n], rgba }
}

fn pack(c: [f32; 4]) -> [u8; 4] {
    let a = c[3].clamp(0.0, 1.0);
    let q = |v: f32| (v.clamp(0.0, 1.0) * a * 255.0 + 0.5) as u8;
    [q(c[0]), q(c[1]), q(c[2]), (a * 255.0 + 0.5) as u8]
}

impl Colorer {
    fn new(paint: &Paint, node_opacity: f32, size: (f64, f64)) -> Result<Self> {
        if !matches!(paint.blend, BlendMode::Normal | BlendMode::PassThrough) {
            return no("a paint with a blend mode");
        }
        let alpha = paint.opacity * node_opacity;
        match &paint.kind {
            PaintKind::Solid { color } => Ok(Colorer::Solid(premul(*color, alpha))),
            PaintKind::LinearGradient { start, end, stops } => {
                let [a, b] = stops.as_slice() else {
                    return no("a gradient with other than two stops");
                };
                if a.position != 0.0 || b.position != 1.0 {
                    return no("a gradient whose stops are not at 0 and 1");
                }
                let s = (start.x * size.0, start.y * size.1);
                let axis = (end.x * size.0 - s.0, end.y * size.1 - s.1);
                Ok(Colorer::Linear {
                    start: s,
                    axis,
                    len2: (axis.0 * axis.0 + axis.1 * axis.1).max(1e-12),
                    from: premul(a.color, alpha),
                    to: premul(b.color, alpha),
                })
            }
            PaintKind::RadialGradient { .. } => no("a radial gradient"),
            PaintKind::AngularGradient { .. } | PaintKind::DiamondGradient { .. } => {
                no("an angular or diamond gradient without its texture")
            }
            PaintKind::Image { .. } => no("an image paint without its image"),
        }
    }

    fn image(doc: &Document, paint: &Paint, node_opacity: f32, size: (f64, f64)) -> Option<Self> {
        let PaintKind::Image {
            asset,
            scale_mode,
            adjust,
        } = &paint.kind
        else {
            return None;
        };
        let img = deesainer_render::images::get(doc, asset)?;
        let (iw, ih) = (f64::from(img.width()), f64::from(img.height()));
        let (w, h) = size;
        let (origin, shown) = match scale_mode {
            ImageScaleMode::Stretch => ((0.0, 0.0), (w, h)),
            ImageScaleMode::Fill | ImageScaleMode::Fit => {
                let k = if matches!(scale_mode, ImageScaleMode::Fill) {
                    (w / iw).max(h / ih)
                } else {
                    (w / iw).min(h / ih)
                };
                (((w - iw * k) / 2.0, (h - ih * k) / 2.0), (iw * k, ih * k))
            }
            ImageScaleMode::Tile => ((0.0, 0.0), (iw, ih)),
        };
        if !(shown.0 > 0.0 && shown.1 > 0.0 && shown.0.is_finite() && shown.1.is_finite()) {
            return None;
        }
        Some(Colorer::Image {
            texture: TextureRef::Image {
                asset: asset.clone(),
                repeat: matches!(scale_mode, ImageScaleMode::Tile),
                adjust: adjust.key(),
            },
            tint: [
                1.0,
                1.0,
                1.0,
                (paint.opacity * node_opacity).clamp(0.0, 1.0),
            ],
            origin,
            size: shown,
        })
    }

    fn gradient(
        paint: &Paint,
        node_opacity: f32,
        size: (f64, f64),
        gradients: &mut BTreeMap<u64, Pixels>,
    ) -> Option<Self> {
        let tint = [
            1.0,
            1.0,
            1.0,
            (paint.opacity * node_opacity).clamp(0.0, 1.0),
        ];
        match &paint.kind {
            PaintKind::LinearGradient { start, end, stops } => {
                let exact =
                    matches!(stops.as_slice(), [a, b] if a.position == 0.0 && b.position == 1.0);
                if exact {
                    return None;
                }
                let stops = sorted_stops(stops);
                let key = gradient_key(0, &[], &stops);
                gradients.entry(key).or_insert_with(|| ramp_pixels(&stops));
                let s = (start.x * size.0, start.y * size.1);
                let axis = (end.x * size.0 - s.0, end.y * size.1 - s.1);
                Some(Colorer::Ramp {
                    texture: TextureRef::Gradient(key),
                    tint,
                    start: s,
                    axis,
                    len2: (axis.0 * axis.0 + axis.1 * axis.1).max(1e-12),
                })
            }
            PaintKind::RadialGradient {
                center,
                radius,
                stops,
            } => {
                let stops = sorted_stops(stops);
                let key = gradient_key(1, &[], &stops);
                gradients
                    .entry(key)
                    .or_insert_with(|| radial_pixels(&stops));
                Some(Colorer::Radial {
                    texture: TextureRef::Gradient(key),
                    tint,
                    center: (center.x * size.0, center.y * size.1),
                    radius: ((radius.x * size.0).max(1e-3), (radius.y * size.1).max(1e-3)),
                })
            }
            PaintKind::DiamondGradient {
                center,
                radius,
                stops,
            } => {
                let stops = sorted_stops(stops);
                let key = gradient_key(2, &[], &stops);
                gradients
                    .entry(key)
                    .or_insert_with(|| diamond_pixels(&stops));
                Some(Colorer::Radial {
                    texture: TextureRef::Gradient(key),
                    tint,
                    center: (center.x * size.0, center.y * size.1),
                    radius: (
                        (radius.x * size.0).abs().max(1e-3),
                        (radius.y * size.1).abs().max(1e-3),
                    ),
                })
            }
            PaintKind::AngularGradient {
                center,
                angle,
                stops,
            } => {
                let stops = sorted_stops(stops);
                let c = (center.x * size.0, center.y * size.1);
                let key = gradient_key(
                    3,
                    &[center.x, center.y, *angle, size.0 / size.1.max(1e-9)],
                    &stops,
                );
                gradients
                    .entry(key)
                    .or_insert_with(|| angular_pixels(&stops, c, *angle, size));
                Some(Colorer::BoxMap {
                    texture: TextureRef::Gradient(key),
                    tint,
                    size,
                })
            }
            _ => None,
        }
    }

    fn texture(&self) -> Option<&TextureRef> {
        match self {
            Colorer::Image { texture, .. }
            | Colorer::Ramp { texture, .. }
            | Colorer::Radial { texture, .. }
            | Colorer::BoxMap { texture, .. } => Some(texture),
            _ => None,
        }
    }

    fn uv(&self, p: (f64, f64)) -> [f32; 2] {
        let n = GRADIENT_TEXELS as f64;
        match self {
            Colorer::Ramp {
                start, axis, len2, ..
            } => {
                let t = ((p.0 - start.0) * axis.0 + (p.1 - start.1) * axis.1) / len2;
                [((t * (n - 1.0) + 0.5) / n) as f32, 0.5]
            }
            Colorer::BoxMap { size, .. } => [
                (p.0 / size.0.max(1e-9)) as f32,
                (p.1 / size.1.max(1e-9)) as f32,
            ],
            Colorer::Radial { center, radius, .. } => [
                (((p.0 - center.0) / radius.0 + 1.0) / 2.0) as f32,
                (((p.1 - center.1) / radius.1 + 1.0) / 2.0) as f32,
            ],
            Colorer::Image { origin, size, .. } => [
                ((p.0 - origin.0) / size.0) as f32,
                ((p.1 - origin.1) / size.1) as f32,
            ],
            _ => [0.0, 0.0],
        }
    }

    fn at(&self, p: (f64, f64)) -> [u8; 4] {
        match self {
            Colorer::Solid(c)
            | Colorer::Image { tint: c, .. }
            | Colorer::Ramp { tint: c, .. }
            | Colorer::Radial { tint: c, .. }
            | Colorer::BoxMap { tint: c, .. } => pack(*c),
            Colorer::Linear {
                start,
                axis,
                len2,
                from,
                to,
            } => {
                let t = (((p.0 - start.0) * axis.0 + (p.1 - start.1) * axis.1) / len2)
                    .clamp(0.0, 1.0) as f32;
                pack(lerp(*from, *to, t))
            }
        }
    }
}

fn to_lyon(path: &VectorPath) -> Path {
    let p = |q: Point| point(q.x as f32, q.y as f32);
    let mut b = Path::builder();
    let mut open = false;
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo { to } => {
                if open {
                    b.end(false);
                }
                b.begin(p(to));
                open = true;
            }
            PathCmd::LineTo { to } if open => {
                b.line_to(p(to));
            }
            PathCmd::QuadTo { ctrl, to } if open => {
                b.quadratic_bezier_to(p(ctrl), p(to));
            }
            PathCmd::CubicTo { ctrl1, ctrl2, to } if open => {
                b.cubic_bezier_to(p(ctrl1), p(ctrl2), p(to));
            }
            PathCmd::Close if open => {
                b.end(true);
                open = false;
            }
            _ => {}
        }
    }
    if open {
        b.end(false);
    }
    b.build()
}

struct Builder<'a> {
    doc: &'a Document,
    text: Option<&'a dyn TextPainter>,
    overrides: Vec<BTreeMap<NodeId, Vec<PropValue>>>,
    slots: Vec<BTreeMap<NodeId, NodeId>>,
    region: Rect,
    scale: f64,
    batches: Vec<Batch>,
    items: Vec<Item>,
    layer_depth: usize,
    sealed: bool,
    node_bounds: [f32; 4],
    triangles: usize,
    textures: BTreeSet<TextureRef>,
    gradients: BTreeMap<u64, Pixels>,
    adjustments: BTreeMap<u64, deesainer_doc::ImageAdjust>,
    fill: FillTessellator,
    stroke: StrokeTessellator,
}

type Clip = Option<[f32; 4]>;

fn intersect(a: Clip, b: [f32; 4]) -> [f32; 4] {
    match a {
        None => b,
        Some(a) => [
            a[0].max(b[0]),
            a[1].max(b[1]),
            a[2].min(b[2]),
            a[3].min(b[3]),
        ],
    }
}

impl<'a> Builder<'a> {
    fn resolved(&self, id: NodeId) -> Option<Cow<'a, Node>> {
        overridden(self.doc, &self.overrides, id)
    }

    fn batch(&mut self, clip: Clip, texture: Option<&TextureRef>) -> &mut Batch {
        let reuse = !self.sealed
            && self.batches.last().is_some_and(|b| {
                b.clip == clip && b.texture.as_ref() == texture && b.vertices.len() < BATCH_VERTICES
            });
        if !reuse {
            self.items.push(Item::Batch(self.batches.len()));
            self.batches.push(Batch {
                clip,
                texture: texture.cloned(),
                ..Batch::default()
            });
            self.sealed = false;
        }
        let last = self.batches.len() - 1;
        &mut self.batches[last]
    }

    fn begin(&mut self, layer: Layer) -> Result<()> {
        if self.layer_depth >= MAX_LAYER_DEPTH {
            return no("layers nested deeper than 32");
        }
        self.layer_depth += 1;
        self.items.push(Item::Begin(layer));
        self.sealed = true;
        Ok(())
    }

    fn mask_end(&mut self) {
        self.items.push(Item::MaskEnd);
        self.sealed = true;
    }

    fn end(&mut self) {
        self.layer_depth = self.layer_depth.saturating_sub(1);
        self.items.push(Item::End);
        self.sealed = true;
    }

    fn tolerance(&self, t: &Affine) -> f32 {
        let s = t.determinant().abs().sqrt().max(1e-9) * self.scale;
        (TOLERANCE_PX / s) as f32
    }

    fn push(&mut self, clip: Clip, texture: Option<&TextureRef>, buf: VertexBuffers<Vertex, u32>) {
        self.triangles += buf.indices.len() / 3;
        if let Some(t) = texture {
            self.textures.insert(t.clone());
        }
        let batch = self.batch(clip, texture);
        let base = batch.vertices.len() as u32;
        for v in &buf.vertices {
            batch.bounds = [
                batch.bounds[0].min(v.pos[0]),
                batch.bounds[1].min(v.pos[1]),
                batch.bounds[2].max(v.pos[0]),
                batch.bounds[3].max(v.pos[1]),
            ];
        }
        batch.vertices.extend(buf.vertices);
        batch
            .indices
            .extend(buf.indices.into_iter().map(|i| i + base));
    }

    fn fill_path(
        &mut self,
        path: &VectorPath,
        t: &Affine,
        colorer: &Colorer,
        clip: Clip,
    ) -> Result<()> {
        let lyon = to_lyon(path);
        let rule = match path.fill_rule {
            FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
            FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
        };
        let options = FillOptions::tolerance(self.tolerance(t)).with_fill_rule(rule);
        let mut buf: VertexBuffers<Vertex, u32> = VertexBuffers::new();
        let make = |v: FillVertex<'_>| {
            let p = v.position();
            let local = (f64::from(p.x), f64::from(p.y));
            let q = t.apply(Point::new(local.0, local.1));
            Vertex {
                pos: [q.x as f32, q.y as f32],
                uv: colorer.uv(local),
                color: colorer.at(local),
            }
        };
        self.fill
            .tessellate_path(&lyon, &options, &mut BuffersBuilder::new(&mut buf, make))
            .map_err(|e| Unsupported(format!("a shape that cannot be tessellated ({e})")))?;
        self.push(clip, colorer.texture(), buf);
        Ok(())
    }

    fn stroke_path(
        &mut self,
        path: &VectorPath,
        t: &Affine,
        node: &Node,
        width: f64,
        colorer: &Colorer,
        clip: Clip,
    ) -> Result<()> {
        let lyon = to_lyon(path);
        let cap = match node.stroke.cap {
            StrokeCap::None => LineCap::Butt,
            StrokeCap::Round => LineCap::Round,
            StrokeCap::Square => LineCap::Square,
        };
        let join = match node.stroke.join {
            StrokeJoin::Miter => LineJoin::Miter,
            StrokeJoin::Round => LineJoin::Round,
            StrokeJoin::Bevel => LineJoin::Bevel,
        };
        let options = StrokeOptions::tolerance(self.tolerance(t))
            .with_line_width(width as f32)
            .with_line_cap(cap)
            .with_line_join(join)
            .with_miter_limit(4.0);
        let mut buf: VertexBuffers<Vertex, u32> = VertexBuffers::new();
        let make = |v: StrokeVertex<'_, '_>| {
            let p = v.position();
            let local = (f64::from(p.x), f64::from(p.y));
            let q = t.apply(Point::new(local.0, local.1));
            Vertex {
                pos: [q.x as f32, q.y as f32],
                uv: colorer.uv(local),
                color: colorer.at(local),
            }
        };
        self.stroke
            .tessellate_path(&lyon, &options, &mut BuffersBuilder::new(&mut buf, make))
            .map_err(|e| Unsupported(format!("a stroke that cannot be tessellated ({e})")))?;
        self.push(clip, None, buf);
        Ok(())
    }

    fn node(&mut self, id: NodeId, parent_t: &Affine, clip: Clip, depth: usize) -> Result<()> {
        if depth > deesainer_doc::MAX_DEPTH {
            return Ok(());
        }
        let Some(node) = self.resolved(id) else {
            return Ok(());
        };
        if !node.visible {
            return Ok(());
        }
        let t = parent_t.then(Document::local_transform(&node));
        let (w, h) = (node.rect.width, node.rect.height);
        let is_container = (node.kind.is_container()
            || matches!(node.kind, NodeKind::Instance { .. }))
            && !matches!(node.kind, NodeKind::BooleanOp { .. });
        let clips = node.clips_content && node.kind.is_frame_like();
        let reach = node.stroke.weight.max(0.0) * 2.0 + 2.0;
        let bounds = t
            .transform_rect_bounds(&Rect::new(0.0, 0.0, w, h))
            .inflate(reach);
        let k = t.determinant().abs().sqrt();
        let effect_reach = node
            .effects
            .iter()
            .filter(|e| e.is_visible())
            .map(|e| match e {
                Effect::DropShadow {
                    offset,
                    blur,
                    spread,
                    ..
                } => (blur * 2.0 + spread.max(0.0) + offset.x.abs().max(offset.y.abs())) * k,
                Effect::LayerBlur { radius, .. } => radius * 1.5 * k,
                _ => 0.0,
            })
            .fold(0.0, f64::max);
        if (!is_container || clips) && !self.region.intersects(&bounds.inflate(effect_reach)) {
            return Ok(());
        }
        self.content(&node, &t, clip, depth, true)
    }

    fn content(
        &mut self,
        node: &Node,
        t: &Affine,
        clip: Clip,
        depth: usize,
        full: bool,
    ) -> Result<()> {
        let t = *t;
        let (w, h) = (node.rect.width, node.rect.height);
        let is_container = (node.kind.is_container()
            || matches!(node.kind, NodeKind::Instance { .. }))
            && !matches!(node.kind, NodeKind::BooleanOp { .. });
        let clips = node.clips_content && node.kind.is_frame_like();
        let reach = node.stroke.weight.max(0.0) * 2.0 + 2.0;
        let bounds = t
            .transform_rect_bounds(&Rect::new(0.0, 0.0, w, h))
            .inflate(reach);
        let k = t.determinant().abs().sqrt();
        let effects: Vec<&Effect> = if full {
            node.effects.iter().filter(|e| e.is_visible()).collect()
        } else {
            Vec::new()
        };
        let layer_blur = effects
            .iter()
            .find_map(|e| match e {
                Effect::LayerBlur { radius, .. } if *radius > 0.0 => Some((*radius * k) as f32),
                _ => None,
            })
            .unwrap_or(0.0);
        let has_children = match &node.kind {
            NodeKind::Instance { component, .. } => self
                .doc
                .get(*component)
                .is_some_and(|m| !m.children.is_empty()),
            _ => !node.children.is_empty() && !matches!(node.kind, NodeKind::BooleanOp { .. }),
        };
        let blends = !matches!(node.blend, BlendMode::PassThrough | BlendMode::Normal);
        let layered = full && (blends || (node.opacity < 1.0 && has_children) || layer_blur > 0.0);
        let mut inner_clip = clip;
        let mut clip_mask = false;
        if clips {
            let r = &node.corner_radius;
            let rounded = r.top_left != 0.0
                || r.top_right != 0.0
                || r.bottom_left != 0.0
                || r.bottom_right != 0.0;
            let rotated = t.b.abs() > 1e-9 || t.c.abs() > 1e-9;
            let b = t.transform_rect_bounds(&Rect::new(0.0, 0.0, w, h));
            let scissor = [b.x as f32, b.y as f32, b.max_x() as f32, b.max_y() as f32];
            inner_clip = Some(intersect(clip, scissor));
            clip_mask = rounded || rotated;
        }
        let page_box = [
            bounds.x as f32,
            bounds.y as f32,
            bounds.max_x() as f32,
            bounds.max_y() as f32,
        ];
        let effect_box = {
            let pad = f64::from(layer_blur) * 1.5 + 2.0;
            [
                page_box[0] - pad as f32,
                page_box[1] - pad as f32,
                page_box[2] + pad as f32,
                page_box[3] + pad as f32,
            ]
        };
        if layered {
            self.begin(Layer {
                blur: layer_blur,
                ..Layer::plain(
                    node.opacity.clamp(0.0, 1.0),
                    node.blend,
                    if is_container && !clips && layer_blur == 0.0 {
                        None
                    } else {
                        Some(effect_box)
                    },
                )
            })?;
        }
        let opacity = if layered || !full { 1.0 } else { node.opacity };
        let outline = deesainer_vector::resolved_outline(self.doc, node);
        for e in &effects {
            if let Effect::DropShadow {
                color,
                offset,
                blur,
                spread,
                ..
            } = e
            {
                self.drop_shadow(
                    node, &t, clip, depth, *color, *offset, *blur, *spread, page_box,
                )?;
            }
        }
        for e in &effects {
            if let (Effect::BackgroundBlur { radius, .. }, Some(path)) = (e, &outline) {
                if *radius > 0.0 {
                    self.background_blur(path, &t, clip, (*radius * k) as f32)?;
                }
            }
        }
        self.node_bounds = page_box;
        if let Some(path) = &outline {
            for (part, paints) in node.fill_parts(path) {
                for paint in paints.iter().filter(|p| p.visible) {
                    self.paint(&part, &t, paint, opacity, (w, h), clip)?;
                }
            }
        } else if matches!(node.kind, NodeKind::Text(_)) {
            self.text(node, &t, clip, (w, h))?;
        }
        if let Some(path) = &outline {
            for e in &effects {
                if let Effect::InnerShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    ..
                } = e
                {
                    let b = t.transform_rect_bounds(&Rect::new(0.0, 0.0, w, h));
                    let node_box = [b.x as f32, b.y as f32, b.max_x() as f32, b.max_y() as f32];
                    self.inner_shadow(path, &t, clip, *color, *offset, *blur, *spread, node_box)?;
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
                    let drawn = self.clipped_children(
                        &kids,
                        &t,
                        inner_clip,
                        depth,
                        clip_mask.then_some(outline.as_ref()).flatten(),
                        page_box,
                    );
                    self.slots.pop();
                    self.overrides.pop();
                    drawn?;
                    Vec::new()
                } else {
                    Vec::new()
                }
            }
            _ => node.children.clone(),
        };
        if !children.is_empty() {
            self.clipped_children(
                &children,
                &t,
                inner_clip,
                depth,
                clip_mask.then_some(outline.as_ref()).flatten(),
                page_box,
            )?;
        }
        let strokes: Vec<&Paint> = node.strokes.iter().filter(|p| p.visible).collect();
        let sides = deesainer_render::side_weights(node);
        if !strokes.is_empty() && (node.stroke.weight > 0.0 || sides.is_some()) {
            let Some(path) = &outline else {
                if matches!(node.kind, NodeKind::Text(_)) {
                    self.text_strokes(node, &t, clip, &strokes, opacity)?;
                }
                if layered {
                    self.end();
                }
                return Ok(());
            };
            let dash = dash_pattern(&node.stroke.dash);
            let open = matches!(node.kind, NodeKind::Line)
                || !path.cmds.iter().any(|c| matches!(c, PathCmd::Close));
            let dashed_ring = sides.is_none()
                && !open
                && node.stroke.align != StrokeAlign::Center
                && dash.is_some();
            let ring = if let Some(sides) = sides {
                Some(deesainer_vector::side_stroke_ring(
                    node.rect.width,
                    node.rect.height,
                    &node.corner_radius,
                    sides,
                    node.stroke.align,
                ))
            } else if open || node.stroke.align == StrokeAlign::Center || dashed_ring {
                None
            } else {
                let tolerance = f64::from(self.tolerance(&t));
                Some(deesainer_vector::boolean::stroke_ring(
                    &deesainer_vector::flatten(path, tolerance),
                    node.stroke.weight,
                    node.stroke.align == StrokeAlign::Outside,
                    node.stroke.join,
                    path.fill_rule,
                ))
            };
            let dashed = match &dash {
                Some(pattern) => Some(dash_path(path, pattern, f64::from(self.tolerance(&t)))?),
                None => None,
            };
            let line = dashed.as_ref().unwrap_or(path);
            let width = if dashed_ring {
                node.stroke.weight * 2.0
            } else {
                node.stroke.weight
            };
            if dashed_ring {
                self.begin(Layer {
                    masked: true,
                    mask_outside: node.stroke.align == StrokeAlign::Outside,
                    ..Layer::plain(1.0, BlendMode::Normal, Some(page_box))
                })?;
                self.fill_path(path, &t, &Colorer::Solid([1.0; 4]), clip)?;
                self.mask_end();
            }
            for paint in strokes {
                let solid = match &paint.kind {
                    PaintKind::Solid { .. } => paint.clone(),
                    PaintKind::Image { .. } => continue,
                    _ => match paint.primary_color() {
                        Some(color) => Paint {
                            kind: PaintKind::Solid { color },
                            ..paint.clone()
                        },
                        None => continue,
                    },
                };
                let blended = !matches!(solid.blend, BlendMode::Normal | BlendMode::PassThrough);
                if blended {
                    self.begin(Layer::plain(1.0, solid.blend, Some(page_box)))?;
                }
                let solid = Paint {
                    blend: BlendMode::Normal,
                    ..solid
                };
                let colorer = Colorer::new(&solid, opacity, (w, h))?;
                match &ring {
                    Some(ring) => self.fill_path(ring, &t, &colorer, clip)?,
                    None => self.stroke_path(line, &t, node, width, &colorer, clip)?,
                }
                if blended {
                    self.end();
                }
            }
            if dashed_ring {
                self.end();
            }
        }
        if layered {
            self.end();
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn drop_shadow(
        &mut self,
        node: &Node,
        t: &Affine,
        clip: Clip,
        depth: usize,
        color: Rgba,
        offset: Point,
        blur: f64,
        spread: f64,
        page_box: [f32; 4],
    ) -> Result<()> {
        let k = t.determinant().abs().sqrt();
        let pad =
            ((blur * 2.0 + spread.max(0.0) + offset.x.abs().max(offset.y.abs())) * k + 2.0) as f32;
        self.begin(Layer {
            blur: (blur * k) as f32,
            tint: Some([color.r, color.g, color.b, color.a]),
            ..Layer::plain(
                1.0,
                BlendMode::Normal,
                Some([
                    page_box[0] - pad,
                    page_box[1] - pad,
                    page_box[2] + pad,
                    page_box[3] + pad,
                ]),
            )
        })?;
        let shifted = t.then(Affine::translate(offset.x, offset.y));
        self.content(node, &shifted, clip, depth, false)?;
        if spread > 0.0 {
            if let Some(path) = deesainer_vector::resolved_outline(self.doc, node) {
                let tolerance = f64::from(self.tolerance(&shifted));
                let grown = deesainer_vector::boolean::stroke_ring(
                    &deesainer_vector::flatten(&path, tolerance),
                    spread,
                    true,
                    StrokeJoin::Miter,
                    path.fill_rule,
                );
                self.fill_path(&grown, &shifted, &Colorer::Solid([1.0; 4]), clip)?;
            }
        }
        self.end();
        Ok(())
    }

    fn background_blur(
        &mut self,
        path: &VectorPath,
        t: &Affine,
        clip: Clip,
        radius: f32,
    ) -> Result<()> {
        let Some(b) = deesainer_vector::path_bounds(path) else {
            return Ok(());
        };
        let b = t.transform_rect_bounds(&b);
        self.begin(Layer {
            masked: true,
            backdrop: true,
            blur: radius,
            ..Layer::plain(
                1.0,
                BlendMode::Normal,
                Some([b.x as f32, b.y as f32, b.max_x() as f32, b.max_y() as f32]),
            )
        })?;
        self.fill_path(path, t, &Colorer::Solid([1.0; 4]), clip)?;
        self.mask_end();
        self.end();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn inner_shadow(
        &mut self,
        path: &VectorPath,
        t: &Affine,
        clip: Clip,
        color: Rgba,
        offset: Point,
        blur: f64,
        spread: f64,
        page_box: [f32; 4],
    ) -> Result<()> {
        let k = t.determinant().abs().sqrt();
        self.begin(Layer {
            masked: true,
            invert: true,
            blur: (blur * k) as f32,
            tint: Some([color.r, color.g, color.b, color.a]),
            ..Layer::plain(1.0, BlendMode::Normal, Some(page_box))
        })?;
        self.fill_path(path, t, &Colorer::Solid([1.0; 4]), clip)?;
        self.mask_end();
        let shifted = t.then(Affine::translate(offset.x, offset.y));
        let hole = if spread > 0.0 {
            let tolerance = f64::from(self.tolerance(&shifted));
            let polys = deesainer_vector::flatten(path, tolerance);
            let ring = deesainer_vector::boolean::stroke_ring(
                &polys,
                spread,
                false,
                StrokeJoin::Miter,
                path.fill_rule,
            );
            deesainer_vector::boolean::apply(
                BoolOp::Subtract,
                &[polys, deesainer_vector::flatten(&ring, tolerance)],
            )
        } else {
            path.clone()
        };
        self.fill_path(&hole, &shifted, &Colorer::Solid([1.0; 4]), clip)?;
        self.end();
        Ok(())
    }

    fn clipped_children(
        &mut self,
        ids: &[NodeId],
        t: &Affine,
        clip: Clip,
        depth: usize,
        clip_path: Option<&VectorPath>,
        bounds: [f32; 4],
    ) -> Result<()> {
        let Some(path) = clip_path else {
            return self.children(ids, t, clip, depth);
        };
        self.begin(Layer {
            masked: true,
            ..Layer::plain(1.0, BlendMode::Normal, Some(bounds))
        })?;
        self.fill_path(path, t, &Colorer::Solid([1.0, 1.0, 1.0, 1.0]), clip)?;
        self.mask_end();
        self.children(ids, t, clip, depth)?;
        self.end();
        Ok(())
    }

    fn children(&mut self, ids: &[NodeId], t: &Affine, clip: Clip, depth: usize) -> Result<()> {
        for (i, id) in ids.iter().enumerate() {
            let is_mask = self.resolved(*id).is_some_and(|n| n.is_mask && n.visible);
            if !is_mask {
                self.node(*id, t, clip, depth + 1)?;
                continue;
            }
            self.begin(Layer {
                masked: true,
                ..Layer::plain(1.0, BlendMode::Normal, None)
            })?;
            self.node(*id, t, clip, depth + 1)?;
            self.mask_end();
            for sibling in ids.get(i + 1..).unwrap_or_default() {
                self.node(*sibling, t, clip, depth + 1)?;
            }
            self.end();
            break;
        }
        Ok(())
    }

    fn text(&mut self, node: &Node, t: &Affine, clip: Clip, size: (f64, f64)) -> Result<()> {
        let Some(painter) = self.text else {
            return no("text without a text painter");
        };
        let (glyphs, colored) = painter.parts(node);
        let mut base = VectorPath::default();
        for g in glyphs {
            base.cmds.extend(g.cmds);
        }
        for paint in node.fills.iter().filter(|p| p.visible) {
            self.paint(&base, t, paint, node.opacity, size, clip)?;
        }
        for (paint, path) in colored.iter().filter(|(p, _)| p.visible) {
            self.paint(path, t, paint, node.opacity, size, clip)?;
        }
        Ok(())
    }

    fn text_strokes(
        &mut self,
        node: &Node,
        t: &Affine,
        clip: Clip,
        strokes: &[&Paint],
        opacity: f32,
    ) -> Result<()> {
        let Some(painter) = self.text else {
            return no("text without a text painter");
        };
        let (glyphs, colored) = painter.parts(node);
        let mut all = VectorPath::default();
        for g in glyphs {
            all.cmds.extend(g.cmds);
        }
        for (_, p) in colored {
            all.cmds.extend(p.cmds);
        }
        let all = deesainer_vector::boolean::merged_outline(&all, f64::from(self.tolerance(t)));
        let line = match dash_pattern(&node.stroke.dash) {
            Some(pattern) => dash_path(&all, &pattern, f64::from(self.tolerance(t)))?,
            None => all,
        };
        for paint in strokes {
            let Some(color) = paint.primary_color() else {
                continue;
            };
            let solid = Paint {
                kind: PaintKind::Solid { color },
                blend: BlendMode::Normal,
                ..(*paint).clone()
            };
            let colorer = Colorer::new(&solid, opacity, (node.rect.width, node.rect.height))?;
            self.stroke_path(&line, t, node, node.stroke.weight, &colorer, clip)?;
        }
        Ok(())
    }

    fn paint(
        &mut self,
        path: &VectorPath,
        t: &Affine,
        paint: &Paint,
        opacity: f32,
        size: (f64, f64),
        clip: Clip,
    ) -> Result<()> {
        if !matches!(paint.blend, BlendMode::Normal | BlendMode::PassThrough) {
            let bounds = self.node_bounds;
            self.begin(Layer::plain(1.0, paint.blend, Some(bounds)))?;
            let normal = Paint {
                blend: BlendMode::Normal,
                ..paint.clone()
            };
            let drawn = self.paint(path, t, &normal, opacity, size, clip);
            self.end();
            return drawn;
        }
        if let Some(colorer) = Colorer::gradient(paint, opacity, size, &mut self.gradients) {
            return self.fill_path(path, t, &colorer, clip);
        }
        if !matches!(paint.kind, PaintKind::Image { .. }) {
            let colorer = Colorer::new(paint, opacity, size)?;
            return self.fill_path(path, t, &colorer, clip);
        }
        let Some(colorer) = Colorer::image(self.doc, paint, opacity, size) else {
            return Ok(());
        };
        if let PaintKind::Image { adjust, .. } = &paint.kind {
            if !adjust.is_identity() {
                self.adjustments.insert(adjust.key(), *adjust);
            }
        }
        let fit = matches!(
            paint.kind,
            PaintKind::Image {
                scale_mode: ImageScaleMode::Fit,
                ..
            }
        );
        let Colorer::Image {
            origin,
            size: shown,
            ..
        } = &colorer
        else {
            return Ok(());
        };
        let covers = origin.0 <= 0.0
            && origin.1 <= 0.0
            && origin.0 + shown.0 >= size.0
            && origin.1 + shown.1 >= size.1;
        if !fit || covers {
            return self.fill_path(path, t, &colorer, clip);
        }
        let (x0, y0, x1, y1) = (origin.0, origin.1, origin.0 + shown.0, origin.1 + shown.1);
        let area = vec![vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]];
        let tolerance = f64::from(self.tolerance(t));
        let shape = deesainer_vector::boolean::normalize(
            &deesainer_vector::flatten(path, tolerance),
            path.fill_rule,
        );
        let visible = deesainer_vector::boolean::apply(BoolOp::Intersect, &[shape, area]);
        self.fill_path(&visible, t, &colorer, clip)
    }
}

const MAX_DASHES: usize = 100_000;

fn dash_pattern(dash: &[f64]) -> Option<Vec<f64>> {
    if dash.len() < 2 {
        return None;
    }
    let mut v: Vec<f64> = dash
        .iter()
        .map(|d| if d.is_finite() { d.max(0.0) } else { 0.0 })
        .collect();
    if v.len() % 2 == 1 {
        v.extend(v.clone());
    }
    (v.iter().sum::<f64>() > 0.0).then_some(v)
}

fn dash_path(path: &VectorPath, pattern: &[f64], tolerance: f64) -> Result<VectorPath> {
    let mut subpaths: Vec<(VectorPath, bool)> = Vec::new();
    for cmd in &path.cmds {
        match cmd {
            PathCmd::MoveTo { .. } => subpaths.push((
                VectorPath {
                    cmds: vec![*cmd],
                    fill_rule: path.fill_rule,
                },
                false,
            )),
            PathCmd::Close => {
                if let Some(last) = subpaths.last_mut() {
                    last.1 = true;
                }
            }
            _ => {
                if let Some(last) = subpaths.last_mut() {
                    if !last.1 {
                        last.0.cmds.push(*cmd);
                    }
                }
            }
        }
    }
    let mut out = VectorPath::default();
    let mut dashes = 0usize;
    for (sub, closed) in &subpaths {
        for mut line in deesainer_vector::flatten(sub, tolerance) {
            if *closed {
                if let Some(first) = line.first().copied() {
                    line.push(first);
                }
            }
            let mut index = 0usize;
            let mut left = pattern.first().copied().unwrap_or(0.0);
            let mut on = true;
            let mut current: Vec<Point> = Vec::new();
            if let Some(first) = line.first() {
                current.push(*first);
            }
            for pair in line.windows(2) {
                let [mut a, b] = [pair[0], pair[1]];
                let mut seg = a.distance(b);
                while seg > 0.0 {
                    if left >= seg {
                        left -= seg;
                        if on {
                            current.push(b);
                        }
                        seg = 0.0;
                    } else {
                        let k = left / seg;
                        let mid = a.lerp(b, k);
                        if on {
                            current.push(mid);
                            if current.len() > 1 {
                                dashes += 1;
                                if dashes > MAX_DASHES {
                                    return no("a dash pattern too fine to mesh");
                                }
                                push_polyline(&mut out, &current);
                            }
                            current.clear();
                        } else {
                            current.clear();
                            current.push(mid);
                        }
                        on = !on;
                        seg -= left;
                        a = mid;
                        index = (index + 1) % pattern.len();
                        left = pattern.get(index).copied().unwrap_or(0.0);
                    }
                }
            }
            if on && current.len() > 1 {
                push_polyline(&mut out, &current);
            }
        }
    }
    Ok(out)
}

fn push_polyline(out: &mut VectorPath, points: &[Point]) {
    let mut iter = points.iter();
    if let Some(first) = iter.next() {
        out.cmds.push(PathCmd::MoveTo { to: *first });
        out.cmds.extend(iter.map(|p| PathCmd::LineTo { to: *p }));
    }
}

pub fn texture_pixels(
    doc: &Document,
    asset: &AssetId,
    max_side: usize,
) -> Option<([usize; 2], Vec<u8>)> {
    texture_pixels_adjusted(doc, asset, &deesainer_doc::ImageAdjust::default(), max_side)
}

pub fn texture_pixels_adjusted(
    doc: &Document,
    asset: &AssetId,
    adjust: &deesainer_doc::ImageAdjust,
    max_side: usize,
) -> Option<([usize; 2], Vec<u8>)> {
    let img = deesainer_render::images::get_adjusted(doc, asset, adjust)?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let data = img.data();
    let step = w.max(h).div_ceil(max_side.max(1)).max(1);
    if step == 1 {
        return Some(([w, h], data.to_vec()));
    }
    let (ow, oh) = (w.div_ceil(step), h.div_ceil(step));
    let mut out = Vec::with_capacity(ow.checked_mul(oh)?.checked_mul(4)?);
    for oy in 0..oh {
        for ox in 0..ow {
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in oy * step..((oy + 1) * step).min(h) {
                for x in ox * step..((ox + 1) * step).min(w) {
                    let i = (y * w + x) * 4;
                    if let Some(px) = data.get(i..i + 4) {
                        for (s, v) in sum.iter_mut().zip(px) {
                            *s += u32::from(*v);
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            out.extend(sum.iter().map(|s| ((s + n / 2) / n) as u8));
        }
    }
    Some(([ow, oh], out))
}

pub fn build(
    doc: &Document,
    page: NodeId,
    region: Rect,
    scale: f64,
    text: Option<&dyn TextPainter>,
) -> Result<SceneMesh> {
    let Some(page_node) = doc.get(page) else {
        return no("the page does not exist");
    };
    let background = Some(pack(premul(
        deesainer_render::page_background(doc, page),
        1.0,
    )));
    let mut builder = Builder {
        doc,
        text,
        overrides: Vec::new(),
        slots: Vec::new(),
        region,
        scale: scale.max(1e-6),
        batches: Vec::new(),
        items: Vec::new(),
        layer_depth: 0,
        sealed: false,
        node_bounds: [0.0; 4],
        triangles: 0,
        textures: BTreeSet::new(),
        gradients: BTreeMap::new(),
        adjustments: BTreeMap::new(),
        fill: FillTessellator::new(),
        stroke: StrokeTessellator::new(),
    };
    let top = page_node.children.clone();
    builder.children(&top, &Affine::IDENTITY, None, 0)?;
    Ok(SceneMesh {
        batches: builder.batches,
        items: builder.items,
        region,
        scale,
        background,
        triangles: builder.triangles,
        textures: builder.textures,
        gradients: builder.gradients,
        adjustments: builder.adjustments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::Effect;

    fn add(doc: &mut Document, parent: NodeId, id: u64, kind: NodeKind, rect: Rect) -> NodeId {
        let at = doc.get(parent).map_or(0, |p| p.children.len());
        doc.insert(
            Node::new(NodeId(id), format!("n{id}"), kind, rect),
            parent,
            at,
        )
        .unwrap();
        NodeId(id)
    }

    fn red() -> Paint {
        Paint::solid(Rgba::new(1.0, 0.0, 0.0, 1.0))
    }

    fn everything() -> Rect {
        Rect::new(-1e6, -1e6, 2e6, 2e6)
    }

    fn area(mesh: &SceneMesh) -> f64 {
        mesh.batches
            .iter()
            .map(|b| {
                b.indices
                    .chunks(3)
                    .map(|t| {
                        let p = |i: usize| b.vertices[t[i] as usize].pos;
                        let (a, bb, c) = (p(0), p(1), p(2));
                        f64::from(
                            ((bb[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (bb[1] - a[1])).abs(),
                        ) / 2.0
                    })
                    .sum::<f64>()
            })
            .sum()
    }

    fn doc_with(kind: NodeKind, rect: Rect, f: impl FnOnce(&mut Node)) -> (Document, NodeId) {
        let mut doc = Document::new();
        let page = doc.first_page();
        let id = add(&mut doc, page, 10, kind, rect);
        f(doc.get_mut(id).unwrap());
        (doc, page)
    }

    fn with_image(doc: &mut Document, id: &str, w: u32, h: u32) -> AssetId {
        let mut pix = tiny_skia::Pixmap::new(w, h).unwrap();
        pix.fill(tiny_skia::Color::from_rgba8(0, 128, 255, 255));
        let bytes = deesainer_render::encode_png(&pix).unwrap();
        let asset = AssetId(id.to_owned());
        doc.insert_asset(asset.clone(), std::sync::Arc::new(bytes));
        asset
    }

    fn image(asset: &AssetId, scale_mode: ImageScaleMode) -> Paint {
        Paint {
            kind: PaintKind::Image {
                asset: asset.clone(),
                scale_mode,
                adjust: Default::default(),
            },
            ..red()
        }
    }

    fn image_doc(id: &str, iw: u32, ih: u32, mode: ImageScaleMode) -> (Document, NodeId) {
        let mut doc = Document::new();
        let page = doc.first_page();
        let asset = with_image(&mut doc, id, iw, ih);
        let n = add(
            &mut doc,
            page,
            10,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 200.0, 100.0),
        );
        doc.get_mut(n).unwrap().fills = vec![image(&asset, mode)];
        (doc, page)
    }

    fn uv_range(mesh: &SceneMesh) -> [f32; 4] {
        let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for v in mesh.batches.iter().flat_map(|b| &b.vertices) {
            r = [
                r[0].min(v.uv[0]),
                r[1].min(v.uv[1]),
                r[2].max(v.uv[0]),
                r[3].max(v.uv[1]),
            ];
        }
        r
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn image_fills_are_textured_triangles() {
        let (doc, page) = image_doc("mesh-stretch", 10, 10, ImageScaleMode::Stretch);
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let batch = &mesh.batches[0];
        let Some(TextureRef::Image { asset, repeat, .. }) = batch.texture.clone() else {
            panic!("not an image batch");
        };
        assert_eq!(asset.0, "mesh-stretch");
        assert!(!repeat);
        assert_eq!(mesh.textures.len(), 1);
        assert!(close(uv_range(&mesh), [0.0, 0.0, 1.0, 1.0]));
        assert_eq!(batch.vertices[0].color, [255, 255, 255, 255]);
        assert!((area(&mesh) - 20_000.0).abs() < 1e-3);
    }

    #[test]
    fn fill_crops_and_tile_repeats() {
        let (doc, page) = image_doc("mesh-fill", 10, 10, ImageScaleMode::Fill);
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(close(uv_range(&mesh), [0.0, 0.25, 1.0, 0.75]));
        let (doc, page) = image_doc("mesh-tile", 50, 25, ImageScaleMode::Tile);
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(matches!(
            mesh.batches[0].texture,
            Some(TextureRef::Image { repeat: true, .. })
        ));
        assert!(close(uv_range(&mesh), [0.0, 0.0, 4.0, 4.0]));
    }

    #[test]
    fn fit_draws_only_where_the_image_is() {
        let (doc, page) = image_doc("mesh-fit", 10, 10, ImageScaleMode::Fit);
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!((area(&mesh) - 10_000.0).abs() < 1e-2, "{}", area(&mesh));
        assert!(close(uv_range(&mesh), [0.0, 0.0, 1.0, 1.0]));
    }

    #[test]
    fn fit_images_respect_even_odd_shapes() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let asset = with_image(&mut doc, "mesh-fit-evenodd", 10, 10);
        let sq = |x: f64, y: f64, s: f64| {
            vec![
                PathCmd::MoveTo {
                    to: Point::new(x, y),
                },
                PathCmd::LineTo {
                    to: Point::new(x + s, y),
                },
                PathCmd::LineTo {
                    to: Point::new(x + s, y + s),
                },
                PathCmd::LineTo {
                    to: Point::new(x, y + s),
                },
                PathCmd::Close,
            ]
        };
        let mut cmds = sq(0.0, 0.0, 100.0);
        cmds.extend(sq(20.0, 20.0, 60.0));
        let path = VectorPath {
            cmds,
            fill_rule: FillRule::EvenOdd,
        };
        let n = add(
            &mut doc,
            page,
            10,
            NodeKind::Vector { path },
            Rect::new(0.0, 0.0, 200.0, 100.0),
        );
        doc.get_mut(n).unwrap().fills = vec![image(&asset, ImageScaleMode::Fit)];
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let expect = 2.0 * 50.0 * 20.0 + 20.0 * 60.0;
        assert!((area(&mesh) - expect).abs() < 1.0, "{}", area(&mesh));
    }

    #[test]
    fn the_page_background_follows_the_cpu_renderer() {
        let mut doc = Document::new();
        let page = doc.first_page();
        doc.get_mut(page).unwrap().fills = vec![Paint::linear(
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            vec![
                ColorStop {
                    position: 0.0,
                    color: Rgba::new(0.0, 0.0, 1.0, 1.0),
                },
                ColorStop {
                    position: 1.0,
                    color: Rgba::new(1.0, 0.0, 0.0, 1.0),
                },
            ],
        )];
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert_eq!(mesh.background, Some([0, 0, 255, 255]));
        doc.get_mut(page).unwrap().fills.clear();
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert_eq!(mesh.background, Some([255, 255, 255, 255]));
    }

    #[test]
    fn rounded_image_shapes_and_shared_textures() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let asset = with_image(&mut doc, "mesh-shared", 8, 8);
        for i in 0..3 {
            let n = add(
                &mut doc,
                page,
                10 + i,
                NodeKind::Rectangle,
                Rect::new(i as f64 * 120.0, 0.0, 100.0, 100.0),
            );
            let node = doc.get_mut(n).unwrap();
            node.fills = vec![image(&asset, ImageScaleMode::Fill)];
            node.corner_radius = deesainer_doc::CornerRadius::uniform(30.0);
        }
        let mesh = build(&doc, page, everything(), 4.0, None).unwrap();
        assert_eq!(mesh.textures.len(), 1);
        assert_eq!(mesh.batches.len(), 1);
        assert!(area(&mesh) < 3.0 * 10_000.0 - 100.0);
    }

    #[test]
    fn an_undecodable_image_draws_nothing() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let asset = AssetId("mesh-broken".into());
        doc.insert_asset(asset.clone(), std::sync::Arc::new(vec![1, 2, 3]));
        let n = add(
            &mut doc,
            page,
            10,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        doc.get_mut(n).unwrap().fills = vec![image(&asset, ImageScaleMode::Fill)];
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert_eq!(mesh.triangles, 0);
        assert!(mesh.textures.is_empty());
    }

    #[test]
    fn texture_pixels_are_capped() {
        let mut doc = Document::new();
        let asset = with_image(&mut doc, "mesh-big", 300, 120);
        let ([w, h], px) = texture_pixels(&doc, &asset, 100).unwrap();
        assert_eq!((w, h), (100, 40));
        assert_eq!(px.len(), w * h * 4);
        assert_eq!(&px[..4], &[0, 128, 255, 255]);
        let ([w, h], _) = texture_pixels(&doc, &asset, 4096).unwrap();
        assert_eq!((w, h), (300, 120));
    }

    #[test]
    fn shapes_cover_their_area() {
        let (doc, page) = doc_with(
            NodeKind::Rectangle,
            Rect::new(10.0, 20.0, 100.0, 50.0),
            |n| {
                n.fills = vec![red()];
            },
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!((area(&mesh) - 5000.0).abs() < 1e-3, "{}", area(&mesh));
        let v = mesh.batches[0].vertices[0];
        assert_eq!(v.color, [255, 0, 0, 255]);
        assert!(v.pos[0] >= 10.0 && v.pos[1] >= 20.0);
        assert_eq!(mesh.triangles, 2);
        let (doc, page) = doc_with(NodeKind::ellipse(), Rect::new(0.0, 0.0, 100.0, 60.0), |n| {
            n.fills = vec![red()];
        });
        let mesh = build(&doc, page, everything(), 4.0, None).unwrap();
        let exact = std::f64::consts::PI * 50.0 * 30.0;
        assert!(
            (area(&mesh) - exact).abs() / exact < 0.01,
            "{}",
            area(&mesh)
        );
    }

    #[test]
    fn batches_know_their_bounds() {
        let (doc, page) = doc_with(
            NodeKind::Rectangle,
            Rect::new(10.0, 20.0, 100.0, 50.0),
            |n| {
                n.fills = vec![red()];
            },
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert_eq!(mesh.batches[0].bounds, [10.0, 20.0, 110.0, 70.0]);
    }

    #[test]
    fn rounded_corners_remove_area_and_opacity_scales_alpha() {
        let (doc, page) = doc_with(
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            |n| {
                n.fills = vec![red()];
                n.corner_radius = deesainer_doc::CornerRadius::uniform(20.0);
                n.opacity = 0.5;
            },
        );
        let mesh = build(&doc, page, everything(), 4.0, None).unwrap();
        let a = area(&mesh);
        assert!(a < 10_000.0 - 50.0 && a > 9_000.0, "{a}");
        assert_eq!(mesh.batches[0].vertices[0].color[3], 128);
    }

    #[test]
    fn a_two_stop_gradient_is_exact_at_the_corners() {
        let gradient = Paint::linear(
            Point::new(0.0, 0.5),
            Point::new(1.0, 0.5),
            vec![
                ColorStop {
                    position: 0.0,
                    color: Rgba::new(1.0, 0.0, 0.0, 1.0),
                },
                ColorStop {
                    position: 1.0,
                    color: Rgba::new(0.0, 0.0, 1.0, 1.0),
                },
            ],
        );
        let (doc, page) = doc_with(NodeKind::Rectangle, Rect::new(0.0, 0.0, 100.0, 40.0), |n| {
            n.fills = vec![gradient];
        });
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        for v in &mesh.batches[0].vertices {
            let expect = if v.pos[0] < 1.0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            assert_eq!(v.color, expect);
        }
    }

    #[test]
    fn strokes_are_centred_bands() {
        let (doc, page) = doc_with(
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            |n| {
                n.strokes = vec![red()];
                n.stroke.weight = 4.0;
                n.stroke.align = StrokeAlign::Center;
            },
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!((area(&mesh) - 1600.0).abs() < 1.0, "{}", area(&mesh));
    }

    #[test]
    fn network_regions_with_their_own_fill_get_their_own_colour() {
        use deesainer_doc::VectorNetwork;
        let square = VectorPath {
            cmds: vec![
                PathCmd::MoveTo {
                    to: Point::new(0.0, 0.0),
                },
                PathCmd::LineTo {
                    to: Point::new(100.0, 0.0),
                },
                PathCmd::LineTo {
                    to: Point::new(100.0, 100.0),
                },
                PathCmd::LineTo {
                    to: Point::new(0.0, 100.0),
                },
                PathCmd::Close,
            ],
            fill_rule: FillRule::NonZero,
        };
        let mut net = VectorNetwork::from_path(&square);
        net.connect(0, 2);
        net.set_region_paints(0, Some(vec![Paint::solid(Rgba::new(0.0, 1.0, 0.0, 1.0))]));
        let path = net.to_path(FillRule::NonZero);
        let (doc, page) = doc_with(
            NodeKind::Vector { path },
            Rect::new(0.0, 0.0, 100.0, 100.0),
            |n| {
                n.fills = vec![red()];
                n.network = Some(net);
            },
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let colours: std::collections::BTreeSet<[u8; 4]> =
            mesh.batches[0].vertices.iter().map(|v| v.color).collect();
        assert_eq!(
            colours,
            [[0, 255, 0, 255], [255, 0, 0, 255]].into_iter().collect()
        );
        assert!((area(&mesh) - 10_000.0).abs() < 1.0, "{}", area(&mesh));
    }

    #[test]
    fn what_the_mesh_cannot_draw_is_named() {
        let reason = |f: &dyn Fn(&mut Node)| {
            let (doc, page) = doc_with(NodeKind::Rectangle, Rect::new(0.0, 0.0, 10.0, 10.0), |n| {
                n.fills = vec![red()];
                f(n);
            });
            build(&doc, page, everything(), 1.0, None).unwrap_err().0
        };
        assert!(reason(&|n| {
            n.strokes = vec![red()];
            n.stroke.align = StrokeAlign::Center;
            n.stroke.weight = 1.0;
            n.stroke.dash = vec![1e-6, 1e-6];
        })
        .contains("too fine"));
    }

    #[test]
    fn group_opacity_and_blend_modes_are_layers() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let frame = add(
            &mut doc,
            page,
            10,
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 50.0, 50.0),
        );
        let child = add(
            &mut doc,
            frame,
            11,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        doc.get_mut(child).unwrap().fills = vec![red()];
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(!mesh.has_layers());
        assert_eq!(mesh.items, vec![Item::Batch(0)]);
        let f = doc.get_mut(frame).unwrap();
        f.opacity = 0.5;
        f.clips_content = false;
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let Some(Item::Begin(layer)) = mesh.items.first() else {
            panic!("{:?}", mesh.items);
        };
        assert_eq!(layer.opacity, 0.5);
        assert_eq!(
            layer.bounds, None,
            "an unclipped frame's children can reach anywhere"
        );
        assert_eq!(mesh.items.last(), Some(&Item::End));
        assert_eq!(mesh.batches[0].vertices[0].color, [255, 0, 0, 255]);
        let f = doc.get_mut(frame).unwrap();
        f.opacity = 1.0;
        doc.get_mut(child).unwrap().opacity = 0.5;
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(!mesh.has_layers());
        assert_eq!(mesh.batches[0].vertices[0].color[3], 128);
        let c = doc.get_mut(child).unwrap();
        c.blend = BlendMode::Multiply;
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let Some(Item::Begin(layer)) = mesh.items.first() else {
            panic!("{:?}", mesh.items);
        };
        assert_eq!((layer.blend, layer.opacity), (BlendMode::Multiply, 0.5));
        assert!(layer.bounds.is_some());
        let c = doc.get_mut(child).unwrap();
        c.blend = BlendMode::PassThrough;
        c.opacity = 1.0;
        c.fills = vec![red(), {
            let mut p = red();
            p.blend = BlendMode::Screen;
            p
        }];
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(
            matches!(
                mesh.items.as_slice(),
                [
                    Item::Batch(0),
                    Item::Begin(Layer {
                        blend: BlendMode::Screen,
                        ..
                    }),
                    Item::Batch(1),
                    Item::End
                ]
            ),
            "{:?}",
            mesh.items
        );
    }

    #[test]
    fn layers_nest_at_most_thirty_two_deep() {
        let mut doc = Document::new();
        let mut parent = doc.first_page();
        let page = parent;
        for i in 0..34 {
            parent = add(
                &mut doc,
                parent,
                10 + i,
                NodeKind::Frame,
                Rect::new(0.0, 0.0, 50.0, 50.0),
            );
            doc.get_mut(parent).unwrap().opacity = 0.9;
        }
        let leaf = add(
            &mut doc,
            parent,
            99,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        doc.get_mut(leaf).unwrap().fills = vec![red()];
        let why = build(&doc, page, everything(), 1.0, None).unwrap_err().0;
        assert!(why.contains("deeper than 32"), "{why}");
    }

    #[test]
    fn what_needs_an_offscreen_pass_falls_back() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let frame = add(
            &mut doc,
            page,
            10,
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 50.0, 50.0),
        );
        let child = add(
            &mut doc,
            frame,
            11,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        doc.get_mut(child).unwrap().fills = vec![red()];
        assert!(build(&doc, page, everything(), 1.0, None).is_ok());
        let f = doc.get_mut(frame).unwrap();
        f.clips_content = true;
        f.rotation = 30.0;
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(mesh.items.iter().any(|i| matches!(i, Item::MaskEnd)));
    }

    #[test]
    fn effects_are_layers_in_the_cpu_renderers_order() {
        let shadow = |inner: bool| {
            let (color, offset, blur, spread, visible) = (
                Rgba::new(0.0, 0.0, 0.0, 0.5),
                Point::new(2.0, 4.0),
                6.0,
                1.0,
                true,
            );
            if inner {
                Effect::InnerShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    visible,
                }
            } else {
                Effect::DropShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    visible,
                }
            }
        };
        let (doc, page) = doc_with(NodeKind::Rectangle, Rect::new(0.0, 0.0, 40.0, 30.0), |n| {
            n.fills = vec![red()];
            n.effects = vec![
                shadow(true),
                Effect::BackgroundBlur {
                    radius: 5.0,
                    visible: true,
                },
                shadow(false),
                Effect::LayerBlur {
                    radius: 3.0,
                    visible: true,
                },
            ];
        });
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let begins: Vec<&Layer> = mesh
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Begin(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(begins.len(), 4, "{:?}", mesh.items);
        assert_eq!(begins[0].blur, 3.0);
        assert!(begins[1].tint.is_some() && !begins[1].invert && !begins[1].masked);
        assert!(begins[2].backdrop && begins[2].masked);
        assert!(begins[3].tint.is_some() && begins[3].invert && begins[3].masked);
        let fill_at = mesh
            .items
            .iter()
            .position(|i| matches!(i, Item::Batch(b) if mesh.batches[*b].vertices[0].color == [255, 0, 0, 255]))
            .unwrap();
        let inner_at = mesh
            .items
            .iter()
            .rposition(|i| matches!(i, Item::Begin(_)))
            .unwrap();
        assert!(fill_at < inner_at);
        let (doc, page) = doc_with(NodeKind::Rectangle, Rect::new(0.0, 0.0, 40.0, 30.0), |n| {
            n.fills = vec![red()];
            n.effects = vec![Effect::LayerBlur {
                radius: 3.0,
                visible: false,
            }];
        });
        assert!(!build(&doc, page, everything(), 1.0, None)
            .unwrap()
            .has_layers());
    }

    fn stop(position: f32, r: f32, g: f32, b: f32) -> ColorStop {
        ColorStop {
            position,
            color: Rgba::new(r, g, b, 1.0),
        }
    }

    #[test]
    fn multi_stop_and_radial_gradients_use_generated_textures() {
        let three = Paint::linear(
            Point::new(0.0, 0.5),
            Point::new(1.0, 0.5),
            vec![
                stop(0.0, 1.0, 0.0, 0.0),
                stop(0.5, 0.0, 1.0, 0.0),
                stop(1.0, 0.0, 0.0, 1.0),
            ],
        );
        let (doc, page) = doc_with(NodeKind::Rectangle, Rect::new(0.0, 0.0, 100.0, 40.0), |n| {
            n.fills = vec![three];
        });
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let Some(TextureRef::Gradient(key)) = mesh.batches[0].texture.clone() else {
            panic!("not a gradient batch");
        };
        let ramp = &mesh.gradients[&key];
        assert_eq!(ramp.size, [256, 1]);
        assert_eq!(&ramp.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&ramp.rgba[255 * 4..], &[0, 0, 255, 255]);
        let mid = &ramp.rgba[127 * 4..128 * 4];
        assert!(mid[1] > 240 && mid[0] < 10, "{mid:?}");
        let us: Vec<f32> = mesh.batches[0].vertices.iter().map(|v| v.uv[0]).collect();
        let lo = us.iter().copied().fold(f32::MAX, f32::min);
        let hi = us.iter().copied().fold(f32::MIN, f32::max);
        assert!((lo - 0.5 / 256.0).abs() < 1e-5 && (hi - 255.5 / 256.0).abs() < 1e-5);

        let mut radial = red();
        radial.kind = PaintKind::RadialGradient {
            center: Point::new(0.5, 0.5),
            radius: Point::new(0.5, 0.5),
            stops: vec![stop(0.0, 1.0, 1.0, 1.0), stop(1.0, 0.0, 0.0, 0.0)],
        };
        let (doc, page) = doc_with(NodeKind::Rectangle, Rect::new(0.0, 0.0, 100.0, 40.0), |n| {
            n.fills = vec![radial.clone(), radial];
        });
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert_eq!(mesh.gradients.len(), 1);
        let map = mesh.gradients.values().next().unwrap();
        assert_eq!(map.size, [256, 256]);
        let at = |x: usize, y: usize| &map.rgba[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
        assert!(at(128, 128)[0] > 250, "the centre is the first stop");
        assert_eq!(at(0, 0), &[0, 0, 0, 255], "corners are the last stop");
        assert!(close(uv_range(&mesh), [0.0, 0.0, 1.0, 1.0]));
    }

    #[test]
    fn inside_and_outside_strokes_are_rings() {
        let stroked = |align: StrokeAlign| {
            let (doc, page) = doc_with(
                NodeKind::Rectangle,
                Rect::new(0.0, 0.0, 100.0, 100.0),
                |n| {
                    n.strokes = vec![red()];
                    n.stroke.weight = 10.0;
                    n.stroke.align = align;
                },
            );
            area(&build(&doc, page, everything(), 1.0, None).unwrap())
        };
        assert!((stroked(StrokeAlign::Inside) - 3600.0).abs() < 1.0);
        assert!((stroked(StrokeAlign::Outside) - 4400.0).abs() < 1.0);
        assert!((stroked(StrokeAlign::Center) - 4000.0).abs() < 1.0);
    }

    #[test]
    fn dashed_inside_and_outside_strokes_are_masked_double_strokes() {
        for (align, outside) in [(StrokeAlign::Inside, false), (StrokeAlign::Outside, true)] {
            let (doc, page) = doc_with(
                NodeKind::Rectangle,
                Rect::new(0.0, 0.0, 100.0, 100.0),
                |n| {
                    n.strokes = vec![red()];
                    n.stroke.weight = 4.0;
                    n.stroke.align = align;
                    n.stroke.dash = vec![10.0, 10.0];
                },
            );
            let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
            let Some(Item::Begin(layer)) = mesh.items.first() else {
                panic!("{:?}", mesh.items);
            };
            assert!(layer.masked);
            assert_eq!(layer.mask_outside, outside);
            assert!(matches!(
                mesh.items.as_slice(),
                [
                    Item::Begin(_),
                    Item::Batch(0),
                    Item::MaskEnd,
                    Item::Batch(1),
                    Item::End
                ]
            ));
            let dashes = mesh.batches[1]
                .indices
                .chunks(3)
                .map(|t| {
                    let p = |i: usize| mesh.batches[1].vertices[t[i] as usize].pos;
                    let (a, b, c) = (p(0), p(1), p(2));
                    f64::from(((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs())
                        / 2.0
                })
                .sum::<f64>();
            assert!((dashes - 1600.0).abs() < 40.0, "{dashes}");
        }
    }

    #[test]
    fn dashed_strokes_draw_only_the_dashes() {
        let (doc, page) = doc_with(NodeKind::Line, Rect::new(0.0, 0.0, 100.0, 0.0), |n| {
            n.strokes = vec![red()];
            n.stroke.weight = 2.0;
            n.stroke.dash = vec![10.0, 10.0];
        });
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!((area(&mesh) - 100.0).abs() < 0.5, "{}", area(&mesh));
        assert_eq!(
            dash_pattern(&[10.0, 5.0, 10.0]).unwrap(),
            vec![10.0, 5.0, 10.0, 10.0, 5.0, 10.0]
        );
        assert!(dash_pattern(&[4.0]).is_none());
        assert!(dash_pattern(&[0.0, 0.0]).is_none());
        let (doc, page) = doc_with(
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            |n| {
                n.strokes = vec![red()];
                n.stroke.weight = 2.0;
                n.stroke.align = StrokeAlign::Center;
                n.stroke.dash = vec![25.0, 25.0];
            },
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let a = area(&mesh);
        assert!(a > 390.0 && a < 420.0, "{a}");
    }

    #[test]
    fn frames_clip_and_far_nodes_are_culled() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let frame = add(
            &mut doc,
            page,
            10,
            NodeKind::Frame,
            Rect::new(100.0, 100.0, 50.0, 50.0),
        );
        doc.get_mut(frame).unwrap().clips_content = true;
        let inner = add(
            &mut doc,
            frame,
            11,
            NodeKind::Rectangle,
            Rect::new(-20.0, -20.0, 200.0, 200.0),
        );
        doc.get_mut(inner).unwrap().fills = vec![red()];
        let far = add(
            &mut doc,
            page,
            12,
            NodeKind::Rectangle,
            Rect::new(5000.0, 5000.0, 10.0, 10.0),
        );
        doc.get_mut(far).unwrap().fills = vec![red()];
        let mesh = build(&doc, page, Rect::new(0.0, 0.0, 400.0, 400.0), 1.0, None).unwrap();
        assert_eq!(mesh.batches.len(), 1, "the far node is skipped");
        assert_eq!(mesh.batches[0].clip, Some([100.0, 100.0, 150.0, 150.0]));
        doc.get_mut(frame).unwrap().corner_radius = deesainer_doc::CornerRadius::uniform(8.0);
        let mesh = build(&doc, page, Rect::new(0.0, 0.0, 400.0, 400.0), 1.0, None).unwrap();
        assert!(
            matches!(
                mesh.items.as_slice(),
                [
                    Item::Begin(Layer { masked: true, .. }),
                    Item::Batch(0),
                    Item::MaskEnd,
                    Item::Batch(1),
                    Item::End
                ]
            ),
            "{:?}",
            mesh.items
        );
        assert_eq!(mesh.batches[0].vertices[0].color, [255, 255, 255, 255]);
        assert_eq!(mesh.batches[1].clip, Some([100.0, 100.0, 150.0, 150.0]));
    }

    #[test]
    fn a_mask_masks_the_siblings_above_it() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let below = add(
            &mut doc,
            page,
            10,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let mask = add(
            &mut doc,
            page,
            11,
            NodeKind::ellipse(),
            Rect::new(0.0, 0.0, 50.0, 50.0),
        );
        let above = add(
            &mut doc,
            page,
            12,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 80.0, 80.0),
        );
        for id in [below, mask, above] {
            doc.get_mut(id).unwrap().fills = vec![red()];
        }
        doc.get_mut(mask).unwrap().is_mask = true;
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(
            matches!(
                mesh.items.as_slice(),
                [
                    Item::Batch(0),
                    Item::Begin(Layer {
                        masked: true,
                        bounds: None,
                        ..
                    }),
                    Item::Batch(1),
                    Item::MaskEnd,
                    Item::Batch(2),
                    Item::End
                ]
            ),
            "{:?}",
            mesh.items
        );
        doc.get_mut(mask).unwrap().visible = false;
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(!mesh.has_layers());
    }

    #[test]
    fn instances_expand_with_overrides() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let comp = add(
            &mut doc,
            page,
            10,
            NodeKind::Component {
                description: String::new(),
                properties: Vec::new(),
            },
            Rect::new(0.0, 0.0, 20.0, 20.0),
        );
        let inner = add(
            &mut doc,
            comp,
            11,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 20.0, 20.0),
        );
        doc.get_mut(inner).unwrap().fills = vec![red()];
        add(
            &mut doc,
            page,
            12,
            NodeKind::Instance {
                component: comp,
                overrides: vec![deesainer_doc::Override {
                    target: inner,
                    value: PropValue::Fills(vec![Paint::solid(Rgba::new(0.0, 0.0, 1.0, 1.0))]),
                }],
            },
            Rect::new(30.0, 0.0, 20.0, 20.0),
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let mut red_area = 0.0;
        let mut blue_area = 0.0;
        for batch in &mesh.batches {
            let tri = |i: usize| batch.vertices[batch.indices[i] as usize];
            for t in batch.indices.chunks(3) {
                let (a, b, c) = (tri(t[0] as usize), tri(t[1] as usize), tri(t[2] as usize));
                let area = f64::from(
                    ((b.pos[0] - a.pos[0]) * (c.pos[1] - a.pos[1])
                        - (c.pos[0] - a.pos[0]) * (b.pos[1] - a.pos[1]))
                        .abs(),
                ) / 2.0;
                match a.color {
                    [255, 0, 0, 255] => red_area += area,
                    [0, 0, 255, 255] => blue_area += area,
                    _ => panic!("unexpected colour {:?}", a.color),
                }
            }
        }
        assert!((red_area - 400.0).abs() < 1.0, "{red_area}");
        assert!((blue_area - 400.0).abs() < 1.0, "{blue_area}");
    }

    #[test]
    fn resized_instances_resolve_child_constraints() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let comp = add(
            &mut doc,
            page,
            10,
            NodeKind::Component {
                description: String::new(),
                properties: Vec::new(),
            },
            Rect::new(0.0, 0.0, 100.0, 40.0),
        );
        let pinned = add(
            &mut doc,
            comp,
            11,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 20.0, 20.0),
        );
        let pinned_node = doc.get_mut(pinned).unwrap();
        pinned_node.fills = vec![red()];
        pinned_node.constraints.horizontal = deesainer_doc::Constraint::Min;
        pinned_node.constraints.vertical = deesainer_doc::Constraint::Min;
        let stretchy = add(
            &mut doc,
            comp,
            12,
            NodeKind::Rectangle,
            Rect::new(20.0, 20.0, 20.0, 20.0),
        );
        let stretchy_node = doc.get_mut(stretchy).unwrap();
        stretchy_node.fills = vec![red()];
        stretchy_node.constraints.horizontal = deesainer_doc::Constraint::Max;
        stretchy_node.constraints.vertical = deesainer_doc::Constraint::Min;
        add(
            &mut doc,
            page,
            13,
            NodeKind::Instance {
                component: comp,
                overrides: Vec::new(),
            },
            Rect::new(200.0, 0.0, 200.0, 40.0),
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let vertices: Vec<[f32; 2]> = mesh
            .batches
            .iter()
            .flat_map(|b| b.vertices.iter().map(|v| v.pos))
            .collect();
        assert!(vertices.iter().any(|p| (p[0] - 200.0).abs() < 0.5));
        assert!(vertices.iter().any(|p| (p[0] - 320.0).abs() < 0.5));
        assert!(!vertices.iter().any(|p| p[0] > 230.0 && p[0] < 310.0));
    }

    #[test]
    fn nested_instances_expand() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let leaf = add(
            &mut doc,
            page,
            10,
            NodeKind::Component {
                description: String::new(),
                properties: Vec::new(),
            },
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        add(
            &mut doc,
            leaf,
            11,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        doc.get_mut(NodeId(11)).unwrap().fills = vec![red()];
        let outer = add(
            &mut doc,
            page,
            12,
            NodeKind::Component {
                description: String::new(),
                properties: Vec::new(),
            },
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        add(
            &mut doc,
            outer,
            13,
            NodeKind::Instance {
                component: leaf,
                overrides: Vec::new(),
            },
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        add(
            &mut doc,
            page,
            14,
            NodeKind::Instance {
                component: outer,
                overrides: Vec::new(),
            },
            Rect::new(50.0, 50.0, 10.0, 10.0),
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        let red = area(&mesh);
        assert!((red - 300.0).abs() < 1.0, "{red}");
    }

    #[test]
    fn missing_components_and_background_are_handled() {
        let mut doc = Document::new();
        let page = doc.first_page();
        add(
            &mut doc,
            page,
            10,
            NodeKind::Instance {
                component: NodeId(99),
                overrides: Vec::new(),
            },
            Rect::new(20.0, 0.0, 10.0, 10.0),
        );
        let mesh = build(&doc, page, everything(), 1.0, None).unwrap();
        assert!(mesh.batches.is_empty());
        let empty = Document::new();
        let p = empty.first_page();
        let mesh = build(&empty, p, everything(), 1.0, None).unwrap();
        assert_eq!(mesh.background, Some([245, 245, 245, 255]));
        assert!(mesh.batches.is_empty());
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore = "timing only"]
    fn ten_thousand_shapes() {
        let mut doc = Document::new();
        let page = doc.first_page();
        for i in 0..10_000u64 {
            let x = (i % 100) as f64 * 40.0;
            let y = (i / 100) as f64 * 40.0;
            let kind = if i % 3 == 0 {
                NodeKind::ellipse()
            } else {
                NodeKind::Rectangle
            };
            let mut n = Node::new(
                NodeId(100 + i),
                format!("n{i}"),
                kind,
                Rect::new(x, y, 32.0, 32.0),
            );
            n.fills = vec![Paint::solid(Rgba::new(0.2, 0.5, 0.9, 1.0))];
            let at = doc.get(page).map_or(0, |p| p.children.len());
            doc.insert(n, page, at).unwrap();
        }
        let started = std::time::Instant::now();
        let mesh = build(&doc, page, Rect::new(0.0, 0.0, 4000.0, 4000.0), 1.0, None).unwrap();
        println!(
            "10000 shapes: {} triangles in {:?}",
            mesh.triangles,
            started.elapsed()
        );
    }
}
