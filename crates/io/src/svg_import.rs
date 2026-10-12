use std::collections::BTreeMap;
use std::sync::Arc;

use deesainer_color::Rgba;
use deesainer_doc::{
    AssetId, ColorStop, FillRule, ImageScaleMode, Node, NodeId, NodeKind, Paint, PaintKind,
    PathCmd, StrokeAlign, StrokeCap, StrokeJoin, StrokeStyle, VectorPath,
};
use deesainer_geom::{Affine, Point, Rect};

use crate::IoError;

pub struct Imported {
    pub root: NodeId,
    pub nodes: BTreeMap<NodeId, Node>,
    pub assets: Vec<(AssetId, Arc<Vec<u8>>)>,
    pub size: (f64, f64),
}

struct Builder {
    nodes: BTreeMap<NodeId, Node>,
    assets: Vec<(AssetId, Arc<Vec<u8>>)>,
    next: u64,
}

fn affine(t: usvg::Transform) -> Affine {
    Affine {
        a: f64::from(t.sx),
        b: f64::from(t.ky),
        c: f64::from(t.kx),
        d: f64::from(t.sy),
        e: f64::from(t.tx),
        f: f64::from(t.ty),
    }
}

fn rgba(c: usvg::Color, opacity: f32) -> Rgba {
    Rgba::from_rgba8([c.red, c.green, c.blue, 255]).with_alpha(opacity.clamp(0.0, 1.0))
}

fn to_path(p: &usvg::tiny_skia_path::Path, t: &Affine) -> VectorPath {
    let pt = |q: usvg::tiny_skia_path::Point| t.apply(Point::new(f64::from(q.x), f64::from(q.y)));
    let mut cmds = Vec::new();
    for seg in p.segments() {
        use usvg::tiny_skia_path::PathSegment as S;
        cmds.push(match seg {
            S::MoveTo(a) => PathCmd::MoveTo { to: pt(a) },
            S::LineTo(a) => PathCmd::LineTo { to: pt(a) },
            S::QuadTo(c, a) => PathCmd::QuadTo {
                ctrl: pt(c),
                to: pt(a),
            },
            S::CubicTo(c1, c2, a) => PathCmd::CubicTo {
                ctrl1: pt(c1),
                ctrl2: pt(c2),
                to: pt(a),
            },
            S::Close => PathCmd::Close,
        });
    }
    VectorPath {
        cmds,
        fill_rule: FillRule::NonZero,
    }
}

fn paint(p: &usvg::Paint, opacity: f32, abs: &Affine, bbox: &Rect) -> Option<Paint> {
    let norm = |q: Point| -> Point {
        Point::new(
            (q.x - bbox.x) / bbox.width.max(1e-9),
            (q.y - bbox.y) / bbox.height.max(1e-9),
        )
    };
    let stops = |s: &[usvg::Stop]| -> Vec<ColorStop> {
        s.iter()
            .map(|st| ColorStop {
                position: st.offset().get(),
                color: rgba(st.color(), st.opacity().get()),
            })
            .collect()
    };
    let kind = match p {
        usvg::Paint::Color(c) => PaintKind::Solid {
            color: rgba(*c, 1.0),
        },
        usvg::Paint::LinearGradient(g) => {
            let t = abs.then(affine(g.transform()));
            PaintKind::LinearGradient {
                start: norm(t.apply(Point::new(f64::from(g.x1()), f64::from(g.y1())))),
                end: norm(t.apply(Point::new(f64::from(g.x2()), f64::from(g.y2())))),
                stops: stops(g.stops()),
            }
        }
        usvg::Paint::RadialGradient(g) => {
            let t = abs.then(affine(g.transform()));
            let c = t.apply(Point::new(f64::from(g.cx()), f64::from(g.cy())));
            let r = f64::from(g.r().get());
            let edge_x = t.apply(Point::new(f64::from(g.cx()) + r, f64::from(g.cy())));
            let edge_y = t.apply(Point::new(f64::from(g.cx()), f64::from(g.cy()) + r));
            PaintKind::RadialGradient {
                center: norm(c),
                radius: Point::new(
                    c.distance(edge_x) / bbox.width.max(1e-9),
                    c.distance(edge_y) / bbox.height.max(1e-9),
                ),
                stops: stops(g.stops()),
            }
        }
        usvg::Paint::Pattern(_) => return None,
    };
    Some(Paint {
        kind,
        opacity: opacity.clamp(0.0, 1.0),
        visible: true,
        blend: deesainer_color::BlendMode::Normal,
    })
}

impl Builder {
    fn id(&mut self) -> NodeId {
        self.next += 1;
        NodeId(self.next)
    }

    fn add(&mut self, mut node: Node, parent: NodeId) -> NodeId {
        node.parent = Some(parent);
        let id = node.id;
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.push(id);
        }
        self.nodes.insert(id, node);
        id
    }

    fn group(&mut self, g: &usvg::Group, parent: NodeId, depth: usize) {
        if depth > 256 {
            return;
        }
        for child in g.children() {
            match child {
                usvg::Node::Group(inner) => {
                    let id = self.id();
                    let name = if inner.id().is_empty() {
                        "Group".to_owned()
                    } else {
                        inner.id().to_owned()
                    };
                    let mut node = Node::new(id, name, NodeKind::Group, Rect::default());
                    node.opacity = inner.opacity().get();
                    self.add(node, parent);
                    self.group(inner, id, depth + 1);
                    if self.nodes.get(&id).is_some_and(|n| n.children.is_empty()) {
                        self.nodes.remove(&id);
                        if let Some(p) = self.nodes.get_mut(&parent) {
                            p.children.retain(|c| *c != id);
                        }
                    }
                }
                usvg::Node::Path(path) => self.path(path, parent),
                usvg::Node::Image(img) => self.image(img, parent),
                usvg::Node::Text(_) => {}
            }
        }
    }

    fn path(&mut self, path: &usvg::Path, parent: NodeId) {
        if !path.is_visible() {
            return;
        }
        let abs = affine(path.abs_transform());
        let baked = to_path(path.data(), &abs);
        let Some(bbox) = deesainer_vector::path_bounds(&baked) else {
            return;
        };
        if !bbox.is_valid() {
            return;
        }
        let local = baked.map_points(|q| Point::new(q.x - bbox.x, q.y - bbox.y));
        let id = self.id();
        let name = if path.id().is_empty() {
            "Vector".to_owned()
        } else {
            path.id().to_owned()
        };
        let mut node = Node::new(id, name, NodeKind::Vector { path: local }, bbox);
        if let Some(fill) = path.fill() {
            if let NodeKind::Vector { path: p } = &mut node.kind {
                p.fill_rule = match fill.rule() {
                    usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                    usvg::FillRule::NonZero => FillRule::NonZero,
                };
            }
            if let Some(p) = paint(fill.paint(), fill.opacity().get(), &abs, &bbox) {
                node.fills.push(p);
            }
        }
        if let Some(stroke) = path.stroke() {
            if let Some(p) = paint(stroke.paint(), stroke.opacity().get(), &abs, &bbox) {
                node.strokes.push(p);
            }
            let scale = abs.determinant().abs().sqrt();
            node.stroke = StrokeStyle {
                weight: f64::from(stroke.width().get()) * scale,
                align: StrokeAlign::Center,
                cap: match stroke.linecap() {
                    usvg::LineCap::Butt => StrokeCap::None,
                    usvg::LineCap::Round => StrokeCap::Round,
                    usvg::LineCap::Square => StrokeCap::Square,
                },
                join: match stroke.linejoin() {
                    usvg::LineJoin::Round => StrokeJoin::Round,
                    usvg::LineJoin::Bevel => StrokeJoin::Bevel,
                    _ => StrokeJoin::Miter,
                },
                dash: stroke
                    .dasharray()
                    .map(|d| d.iter().map(|v| f64::from(*v) * scale).collect())
                    .unwrap_or_default(),
                sides: None,
            };
        }
        self.add(node, parent);
    }

    fn image(&mut self, img: &usvg::Image, parent: NodeId) {
        let bytes = match img.kind() {
            usvg::ImageKind::PNG(b) | usvg::ImageKind::JPEG(b) => b.clone(),
            _ => return,
        };
        let abs = affine(img.abs_transform());
        let size = img.size();
        let r = abs.transform_rect_bounds(&Rect::new(
            0.0,
            0.0,
            f64::from(size.width()),
            f64::from(size.height()),
        ));
        let asset = deesainer_format::asset_id(&bytes);
        self.assets
            .push((asset.clone(), Arc::new(bytes.as_ref().clone())));
        let id = self.id();
        let mut node = Node::new(id, "Image", NodeKind::Rectangle, r);
        node.fills.push(Paint {
            kind: PaintKind::Image {
                asset,
                scale_mode: ImageScaleMode::Stretch,
                adjust: Default::default(),
            },
            ..Paint::solid(Rgba::WHITE)
        });
        self.add(node, parent);
    }
}

pub fn import_svg(data: &str, name: &str) -> Result<Imported, IoError> {
    if data.len() > 64 * 1024 * 1024 {
        return Err(IoError::BadSvg("larger than 64 MB".into()));
    }
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_str(data, &opts).map_err(|e| IoError::BadSvg(e.to_string()))?;
    let size = tree.size();
    let mut b = Builder {
        nodes: BTreeMap::new(),
        assets: Vec::new(),
        next: 0,
    };
    let root = b.id();
    let frame = Node::new(
        root,
        name,
        NodeKind::Frame,
        Rect::new(0.0, 0.0, f64::from(size.width()), f64::from(size.height())),
    );
    b.nodes.insert(root, frame);
    b.group(tree.root(), root, 0);
    if let Some(f) = b.nodes.get_mut(&root) {
        f.clips_content = false;
    }
    Ok(Imported {
        root,
        nodes: b.nodes,
        assets: b.assets,
        size: (f64::from(size.width()), f64::from(size.height())),
    })
}
