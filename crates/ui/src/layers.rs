use deesainer_doc::{NodeId, NodeKind};
use egui::{vec2, Align, Align2, Layout, Rect, Sense, Stroke, Vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::{self, TOKENS};
use crate::widgets::Tip;
use crate::widgets::{self, PAD};
use crate::App;

const ROW_H: f32 = 28.0;
const INDENT: f32 = 16.0;

pub fn panel(app: &mut App, ui: &mut egui::Ui) {
    pages(app, ui);
    widgets::divider(ui);
    let mut collapse = false;
    widgets::header(ui, app.tr("Layers"), |ui| {
        collapse =
            icons::button(ui, Icon::Collapse, false, app.tr("Collapse layers (⌥L)")).clicked();
    });
    if collapse {
        app.expanded.clear();
        app.info(app.tr("Layers collapsed"));
    }
    let doc = app.session.snapshot();
    let current = app.session.current_page();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some(page) = doc.get(current) {
                for child in page.children.iter().rev() {
                    row(app, ui, *child, 0);
                }
                if page.children.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(app.tr("No layers yet. Press F to draw a frame."))
                            .color(TOKENS.text_secondary),
                    );
                }
            }
        });
}

fn pages(app: &mut App, ui: &mut egui::Ui) {
    widgets::divider(ui);
    let mut add = false;
    widgets::header(ui, app.tr("Pages"), |ui| {
        add = icons::button(ui, Icon::Plus, false, app.tr("Add new page")).clicked();
    });
    if add {
        app.dispatch("page.create", json!({}));
        app.fitted = false;
    }
    let doc = app.session.snapshot();
    let current = app.session.current_page();
    ui.spacing_mut().item_spacing.y = 0.0;
    for page in doc.pages() {
        let name = doc.get(*page).map(|n| n.name.clone()).unwrap_or_default();
        let (rect, resp) = full_row(ui);
        let selected = *page == current;
        if selected {
            ui.painter().rect_filled(rect, 5.0, TOKENS.row_hover);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 5.0, TOKENS.field);
        }
        let font = if selected {
            theme::strong()
        } else {
            theme::body()
        };
        ui.painter().text(
            rect.left_center() + vec2(24.0, 0.0),
            Align2::LEFT_CENTER,
            name,
            font,
            TOKENS.text,
        );
        if resp.clicked() && !selected {
            app.dispatch("page.select", json!({ "id": page }));
            app.fitted = false;
        }
        if widgets::double_or_more(&resp) {
            app.renaming = Some((
                *page,
                doc.get(*page).map(|n| n.name.clone()).unwrap_or_default(),
            ));
        }
        let single = doc.pages().len() < 2;
        widgets::context_menu(&resp, |ui| {
            ui.set_min_width(180.0);
            if widgets::menu_item(ui, None, None, app.tr("Rename page"), "", true).clicked() {
                app.renaming = Some((
                    *page,
                    doc.get(*page).map(|n| n.name.clone()).unwrap_or_default(),
                ));
                ui.close();
            }
            if widgets::menu_item(ui, None, None, app.tr("Duplicate page"), "", true).clicked() {
                app.dispatch("page.duplicate", json!({ "id": page }));
                ui.close();
            }
            widgets::menu_separator(ui);
            if widgets::menu_item(ui, None, None, app.tr("Delete page"), "", !single).clicked() {
                app.dispatch("page.delete", json!({ "id": page }));
                app.fitted = false;
                ui.close();
            }
        });
        if let Some((rid, buffer)) = &mut app.renaming {
            if *rid == *page {
                let edit_rect = rect.shrink2(vec2(20.0, 3.0));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(edit_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                let r =
                    child.add(egui::TextEdit::singleline(buffer).desired_width(edit_rect.width()));
                r.request_focus();
                if r.lost_focus() {
                    let name = buffer.clone();
                    app.renaming = None;
                    app.dispatch("node.rename", json!({ "id": page, "name": name }));
                }
            }
        }
    }
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.add_space(8.0);
}

fn full_row(ui: &mut egui::Ui) -> (Rect, egui::Response) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    let rect = Rect::from_min_max(
        rect.min - vec2(PAD - 4.0, 0.0),
        rect.max + vec2(PAD - 4.0, 0.0),
    );
    (rect, resp)
}

pub fn kind_icon(kind: &NodeKind) -> Icon {
    match kind {
        NodeKind::Frame | NodeKind::SlotContent { .. } => Icon::Frame,
        NodeKind::Group => Icon::Group,
        NodeKind::Component { .. } | NodeKind::ComponentSet => Icon::Component,
        NodeKind::Instance { .. } => Icon::Instance,
        NodeKind::Rectangle => Icon::Rectangle,
        NodeKind::Ellipse { .. } => Icon::Ellipse,
        NodeKind::Line => Icon::Line,
        NodeKind::Polygon { .. } => Icon::Polygon,
        NodeKind::Star { .. } => Icon::Star,
        NodeKind::Vector { .. } => Icon::Vector,
        NodeKind::Text(_) => Icon::Text,
        NodeKind::BooleanOp { .. } => Icon::Boolean,
        NodeKind::Document | NodeKind::Page => Icon::Layers,
    }
}

fn row(app: &mut App, ui: &mut egui::Ui, id: NodeId, depth: usize) {
    let doc = app.session.snapshot();
    let Some(node) = doc.get(id) else { return };
    if depth > 64 {
        return;
    }
    let selected = app.session.selection().contains(&id);
    let has_children = !node.children.is_empty();
    let expanded = app.expanded.contains(&id);
    let component = matches!(
        node.kind,
        NodeKind::Component { .. } | NodeKind::ComponentSet | NodeKind::Instance { .. }
    );
    let (rect, resp) = full_row(ui);
    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(rect, 5.0, TOKENS.row_selected);
    } else if resp.hovered() {
        painter.rect_filled(rect, 5.0, TOKENS.row_hover);
    }
    let x0 = rect.min.x + 4.0 + depth as f32 * INDENT;
    let cy = rect.center().y;
    let chev = Rect::from_center_size(egui::pos2(x0 + 8.0, cy), Vec2::splat(16.0));
    if has_children {
        let icon = if expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        icons::paint(&painter, chev.shrink(2.0), icon, TOKENS.text_secondary);
        let c = ui.interact(chev, ui.id().with(("chev", id)), Sense::click());
        if c.clicked() {
            if expanded {
                app.expanded.remove(&id);
            } else {
                app.expanded.insert(id);
            }
        }
    }
    let tint = if component {
        TOKENS.component
    } else if node.visible {
        TOKENS.text_secondary
    } else {
        TOKENS.text_tertiary
    };
    let icon_rect = Rect::from_center_size(egui::pos2(x0 + 26.0, cy), Vec2::splat(14.0));
    icons::paint(&painter, icon_rect, kind_icon(&node.kind), tint);
    let text_x = x0 + 40.0;
    if let Some((rid, buffer)) = &mut app.renaming {
        if *rid == id {
            let edit_rect = Rect::from_min_max(
                egui::pos2(text_x - 2.0, rect.min.y + 3.0),
                egui::pos2(rect.max.x - 8.0, rect.max.y - 3.0),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(edit_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            let r = child.add(egui::TextEdit::singleline(buffer).desired_width(edit_rect.width()));
            r.request_focus();
            if r.lost_focus() {
                let name = buffer.clone();
                app.renaming = None;
                app.dispatch("node.rename", json!({ "id": id, "name": name }));
            }
            return;
        }
    }
    let text_color = if component {
        TOKENS.component
    } else if node.visible {
        TOKENS.text
    } else {
        TOKENS.text_tertiary
    };
    let font = if node.kind.is_frame_like() && depth == 0 {
        theme::strong()
    } else {
        theme::body()
    };
    let shown: String = node
        .name
        .split(['\n', '\r'])
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    painter.text(
        egui::pos2(text_x, cy),
        Align2::LEFT_CENTER,
        shown,
        font,
        text_color,
    );
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    let eye_rect = Rect::from_center_size(egui::pos2(rect.max.x - 16.0, cy), Vec2::splat(20.0));
    let lock_rect = Rect::from_center_size(egui::pos2(rect.max.x - 38.0, cy), Vec2::splat(20.0));
    if hovered || !node.visible {
        let e = ui
            .interact(eye_rect, ui.id().with(("eye", id)), Sense::click())
            .tip(app.tr("Show/Hide (⇧⌘H)"));
        let icon = if node.visible {
            Icon::Eye
        } else {
            Icon::EyeOff
        };
        icons::paint(&painter, eye_rect.shrink(3.0), icon, TOKENS.text_secondary);
        if e.clicked() {
            app.dispatch("object.toggleVisible", json!({ "ids": [id] }));
        }
    }
    if hovered || node.locked {
        let l = ui
            .interact(lock_rect, ui.id().with(("lock", id)), Sense::click())
            .tip(app.tr("Lock/Unlock (⇧⌘L)"));
        let color = if node.locked {
            TOKENS.text_secondary
        } else {
            TOKENS.text_tertiary
        };
        icons::paint(&painter, lock_rect.shrink(3.0), Icon::Lock, color);
        if l.clicked() {
            app.dispatch("object.toggleLock", json!({ "ids": [id] }));
        }
    }
    if widgets::double_or_more(&resp) {
        app.renaming = Some((id, node.name.clone()));
    } else if resp.clicked() {
        let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        let mut ids: Vec<NodeId> = if shift || cmd {
            app.session.selection().to_vec()
        } else {
            Vec::new()
        };
        if cmd && ids.contains(&id) {
            ids.retain(|x| *x != id);
        } else if !ids.contains(&id) {
            ids.push(id);
        }
        app.dispatch("selection.set", json!({ "ids": ids }));
    }
    if resp.secondary_clicked() && !selected {
        app.dispatch("selection.set", json!({ "ids": [id] }));
    }
    widgets::context_menu(&resp, |ui| crate::menu::object_context(app, ui, true));
    if resp.drag_started() {
        resp.dnd_set_drag_payload(id);
    }
    let container = node.kind.is_container() && !matches!(node.kind, NodeKind::BooleanOp { .. });
    let pointer_y = ui.input(|i| i.pointer.interact_pos().map(|p| p.y));
    let open = container && has_children && expanded;
    let drop = pointer_y.map(|y| drop_zone((y - rect.min.y) / rect.height(), container));
    if resp.dnd_hover_payload::<NodeId>().is_some_and(|d| *d != id) {
        let stroke = Stroke::new(2.0, TOKENS.accent);
        match drop {
            Some(Drop::Into) => {
                painter.rect_stroke(rect, 5.0, stroke, egui::StrokeKind::Inside);
            }
            Some(Drop::Above) => {
                painter.line_segment(
                    [
                        egui::pos2(x0, rect.min.y),
                        egui::pos2(rect.max.x, rect.min.y),
                    ],
                    stroke,
                );
            }
            Some(Drop::Below) => {
                let x = if open { x0 + INDENT } else { x0 };
                painter.line_segment(
                    [
                        egui::pos2(x, rect.max.y),
                        egui::pos2(rect.max.x, rect.max.y),
                    ],
                    stroke,
                );
            }
            None => {}
        }
    }
    if let (Some(dragged), Some(drop)) = (resp.dnd_release_payload::<NodeId>(), drop) {
        let dragged = *dragged;
        if dragged != id {
            if drop == Drop::Into || (drop == Drop::Below && open) {
                app.dispatch("node.reparent", json!({ "id": dragged, "parent": id }));
            } else if let Some(parent) = node.parent {
                let target = doc.index_in_parent(id).unwrap_or(0);
                let from = doc
                    .get(dragged)
                    .filter(|d| d.parent == Some(parent))
                    .and_then(|_| doc.index_in_parent(dragged).ok());
                let index = drop_index(target, from, drop == Drop::Above);
                app.dispatch(
                    "node.reparent",
                    json!({ "id": dragged, "parent": parent, "index": index }),
                );
            }
        }
    }
    if has_children && expanded {
        for child in node.children.iter().rev() {
            row(app, ui, *child, depth + 1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drop {
    Above,
    Into,
    Below,
}

fn drop_zone(frac: f32, container: bool) -> Drop {
    if container {
        if frac < 0.25 {
            Drop::Above
        } else if frac > 0.75 {
            Drop::Below
        } else {
            Drop::Into
        }
    } else if frac < 0.5 {
        Drop::Above
    } else {
        Drop::Below
    }
}

fn drop_index(target: usize, from: Option<usize>, above: bool) -> usize {
    let index = if above { target + 1 } else { target };
    match from {
        Some(f) if f < index => index - 1,
        _ => index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_follow_the_pointer_in_the_row() {
        assert_eq!(drop_zone(0.1, false), Drop::Above);
        assert_eq!(drop_zone(0.6, false), Drop::Below);
        assert_eq!(drop_zone(0.1, true), Drop::Above);
        assert_eq!(drop_zone(0.5, true), Drop::Into);
        assert_eq!(drop_zone(0.9, true), Drop::Below);
    }

    #[test]
    fn drop_indices_account_for_the_layer_leaving_its_slot() {
        assert_eq!(drop_index(2, None, true), 3);
        assert_eq!(drop_index(2, None, false), 2);
        assert_eq!(drop_index(2, Some(0), true), 2);
        assert_eq!(drop_index(2, Some(0), false), 1);
        assert_eq!(drop_index(2, Some(3), true), 3);
        assert_eq!(drop_index(2, Some(3), false), 2);
    }
}
