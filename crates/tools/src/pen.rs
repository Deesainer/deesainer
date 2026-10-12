use deesainer_doc::{NodeId, PathCmd};
use deesainer_geom::Point;
use serde_json::json;

use crate::{Input, Out, Overlay, Pointer, Then};

#[derive(Debug, Clone, Copy, PartialEq)]
struct PenPoint {
    pos: Point,
    handle: Option<Point>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PenState {
    parent: Option<NodeId>,
    points: Vec<PenPoint>,
    dragging: bool,
    cursor: Point,
}

const CLOSE_PX: f64 = 8.0;

impl PenState {
    pub fn new(parent: Option<NodeId>) -> Self {
        Self {
            parent,
            points: Vec::new(),
            dragging: false,
            cursor: Point::ZERO,
        }
    }

    pub fn pointer(&mut self, input: &Input<'_>, ev: Pointer) -> (bool, Vec<Out>) {
        match ev {
            Pointer::Down { pos, .. } => {
                if self.points.len() >= 2
                    && self
                        .points
                        .first()
                        .is_some_and(|p| p.pos.distance(pos) * input.zoom <= CLOSE_PX)
                {
                    return (true, self.emit(input, true));
                }
                self.points.push(PenPoint { pos, handle: None });
                self.dragging = true;
                (false, Vec::new())
            }
            Pointer::Move { pos, .. } => {
                self.cursor = pos;
                if self.dragging {
                    if let Some(last) = self.points.last_mut() {
                        if last.pos.distance(pos) * input.zoom > 2.0 {
                            last.handle = Some(pos);
                        }
                    }
                }
                (false, Vec::new())
            }
            Pointer::Up { .. } => {
                self.dragging = false;
                (false, Vec::new())
            }
            Pointer::DoubleClick { .. } => (true, self.emit(input, false)),
        }
    }

    pub fn finish(self) -> Vec<Out> {
        self.emit_raw(false)
    }

    fn emit(&self, _input: &Input<'_>, closed: bool) -> Vec<Out> {
        self.emit_raw(closed)
    }

    fn emit_raw(&self, closed: bool) -> Vec<Out> {
        if self.points.len() < 2 {
            return Vec::new();
        }
        let mut cmds = vec![PathCmd::MoveTo {
            to: self.points[0].pos,
        }];
        let segment = |a: &PenPoint, b: &PenPoint| -> PathCmd {
            let c1 = a.handle.unwrap_or(a.pos);
            let c2 = b.handle.map_or(b.pos, |h| b.pos + (b.pos - h));
            if a.handle.is_none() && b.handle.is_none() {
                PathCmd::LineTo { to: b.pos }
            } else {
                PathCmd::CubicTo {
                    ctrl1: c1,
                    ctrl2: c2,
                    to: b.pos,
                }
            }
        };
        for pair in self.points.windows(2) {
            cmds.push(segment(&pair[0], &pair[1]));
        }
        if closed {
            if let (Some(last), Some(first)) = (self.points.last(), self.points.first()) {
                cmds.push(segment(last, first));
            }
            cmds.push(PathCmd::Close);
        }
        let mut params = json!({ "cmds": cmds, "space": "page" });
        if let Some(p) = self.parent {
            params["parent"] = json!(p);
        }
        vec![Out::Run {
            command: "node.create.vector".into(),
            params,
            then: Then::Nothing,
        }]
    }

    pub fn overlay(&self) -> Overlay {
        let mut pts: Vec<Point> = self.points.iter().map(|p| p.pos).collect();
        if !self.dragging && !pts.is_empty() {
            pts.push(self.cursor);
        }
        Overlay::PenPath(pts, false)
    }
}
