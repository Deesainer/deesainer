use deesainer_doc::{Node, NodeId, NodeKind};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::widgets::{self, Lead};
use crate::App;

pub fn extra_sections(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    component_section(app, ui, id, node);
    shape_options(app, ui, id, node);
    child_layout(app, ui, id, node);
    auto_layout(app, ui, id, node);
    variable_modes(app, ui, id, node);
}

fn variable_modes(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    if !node.kind.is_frame_like() {
        return;
    }
    let doc = app.session.snapshot();
    let choices: Vec<(deesainer_doc::CollectionId, deesainer_doc::Modes)> = doc
        .collections()
        .filter(|(_, m)| m.names.len() > 1)
        .map(|(c, m)| (c, m.clone()))
        .collect();
    if choices.is_empty() {
        return;
    }
    widgets::divider(ui);
    widgets::header(ui, app.tr("Variable modes"), |_| {});
    let full = widgets::half(ui) * 2.0 + 8.0;
    for (collection, modes) in choices {
        let inherited = node
            .parent
            .and_then(|p| doc.mode_at(p, collection))
            .unwrap_or(modes.active);
        let auto = format!(
            "{} ({})",
            app.tr("Auto"),
            modes.names.get(inherited).cloned().unwrap_or_default()
        );
        let mut options = vec![auto];
        options.extend(modes.names.iter().cloned());
        let current = node
            .variable_modes
            .get(&collection)
            .and_then(|m| modes.names.get(*m))
            .cloned()
            .unwrap_or_else(|| options[0].clone());
        ui.push_id(("variable-mode", collection.0), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&modes.name).color(crate::theme::TOKENS.text_secondary),
                );
            });
            if let Some(i) = pick(ui, full, &current, &options) {
                let mode = if i == 0 { json!(null) } else { json!(i - 1) };
                app.dispatch(
                    "node.setVariableMode",
                    json!({ "ids": [id], "collection": collection.0, "mode": mode }),
                );
            }
        });
    }
    ui.add_space(8.0);
}

fn pick(ui: &mut egui::Ui, width: f32, current: &str, options: &[String]) -> Option<usize> {
    let resp = widgets::select(ui, width, current);
    let mut picked = None;
    widgets::menu(&resp).show(|ui| {
        for (i, o) in options.iter().enumerate() {
            if widgets::menu_item(ui, Some(o == current), None, o, "", true).clicked() {
                picked = Some(i);
                ui.close();
            }
        }
    });
    picked
}

fn prop_icon(kind: &deesainer_doc::ComponentPropKind) -> Icon {
    match kind {
        deesainer_doc::ComponentPropKind::Boolean { .. } => Icon::Eye,
        deesainer_doc::ComponentPropKind::Text { .. } => Icon::Text,
        deesainer_doc::ComponentPropKind::InstanceSwap { .. } => Icon::Instance,
        deesainer_doc::ComponentPropKind::Slot { .. } => Icon::Frame,
    }
}

#[derive(Clone, Default)]
struct PropDraft {
    kind: usize,
    target: Option<NodeId>,
    name: String,
}

fn component_section(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let doc = app.session.snapshot();
    match &node.kind {
        NodeKind::Component { properties, .. } => {
            let variants = deesainer_engine::components::variant_props(&node.name);
            if !variants.is_empty() {
                widgets::section(
                    ui,
                    app.tr("Variant"),
                    |_| {},
                    |ui| {
                        for (k, v) in &variants {
                            ui.label(format!("{k}: {v}"));
                        }
                    },
                );
            }
            widgets::divider(ui);
            let mut plus = None;
            let actions = |ui: &mut egui::Ui, plus: &mut Option<egui::Response>| {
                *plus = Some(icons::button(
                    ui,
                    Icon::Plus,
                    false,
                    app.tr("Create property"),
                ));
            };
            if properties.is_empty() {
                widgets::header_empty(ui, app.tr("Properties"), |ui| actions(ui, &mut plus));
            } else {
                widgets::header(ui, app.tr("Properties"), |ui| actions(ui, &mut plus));
            }
            if let Some(resp) = &plus {
                add_property_popover(app, resp, id, &doc);
            }
            let mut remove = None;
            for p in properties {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 32.0).max(60.0);
                    widgets::field(ui, w, |ui| {
                        icons::glyph(ui, prop_icon(&p.kind), crate::theme::TOKENS.text_secondary);
                        ui.label(egui::RichText::new(&p.name).font(crate::theme::strong()));
                        let target = doc.get(p.target).map_or("(missing)", |n| n.name.as_str());
                        ui.label(
                            egui::RichText::new(target).color(crate::theme::TOKENS.text_secondary),
                        );
                    });
                    if icons::button(ui, Icon::Minus, false, "Remove property").clicked() {
                        remove = Some(p.name.clone());
                    }
                });
            }
            if let Some(name) = remove {
                app.dispatch(
                    "component.removeProperty",
                    json!({ "id": id, "name": name }),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Create instance").clicked() {
                    app.dispatch("component.instantiate", json!({ "component": id }));
                }
                if ui
                    .button("Add slot")
                    .on_hover_text("A place each instance fills with its own content")
                    .clicked()
                {
                    let n = properties
                        .iter()
                        .filter(|p| {
                            matches!(
                                p.kind,
                                deesainer_doc::ComponentPropKind::InstanceSwap { .. }
                                    | deesainer_doc::ComponentPropKind::Slot { .. }
                            )
                        })
                        .count()
                        + 1;
                    app.dispatch(
                        "component.addSlot",
                        json!({ "id": id, "name": format!("Slot {n}") }),
                    );
                }
            });
            ui.add_space(8.0);
        }
        NodeKind::Instance {
            component,
            overrides,
        } => {
            widgets::divider(ui);
            widgets::header(ui, app.tr("Instance"), |ui| {
                if !overrides.is_empty()
                    && icons::button(ui, Icon::Rotate, false, "Reset all changes").clicked()
                {
                    app.dispatch("component.resetOverrides", json!({ "ids": [id] }));
                }
                if icons::button(ui, Icon::Group, false, "Detach instance (⌥⌘B)").clicked() {
                    app.dispatch("component.detach", json!({ "ids": [id] }));
                }
                if icons::button(ui, Icon::Component, false, "Go to main component").clicked() {
                    app.dispatch("component.goToMain", json!({ "id": id }));
                    app.fitted = false;
                }
            });
            let Some(main) = doc.get(*component).cloned() else {
                ui.label(
                    egui::RichText::new(app.tr("Main component missing."))
                        .color(crate::theme::TOKENS.text_secondary),
                );
                return;
            };
            let full = widgets::half(ui) * 2.0 + 8.0;
            if let Some(set) = main
                .parent
                .and_then(|s| doc.get(s))
                .filter(|s| matches!(s.kind, NodeKind::ComponentSet))
            {
                let current = deesainer_engine::components::variant_props(&main.name);
                let mut options: std::collections::BTreeMap<String, Vec<String>> =
                    std::collections::BTreeMap::new();
                for c in set.children.iter().filter_map(|c| doc.get(*c)) {
                    for (k, v) in deesainer_engine::components::variant_props(&c.name) {
                        let e = options.entry(k).or_default();
                        if !e.contains(&v) {
                            e.push(v);
                        }
                    }
                }
                for (prop, values) in options {
                    let now = current.get(&prop).cloned().unwrap_or_default();
                    widgets::labelled(ui, &prop, |ui| {
                        if let Some(i) = pick(ui, full - 72.0, &now, &values) {
                            app.dispatch(
                                "instance.setVariant",
                                json!({ "ids": [id], "properties": { prop.clone(): values[i].clone() } }),
                            );
                        }
                    });
                }
            }
            let props = match &main.kind {
                NodeKind::Component { properties, .. } => properties.clone(),
                _ => Vec::new(),
            };
            let current_override = |target: NodeId, text: bool, vis: bool, inst: bool| {
                overrides.iter().rev().find_map(|o| {
                    if o.target != target {
                        return None;
                    }
                    match (&o.value, text, vis, inst) {
                        (deesainer_doc::PropValue::Kind(NodeKind::Text(d)), true, _, _) => {
                            Some(deesainer_doc::PropValue::Kind(NodeKind::Text(d.clone())))
                        }
                        (deesainer_doc::PropValue::Visible(v), _, true, _) => {
                            Some(deesainer_doc::PropValue::Visible(*v))
                        }
                        (
                            deesainer_doc::PropValue::Kind(NodeKind::Instance {
                                component, ..
                            }),
                            _,
                            _,
                            true,
                        ) => Some(deesainer_doc::PropValue::Kind(NodeKind::Instance {
                            component: *component,
                            overrides: Vec::new(),
                        })),
                        _ => None,
                    }
                })
            };
            for p in &props {
                use deesainer_doc::ComponentPropKind as K;
                match &p.kind {
                    K::Boolean { default } => {
                        let now = match current_override(p.target, false, true, false) {
                            Some(deesainer_doc::PropValue::Visible(v)) => v,
                            _ => *default,
                        };
                        let mut v = now;
                        widgets::labelled(ui, &p.name, |ui| {
                            ui.checkbox(&mut v, "");
                        });
                        if v != now {
                            app.dispatch(
                                "instance.setProperty",
                                json!({ "id": id, "name": p.name, "value": v }),
                            );
                        }
                    }
                    K::Text { default } => {
                        let now = match current_override(p.target, true, false, false) {
                            Some(deesainer_doc::PropValue::Kind(NodeKind::Text(d))) => d.content,
                            _ => default.clone(),
                        };
                        let key = egui::Id::new(("prop-text", id.0, p.name.clone()));
                        let mut buf: String =
                            ui.data(|d| d.get_temp(key)).unwrap_or_else(|| now.clone());
                        let focused = widgets::labelled(ui, &p.name, |ui| {
                            widgets::field(ui, full - 72.0, |ui| {
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut buf)
                                        .frame(egui::Frame::NONE)
                                        .desired_width(f32::INFINITY),
                                );
                                if r.lost_focus() && buf != now {
                                    app.dispatch(
                                        "instance.setProperty",
                                        json!({ "id": id, "name": p.name, "value": buf }),
                                    );
                                }
                                r.has_focus()
                            })
                        });
                        if focused {
                            ui.data_mut(|d| d.insert_temp(key, buf));
                        } else {
                            ui.data_mut(|d| d.remove::<String>(key));
                        }
                    }
                    K::InstanceSwap { default } => {
                        let now = match current_override(p.target, false, false, true) {
                            Some(deesainer_doc::PropValue::Kind(NodeKind::Instance {
                                component,
                                ..
                            })) => component,
                            _ => *default,
                        };
                        let mains: Vec<(NodeId, String)> = doc
                            .nodes()
                            .filter(|n| {
                                matches!(n.kind, NodeKind::Component { .. }) && n.id != *component
                            })
                            .map(|n| (n.id, n.name.clone()))
                            .collect();
                        let names: Vec<String> = mains.iter().map(|(_, n)| n.clone()).collect();
                        let shown = doc
                            .get(now)
                            .map_or("(missing)", |n| n.name.as_str())
                            .to_owned();
                        widgets::labelled(ui, &p.name, |ui| {
                            if let Some(i) = pick(ui, full - 72.0, &shown, &names) {
                                app.dispatch(
                                    "instance.setProperty",
                                    json!({ "id": id, "name": p.name, "value": mains[i].0 }),
                                );
                            }
                        });
                    }
                    K::Slot { preferred } => {
                        let held = doc.get(id).and_then(|inst| {
                            inst.children
                                .iter()
                                .find_map(|c| match doc.get(*c).map(|n| &n.kind) {
                                    Some(NodeKind::SlotContent { slot }) if *slot == p.name => {
                                        doc.get(*c).map(|n| n.children.len())
                                    }
                                    _ => None,
                                })
                        });
                        let offered: Vec<(NodeId, String)> = if preferred.is_empty() {
                            doc.nodes()
                                .filter(|n| {
                                    matches!(n.kind, NodeKind::Component { .. })
                                        && n.id != *component
                                })
                                .map(|n| (n.id, n.name.clone()))
                                .collect()
                        } else {
                            preferred
                                .iter()
                                .filter_map(|c| doc.get(*c).map(|n| (n.id, n.name.clone())))
                                .collect()
                        };
                        let others: Vec<NodeId> = app
                            .session
                            .selection()
                            .iter()
                            .copied()
                            .filter(|s| *s != id)
                            .collect();
                        widgets::labelled(ui, &p.name, |ui| {
                            let status = match held {
                                Some(n) => format!("{n} layer{}", if n == 1 { "" } else { "s" }),
                                None => "Default content".to_owned(),
                            };
                            ui.label(
                                egui::RichText::new(status)
                                    .color(crate::theme::TOKENS.text_secondary),
                            );
                        });
                        ui.horizontal(|ui| {
                            let names: Vec<String> =
                                offered.iter().map(|(_, n)| n.clone()).collect();
                            if let Some(i) = pick(ui, 110.0, app.tr("Insert…"), &names) {
                                app.dispatch(
                                    "instance.fillSlot",
                                    json!({ "id": id, "name": p.name, "component": offered[i].0 }),
                                );
                            }
                            if !others.is_empty()
                                && ui
                                    .button("Use selection")
                                    .on_hover_text("Move the other selected layers into this slot")
                                    .clicked()
                            {
                                app.dispatch(
                                    "instance.fillSlot",
                                    json!({ "id": id, "name": p.name, "ids": others }),
                                );
                            }
                            if held.is_some() && ui.button("Clear").clicked() {
                                app.dispatch(
                                    "instance.clearSlot",
                                    json!({ "id": id, "name": p.name }),
                                );
                            }
                        });
                    }
                }
            }
            let bound: Vec<NodeId> = props.iter().map(|p| p.target).collect();
            for tid in doc.subtree_pre_order(*component).unwrap_or_default() {
                if bound.contains(&tid) {
                    continue;
                }
                let Some(t) = doc.get(tid) else { continue };
                let Some(data) = t.text() else { continue };
                let current = overrides
                    .iter()
                    .rev()
                    .find_map(|o| match (&o.value, o.target == tid) {
                        (deesainer_doc::PropValue::Kind(NodeKind::Text(d)), true) => {
                            Some(d.content.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| data.content.clone());
                let mut buf = current.clone();
                widgets::labelled(ui, &t.name, |ui| {
                    widgets::field(ui, full - 72.0, |ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut buf)
                                .frame(egui::Frame::NONE)
                                .desired_width(f32::INFINITY),
                        );
                        if r.lost_focus() && buf != current {
                            app.dispatch(
                                "instance.setOverride",
                                json!({ "id": id, "target": tid, "prop": "text", "value": buf }),
                            );
                        }
                    });
                });
            }
            ui.add_space(8.0);
        }
        _ => {}
    }
}

pub(crate) fn instance_layer_section(
    app: &mut App,
    ui: &mut egui::Ui,
    instance: &Node,
    target: NodeId,
) {
    let doc = app.session.snapshot();
    let Some((_, _, layer)) = deesainer_engine::slots::slot_box(&doc, instance, target) else {
        app.instance_layer = None;
        return;
    };
    let id = instance.id;
    widgets::divider(ui);
    widgets::header(ui, &format!("Instance layer: {}", layer.name), |ui| {
        if icons::button(ui, Icon::Close, false, "Back to the instance (Esc)").clicked() {
            app.instance_layer = None;
        }
    });
    let full = widgets::half(ui) * 2.0 + 8.0;
    let set = |app: &mut App, prop: &str, value: serde_json::Value| {
        app.dispatch(
            "instance.setOverride",
            json!({ "id": id, "target": target, "prop": prop, "value": value }),
        );
    };
    if let Some(data) = layer.text() {
        let key = ui.id().with(("override-text", target.0));
        let mut buf: String = ui
            .data(|d| d.get_temp(key))
            .unwrap_or_else(|| data.content.clone());
        widgets::field(ui, full, |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY),
            );
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(key, buf.clone()));
            }
            if r.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(key));
                if buf != data.content {
                    set(app, "text", json!(buf));
                }
            }
        });
    }
    ui.horizontal(|ui| {
        let solid = layer.fills.iter().rev().find_map(|p| match p.kind {
            deesainer_doc::PaintKind::Solid { color } => Some(color),
            _ => None,
        });
        if let Some(c) = solid {
            let mut c32 = crate::theme::color32(c);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                set(app, "fill", json!(crate::theme::rgba(c32).to_hex()));
            }
            ui.label("Fill");
        }
        let mut visible = layer.visible;
        if ui.checkbox(&mut visible, "Visible").changed() {
            set(app, "visible", json!(visible));
        }
    });
    let mut pct = f64::from(layer.opacity) * 100.0;
    let resp = widgets::number(
        ui,
        widgets::Lead::Icon(Icon::Opacity),
        &mut pct,
        0.5,
        "%",
        widgets::half(ui),
    );
    if resp.changed() {
        set(app, "opacity", json!((pct / 100.0).clamp(0.0, 1.0)));
    }
    ui.add_space(8.0);
}

fn add_property_popover(
    app: &mut App,
    resp: &egui::Response,
    id: NodeId,
    doc: &deesainer_doc::Document,
) {
    let key = egui::Id::new(("prop-draft", id.0));
    let mut draft: PropDraft = resp.ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    let kinds = [
        ("boolean", app.tr("Boolean (show/hide)")),
        ("text", app.tr("Text")),
        ("instanceSwap", app.tr("Instance swap")),
    ];
    let mut add = false;
    widgets::popover(resp, "Create property").show(|ui| {
        ui.set_width(240.0);
        if widgets::popover_title(ui, app.tr("Create property")) {
            ui.close();
        }
        let w = 150.0;
        let cur = kinds[draft.kind.min(2)].0;
        widgets::labelled(ui, app.tr("Type"), |ui| {
            if let Some(v) = widgets::choice(ui, w, cur, &kinds) {
                draft.kind = kinds.iter().position(|(k, _)| *k == v).unwrap_or(0);
                draft.target = None;
            }
        });
        let candidates: Vec<(NodeId, String)> = doc
            .subtree_pre_order(id)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| *n != id)
            .filter_map(|n| doc.get(n))
            .filter(|n| match draft.kind {
                1 => n.text().is_some(),
                2 => matches!(n.kind, NodeKind::Instance { .. }),
                _ => true,
            })
            .map(|n| (n.id, n.name.clone()))
            .collect();
        let shown = draft
            .target
            .and_then(|t| {
                candidates
                    .iter()
                    .find(|(c, _)| *c == t)
                    .map(|(_, n)| n.clone())
            })
            .unwrap_or_else(|| "Select a layer".into());
        let names: Vec<String> = candidates.iter().map(|(_, n)| n.clone()).collect();
        widgets::labelled(ui, app.tr("Layer"), |ui| {
            if let Some(i) = pick(ui, w, &shown, &names) {
                draft.target = Some(candidates[i].0);
                if draft.name.is_empty() {
                    draft.name = candidates[i].1.clone();
                }
            }
        });
        widgets::labelled(ui, app.tr("Name"), |ui| {
            widgets::field(ui, w, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY),
                );
            });
        });
        ui.add_space(6.0);
        let ready = draft.target.is_some() && !draft.name.trim().is_empty();
        if ui
            .add_enabled(
                ready,
                egui::Button::new(
                    egui::RichText::new("Create property").color(crate::theme::TOKENS.accent_text),
                )
                .fill(crate::theme::TOKENS.accent),
            )
            .clicked()
        {
            add = true;
            ui.close();
        }
    });
    if add {
        if let Some(target) = draft.target {
            app.dispatch(
                "component.addProperty",
                json!({ "id": id, "name": draft.name.trim(), "kind": kinds[draft.kind.min(2)].0, "target": target }),
            );
        }
        draft = PropDraft::default();
    }
    resp.ctx.data_mut(|d| d.insert_temp(key, draft));
}

fn enum_name<T: serde::Serialize>(v: T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn sizing(app: &App) -> [(&'static str, &'static str); 3] {
    [
        (app.tr("Fixed"), "fixed"),
        (app.tr("Hug"), "hug"),
        (app.tr("Fill"), "fill"),
    ]
}

fn auto_layout(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    if !matches!(node.kind, NodeKind::Frame | NodeKind::Component { .. }) {
        return;
    }
    let Some(l) = node.layout else { return };
    let mut remove = false;
    widgets::section(
        ui,
        "Auto layout",
        |ui| {
            remove = icons::button(ui, Icon::Minus, false, "Remove auto layout").clicked();
        },
        |ui| {
            let half = widgets::half(ui);
            let horizontal = enum_name(l.direction) != "vertical";
            let (w_key, h_key) = if horizontal {
                ("primarySizing", "counterSizing")
            } else {
                ("counterSizing", "primarySizing")
            };
            let (w_now, h_now) = if horizontal {
                (l.primary_sizing, l.counter_sizing)
            } else {
                (l.counter_sizing, l.primary_sizing)
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (key, now) in [(w_key, w_now), (h_key, h_now)] {
                    if let Some(v) = widgets::choice(ui, half, &enum_name(now), &sizing(app)[..2]) {
                        app.dispatch("layout.setAutoLayout", json!({ "id": id, key: v }));
                    }
                }
            });
            if l.direction == deesainer_doc::LayoutDirection::Grid {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let mut cols = l.columns() as f64;
                    let r = widgets::number(ui, widgets::Lead::Text(app.tr("Columns")), &mut cols, 0.1, "", half);
                    if r.changed() {
                        app.dispatch(
                            "layout.setAutoLayout",
                            json!({ "id": id, "gridColumns": (cols.round() as u32).clamp(1, 100) }),
                        );
                    }
                    let mut rows = f64::from(l.grid_rows);
                    let r = widgets::number(ui, widgets::Lead::Text(app.tr("Rows")), &mut rows, 0.1, "", half);
                    if r.changed() {
                        app.dispatch(
                            "layout.setAutoLayout",
                            json!({ "id": id, "gridRows": (rows.round().max(0.0) as u32).min(100) }),
                        );
                    }
                });
            }
            let index = |a: &str| match a {
                "center" => 1,
                "end" => 2,
                _ => 0,
            };
            let p = index(&enum_name(l.primary_align));
            let c = index(&enum_name(l.counter_align));
            let active = if horizontal { (p, c) } else { (c, p) };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if let Some((col, row)) = widgets::align_grid(ui, half, active, horizontal) {
                    let names = ["start", "center", "end"];
                    let (pi, ci) = if horizontal { (col, row) } else { (row, col) };
                    app.dispatch(
                        "layout.setAutoLayout",
                        json!({ "id": id, "primaryAlign": names[pi], "counterAlign": names[ci] }),
                    );
                }
                ui.vertical(|ui| {
                    let mut gap = l.gap;
                    let gap_icon = if horizontal {
                        Icon::DistributeH
                    } else {
                        Icon::DistributeV
                    };
                    let resp = widgets::number(ui, Lead::Icon(gap_icon), &mut gap, 1.0, "", half);
                    crate::design::edit(
                        app,
                        &resp,
                        app.tr("Gap"),
                        "layout.setAutoLayout",
                        json!({ "id": id, "gap": gap.max(0.0) }),
                    );
                    let between = enum_name(l.primary_align) == "spaceBetween";
                    let mut b = between;
                    if ui.checkbox(&mut b, "Space between").changed() {
                        let v = if b { "spaceBetween" } else { "start" };
                        app.dispatch(
                            "layout.setAutoLayout",
                            json!({ "id": id, "primaryAlign": v }),
                        );
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let pd = l.padding;
                let mut ph = pd.left;
                let resp = widgets::number(ui, Lead::Icon(Icon::PadH), &mut ph, 1.0, "", half);
                crate::design::edit(
                    app,
                    &resp,
                    app.tr("Padding"),
                    "layout.setAutoLayout",
                    json!({ "id": id, "padding": { "top": pd.top, "bottom": pd.bottom, "left": ph.max(0.0), "right": ph.max(0.0) } }),
                );
                let mut pv = pd.top;
                let resp = widgets::number(ui, Lead::Icon(Icon::PadV), &mut pv, 1.0, "", half);
                crate::design::edit(
                    app,
                    &resp,
                    app.tr("Padding"),
                    "layout.setAutoLayout",
                    json!({ "id": id, "padding": { "top": pv.max(0.0), "bottom": pv.max(0.0), "left": pd.left, "right": pd.right } }),
                );
            });
            let mut wrap = l.wrap;
            if l.direction == deesainer_doc::LayoutDirection::Horizontal
                && ui.checkbox(&mut wrap, "Wrap").changed()
            {
                app.dispatch("layout.setAutoLayout", json!({ "id": id, "wrap": wrap }));
            }
        },
    );
    if remove {
        app.dispatch(
            "layout.setAutoLayout",
            json!({ "id": id, "direction": "none" }),
        );
    }
}

fn child_layout(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let doc = app.session.snapshot();
    let Some(parent) = node.parent.and_then(|p| doc.get(p)) else {
        return;
    };
    if matches!(parent.kind, NodeKind::Page | NodeKind::Document) {
        return;
    }
    if parent.layout.is_some() {
        let lc = node.layout_child;
        widgets::section(
            ui,
            app.tr("Resizing"),
            |_| {},
            |ui| {
                let half = widgets::half(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (key, now) in [("horizontal", lc.horizontal), ("vertical", lc.vertical)] {
                        if let Some(v) = widgets::choice(ui, half, &enum_name(now), &sizing(app)) {
                            app.dispatch("layout.setChild", json!({ "ids": [id], key: v }));
                        }
                    }
                });
                let mut abs = lc.absolute;
                if ui.checkbox(&mut abs, "Absolute position").changed() {
                    app.dispatch("layout.setChild", json!({ "ids": [id], "absolute": abs }));
                }
            },
        );
    } else if parent.kind.is_frame_like() {
        let c = node.constraints;
        let h_now = match enum_name(c.horizontal).as_str() {
            "min" => "left",
            "max" => "right",
            "center" => "center",
            "stretch" => "leftRight",
            _ => "scale",
        };
        let v_now = match enum_name(c.vertical).as_str() {
            "min" => "top",
            "max" => "bottom",
            "center" => "center",
            "stretch" => "topBottom",
            _ => "scale",
        };
        widgets::section(
            ui,
            app.tr("Constraints"),
            |_| {},
            |ui| {
                let half = widgets::half(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let h = [
                        ("left", app.tr("Left")),
                        ("right", app.tr("Right")),
                        ("leftRight", app.tr("Left and right")),
                        ("center", app.tr("Center")),
                        ("scale", app.tr("Scale")),
                    ];
                    if let Some(v) = widgets::choice(ui, half, h_now, &h) {
                        app.dispatch(
                            "node.setConstraints",
                            json!({ "ids": [id], "horizontal": v }),
                        );
                    }
                    let v = [
                        ("top", app.tr("Top")),
                        ("bottom", app.tr("Bottom")),
                        ("topBottom", app.tr("Top and bottom")),
                        ("center", app.tr("Center")),
                        ("scale", app.tr("Scale")),
                    ];
                    if let Some(k) = widgets::choice(ui, half, v_now, &v) {
                        app.dispatch("node.setConstraints", json!({ "ids": [id], "vertical": k }));
                    }
                });
            },
        );
    }
}

fn shape_options(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let kind = node.kind.clone();
    if matches!(kind, NodeKind::Vector { .. }) {
        widgets::section(
            ui,
            "Vector",
            |_| {},
            |ui| {
                let hint = if app.tools.editing_path() == Some(id) {
                    "Drag anchors or curve handles on the canvas."
                } else {
                    "Double-click to edit points. Click a segment to add one, double-click a point to switch corner and curve, Delete removes it."
                };
                ui.label(egui::RichText::new(hint).color(crate::theme::TOKENS.text_secondary));
                let network = match &kind {
                    NodeKind::Vector { path } => node.network.as_ref().filter(|n| n.matches(path)),
                    _ => None,
                };
                ui.add_space(4.0);
                match network {
                    Some(net) => {
                        ui.label(format!(
                            "Network: {} vertices, {} segments, {} regions",
                            net.vertices.len(),
                            net.segments.len(),
                            net.regions.len()
                        ));
                        widgets::divider(ui);
                        ui.label(egui::RichText::new("Regions").strong());
                        for i in 0..net.regions.len() {
                            let own = net.paints_of(i);
                            let shown = own
                                .unwrap_or(node.fills.as_slice())
                                .iter()
                                .find_map(|p| p.visible.then(|| p.primary_color()).flatten());
                            let picked = app.net_region == Some((id, i));
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(picked, format!("Region {}", i + 1))
                                    .clicked()
                                {
                                    app.net_region = Some((id, i));
                                }
                                let mut c =
                                    shown.map_or(egui::Color32::TRANSPARENT, crate::theme::color32);
                                ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                                if ui.color_edit_button_srgba(&mut c).changed() {
                                    let paint = deesainer_doc::Paint::solid(crate::theme::rgba(c));
                                    app.dispatch(
                                        "vector.setRegionFill",
                                        json!({ "id": id, "region": i, "fills": [paint] }),
                                    );
                                }
                                if own.is_some()
                                    && icons::button(ui, Icon::Minus, false, "Use the layer fill")
                                        .clicked()
                                {
                                    app.dispatch(
                                        "vector.setRegionFill",
                                        json!({ "id": id, "region": i, "fills": null }),
                                    );
                                }
                                ui.label(if own.is_some() {
                                    "own fill"
                                } else {
                                    "layer fill"
                                });
                            });
                        }
                        ui.label(
                            egui::RichText::new(
                                "Cmd-click a region on the canvas to pick it. Click a vertex, Shift-click another to join them, drag to move, Alt-click empty canvas to add one, Delete removes the selected vertex. Drag the handles of a selected vertex to bend its segments; double-click a handle to straighten.",
                            )
                            .color(crate::theme::TOKENS.text_secondary),
                        );
                    }
                    None => {
                        if ui
                            .button("Edit as network")
                            .on_hover_text("Lets more than two segments meet at a vertex")
                            .clicked()
                        {
                            app.dispatch("vector.toNetwork", json!({ "id": id }));
                        }
                    }
                }
            },
        );
        return;
    }
    if !matches!(
        kind,
        NodeKind::Polygon { .. } | NodeKind::Star { .. } | NodeKind::Ellipse { .. }
    ) {
        return;
    }
    widgets::section(
        ui,
        "Shape",
        |_| {},
        |ui| {
            let half = widgets::half(ui);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                match kind {
                    NodeKind::Polygon { sides } => {
                        let mut n = f64::from(sides);
                        let resp = widgets::number(ui, Lead::Text("Count"), &mut n, 0.1, "", half);
                        crate::design::edit(
                            app,
                            &resp,
                            "Shape options",
                            "node.setShape",
                            json!({ "id": id, "count": n.round().clamp(3.0, 60.0) as u32 }),
                        );
                    }
                    NodeKind::Star {
                        points,
                        inner_ratio,
                    } => {
                        let mut n = f64::from(points);
                        let resp = widgets::number(ui, Lead::Text("Count"), &mut n, 0.1, "", half);
                        crate::design::edit(
                            app,
                            &resp,
                            "Shape options",
                            "node.setShape",
                            json!({ "id": id, "count": n.round().clamp(3.0, 60.0) as u32 }),
                        );
                        let mut r = inner_ratio * 100.0;
                        let resp = widgets::number(ui, Lead::Text("Ratio"), &mut r, 0.5, "%", half);
                        crate::design::edit(
                            app,
                            &resp,
                            "Shape options",
                            "node.setShape",
                            json!({ "id": id, "innerRatio": r.clamp(0.0, 100.0) / 100.0 }),
                        );
                    }
                    NodeKind::Ellipse {
                        arc_start, arc_end, ..
                    } => {
                        let (mut st, mut en) = (arc_start, arc_end);
                        let resp =
                            widgets::number(ui, Lead::Text("Start"), &mut st, 1.0, "°", half);
                        crate::design::edit(
                            app,
                            &resp,
                            "Arc",
                            "node.setShape",
                            json!({ "id": id, "arcStart": st }),
                        );
                        let resp =
                            widgets::number(ui, Lead::Text("Sweep"), &mut en, 1.0, "°", half);
                        crate::design::edit(
                            app,
                            &resp,
                            "Arc",
                            "node.setShape",
                            json!({ "id": id, "arcEnd": en }),
                        );
                    }
                    _ => {}
                }
            });
            if let NodeKind::Ellipse { inner_ratio, .. } = kind {
                let mut r = inner_ratio * 100.0;
                let resp = widgets::number(ui, Lead::Text("Ratio"), &mut r, 0.5, "%", half);
                crate::design::edit(
                    app,
                    &resp,
                    "Arc",
                    "node.setShape",
                    json!({ "id": id, "innerRatio": r.clamp(0.0, 99.0) / 100.0 }),
                );
            }
        },
    );
}

fn missing_fonts(app: &mut App, ui: &mut egui::Ui) {
    let key = egui::Id::new("missing-fonts");
    let revision = app.session.revision();
    let cached = ui
        .ctx()
        .data(|d| d.get_temp::<(u64, Vec<(String, u64)>)>(key))
        .filter(|(r, _)| *r == revision);
    let missing = match cached {
        Some((_, m)) => m,
        None => {
            let m: Vec<(String, u64)> = app
                .session
                .dispatch("text.missingFonts", &json!({}))
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|m| Some((m["family"].as_str()?.to_owned(), m["layers"].as_u64()?)))
                .collect();
            ui.ctx()
                .data_mut(|d| d.insert_temp(key, (revision, m.clone())));
            m
        }
    };
    if missing.is_empty() {
        return;
    }
    ui.separator();
    ui.strong("Missing fonts");
    ui.label(
        egui::RichText::new("Their text draws in a fallback font.")
            .color(crate::theme::TOKENS.text_secondary),
    );
    let families = app.font_families();
    for (family, layers) in missing {
        let mut picked = None;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("replace-font", &family))
                .selected_text("Replace")
                .width(76.0)
                .height(320.0)
                .show_ui(ui, |ui| {
                    for f in &families {
                        if ui.selectable_label(false, f).clicked() {
                            picked = Some(f.clone());
                        }
                    }
                });
            ui.add(egui::Label::new(format!("{family} ({layers})")).truncate());
        });
        if let Some(to) = picked {
            app.dispatch("text.replaceFont", json!({ "from": family, "to": to }));
        }
    }
}

pub fn page_extras(app: &mut App, ui: &mut egui::Ui) {
    missing_fonts(app, ui);
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong("Local styles");
    });
    let doc = app.session.snapshot();
    let styles: Vec<(u64, String, String)> = doc
        .styles()
        .map(|(id, s)| {
            let kind = match s.kind {
                deesainer_doc::StyleKind::Paint(_) => "paint",
                deesainer_doc::StyleKind::Text(_) => "text",
                deesainer_doc::StyleKind::Effect(_) => "effect",
                deesainer_doc::StyleKind::Grid(_) => "grid",
            };
            (id.0, s.name.clone(), kind.to_owned())
        })
        .collect();
    if styles.is_empty() {
        ui.weak("No styles. Select a layer and use \"Create style\" in its Fill section.");
    }
    for (id, name, kind) in styles {
        ui.horizontal(|ui| {
            ui.label(format!("{name}  ({kind})"));
            if ui.small_button("Delete").clicked() {
                app.dispatch("style.delete", json!({ "id": id }));
            }
        });
    }
    ui.separator();
    if ui.button("Variables…").clicked() {
        app.show_variables = true;
    }
}

pub fn fill_style_row(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let doc = app.session.snapshot();
    ui.horizontal(|ui| {
        if let Some(sid) = node.styles.fill {
            let name = doc.style(sid).map(|s| s.name.clone()).unwrap_or_default();
            ui.label(format!("Style: {name}"));
            if ui.small_button("Detach").clicked() {
                app.dispatch("style.detach", json!({ "ids": [id], "target": "fill" }));
            }
        } else {
            ui.menu_button("Apply style", |ui| {
                for (sid, s) in doc.styles() {
                    if matches!(s.kind, deesainer_doc::StyleKind::Paint(_))
                        && ui.button(&s.name).clicked()
                    {
                        app.dispatch("style.apply", json!({ "ids": [id], "style": sid.0 }));
                        ui.close();
                    }
                }
            });
            if ui.small_button("Create style").clicked() {
                let n = doc.styles().count() + 1;
                app.dispatch(
                    "style.create",
                    json!({ "name": format!("Color {n}"), "kind": "paint", "from": id }),
                );
                if let Some(sid) = app.session.doc().styles().map(|(k, _)| k.0).max() {
                    app.dispatch("style.apply", json!({ "ids": [id], "style": sid }));
                }
            }
        }
    });
}

pub fn grid_style_row(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let doc = app.session.snapshot();
    let grid_styles: Vec<(u64, String)> = doc
        .styles()
        .filter(|(_, s)| matches!(s.kind, deesainer_doc::StyleKind::Grid(_)))
        .map(|(k, s)| (k.0, s.name.clone()))
        .collect();
    if node.styles.grid.is_none() && node.layout_grids.is_empty() && grid_styles.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        if let Some(sid) = node.styles.grid {
            let name = doc.style(sid).map(|s| s.name.clone()).unwrap_or_default();
            icons::glyph(ui, Icon::Styles, crate::theme::TOKENS.text_secondary);
            ui.label(name);
            if ui.small_button("Detach").clicked() {
                app.dispatch("style.detach", json!({ "ids": [id], "target": "grid" }));
            }
            return;
        }
        if !grid_styles.is_empty() {
            ui.menu_button("Apply grid style", |ui| {
                for (sid, name) in &grid_styles {
                    if ui.button(name).clicked() {
                        app.dispatch(
                            "style.apply",
                            json!({ "ids": [id], "style": sid, "target": "grid" }),
                        );
                        ui.close();
                    }
                }
            });
        }
        if !node.layout_grids.is_empty() && ui.small_button("Create style").clicked() {
            let n = grid_styles.len() + 1;
            if let Ok(out) = app.session.dispatch(
                "style.create",
                &json!({ "name": format!("Grid {n}"), "kind": "grid", "from": id }),
            ) {
                app.dispatch(
                    "style.apply",
                    json!({ "ids": [id], "style": out["id"], "target": "grid" }),
                );
            }
        }
    });
    ui.add_space(8.0);
}

pub fn variables_view(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Variables").font(crate::theme::title()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            if icons::button(ui, Icon::Close, false, "Close variables").clicked() {
                app.show_variables = false;
            }
        });
    });
    ui.add_space(6.0);
    widgets::divider(ui);
    ui.add_space(12.0);
    if app.session.doc().variables().count() == 0 {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.label(
                egui::RichText::new("No variables created in this file")
                    .font(crate::theme::title()),
            );
            ui.label(
                egui::RichText::new(
                    "Save colors, numbers and text to reuse them in styles and across layers.",
                )
                .color(crate::theme::TOKENS.text_secondary),
            );
            ui.add_space(8.0);
            if widgets::primary(ui, "+ Create").clicked() {
                app.dispatch(
                    "variable.create",
                    json!({ "name": "var-1", "value": "#1E90FF" }),
                );
            }
        });
        return;
    }
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 0))
        .show(ui, |ui| {
            variables_body(app, ui);
        });
}

fn variables_body(app: &mut App, ui: &mut egui::Ui) {
    {
        let doc = app.session.snapshot();
        let key = egui::Id::new("variables-collection");
        let first = doc
            .collections()
            .map(|(c, _)| c)
            .find(|c| doc.variables().any(|(_, v)| v.collection == *c))
            .unwrap_or_default();
        let mut cid = ui
            .data(|d| d.get_temp::<u64>(key))
            .map_or(first, deesainer_doc::CollectionId);
        if doc.collection(cid).is_none() {
            cid = deesainer_doc::CollectionId::DEFAULT;
        }
        ui.horizontal(|ui| {
            for (c, m) in doc.collections() {
                if ui.selectable_label(c == cid, &m.name).clicked() {
                    cid = c;
                }
            }
            if ui.small_button("+ Collection").clicked() {
                let n = doc.collections().count() + 1;
                if let Ok(out) = app.session.dispatch(
                    "collection.create",
                    &json!({ "name": format!("Collection {n}") }),
                ) {
                    cid = deesainer_doc::CollectionId(out["id"].as_u64().unwrap_or(0));
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(key, cid.0));
        let Some(modes) = doc
            .collection(cid)
            .cloned()
            .or_else(|| app.session.doc().collection(cid).cloned())
        else {
            return;
        };
        ui.horizontal(|ui| {
            let mut name = modes.name.clone();
            if ui.text_edit_singleline(&mut name).lost_focus() && name != modes.name {
                app.dispatch(
                    "collection.rename",
                    json!({ "collection": cid.0, "name": name }),
                );
            }
            if !cid.is_default() && ui.small_button("Delete collection").clicked() {
                app.dispatch("collection.delete", json!({ "collection": cid.0 }));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Default mode");
            for (i, m) in modes.names.iter().enumerate() {
                if ui.selectable_label(i == modes.active, m).clicked() {
                    app.dispatch("mode.setActive", json!({ "index": i, "collection": cid.0 }));
                }
            }
            if ui.small_button("+ Mode").clicked() {
                app.dispatch(
                    "mode.add",
                    json!({ "name": format!("Mode {}", modes.names.len() + 1), "collection": cid.0 }),
                );
            }
        });
        ui.horizontal(|ui| {
            for (label, value) in [
                ("+ Color", json!("#1E90FF")),
                ("+ Number", json!(8)),
                ("+ String", json!("Text")),
            ] {
                if ui.button(label).clicked() {
                    let n = doc.variables().count() + 1;
                    app.dispatch(
                        "variable.create",
                        json!({ "name": format!("var-{n}"), "value": value, "collection": cid.0 }),
                    );
                }
            }
        });
        ui.separator();
        egui::Grid::new("vars").striped(true).show(ui, |ui| {
            ui.strong("Name");
            for m in &modes.names {
                ui.strong(m);
            }
            ui.label("");
            ui.end_row();
            for (vid, var) in doc.variables().filter(|(_, v)| v.collection == cid) {
                let mut name = var.name.clone();
                if ui.text_edit_singleline(&mut name).lost_focus() && name != var.name {
                    app.dispatch("variable.set", json!({ "id": vid.0, "name": name }));
                }
                for (mi, value) in var.values.iter().enumerate() {
                    ui.push_id((vid.0, mi), |ui| match value {
                        deesainer_doc::VarValue::Color(c) => {
                            let mut c32 = crate::theme::color32(*c);
                            if ui.color_edit_button_srgba(&mut c32).changed() {
                                app.dispatch("variable.set", json!({ "id": vid.0, "mode": mi, "value": crate::theme::rgba(c32).to_hex() }));
                            }
                        }
                        deesainer_doc::VarValue::Number(n) => {
                            let mut v = *n;
                            if ui.add(egui::DragValue::new(&mut v)).changed() {
                                app.dispatch("variable.set", json!({ "id": vid.0, "mode": mi, "value": v }));
                            }
                        }
                        deesainer_doc::VarValue::String(s) => {
                            let mut t = s.clone();
                            if ui.text_edit_singleline(&mut t).lost_focus() && &t != s {
                                app.dispatch("variable.set", json!({ "id": vid.0, "mode": mi, "value": t }));
                            }
                        }
                        deesainer_doc::VarValue::Bool(b) => {
                            let mut v = *b;
                            if ui.checkbox(&mut v, "").changed() {
                                app.dispatch("variable.set", json!({ "id": vid.0, "mode": mi, "value": v }));
                            }
                        }
                        deesainer_doc::VarValue::Alias(a) => {
                            let target = doc.variable(*a).map(|v| v.name.clone()).unwrap_or_default();
                            ui.label(format!("→ {target}"));
                        }
                    });
                }
                ui.horizontal(|ui| {
                    let sel = app.session.selection().to_vec();
                    if !sel.is_empty() {
                        ui.menu_button("Bind", |ui| {
                            for (label, target) in [
                                ("Fill", "fillColor"),
                                ("Stroke", "strokeColor"),
                                ("Opacity", "opacity"),
                                ("Radius", "cornerRadius"),
                                ("Width", "width"),
                                ("Height", "height"),
                                (app.tr("Gap"), "gap"),
                                (app.tr("Padding"), "padding"),
                                ("Text", "textContent"),
                                ("Visible", "visible"),
                            ] {
                                let allowed = serde_json::from_value::<deesainer_doc::BindTarget>(json!(target))
                                    .is_ok_and(|t| var.allows(t));
                                if allowed && ui.button(label).clicked() {
                                    app.dispatch("variable.bind", json!({ "ids": sel, "target": target, "variable": vid.0 }));
                                    ui.close();
                                }
                            }
                        });
                    }
                    if ui.small_button("Delete").clicked() {
                        app.dispatch("variable.delete", json!({ "id": vid.0 }));
                    }
                });
                ui.end_row();
            }
        });
    }
}

fn scrolling_section(
    app: &mut App,
    ui: &mut egui::Ui,
    id: NodeId,
    node: &Node,
    doc: &deesainer_doc::Document,
) {
    let in_frame = node
        .parent
        .and_then(|p| doc.get(p))
        .is_some_and(|p| p.kind.is_frame_like());
    if !node.kind.is_frame_like() && !in_frame {
        return;
    }
    widgets::divider(ui);
    widgets::header(ui, "Scrolling", |_| {});
    let full = widgets::half(ui) * 2.0 + 8.0;
    if node.kind.is_frame_like() {
        let keys = ["none", "vertical", "horizontal", "both"];
        let labels: Vec<String> = [
            "No scrolling",
            "Vertical scrolling",
            "Horizontal scrolling",
            "Horizontal and vertical",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let current = keys
            .iter()
            .position(|k| *k == enum_name(node.scrolling.overflow))
            .unwrap_or(0);
        if let Some(i) = pick(ui, full, &labels[current], &labels) {
            app.dispatch(
                "prototype.setScrolling",
                json!({ "ids": [id], "overflow": keys[i] }),
            );
        }
    }
    if in_frame {
        let mut fixed = node.scrolling.fixed;
        if ui
            .checkbox(&mut fixed, "Fix position when scrolling")
            .changed()
        {
            app.dispatch(
                "prototype.setScrolling",
                json!({ "ids": [id], "fixed": fixed }),
            );
        }
    }
    ui.add_space(8.0);
}

#[derive(Clone, Default)]
struct DraftInteraction {
    trigger: String,
    action: String,
    to: Option<NodeId>,
    url: String,
    transition: String,
    variable: Option<u64>,
    value: String,
}

const TRIGGERS: [(&str, &str); 4] = [
    ("click", "On click"),
    ("hover", "While hovering"),
    ("press", "While pressing"),
    ("delay", "After delay"),
];
const ACTIONS: [(&str, &str); 6] = [
    ("navigate", "Navigate to"),
    ("back", "Back"),
    ("overlay", "Open overlay"),
    ("closeOverlay", "Close overlay"),
    ("url", "Open link"),
    ("setVariable", "Set variable"),
];
const ANIMATIONS: [(&str, &str); 5] = [
    ("instant", "Instant"),
    ("dissolve", "Dissolve"),
    ("smartAnimate", "Smart animate"),
    ("moveIn", "Move in"),
    ("push", "Push"),
];

pub(crate) fn prototype_section(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let doc = app.session.snapshot();
    let page = app.session.current_page();
    let frames = deesainer_tools::proto::frames(&doc, page);
    scrolling_section(app, ui, id, node, &doc);
    if node.parent == Some(page) && node.kind.is_frame_like() {
        let starts = deesainer_tools::proto::flow_starts(&doc, page);
        let is_start = starts.contains(&id);
        widgets::divider(ui);
        let mut toggle = false;
        let plus = |ui: &mut egui::Ui| {
            let icon = if is_start { Icon::Minus } else { Icon::Plus };
            icons::button(ui, icon, false, "Flow starting point").clicked()
        };
        if is_start {
            widgets::header(ui, "Flow starting point", |ui| toggle = plus(ui));
            let n = starts.iter().position(|f| *f == id).map_or(1, |i| i + 1);
            ui.horizontal(|ui| {
                icons::glyph(ui, Icon::Play, crate::theme::TOKENS.text_secondary);
                ui.label(format!("Flow {n}"));
            });
            ui.add_space(8.0);
        } else {
            widgets::header_empty(ui, "Flow starting point", |ui| toggle = plus(ui));
        }
        if toggle {
            let cmd = if is_start {
                "prototype.removeFlowStart"
            } else {
                "prototype.setFlowStart"
            };
            app.dispatch(cmd, json!({ "frame": id }));
        }
    }
    widgets::divider(ui);
    let key = egui::Id::new(("draft-interaction", id.0));
    let mut draft: DraftInteraction =
        ui.data_mut(|d| d.get_temp(key))
            .unwrap_or_else(|| DraftInteraction {
                trigger: "click".into(),
                action: "navigate".into(),
                transition: "instant".into(),
                ..Default::default()
            });
    let mut plus_resp = None;
    let header = |ui: &mut egui::Ui, plus_resp: &mut Option<egui::Response>| {
        *plus_resp = Some(icons::button(ui, Icon::Plus, false, "Add interaction"));
    };
    if node.interactions.is_empty() {
        widgets::header_empty(ui, "Interactions", |ui| header(ui, &mut plus_resp));
    } else {
        widgets::header(ui, "Interactions", |ui| header(ui, &mut plus_resp));
    }
    let mut add = false;
    if let Some(resp) = &plus_resp {
        widgets::popover(resp, "Interaction").show(|ui| {
            ui.set_width(240.0);
            if widgets::popover_title(ui, "Interaction") {
                ui.close();
            }
            let w = 160.0;
            widgets::labelled(ui, "Trigger", |ui| {
                if let Some(v) = widgets::choice(ui, w, &draft.trigger, &TRIGGERS) {
                    draft.trigger = v.into();
                }
            });
            widgets::labelled(ui, "Action", |ui| {
                if let Some(v) = widgets::choice(ui, w, &draft.action, &ACTIONS) {
                    draft.action = v.into();
                }
            });
            if matches!(draft.action.as_str(), "navigate" | "overlay") {
                let names: Vec<(NodeId, String)> = frames
                    .iter()
                    .filter(|f| **f != id)
                    .map(|f| (*f, doc.get(*f).map(|n| n.name.clone()).unwrap_or_default()))
                    .collect();
                let current = draft
                    .to
                    .and_then(|t| names.iter().find(|(f, _)| *f == t).map(|(_, n)| n.clone()))
                    .unwrap_or_else(|| "Select a frame".into());
                widgets::labelled(ui, "Destination", |ui| {
                    let r = widgets::select(ui, w, &current);
                    widgets::menu(&r).show(|ui| {
                        for (f, n) in &names {
                            if widgets::menu_item(
                                ui,
                                Some(draft.to == Some(*f)),
                                Some(Icon::Frame),
                                n,
                                "",
                                true,
                            )
                            .clicked()
                            {
                                draft.to = Some(*f);
                                ui.close();
                            }
                        }
                    });
                });
                widgets::labelled(ui, "Animation", |ui| {
                    if let Some(v) = widgets::choice(ui, w, &draft.transition, &ANIMATIONS) {
                        draft.transition = v.into();
                    }
                });
            }
            if draft.action == "url" {
                widgets::labelled(ui, "Link", |ui| {
                    widgets::field(ui, w, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.url)
                                .frame(egui::Frame::NONE)
                                .hint_text("https://"),
                        )
                    });
                });
            }
            if draft.action == "setVariable" {
                let vars: Vec<(u64, String, &'static str)> = doc
                    .variables()
                    .filter_map(|(vid, v)| {
                        doc.resolve_variable(vid)
                            .map(|r| (vid.0, v.name.clone(), r.type_name()))
                    })
                    .collect();
                let current = draft
                    .variable
                    .and_then(|v| vars.iter().find(|(i, ..)| *i == v))
                    .map(|(_, n, _)| n.clone())
                    .unwrap_or_else(|| "Select a variable".into());
                widgets::labelled(ui, "Variable", |ui| {
                    let r = widgets::select(ui, w, &current);
                    widgets::menu(&r).show(|ui| {
                        for (vid, name, ty) in &vars {
                            let label = format!("{name} ({ty})");
                            if widgets::menu_item(
                                ui,
                                Some(draft.variable == Some(*vid)),
                                None,
                                &label,
                                "",
                                true,
                            )
                            .clicked()
                            {
                                draft.variable = Some(*vid);
                                ui.close();
                            }
                        }
                    });
                });
                widgets::labelled(ui, "Value", |ui| {
                    widgets::field(ui, w, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.value)
                                .frame(egui::Frame::NONE)
                                .hint_text("true, 3, #FF0000, text"),
                        )
                    });
                });
            }
            ui.add_space(6.0);
            let ready = match draft.action.as_str() {
                "navigate" | "overlay" => draft.to.is_some(),
                "url" => !draft.url.trim().is_empty(),
                "setVariable" => draft.variable.is_some() && !draft.value.trim().is_empty(),
                _ => true,
            };
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(
                        egui::RichText::new("Add interaction")
                            .color(crate::theme::TOKENS.accent_text),
                    )
                    .fill(crate::theme::TOKENS.accent),
                )
                .clicked()
            {
                add = true;
                ui.close();
            }
        });
    }
    if add {
        let mut p = json!({
            "id": id,
            "trigger": draft.trigger,
            "action": draft.action,
            "transition": draft.transition,
            "duration": 300,
        });
        if let Some(to) = draft.to {
            p["to"] = json!(to);
        }
        if draft.action == "url" {
            p["url"] = json!(draft.url);
        }
        if draft.trigger == "delay" {
            p["delay"] = json!(800);
        }
        if let (true, Some(v)) = (draft.action == "setVariable", draft.variable) {
            let t = draft.value.trim();
            let ty = doc
                .resolve_variable(deesainer_doc::VariableId(v))
                .map_or("string", |r| r.type_name());
            let value = match (ty, t) {
                ("bool", "true") => json!(true),
                ("bool", "false") => json!(false),
                ("number", _) => t.parse::<f64>().map_or_else(|_| json!(t), |n| json!(n)),
                _ => json!(t),
            };
            p["variable"] = json!(v);
            p["value"] = value;
        }
        app.dispatch("prototype.addInteraction", p);
        app.show_prototype = true;
    }
    ui.data_mut(|d| d.insert_temp(key, draft));
    let mut remove = None;
    for (i, it) in node.interactions.iter().enumerate() {
        let trigger = match it.trigger {
            deesainer_doc::Trigger::OnClick => "Click".to_owned(),
            deesainer_doc::Trigger::OnHover => "Hover".to_owned(),
            deesainer_doc::Trigger::OnPress => "Press".to_owned(),
            deesainer_doc::Trigger::AfterDelay { ms } => format!("{ms} ms"),
        };
        let action = match &it.action {
            deesainer_doc::Action::Navigate { to } => {
                doc.get(*to).map(|n| n.name.clone()).unwrap_or_default()
            }
            deesainer_doc::Action::OpenOverlay { frame } => {
                format!(
                    "Overlay {}",
                    doc.get(*frame).map(|n| n.name.clone()).unwrap_or_default()
                )
            }
            deesainer_doc::Action::CloseOverlay => "Close overlay".into(),
            deesainer_doc::Action::Back => "Back".into(),
            deesainer_doc::Action::OpenUrl { url } => url.clone(),
            deesainer_doc::Action::SetVariable { variable, .. } => format!(
                "Set {}",
                doc.variable(*variable)
                    .map(|v| v.name.clone())
                    .unwrap_or_default()
            ),
            deesainer_doc::Action::Conditional { condition, .. } => format!(
                "If {}",
                doc.variable(condition.variable)
                    .map(|v| v.name.clone())
                    .unwrap_or_default()
            ),
        };
        ui.push_id(("interaction", i), |ui| {
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 32.0).max(60.0);
                widgets::field(ui, w, |ui| {
                    ui.label(
                        egui::RichText::new(&trigger).color(crate::theme::TOKENS.text_secondary),
                    );
                    icons::glyph(ui, Icon::ChevronRight, crate::theme::TOKENS.text_secondary);
                    ui.label(action);
                });
                if icons::button(ui, Icon::Minus, false, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        app.dispatch(
            "prototype.removeInteraction",
            json!({ "id": id, "index": i }),
        );
    }
    if !node.interactions.is_empty() {
        ui.add_space(8.0);
    }
}

fn shared_name<T: serde::Serialize>(nodes: &[Node], f: impl Fn(&Node) -> T) -> String {
    let mut names = nodes.iter().map(|n| enum_name(f(n)));
    let first = names.next().unwrap_or_default();
    if names.all(|n| n == first) {
        first
    } else {
        "Mixed".into()
    }
}

fn shared_f64(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<f64> = values.collect();
    let first = *values.first()?;
    values
        .iter()
        .all(|v| (v - first).abs() < 1e-6)
        .then_some(first)
}

fn dispatch_each(
    app: &mut App,
    label: &str,
    command: &str,
    ids: &[NodeId],
    params: impl Fn(NodeId) -> serde_json::Value,
) {
    app.session.begin_group(label);
    for id in ids {
        app.dispatch(command, params(*id));
    }
    app.session.end_group();
}

pub fn multi_sections(app: &mut App, ui: &mut egui::Ui, nodes: &[Node]) {
    if nodes.is_empty() {
        return;
    }
    let doc = app.session.snapshot();
    let ids: Vec<NodeId> = nodes.iter().map(|n| n.id).collect();
    let parents: Vec<Option<&Node>> = nodes
        .iter()
        .map(|n| {
            n.parent
                .and_then(|p| doc.get(p))
                .filter(|p| !matches!(p.kind, NodeKind::Page | NodeKind::Document))
        })
        .collect();
    let all_in_layout = parents
        .iter()
        .all(|p| p.is_some_and(|p| p.layout.is_some()));
    let all_constrained = parents
        .iter()
        .all(|p| p.is_some_and(|p| p.layout.is_none() && p.kind.is_frame_like()));
    if all_in_layout {
        widgets::section(
            ui,
            app.tr("Resizing"),
            |_| {},
            |ui| {
                let half = widgets::half(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let axes: [(&str, String); 2] = [
                        (
                            "horizontal",
                            shared_name(nodes, |n| n.layout_child.horizontal),
                        ),
                        ("vertical", shared_name(nodes, |n| n.layout_child.vertical)),
                    ];
                    for (key, now) in axes {
                        if let Some(v) = widgets::choice(ui, half, &now, &sizing(app)) {
                            app.dispatch("layout.setChild", json!({ "ids": ids, key: v }));
                        }
                    }
                });
            },
        );
    } else if all_constrained {
        let word = |name: String, pairs: [(&str, &str); 4]| -> String {
            pairs.iter().find(|(k, _)| *k == name).map_or_else(
                || {
                    if name == "Mixed" {
                        name.clone()
                    } else {
                        "scale".into()
                    }
                },
                |(_, v)| (*v).to_owned(),
            )
        };
        let h_now = word(
            shared_name(nodes, |n| n.constraints.horizontal),
            [
                ("min", "left"),
                ("max", "right"),
                ("center", "center"),
                ("stretch", "leftRight"),
            ],
        );
        let v_now = word(
            shared_name(nodes, |n| n.constraints.vertical),
            [
                ("min", "top"),
                ("max", "bottom"),
                ("center", "center"),
                ("stretch", "topBottom"),
            ],
        );
        widgets::section(
            ui,
            app.tr("Constraints"),
            |_| {},
            |ui| {
                let half = widgets::half(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let h = [
                        ("left", app.tr("Left")),
                        ("right", app.tr("Right")),
                        ("leftRight", app.tr("Left and right")),
                        ("center", app.tr("Center")),
                        ("scale", app.tr("Scale")),
                    ];
                    if let Some(v) = widgets::choice(ui, half, &h_now, &h) {
                        app.dispatch(
                            "node.setConstraints",
                            json!({ "ids": ids, "horizontal": v }),
                        );
                    }
                    let v = [
                        ("top", app.tr("Top")),
                        ("bottom", app.tr("Bottom")),
                        ("topBottom", app.tr("Top and bottom")),
                        ("center", app.tr("Center")),
                        ("scale", app.tr("Scale")),
                    ];
                    if let Some(k) = widgets::choice(ui, half, &v_now, &v) {
                        app.dispatch("node.setConstraints", json!({ "ids": ids, "vertical": k }));
                    }
                });
            },
        );
    }
    let layouts: Vec<_> = nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Frame | NodeKind::Component { .. }))
        .filter_map(|n| n.layout)
        .collect();
    if layouts.len() != nodes.len() {
        return;
    }
    multi_auto_layout(app, ui, nodes, &ids);
}

fn multi_auto_layout(app: &mut App, ui: &mut egui::Ui, nodes: &[Node], ids: &[NodeId]) {
    let layout = |n: &Node| n.layout.unwrap_or_default();
    let direction = shared_name(nodes, |n| layout(n).direction);
    let mut remove = false;
    widgets::section(
        ui,
        "Auto layout",
        |ui| {
            remove = icons::button(ui, Icon::Minus, false, "Remove auto layout").clicked();
        },
        |ui| {
            let half = widgets::half(ui);
            let horizontal = direction == "horizontal";
            if direction == "Mixed" {
                ui.label(egui::RichText::new("Directions differ").weak());
            } else {
                let (w_key, h_key) = if horizontal {
                    ("primarySizing", "counterSizing")
                } else {
                    ("counterSizing", "primarySizing")
                };
                let now = |key: &str| {
                    if key == "primarySizing" {
                        shared_name(nodes, |n| layout(n).primary_sizing)
                    } else {
                        shared_name(nodes, |n| layout(n).counter_sizing)
                    }
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for key in [w_key, h_key] {
                        if let Some(v) = widgets::choice(ui, half, &now(key), &sizing(app)[..2]) {
                            dispatch_each(
                                app,
                                "Sizing",
                                "layout.setAutoLayout",
                                ids,
                                |id| json!({ "id": id, key: v }),
                            );
                        }
                    }
                });
                let p = shared_name(nodes, |n| layout(n).primary_align);
                let c = shared_name(nodes, |n| layout(n).counter_align);
                let index = |a: &str| match a {
                    "center" => Some(1),
                    "end" => Some(2),
                    "start" => Some(0),
                    _ => None,
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if let (Some(pi), Some(ci)) = (index(&p), index(&c)) {
                        let active = if horizontal { (pi, ci) } else { (ci, pi) };
                        if let Some((col, row)) = widgets::align_grid(ui, half, active, horizontal) {
                            let names = ["start", "center", "end"];
                            let (a, b) = if horizontal { (col, row) } else { (row, col) };
                            dispatch_each(app, "Alignment", "layout.setAutoLayout", ids, |id| {
                                json!({ "id": id, "primaryAlign": names[a], "counterAlign": names[b] })
                            });
                        }
                    } else {
                        ui.label(egui::RichText::new("Alignment differs").weak());
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let gaps = shared_f64(nodes.iter().map(|n| layout(n).gap));
                let (resp, v) =
                    widgets::number_mixed(ui, app.tr("Gap"), gaps, layout(&nodes[0]).gap, half);
                if let Some(v) = v {
                    let steps = ids
                        .iter()
                        .map(|id| {
                            (
                                "layout.setAutoLayout".to_owned(),
                                json!({ "id": id, "gap": v.max(0.0) }),
                            )
                        })
                        .collect();
                    crate::design::edit_each(app, &resp, app.tr("Gap"), steps);
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let first = layout(&nodes[0]).padding;
                let ph = shared_f64(nodes.iter().flat_map(|n| {
                    let p = layout(n).padding;
                    [p.left, p.right]
                }));
                let (resp, v) = widgets::number_mixed(ui, "H", ph, first.left, half);
                if let Some(v) = v {
                    let steps = nodes
                        .iter()
                        .map(|n| {
                            let p = layout(n).padding;
                            ("layout.setAutoLayout".to_owned(), json!({ "id": n.id, "padding": { "top": p.top, "bottom": p.bottom, "left": v.max(0.0), "right": v.max(0.0) } }))
                        })
                        .collect();
                    crate::design::edit_each(app, &resp, app.tr("Padding"), steps);
                }
                let pv = shared_f64(nodes.iter().flat_map(|n| {
                    let p = layout(n).padding;
                    [p.top, p.bottom]
                }));
                let (resp, v) = widgets::number_mixed(ui, "V", pv, first.top, half);
                if let Some(v) = v {
                    let steps = nodes
                        .iter()
                        .map(|n| {
                            let p = layout(n).padding;
                            ("layout.setAutoLayout".to_owned(), json!({ "id": n.id, "padding": { "top": v.max(0.0), "bottom": v.max(0.0), "left": p.left, "right": p.right } }))
                        })
                        .collect();
                    crate::design::edit_each(app, &resp, app.tr("Padding"), steps);
                }
            });
        },
    );
    if remove {
        dispatch_each(
            app,
            "Remove auto layout",
            "layout.setAutoLayout",
            ids,
            |id| json!({ "id": id, "direction": "none" }),
        );
    }
}
