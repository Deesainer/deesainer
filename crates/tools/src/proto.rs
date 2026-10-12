use deesainer_doc::{
    Action, Direction, Document, Easing, Interaction, NodeId, NodeKind, TransitionKind, Trigger,
};
use deesainer_geom::Point;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveTransition {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: TransitionKind,
    pub easing: Easing,
    pub duration: f64,
    pub elapsed: f64,
    pub reverse: bool,
}

impl ActiveTransition {
    pub fn progress(&self) -> f64 {
        if self.duration <= 0.0 {
            1.0
        } else {
            self.easing.apply(self.elapsed / self.duration)
        }
    }

    pub fn offsets(&self) -> ((f64, f64), (f64, f64)) {
        let p = self.progress();
        let dir = match self.kind {
            TransitionKind::MoveIn { direction } | TransitionKind::Push { direction } => direction,
            _ => return ((0.0, 0.0), (0.0, 0.0)),
        };
        let (mut dx, mut dy) = match dir {
            Direction::Left => (1.0, 0.0),
            Direction::Right => (-1.0, 0.0),
            Direction::Up => (0.0, 1.0),
            Direction::Down => (0.0, -1.0),
        };
        if self.reverse {
            dx = -dx;
            dy = -dy;
        }
        let incoming = (dx * (1.0 - p), dy * (1.0 - p));
        let outgoing = if matches!(self.kind, TransitionKind::Push { .. }) {
            (-dx * p, -dy * p)
        } else {
            (0.0, 0.0)
        };
        (incoming, outgoing)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Navigated(NodeId),
    OverlayOpened(NodeId),
    OverlayClosed,
    OpenUrl(String),
    VariableSet(deesainer_doc::VariableId),
}

#[derive(Debug, Clone)]
pub struct Player {
    pub page: NodeId,
    pub current: NodeId,
    history: Vec<NodeId>,
    pub overlays: Vec<NodeId>,
    pub transition: Option<ActiveTransition>,
    timers: Vec<(f64, Interaction)>,
    hovered: Option<NodeId>,
    pub variables: std::collections::BTreeMap<deesainer_doc::VariableId, deesainer_doc::VarValue>,
}

pub fn frames(doc: &Document, page: NodeId) -> Vec<NodeId> {
    doc.get(page)
        .map(|p| {
            p.children
                .iter()
                .copied()
                .filter(|c| {
                    doc.get(*c)
                        .is_some_and(|n| n.kind.is_frame_like() && n.visible)
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn flow_starts(doc: &Document, page: NodeId) -> Vec<NodeId> {
    doc.get(page)
        .map(|p| {
            p.interactions
                .iter()
                .filter_map(|i| match i.action {
                    Action::Navigate { to } => Some(to),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Player {
    pub fn start(doc: &Document, page: NodeId, frame: Option<NodeId>) -> Option<Player> {
        let current = frame
            .filter(|f| doc.contains(*f))
            .or_else(|| flow_starts(doc, page).first().copied())
            .or_else(|| frames(doc, page).first().copied())?;
        let mut p = Player {
            page,
            current,
            history: Vec::new(),
            overlays: Vec::new(),
            transition: None,
            timers: Vec::new(),
            hovered: None,
            variables: Default::default(),
        };
        p.arm_timers(doc, current);
        Some(p)
    }

    pub fn top(&self) -> NodeId {
        self.overlays.last().copied().unwrap_or(self.current)
    }

    fn arm_timers(&mut self, doc: &Document, frame: NodeId) {
        self.timers.clear();
        for id in doc.subtree_pre_order(frame).unwrap_or_default() {
            if let Some(n) = doc.get(id) {
                for i in &n.interactions {
                    if let Trigger::AfterDelay { ms } = i.trigger {
                        self.timers.push((f64::from(ms) / 1000.0, i.clone()));
                    }
                }
            }
        }
    }

    fn perform(&mut self, doc: &Document, i: &Interaction) -> Option<Event> {
        self.run_action(doc, i, &i.action, 0)
    }

    pub fn variable_value(
        &self,
        doc: &Document,
        id: deesainer_doc::VariableId,
    ) -> Option<deesainer_doc::VarValue> {
        self.variables
            .get(&id)
            .cloned()
            .or_else(|| doc.resolve_variable(id).cloned())
    }

    fn run_action(
        &mut self,
        doc: &Document,
        i: &Interaction,
        action: &Action,
        depth: usize,
    ) -> Option<Event> {
        match action {
            Action::SetVariable { variable, value } => {
                let fits = doc
                    .resolve_variable(*variable)
                    .is_some_and(|v| v.type_name() == value.type_name());
                if !fits {
                    return None;
                }
                self.variables.insert(*variable, value.clone());
                return Some(Event::VariableSet(*variable));
            }
            Action::Conditional {
                condition,
                then,
                otherwise,
            } => {
                if depth > 8 {
                    return None;
                }
                let holds = self
                    .variable_value(doc, condition.variable)
                    .is_some_and(|v| condition.holds(&v));
                let branch = if holds { then } else { otherwise };
                let mut last = None;
                for a in branch.iter().take(32) {
                    if let Some(e) = self.run_action(doc, i, a, depth + 1) {
                        last = Some(e);
                    }
                }
                return last;
            }
            _ => {}
        }
        match action {
            Action::Navigate { to } => {
                if !doc.contains(*to) || *to == self.current {
                    return None;
                }
                self.history.push(self.current);
                self.go(doc, *to, i, false);
                Some(Event::Navigated(*to))
            }
            Action::Back => {
                let prev = self.history.pop()?;
                self.go(doc, prev, i, true);
                Some(Event::Navigated(prev))
            }
            Action::OpenOverlay { frame } => {
                if !doc.contains(*frame) {
                    return None;
                }
                self.overlays.push(*frame);
                Some(Event::OverlayOpened(*frame))
            }
            Action::CloseOverlay => {
                self.overlays.pop()?;
                Some(Event::OverlayClosed)
            }
            Action::OpenUrl { url } => Some(Event::OpenUrl(url.clone())),
            Action::SetVariable { .. } | Action::Conditional { .. } => None,
        }
    }

    fn go(&mut self, doc: &Document, to: NodeId, i: &Interaction, reverse: bool) {
        let from = self.current;
        self.current = to;
        self.overlays.clear();
        let duration = f64::from(i.transition.duration_ms) / 1000.0;
        self.transition = (i.transition.kind != TransitionKind::Instant && duration > 0.0)
            .then_some(ActiveTransition {
                from,
                to,
                kind: i.transition.kind,
                easing: i.transition.easing,
                duration,
                elapsed: 0.0,
                reverse,
            });
        self.arm_timers(doc, to);
    }

    fn find(&self, doc: &Document, p: Point, wants: fn(&Trigger) -> bool) -> Option<Interaction> {
        let top = self.top();
        let origin = doc.world_bounds(top)?.origin();
        let page_point = Point::new(origin.x + p.x, origin.y + p.y);
        let path = deesainer_vector::hit_path(doc, self.page, page_point, 0.0);
        let start = path.iter().position(|id| *id == top)?;
        for id in path[start..].iter().rev() {
            if let Some(i) = doc
                .get(*id)
                .and_then(|n| n.interactions.iter().find(|i| wants(&i.trigger)))
            {
                return Some(i.clone());
            }
        }
        None
    }

    pub fn click(&mut self, doc: &Document, p: Point) -> Option<Event> {
        if self.transition.is_some() {
            return None;
        }
        let i = self.find(doc, p, |t| matches!(t, Trigger::OnClick | Trigger::OnPress))?;
        self.perform(doc, &i)
    }

    pub fn hover(&mut self, doc: &Document, p: Point) -> Option<Event> {
        if self.transition.is_some() {
            return None;
        }
        let i = self.find(doc, p, |t| matches!(t, Trigger::OnHover));
        let key = i.as_ref().map(|_| self.top());
        if i.is_none() {
            self.hovered = None;
            return None;
        }
        if self.hovered == key {
            return None;
        }
        self.hovered = key;
        i.and_then(|i| self.perform(doc, &i))
    }

    pub fn tick(&mut self, doc: &Document, dt: f64) -> Vec<Event> {
        if let Some(t) = &mut self.transition {
            t.elapsed += dt.max(0.0);
            if t.elapsed >= t.duration {
                self.transition = None;
            }
            return Vec::new();
        }
        let mut fired = Vec::new();
        let mut due = Vec::new();
        for (remaining, i) in &mut self.timers {
            *remaining -= dt;
            if *remaining <= 0.0 {
                due.push(i.clone());
            }
        }
        self.timers.retain(|(r, _)| *r > 0.0);
        for i in due {
            if let Some(e) = self.perform(doc, &i) {
                fired.push(e);
                break;
            }
        }
        fired
    }

    pub fn targets(doc: &Document, frame: NodeId) -> Vec<(NodeId, NodeId)> {
        let mut out = Vec::new();
        for id in doc.subtree_pre_order(frame).unwrap_or_default() {
            let Some(n) = doc.get(id) else { continue };
            if matches!(n.kind, NodeKind::Page | NodeKind::Document) {
                continue;
            }
            for i in &n.interactions {
                match i.action {
                    Action::Navigate { to } | Action::OpenOverlay { frame: to } => {
                        out.push((id, to))
                    }
                    _ => {}
                }
            }
        }
        out
    }
}

pub type ScrollOffsets = std::collections::BTreeMap<NodeId, (f64, f64)>;

pub fn scroll_range(doc: &Document, frame: NodeId) -> (f64, f64) {
    let Some(f) = doc.get(frame) else {
        return (0.0, 0.0);
    };
    let (sx, sy) = f.scrolling.overflow.axes();
    let (mut w, mut h) = (0.0f64, 0.0f64);
    for c in f.children.iter().filter_map(|c| doc.get(*c)) {
        if c.scrolling.fixed || !c.visible {
            continue;
        }
        w = w.max(c.rect.max_x());
        h = h.max(c.rect.max_y());
    }
    let pad = f
        .layout
        .map(|l| (l.padding.right, l.padding.bottom))
        .unwrap_or((0.0, 0.0));
    (
        if sx {
            (w + pad.0 - f.rect.width).max(0.0)
        } else {
            0.0
        },
        if sy {
            (h + pad.1 - f.rect.height).max(0.0)
        } else {
            0.0
        },
    )
}

pub fn clamp_scroll(doc: &Document, offsets: &mut ScrollOffsets) {
    for (frame, (x, y)) in offsets.iter_mut() {
        let (rx, ry) = scroll_range(doc, *frame);
        *x = x.clamp(0.0, rx);
        *y = y.clamp(0.0, ry);
    }
}

pub fn scrolled_view<'a>(
    doc: &'a Document,
    offsets: &ScrollOffsets,
) -> std::borrow::Cow<'a, Document> {
    if offsets.values().all(|(x, y)| *x == 0.0 && *y == 0.0) {
        return std::borrow::Cow::Borrowed(doc);
    }
    let mut view = doc.clone();
    for (frame, (x, y)) in offsets {
        let Some(children) = doc.get(*frame).map(|f| f.children.clone()) else {
            continue;
        };
        let mut fixed = Vec::new();
        for c in &children {
            let Some(node) = view.get_mut(*c) else {
                continue;
            };
            if node.scrolling.fixed {
                fixed.push(*c);
            } else {
                node.rect.x -= x;
                node.rect.y -= y;
            }
        }
        let n = children.len();
        for c in fixed {
            let _ = view.move_node(c, *frame, n.saturating_sub(1));
        }
    }
    std::borrow::Cow::Owned(view)
}

pub fn scroll_target(doc: &Document, page: NodeId, top: NodeId, p: Point) -> Option<NodeId> {
    let origin = doc.world_bounds(top)?.origin();
    let page_point = Point::new(origin.x + p.x, origin.y + p.y);
    let mut path = deesainer_vector::hit_path(doc, page, page_point, 0.0);
    if !path.contains(&top) {
        path = vec![top];
    }
    path.iter()
        .rev()
        .copied()
        .chain(std::iter::once(top))
        .find(|id| {
            doc.get(*id)
                .is_some_and(|n| n.scrolling.overflow != deesainer_doc::Overflow::None)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::{Node, Paint, Transition};
    use deesainer_geom::Rect;

    fn setup() -> (Document, NodeId, NodeId, NodeId, NodeId) {
        let mut doc = Document::new();
        let page = doc.first_page();
        let a = Node::new(
            doc.next_free_id(),
            "Home",
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        let aid = a.id;
        doc.insert(a, page, 0).unwrap();
        let b = Node::new(
            doc.next_free_id(),
            "Details",
            NodeKind::Frame,
            Rect::new(200.0, 0.0, 100.0, 100.0),
        );
        let bid = b.id;
        doc.insert(b, page, 1).unwrap();
        let mut button = Node::new(
            doc.next_free_id(),
            "Button",
            NodeKind::Rectangle,
            Rect::new(10.0, 10.0, 30.0, 20.0),
        );
        button
            .fills
            .push(Paint::solid(deesainer_color::Rgba::BLACK));
        button.interactions.push(Interaction {
            trigger: Trigger::OnClick,
            action: Action::Navigate { to: bid },
            transition: Transition {
                kind: TransitionKind::SmartAnimate,
                duration_ms: 300,
                easing: Easing::EaseOut,
            },
        });
        let button_id = button.id;
        doc.insert(button, aid, 0).unwrap();
        let mut back = Node::new(
            doc.next_free_id(),
            "Back",
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 20.0, 20.0),
        );
        back.fills.push(Paint::solid(deesainer_color::Rgba::BLACK));
        back.interactions.push(Interaction {
            trigger: Trigger::OnClick,
            action: Action::Back,
            transition: Transition::default(),
        });
        doc.insert(back, bid, 0).unwrap();
        (doc, page, aid, bid, button_id)
    }

    #[test]
    fn click_navigates_with_transition_and_back_returns() {
        let (doc, page, a, b, _) = setup();
        let mut p = Player::start(&doc, page, None).unwrap();
        assert_eq!(p.current, a);
        assert_eq!(p.click(&doc, Point::new(90.0, 90.0)), None);
        assert_eq!(
            p.click(&doc, Point::new(20.0, 15.0)),
            Some(Event::Navigated(b))
        );
        assert!(p.transition.is_some());
        assert_eq!(p.click(&doc, Point::new(5.0, 5.0)), None);
        p.tick(&doc, 0.15);
        let progress = p.transition.map(|t| t.progress()).unwrap();
        assert!(progress > 0.5 && progress < 1.0, "{progress}");
        p.tick(&doc, 1.0);
        assert!(p.transition.is_none());
        assert_eq!(
            p.click(&doc, Point::new(5.0, 5.0)),
            Some(Event::Navigated(a))
        );
    }

    #[test]
    fn delays_overlays_and_flow_starts() {
        let (mut doc, page, a, b, button) = setup();
        doc.get_mut(button).unwrap().interactions = vec![Interaction {
            trigger: Trigger::OnClick,
            action: Action::OpenOverlay { frame: b },
            transition: Transition::default(),
        }];
        doc.get_mut(a).unwrap().interactions.push(Interaction {
            trigger: Trigger::AfterDelay { ms: 500 },
            action: Action::OpenUrl {
                url: "https://example.com".into(),
            },
            transition: Transition::default(),
        });
        doc.get_mut(page).unwrap().interactions.push(Interaction {
            trigger: Trigger::OnClick,
            action: Action::Navigate { to: a },
            transition: Transition::default(),
        });
        assert_eq!(flow_starts(&doc, page), vec![a]);
        let mut p = Player::start(&doc, page, None).unwrap();
        assert!(p.tick(&doc, 0.2).is_empty());
        assert_eq!(
            p.tick(&doc, 0.4),
            vec![Event::OpenUrl("https://example.com".into())]
        );
        assert_eq!(
            p.click(&doc, Point::new(20.0, 15.0)),
            Some(Event::OverlayOpened(b))
        );
        assert_eq!(p.top(), b);
        assert_eq!(Player::targets(&doc, a).len(), 1);
    }

    #[test]
    fn push_offsets_move_both_frames() {
        let t = ActiveTransition {
            from: NodeId(1),
            to: NodeId(2),
            kind: TransitionKind::Push {
                direction: Direction::Left,
            },
            easing: Easing::Linear,
            duration: 1.0,
            elapsed: 0.5,
            reverse: false,
        };
        let (incoming, outgoing) = t.offsets();
        assert_eq!(incoming, (0.5, 0.0));
        assert_eq!(outgoing, (-0.5, 0.0));
    }

    #[test]
    fn scrolling_moves_content_but_not_fixed_layers() {
        use deesainer_doc::{Node, Overflow, Scrolling};
        use deesainer_geom::Rect;
        let mut doc = Document::new();
        let page = doc.first_page();
        let mut f = Node::new(
            doc.next_free_id(),
            "F",
            NodeKind::Frame,
            Rect::new(0.0, 0.0, 100.0, 200.0),
        );
        f.scrolling.overflow = Overflow::Vertical;
        let fid = f.id;
        doc.insert(f, page, 0).unwrap();
        let mut add = |r: Rect, fixed: bool| {
            let mut n = Node::new(doc.next_free_id(), "c", NodeKind::Rectangle, r);
            n.scrolling = Scrolling {
                fixed,
                ..Scrolling::default()
            };
            let id = n.id;
            let at = doc.get(fid).map_or(0, |p| p.children.len());
            doc.insert(n, fid, at).unwrap();
            id
        };
        let header = add(Rect::new(0.0, 0.0, 100.0, 40.0), true);
        let long = add(Rect::new(0.0, 40.0, 100.0, 460.0), false);
        assert_eq!(scroll_range(&doc, fid), (0.0, 300.0));
        let mut offsets = ScrollOffsets::from([(fid, (50.0, 1000.0))]);
        clamp_scroll(&doc, &mut offsets);
        assert_eq!(offsets[&fid], (0.0, 300.0));
        let view = scrolled_view(&doc, &offsets);
        assert_eq!(view.get(long).unwrap().rect.y, -260.0);
        assert_eq!(view.get(header).unwrap().rect.y, 0.0);
        assert_eq!(view.get(fid).unwrap().children.last(), Some(&header));
        assert_eq!(
            scroll_target(&doc, page, fid, Point::new(50.0, 100.0)),
            Some(fid)
        );
    }
}
