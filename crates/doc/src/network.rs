use deesainer_geom::Point;
use serde::{Deserialize, Serialize};

use crate::paint::Paint;
use crate::vector::{FillRule, PathCmd, VectorPath};

pub const MAX_VERTICES: usize = 100_000;
pub const MAX_SEGMENTS: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub a: usize,
    pub b: usize,
    #[serde(default)]
    pub ta: Point,
    #[serde(default)]
    pub tb: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionEdge {
    pub segment: usize,
    pub forward: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionPaint {
    pub segments: Vec<usize>,
    pub fills: Vec<Paint>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VectorNetwork {
    pub vertices: Vec<Point>,
    pub segments: Vec<Segment>,
    pub regions: Vec<Vec<RegionEdge>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub region_paints: Vec<RegionPaint>,
}

fn region_key(region: &[RegionEdge]) -> Vec<usize> {
    let mut key: Vec<usize> = region.iter().map(|e| e.segment).collect();
    key.sort_unstable();
    key.dedup();
    key
}

fn is_zero(p: Point) -> bool {
    p.x == 0.0 && p.y == 0.0
}

fn same(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

impl Segment {
    pub fn is_line(&self) -> bool {
        is_zero(self.ta) && is_zero(self.tb)
    }
}

impl VectorNetwork {
    pub fn is_valid(&self) -> bool {
        let n = self.vertices.len();
        self.vertices.len() <= MAX_VERTICES
            && self.segments.len() <= MAX_SEGMENTS
            && self.vertices.iter().all(|p| p.is_finite())
            && self
                .segments
                .iter()
                .all(|s| s.a < n && s.b < n && s.ta.is_finite() && s.tb.is_finite())
            && self.regions.iter().all(|r| {
                r.len() <= MAX_SEGMENTS * 2 && r.iter().all(|e| e.segment < self.segments.len())
            })
    }

    fn oriented(&self, e: RegionEdge) -> Option<[Point; 4]> {
        let s = self.segments.get(e.segment)?;
        let (pa, pb) = (*self.vertices.get(s.a)?, *self.vertices.get(s.b)?);
        let (ca, cb) = (
            Point::new(pa.x + s.ta.x, pa.y + s.ta.y),
            Point::new(pb.x + s.tb.x, pb.y + s.tb.y),
        );
        Some(if e.forward {
            [pa, ca, cb, pb]
        } else {
            [pb, cb, ca, pa]
        })
    }

    fn edge_cmd(&self, e: RegionEdge) -> Option<PathCmd> {
        let seg = self.segments.get(e.segment)?;
        let [_, c1, c2, end] = self.oriented(e)?;
        Some(if seg.is_line() {
            PathCmd::LineTo { to: end }
        } else {
            PathCmd::CubicTo {
                ctrl1: c1,
                ctrl2: c2,
                to: end,
            }
        })
    }

    pub fn to_path(&self, fill_rule: FillRule) -> VectorPath {
        let mut cmds = Vec::new();
        let mut used = vec![false; self.segments.len()];
        for region in &self.regions {
            let Some(first) = region.first().and_then(|e| self.oriented(*e)) else {
                continue;
            };
            cmds.push(PathCmd::MoveTo { to: first[0] });
            for (i, e) in region.iter().enumerate() {
                let closing_line = i + 1 == region.len()
                    && self.segments.get(e.segment).is_some_and(Segment::is_line)
                    && self.oriented(*e).is_some_and(|o| same(o[3], first[0]));
                if let Some(cmd) = self.edge_cmd(*e).filter(|_| !closing_line) {
                    cmds.push(cmd);
                }
                if let Some(u) = used.get_mut(e.segment) {
                    *u = true;
                }
            }
            cmds.push(PathCmd::Close);
        }
        let mut last_end: Option<usize> = None;
        for (i, seg) in self.segments.iter().enumerate() {
            if used[i] {
                continue;
            }
            let e = RegionEdge {
                segment: i,
                forward: true,
            };
            let Some(points) = self.oriented(e) else {
                continue;
            };
            if last_end != Some(seg.a) {
                cmds.push(PathCmd::MoveTo { to: points[0] });
            }
            if let Some(cmd) = self.edge_cmd(e) {
                cmds.push(cmd);
            }
            last_end = Some(seg.b);
        }
        VectorPath { cmds, fill_rule }
    }

    fn vertex_at(&mut self, p: Point) -> usize {
        if let Some(i) = self.vertices.iter().position(|v| same(*v, p)) {
            return i;
        }
        self.vertices.push(p);
        self.vertices.len() - 1
    }

    pub fn from_path(path: &VectorPath) -> Self {
        let mut net = Self::default();
        let mut cur: Option<usize> = None;
        let mut start: Option<usize> = None;
        let mut loop_edges: Vec<RegionEdge> = Vec::new();
        let line = |net: &mut Self,
                    from: usize,
                    to: usize,
                    c1: Option<Point>,
                    c2: Option<Point>,
                    edges: &mut Vec<RegionEdge>| {
            let (pa, pb) = (net.vertices[from], net.vertices[to]);
            let ta = c1.map_or(Point::default(), |c| Point::new(c.x - pa.x, c.y - pa.y));
            let tb = c2.map_or(Point::default(), |c| Point::new(c.x - pb.x, c.y - pb.y));
            net.segments.push(Segment {
                a: from,
                b: to,
                ta,
                tb,
            });
            edges.push(RegionEdge {
                segment: net.segments.len() - 1,
                forward: true,
            });
        };
        for cmd in &path.cmds {
            match *cmd {
                PathCmd::MoveTo { to } => {
                    let v = net.vertex_at(to);
                    cur = Some(v);
                    start = Some(v);
                    loop_edges.clear();
                }
                PathCmd::LineTo { to } => {
                    let Some(from) = cur else { continue };
                    let v = net.vertex_at(to);
                    line(&mut net, from, v, None, None, &mut loop_edges);
                    cur = Some(v);
                }
                PathCmd::QuadTo { ctrl, to } => {
                    let Some(from) = cur else { continue };
                    let p0 = net.vertices[from];
                    let c1 = Point::new(
                        p0.x + 2.0 / 3.0 * (ctrl.x - p0.x),
                        p0.y + 2.0 / 3.0 * (ctrl.y - p0.y),
                    );
                    let c2 = Point::new(
                        to.x + 2.0 / 3.0 * (ctrl.x - to.x),
                        to.y + 2.0 / 3.0 * (ctrl.y - to.y),
                    );
                    let v = net.vertex_at(to);
                    line(&mut net, from, v, Some(c1), Some(c2), &mut loop_edges);
                    cur = Some(v);
                }
                PathCmd::CubicTo { ctrl1, ctrl2, to } => {
                    let Some(from) = cur else { continue };
                    let v = net.vertex_at(to);
                    line(&mut net, from, v, Some(ctrl1), Some(ctrl2), &mut loop_edges);
                    cur = Some(v);
                }
                PathCmd::Close => {
                    if let (Some(c), Some(s)) = (cur, start) {
                        if c != s {
                            line(&mut net, c, s, None, None, &mut loop_edges);
                        }
                        if !loop_edges.is_empty() {
                            net.regions.push(std::mem::take(&mut loop_edges));
                        }
                        cur = Some(s);
                    }
                }
            }
        }
        net
    }

    pub fn from_cubics(curves: &[[Point; 4]]) -> Self {
        let mut net = Self::default();
        for [p0, c1, c2, p3] in curves {
            let a = net.vertex_at(*p0);
            let b = net.vertex_at(*p3);
            let ta = Point::new(c1.x - p0.x, c1.y - p0.y);
            let tb = Point::new(c2.x - p3.x, c2.y - p3.y);
            net.segments.push(Segment { a, b, ta, tb });
        }
        net.rebuild_regions();
        net
    }

    fn leaving(&self, segment: usize, forward: bool) -> f64 {
        let Some(e) = self.oriented(RegionEdge { segment, forward }) else {
            return 0.0;
        };
        let [p0, c1, c2, p3] = e;
        let dir = [c1, c2, p3]
            .into_iter()
            .map(|q| Point::new(q.x - p0.x, q.y - p0.y))
            .find(|d| d.x.abs() > 1e-12 || d.y.abs() > 1e-12)
            .unwrap_or(Point::new(1.0, 0.0));
        dir.y.atan2(dir.x)
    }

    fn loop_area(&self, edges: &[RegionEdge]) -> f64 {
        let mut pts: Vec<Point> = Vec::new();
        for e in edges {
            let Some([p0, c1, c2, p3]) = self.oriented(*e) else {
                continue;
            };
            for k in 0..8 {
                let t = f64::from(k) / 8.0;
                let u = 1.0 - t;
                pts.push(Point::new(
                    u * u * u * p0.x
                        + 3.0 * u * u * t * c1.x
                        + 3.0 * u * t * t * c2.x
                        + t * t * t * p3.x,
                    u * u * u * p0.y
                        + 3.0 * u * u * t * c1.y
                        + 3.0 * u * t * t * c2.y
                        + t * t * t * p3.y,
                ));
            }
        }
        let n = pts.len();
        (0..n)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
            / 2.0
    }

    pub fn rebuild_regions(&mut self) {
        let nseg = self.segments.len();
        let nv = self.vertices.len();
        let origin = |h: usize| {
            let s = &self.segments[h / 2];
            if h.is_multiple_of(2) {
                s.a
            } else {
                s.b
            }
        };
        let mut around: Vec<Vec<(f64, usize)>> = vec![Vec::new(); nv];
        for h in 0..nseg * 2 {
            around[origin(h)].push((self.leaving(h / 2, h.is_multiple_of(2)), h));
        }
        let mut slot: Vec<usize> = vec![0; nseg * 2];
        for list in &mut around {
            list.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (i, (_, h)) in list.iter().enumerate() {
                slot[*h] = i;
            }
        }
        let next = |h: usize| -> usize {
            let twin = h ^ 1;
            let list = &around[origin(twin)];
            let i = slot[twin];
            list[(i + list.len() - 1) % list.len()].1
        };
        let mut seen = vec![false; nseg * 2];
        let mut regions = Vec::new();
        for start in 0..nseg * 2 {
            if seen[start] {
                continue;
            }
            let mut edges = Vec::new();
            let mut h = start;
            while !seen[h] && edges.len() <= nseg * 2 {
                seen[h] = true;
                edges.push(RegionEdge {
                    segment: h / 2,
                    forward: h.is_multiple_of(2),
                });
                h = next(h);
            }
            if h == start && self.loop_area(&edges) > 1e-9 {
                regions.push(edges);
            }
        }
        self.regions = regions;
        let keys: Vec<Vec<usize>> = self.regions.iter().map(|r| region_key(r)).collect();
        self.region_paints.retain(|p| keys.contains(&p.segments));
    }

    pub fn region_path(&self, index: usize, fill_rule: FillRule) -> Option<VectorPath> {
        let region = self.regions.get(index)?;
        let one = Self {
            vertices: self.vertices.clone(),
            segments: self.segments.clone(),
            regions: vec![region.clone()],
            region_paints: Vec::new(),
        };
        let mut path = one.to_path(fill_rule);
        let end = path
            .cmds
            .iter()
            .position(|c| matches!(c, PathCmd::Close))
            .map_or(path.cmds.len(), |i| i + 1);
        path.cmds.truncate(end);
        Some(path)
    }

    pub fn paints_of(&self, index: usize) -> Option<&[Paint]> {
        let key = region_key(self.regions.get(index)?);
        self.region_paints
            .iter()
            .find(|p| p.segments == key)
            .map(|p| p.fills.as_slice())
    }

    pub fn set_region_paints(&mut self, index: usize, fills: Option<Vec<Paint>>) -> bool {
        let Some(region) = self.regions.get(index) else {
            return false;
        };
        let key = region_key(region);
        self.region_paints.retain(|p| p.segments != key);
        if let Some(fills) = fills {
            self.region_paints.push(RegionPaint {
                segments: key,
                fills,
            });
        }
        true
    }

    pub fn region_at(&self, p: Point) -> Option<usize> {
        let polygon = |region: &[RegionEdge]| -> Vec<Point> {
            let mut pts = Vec::new();
            for e in region {
                let Some([p0, c1, c2, p3]) = self.oriented(*e) else {
                    continue;
                };
                for k in 0..8 {
                    let t = f64::from(k) / 8.0;
                    let u = 1.0 - t;
                    pts.push(Point::new(
                        u * u * u * p0.x
                            + 3.0 * u * u * t * c1.x
                            + 3.0 * u * t * t * c2.x
                            + t * t * t * p3.x,
                        u * u * u * p0.y
                            + 3.0 * u * u * t * c1.y
                            + 3.0 * u * t * t * c2.y
                            + t * t * t * p3.y,
                    ));
                }
            }
            pts
        };
        let inside = |poly: &[Point]| {
            let mut hit = false;
            let n = poly.len();
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
                {
                    hit = !hit;
                }
            }
            hit
        };
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, r)| inside(&polygon(r)))
            .min_by(|(_, a), (_, b)| self.loop_area(a).total_cmp(&self.loop_area(b)))
            .map(|(i, _)| i)
    }

    fn remap_region_paints(&mut self, map: impl Fn(usize) -> Option<usize>) {
        self.region_paints.retain_mut(|p| {
            let moved: Option<Vec<usize>> = p.segments.iter().map(|s| map(*s)).collect();
            match moved {
                Some(mut v) => {
                    v.sort_unstable();
                    p.segments = v;
                    true
                }
                None => false,
            }
        });
    }

    pub fn add_vertex(&mut self, p: Point) -> usize {
        self.vertices.push(p);
        self.vertices.len() - 1
    }

    pub fn connect(&mut self, a: usize, b: usize) -> Option<usize> {
        if a >= self.vertices.len() || b >= self.vertices.len() || a == b {
            return None;
        }
        if let Some(i) = self
            .segments
            .iter()
            .position(|s| s.is_line() && ((s.a == a && s.b == b) || (s.a == b && s.b == a)))
        {
            return Some(i);
        }
        self.segments.push(Segment {
            a,
            b,
            ta: Point::default(),
            tb: Point::default(),
        });
        self.rebuild_regions();
        Some(self.segments.len() - 1)
    }

    pub fn remove_segment(&mut self, index: usize) -> bool {
        if index >= self.segments.len() {
            return false;
        }
        self.segments.remove(index);
        self.remap_region_paints(|s| match s.cmp(&index) {
            std::cmp::Ordering::Less => Some(s),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(s - 1),
        });
        self.rebuild_regions();
        true
    }

    pub fn remove_vertex(&mut self, index: usize) -> bool {
        if index >= self.vertices.len() {
            return false;
        }
        self.vertices.remove(index);
        let old_segments = self.segments.clone();
        self.segments.retain(|s| s.a != index && s.b != index);
        let kept: Vec<usize> = old_segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.a != index && s.b != index)
            .map(|(i, _)| i)
            .collect();
        self.remap_region_paints(|s| kept.iter().position(|k| *k == s));
        for s in &mut self.segments {
            if s.a > index {
                s.a -= 1;
            }
            if s.b > index {
                s.b -= 1;
            }
        }
        self.rebuild_regions();
        true
    }

    pub fn move_vertex(&mut self, index: usize, to: Point) -> bool {
        match self.vertices.get_mut(index) {
            Some(v) => {
                *v = to;
                true
            }
            None => false,
        }
    }

    pub fn translated(&self, dx: f64, dy: f64) -> Self {
        let mut n = self.clone();
        for v in &mut n.vertices {
            *v = Point::new(v.x + dx, v.y + dy);
        }
        n
    }

    pub fn scaled(&self, sx: f64, sy: f64) -> Self {
        let mut n = self.clone();
        for v in &mut n.vertices {
            *v = Point::new(v.x * sx, v.y * sy);
        }
        for s in &mut n.segments {
            s.ta = Point::new(s.ta.x * sx, s.ta.y * sy);
            s.tb = Point::new(s.tb.x * sx, s.tb.y * sy);
        }
        n
    }

    pub fn matches(&self, path: &VectorPath) -> bool {
        let mine = self.to_path(path.fill_rule);
        mine.cmds.len() == path.cmds.len()
            && mine
                .points()
                .zip(path.points())
                .all(|(a, b)| (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Paint;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn square() -> VectorPath {
        VectorPath {
            cmds: vec![
                PathCmd::MoveTo { to: p(0.0, 0.0) },
                PathCmd::LineTo { to: p(10.0, 0.0) },
                PathCmd::LineTo { to: p(10.0, 10.0) },
                PathCmd::LineTo { to: p(0.0, 10.0) },
                PathCmd::Close,
            ],
            fill_rule: FillRule::NonZero,
        }
    }

    #[test]
    fn a_path_round_trips_through_a_network() {
        let path = square();
        let net = VectorNetwork::from_path(&path);
        assert_eq!(net.vertices.len(), 4);
        assert_eq!(net.segments.len(), 4);
        assert_eq!(net.regions.len(), 1);
        assert!(net.is_valid());
        assert!(net.matches(&path));
    }

    #[test]
    fn a_chord_splits_a_square_into_two_regions() {
        let mut net = VectorNetwork::from_path(&square());
        assert!(net.connect(0, 2).is_some());
        assert_eq!(net.segments.len(), 5);
        assert_eq!(net.regions.len(), 2);
        let total: f64 = net.regions.iter().map(|r| net.loop_area(r)).sum();
        assert!((total - 100.0).abs() < 1e-6, "{total}");
        let walks = net
            .regions
            .iter()
            .flatten()
            .filter(|e| e.segment == 4)
            .count();
        assert_eq!(walks, 2);
        assert_eq!(net.connect(2, 0), Some(4));
        assert!(net.remove_segment(4));
        assert_eq!(net.regions.len(), 1);
    }

    #[test]
    fn a_branch_stays_an_open_segment() {
        let mut net = VectorNetwork::from_path(&square());
        let tip = net.add_vertex(p(20.0, 5.0));
        net.segments.push(Segment {
            a: 1,
            b: tip,
            ta: Point::default(),
            tb: Point::default(),
        });
        net.rebuild_regions();
        assert_eq!(net.regions.len(), 1, "a dangling edge adds no region");
        let path = net.to_path(FillRule::NonZero);
        let closes = path
            .cmds
            .iter()
            .filter(|c| matches!(c, PathCmd::Close))
            .count();
        assert_eq!(closes, 1);
        assert!(path.points().any(|q| q == p(20.0, 5.0)));
    }

    #[test]
    fn removing_a_vertex_reindexes_and_drops_its_segments() {
        let mut net = VectorNetwork::from_path(&square());
        assert!(net.remove_vertex(1));
        assert_eq!(net.vertices.len(), 3);
        assert_eq!(net.segments.len(), 2);
        assert!(net.is_valid());
        assert!(net.regions.is_empty());
    }

    fn blue() -> Paint {
        Paint::solid(deesainer_color::Rgba::new(0.0, 0.0, 1.0, 1.0))
    }

    #[test]
    fn a_region_keeps_its_paint_through_unrelated_edits() {
        let mut net = VectorNetwork::from_path(&square());
        net.segments.insert(
            0,
            Segment {
                a: 0,
                b: 0,
                ta: Point::default(),
                tb: Point::default(),
            },
        );
        for r in &mut net.regions {
            for e in r {
                e.segment += 1;
            }
        }
        let tip = net.add_vertex(p(30.0, 5.0));
        net.segments[0] = Segment {
            a: 1,
            b: tip,
            ta: Point::default(),
            tb: Point::default(),
        };
        net.connect(0, 2);
        net.rebuild_regions();
        assert_eq!(net.regions.len(), 2);
        assert!(net.set_region_paints(1, Some(vec![blue()])));
        let key = net.region_paints[0].segments.clone();
        let tip2 = net.add_vertex(p(-5.0, 5.0));
        net.segments.push(Segment {
            a: 3,
            b: tip2,
            ta: Point::default(),
            tb: Point::default(),
        });
        net.rebuild_regions();
        assert_eq!(net.region_paints.len(), 1);
        assert_eq!(net.region_paints[0].segments, key);
        assert!(net.remove_segment(0));
        assert_eq!(net.region_paints.len(), 1);
        let shifted: Vec<usize> = key.iter().map(|s| s - 1).collect();
        assert_eq!(net.region_paints[0].segments, shifted);
        let own = (0..net.regions.len())
            .filter(|i| net.paints_of(*i).is_some())
            .count();
        assert_eq!(own, 1);
        let painted = (0..net.regions.len())
            .find(|i| net.paints_of(*i).is_some())
            .unwrap();
        let edges: Vec<usize> = net.regions[painted].iter().map(|e| e.segment).collect();
        let mut verts: Vec<usize> = edges
            .iter()
            .flat_map(|s| [net.segments[*s].a, net.segments[*s].b])
            .collect();
        verts.sort_unstable();
        verts.dedup();
        assert!(verts.len() >= 3);
        assert!(net.remove_segment(edges[0]));
        assert!(
            net.region_paints.is_empty()
                || net
                    .region_paints
                    .iter()
                    .all(|r| net.regions.iter().any(|g| region_key(g) == r.segments))
        );
    }

    #[test]
    fn regions_have_paths_and_can_be_found_by_point() {
        let mut net = VectorNetwork::from_path(&square());
        net.connect(0, 2);
        assert_eq!(net.regions.len(), 2);
        for i in 0..2 {
            let path = net.region_path(i, FillRule::NonZero).unwrap();
            let closes = path
                .cmds
                .iter()
                .filter(|c| matches!(c, PathCmd::Close))
                .count();
            assert_eq!(closes, 1);
        }
        let a = net.region_at(p(8.0, 2.0)).unwrap();
        let b = net.region_at(p(2.0, 8.0)).unwrap();
        assert_ne!(a, b);
        assert!(net.region_at(p(50.0, 50.0)).is_none());
    }

    #[test]
    fn invalid_networks_are_rejected() {
        let mut net = VectorNetwork::from_path(&square());
        net.segments[0].b = 99;
        assert!(!net.is_valid());
        let mut net = VectorNetwork::from_path(&square());
        net.vertices[0] = p(f64::NAN, 0.0);
        assert!(!net.is_valid());
    }

    #[test]
    fn curves_keep_their_tangents() {
        let path = VectorPath {
            cmds: vec![
                PathCmd::MoveTo { to: p(0.0, 0.0) },
                PathCmd::CubicTo {
                    ctrl1: p(0.0, 10.0),
                    ctrl2: p(10.0, 10.0),
                    to: p(10.0, 0.0),
                },
            ],
            fill_rule: FillRule::NonZero,
        };
        let net = VectorNetwork::from_path(&path);
        assert_eq!(net.segments[0].ta, p(0.0, 10.0));
        assert_eq!(net.segments[0].tb, p(0.0, 10.0));
        assert!(net.matches(&path));
    }
}
