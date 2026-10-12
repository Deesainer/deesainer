use deesainer_doc::{Document, NodeId};
use deesainer_geom::{Point, Rect};

#[derive(Debug, Clone, Default)]
pub struct Snap {
    pub dx: f64,
    pub dy: f64,
    pub guides: Vec<(Point, Point)>,
    pub gaps: Vec<(Point, Point)>,
}

impl Snap {
    pub fn none() -> Self {
        Self::default()
    }
}

fn candidates(doc: &Document, page: NodeId, selection: &[NodeId]) -> Vec<Rect> {
    let mut ids: Vec<NodeId> = doc
        .get(page)
        .map(|p| p.children.clone())
        .unwrap_or_default();
    for s in selection {
        if let Some(parent) = doc.get(*s).and_then(|n| n.parent) {
            if parent != page {
                ids.push(parent);
                if let Some(pn) = doc.get(parent) {
                    ids.extend(pn.children.iter().copied());
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter(|id| !selection.contains(id))
        .filter(|id| {
            !selection.iter().any(|s| {
                doc.is_ancestor(*id, *s).unwrap_or(false)
                    && doc.get(*id).is_some_and(|n| !n.kind.is_frame_like())
            })
        })
        .filter(|id| doc.get(*id).is_some_and(|n| n.visible))
        .filter_map(|id| doc.world_bounds(id))
        .collect()
}

fn xs(r: &Rect) -> [f64; 3] {
    [r.x, r.center().x, r.max_x()]
}

fn ys(r: &Rect) -> [f64; 3] {
    [r.y, r.center().y, r.max_y()]
}

fn best(ours: &[f64], theirs: &[(f64, Rect)], threshold: f64) -> Option<(f64, f64, Rect)> {
    let mut found: Option<(f64, f64, Rect)> = None;
    for o in ours {
        for (t, r) in theirs {
            let d = t - o;
            if d.abs() <= threshold && found.is_none_or_closer(d) {
                found = Some((d, *t, *r));
            }
        }
    }
    found
}

trait Closer {
    fn is_none_or_closer(&self, d: f64) -> bool;
}

impl Closer for Option<(f64, f64, Rect)> {
    fn is_none_or_closer(&self, d: f64) -> bool {
        match self {
            None => true,
            Some((best, _, _)) => d.abs() < best.abs(),
        }
    }
}

fn lines(doc: &Document, page: NodeId, selection: &[NodeId]) -> (Vec<f64>, Vec<f64>) {
    use deesainer_doc::GridPattern;
    let (mut vx, mut vy) = (Vec::new(), Vec::new());
    if let Some(p) = doc.get(page) {
        for g in &p.guides {
            if g.vertical {
                vx.push(g.at);
            } else {
                vy.push(g.at);
            }
        }
    }
    let mut frames: Vec<NodeId> = doc
        .get(page)
        .map(|p| p.children.clone())
        .unwrap_or_default();
    for s in selection {
        if let Some(parent) = doc.get(*s).and_then(|n| n.parent) {
            frames.push(parent);
        }
    }
    frames.sort();
    frames.dedup();
    for id in frames {
        let Some(n) = doc.get(id) else { continue };
        if n.layout_grids.is_empty() || !n.visible || !n.kind.is_frame_like() {
            continue;
        }
        let Some(b) = doc.world_bounds(id) else {
            continue;
        };
        for g in n.layout_grids.iter().filter(|g| g.visible) {
            match g.pattern {
                GridPattern::Columns => {
                    for (x0, w) in g.bands(b.width) {
                        vx.extend([b.x + x0, b.x + x0 + w]);
                    }
                }
                GridPattern::Rows => {
                    for (y0, h) in g.bands(b.height) {
                        vy.extend([b.y + y0, b.y + y0 + h]);
                    }
                }
                GridPattern::Grid => {}
            }
        }
    }
    (vx, vy)
}

pub fn snap_rect(
    doc: &Document,
    page: NodeId,
    selection: &[NodeId],
    moving: Rect,
    threshold: f64,
) -> Snap {
    let cands = candidates(doc, page, selection);
    let mut cx: Vec<(f64, Rect)> = cands.iter().flat_map(|r| xs(r).map(|v| (v, *r))).collect();
    let mut cy: Vec<(f64, Rect)> = cands.iter().flat_map(|r| ys(r).map(|v| (v, *r))).collect();
    let (gx, gy) = lines(doc, page, selection);
    cx.extend(
        gx.iter()
            .map(|x| (*x, Rect::new(*x, moving.y, 0.0, moving.height))),
    );
    cy.extend(
        gy.iter()
            .map(|y| (*y, Rect::new(moving.x, *y, moving.width, 0.0))),
    );
    let mut snap = Snap::none();
    let edge_x = best(&xs(&moving), &cx, threshold);
    let space_x = spacing(&cands, moving, Axis::X, threshold);
    match (edge_x, space_x) {
        (Some((d, _, _)), Some(sp)) if sp.delta.abs() < d.abs() => {
            snap.dx = sp.delta;
            snap.gaps.extend(sp.gaps);
        }
        (Some((d, x, r)), _) => {
            snap.dx = d;
            let y0 = moving.y.min(r.y);
            let y1 = moving.max_y().max(r.max_y());
            snap.guides.push((Point::new(x, y0), Point::new(x, y1)));
        }
        (None, Some(sp)) => {
            snap.dx = sp.delta;
            snap.gaps.extend(sp.gaps);
        }
        (None, None) => {}
    }
    let edge_y = best(&ys(&moving), &cy, threshold);
    let space_y = spacing(&cands, moving, Axis::Y, threshold);
    match (edge_y, space_y) {
        (Some((d, _, _)), Some(sp)) if sp.delta.abs() < d.abs() => {
            snap.dy = sp.delta;
            snap.gaps.extend(sp.gaps);
        }
        (Some((d, y, r)), _) => {
            snap.dy = d;
            let x0 = moving.x.min(r.x);
            let x1 = moving.max_x().max(r.max_x());
            snap.guides.push((Point::new(x0, y), Point::new(x1, y)));
        }
        (None, Some(sp)) => {
            snap.dy = sp.delta;
            snap.gaps.extend(sp.gaps);
        }
        (None, None) => {}
    }
    snap
}

pub fn to_pixel_grid(moving: Rect, snap: &mut Snap) {
    snap.dx = (moving.x + snap.dx).round() - moving.x;
    snap.dy = (moving.y + snap.dy).round() - moving.y;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

fn span(r: &Rect, axis: Axis) -> ((f64, f64), (f64, f64)) {
    match axis {
        Axis::X => ((r.x, r.max_x()), (r.y, r.max_y())),
        Axis::Y => ((r.y, r.max_y()), (r.x, r.max_x())),
    }
}

fn gap_line(axis: Axis, a: f64, b: f64, across: f64) -> (Point, Point) {
    match axis {
        Axis::X => (Point::new(a, across), Point::new(b, across)),
        Axis::Y => (Point::new(across, a), Point::new(across, b)),
    }
}

struct Spacing {
    delta: f64,
    gaps: Vec<(Point, Point)>,
}

fn spacing(cands: &[Rect], moving: Rect, axis: Axis, threshold: f64) -> Option<Spacing> {
    const EPS: f64 = 1e-6;
    let ((m0, m1), (c0, c1)) = span(&moving, axis);
    let size = m1 - m0;
    let row: Vec<((f64, f64), (f64, f64))> = cands
        .iter()
        .map(|r| span(r, axis))
        .filter(|(_, (a0, a1))| *a1 > c0 && *a0 < c1)
        .filter(|((a0, a1), _)| *a1 <= m0 + threshold || *a0 >= m1 - threshold)
        .collect();
    let before = row
        .iter()
        .filter(|((_, a1), _)| *a1 <= m0 + threshold)
        .max_by(|a, b| a.0 .1.total_cmp(&b.0 .1))
        .copied();
    let after = row
        .iter()
        .filter(|((a0, _), _)| *a0 >= m1 - threshold)
        .min_by(|a, b| a.0 .0.total_cmp(&b.0 .0))
        .copied();
    let mut existing: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (i, ((_, a1), (ac0, ac1))) in row.iter().enumerate() {
        let next = row
            .iter()
            .enumerate()
            .filter(|(j, ((b0, _), _))| *j != i && *b0 >= *a1 - EPS)
            .min_by(|a, b| a.1 .0 .0.total_cmp(&b.1 .0 .0));
        if let Some((_, ((b0, _), (bc0, bc1)))) = next {
            let gap = b0 - a1;
            let (lo, hi) = (ac0.max(*bc0), ac1.min(*bc1));
            if gap > EPS && lo < hi {
                existing.push((gap, *a1, *b0, (lo + hi) / 2.0));
            }
        }
    }
    let mid = |a: (f64, f64), b: (f64, f64)| {
        let (lo, hi) = (a.0.max(b.0), a.1.min(b.1));
        if lo < hi {
            (lo + hi) / 2.0
        } else {
            (c0 + c1) / 2.0
        }
    };
    let mut found: Option<Spacing> = None;
    let mut offer = |delta: f64, gaps: Vec<(Point, Point)>| {
        if delta.abs() <= threshold && found.as_ref().is_none_or(|f| delta.abs() < f.delta.abs()) {
            found = Some(Spacing { delta, gaps });
        }
    };
    if let (Some((b, bc)), Some((a, ac))) = (before, after) {
        let free = a.0 - b.1 - size;
        if free > 0.0 {
            let start = b.1 + free / 2.0;
            let delta = start - m0;
            offer(
                delta,
                vec![
                    gap_line(axis, b.1, start, mid(bc, (c0, c1))),
                    gap_line(axis, start + size, a.0, mid(ac, (c0, c1))),
                ],
            );
        }
    }
    for (gap, from, to, across) in &existing {
        let shown = gap_line(axis, *from, *to, *across);
        if let Some((b, bc)) = before {
            if (b.1 - to).abs() > EPS || (b.1 - from).abs() > EPS {
                let start = b.1 + gap;
                offer(
                    start - m0,
                    vec![shown, gap_line(axis, b.1, start, mid(bc, (c0, c1)))],
                );
            }
        }
        if let Some((a, ac)) = after {
            let end = a.0 - gap;
            offer(
                end - m1,
                vec![gap_line(axis, end, a.0, mid(ac, (c0, c1))), shown],
            );
        }
    }
    found
}

pub fn snap_point(
    doc: &Document,
    page: NodeId,
    selection: &[NodeId],
    p: Point,
    threshold: f64,
) -> Snap {
    snap_rect(
        doc,
        page,
        selection,
        Rect::new(p.x, p.y, 0.0, 0.0),
        threshold,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::{Node, NodeKind};

    #[test]
    fn snaps_left_edge_to_neighbour() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let a = Node::new(
            doc.next_free_id(),
            "A",
            NodeKind::Rectangle,
            Rect::new(100.0, 0.0, 50.0, 50.0),
        );
        doc.insert(a, page, 0).unwrap();
        let b = Node::new(
            doc.next_free_id(),
            "B",
            NodeKind::Rectangle,
            Rect::new(0.0, 100.0, 50.0, 50.0),
        );
        let bid = b.id;
        doc.insert(b, page, 1).unwrap();
        let s = snap_rect(&doc, page, &[bid], Rect::new(103.0, 100.0, 50.0, 50.0), 5.0);
        assert_eq!(s.dx, -3.0);
        assert_eq!(s.guides.len(), 1);
    }

    #[test]
    fn snaps_to_ruler_guides_and_column_edges() {
        use deesainer_doc::{GridPattern, LayoutGrid, PropValue, RulerGuide};
        let mut doc = Document::new();
        let page = doc.first_page();
        if let Some(p) = doc.get_mut(page) {
            p.set(PropValue::Guides(vec![RulerGuide {
                vertical: true,
                at: 400.0,
            }]));
        }
        let mut f = Node::new(
            doc.next_free_id(),
            "F",
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        f.layout_grids = vec![LayoutGrid {
            pattern: GridPattern::Columns,
            count: 3,
            gutter: 0.0,
            offset: 0.0,
            ..LayoutGrid::default()
        }];
        doc.insert(f, page, 0).unwrap();
        let r = Node::new(
            doc.next_free_id(),
            "R",
            NodeKind::Rectangle,
            Rect::new(500.0, 500.0, 20.0, 20.0),
        );
        let rid = r.id;
        doc.insert(r, page, 1).unwrap();
        let s = snap_rect(&doc, page, &[rid], Rect::new(397.0, 500.0, 20.0, 20.0), 5.0);
        assert_eq!(s.dx, 3.0);
        let s = snap_rect(&doc, page, &[rid], Rect::new(102.0, 500.0, 20.0, 20.0), 5.0);
        assert_eq!(s.dx, -2.0);
    }

    fn row(doc: &mut Document, page: NodeId, rects: &[Rect]) -> Vec<NodeId> {
        rects
            .iter()
            .map(|r| {
                let n = Node::new(doc.next_free_id(), "R", NodeKind::Rectangle, *r);
                let id = n.id;
                let at = doc.get(page).map_or(0, |p| p.children.len());
                doc.insert(n, page, at).unwrap();
                id
            })
            .collect()
    }

    #[test]
    fn snaps_to_equal_spacing_between_neighbours() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let ids = row(
            &mut doc,
            page,
            &[
                Rect::new(0.0, 0.0, 40.0, 40.0),
                Rect::new(200.0, 10.0, 40.0, 40.0),
                Rect::new(90.0, 300.0, 40.0, 40.0),
            ],
        );
        let s = snap_rect(
            &doc,
            page,
            &[ids[2]],
            Rect::new(103.0, 3.0, 40.0, 30.0),
            5.0,
        );
        assert_eq!(s.dx, -3.0);
        assert_eq!(s.gaps.len(), 2);
        let len = |g: &(Point, Point)| (g.1.x - g.0.x).abs();
        assert_eq!(len(&s.gaps[0]), 60.0);
        assert_eq!(len(&s.gaps[1]), 60.0);
    }

    #[test]
    fn repeats_a_gap_already_in_the_row() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let ids = row(
            &mut doc,
            page,
            &[
                Rect::new(0.0, 0.0, 40.0, 40.0),
                Rect::new(60.0, 0.0, 40.0, 40.0),
                Rect::new(0.0, 300.0, 40.0, 40.0),
            ],
        );
        let s = snap_rect(
            &doc,
            page,
            &[ids[2]],
            Rect::new(122.0, 5.0, 40.0, 30.0),
            5.0,
        );
        assert_eq!(s.dx, -2.0);
        assert_eq!(s.gaps.len(), 2);
    }

    #[test]
    fn the_pixel_grid_rounds_the_snapped_origin() {
        let mut s = Snap {
            dx: 0.25,
            dy: 0.0,
            ..Snap::none()
        };
        to_pixel_grid(Rect::new(10.4, 7.6, 5.0, 5.0), &mut s);
        assert!((10.4 + s.dx - 11.0).abs() < 1e-9);
        assert!((7.6 + s.dy - 8.0).abs() < 1e-9);
    }
}
