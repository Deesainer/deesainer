use deesainer_doc::{Action, Document, NodeId, Trigger};
use deesainer_geom::Point;
use egui::{pos2, vec2, Pos2, Sense, Stroke};
use serde_json::json;

use crate::theme::TOKENS;
use crate::{App, RightTab};

fn top_frame(doc: &Document, page: NodeId, id: NodeId) -> Option<NodeId> {
    let mut at = id;
    for _ in 0..deesainer_doc::MAX_DEPTH {
        let parent = doc.get(at)?.parent?;
        if parent == page {
            return Some(at);
        }
        at = parent;
    }
    None
}

pub(crate) fn target_at(doc: &Document, page: NodeId, from: NodeId, p: Point) -> Option<NodeId> {
    let own = top_frame(doc, page, from);
    deesainer_tools::proto::frames(doc, page)
        .into_iter()
        .rev()
        .filter(|f| Some(*f) != own)
        .find(|f| doc.world_bounds(*f).is_some_and(|b| b.contains(p)))
}

pub(crate) fn connect_handle(app: &mut App, ui: &mut egui::Ui, origin: Pos2) -> bool {
    if app.right_tab != RightTab::Prototype {
        app.connect_drag = None;
        return false;
    }
    let page = app.session.current_page();
    let [id] = app.session.selection() else {
        return false;
    };
    let id = *id;
    let doc = app.session.snapshot();
    let Some(b) = doc.world_bounds(id).filter(|_| id != page) else {
        return false;
    };
    let handle = app
        .camera
        .to_screen(origin, Point::new(b.max_x(), b.center().y))
        + vec2(14.0, 0.0);
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("connect-handle"),
        ))
        .with_clip_rect(ui.max_rect());
    let resp = ui.interact(
        egui::Rect::from_center_size(handle, vec2(18.0, 18.0)),
        ui.id().with(("connect", id.0)),
        Sense::drag(),
    );
    let color = TOKENS.selection;
    if resp.drag_started() {
        app.connect_drag = Some(id);
    }
    let pointer = resp
        .interact_pointer_pos()
        .or(ui.input(|i| i.pointer.hover_pos()));
    if app.connect_drag == Some(id) {
        if let Some(end) = pointer {
            let p = app.camera.to_page(origin, end);
            let target = target_at(&doc, page, id, p);
            if let Some(t) = target.and_then(|t| doc.world_bounds(t)) {
                let a = app.camera.to_screen(origin, t.origin());
                let z = app
                    .camera
                    .to_screen(origin, Point::new(t.max_x(), t.max_y()));
                painter.rect_stroke(
                    egui::Rect::from_min_max(a, z),
                    0.0,
                    Stroke::new(2.0, color),
                    egui::StrokeKind::Outside,
                );
            }
            let mid = pos2((handle.x + end.x) / 2.0, handle.y);
            painter.add(egui::Shape::line(
                vec![handle, mid, pos2(mid.x, end.y), end],
                Stroke::new(1.5, color),
            ));
            if resp.drag_stopped() {
                app.connect_drag = None;
                if let Some(t) = target {
                    connect(app, &doc, id, t);
                }
            }
        }
        if !resp.dragged() && !resp.drag_stopped() {
            app.connect_drag = None;
        }
    }
    let hot = resp.hovered() || resp.dragged();
    painter.circle(
        handle,
        if hot { 7.0 } else { 6.0 },
        if hot { color } else { egui::Color32::WHITE },
        Stroke::new(1.5, color),
    );
    let plus = if hot { egui::Color32::WHITE } else { color };
    painter.line_segment(
        [handle - vec2(3.0, 0.0), handle + vec2(3.0, 0.0)],
        Stroke::new(1.5, plus),
    );
    painter.line_segment(
        [handle - vec2(0.0, 3.0), handle + vec2(0.0, 3.0)],
        Stroke::new(1.5, plus),
    );
    hot || app.connect_drag.is_some()
}

fn connect(app: &mut App, doc: &Document, id: NodeId, to: NodeId) {
    let existing = doc.get(id).and_then(|n| {
        n.interactions
            .iter()
            .position(|i| i.trigger == Trigger::OnClick && !matches!(i.action, Action::Back))
    });
    app.session.begin_group("Connect");
    if let Some(index) = existing {
        app.dispatch(
            "prototype.removeInteraction",
            json!({ "id": id, "index": index }),
        );
    }
    app.dispatch(
        "prototype.addInteraction",
        json!({ "id": id, "trigger": "click", "action": "navigate", "to": to }),
    );
    app.session.end_group();
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_geom::Rect;

    #[test]
    fn targets_are_other_top_level_frames() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let mut add = |parent: NodeId, kind: deesainer_doc::NodeKind, r: Rect| {
            let n = deesainer_doc::Node::new(doc.next_free_id(), "n", kind, r);
            let id = n.id;
            let at = doc.get(parent).map_or(0, |p| p.children.len());
            doc.insert(n, parent, at).unwrap();
            id
        };
        let a = add(
            page,
            deesainer_doc::NodeKind::Frame,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        let button = add(
            a,
            deesainer_doc::NodeKind::Rectangle,
            Rect::new(10.0, 10.0, 20.0, 20.0),
        );
        let b = add(
            page,
            deesainer_doc::NodeKind::Frame,
            Rect::new(200.0, 0.0, 100.0, 100.0),
        );
        assert_eq!(
            target_at(&doc, page, button, Point::new(250.0, 50.0)),
            Some(b)
        );
        assert_eq!(target_at(&doc, page, button, Point::new(50.0, 50.0)), None);
        assert_eq!(target_at(&doc, page, button, Point::new(150.0, 50.0)), None);
    }
}
