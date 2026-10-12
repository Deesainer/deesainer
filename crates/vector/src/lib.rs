pub mod boolean;

use deesainer_doc::{CornerRadius, Document, FillRule, Node, NodeKind, PathCmd, VectorPath};
use deesainer_geom::{Point, Rect};

const KAPPA: f64 = 0.552_284_749_830_793_4;

#[derive(Debug, Default, Clone)]
pub struct Builder {
    cmds: Vec<PathCmd>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.cmds.push(PathCmd::MoveTo {
            to: Point::new(x, y),
        });
        self
    }

    pub fn line_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.cmds.push(PathCmd::LineTo {
            to: Point::new(x, y),
        });
        self
    }

    pub fn cubic_to(&mut self, c1: Point, c2: Point, to: Point) -> &mut Self {
        self.cmds.push(PathCmd::CubicTo {
            ctrl1: c1,
            ctrl2: c2,
            to,
        });
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.cmds.push(PathCmd::Close);
        self
    }

    pub fn finish(&mut self) -> VectorPath {
        VectorPath {
            cmds: std::mem::take(&mut self.cmds),
            fill_rule: FillRule::NonZero,
        }
    }
}

pub fn side_stroke_ring(
    w: f64,
    h: f64,
    r: &CornerRadius,
    sides: [f64; 4],
    align: deesainer_doc::StrokeAlign,
) -> VectorPath {
    use deesainer_doc::StrokeAlign;
    let [t, rt, b, l] = sides.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let (out, inn) = match align {
        StrokeAlign::Inside => (0.0, 1.0),
        StrokeAlign::Center => (0.5, 0.5),
        StrokeAlign::Outside => (1.0, 0.0),
    };
    let boxed = |x0: f64, y0: f64, x1: f64, y1: f64, radii: CornerRadius| {
        rounded_rect((x1 - x0).max(0.0), (y1 - y0).max(0.0), &radii)
            .map_points(|q| Point::new(q.x + x0, q.y + y0))
    };
    let grow = |r: f64, a: f64, b: f64, k: f64| (r + (a + b) / 2.0 * k).max(0.0);
    let outer = boxed(
        -l * out,
        -t * out,
        w + rt * out,
        h + b * out,
        CornerRadius {
            top_left: if r.top_left > 0.0 {
                grow(r.top_left, t, l, out)
            } else {
                0.0
            },
            top_right: if r.top_right > 0.0 {
                grow(r.top_right, t, rt, out)
            } else {
                0.0
            },
            bottom_right: if r.bottom_right > 0.0 {
                grow(r.bottom_right, b, rt, out)
            } else {
                0.0
            },
            bottom_left: if r.bottom_left > 0.0 {
                grow(r.bottom_left, b, l, out)
            } else {
                0.0
            },
        },
    );
    let inner = boxed(
        l * inn,
        t * inn,
        w - rt * inn,
        h - b * inn,
        CornerRadius {
            top_left: grow(r.top_left, t, l, -inn),
            top_right: grow(r.top_right, t, rt, -inn),
            bottom_right: grow(r.bottom_right, b, rt, -inn),
            bottom_left: grow(r.bottom_left, b, l, -inn),
        },
    );
    let mut cmds = outer.cmds;
    if w - (rt + l) * inn > 0.0 && h - (t + b) * inn > 0.0 {
        cmds.extend(inner.cmds);
    }
    VectorPath {
        cmds,
        fill_rule: FillRule::EvenOdd,
    }
}

pub fn rounded_rect(w: f64, h: f64, r: &CornerRadius) -> VectorPath {
    let mut b = Builder::new();
    if r.is_zero() || w <= 0.0 || h <= 0.0 {
        b.move_to(0.0, 0.0)
            .line_to(w, 0.0)
            .line_to(w, h)
            .line_to(0.0, h)
            .close();
        return b.finish();
    }
    let sum_top = r.top_left + r.top_right;
    let sum_bottom = r.bottom_left + r.bottom_right;
    let sum_left = r.top_left + r.bottom_left;
    let sum_right = r.top_right + r.bottom_right;
    let mut k: f64 = 1.0;
    for (sum, side) in [(sum_top, w), (sum_bottom, w), (sum_left, h), (sum_right, h)] {
        if sum > side && sum > 0.0 {
            k = k.min(side / sum);
        }
    }
    let (tl, tr, br, bl) = (
        r.top_left * k,
        r.top_right * k,
        r.bottom_right * k,
        r.bottom_left * k,
    );
    let p = Point::new;
    b.move_to(tl, 0.0).line_to(w - tr, 0.0);
    if tr > 0.0 {
        b.cubic_to(p(w - tr + tr * KAPPA, 0.0), p(w, tr - tr * KAPPA), p(w, tr));
    }
    b.line_to(w, h - br);
    if br > 0.0 {
        b.cubic_to(
            p(w, h - br + br * KAPPA),
            p(w - br + br * KAPPA, h),
            p(w - br, h),
        );
    }
    b.line_to(bl, h);
    if bl > 0.0 {
        b.cubic_to(
            p(bl - bl * KAPPA, h),
            p(0.0, h - bl + bl * KAPPA),
            p(0.0, h - bl),
        );
    }
    b.line_to(0.0, tl);
    if tl > 0.0 {
        b.cubic_to(p(0.0, tl - tl * KAPPA), p(tl - tl * KAPPA, 0.0), p(tl, 0.0));
    }
    b.close();
    b.finish()
}

fn arc(b: &mut Builder, c: Point, rx: f64, ry: f64, a0: f64, a1: f64) {
    let total = a1 - a0;
    let segments = (total.abs() / 90.0).ceil().max(1.0) as usize;
    let step = total / segments as f64;
    for i in 0..segments {
        let t0 = (a0 + step * i as f64).to_radians();
        let t1 = (a0 + step * (i + 1) as f64).to_radians();
        let k = 4.0 / 3.0 * ((t1 - t0) / 4.0).tan();
        let p0 = Point::new(c.x + rx * t0.cos(), c.y + ry * t0.sin());
        let p1 = Point::new(c.x + rx * t1.cos(), c.y + ry * t1.sin());
        let c1 = Point::new(p0.x - k * rx * t0.sin(), p0.y + k * ry * t0.cos());
        let c2 = Point::new(p1.x + k * rx * t1.sin(), p1.y - k * ry * t1.cos());
        b.cubic_to(c1, c2, p1);
    }
}

fn point_on(c: Point, rx: f64, ry: f64, deg: f64) -> Point {
    let t = deg.to_radians();
    Point::new(c.x + rx * t.cos(), c.y + ry * t.sin())
}

pub fn ellipse(w: f64, h: f64, arc_start: f64, arc_end: f64, inner_ratio: f64) -> VectorPath {
    let c = Point::new(w / 2.0, h / 2.0);
    let (rx, ry) = (w / 2.0, h / 2.0);
    let sweep = (arc_end - arc_start).clamp(-360.0, 360.0);
    let full = sweep.abs() >= 360.0 - 1e-9;
    let inner = inner_ratio.clamp(0.0, 0.999);
    let mut b = Builder::new();
    let start = point_on(c, rx, ry, arc_start);
    b.move_to(start.x, start.y);
    arc(&mut b, c, rx, ry, arc_start, arc_start + sweep);
    if full {
        b.close();
        if inner > 0.0 {
            let s = point_on(c, rx * inner, ry * inner, arc_start);
            b.move_to(s.x, s.y);
            arc(
                &mut b,
                c,
                rx * inner,
                ry * inner,
                arc_start,
                arc_start - 360.0,
            );
            b.close();
        }
    } else if inner > 0.0 {
        let e = point_on(c, rx * inner, ry * inner, arc_start + sweep);
        b.line_to(e.x, e.y);
        arc(
            &mut b,
            c,
            rx * inner,
            ry * inner,
            arc_start + sweep,
            arc_start,
        );
        b.close();
    } else {
        b.line_to(c.x, c.y);
        b.close();
    }
    b.finish()
}

pub fn polygon(w: f64, h: f64, sides: u32) -> VectorPath {
    let n = sides.clamp(3, 360);
    let c = Point::new(w / 2.0, h / 2.0);
    let mut b = Builder::new();
    for i in 0..n {
        let deg = -90.0 + 360.0 * f64::from(i) / f64::from(n);
        let p = point_on(c, w / 2.0, h / 2.0, deg);
        if i == 0 {
            b.move_to(p.x, p.y);
        } else {
            b.line_to(p.x, p.y);
        }
    }
    b.close();
    b.finish()
}

pub fn star(w: f64, h: f64, points: u32, inner_ratio: f64) -> VectorPath {
    let n = points.clamp(3, 360);
    let c = Point::new(w / 2.0, h / 2.0);
    let inner = inner_ratio.clamp(0.0, 1.0);
    let mut b = Builder::new();
    for i in 0..(2 * n) {
        let deg = -90.0 + 180.0 * f64::from(i) / f64::from(n);
        let k = if i % 2 == 0 { 1.0 } else { inner };
        let p = point_on(c, w / 2.0 * k, h / 2.0 * k, deg);
        if i == 0 {
            b.move_to(p.x, p.y);
        } else {
            b.line_to(p.x, p.y);
        }
    }
    b.close();
    b.finish()
}

pub fn outline(node: &Node) -> Option<VectorPath> {
    let (w, h) = (node.rect.width, node.rect.height);
    match &node.kind {
        NodeKind::Rectangle
        | NodeKind::Frame
        | NodeKind::Component { .. }
        | NodeKind::ComponentSet
        | NodeKind::Instance { .. }
        | NodeKind::SlotContent { .. } => Some(rounded_rect(w, h, &node.corner_radius)),
        NodeKind::Ellipse {
            arc_start,
            arc_end,
            inner_ratio,
        } => Some(ellipse(w, h, *arc_start, *arc_end, *inner_ratio)),
        NodeKind::Polygon { sides } => Some(polygon(w, h, *sides)),
        NodeKind::Star {
            points,
            inner_ratio,
        } => Some(star(w, h, *points, *inner_ratio)),
        NodeKind::Line => {
            let mut b = Builder::new();
            b.move_to(0.0, 0.0).line_to(w, 0.0);
            Some(b.finish())
        }
        NodeKind::Vector { path } => Some(path.clone()),
        NodeKind::Document
        | NodeKind::Page
        | NodeKind::Group
        | NodeKind::Text(_)
        | NodeKind::BooleanOp { .. } => None,
    }
}

pub fn resolved_outline(doc: &Document, node: &Node) -> Option<VectorPath> {
    match &node.kind {
        NodeKind::BooleanOp { op } => {
            let operands: Vec<Vec<Vec<Point>>> = node
                .children
                .iter()
                .filter_map(|c| doc.get(*c))
                .filter(|c| c.visible)
                .filter_map(|c| {
                    let path = resolved_outline(doc, c)?;
                    let t = Document::local_transform(c);
                    Some(flatten(&path.map_points(|p| t.apply(p)), 0.25))
                })
                .collect();
            Some(boolean::apply(*op, &operands))
        }
        _ => outline(node),
    }
}

pub fn flatten(path: &VectorPath, tolerance: f64) -> Vec<Vec<Point>> {
    let tol = tolerance.max(0.01);
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut last = Point::ZERO;
    let mut start = Point::ZERO;
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo { to } => {
                if current.len() > 1 {
                    out.push(std::mem::take(&mut current));
                }
                current.clear();
                current.push(to);
                last = to;
                start = to;
            }
            PathCmd::LineTo { to } => {
                current.push(to);
                last = to;
            }
            PathCmd::QuadTo { ctrl, to } => {
                let n = segments_for(last.distance(ctrl) + ctrl.distance(to), tol);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let a = last.lerp(ctrl, t);
                    let b = ctrl.lerp(to, t);
                    current.push(a.lerp(b, t));
                }
                last = to;
            }
            PathCmd::CubicTo { ctrl1, ctrl2, to } => {
                let len = last.distance(ctrl1) + ctrl1.distance(ctrl2) + ctrl2.distance(to);
                let n = segments_for(len, tol);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    current.push(cubic_point(last, ctrl1, ctrl2, to, t));
                }
                last = to;
            }
            PathCmd::Close => {
                if current.len() > 1 {
                    out.push(std::mem::take(&mut current));
                }
                current.clear();
                last = start;
                current.push(start);
            }
        }
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

fn segments_for(len: f64, tol: f64) -> usize {
    if !len.is_finite() {
        return 1;
    }
    ((len / tol).sqrt().ceil() as usize).clamp(1, 256)
}

pub fn cubic_point(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    Point::new(
        a * p0.x + b * p1.x + c * p2.x + d * p3.x,
        a * p0.y + b * p1.y + c * p2.y + d * p3.y,
    )
}

pub fn contains(polys: &[Vec<Point>], p: Point, rule: FillRule) -> bool {
    let mut winding = 0i32;
    let mut crossings = 0u32;
    for poly in polys {
        let n = poly.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            if (a.y <= p.y) != (b.y <= p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > p.x {
                    crossings += 1;
                    winding += if b.y > a.y { 1 } else { -1 };
                }
            }
        }
    }
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => crossings % 2 == 1,
    }
}

pub fn distance_to_outline(polys: &[Vec<Point>], p: Point, closed: bool) -> f64 {
    let mut best = f64::INFINITY;
    for poly in polys {
        let n = poly.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            best = best.min(segment_distance(a, b, p));
        }
    }
    best
}

fn segment_distance(a: Point, b: Point, p: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 < 1e-18 {
        return a.distance(p);
    }
    let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

pub fn path_bounds(path: &VectorPath) -> Option<Rect> {
    Rect::bounding(flatten(path, 0.5).into_iter().flatten())
}

pub fn hit_local(doc: &Document, node: &Node, p: Point, slop: f64) -> bool {
    let Some(path) = resolved_outline(doc, node) else {
        return Rect::new(0.0, 0.0, node.rect.width, node.rect.height)
            .inflate(slop)
            .contains(p);
    };
    let polys = flatten(&path, 0.5);
    let filled = node.fills.iter().any(|f| f.visible);
    let closed = !matches!(node.kind, NodeKind::Line);
    if filled && closed && contains(&polys, p, path.fill_rule) {
        return true;
    }
    let half_stroke = if node.strokes.iter().any(|s| s.visible) {
        node.stroke.weight / 2.0
    } else {
        0.0
    };
    distance_to_outline(&polys, p, closed) <= slop + half_stroke
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_outline_contains_center_not_corner_when_rounded() {
        let path = rounded_rect(100.0, 100.0, &CornerRadius::uniform(50.0));
        let polys = flatten(&path, 0.1);
        assert!(contains(&polys, Point::new(50.0, 50.0), FillRule::NonZero));
        assert!(!contains(&polys, Point::new(2.0, 2.0), FillRule::NonZero));
    }

    #[test]
    fn oversized_radii_are_clamped() {
        let path = rounded_rect(10.0, 10.0, &CornerRadius::uniform(100.0));
        let b = path_bounds(&path).unwrap();
        assert!((b.width - 10.0).abs() < 0.01 && (b.height - 10.0).abs() < 0.01);
    }

    #[test]
    fn ellipse_and_donut() {
        let e = flatten(&ellipse(100.0, 50.0, 0.0, 360.0, 0.0), 0.1);
        assert!(contains(&e, Point::new(50.0, 25.0), FillRule::NonZero));
        assert!(!contains(&e, Point::new(2.0, 2.0), FillRule::NonZero));
        let d = flatten(&ellipse(100.0, 100.0, 0.0, 360.0, 0.5), 0.1);
        assert!(!contains(&d, Point::new(50.0, 50.0), FillRule::NonZero));
        assert!(contains(&d, Point::new(50.0, 10.0), FillRule::NonZero));
    }

    #[test]
    fn polygon_and_star_vertices() {
        let tri = polygon(100.0, 100.0, 3);
        assert_eq!(tri.cmds.len(), 4);
        let s = star(100.0, 100.0, 5, 0.4);
        assert_eq!(s.cmds.len(), 11);
        let polys = flatten(&s, 0.1);
        assert!(contains(&polys, Point::new(50.0, 50.0), FillRule::NonZero));
    }

    #[test]
    fn line_hit_uses_slop() {
        let doc = Document::new();
        let mut n = Node::new(
            deesainer_doc::NodeId(99),
            "L",
            NodeKind::Line,
            Rect::new(0.0, 0.0, 100.0, 0.0),
        );
        n.strokes
            .push(deesainer_doc::Paint::solid(deesainer_color::Rgba::BLACK));
        assert!(hit_local(&doc, &n, Point::new(50.0, 2.0), 3.0));
        assert!(!hit_local(&doc, &n, Point::new(50.0, 10.0), 3.0));
    }
}

pub fn hit_path(
    doc: &Document,
    page: deesainer_doc::NodeId,
    p: Point,
    slop: f64,
) -> Vec<deesainer_doc::NodeId> {
    fn visit(
        doc: &Document,
        id: deesainer_doc::NodeId,
        world: &deesainer_geom::Affine,
        p: Point,
        slop: f64,
        depth: usize,
        out: &mut Vec<deesainer_doc::NodeId>,
    ) -> bool {
        let Some(node) = doc.get(id) else {
            return false;
        };
        if !node.visible || node.locked || depth > deesainer_doc::MAX_DEPTH {
            return false;
        }
        let t = world.then(Document::local_transform(node));
        let Some(inv) = t.invert() else { return false };
        let local = inv.apply(p);
        let in_box = Rect::new(0.0, 0.0, node.rect.width, node.rect.height)
            .inflate(slop)
            .contains(local);
        let clips = node.clips_content && node.kind.is_frame_like();
        let descend = (node.kind.is_container() || matches!(node.kind, NodeKind::Instance { .. }))
            && !matches!(node.kind, NodeKind::BooleanOp { .. });
        if descend && (!clips || in_box) {
            for child in node.children.iter().rev() {
                out.push(id);
                if visit(doc, *child, &t, p, slop, depth + 1, out) {
                    return true;
                }
                out.pop();
            }
        }
        let self_hit = match node.kind {
            NodeKind::Group | NodeKind::SlotContent { .. } => false,
            NodeKind::Frame
            | NodeKind::Component { .. }
            | NodeKind::ComponentSet
            | NodeKind::Instance { .. } => in_box,
            _ => in_box && hit_local(doc, node, local, slop),
        };
        if self_hit {
            out.push(id);
        }
        self_hit
    }
    let Some(page_node) = doc.get(page) else {
        return Vec::new();
    };
    let world = doc.world_transform(page);
    for child in page_node.children.iter().rev() {
        let mut out = Vec::new();
        if visit(doc, *child, &world, p, slop, 1, &mut out) {
            return out;
        }
    }
    Vec::new()
}

pub fn hit_test(
    doc: &Document,
    page: deesainer_doc::NodeId,
    p: Point,
    slop: f64,
    deep: bool,
) -> Option<deesainer_doc::NodeId> {
    let path = hit_path(doc, page, p, slop);
    if deep {
        return path.last().copied();
    }
    let first = *path.first()?;
    if doc
        .get(first)
        .is_some_and(|n| matches!(n.kind, NodeKind::Instance { .. }))
    {
        return Some(first);
    }
    let is_board = doc.get(first).is_some_and(|n| {
        matches!(
            n.kind,
            NodeKind::Frame
                | NodeKind::Component { .. }
                | NodeKind::ComponentSet
                | NodeKind::Instance { .. }
        )
    });
    if is_board && path.len() > 1 {
        path.get(1).copied()
    } else {
        Some(first)
    }
}

pub fn marquee(
    doc: &Document,
    page: deesainer_doc::NodeId,
    area: Rect,
    contain: bool,
) -> Vec<deesainer_doc::NodeId> {
    let mut out = Vec::new();
    let Some(page_node) = doc.get(page) else {
        return out;
    };
    let check = |id: deesainer_doc::NodeId| -> bool {
        doc.world_bounds(id).is_some_and(|b| {
            if contain {
                area.contains_rect(&b)
            } else {
                area.intersects(&b)
            }
        })
    };
    for child in &page_node.children {
        let Some(node) = doc.get(*child) else {
            continue;
        };
        if !node.visible || node.locked {
            continue;
        }
        let board = matches!(node.kind, NodeKind::Frame | NodeKind::Component { .. })
            && !node.children.is_empty();
        let frame_fully = doc
            .world_bounds(*child)
            .is_some_and(|b| area.contains_rect(&b));
        if board && !frame_fully {
            for c in &node.children {
                if doc.get(*c).is_some_and(|n| n.visible && !n.locked) && check(*c) {
                    out.push(*c);
                }
            }
        } else if check(*child) {
            out.push(*child);
        }
    }
    out
}

#[cfg(test)]
mod hit_tests {
    use super::*;
    use deesainer_doc::{NodeId, Paint};

    #[test]
    fn click_selects_child_of_board_and_deep_selects_leaf() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let mut frame = Node::new(
            doc.next_free_id(),
            "Board",
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        frame.fills.push(Paint::solid(deesainer_color::Rgba::WHITE));
        let f = frame.id;
        doc.insert(frame, page, 0).unwrap();
        let g = Node::new(
            doc.next_free_id(),
            "G",
            NodeKind::Group,
            Rect::new(10.0, 10.0, 20.0, 20.0),
        );
        let gid = g.id;
        doc.insert(g, f, 0).unwrap();
        let mut r = Node::new(
            doc.next_free_id(),
            "R",
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 20.0, 20.0),
        );
        r.fills.push(Paint::solid(deesainer_color::Rgba::BLACK));
        let rid = r.id;
        doc.insert(r, gid, 0).unwrap();
        let p = Point::new(15.0, 15.0);
        assert_eq!(hit_test(&doc, page, p, 0.0, false), Some(gid));
        assert_eq!(hit_test(&doc, page, p, 0.0, true), Some(rid));
        assert_eq!(
            hit_test(&doc, page, Point::new(80.0, 80.0), 0.0, false),
            Some(f)
        );
        assert_eq!(
            hit_test(&doc, page, Point::new(500.0, 80.0), 0.0, false),
            None
        );
        assert_eq!(
            marquee(&doc, page, Rect::new(5.0, 5.0, 30.0, 30.0), false),
            vec![gid]
        );
        let _ = NodeId(0);
    }
}
