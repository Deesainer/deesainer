use deesainer_geom::{Point, Rect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PathCmd {
    MoveTo {
        to: Point,
    },
    LineTo {
        to: Point,
    },
    QuadTo {
        ctrl: Point,
        to: Point,
    },
    CubicTo {
        ctrl1: Point,
        ctrl2: Point,
        to: Point,
    },
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VectorPath {
    pub cmds: Vec<PathCmd>,
    pub fill_rule: FillRule,
}

impl VectorPath {
    pub fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.cmds.iter().flat_map(|c| {
            let pts: Vec<Point> = match *c {
                PathCmd::MoveTo { to } | PathCmd::LineTo { to } => vec![to],
                PathCmd::QuadTo { ctrl, to } => vec![ctrl, to],
                PathCmd::CubicTo { ctrl1, ctrl2, to } => vec![ctrl1, ctrl2, to],
                PathCmd::Close => vec![],
            };
            pts
        })
    }

    pub fn is_finite(&self) -> bool {
        self.points().all(|p| p.is_finite())
    }

    pub fn bounds(&self) -> Option<Rect> {
        Rect::bounding(self.points())
    }

    pub fn map_points(&self, f: impl Fn(Point) -> Point) -> VectorPath {
        let cmds = self
            .cmds
            .iter()
            .map(|c| match *c {
                PathCmd::MoveTo { to } => PathCmd::MoveTo { to: f(to) },
                PathCmd::LineTo { to } => PathCmd::LineTo { to: f(to) },
                PathCmd::QuadTo { ctrl, to } => PathCmd::QuadTo {
                    ctrl: f(ctrl),
                    to: f(to),
                },
                PathCmd::CubicTo { ctrl1, ctrl2, to } => PathCmd::CubicTo {
                    ctrl1: f(ctrl1),
                    ctrl2: f(ctrl2),
                    to: f(to),
                },
                PathCmd::Close => PathCmd::Close,
            })
            .collect();
        VectorPath {
            cmds,
            fill_rule: self.fill_rule,
        }
    }

    pub fn rescale(&self, from_w: f64, from_h: f64, to_w: f64, to_h: f64) -> VectorPath {
        let sx = if from_w.abs() > 1e-9 {
            to_w / from_w
        } else {
            1.0
        };
        let sy = if from_h.abs() > 1e-9 {
            to_h / from_h
        } else {
            1.0
        };
        self.map_points(|p| Point::new(p.x * sx, p.y * sy))
    }
}
