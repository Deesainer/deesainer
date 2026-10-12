pub mod pen;
pub mod proto;
pub mod snap;

use deesainer_doc::{Document, NodeId, NodeKind};
use deesainer_geom::{Point, Rect};
use serde_json::{json, Value};

pub use pen::PenState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Move,
    Frame,
    Rectangle,
    Ellipse,
    Line,
    Polygon,
    Star,
    Text,
    Pen,
    Hand,
}

impl ToolKind {
    pub const ALL: [ToolKind; 10] = [
        ToolKind::Move,
        ToolKind::Frame,
        ToolKind::Rectangle,
        ToolKind::Ellipse,
        ToolKind::Line,
        ToolKind::Polygon,
        ToolKind::Star,
        ToolKind::Pen,
        ToolKind::Text,
        ToolKind::Hand,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Move => "Move",
            ToolKind::Frame => "Frame",
            ToolKind::Rectangle => "Rectangle",
            ToolKind::Ellipse => "Ellipse",
            ToolKind::Line => "Line",
            ToolKind::Polygon => "Polygon",
            ToolKind::Star => "Star",
            ToolKind::Text => "Text",
            ToolKind::Pen => "Pen",
            ToolKind::Hand => "Hand",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            ToolKind::Move => "V",
            ToolKind::Frame => "F",
            ToolKind::Rectangle => "R",
            ToolKind::Ellipse => "O",
            ToolKind::Line => "L",
            ToolKind::Polygon => "",
            ToolKind::Star => "",
            ToolKind::Text => "T",
            ToolKind::Pen => "P",
            ToolKind::Hand => "H",
        }
    }

    pub fn create_command(self) -> Option<&'static str> {
        match self {
            ToolKind::Frame => Some("node.create.frame"),
            ToolKind::Rectangle => Some("node.create.rectangle"),
            ToolKind::Ellipse => Some("node.create.ellipse"),
            ToolKind::Polygon => Some("node.create.polygon"),
            ToolKind::Star => Some("node.create.star"),
            ToolKind::Line => Some("node.create.line"),
            _ => None,
        }
    }

    pub fn for_command(id: &str) -> Option<ToolKind> {
        ToolKind::ALL
            .into_iter()
            .find(|t| t.create_command() == Some(id))
            .or(match id {
                "node.create.text" => Some(ToolKind::Text),
                "node.create.vector" => Some(ToolKind::Pen),
                _ => None,
            })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub command: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    Down { pos: Point, mods: Mods },
    Move { pos: Point, mods: Mods },
    Up { pos: Point, mods: Mods },
    DoubleClick { pos: Point, mods: Mods },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    Nothing,
    EditText,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    BeginGroup(String),
    EndGroup,
    Run {
        command: String,
        params: Value,
        then: Then,
    },
    SetTool(ToolKind),
    Pan(f64, f64),
    EditText(NodeId),
}

fn run(command: &str, params: Value) -> Out {
    Out::Run {
        command: command.to_owned(),
        params,
        then: Then::Nothing,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::TopLeft,
        Handle::Top,
        Handle::TopRight,
        Handle::Right,
        Handle::BottomRight,
        Handle::Bottom,
        Handle::BottomLeft,
        Handle::Left,
    ];

    pub fn at(self, r: &Rect) -> Point {
        let (x0, y0, x1, y1) = (r.x, r.y, r.max_x(), r.max_y());
        let (cx, cy) = (r.center().x, r.center().y);
        match self {
            Handle::TopLeft => Point::new(x0, y0),
            Handle::Top => Point::new(cx, y0),
            Handle::TopRight => Point::new(x1, y0),
            Handle::Right => Point::new(x1, cy),
            Handle::BottomRight => Point::new(x1, y1),
            Handle::Bottom => Point::new(cx, y1),
            Handle::BottomLeft => Point::new(x0, y1),
            Handle::Left => Point::new(x0, cy),
        }
    }

    fn moves(self) -> (i8, i8) {
        match self {
            Handle::TopLeft => (-1, -1),
            Handle::Top => (0, -1),
            Handle::TopRight => (1, -1),
            Handle::Right => (1, 0),
            Handle::BottomRight => (1, 1),
            Handle::Bottom => (0, 1),
            Handle::BottomLeft => (-1, 1),
            Handle::Left => (-1, 0),
        }
    }

    pub fn is_corner(self) -> bool {
        let (x, y) = self.moves();
        x != 0 && y != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    Marquee(Rect),
    Preview(Rect),
    PreviewLine(Point, Point),
    Guide(Point, Point),
    Gap(Point, Point),
    Insert(Point, Point),
    Ghost(Rect),
    DropTarget(Rect),
    PenPath(Vec<Point>, bool),
    Anchors(Vec<Point>, Option<usize>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Crosshair,
    Grab,
    Grabbing,
    Text,
    Resize(Handle),
    Rotate,
}

pub struct Input<'a> {
    pub doc: &'a Document,
    pub page: NodeId,
    pub selection: &'a [NodeId],
    pub zoom: f64,
    pub pixel_grid: bool,
}

impl Input<'_> {
    fn px(&self, screen_px: f64) -> f64 {
        screen_px / self.zoom.max(1e-6)
    }
}

pub fn selection_bounds(doc: &Document, sel: &[NodeId]) -> Option<Rect> {
    sel.iter()
        .filter_map(|id| doc.world_bounds(*id))
        .reduce(Rect::union)
}

#[derive(Debug, Clone)]
enum State {
    Idle,
    Marquee {
        start: Point,
        current: Point,
        additive: bool,
    },
    Pressed {
        start: Point,
        hit: NodeId,
        toggle_on_up: bool,
    },
    Moving {
        last_delta: Point,
        start: Point,
        origin: Rect,
    },
    Resizing {
        id: NodeId,
        handle: Handle,
        start_bounds: Rect,
        parent_origin: Point,
    },
    Rotating {
        id: NodeId,
        center: Point,
        start_angle: f64,
        start_rotation: f64,
    },
    Drawing {
        start: Point,
        current: Point,
        parent: Option<NodeId>,
    },
    Panning {
        last: Point,
    },
    Pen(PenState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathPoint {
    Anchor(usize),
    QuadControl(usize),
    CubicControl1(usize),
    CubicControl2(usize),
}

pub struct Tools {
    pub active: ToolKind,
    state: State,
    hover: Option<NodeId>,
    guides: Vec<(Point, Point)>,
    gaps: Vec<(Point, Point)>,
    drop: Option<DropPlan>,
    path_edit: Option<NodeId>,
    drag_point: Option<PathPoint>,
    selected_anchor: Option<usize>,
}

impl Default for Tools {
    fn default() -> Self {
        Self::new()
    }
}

const DRAG_THRESHOLD_PX: f64 = 3.0;
const HANDLE_PX: f64 = 6.0;
const ROTATE_PX: f64 = 16.0;

impl Tools {
    pub fn new() -> Self {
        Self {
            active: ToolKind::Move,
            state: State::Idle,
            hover: None,
            guides: Vec::new(),
            gaps: Vec::new(),
            drop: None,
            path_edit: None,
            drag_point: None,
            selected_anchor: None,
        }
    }

    pub fn editing_path(&self) -> Option<NodeId> {
        self.path_edit
    }

    pub fn exit_path_edit(&mut self) -> Vec<Out> {
        self.path_edit = None;
        self.selected_anchor = None;
        if self.drag_point.take().is_some() {
            vec![Out::EndGroup]
        } else {
            Vec::new()
        }
    }

    fn path_points(doc: &Document, id: NodeId) -> Vec<(PathPoint, Point)> {
        let Some(node) = doc.get(id) else {
            return Vec::new();
        };
        let NodeKind::Vector { path } = &node.kind else {
            return Vec::new();
        };
        let t = doc.world_transform(id);
        let anchors = path.cmds.iter().enumerate().filter_map(|(i, c)| match c {
            deesainer_doc::PathCmd::MoveTo { to }
            | deesainer_doc::PathCmd::LineTo { to }
            | deesainer_doc::PathCmd::QuadTo { to, .. }
            | deesainer_doc::PathCmd::CubicTo { to, .. } => {
                Some((PathPoint::Anchor(i), t.apply(*to)))
            }
            deesainer_doc::PathCmd::Close => None,
        });
        let controls = path.cmds.iter().enumerate().flat_map(|(i, c)| {
            let points = match c {
                deesainer_doc::PathCmd::QuadTo { ctrl, .. } => {
                    [Some((PathPoint::QuadControl(i), t.apply(*ctrl))), None]
                }
                deesainer_doc::PathCmd::CubicTo { ctrl1, ctrl2, .. } => [
                    Some((PathPoint::CubicControl1(i), t.apply(*ctrl1))),
                    Some((PathPoint::CubicControl2(i), t.apply(*ctrl2))),
                ],
                _ => [None, None],
            };
            points.into_iter().flatten()
        });
        anchors.chain(controls).collect()
    }

    fn insert_on_segment(
        input: &Input<'_>,
        id: NodeId,
        pos: Point,
    ) -> Option<(Vec<deesainer_doc::PathCmd>, usize)> {
        use deesainer_doc::PathCmd as C;
        let node = input.doc.get(id)?;
        let NodeKind::Vector { path } = &node.kind else {
            return None;
        };
        let t = input.doc.world_transform(id);
        let lerp =
            |a: Point, b: Point, u: f64| Point::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u);
        let eval = |p0: Point, c: &C, u: f64| -> Point {
            match c {
                C::LineTo { to } => lerp(p0, *to, u),
                C::QuadTo { ctrl, to } => lerp(lerp(p0, *ctrl, u), lerp(*ctrl, *to, u), u),
                C::CubicTo { ctrl1, ctrl2, to } => {
                    let a = lerp(p0, *ctrl1, u);
                    let b = lerp(*ctrl1, *ctrl2, u);
                    let c2 = lerp(*ctrl2, *to, u);
                    lerp(lerp(a, b, u), lerp(b, c2, u), u)
                }
                _ => p0,
            }
        };
        let mut best: Option<(usize, f64, f64)> = None;
        let mut cur = Point::new(0.0, 0.0);
        for (i, c) in path.cmds.iter().enumerate() {
            match c {
                C::MoveTo { to } => cur = *to,
                C::LineTo { to } | C::QuadTo { to, .. } | C::CubicTo { to, .. } => {
                    for k in 1..64 {
                        let u = f64::from(k) / 64.0;
                        let d = t.apply(eval(cur, c, u)).distance(pos) * input.zoom;
                        if best.is_none_or(|b| d < b.2) {
                            best = Some((i, u, d));
                        }
                    }
                    cur = *to;
                }
                C::Close => {}
            }
        }
        let (i, u, d) = best?;
        if d > 6.0 {
            return None;
        }
        let mut p0 = Point::new(0.0, 0.0);
        for c in &path.cmds[..i] {
            if let C::MoveTo { to }
            | C::LineTo { to }
            | C::QuadTo { to, .. }
            | C::CubicTo { to, .. } = c
            {
                p0 = *to;
            }
        }
        let (first, second) = match path.cmds[i] {
            C::LineTo { to } => (
                C::LineTo {
                    to: lerp(p0, to, u),
                },
                C::LineTo { to },
            ),
            C::QuadTo { ctrl, to } => {
                let a = lerp(p0, ctrl, u);
                let b = lerp(ctrl, to, u);
                (
                    C::QuadTo {
                        ctrl: a,
                        to: lerp(a, b, u),
                    },
                    C::QuadTo { ctrl: b, to },
                )
            }
            C::CubicTo { ctrl1, ctrl2, to } => {
                let a = lerp(p0, ctrl1, u);
                let b = lerp(ctrl1, ctrl2, u);
                let c = lerp(ctrl2, to, u);
                let ab = lerp(a, b, u);
                let bc = lerp(b, c, u);
                let m = lerp(ab, bc, u);
                (
                    C::CubicTo {
                        ctrl1: a,
                        ctrl2: ab,
                        to: m,
                    },
                    C::CubicTo {
                        ctrl1: bc,
                        ctrl2: c,
                        to,
                    },
                )
            }
            _ => return None,
        };
        let mut cmds = path.cmds.clone();
        cmds[i] = first;
        cmds.insert(i + 1, second);
        Some((cmds, i))
    }

    fn path_edit_pointer(
        &mut self,
        input: &Input<'_>,
        id: NodeId,
        ev: Pointer,
    ) -> Option<Vec<Out>> {
        match ev {
            Pointer::Down { pos, .. } => {
                let near = Self::path_points(input.doc, id)
                    .into_iter()
                    .filter_map(|(point, p)| {
                        let distance = p.distance(pos) * input.zoom;
                        (distance <= 6.0).then_some((point, distance))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(point, _)| point);
                match near {
                    Some(point) => {
                        self.drag_point = Some(point);
                        self.selected_anchor = match point {
                            PathPoint::Anchor(i) => Some(i),
                            _ => self.selected_anchor,
                        };
                        Some(vec![Out::BeginGroup("Edit path".into())])
                    }
                    None => {
                        if let Some((cmds, index)) = Self::insert_on_segment(input, id, pos) {
                            self.drag_point = Some(PathPoint::Anchor(index));
                            return Some(vec![
                                Out::BeginGroup("Add point".into()),
                                run("vector.setPath", json!({ "id": id, "cmds": cmds })),
                            ]);
                        }
                        self.path_edit = None;
                        None
                    }
                }
            }
            Pointer::Move { pos, .. } => {
                let point = self.drag_point?;
                let node = input.doc.get(id)?;
                let NodeKind::Vector { path } = &node.kind else {
                    return Some(Vec::new());
                };
                let inv = input.doc.world_transform(id).invert()?;
                let local = inv.apply(pos);
                let mut cmds = path.cmds.clone();
                match point {
                    PathPoint::Anchor(i) => {
                        let mut delta = None;
                        if let Some(cmd) = cmds.get_mut(i) {
                            match cmd {
                                deesainer_doc::PathCmd::MoveTo { to }
                                | deesainer_doc::PathCmd::LineTo { to } => {
                                    delta = Some(local - *to);
                                    *to = local;
                                }
                                deesainer_doc::PathCmd::QuadTo { ctrl, to } => {
                                    let d = local - *to;
                                    *ctrl = *ctrl + d;
                                    *to = local;
                                    delta = Some(d);
                                }
                                deesainer_doc::PathCmd::CubicTo { ctrl2, to, .. } => {
                                    let d = local - *to;
                                    *ctrl2 = *ctrl2 + d;
                                    *to = local;
                                    delta = Some(d);
                                }
                                deesainer_doc::PathCmd::Close => {}
                            }
                        }
                        if let Some(d) = delta {
                            if let Some(next) = cmds.get_mut(i.saturating_add(1)) {
                                match next {
                                    deesainer_doc::PathCmd::QuadTo { ctrl, .. } => {
                                        *ctrl = *ctrl + d;
                                    }
                                    deesainer_doc::PathCmd::CubicTo { ctrl1, .. } => {
                                        *ctrl1 = *ctrl1 + d;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    PathPoint::QuadControl(i) => {
                        if let Some(deesainer_doc::PathCmd::QuadTo { ctrl, .. }) = cmds.get_mut(i) {
                            *ctrl = local;
                        }
                    }
                    PathPoint::CubicControl1(i) => {
                        if let Some(deesainer_doc::PathCmd::CubicTo { ctrl1, .. }) = cmds.get_mut(i)
                        {
                            *ctrl1 = local;
                        }
                    }
                    PathPoint::CubicControl2(i) => {
                        if let Some(deesainer_doc::PathCmd::CubicTo { ctrl2, .. }) = cmds.get_mut(i)
                        {
                            *ctrl2 = local;
                        }
                    }
                }
                Some(vec![run(
                    "vector.setPath",
                    json!({ "id": id, "cmds": cmds }),
                )])
            }
            Pointer::Up { .. } => {
                if self.drag_point.take().is_some() {
                    Some(vec![Out::EndGroup])
                } else {
                    Some(Vec::new())
                }
            }
            Pointer::DoubleClick { pos, .. } => {
                let anchor = Self::path_points(input.doc, id)
                    .into_iter()
                    .filter_map(|(point, p)| match point {
                        PathPoint::Anchor(i) if p.distance(pos) * input.zoom <= 6.0 => Some(i),
                        _ => None,
                    })
                    .next();
                match anchor {
                    Some(i) => {
                        let cmds = Self::toggle_curve(input.doc, id, i)?;
                        Some(vec![run(
                            "vector.setPath",
                            json!({ "id": id, "cmds": cmds }),
                        )])
                    }
                    None => {
                        self.path_edit = None;
                        Some(Vec::new())
                    }
                }
            }
        }
    }

    pub fn delete_selected_point(&mut self, doc: &Document) -> Vec<Out> {
        use deesainer_doc::PathCmd as C;
        let (Some(id), Some(i)) = (self.path_edit, self.selected_anchor) else {
            return Vec::new();
        };
        let Some(node) = doc.get(id) else {
            return Vec::new();
        };
        let NodeKind::Vector { path } = &node.kind else {
            return Vec::new();
        };
        let drawn = path.cmds.iter().filter(|c| !matches!(c, C::Close)).count();
        if drawn <= 2 || i >= path.cmds.len() {
            return Vec::new();
        }
        let mut cmds = path.cmds.clone();
        if matches!(cmds[i], C::MoveTo { .. }) {
            let next_to = match cmds.get(i + 1) {
                Some(C::LineTo { to } | C::QuadTo { to, .. } | C::CubicTo { to, .. }) => Some(*to),
                _ => None,
            };
            match next_to {
                Some(to) => {
                    cmds.remove(i);
                    cmds[i] = C::MoveTo { to };
                }
                None => return Vec::new(),
            }
        } else {
            cmds.remove(i);
        }
        self.selected_anchor = None;
        vec![
            Out::BeginGroup("Delete point".into()),
            run("vector.setPath", json!({ "id": id, "cmds": cmds })),
            Out::EndGroup,
        ]
    }

    fn toggle_curve(doc: &Document, id: NodeId, i: usize) -> Option<Vec<deesainer_doc::PathCmd>> {
        use deesainer_doc::PathCmd as C;
        let node = doc.get(id)?;
        let NodeKind::Vector { path } = &node.kind else {
            return None;
        };
        let mut cmds = path.cmds.clone();
        let anchor = |c: &C| match c {
            C::MoveTo { to } | C::LineTo { to } | C::QuadTo { to, .. } | C::CubicTo { to, .. } => {
                Some(*to)
            }
            C::Close => None,
        };
        let to = anchor(cmds.get(i)?)?;
        let prev = i.checked_sub(1).and_then(|k| cmds.get(k)).and_then(anchor);
        let next = cmds.get(i + 1).and_then(anchor);
        let has_handles = matches!(cmds[i], C::CubicTo { ctrl2, to: t, .. } if ctrl2 != t)
            || matches!(cmds.get(i + 1), Some(C::CubicTo { ctrl1, .. }) if *ctrl1 != to);
        let third = |a: Point, b: Point| Point::new((b.x - a.x) / 3.0, (b.y - a.y) / 3.0);
        if has_handles {
            if let C::CubicTo { ctrl2, .. } = &mut cmds[i] {
                *ctrl2 = to;
            }
            if let Some(C::CubicTo { ctrl1, .. }) = cmds.get_mut(i + 1) {
                *ctrl1 = to;
            }
            return Some(cmds);
        }
        let tangent = match (prev, next) {
            (Some(p), Some(n)) => Point::new((n.x - p.x) / 6.0, (n.y - p.y) / 6.0),
            (Some(p), None) => third(p, to),
            (None, Some(n)) => third(to, n),
            (None, None) => return None,
        };
        if let (Some(p), C::LineTo { .. } | C::CubicTo { .. }) = (prev, cmds[i]) {
            let c1 = match &cmds[i] {
                C::CubicTo { ctrl1, .. } => *ctrl1,
                _ => Point::new(p.x + (to.x - p.x) / 3.0, p.y + (to.y - p.y) / 3.0),
            };
            cmds[i] = C::CubicTo {
                ctrl1: c1,
                ctrl2: Point::new(to.x - tangent.x, to.y - tangent.y),
                to,
            };
        }
        if let (Some(n), Some(C::LineTo { .. } | C::CubicTo { .. })) =
            (next, cmds.get(i + 1).cloned())
        {
            let c2 = match &cmds[i + 1] {
                C::CubicTo { ctrl2, .. } => *ctrl2,
                _ => Point::new(n.x - (n.x - to.x) / 3.0, n.y - (n.y - to.y) / 3.0),
            };
            cmds[i + 1] = C::CubicTo {
                ctrl1: Point::new(to.x + tangent.x, to.y + tangent.y),
                ctrl2: c2,
                to: n,
            };
        }
        Some(cmds)
    }

    pub fn set_tool(&mut self, tool: ToolKind) -> Vec<Out> {
        let mut out = self.cancel();
        if tool != ToolKind::Move {
            out.extend(self.exit_path_edit());
        }
        self.active = tool;
        out
    }

    pub fn cancel(&mut self) -> Vec<Out> {
        let mut out = match std::mem::replace(&mut self.state, State::Idle) {
            State::Moving { .. } | State::Resizing { .. } | State::Rotating { .. } => {
                vec![Out::EndGroup]
            }
            State::Pen(pen) => pen.finish(),
            _ => Vec::new(),
        };
        if self.drag_point.take().is_some() && !out.iter().any(|o| matches!(o, Out::EndGroup)) {
            out.push(Out::EndGroup);
        }
        self.guides.clear();
        self.gaps.clear();
        out
    }

    pub fn is_busy(&self) -> bool {
        !matches!(self.state, State::Idle) || self.drag_point.is_some()
    }

    pub fn hover(&self) -> Option<NodeId> {
        self.hover
    }

    pub fn handle_at(input: &Input<'_>, pos: Point) -> Option<(NodeId, Handle)> {
        let [id] = input.selection else { return None };
        let b = input.doc.world_bounds(*id)?;
        let r = input.px(HANDLE_PX);
        Handle::ALL
            .into_iter()
            .find(|h| h.at(&b).distance(pos) <= r)
            .map(|h| (*id, h))
    }

    fn rotate_zone(input: &Input<'_>, pos: Point) -> Option<NodeId> {
        let [id] = input.selection else { return None };
        let b = input.doc.world_bounds(*id)?;
        let inner = input.px(HANDLE_PX);
        let outer = input.px(ROTATE_PX);
        [
            Handle::TopLeft,
            Handle::TopRight,
            Handle::BottomRight,
            Handle::BottomLeft,
        ]
        .into_iter()
        .any(|h| {
            let d = h.at(&b).distance(pos);
            d > inner && d <= outer && !b.contains(pos)
        })
        .then_some(*id)
    }

    pub fn cursor(&self, input: &Input<'_>, pos: Point) -> Cursor {
        if let Some(id) = self.path_edit.filter(|_| self.active == ToolKind::Move) {
            if self.drag_point.is_some() {
                return Cursor::Grabbing;
            }
            if Self::path_points(input.doc, id)
                .into_iter()
                .any(|(_, point)| point.distance(pos) * input.zoom <= 6.0)
            {
                return Cursor::Grab;
            }
        }
        match &self.state {
            State::Moving { .. } | State::Panning { .. } => return Cursor::Grabbing,
            State::Resizing { handle, .. } => return Cursor::Resize(*handle),
            State::Rotating { .. } => return Cursor::Rotate,
            _ => {}
        }
        match self.active {
            ToolKind::Move => {
                if let Some((_, h)) = Self::handle_at(input, pos) {
                    Cursor::Resize(h)
                } else if Self::rotate_zone(input, pos).is_some() {
                    Cursor::Rotate
                } else {
                    Cursor::Default
                }
            }
            ToolKind::Hand => Cursor::Grab,
            ToolKind::Text => Cursor::Text,
            _ => Cursor::Crosshair,
        }
    }

    pub fn overlays(&self) -> Vec<Overlay> {
        let mut out: Vec<Overlay> = self
            .guides
            .iter()
            .map(|(a, b)| Overlay::Guide(*a, *b))
            .chain(self.gaps.iter().map(|(a, b)| Overlay::Gap(*a, *b)))
            .collect();
        if let Some(d) = &self.drop {
            if let Some(ghost) = d.ghost {
                out.push(Overlay::Ghost(ghost));
            }
            if let Some((a, b)) = d.line {
                out.push(Overlay::Insert(a, b));
            }
            if let Some(t) = d.target_bounds {
                out.push(Overlay::DropTarget(t));
            }
        }
        match &self.state {
            State::Marquee { start, current, .. } => {
                out.push(Overlay::Marquee(Rect::from_points(*start, *current)))
            }
            State::Drawing { start, current, .. } => {
                if self.active == ToolKind::Line {
                    out.push(Overlay::PreviewLine(*start, *current));
                } else {
                    out.push(Overlay::Preview(Rect::from_points(*start, *current)));
                }
            }
            State::Pen(pen) => out.push(pen.overlay()),
            _ => {}
        }
        out
    }

    pub fn doc_overlays(&self, doc: &Document) -> Vec<Overlay> {
        let Some(id) = self.path_edit else {
            return Vec::new();
        };
        let Some(node) = doc.get(id) else {
            return Vec::new();
        };
        let NodeKind::Vector { path } = &node.kind else {
            return Vec::new();
        };
        let transform = doc.world_transform(id);
        let mut out = Vec::new();
        let mut start = None;
        for cmd in &path.cmds {
            match cmd {
                deesainer_doc::PathCmd::MoveTo { to } | deesainer_doc::PathCmd::LineTo { to } => {
                    start = Some(transform.apply(*to))
                }
                deesainer_doc::PathCmd::QuadTo { ctrl, to } => {
                    let control = transform.apply(*ctrl);
                    let end = transform.apply(*to);
                    if let Some(begin) = start {
                        out.push(Overlay::PreviewLine(begin, control));
                    }
                    out.push(Overlay::PreviewLine(control, end));
                    start = Some(end);
                }
                deesainer_doc::PathCmd::CubicTo { ctrl1, ctrl2, to } => {
                    let first = transform.apply(*ctrl1);
                    let second = transform.apply(*ctrl2);
                    let end = transform.apply(*to);
                    if let Some(begin) = start {
                        out.push(Overlay::PreviewLine(begin, first));
                    }
                    out.push(Overlay::PreviewLine(end, second));
                    start = Some(end);
                }
                deesainer_doc::PathCmd::Close => start = None,
            }
        }
        let points = Self::path_points(doc, id);
        for controls in [true, false] {
            let group: Vec<_> = points
                .iter()
                .filter(|(point, _)| matches!(point, PathPoint::Anchor(_)) != controls)
                .copied()
                .collect();
            if !group.is_empty() {
                out.push(Overlay::Anchors(
                    group.iter().map(|(_, pos)| *pos).collect(),
                    self.drag_point
                        .and_then(|active| group.iter().position(|(point, _)| *point == active)),
                ));
            }
        }
        out
    }

    pub fn container_at(doc: &Document, page: NodeId, pos: Point) -> Option<NodeId> {
        let path = deesainer_vector::hit_path(doc, page, pos, 0.0);
        let mut found = None;
        for id in path {
            match doc.get(id).map(|n| &n.kind) {
                Some(NodeKind::Frame | NodeKind::Component { .. }) => found = Some(id),
                Some(NodeKind::Instance { .. }) => break,
                _ => {}
            }
        }
        found.filter(|f| doc.world_bounds(*f).is_some_and(|b| b.contains(pos)))
    }

    pub fn to_local(doc: &Document, parent: Option<NodeId>, page: NodeId, pos: Point) -> Point {
        let target = parent.unwrap_or(page);
        doc.world_transform(target)
            .invert()
            .map_or(pos, |inv| inv.apply(pos))
    }

    pub fn pointer(&mut self, input: &Input<'_>, ev: Pointer) -> Vec<Out> {
        if let Pointer::Move { pos, .. } = ev {
            if matches!(self.state, State::Idle) && self.active == ToolKind::Move {
                self.hover =
                    deesainer_vector::hit_test(input.doc, input.page, pos, input.px(3.0), false);
            }
        }
        match self.active {
            ToolKind::Move => self.move_tool(input, ev),
            ToolKind::Hand => self.hand_tool(ev),
            ToolKind::Text => self.text_tool(input, ev),
            ToolKind::Pen => self.pen_tool(input, ev),
            _ => self.draw_tool(input, ev),
        }
    }

    fn hand_tool(&mut self, ev: Pointer) -> Vec<Out> {
        match (ev, &self.state) {
            (Pointer::Down { pos, .. }, _) => {
                self.state = State::Panning { last: pos };
                Vec::new()
            }
            (Pointer::Move { pos, .. }, State::Panning { last }) => {
                let d = pos - *last;
                vec![Out::Pan(d.x, d.y)]
            }
            (Pointer::Up { .. }, _) => {
                self.state = State::Idle;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn move_tool(&mut self, input: &Input<'_>, ev: Pointer) -> Vec<Out> {
        if let Some(id) = self.path_edit {
            if !input.doc.contains(id) {
                self.path_edit = None;
                if self.drag_point.take().is_some() {
                    return vec![Out::EndGroup];
                }
            } else if let Some(out) = self.path_edit_pointer(input, id, ev) {
                return out;
            }
        }
        let state = std::mem::replace(&mut self.state, State::Idle);
        let (next, out) = match (ev, state) {
            (Pointer::Down { pos, mods }, State::Idle) => self.move_down(input, pos, mods),
            (Pointer::DoubleClick { pos, .. }, state) => {
                let deep =
                    deesainer_vector::hit_test(input.doc, input.page, pos, input.px(3.0), true);
                match deep.and_then(|id| input.doc.get(id).map(|n| (id, n))) {
                    Some((id, n)) if matches!(n.kind, NodeKind::Text(_)) => {
                        (State::Idle, vec![Out::EditText(id)])
                    }
                    Some((id, n))
                        if matches!(n.kind, NodeKind::Vector { .. })
                            && input.selection.contains(&id) =>
                    {
                        self.path_edit = Some(id);
                        (State::Idle, Vec::new())
                    }
                    Some((id, _)) => (
                        State::Idle,
                        vec![run("selection.set", json!({ "ids": [id] }))],
                    ),
                    None => (state, Vec::new()),
                }
            }
            (
                Pointer::Move { pos, .. },
                State::Marquee {
                    start, additive, ..
                },
            ) => {
                let area = Rect::from_points(start, pos);
                let mut ids = deesainer_vector::marquee(input.doc, input.page, area, false);
                if additive {
                    for s in input.selection {
                        if !ids.contains(s) {
                            ids.push(*s);
                        }
                    }
                }
                let out = if ids.as_slice() != input.selection {
                    vec![run("selection.set", json!({ "ids": ids }))]
                } else {
                    Vec::new()
                };
                (
                    State::Marquee {
                        start,
                        current: pos,
                        additive,
                    },
                    out,
                )
            }
            (
                Pointer::Move { pos, mods },
                State::Pressed {
                    start,
                    hit,
                    toggle_on_up,
                },
            ) => {
                if start.distance(pos) * input.zoom < DRAG_THRESHOLD_PX {
                    (
                        State::Pressed {
                            start,
                            hit,
                            toggle_on_up,
                        },
                        Vec::new(),
                    )
                } else {
                    let origin = selection_bounds(input.doc, input.selection).unwrap_or_default();
                    let mut out = vec![Out::BeginGroup("Move".into())];
                    let (state, more) =
                        self.move_drag(input, start, Point::ZERO, origin, pos, mods);
                    out.extend(more);
                    (state, out)
                }
            }
            (
                Pointer::Move { pos, mods },
                State::Moving {
                    last_delta,
                    start,
                    origin,
                },
            ) => self.move_drag(input, start, last_delta, origin, pos, mods),
            (
                Pointer::Move { pos, mods },
                State::Resizing {
                    id,
                    handle,
                    start_bounds,
                    parent_origin,
                },
            ) => {
                let out =
                    self.resize_drag(input, id, handle, start_bounds, parent_origin, pos, mods);
                (
                    State::Resizing {
                        id,
                        handle,
                        start_bounds,
                        parent_origin,
                    },
                    out,
                )
            }
            (
                Pointer::Move { pos, mods },
                State::Rotating {
                    id,
                    center,
                    start_angle,
                    start_rotation,
                },
            ) => {
                let angle = (pos.y - center.y).atan2(pos.x - center.x).to_degrees();
                let mut deg = start_rotation + angle - start_angle;
                if mods.shift {
                    deg = (deg / 15.0).round() * 15.0;
                }
                (
                    State::Rotating {
                        id,
                        center,
                        start_angle,
                        start_rotation,
                    },
                    vec![run(
                        "node.setRotation",
                        json!({ "ids": [id], "degrees": deg }),
                    )],
                )
            }
            (
                Pointer::Up { mods, .. },
                State::Pressed {
                    hit, toggle_on_up, ..
                },
            ) => {
                let out = if toggle_on_up && mods.shift {
                    let ids: Vec<NodeId> = input
                        .selection
                        .iter()
                        .copied()
                        .filter(|s| *s != hit)
                        .collect();
                    vec![run("selection.set", json!({ "ids": ids }))]
                } else if !mods.shift && input.selection.len() > 1 {
                    vec![run("selection.set", json!({ "ids": [hit] }))]
                } else {
                    Vec::new()
                };
                (State::Idle, out)
            }
            (Pointer::Up { .. }, State::Moving { .. }) => {
                self.guides.clear();
                self.gaps.clear();
                let mut out = self
                    .drop
                    .take()
                    .map(|d| d.outs(input.selection))
                    .unwrap_or_default();
                out.push(Out::EndGroup);
                (State::Idle, out)
            }
            (Pointer::Up { .. }, State::Resizing { .. } | State::Rotating { .. }) => {
                self.guides.clear();
                self.gaps.clear();
                (State::Idle, vec![Out::EndGroup])
            }
            (Pointer::Up { .. }, State::Marquee { .. }) => (State::Idle, Vec::new()),
            (_, state) => (state, Vec::new()),
        };
        self.state = next;
        out
    }

    fn move_down(&mut self, input: &Input<'_>, pos: Point, mods: Mods) -> (State, Vec<Out>) {
        if let Some((id, handle)) = Self::handle_at(input, pos) {
            let rotated = input.doc.get(id).is_some_and(|n| n.rotation != 0.0);
            if let (false, Some(b)) = (rotated, input.doc.world_bounds(id)) {
                let parent = input
                    .doc
                    .get(id)
                    .and_then(|n| n.parent)
                    .unwrap_or(input.page);
                let po = input.doc.world_transform(parent).apply(Point::ZERO);
                return (
                    State::Resizing {
                        id,
                        handle,
                        start_bounds: b,
                        parent_origin: po,
                    },
                    vec![Out::BeginGroup("Resize".into())],
                );
            }
        }
        if let Some(id) = Self::rotate_zone(input, pos) {
            if let (Some(b), Some(n)) = (input.doc.world_bounds(id), input.doc.get(id)) {
                let c = b.center();
                return (
                    State::Rotating {
                        id,
                        center: c,
                        start_angle: (pos.y - c.y).atan2(pos.x - c.x).to_degrees(),
                        start_rotation: n.rotation,
                    },
                    vec![Out::BeginGroup("Rotate".into())],
                );
            }
        }
        let mut hit =
            deesainer_vector::hit_test(input.doc, input.page, pos, input.px(3.0), mods.command);
        if let Some(h) = hit.filter(|h| !input.selection.contains(h)) {
            let path = deesainer_vector::hit_path(input.doc, input.page, pos, input.px(3.0));
            if let Some(at) = path.iter().position(|id| *id == h) {
                if let Some(deeper) = path[at + 1..]
                    .iter()
                    .rev()
                    .find(|id| input.selection.contains(id))
                {
                    hit = Some(*deeper);
                }
            }
        }
        let in_selection_bounds =
            selection_bounds(input.doc, input.selection).is_some_and(|b| b.contains(pos));
        match hit {
            Some(id) if input.selection.contains(&id) => (
                State::Pressed {
                    start: pos,
                    hit: id,
                    toggle_on_up: true,
                },
                Vec::new(),
            ),
            Some(id) => {
                let ids: Vec<NodeId> = if mods.shift {
                    input.selection.iter().copied().chain([id]).collect()
                } else {
                    vec![id]
                };
                (
                    State::Pressed {
                        start: pos,
                        hit: id,
                        toggle_on_up: false,
                    },
                    vec![run("selection.set", json!({ "ids": ids }))],
                )
            }
            None if in_selection_bounds && input.selection.len() > 1 => (
                State::Pressed {
                    start: pos,
                    hit: input.selection[0],
                    toggle_on_up: false,
                },
                Vec::new(),
            ),
            None => {
                let out = if mods.shift || input.selection.is_empty() {
                    Vec::new()
                } else {
                    vec![run("selection.clear", Value::Null)]
                };
                (
                    State::Marquee {
                        start: pos,
                        current: pos,
                        additive: mods.shift,
                    },
                    out,
                )
            }
        }
    }

    fn move_drag(
        &mut self,
        input: &Input<'_>,
        start: Point,
        last_delta: Point,
        origin: Rect,
        pos: Point,
        mods: Mods,
    ) -> (State, Vec<Out>) {
        let mut delta = pos - start;
        if mods.shift {
            if delta.x.abs() > delta.y.abs() {
                delta.y = 0.0;
            } else {
                delta.x = 0.0;
            }
        }
        let moved = origin.translate(delta.x, delta.y);
        let mut snap = if mods.command {
            snap::Snap::none()
        } else {
            snap::snap_rect(input.doc, input.page, input.selection, moved, input.px(5.0))
        };
        if input.pixel_grid {
            snap::to_pixel_grid(moved, &mut snap);
        }
        delta = delta + Point::new(snap.dx, snap.dy);
        self.guides = snap.guides;
        self.gaps = snap.gaps;
        self.drop = DropPlan::new(
            input,
            origin.translate(delta.x, delta.y),
            pos,
            delta,
            last_delta,
        );
        if self.drop.as_ref().is_some_and(|d| d.pinned) {
            return (
                State::Moving {
                    last_delta,
                    start,
                    origin,
                },
                Vec::new(),
            );
        }
        let step = delta - last_delta;
        let out = if step.x.abs() > 1e-9 || step.y.abs() > 1e-9 {
            vec![run("node.move", json!({ "dx": step.x, "dy": step.y }))]
        } else {
            Vec::new()
        };
        (
            State::Moving {
                last_delta: delta,
                start,
                origin,
            },
            out,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resize_drag(
        &mut self,
        input: &Input<'_>,
        id: NodeId,
        handle: Handle,
        b: Rect,
        parent_origin: Point,
        pos: Point,
        mods: Mods,
    ) -> Vec<Out> {
        let (mx, my) = handle.moves();
        let (mut x0, mut y0, mut x1, mut y1) = (b.x, b.y, b.max_x(), b.max_y());
        let snapped = if mods.command {
            pos
        } else {
            let s = snap::snap_point(input.doc, input.page, input.selection, pos, input.px(5.0));
            self.guides = s.guides;
            Point::new(pos.x + s.dx, pos.y + s.dy)
        };
        let snapped = if input.pixel_grid {
            Point::new(snapped.x.round(), snapped.y.round())
        } else {
            snapped
        };
        if mx < 0 {
            x0 = snapped.x;
        } else if mx > 0 {
            x1 = snapped.x;
        }
        if my < 0 {
            y0 = snapped.y;
        } else if my > 0 {
            y1 = snapped.y;
        }
        if mods.alt {
            let c = b.center();
            if mx != 0 {
                let half = (snapped.x - c.x).abs();
                x0 = c.x - half;
                x1 = c.x + half;
            }
            if my != 0 {
                let half = (snapped.y - c.y).abs();
                y0 = c.y - half;
                y1 = c.y + half;
            }
        }
        if mods.shift && handle.is_corner() && b.width > 0.0 && b.height > 0.0 {
            let k = ((x1 - x0).abs() / b.width).max((y1 - y0).abs() / b.height);
            let (w, h) = (b.width * k, b.height * k);
            if mods.alt {
                let c = b.center();
                x0 = c.x - w / 2.0;
                x1 = c.x + w / 2.0;
                y0 = c.y - h / 2.0;
                y1 = c.y + h / 2.0;
            } else {
                let ax = if mx < 0 { b.max_x() } else { b.x };
                let ay = if my < 0 { b.max_y() } else { b.y };
                let sx = if (if mx < 0 { x0 } else { x1 }) >= ax {
                    1.0
                } else {
                    -1.0
                };
                let sy = if (if my < 0 { y0 } else { y1 }) >= ay {
                    1.0
                } else {
                    -1.0
                };
                x0 = ax;
                x1 = ax + sx * w;
                y0 = ay;
                y1 = ay + sy * h;
            }
        }
        let r = Rect::from_points(Point::new(x0, y0), Point::new(x1, y1));
        vec![run(
            "node.resize",
            json!({
                "id": id,
                "x": r.x - parent_origin.x,
                "y": r.y - parent_origin.y,
                "width": r.width.max(0.01),
                "height": r.height.max(0.01),
            }),
        )]
    }

    fn draw_tool(&mut self, input: &Input<'_>, ev: Pointer) -> Vec<Out> {
        let state = std::mem::replace(&mut self.state, State::Idle);
        let grid = |p: Point| {
            if input.pixel_grid {
                Point::new(p.x.round(), p.y.round())
            } else {
                p
            }
        };
        match (ev, state) {
            (Pointer::Down { pos, .. }, State::Idle) => {
                let pos = grid(pos);
                let parent = Self::container_at(input.doc, input.page, pos);
                self.state = State::Drawing {
                    start: pos,
                    current: pos,
                    parent,
                };
                Vec::new()
            }
            (Pointer::Move { pos, mods }, State::Drawing { start, parent, .. }) => {
                self.state = State::Drawing {
                    start,
                    current: grid(constrain(start, pos, mods, self.active)),
                    parent,
                };
                Vec::new()
            }
            (Pointer::Up { pos, mods }, State::Drawing { start, parent, .. }) => {
                let end = grid(constrain(start, pos, mods, self.active));
                let tiny = start.distance(end) * input.zoom < 2.0;
                let Some(cmd) = self.active.create_command() else {
                    return Vec::new();
                };
                let a = Self::to_local(input.doc, parent, input.page, start);
                let b = Self::to_local(input.doc, parent, input.page, end);
                let mut params = if self.active == ToolKind::Line {
                    let b = if tiny {
                        Point::new(a.x + 100.0, a.y)
                    } else {
                        b
                    };
                    json!({ "x": a.x, "y": a.y, "x2": b.x, "y2": b.y })
                } else {
                    let r = if tiny {
                        Rect::new(a.x - 50.0, a.y - 50.0, 100.0, 100.0)
                    } else {
                        Rect::from_points(a, b)
                    };
                    json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height })
                };
                if let Some(p) = parent {
                    params["parent"] = json!(p);
                }
                let out = vec![run(cmd, params), Out::SetTool(ToolKind::Move)];
                self.active = ToolKind::Move;
                out
            }
            (_, state) => {
                self.state = state;
                Vec::new()
            }
        }
    }

    fn text_tool(&mut self, input: &Input<'_>, ev: Pointer) -> Vec<Out> {
        match ev {
            Pointer::Down { pos, .. } => {
                if let Some(id) =
                    deesainer_vector::hit_test(input.doc, input.page, pos, input.px(2.0), true)
                {
                    if input
                        .doc
                        .get(id)
                        .is_some_and(|n| matches!(n.kind, NodeKind::Text(_)))
                    {
                        self.active = ToolKind::Move;
                        return vec![Out::SetTool(ToolKind::Move), Out::EditText(id)];
                    }
                }
                let parent = Self::container_at(input.doc, input.page, pos);
                let p = Self::to_local(input.doc, parent, input.page, pos);
                let mut params = json!({ "x": p.x, "y": p.y, "content": "" });
                if let Some(parent) = parent {
                    params["parent"] = json!(parent);
                }
                self.active = ToolKind::Move;
                vec![
                    Out::Run {
                        command: "node.create.text".into(),
                        params,
                        then: Then::EditText,
                    },
                    Out::SetTool(ToolKind::Move),
                ]
            }
            _ => Vec::new(),
        }
    }

    fn pen_tool(&mut self, input: &Input<'_>, ev: Pointer) -> Vec<Out> {
        let mut pen = match std::mem::replace(&mut self.state, State::Idle) {
            State::Pen(p) => p,
            _ => PenState::new(Self::container_at(
                input.doc,
                input.page,
                match ev {
                    Pointer::Down { pos, .. }
                    | Pointer::Move { pos, .. }
                    | Pointer::Up { pos, .. }
                    | Pointer::DoubleClick { pos, .. } => pos,
                },
            )),
        };
        let (done, out) = pen.pointer(input, ev);
        if done {
            self.active = ToolKind::Move;
            let mut out = out;
            out.push(Out::SetTool(ToolKind::Move));
            return out;
        }
        self.state = State::Pen(pen);
        out
    }

    pub fn finish_pen(&mut self) -> Vec<Out> {
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Pen(pen) => {
                let mut out = pen.finish();
                self.active = ToolKind::Move;
                out.push(Out::SetTool(ToolKind::Move));
                out
            }
            other => {
                self.state = other;
                Vec::new()
            }
        }
    }
}

#[derive(Debug, Clone)]
struct DropPlan {
    parent: NodeId,
    index: Option<usize>,
    from: NodeId,
    pinned: bool,
    pending: Point,
    ghost: Option<Rect>,
    line: Option<(Point, Point)>,
    target_bounds: Option<Rect>,
}

impl DropPlan {
    fn new(
        input: &Input<'_>,
        moved: Rect,
        pos: Point,
        delta: Point,
        applied: Point,
    ) -> Option<Self> {
        let doc = input.doc;
        let sel = input.selection;
        let from = doc.get(*sel.first()?)?.parent?;
        if sel
            .iter()
            .any(|s| doc.get(*s).and_then(|n| n.parent) != Some(from))
        {
            return None;
        }
        let from_node = doc.get(from)?;
        let movable_parent = from == input.page
            || matches!(from_node.kind, NodeKind::Frame | NodeKind::Component { .. });
        if !movable_parent {
            return None;
        }
        let in_flow = |id: &NodeId| {
            doc.get(*id)
                .is_some_and(|n| !n.layout_child.absolute && n.visible)
        };
        let pinned = from_node.layout.is_some() && sel.iter().all(in_flow);
        let parent = frame_under(doc, input.page, pos, sel).unwrap_or(input.page);
        let target = doc.get(parent)?;
        let target_bounds = (parent != from && parent != input.page)
            .then(|| doc.world_bounds(parent))
            .flatten();
        let (index, line) = match target.layout {
            Some(layout) if sel.iter().all(in_flow) => {
                let horizontal = layout.direction == deesainer_doc::LayoutDirection::Horizontal;
                let others: Vec<(usize, Rect)> = target
                    .children
                    .iter()
                    .filter(|c| !sel.contains(c))
                    .enumerate()
                    .filter(|(_, c)| in_flow(c))
                    .filter_map(|(i, c)| doc.world_bounds(*c).map(|b| (i, b)))
                    .collect();
                let along = |r: &Rect| {
                    if horizontal {
                        r.center().x
                    } else {
                        r.center().y
                    }
                };
                let p = if horizontal { pos.x } else { pos.y };
                let grid = layout.direction == deesainer_doc::LayoutDirection::Grid;
                let next = if grid {
                    others
                        .iter()
                        .find(|(_, r)| pos.y < r.y || (pos.y <= r.max_y() && pos.x < r.center().x))
                } else {
                    others.iter().find(|(_, r)| p < along(r))
                };
                let count = target.children.iter().filter(|c| !sel.contains(c)).count();
                let index = next.map_or(count, |(i, _)| *i);
                let frame = doc.world_bounds(parent)?;
                let gap = layout.gap.max(0.0) / 2.0;
                let at = match (next, others.last()) {
                    (Some((_, r)), _) if horizontal => r.x - gap,
                    (Some((_, r)), _) => r.y - gap,
                    (None, Some((_, r))) if horizontal => r.max_x() + gap,
                    (None, Some((_, r))) => r.max_y() + gap,
                    (None, None) if horizontal => frame.x + layout.padding.left,
                    (None, None) => frame.y + layout.padding.top,
                };
                let line = if grid {
                    let (x, r) = match (next, others.last()) {
                        (Some((_, r)), _) => (r.x - gap, *r),
                        (None, Some((_, r))) => (r.max_x() + gap, *r),
                        (None, None) => (frame.x + layout.padding.left, frame),
                    };
                    (Point::new(x, r.y), Point::new(x, r.max_y()))
                } else if horizontal {
                    (
                        Point::new(at, frame.y + layout.padding.top),
                        Point::new(at, frame.max_y() - layout.padding.bottom),
                    )
                } else {
                    (
                        Point::new(frame.x + layout.padding.left, at),
                        Point::new(frame.max_x() - layout.padding.right, at),
                    )
                };
                (Some(index), Some(line))
            }
            _ => (None, None),
        };
        Some(Self {
            parent,
            index,
            from,
            pinned,
            pending: delta - applied,
            ghost: pinned.then_some(moved),
            line,
            target_bounds,
        })
    }

    fn outs(&self, sel: &[NodeId]) -> Vec<Out> {
        let mut out = Vec::new();
        match self.index {
            Some(index) => {
                if self.parent == self.from {
                    for id in sel {
                        out.push(run(
                            "node.reparent",
                            json!({ "id": id, "parent": self.parent }),
                        ));
                    }
                }
                for (i, id) in sel.iter().enumerate() {
                    out.push(run(
                        "node.reparent",
                        json!({ "id": id, "parent": self.parent, "index": index + i }),
                    ));
                }
            }
            None if self.parent != self.from => {
                for id in sel {
                    out.push(run(
                        "node.reparent",
                        json!({ "id": id, "parent": self.parent }),
                    ));
                }
            }
            None => {}
        }
        if self.pinned && self.index.is_none() && self.pending != Point::ZERO {
            out.push(run(
                "node.move",
                json!({ "ids": sel, "dx": self.pending.x, "dy": self.pending.y }),
            ));
        }
        out
    }
}

fn frame_under(doc: &Document, page: NodeId, pos: Point, moving: &[NodeId]) -> Option<NodeId> {
    let mut found = None;
    let mut at = page;
    'down: for _ in 0..deesainer_doc::MAX_DEPTH {
        let node = doc.get(at)?;
        for child in node.children.iter().rev() {
            if moving.contains(child) {
                continue;
            }
            let Some(c) = doc.get(*child) else { continue };
            if !c.visible || !matches!(c.kind, NodeKind::Frame | NodeKind::Component { .. }) {
                continue;
            }
            if doc.world_bounds(*child).is_some_and(|b| b.contains(pos)) {
                found = Some(*child);
                at = *child;
                continue 'down;
            }
        }
        break;
    }
    found
}

fn constrain(start: Point, pos: Point, mods: Mods, tool: ToolKind) -> Point {
    if !mods.shift {
        return pos;
    }
    let d = pos - start;
    if tool == ToolKind::Line {
        let angle = d.y.atan2(d.x);
        let step = std::f64::consts::FRAC_PI_4;
        let a = (angle / step).round() * step;
        let len = start.distance(pos);
        return Point::new(start.x + len * a.cos(), start.y + len * a.sin());
    }
    let s = d.x.abs().max(d.y.abs());
    Point::new(start.x + s * d.x.signum(), start.y + s * d.y.signum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::{Node, Paint};

    fn doc_with_rect() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let page = doc.first_page();
        let mut r = Node::new(
            doc.next_free_id(),
            "R",
            NodeKind::Rectangle,
            Rect::new(10.0, 10.0, 50.0, 50.0),
        );
        r.fills.push(Paint::solid(deesainer_color::Rgba::BLACK));
        let id = r.id;
        doc.insert(r, page, 0).unwrap();
        (doc, page, id)
    }

    fn cmds(out: &[Out]) -> Vec<&str> {
        out.iter()
            .filter_map(|o| match o {
                Out::Run { command, .. } => Some(command.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn click_selects_and_drag_moves_in_one_group() {
        let (doc, page, id) = doc_with_rect();
        let mut t = Tools::new();
        let none: Vec<NodeId> = vec![];
        let input = Input {
            doc: &doc,
            page,
            selection: &none,
            zoom: 1.0,
            pixel_grid: false,
        };
        let out = t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(20.0, 20.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(cmds(&out), vec!["selection.set"]);
        let sel = vec![id];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: false,
        };
        let out = t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(40.0, 20.0),
                mods: Mods {
                    command: true,
                    ..Mods::default()
                },
            },
        );
        assert_eq!(out.first(), Some(&Out::BeginGroup("Move".into())));
        assert_eq!(cmds(&out), vec!["node.move"]);
        let out = t.pointer(
            &input,
            Pointer::Up {
                pos: Point::new(40.0, 20.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(out, vec![Out::EndGroup]);
    }

    #[test]
    fn a_deep_selection_is_dragged_not_its_frame() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let outer = Node::new(
            doc.next_free_id(),
            "Outer",
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        let outer_id = outer.id;
        doc.insert(outer, page, 0).unwrap();
        let inner = Node::new(
            doc.next_free_id(),
            "Inner",
            NodeKind::Frame,
            Rect::new(10.0, 10.0, 100.0, 100.0),
        );
        let inner_id = inner.id;
        doc.insert(inner, outer_id, 0).unwrap();
        let mut r = Node::new(
            doc.next_free_id(),
            "R",
            NodeKind::Rectangle,
            Rect::new(10.0, 10.0, 30.0, 30.0),
        );
        r.fills.push(Paint::solid(deesainer_color::Rgba::BLACK));
        let rid = r.id;
        doc.insert(r, inner_id, 0).unwrap();
        let sel = vec![rid];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: false,
        };
        let mut t = Tools::new();
        let out = t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(30.0, 30.0),
                mods: Mods::default(),
            },
        );
        assert!(cmds(&out).is_empty(), "{out:?}");
        let out = t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(50.0, 30.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(cmds(&out), vec!["node.move"]);
    }

    #[test]
    fn empty_click_marquee_selects() {
        let (doc, page, _) = doc_with_rect();
        let mut t = Tools::new();
        let none: Vec<NodeId> = vec![];
        let input = Input {
            doc: &doc,
            page,
            selection: &none,
            zoom: 1.0,
            pixel_grid: false,
        };
        t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(0.0, 0.0),
                mods: Mods::default(),
            },
        );
        let out = t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(30.0, 30.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(cmds(&out), vec!["selection.set"]);
        assert!(matches!(t.overlays().last(), Some(Overlay::Marquee(_))));
    }

    #[test]
    fn rectangle_tool_creates_inside_frame_and_returns_to_move() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let frame = Node::new(
            doc.next_free_id(),
            "F",
            NodeKind::Frame,
            Rect::new(100.0, 100.0, 200.0, 200.0),
        );
        let fid = frame.id;
        doc.insert(frame, page, 0).unwrap();
        let mut t = Tools::new();
        t.set_tool(ToolKind::Rectangle);
        let none: Vec<NodeId> = vec![];
        let input = Input {
            doc: &doc,
            page,
            selection: &none,
            zoom: 1.0,
            pixel_grid: false,
        };
        t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(110.0, 110.0),
                mods: Mods::default(),
            },
        );
        t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(150.0, 130.0),
                mods: Mods::default(),
            },
        );
        let out = t.pointer(
            &input,
            Pointer::Up {
                pos: Point::new(150.0, 130.0),
                mods: Mods::default(),
            },
        );
        match &out[0] {
            Out::Run {
                command, params, ..
            } => {
                assert_eq!(command, "node.create.rectangle");
                assert_eq!(params["parent"], json!(fid));
                assert_eq!(params["x"], json!(10.0));
                assert_eq!(params["width"], json!(40.0));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(t.active, ToolKind::Move);
    }

    #[test]
    fn handles_resize_selected_layer() {
        let (doc, page, id) = doc_with_rect();
        let mut t = Tools::new();
        let sel = vec![id];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: false,
        };
        let out = t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(60.0, 60.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(out, vec![Out::BeginGroup("Resize".into())]);
        let out = t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(80.0, 90.0),
                mods: Mods {
                    command: true,
                    ..Mods::default()
                },
            },
        );
        match &out[0] {
            Out::Run {
                command, params, ..
            } => {
                assert_eq!(command, "node.resize");
                assert_eq!(params["width"], json!(70.0));
                assert_eq!(params["height"], json!(80.0));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    fn params_of<'o>(out: &'o [Out], command: &str) -> &'o Value {
        out.iter()
            .find_map(|o| match o {
                Out::Run {
                    command: c, params, ..
                } if c == command => Some(params),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn the_pixel_grid_keeps_moves_and_new_shapes_on_whole_pixels() {
        let (doc, page, id) = doc_with_rect();
        let mut t = Tools::new();
        let sel = vec![id];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: true,
        };
        let down = Pointer::Down {
            pos: Point::new(20.0, 20.0),
            mods: Mods::default(),
        };
        t.pointer(&input, down);
        let free = Mods {
            command: true,
            ..Mods::default()
        };
        let out = t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(40.3, 20.6),
                mods: free,
            },
        );
        let p = params_of(&out, "node.move");
        assert_eq!(
            (p["dx"].as_f64(), p["dy"].as_f64()),
            (Some(20.0), Some(1.0))
        );
        t.pointer(
            &input,
            Pointer::Up {
                pos: Point::new(40.3, 20.6),
                mods: free,
            },
        );
        let none: Vec<NodeId> = vec![];
        let input = Input {
            selection: &none,
            ..input
        };
        t.set_tool(ToolKind::Rectangle);
        t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(200.4, 200.6),
                mods: Mods::default(),
            },
        );
        let out = t.pointer(
            &input,
            Pointer::Up {
                pos: Point::new(250.7, 240.2),
                mods: Mods::default(),
            },
        );
        let p = params_of(&out, "node.create.rectangle");
        for (k, v) in [
            ("x", 200.0),
            ("y", 201.0),
            ("width", 51.0),
            ("height", 39.0),
        ] {
            assert_eq!(p[k].as_f64(), Some(v), "{k}: {p}");
        }
    }

    fn filled(doc: &mut Document, parent: NodeId, kind: NodeKind, r: Rect) -> NodeId {
        let mut n = Node::new(doc.next_free_id(), "N", kind, r);
        n.fills.push(Paint::solid(deesainer_color::Rgba::BLACK));
        let id = n.id;
        let at = doc.get(parent).map_or(0, |p| p.children.len());
        doc.insert(n, parent, at).unwrap();
        id
    }

    fn drag(t: &mut Tools, input: &Input<'_>, from: Point, to: Point) -> Vec<Out> {
        let mut out = t.pointer(
            input,
            Pointer::Down {
                pos: from,
                mods: Mods::default(),
            },
        );
        let free = Mods {
            command: true,
            ..Mods::default()
        };
        out.extend(t.pointer(
            input,
            Pointer::Move {
                pos: to,
                mods: free,
            },
        ));
        out.extend(t.pointer(
            input,
            Pointer::Up {
                pos: to,
                mods: free,
            },
        ));
        out
    }

    #[test]
    fn dragging_in_auto_layout_reorders_instead_of_moving() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let f = filled(
            &mut doc,
            page,
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 300.0, 100.0),
        );
        doc.get_mut(f).unwrap().layout = Some(deesainer_doc::AutoLayout {
            direction: deesainer_doc::LayoutDirection::Horizontal,
            gap: 10.0,
            ..deesainer_doc::AutoLayout::default()
        });
        let a = filled(
            &mut doc,
            f,
            NodeKind::Rectangle,
            Rect::new(10.0, 10.0, 50.0, 50.0),
        );
        let _b = filled(
            &mut doc,
            f,
            NodeKind::Rectangle,
            Rect::new(70.0, 10.0, 50.0, 50.0),
        );
        let _c = filled(
            &mut doc,
            f,
            NodeKind::Rectangle,
            Rect::new(130.0, 10.0, 50.0, 50.0),
        );
        let sel = vec![a];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: false,
        };
        let mut t = Tools::new();
        t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(35.0, 35.0),
                mods: Mods::default(),
            },
        );
        let free = Mods {
            command: true,
            ..Mods::default()
        };
        let moved = t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(150.0, 35.0),
                mods: free,
            },
        );
        assert!(!cmds(&moved).contains(&"node.move"), "{moved:?}");
        let overlays = t.overlays();
        assert!(overlays.iter().any(|o| matches!(o, Overlay::Ghost(_))));
        assert!(overlays.iter().any(|o| matches!(o, Overlay::Insert(..))));
        let up = t.pointer(
            &input,
            Pointer::Up {
                pos: Point::new(150.0, 35.0),
                mods: free,
            },
        );
        let last = up
            .iter()
            .rev()
            .filter_map(|o| match o {
                Out::Run {
                    command, params, ..
                } if command == "node.reparent" => Some(params),
                _ => None,
            })
            .next()
            .unwrap();
        assert_eq!(last["index"], json!(1));
        assert_eq!(last["parent"], json!(f));
        assert_eq!(up.last(), Some(&Out::EndGroup));
    }

    #[test]
    fn dropping_a_layer_on_a_frame_moves_it_inside() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let f = filled(
            &mut doc,
            page,
            NodeKind::Frame,
            Rect::new(200.0, 0.0, 200.0, 200.0),
        );
        let r = filled(
            &mut doc,
            page,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 40.0, 40.0),
        );
        let sel = vec![r];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: false,
        };
        let mut t = Tools::new();
        let out = drag(
            &mut t,
            &input,
            Point::new(20.0, 20.0),
            Point::new(300.0, 100.0),
        );
        assert!(cmds(&out).contains(&"node.move"));
        let reparent = params_of(&out, "node.reparent");
        assert_eq!(reparent["parent"], json!(f));
        let mut t = Tools::new();
        let out = drag(
            &mut t,
            &input,
            Point::new(20.0, 20.0),
            Point::new(60.0, 300.0),
        );
        assert!(!cmds(&out).contains(&"node.reparent"));
    }

    #[test]
    fn moving_between_two_layers_shows_spacing_guides() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let mut ids = Vec::new();
        for (i, x) in [0.0, 200.0, 90.0].into_iter().enumerate() {
            let y = if i == 2 { 300.0 } else { 0.0 };
            let mut n = Node::new(
                doc.next_free_id(),
                "R",
                NodeKind::Rectangle,
                Rect::new(x, y, 40.0, 40.0),
            );
            n.fills.push(Paint::solid(deesainer_color::Rgba::BLACK));
            ids.push(n.id);
            doc.insert(n, page, i).unwrap();
        }
        let mut t = Tools::new();
        let sel = vec![ids[2]];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: true,
        };
        t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(100.0, 310.0),
                mods: Mods::default(),
            },
        );
        t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(108.0, 17.0),
                mods: Mods::default(),
            },
        );
        let gaps = t
            .overlays()
            .into_iter()
            .filter(|o| matches!(o, Overlay::Gap(..)))
            .count();
        assert_eq!(gaps, 2);
    }
}

#[cfg(test)]
mod path_edit_tests {
    use super::*;
    use deesainer_doc::{Node, PathCmd, VectorPath};

    fn cubic_doc() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let page = doc.first_page();
        let path = VectorPath {
            cmds: vec![
                PathCmd::MoveTo {
                    to: Point::new(0.0, 0.0),
                },
                PathCmd::CubicTo {
                    ctrl1: Point::new(30.0, 30.0),
                    ctrl2: Point::new(70.0, 30.0),
                    to: Point::new(100.0, 0.0),
                },
            ],
            ..VectorPath::default()
        };
        let v = Node::new(
            doc.next_free_id(),
            "Curve",
            NodeKind::Vector { path },
            Rect::new(10.0, 10.0, 100.0, 30.0),
        );
        let id = v.id;
        doc.insert(v, page, 0).unwrap();
        (doc, page, id)
    }

    fn emitted_path(out: &[Out]) -> Vec<PathCmd> {
        let Some(Out::Run {
            command, params, ..
        }) = out.first()
        else {
            panic!("expected a path command");
        };
        assert_eq!(command, "vector.setPath");
        serde_json::from_value(params["cmds"].clone()).unwrap()
    }

    #[test]
    fn cubic_handles_are_visible_and_drag_independently() {
        let (doc, page, id) = cubic_doc();
        let selection = vec![id];
        let input = Input {
            doc: &doc,
            page,
            selection: &selection,
            zoom: 1.0,
            pixel_grid: false,
        };
        let mut tools = Tools::new();
        tools.path_edit = Some(id);

        let overlays = tools.doc_overlays(&doc);
        assert!(overlays.contains(&Overlay::PreviewLine(
            Point::new(10.0, 10.0),
            Point::new(40.0, 40.0)
        )));
        assert!(overlays.iter().any(|overlay| matches!(
            overlay,
            Overlay::Anchors(points, _) if points.contains(&Point::new(40.0, 40.0))
        )));

        assert_eq!(
            tools.pointer(
                &input,
                Pointer::Down {
                    pos: Point::new(40.0, 40.0),
                    mods: Mods::default()
                }
            ),
            vec![Out::BeginGroup("Edit path".into())]
        );
        let path = emitted_path(&tools.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(50.0, 60.0),
                mods: Mods::default(),
            },
        ));
        assert_eq!(
            path[1],
            PathCmd::CubicTo {
                ctrl1: Point::new(40.0, 50.0),
                ctrl2: Point::new(70.0, 30.0),
                to: Point::new(100.0, 0.0),
            }
        );
        assert_eq!(
            tools.pointer(
                &input,
                Pointer::Up {
                    pos: Point::new(50.0, 60.0),
                    mods: Mods::default()
                }
            ),
            vec![Out::EndGroup]
        );

        assert_eq!(tools.cursor(&input, Point::new(80.0, 40.0)), Cursor::Grab);
        tools.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(80.0, 40.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(
            tools.cursor(&input, Point::new(80.0, 40.0)),
            Cursor::Grabbing
        );
        let path = emitted_path(&tools.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(90.0, 50.0),
                mods: Mods::default(),
            },
        ));
        assert_eq!(
            path[1],
            PathCmd::CubicTo {
                ctrl1: Point::new(30.0, 30.0),
                ctrl2: Point::new(80.0, 40.0),
                to: Point::new(100.0, 0.0),
            }
        );
        assert_eq!(tools.set_tool(ToolKind::Rectangle), vec![Out::EndGroup]);
        assert_eq!(tools.editing_path(), None);
        assert!(tools.doc_overlays(&doc).is_empty());
    }

    #[test]
    fn quadratic_control_moves_without_moving_anchors() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let path = VectorPath {
            cmds: vec![
                PathCmd::MoveTo {
                    to: Point::new(0.0, 0.0),
                },
                PathCmd::QuadTo {
                    ctrl: Point::new(50.0, 40.0),
                    to: Point::new(100.0, 0.0),
                },
            ],
            ..VectorPath::default()
        };
        let v = Node::new(
            doc.next_free_id(),
            "Quadratic",
            NodeKind::Vector { path },
            Rect::new(10.0, 10.0, 100.0, 40.0),
        );
        let id = v.id;
        doc.insert(v, page, 0).unwrap();
        let selection = vec![id];
        let input = Input {
            doc: &doc,
            page,
            selection: &selection,
            zoom: 2.0,
            pixel_grid: false,
        };
        let mut tools = Tools::new();
        tools.path_edit = Some(id);
        assert_eq!(
            tools.pointer(
                &input,
                Pointer::Down {
                    pos: Point::new(60.0, 50.0),
                    mods: Mods::default()
                }
            ),
            vec![Out::BeginGroup("Edit path".into())]
        );
        let path = emitted_path(&tools.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(60.0, 65.0),
                mods: Mods::default(),
            },
        ));
        assert_eq!(
            path,
            vec![
                PathCmd::MoveTo {
                    to: Point::new(0.0, 0.0)
                },
                PathCmd::QuadTo {
                    ctrl: Point::new(50.0, 55.0),
                    to: Point::new(100.0, 0.0)
                }
            ]
        );
    }

    #[test]
    fn moving_anchor_carries_both_attached_cubic_handles() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let path = VectorPath {
            cmds: vec![
                PathCmd::MoveTo {
                    to: Point::new(0.0, 0.0),
                },
                PathCmd::CubicTo {
                    ctrl1: Point::new(30.0, 30.0),
                    ctrl2: Point::new(70.0, 30.0),
                    to: Point::new(100.0, 0.0),
                },
                PathCmd::CubicTo {
                    ctrl1: Point::new(130.0, -30.0),
                    ctrl2: Point::new(170.0, -30.0),
                    to: Point::new(200.0, 0.0),
                },
            ],
            ..VectorPath::default()
        };
        let v = Node::new(
            doc.next_free_id(),
            "S curve",
            NodeKind::Vector { path },
            Rect::new(10.0, 10.0, 200.0, 60.0),
        );
        let id = v.id;
        doc.insert(v, page, 0).unwrap();
        let selection = vec![id];
        let input = Input {
            doc: &doc,
            page,
            selection: &selection,
            zoom: 1.0,
            pixel_grid: false,
        };
        let mut tools = Tools::new();
        tools.path_edit = Some(id);
        tools.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(110.0, 10.0),
                mods: Mods::default(),
            },
        );
        let path = emitted_path(&tools.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(120.0, 15.0),
                mods: Mods::default(),
            },
        ));
        assert_eq!(
            path[1],
            PathCmd::CubicTo {
                ctrl1: Point::new(30.0, 30.0),
                ctrl2: Point::new(80.0, 35.0),
                to: Point::new(110.0, 5.0),
            }
        );
        assert_eq!(
            path[2],
            PathCmd::CubicTo {
                ctrl1: Point::new(140.0, -25.0),
                ctrl2: Point::new(170.0, -30.0),
                to: Point::new(200.0, 0.0),
            }
        );
    }

    #[test]
    fn double_click_vector_then_drag_anchor() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let path = VectorPath {
            cmds: vec![
                PathCmd::MoveTo {
                    to: Point::new(0.0, 0.0),
                },
                PathCmd::LineTo {
                    to: Point::new(100.0, 0.0),
                },
            ],
            ..VectorPath::default()
        };
        let mut v = Node::new(
            doc.next_free_id(),
            "V",
            NodeKind::Vector { path },
            Rect::new(10.0, 10.0, 100.0, 0.0),
        );
        v.strokes
            .push(deesainer_doc::Paint::solid(deesainer_color::Rgba::BLACK));
        let id = v.id;
        doc.insert(v, page, 0).unwrap();
        let mut t = Tools::new();
        let sel = vec![id];
        let input = Input {
            doc: &doc,
            page,
            selection: &sel,
            zoom: 1.0,
            pixel_grid: false,
        };
        t.pointer(
            &input,
            Pointer::DoubleClick {
                pos: Point::new(50.0, 10.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(t.editing_path(), Some(id));
        let out = t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(110.0, 10.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(out, vec![Out::BeginGroup("Edit path".into())]);
        let out = t.pointer(
            &input,
            Pointer::Move {
                pos: Point::new(110.0, 40.0),
                mods: Mods::default(),
            },
        );
        assert!(matches!(&out[0], Out::Run { command, .. } if command == "vector.setPath"));
        assert_eq!(
            t.pointer(
                &input,
                Pointer::Up {
                    pos: Point::new(110.0, 40.0),
                    mods: Mods::default()
                }
            ),
            vec![Out::EndGroup]
        );
        let out = t.pointer(
            &input,
            Pointer::Down {
                pos: Point::new(60.0, 10.0),
                mods: Mods::default(),
            },
        );
        assert_eq!(out[0], Out::BeginGroup("Add point".into()));
        let Out::Run {
            command, params, ..
        } = &out[1]
        else {
            panic!("expected a path update");
        };
        assert_eq!(command, "vector.setPath");
        assert_eq!(params["cmds"].as_array().map(Vec::len), Some(3));
        assert_eq!(t.editing_path(), Some(id));
    }

    fn zigzag() -> (Document, NodeId) {
        use deesainer_doc::PathCmd as C;
        let mut doc = Document::new();
        let page = doc.first_page();
        let pt = |x, y| Point::new(x, y);
        let path = VectorPath {
            cmds: vec![
                C::MoveTo { to: pt(0.0, 0.0) },
                C::LineTo { to: pt(50.0, 40.0) },
                C::LineTo { to: pt(100.0, 0.0) },
            ],
            ..VectorPath::default()
        };
        let v = Node::new(
            doc.next_free_id(),
            "Z",
            NodeKind::Vector { path },
            Rect::new(0.0, 0.0, 100.0, 40.0),
        );
        let id = v.id;
        doc.insert(v, page, 0).unwrap();
        (doc, id)
    }

    #[test]
    fn middle_anchor_toggles_between_corner_and_curve() {
        use deesainer_doc::PathCmd as C;
        let (doc, id) = zigzag();
        let curved = Tools::toggle_curve(&doc, id, 1).unwrap();
        assert!(matches!(curved[1], C::CubicTo { .. }));
        assert!(matches!(curved[2], C::CubicTo { .. }));
        let (C::CubicTo { ctrl2: a, to, .. }, C::CubicTo { ctrl1: b, .. }) =
            (&curved[1], &curved[2])
        else {
            panic!("expected curves");
        };
        let cross = (to.x - a.x) * (b.y - to.y) - (to.y - a.y) * (b.x - to.x);
        assert!(cross.abs() < 1e-9);
        let mut doc2 = doc.clone();
        if let Some(n) = doc2.get_mut(id) {
            n.kind = NodeKind::Vector {
                path: VectorPath {
                    cmds: curved,
                    ..VectorPath::default()
                },
            };
        }
        let flat = Tools::toggle_curve(&doc2, id, 1).unwrap();
        assert!(matches!(&flat[1], C::CubicTo { ctrl2, to, .. } if ctrl2 == to));
    }

    #[test]
    fn delete_removes_the_selected_anchor_but_keeps_two_points() {
        let (doc, id) = zigzag();
        let mut t = Tools::new();
        t.path_edit = Some(id);
        t.selected_anchor = Some(1);
        let out = t.delete_selected_point(&doc);
        let Out::Run { params, .. } = &out[1] else {
            panic!("expected an update");
        };
        assert_eq!(params["cmds"].as_array().map(Vec::len), Some(2));
        assert_eq!(params["cmds"][1]["to"]["x"], 100.0);
        let mut small = doc.clone();
        if let Some(n) = small.get_mut(id) {
            n.kind = NodeKind::Vector {
                path: VectorPath {
                    cmds: vec![
                        deesainer_doc::PathCmd::MoveTo {
                            to: Point::new(0.0, 0.0),
                        },
                        deesainer_doc::PathCmd::LineTo {
                            to: Point::new(9.0, 0.0),
                        },
                    ],
                    ..VectorPath::default()
                },
            };
        }
        t.selected_anchor = Some(1);
        assert!(t.delete_selected_point(&small).is_empty());
    }
}
