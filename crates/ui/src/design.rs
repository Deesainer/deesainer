use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{
    Effect, LineHeight, Node, NodeId, NodeKind, Paint, PaintKind, StrokeAlign, TextStyle,
};
use deesainer_geom::Point;
use serde_json::{json, Value};

use crate::icons::{self, Icon};
use crate::theme::{color32, rgba};
use crate::widgets::{self, Lead};
use crate::App;

pub(crate) fn edit(
    app: &mut App,
    resp: &egui::Response,
    label: &str,
    command: &str,
    params: Value,
) {
    if resp.drag_started() && !app.drag_group_open {
        app.session.begin_group(label);
        app.drag_group_open = true;
    }
    if resp.changed() {
        app.dispatch(command, params);
    }
    if (resp.drag_stopped() || !resp.dragged())
        && app.drag_group_open
        && !resp.is_pointer_button_down_on()
    {
        app.session.end_group();
        app.drag_group_open = false;
    }
}

pub(crate) fn edit_each(
    app: &mut App,
    resp: &egui::Response,
    label: &str,
    steps: Vec<(String, Value)>,
) {
    if resp.drag_started() && !app.drag_group_open {
        app.session.begin_group(label);
        app.drag_group_open = true;
    }
    if resp.changed() {
        let own_group = !app.drag_group_open;
        if own_group {
            app.session.begin_group(label);
        }
        for (cmd, params) in steps {
            app.dispatch(&cmd, params);
        }
        if own_group {
            app.session.end_group();
        }
    }
    if (resp.drag_stopped() || !resp.dragged())
        && app.drag_group_open
        && !resp.is_pointer_button_down_on()
    {
        app.session.end_group();
        app.drag_group_open = false;
    }
}

fn nodes_of(app: &App, ids: &[NodeId]) -> Vec<Node> {
    let doc = app.session.doc();
    ids.iter().filter_map(|i| doc.get(*i).cloned()).collect()
}

fn differ<T: PartialEq>(nodes: &[Node], f: impl Fn(&Node) -> T) -> bool {
    let mut values = nodes.iter().map(f);
    match values.next() {
        Some(first) => values.any(|v| v != first),
        None => false,
    }
}

fn common(values: &[f64]) -> Option<f64> {
    let first = *values.first()?;
    values
        .iter()
        .all(|v| (v - first).abs() < 1e-6)
        .then_some(first)
}

fn multi_geometry(app: &mut App, ui: &mut egui::Ui, nodes: &[Node]) {
    let w = widgets::half(ui);
    let get = |f: fn(&Node) -> f64| nodes.iter().map(f).collect::<Vec<_>>();
    let xs = get(|n| n.rect.x);
    let ys = get(|n| n.rect.y);
    let ws = get(|n| n.rect.width);
    let hs = get(|n| n.rect.height);
    let rs = get(|n| n.rotation);
    let ids: Vec<NodeId> = nodes.iter().map(|n| n.id).collect();
    let row = |ui: &mut egui::Ui,
               app: &mut App,
               a: (&str, &[f64], &str),
               b: Option<(&str, &[f64], &str)>| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (lead, vals, key) in std::iter::once(a).chain(b) {
                let (resp, v) = widgets::number_mixed(
                    ui,
                    lead,
                    common(vals),
                    vals.first().copied().unwrap_or(0.0),
                    w,
                );
                if let Some(v) = v {
                    let steps = ids
                        .iter()
                        .map(|id| match key {
                            "x" | "y" => {
                                ("node.setPosition".to_owned(), json!({ "id": id, key: v }))
                            }
                            "width" | "height" => (
                                "node.resize".to_owned(),
                                json!({ "id": id, key: v.max(0.0) }),
                            ),
                            _ => (
                                "node.setRotation".to_owned(),
                                json!({ "ids": [id], "degrees": v }),
                            ),
                        })
                        .collect();
                    edit_each(app, &resp, app.tr("Edit selection"), steps);
                } else {
                    edit_each(app, &resp, app.tr("Edit selection"), Vec::new());
                }
            }
        });
    };
    widgets::section(
        ui,
        app.tr("Position"),
        |_| {},
        |ui| {
            align_row(app, ui);
            row(ui, app, ("X", &xs, "x"), Some(("Y", &ys, "y")));
            row(ui, app, ("rotation", &rs, "rotation"), None);
        },
    );
    widgets::section(
        ui,
        "Layout",
        |_| {},
        |ui| {
            row(ui, app, ("W", &ws, "width"), Some(("H", &hs, "height")));
        },
    );
}

fn multi_typography(app: &mut App, ui: &mut egui::Ui, ids: &[NodeId], nodes: &[Node]) {
    let styles: Vec<&TextStyle> = nodes
        .iter()
        .filter_map(|n| n.text().map(|t| &t.style))
        .collect();
    let same = |f: &dyn Fn(&TextStyle) -> String| -> Option<String> {
        let first = f(styles.first()?);
        styles.iter().all(|s| f(s) == first).then_some(first)
    };
    let families = app.font_families();
    widgets::section(
        ui,
        app.tr("Typography"),
        |_| {},
        |ui| {
            let full = widgets::half(ui) * 2.0 + 8.0;
            let half = widgets::half(ui);
            let family = same(&|s| s.font_family.clone());
            let resp = widgets::select(ui, full, family.as_deref().unwrap_or("Mixed"));
            widgets::popover(&resp, "Fonts").show(|ui| {
                ui.set_width(240.0);
                if widgets::popover_title(ui, "Fonts") {
                    ui.close();
                }
                if let Some(f) = font_list(app, ui, &families, family.as_deref()) {
                    app.dispatch("text.setStyle", json!({ "ids": ids, "fontFamily": f }));
                    ui.close();
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let weight = same(&|s| s.font_weight.to_string());
                let label = weight
                    .as_deref()
                    .and_then(|w| w.parse::<u16>().ok())
                    .and_then(|w| {
                        WEIGHTS
                            .iter()
                            .find(|(v, _)| *v == w)
                            .map(|(_, n)| (*n).to_owned())
                    })
                    .unwrap_or_else(|| "Mixed".to_owned());
                let wr = widgets::select(ui, half, &label);
                widgets::menu(&wr).show(|ui| {
                    for (w, n) in WEIGHTS {
                        let on = weight.as_deref() == Some(w.to_string().as_str());
                        if widgets::menu_item(ui, Some(on), None, n, "", true).clicked() {
                            app.dispatch("text.setStyle", json!({ "ids": ids, "fontWeight": w }));
                            ui.close();
                        }
                    }
                });
                let sizes: Vec<f64> = styles.iter().map(|s| s.font_size).collect();
                let (resp, v) =
                    widgets::number_mixed(ui, app.tr("Size"), common(&sizes), sizes[0], half);
                if let Some(v) = v {
                    edit_each(
                        app,
                        &resp,
                        app.tr("Font size"),
                        vec![(
                            "text.setStyle".into(),
                            json!({ "ids": ids, "fontSize": v.max(0.5) }),
                        )],
                    );
                } else {
                    edit_each(app, &resp, app.tr("Font size"), Vec::new());
                }
            });
            let spacing: Vec<f64> = styles.iter().map(|s| s.letter_spacing).collect();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (resp, v) = widgets::number_mixed(
                    ui,
                    app.tr("Tracking"),
                    common(&spacing),
                    spacing[0],
                    half,
                );
                if let Some(v) = v {
                    edit_each(
                        app,
                        &resp,
                        app.tr("Letter spacing"),
                        vec![(
                            "text.setStyle".into(),
                            json!({ "ids": ids, "letterSpacing": v }),
                        )],
                    );
                } else {
                    edit_each(app, &resp, app.tr("Letter spacing"), Vec::new());
                }
            });
            let align = nodes
                .iter()
                .filter_map(|n| n.text().map(|t| enum_str(t.align_h)))
                .collect::<Vec<_>>();
            let current = align
                .first()
                .filter(|a| align.iter().all(|x| x == *a))
                .cloned();
            let keys = ["left", "center", "right", "justified"];
            let items = [
                (Icon::TextLeft, "Left"),
                (Icon::TextCenter, "Center"),
                (Icon::TextRight, "Right"),
                (Icon::TextJustify, "Justified"),
            ];
            if let Some(i) = widgets::segmented(
                ui,
                &items,
                current
                    .as_deref()
                    .and_then(|c| keys.iter().position(|k| *k == c)),
                32.0,
            ) {
                app.dispatch("text.setStyle", json!({ "ids": ids, "align": keys[i] }));
            }
        },
    );
}

fn enum_str<T: serde::Serialize>(v: T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub fn panel(app: &mut App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let selection = app.session.selection().to_vec();
            let doc = app.session.snapshot();
            match selection.as_slice() {
                [] => {
                    page_section(app, ui);
                    crate::design_ext::page_extras(app, ui);
                    let page = app.session.current_page();
                    export_section(app, ui, page);
                }
                [id] => {
                    if app.instance_layer.is_some_and(|(i, _)| i != *id) {
                        app.instance_layer = None;
                    }
                    if let Some(node) = doc.get(*id).cloned() {
                        if let Some((_, target)) = app.instance_layer {
                            crate::design_ext::instance_layer_section(app, ui, &node, target);
                        }
                        node_sections(app, ui, *id, &node);
                    }
                }
                ids => {
                    widgets::divider(ui);
                    widgets::title_row(ui, &format!("{} selected", ids.len()), |ui| {
                        selection_actions(app, ui)
                    });
                    let nodes: Vec<Node> =
                        ids.iter().filter_map(|i| doc.get(*i).cloned()).collect();
                    multi_geometry(app, ui, &nodes);
                    if !nodes.is_empty() && nodes.iter().all(|n| n.text().is_some()) {
                        multi_typography(app, ui, &selection, &nodes);
                    }
                    crate::design_ext::multi_sections(app, ui, &nodes);
                    if let Some(first) = nodes.first() {
                        layer_section(app, ui, &selection, first);
                        fill_section(app, ui, &selection, first);
                        stroke_section(app, ui, &selection, first);
                        effects_section(app, ui, &selection, first);
                    }
                }
            }
        });
}

pub fn prototype_panel(app: &mut App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let selection = app.session.selection().to_vec();
            let doc = app.session.snapshot();
            if let [id] = selection.as_slice() {
                if let Some(node) = doc.get(*id).cloned() {
                    crate::design_ext::prototype_section(app, ui, *id, &node);
                    return;
                }
            }
            widgets::section(ui, app.tr("Device"), |_| {}, |ui| {
                let page = app.session.current_page();
                let current = doc.get(page).map(|p| p.device).unwrap_or_default();
                let keys = ["none", "phone", "tablet", "browser"];
                let labels = [app.tr("None"), app.tr("Phone"), app.tr("Tablet"), app.tr("Browser window")];
                let cur = keys
                    .iter()
                    .position(|k| *k == enum_str(current))
                    .unwrap_or(0);
                let full = widgets::half(ui) * 2.0 + 8.0;
                let resp = widgets::select(ui, full, labels[cur]);
                widgets::menu(&resp).show(|ui| {
                    for (i, l) in labels.iter().enumerate() {
                        if widgets::menu_item(ui, Some(i == cur), None, l, "", true).clicked() {
                            app.dispatch("prototype.setDevice", json!({ "device": keys[i] }));
                            ui.close();
                        }
                    }
                });
            });
            widgets::section(ui, app.tr("Flows"), |_| {}, |ui| {
                let page = app.session.current_page();
                let starts = deesainer_tools::proto::flow_starts(&doc, page);
                if starts.is_empty() {
                    ui.label(
                        egui::RichText::new(app.tr("Select a top-level frame to add a starting point and interactions."))
                            .color(crate::theme::TOKENS.text_secondary),
                    );
                }
                for f in starts {
                    let name = doc.get(f).map(|n| n.name.clone()).unwrap_or_default();
                    ui.horizontal(|ui| {
                        icons::glyph(ui, Icon::Play, crate::theme::TOKENS.text_secondary);
                        if ui.link(name).clicked() {
                            app.dispatch("selection.set", json!({ "ids": [f] }));
                        }
                    });
                }
            });
            widgets::section(ui, app.tr("Prototype settings"), |_| {}, |ui| {
                ui.checkbox(&mut app.show_prototype, "Show connections on canvas");
                if ui.button("Present").clicked() {
                    crate::player::present(app);
                }
            });
        });
}

fn page_section(app: &mut App, ui: &mut egui::Ui) {
    let page = app.session.current_page();
    let mut c = color32(deesainer_render::page_background(app.session.doc(), page));
    widgets::section(
        ui,
        app.tr("Page"),
        |_| {},
        |ui| {
            ui.horizontal(|ui| {
                let w = widgets::half(ui) * 2.0 + 8.0;
                let mut pct = (f64::from(c.a()) / 2.55).round();
                let (cc, oc) = widgets::color_pct_field(ui, &mut c, &mut pct, w);
                if oc {
                    let [r, g, b, _] = c.to_srgba_unmultiplied();
                    c = egui::Color32::from_rgba_unmultiplied(
                        r,
                        g,
                        b,
                        (pct.clamp(0.0, 100.0) * 2.55).round() as u8,
                    );
                }
                if cc || oc {
                    app.dispatch("page.setBackground", json!({ "color": rgba(c).to_hex() }));
                }
            });
        },
    );
}

fn align_row(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let h = [
            (Icon::AlignLeft, "Align left (⌥A)"),
            (Icon::AlignHCenter, "Align horizontal centers (⌥H)"),
            (Icon::AlignRight, "Align right (⌥D)"),
        ];
        let v = [
            (Icon::AlignTop, "Align top (⌥W)"),
            (Icon::AlignVCenter, "Align vertical centers (⌥V)"),
            (Icon::AlignBottom, "Align bottom (⌥S)"),
        ];
        let cell = ((widgets::half(ui) - 4.0) / 3.0).floor();
        if let Some(i) = widgets::segmented(ui, &h, None, cell) {
            let cmd = ["align.left", "align.horizontalCenter", "align.right"][i];
            app.dispatch(cmd, Value::Null);
        }
        if let Some(i) = widgets::segmented(ui, &v, None, cell) {
            let cmd = ["align.top", "align.verticalCenter", "align.bottom"][i];
            app.dispatch(cmd, Value::Null);
        }
        if app.session.selection().len() > 2 {
            let d = [
                (Icon::DistributeH, app.tr("Distribute horizontal spacing")),
                (Icon::DistributeV, app.tr("Distribute vertical spacing")),
            ];
            if let Some(i) = widgets::segmented(ui, &d, None, 24.0) {
                let cmd = ["distribute.horizontal", "distribute.vertical"][i];
                app.dispatch(cmd, Value::Null);
            }
        }
    });
}

fn display_type(app: &App, kind: &NodeKind) -> String {
    let raw = match kind {
        NodeKind::BooleanOp { .. } => app.tr("boolean group"),
        k => k.type_name(),
    };
    let mut out = String::new();
    for (i, c) in raw.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn selection_actions(app: &mut App, ui: &mut egui::Ui) {
    let more = icons::button(ui, Icon::More, false, app.tr("More actions"));
    crate::widgets::menu(&more).show(|ui| {
        for (label, cmd, key) in [
            (app.tr("Union"), "object.booleanUnion", "⌥⇧U"),
            (app.tr("Subtract"), "object.booleanSubtract", "⌥⇧S"),
            (app.tr("Intersect"), "object.booleanIntersect", "⌥⇧I"),
            (app.tr("Exclude"), "object.booleanExclude", "⌥⇧E"),
            (app.tr("Flatten"), "object.flatten", "⌘E"),
        ] {
            let enabled = app.session.is_enabled(cmd);
            if widgets::menu_item(ui, None, Some(Icon::Boolean), label, key, enabled).clicked() {
                app.dispatch(cmd, Value::Null);
                ui.close();
            }
        }
        widgets::menu_separator(ui);
        for (label, cmd, key) in [
            (app.tr("Group selection"), "object.group", "⌘G"),
            (
                app.tr("Combine as variants"),
                "component.combineAsVariants",
                "",
            ),
        ] {
            let enabled = app.session.is_enabled(cmd);
            if widgets::menu_item(ui, None, None, label, key, enabled).clicked() {
                app.dispatch(cmd, Value::Null);
                ui.close();
            }
        }
    });
    if icons::button(ui, Icon::Component, false, "Create component (⌥⌘K)").clicked() {
        app.dispatch("component.create", Value::Null);
    }
}

fn node_sections(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    widgets::title_row(ui, &display_type(app, &node.kind), |ui| {
        selection_actions(app, ui)
    });
    widgets::section(
        ui,
        app.tr("Position"),
        |_| {},
        |ui| position_body(app, ui, id, node),
    );
    widgets::section(
        ui,
        app.tr("Layout"),
        |_| {},
        |ui| layout_body(app, ui, id, node),
    );
    layer_section(app, ui, &[id], node);
    if let NodeKind::Text(_) = node.kind {
        text_section(app, ui, id, node);
    }
    crate::design_ext::extra_sections(app, ui, id, node);
    fill_section(app, ui, &[id], node);
    stroke_section(app, ui, &[id], node);
    effects_section(app, ui, &[id], node);
    if node.kind.is_frame_like() {
        layout_guides_section(app, ui, id, node);
    }
    export_section(app, ui, id);
}

fn position_body(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    align_row(app, ui);
    let mut r = node.rect;
    let mut rot = node.rotation;
    let w = widgets::half(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let resp = widgets::number(ui, Lead::Text("X"), &mut r.x, 1.0, "", w);
        edit(
            app,
            &resp,
            "Move",
            "node.setPosition",
            json!({ "id": id, "x": r.x }),
        );
        let resp = widgets::number(ui, Lead::Text("Y"), &mut r.y, 1.0, "", w);
        edit(
            app,
            &resp,
            "Move",
            "node.setPosition",
            json!({ "id": id, "y": r.y }),
        );
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let resp = widgets::number(ui, Lead::Icon(Icon::Angle), &mut rot, 1.0, "°", w);
        edit(
            app,
            &resp,
            "Rotate",
            "node.setRotation",
            json!({ "ids": [id], "degrees": rot }),
        );
        let items = [
            (Icon::Rotate, "Rotate 90° right"),
            (Icon::FlipH, "Flip horizontal (⇧H)"),
            (Icon::FlipV, "Flip vertical (⇧V)"),
        ];
        let cell = ((w - 4.0) / 3.0).floor();
        match widgets::segmented(ui, &items, None, cell) {
            Some(0) => {
                let deg = node.rotation + 90.0;
                app.dispatch("node.setRotation", json!({ "ids": [id], "degrees": deg }));
            }
            Some(1) => {
                app.dispatch("object.flipHorizontal", Value::Null);
            }
            Some(2) => {
                app.dispatch("object.flipVertical", Value::Null);
            }
            _ => {}
        }
    });
}

fn layout_body(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    if matches!(node.kind, NodeKind::Frame | NodeKind::Component { .. }) {
        let current = node.layout.map(|l| enum_str(l.direction));
        let sel = match current.as_deref() {
            Some("vertical") => 1,
            Some("horizontal") => 2,
            Some("grid") => 3,
            _ => 0,
        };
        let items = [
            (Icon::LayoutFree, "Freeform"),
            (Icon::LayoutVertical, "Vertical layout"),
            (Icon::LayoutHorizontal, "Horizontal layout (⇧A)"),
            (Icon::LayoutGrid, "Grid layout"),
        ];
        if let Some(i) = widgets::segmented(ui, &items, Some(sel), 32.0) {
            if i != sel {
                if node.layout.is_none() {
                    app.dispatch("layout.addAutoLayout", json!({ "ids": [id] }));
                }
                let dir = ["none", "vertical", "horizontal", "grid"][i];
                app.dispatch(
                    "layout.setAutoLayout",
                    json!({ "id": id, "direction": dir }),
                );
            }
        }
    }
    if let Some(t) = node.text() {
        let current = enum_str(t.auto_resize);
        let keys = ["widthAndHeight", "height", "none"];
        let sel = keys.iter().position(|k| *k == current);
        let items = [
            (Icon::AutoWidth, "Auto width"),
            (Icon::AutoHeight, "Auto height"),
            (Icon::FixedSize, "Fixed size"),
        ];
        if let Some(i) = widgets::segmented(ui, &items, sel, 32.0) {
            set_layer_text_style(app, id, json!({ "ids": [id], "autoResize": keys[i] }));
        }
    }
    let mut r = node.rect;
    let w = widgets::half(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let resp = widgets::number(ui, Lead::Text("W"), &mut r.width, 1.0, "", w);
        edit(
            app,
            &resp,
            "Resize",
            "node.resize",
            json!({ "id": id, "width": r.width.max(0.0) }),
        );
        let resp = widgets::number(ui, Lead::Text("H"), &mut r.height, 1.0, "", w);
        edit(
            app,
            &resp,
            "Resize",
            "node.resize",
            json!({ "id": id, "height": r.height.max(0.0) }),
        );
    });
    size_limits(app, ui, id, node);
    if node.kind.is_frame_like() {
        let mut clip = node.clips_content;
        if ui.checkbox(&mut clip, "Clip content").changed() {
            app.dispatch(
                "node.setProps",
                json!({ "ids": [id], "clipsContent": clip }),
            );
        }
    }
}

fn size_limits(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let in_layout = node.layout.is_some()
        || node
            .parent
            .and_then(|p| app.session.doc().get(p))
            .is_some_and(|p| p.layout.is_some());
    if !in_layout {
        return;
    }
    let l = node.size_limits;
    let entries = [
        ("Min W", "minWidth", l.min_width, node.rect.width),
        ("Max W", "maxWidth", l.max_width, node.rect.width),
        ("Min H", "minHeight", l.min_height, node.rect.height),
        ("Max H", "maxHeight", l.max_height, node.rect.height),
    ];
    for (label, key, value, _) in entries {
        let Some(mut v) = value else { continue };
        ui.push_id(key, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let w = ui.available_width() - 32.0;
                let resp = widgets::number(ui, Lead::Text(label), &mut v, 1.0, "", w);
                let mut params = json!({ "ids": [id] });
                params[key] = json!(v.max(0.0));
                edit(app, &resp, "Size limit", "node.setProps", params);
                if icons::button(ui, Icon::Minus, false, &format!("Remove {label}")).clicked() {
                    let mut params = json!({ "ids": [id] });
                    params[key] = Value::Null;
                    app.dispatch("node.setProps", params);
                }
            });
        });
    }
    if entries.iter().any(|e| e.2.is_none()) {
        let add = ui.add(
            egui::Button::new(
                egui::RichText::new("Min/max…").color(crate::theme::TOKENS.text_secondary),
            )
            .frame(false),
        );
        widgets::menu(&add).show(|ui| {
            for (label, key, value, current) in entries {
                if value.is_none()
                    && widgets::menu_item(ui, None, None, &format!("Add {label}"), "", true)
                        .clicked()
                {
                    let mut params = json!({ "ids": [id] });
                    params[key] = json!(current.max(0.0).round());
                    app.dispatch("node.setProps", params);
                    ui.close();
                }
            }
        });
    }
}

const BLEND_MODES: [BlendMode; 17] = [
    BlendMode::PassThrough,
    BlendMode::Normal,
    BlendMode::Darken,
    BlendMode::Multiply,
    BlendMode::ColorBurn,
    BlendMode::Lighten,
    BlendMode::Screen,
    BlendMode::ColorDodge,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

fn layer_section(app: &mut App, ui: &mut egui::Ui, ids: &[NodeId], node: &Node) {
    let nodes = nodes_of(app, ids);
    let mixed_blend = differ(&nodes, |n| n.blend);
    let mixed_opacity = differ(&nodes, |n| n.opacity);
    let mixed_radius = differ(&nodes, |n| n.corner_radius);
    let current = if mixed_blend {
        "Mixed".to_owned()
    } else {
        enum_str(node.blend)
    };
    let visible = node.visible;
    let mut new_blend = None;
    let mut corners_row: Option<[f64; 4]> = None;
    let mut toggle = false;
    widgets::section(
        ui,
        "Appearance",
        |ui| {
            let blend = icons::button(ui, Icon::Opacity, false, &format!("Blend mode: {current}"));
            crate::widgets::menu(&blend).show(|ui| {
                for mode in BLEND_MODES {
                    let name = enum_str(mode);
                    if widgets::menu_item(ui, Some(name == current), None, &name, "", true)
                        .clicked()
                    {
                        new_blend = Some(name);
                        ui.close();
                    }
                }
            });
            let eye = if visible { Icon::Eye } else { Icon::EyeOff };
            toggle = icons::button(ui, eye, false, "Show/Hide").clicked();
        },
        |ui| {
            let w = widgets::half(ui);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if mixed_opacity {
                    let (resp, v) = widgets::number_mixed(ui, "opacity", None, 100.0, w);
                    let pct = v.unwrap_or(100.0);
                    edit(
                        app,
                        &resp,
                        "Opacity",
                        "node.setProps",
                        json!({ "ids": ids, "opacity": (pct / 100.0).clamp(0.0, 1.0) }),
                    );
                } else {
                    let mut pct = f64::from(node.opacity) * 100.0;
                    let resp =
                        widgets::number(ui, Lead::Icon(Icon::Opacity), &mut pct, 0.5, "%", w);
                    edit(
                        app,
                        &resp,
                        "Opacity",
                        "node.setProps",
                        json!({ "ids": ids, "opacity": (pct / 100.0).clamp(0.0, 1.0) }),
                    );
                }
                if matches!(
                    node.kind,
                    NodeKind::Rectangle
                        | NodeKind::Frame
                        | NodeKind::Component { .. }
                        | NodeKind::Instance { .. }
                ) {
                    let r = node.corner_radius;
                    let radii = [r.top_left, r.top_right, r.bottom_right, r.bottom_left];
                    let key = egui::Id::new(("independent-corners", node.id.0));
                    let mixed = radii.iter().any(|v| *v != radii[0]);
                    let split = mixed || ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
                    if mixed_radius && !split {
                        let (resp, v) = widgets::number_mixed(ui, "radius", None, 0.0, w);
                        edit(
                            app,
                            &resp,
                            "Corner radius",
                            "node.setProps",
                            json!({ "ids": ids, "cornerRadius": v.unwrap_or(0.0).max(0.0) }),
                        );
                    } else if split {
                        corners_row = Some(radii);
                    } else {
                        let mut radius = r.top_left;
                        let resp =
                            widgets::number(ui, Lead::Icon(Icon::Radius), &mut radius, 0.5, "", w);
                        edit(
                            app,
                            &resp,
                            "Corner radius",
                            "node.setProps",
                            json!({ "ids": ids, "cornerRadius": radius.max(0.0) }),
                        );
                    }
                    if icons::button(ui, Icon::Corners, split, "Independent corners").clicked() {
                        if split && mixed {
                            app.dispatch(
                                "node.setProps",
                                json!({ "ids": ids, "cornerRadius": radii[0] }),
                            );
                        }
                        ui.data_mut(|d| d.insert_temp(key, !split));
                    }
                }
            });
            if let Some(radii) = corners_row {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 12.0) / 4.0;
                    for i in 0..4 {
                        let mut v = radii[i];
                        let resp = widgets::number(ui, Lead::None, &mut v, 0.5, "", w);
                        let mut next = radii;
                        next[i] = v.max(0.0);
                        edit(
                            app,
                            &resp,
                            "Corner radius",
                            "node.setProps",
                            json!({ "ids": ids, "cornerRadii": next }),
                        );
                    }
                });
            }
        },
    );
    if let Some(name) = new_blend {
        app.dispatch("node.setProps", json!({ "ids": ids, "blend": name }));
    }
    if toggle {
        app.dispatch("object.toggleVisible", json!({ "ids": ids }));
    }
}

fn paint_label(p: &Paint) -> String {
    match &p.kind {
        PaintKind::Solid { color } => color.to_hex().trim_start_matches('#').to_uppercase(),
        PaintKind::LinearGradient { .. } => "Linear".into(),
        PaintKind::RadialGradient { .. } => "Radial".into(),
        PaintKind::AngularGradient { .. } => "Angular".into(),
        PaintKind::DiamondGradient { .. } => "Diamond".into(),
        PaintKind::Image { .. } => "Image".into(),
    }
}

fn paint_editor(ui: &mut egui::Ui, p: &mut Paint) -> bool {
    let mut changed = false;
    let current = match p.kind {
        PaintKind::Solid { .. } => "Solid",
        PaintKind::LinearGradient { .. } => "Linear",
        PaintKind::RadialGradient { .. } => "Radial",
        PaintKind::AngularGradient { .. } => "Angular",
        PaintKind::DiamondGradient { .. } => "Diamond",
        PaintKind::Image { .. } => "Image",
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for t in ["Solid", "Linear", "Radial", "Angular", "Diamond"] {
            if widgets::tab(ui, t, t == current).clicked() && t != current {
                let base = p.primary_color().unwrap_or(Rgba::BLACK);
                let stops = vec![
                    deesainer_doc::ColorStop {
                        position: 0.0,
                        color: base,
                    },
                    deesainer_doc::ColorStop {
                        position: 1.0,
                        color: base.with_alpha(0.0),
                    },
                ];
                p.kind = match t {
                    "Linear" => PaintKind::LinearGradient {
                        start: Point::new(0.5, 0.0),
                        end: Point::new(0.5, 1.0),
                        stops,
                    },
                    "Radial" => PaintKind::RadialGradient {
                        center: Point::new(0.5, 0.5),
                        radius: Point::new(0.5, 0.5),
                        stops,
                    },
                    "Angular" => PaintKind::AngularGradient {
                        center: Point::new(0.5, 0.5),
                        angle: 0.0,
                        stops,
                    },
                    "Diamond" => PaintKind::DiamondGradient {
                        center: Point::new(0.5, 0.5),
                        radius: Point::new(0.5, 0.5),
                        stops,
                    },
                    _ => PaintKind::Solid { color: base },
                };
                changed = true;
            }
        }
    });
    ui.add_space(8.0);
    match &mut p.kind {
        PaintKind::Solid { color } => {
            let mut c = color32(*color);
            if widgets::color_picker(ui, &mut c) {
                *color = rgba(c);
                changed = true;
            }
        }
        PaintKind::LinearGradient { stops, .. }
        | PaintKind::RadialGradient { stops, .. }
        | PaintKind::AngularGradient { stops, .. }
        | PaintKind::DiamondGradient { stops, .. } => {
            ui.label(egui::RichText::new("Stops").font(crate::theme::strong()));
            for (i, st) in stops.iter_mut().enumerate() {
                ui.push_id(("stop", i), |ui| {
                    ui.horizontal(|ui| {
                        let mut pos = f64::from(st.position) * 100.0;
                        if widgets::number(ui, Lead::None, &mut pos, 0.5, " %", 60.0).changed() {
                            st.position = (pos / 100.0).clamp(0.0, 1.0) as f32;
                            changed = true;
                        }
                        let mut c = color32(st.color);
                        ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            st.color = rgba(c);
                            changed = true;
                        }
                        ui.label(widgets::hex(c));
                    });
                });
            }
        }
        PaintKind::Image { adjust, .. } => {
            let mut rows = [
                ("Exposure", adjust.exposure),
                ("Contrast", adjust.contrast),
                ("Saturation", adjust.saturation),
                ("Temperature", adjust.temperature),
                ("Tint", adjust.tint),
                ("Highlights", adjust.highlights),
                ("Shadows", adjust.shadows),
            ];
            for (label, v) in &mut rows {
                widgets::labelled(ui, label, |ui| {
                    let mut pct = *v * 100.0;
                    if ui
                        .add(
                            egui::Slider::new(&mut pct, -100.0..=100.0)
                                .show_value(true)
                                .integer(),
                        )
                        .changed()
                    {
                        *v = (pct / 100.0).clamp(-1.0, 1.0);
                        changed = true;
                    }
                });
            }
            if changed {
                let [e, c, s, t, ti, h, sh] = rows.map(|(_, v)| v);
                *adjust = deesainer_doc::ImageAdjust {
                    exposure: e,
                    contrast: c,
                    saturation: s,
                    temperature: t,
                    tint: ti,
                    highlights: h,
                    shadows: sh,
                };
            }
            if !adjust.is_identity() && ui.small_button("Reset adjustments").clicked() {
                *adjust = deesainer_doc::ImageAdjust::default();
                changed = true;
            }
        }
    }
    if let PaintKind::AngularGradient { angle, .. } = &mut p.kind {
        ui.add_space(4.0);
        widgets::labelled(ui, "Start angle", |ui| {
            let mut a = *angle;
            if widgets::number(ui, Lead::None, &mut a, 1.0, "°", 72.0).changed() && a.is_finite() {
                *angle = a.rem_euclid(360.0);
                changed = true;
            }
        });
    }
    changed
}

fn paints_ui(ui: &mut egui::Ui, salt: &str, paints: &mut Vec<Paint>) -> bool {
    paints_ui_with(ui, salt, paints, &mut None)
}

fn paints_ui_with(
    ui: &mut egui::Ui,
    salt: &str,
    paints: &mut Vec<Paint>,
    open_gradient: &mut Option<usize>,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    let total = ui.available_width();
    let main_w = (total - 56.0 - 60.0 - 8.0).max(60.0);
    for (i, p) in paints.iter_mut().enumerate().rev() {
        ui.push_id((salt, i), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mut pct = f64::from(p.opacity) * 100.0;
                let shown = color32(p.primary_color().unwrap_or(Rgba::BLACK));
                let label = paint_label(p);
                let (resp, pct_changed) =
                    widgets::paint_value(ui, shown, &label, &mut pct, main_w + 57.0);
                if let Some(r) = widgets::popover(&resp, "").show(|ui| {
                    ui.set_width(300.0);
                    paint_editor(ui, p)
                }) {
                    changed |= r.inner;
                    if p.stops().is_some() {
                        *open_gradient = Some(i);
                    }
                }
                if pct_changed {
                    p.opacity = (pct / 100.0).clamp(0.0, 1.0) as f32;
                    changed = true;
                }
                let eye = if p.visible { Icon::Eye } else { Icon::EyeOff };
                if icons::button(ui, eye, false, "Show/Hide").clicked() {
                    p.visible = !p.visible;
                    changed = true;
                }
                if icons::button(ui, Icon::Minus, false, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        paints.remove(i);
        changed = true;
    }
    changed
}

fn fill_section(app: &mut App, ui: &mut egui::Ui, ids: &[NodeId], node: &Node) {
    let mut fills = node.fills.clone();
    let mut add = false;
    let single = if let [id] = ids { Some(*id) } else { None };
    widgets::divider(ui);
    widgets::header(ui, "Fill", |ui| {
        add = icons::button(ui, Icon::Plus, false, "Add fill").clicked();
        if icons::button(ui, Icon::Eyedropper, app.eyedropper, "Pick colour (I)").clicked() {
            app.eyedropper = !app.eyedropper;
        }
        if let Some(id) = single {
            let styles = icons::button(ui, Icon::Styles, false, "Styles");
            crate::widgets::menu(&styles).show(|ui| {
                ui.set_min_width(200.0);
                crate::design_ext::fill_style_row(app, ui, id, node);
            });
        }
    });
    let doc = app.session.snapshot();
    let mixed = ids
        .iter()
        .filter_map(|i| doc.get(*i))
        .any(|n| n.fills != node.fills);
    if add {
        if mixed {
            fills = vec![Paint::solid(Rgba::from_rgba8([0xD9, 0xD9, 0xD9, 0xFF]))];
        } else {
            fills.push(Paint::solid(Rgba::from_rgba8([0xD9, 0xD9, 0xD9, 0xFF])));
        }
        app.dispatch("node.setFills", json!({ "ids": ids, "fills": fills }));
        return;
    }
    if mixed {
        ui.label(
            egui::RichText::new("Click + to replace mixed content")
                .color(crate::theme::TOKENS.text_secondary),
        );
        ui.add_space(8.0);
        return;
    }
    if let Some(sid) = node.styles.fill {
        let name = app
            .session
            .doc()
            .style(sid)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            icons::glyph(ui, Icon::Styles, crate::theme::TOKENS.text_secondary);
            ui.label(name);
        });
    }
    let mut open_gradient = None;
    let changed = paints_ui_with(ui, "fill", &mut fills, &mut open_gradient);
    if let ([id], Some(i)) = (ids, open_gradient) {
        app.gradient_edit = Some((*id, i));
        app.gradient_sticky = Some((*id, i));
    }
    if changed {
        app.dispatch("node.setFills", json!({ "ids": ids, "fills": fills }));
    }
    ui.add_space(8.0);
}

fn stroke_section(app: &mut App, ui: &mut egui::Ui, ids: &[NodeId], node: &Node) {
    let mut strokes = node.strokes.clone();
    let mut add = false;
    widgets::divider(ui);
    let plus = |ui: &mut egui::Ui| icons::button(ui, Icon::Plus, false, "Add stroke").clicked();
    if node.strokes.is_empty() {
        widgets::header_empty(ui, "Stroke", |ui| add = plus(ui));
    } else {
        widgets::header(ui, "Stroke", |ui| add = plus(ui));
    }
    let nodes = nodes_of(app, ids);
    let mixed = differ(&nodes, |n| n.strokes.clone());
    if add {
        if mixed {
            strokes = vec![Paint::solid(Rgba::BLACK)];
        } else {
            strokes.push(Paint::solid(Rgba::BLACK));
        }
        app.dispatch("node.setStrokes", json!({ "ids": ids, "strokes": strokes }));
        return;
    }
    if mixed {
        ui.label(
            egui::RichText::new("Click + to replace mixed content")
                .color(crate::theme::TOKENS.text_secondary),
        );
        ui.add_space(8.0);
        return;
    }
    if node.strokes.is_empty() {
        return;
    }
    let mixed_align = differ(&nodes, |n| n.stroke.align);
    let mixed_weight = differ(&nodes, |n| n.stroke.weight);
    if paints_ui(ui, "stroke", &mut strokes) {
        app.dispatch("node.setStrokes", json!({ "ids": ids, "strokes": strokes }));
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let w = widgets::half(ui);
        let current = match node.stroke.align {
            _ if mixed_align => "Mixed",
            StrokeAlign::Inside => "Inside",
            StrokeAlign::Center => "Center",
            StrokeAlign::Outside => "Outside",
        };
        let resp = widgets::select(ui, w, current);
        crate::widgets::menu(&resp).show(|ui| {
            for (label, key) in [
                ("Inside", "inside"),
                ("Center", "center"),
                ("Outside", "outside"),
            ] {
                if widgets::menu_item(ui, Some(label == current), None, label, "", true).clicked() {
                    app.dispatch("node.setProps", json!({ "ids": ids, "strokeAlign": key }));
                    ui.close();
                }
            }
        });
        if mixed_weight {
            let (resp, v) = widgets::number_mixed(ui, "weight", None, 1.0, w);
            edit(
                app,
                &resp,
                "Stroke weight",
                "node.setProps",
                json!({ "ids": ids, "strokeWeight": v.unwrap_or(1.0).max(0.0) }),
            );
        } else {
            let mut weight = node.stroke.weight;
            let resp = widgets::number(ui, Lead::Icon(Icon::Minus), &mut weight, 0.25, "", w);
            edit(
                app,
                &resp,
                "Stroke weight",
                "node.setProps",
                json!({ "ids": ids, "strokeWeight": weight.max(0.0) }),
            );
        }
        let boxy = matches!(
            node.kind,
            NodeKind::Rectangle
                | NodeKind::Frame
                | NodeKind::Component { .. }
                | NodeKind::Instance { .. }
        );
        if boxy {
            let on = node.stroke.sides.is_some();
            if icons::button(ui, Icon::Corners, on, "Individual strokes").clicked() {
                let sides = if on {
                    serde_json::Value::Null
                } else {
                    json!(vec![node.stroke.weight; 4])
                };
                app.dispatch("node.setProps", json!({ "ids": ids, "strokeSides": sides }));
            }
        }
    });
    if let Some(sides) = node.stroke.sides {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let w = ((ui.available_width() - 12.0) / 4.0).floor();
            for (i, label) in ["T", "R", "B", "L"].iter().enumerate() {
                let mut v = sides[i];
                let resp = widgets::number(ui, Lead::Text(label), &mut v, 0.25, "", w);
                let mut next = sides;
                next[i] = v.max(0.0);
                edit(
                    app,
                    &resp,
                    "Stroke weight",
                    "node.setProps",
                    json!({ "ids": ids, "strokeSides": next }),
                );
            }
        });
    }
    ui.add_space(8.0);
}

fn effect_name(e: &Effect) -> &'static str {
    match e {
        Effect::DropShadow { .. } => "Drop shadow",
        Effect::InnerShadow { .. } => "Inner shadow",
        Effect::LayerBlur { .. } => "Layer blur",
        Effect::BackgroundBlur { .. } => "Background blur",
    }
}

fn effects_section(app: &mut App, ui: &mut egui::Ui, ids: &[NodeId], node: &Node) {
    let mut effects = node.effects.clone();
    let mut changed = false;
    widgets::divider(ui);
    let mut add: Option<Effect> = None;
    let actions = |ui: &mut egui::Ui, add: &mut Option<Effect>| {
        let plus = icons::button(ui, Icon::Plus, false, "Add effect");
        crate::widgets::menu(&plus).show(|ui| {
            let shadow = |inner: bool| {
                let (color, offset, blur, spread, visible) = (
                    Rgba::new(0.0, 0.0, 0.0, 0.25),
                    Point::new(0.0, 4.0),
                    4.0,
                    0.0,
                    true,
                );
                if inner {
                    Effect::InnerShadow {
                        color,
                        offset,
                        blur,
                        spread,
                        visible,
                    }
                } else {
                    Effect::DropShadow {
                        color,
                        offset,
                        blur,
                        spread,
                        visible,
                    }
                }
            };
            for (label, e) in [
                ("Inner shadow", shadow(true)),
                ("Drop shadow", shadow(false)),
                (
                    "Layer blur",
                    Effect::LayerBlur {
                        radius: 4.0,
                        visible: true,
                    },
                ),
                (
                    "Background blur",
                    Effect::BackgroundBlur {
                        radius: 4.0,
                        visible: true,
                    },
                ),
            ] {
                if widgets::menu_item(ui, None, None, label, "", true).clicked() {
                    *add = Some(e);
                    ui.close();
                }
            }
        });
    };
    if effects.is_empty() {
        widgets::header_empty(ui, "Effects", |ui| actions(ui, &mut add));
    } else {
        widgets::header(ui, "Effects", |ui| actions(ui, &mut add));
    }
    let mixed = differ(&nodes_of(app, ids), |n| n.effects.clone());
    if let Some(e) = add {
        if mixed {
            effects.clear();
        }
        effects.push(e);
        app.dispatch("node.setEffects", json!({ "ids": ids, "effects": effects }));
        return;
    }
    if mixed {
        ui.label(
            egui::RichText::new("Click + to replace mixed content")
                .color(crate::theme::TOKENS.text_secondary),
        );
        ui.add_space(8.0);
        return;
    }
    let mut remove = None;
    for (i, e) in effects.iter_mut().enumerate() {
        ui.push_id(("effect", i), |ui| {
            let name = effect_name(e);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let w = (ui.available_width() - 56.0).max(60.0);
                let pill = widgets::select(ui, w, name);
                widgets::popover(&pill, name).show(|ui| {
                    ui.set_width(220.0);
                    if widgets::popover_title(ui, name) {
                        ui.close();
                    }
                    changed |= effect_settings(ui, e);
                });
                let visible = match e {
                    Effect::DropShadow { visible, .. }
                    | Effect::InnerShadow { visible, .. }
                    | Effect::LayerBlur { visible, .. }
                    | Effect::BackgroundBlur { visible, .. } => visible,
                };
                let icon = if *visible { Icon::Eye } else { Icon::EyeOff };
                if icons::button(ui, icon, false, "Show/Hide").clicked() {
                    *visible = !*visible;
                    changed = true;
                }
                if icons::button(ui, Icon::Minus, false, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if !effects.is_empty() {
        ui.add_space(8.0);
    }
    if let Some(i) = remove {
        effects.remove(i);
        changed = true;
    }
    if changed {
        app.dispatch("node.setEffects", json!({ "ids": ids, "effects": effects }));
    }
}

fn effect_settings(ui: &mut egui::Ui, e: &mut Effect) -> bool {
    let mut changed = false;
    let w = 140.0;
    match e {
        Effect::DropShadow {
            color,
            offset,
            blur,
            spread,
            ..
        }
        | Effect::InnerShadow {
            color,
            offset,
            blur,
            spread,
            ..
        } => {
            changed |= widgets::labelled(ui, "Position", |ui| {
                widgets::number(ui, Lead::Text("X"), &mut offset.x, 0.5, "", w).changed()
            });
            changed |= widgets::labelled(ui, "", |ui| {
                widgets::number(ui, Lead::Text("Y"), &mut offset.y, 0.5, "", w).changed()
            });
            let mut b = *blur;
            if widgets::labelled(ui, "Blur", |ui| {
                widgets::number(ui, Lead::None, &mut b, 0.5, "", w).changed()
            }) {
                *blur = b.clamp(0.0, 500.0);
                changed = true;
            }
            changed |= widgets::labelled(ui, "Spread", |ui| {
                widgets::number(ui, Lead::None, spread, 0.5, "", w).changed()
            });
            let mut c = color32(*color);
            let mut pct = (f64::from(c.a()) / 2.55).round();
            let (cc, oc) = widgets::labelled(ui, "Color", |ui| {
                ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                let cc = ui.color_edit_button_srgba(&mut c).changed();
                ui.label(widgets::hex(c));
                let oc = widgets::number(ui, Lead::None, &mut pct, 0.5, " %", 56.0).changed();
                (cc, oc)
            });
            if cc {
                *color = rgba(c);
                changed = true;
            }
            if oc {
                *color = color.with_alpha((pct / 100.0).clamp(0.0, 1.0) as f32);
                changed = true;
            }
        }
        Effect::LayerBlur { radius, .. } | Effect::BackgroundBlur { radius, .. } => {
            let mut r = *radius;
            if widgets::labelled(ui, "Blur", |ui| {
                widgets::number(ui, Lead::None, &mut r, 0.5, "", w).changed()
            }) {
                *radius = r.clamp(0.0, 500.0);
                changed = true;
            }
        }
    }
    changed
}

fn layout_guides_section(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    use deesainer_doc::{GridAlign, GridPattern, LayoutGrid};
    let mut grids = node.layout_grids.clone();
    let mut changed = false;
    widgets::divider(ui);
    let mut add = false;
    let plus =
        |ui: &mut egui::Ui| icons::button(ui, Icon::Plus, false, "Add layout guide").clicked();
    if grids.is_empty() {
        widgets::header_empty(ui, "Layout guide", |ui| add = plus(ui));
    } else {
        widgets::header(ui, "Layout guide", |ui| add = plus(ui));
    }
    if add {
        grids.push(LayoutGrid::default());
        changed = true;
    }
    let mut remove = None;
    for (i, g) in grids.iter_mut().enumerate() {
        ui.push_id(("grid", i), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let w = (ui.available_width() - 56.0).max(60.0);
                let label = match g.pattern {
                    GridPattern::Grid => format!("Grid {}px", widgets::trim(g.size)),
                    GridPattern::Columns => format!("Columns ({})", g.count),
                    GridPattern::Rows => format!("Rows ({})", g.count),
                };
                let icon = match g.pattern {
                    GridPattern::Grid => Icon::Frame,
                    GridPattern::Columns => Icon::LayoutHorizontal,
                    GridPattern::Rows => Icon::LayoutVertical,
                };
                let resp = widgets::field(ui, w, |ui| {
                    icons::glyph(ui, icon, crate::theme::TOKENS.text_secondary);
                    ui.add(egui::Label::new(label).sense(egui::Sense::click()))
                });
                widgets::popover(&resp, "Layout guide").show(|ui| {
                    ui.set_width(240.0);
                    if widgets::popover_title(ui, "Layout guide") {
                        ui.close();
                    }
                    let fw = 150.0;
                    let pats = [("grid", "Grid"), ("columns", "Columns"), ("rows", "Rows")];
                    let cur = match g.pattern {
                        GridPattern::Grid => "grid",
                        GridPattern::Columns => "columns",
                        GridPattern::Rows => "rows",
                    };
                    widgets::labelled(ui, "Type", |ui| {
                        if let Some(v) = widgets::choice(ui, fw, cur, &pats) {
                            g.pattern = match v {
                                "columns" => GridPattern::Columns,
                                "rows" => GridPattern::Rows,
                                _ => GridPattern::Grid,
                            };
                            changed = true;
                        }
                    });
                    if g.pattern == GridPattern::Grid {
                        changed |= widgets::labelled(ui, "Size", |ui| {
                            widgets::number(ui, Lead::None, &mut g.size, 0.5, "", fw).changed()
                        });
                    } else {
                        let mut count = f64::from(g.count);
                        if widgets::labelled(ui, "Count", |ui| {
                            widgets::number(ui, Lead::None, &mut count, 0.1, "", fw).changed()
                        }) {
                            g.count = count.round().clamp(1.0, 1000.0) as u32;
                            changed = true;
                        }
                        let aligns = [
                            ("stretch", "Stretch"),
                            ("min", "Left / Top"),
                            ("center", "Center"),
                            ("max", "Right / Bottom"),
                        ];
                        let cur = match g.align {
                            GridAlign::Stretch => "stretch",
                            GridAlign::Min => "min",
                            GridAlign::Center => "center",
                            GridAlign::Max => "max",
                        };
                        widgets::labelled(ui, "Type", |ui| {
                            if let Some(v) = widgets::choice(ui, fw, cur, &aligns) {
                                g.align = match v {
                                    "min" => GridAlign::Min,
                                    "center" => GridAlign::Center,
                                    "max" => GridAlign::Max,
                                    _ => GridAlign::Stretch,
                                };
                                changed = true;
                            }
                        });
                        if g.align != GridAlign::Stretch {
                            changed |= widgets::labelled(ui, "Width", |ui| {
                                widgets::number(ui, Lead::None, &mut g.size, 0.5, "", fw).changed()
                            });
                        }
                        let margin = if g.align == GridAlign::Stretch {
                            "Margin"
                        } else {
                            "Offset"
                        };
                        changed |= widgets::labelled(ui, margin, |ui| {
                            widgets::number(ui, Lead::None, &mut g.offset, 0.5, "", fw).changed()
                        });
                        changed |= widgets::labelled(ui, "Gutter", |ui| {
                            widgets::number(ui, Lead::None, &mut g.gutter, 0.5, "", fw).changed()
                        });
                    }
                    let mut c = color32(g.color);
                    widgets::labelled(ui, "Color", |ui| {
                        ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            g.color = rgba(c);
                            changed = true;
                        }
                        ui.label(widgets::hex(c));
                    });
                });
                let eye = if g.visible { Icon::Eye } else { Icon::EyeOff };
                if icons::button(ui, eye, false, "Show/Hide").clicked() {
                    g.visible = !g.visible;
                    changed = true;
                }
                if icons::button(ui, Icon::Minus, false, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        grids.remove(i);
        changed = true;
    }
    if !grids.is_empty() {
        ui.add_space(8.0);
    }
    if changed {
        if node.styles.grid.is_some() {
            app.dispatch("style.detach", json!({ "ids": [id], "target": "grid" }));
        }
        app.dispatch(
            "node.setProps",
            json!({ "ids": [id], "layoutGrids": grids }),
        );
    }
    crate::design_ext::grid_style_row(app, ui, id, node);
}

const SCALES: [(&str, &str); 6] = [
    ("0.5", "0.5x"),
    ("0.75", "0.75x"),
    ("1", "1x"),
    ("1.5", "1.5x"),
    ("2", "2x"),
    ("3", "3x"),
];
const FORMATS: [(&str, &str); 5] = [
    ("png", "PNG"),
    ("jpg", "JPG"),
    ("webp", "WEBP"),
    ("svg", "SVG"),
    ("pdf", "PDF"),
];

pub(crate) fn export_section(app: &mut App, ui: &mut egui::Ui, id: NodeId) {
    use deesainer_doc::{ExportFormat, ExportSetting};
    let (rows, name) = app
        .session
        .doc()
        .get(id)
        .map(|n| (n.export_settings.clone(), n.name.clone()))
        .unwrap_or_default();
    let mut add = false;
    widgets::divider(ui);
    let plus =
        |ui: &mut egui::Ui| icons::button(ui, Icon::Plus, false, "Add export settings").clicked();
    if rows.is_empty() {
        widgets::header_empty(ui, "Export", |ui| add = plus(ui));
    } else {
        widgets::header(ui, "Export", |ui| add = plus(ui));
    }
    let mut next = rows.clone();
    if add {
        let (scale, suffix) = if next.is_empty() {
            (1.0, String::new())
        } else {
            (2.0, "@2x".to_owned())
        };
        next.push(ExportSetting {
            format: ExportFormat::Png,
            scale,
            suffix,
        });
    }
    let mut remove = None;
    let format_of = |v: &str| match v {
        "jpg" => ExportFormat::Jpg,
        "webp" => ExportFormat::Webp,
        "svg" => ExportFormat::Svg,
        "pdf" => ExportFormat::Pdf,
        _ => ExportFormat::Png,
    };
    for (i, row) in next.iter_mut().enumerate() {
        ui.push_id(("export", i), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let w = ((ui.available_width() - 32.0 - 12.0) / 3.0).floor();
                let scale = fmt_scale(row.scale);
                let raster = !matches!(row.format, ExportFormat::Svg | ExportFormat::Pdf);
                ui.add_enabled_ui(raster, |ui| {
                    if let Some(v) = widgets::choice(ui, w, &scale, &SCALES) {
                        row.scale = v.parse().unwrap_or(1.0);
                    }
                });
                let key = ui.id().with("suffix");
                let mut suffix = ui
                    .data(|d| d.get_temp::<String>(key))
                    .unwrap_or_else(|| row.suffix.clone());
                let edit = ui.add_sized(
                    egui::vec2(w, 24.0),
                    egui::TextEdit::singleline(&mut suffix)
                        .hint_text("Suffix")
                        .vertical_align(egui::Align::Center),
                );
                if edit.changed() {
                    ui.data_mut(|d| d.insert_temp(key, suffix.clone()));
                }
                if edit.lost_focus() {
                    row.suffix = suffix
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(64)
                        .collect();
                    ui.data_mut(|d| d.remove::<String>(key));
                }
                if let Some(v) = widgets::choice(ui, w, row.format.extension(), &FORMATS) {
                    row.format = format_of(v);
                }
                if icons::button(ui, Icon::Minus, false, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        next.remove(i);
    }
    if next != rows {
        app.dispatch(
            "node.setExportSettings",
            json!({ "ids": [id], "settings": next }),
        );
    }
    if !next.is_empty() {
        let label = if next.len() == 1 {
            format!("Export {name}")
        } else {
            format!("Export {name} ({} files)", next.len())
        };
        if ui
            .add_sized(
                egui::vec2(ui.available_width(), 24.0),
                egui::Button::new(label)
                    .stroke(egui::Stroke::new(1.0, crate::theme::TOKENS.border)),
            )
            .clicked()
        {
            crate::menu::run_file_action(app, &format!("exportSettings:{}", id.0));
        }
        ui.add_space(8.0);
    }
}

fn fmt_scale(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}

fn font_list(
    app: &mut App,
    ui: &mut egui::Ui,
    families: &[String],
    current: Option<&str>,
) -> Option<String> {
    let key = egui::Id::new("font-search");
    let mut q: String = ui.data_mut(|d| d.get_temp(key).unwrap_or_default());
    widgets::field(ui, 240.0, |ui| {
        icons::glyph(ui, Icon::Search, crate::theme::TOKENS.text_secondary);
        let r = ui.add(
            egui::TextEdit::singleline(&mut q)
                .frame(egui::Frame::NONE)
                .hint_text("Search fonts")
                .desired_width(f32::INFINITY),
        );
        r.request_focus();
    });
    ui.data_mut(|d| d.insert_temp(key, q.clone()));
    ui.add_space(6.0);
    let ql = q.to_lowercase();
    let shown: Vec<&String> = families
        .iter()
        .filter(|f| ql.is_empty() || f.to_lowercase().contains(&ql))
        .collect();
    let mut picked = None;
    const ROW_H: f32 = 28.0;
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show_rows(ui, ROW_H, shown.len(), |ui, rows| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for f in &shown[rows] {
                let (rect, r) =
                    ui.allocate_exact_size(egui::vec2(240.0, ROW_H), egui::Sense::click());
                let painter = ui.painter_at(rect);
                if r.hovered() {
                    painter.rect_filled(rect, 5.0, crate::theme::TOKENS.row_hover);
                }
                if current == Some(f.as_str()) {
                    painter.text(
                        rect.left_center() + egui::vec2(10.0, 0.0),
                        egui::Align2::CENTER_CENTER,
                        "✓",
                        crate::theme::body(),
                        crate::theme::TOKENS.text,
                    );
                }
                let at = rect.left_center() + egui::vec2(24.0, 0.0);
                match font_preview(app, ui.ctx(), f) {
                    Some(tex) => {
                        let size = tex.size_vec2() / 2.0;
                        let img = egui::Rect::from_min_size(
                            egui::pos2(at.x, rect.center().y - size.y / 2.0),
                            size,
                        );
                        painter.image(
                            tex.id(),
                            img,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    None => {
                        painter.text(
                            at,
                            egui::Align2::LEFT_CENTER,
                            f.as_str(),
                            egui::FontId::proportional(13.0),
                            crate::theme::TOKENS.text,
                        );
                    }
                }
                if r.on_hover_text(f.as_str()).clicked() {
                    picked = Some((*f).clone());
                }
            }
        });
    picked
}

fn font_preview(app: &mut App, ctx: &egui::Context, family: &str) -> Option<egui::TextureHandle> {
    if let Some(t) = app.font_previews.get(family) {
        return t.clone();
    }
    let tex = render_font_preview(family)
        .map(|img| ctx.load_texture(format!("font:{family}"), img, Default::default()));
    if app.font_previews.len() > 4000 {
        app.font_previews.clear();
    }
    app.font_previews.insert(family.to_owned(), tex.clone());
    tex
}

fn render_font_preview(family: &str) -> Option<egui::ColorImage> {
    use deesainer_doc::{Document, TextAutoResize, TextData};
    let data = TextData {
        content: family.to_owned(),
        style: TextStyle {
            font_family: family.to_owned(),
            font_size: 14.0,
            ..TextStyle::default()
        },
        auto_resize: TextAutoResize::WidthAndHeight,
        ..TextData::default()
    };
    let (w, h) = deesainer_text::measure(&data, None);
    let (w, h) = (w.clamp(1.0, 200.0), h.clamp(1.0, 40.0));
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut node = Node::new(
        doc.next_free_id(),
        "preview",
        NodeKind::Text(data),
        deesainer_geom::Rect::new(0.0, 0.0, w, h),
    );
    let c = crate::theme::TOKENS.text;
    node.fills = vec![Paint::solid(Rgba::from_rgba8([c.r(), c.g(), c.b(), 255]))];
    doc.insert(node, page, 0).ok()?;
    let scale = 2.0;
    let vp = deesainer_render::Viewport {
        transform: deesainer_geom::Affine::scale(scale, scale),
        width: (w * scale).ceil() as u32,
        height: (h * scale).ceil() as u32,
    };
    let opts = deesainer_render::Options {
        background: false,
        text: Some(deesainer_engine::text_painter()),
        hidden: &[],
    };
    let (pix, _) = deesainer_render::render_page(&doc, page, &vp, opts).ok()?;
    Some(egui::ColorImage::from_rgba_premultiplied(
        [pix.width() as usize, pix.height() as usize],
        pix.data(),
    ))
}

const WEIGHTS: [(u16, &str); 9] = [
    (100, "Thin"),
    (200, "Extra Light"),
    (300, "Light"),
    (400, "Regular"),
    (500, "Medium"),
    (600, "Semi Bold"),
    (700, "Bold"),
    (800, "Extra Bold"),
    (900, "Black"),
];

fn selected_text_style(app: &App, id: NodeId) -> Option<TextStyle> {
    app.editing
        .as_ref()
        .filter(|editing| editing.id == id)
        .and_then(|editing| editing.editor.selected_style())
}

fn set_layer_text_style(app: &mut App, id: NodeId, params: Value) {
    let editing = app.editing.as_ref().is_some_and(|editing| editing.id == id);
    if editing {
        app.session.begin_group("Text style");
        app.commit_editing();
    }
    app.dispatch("text.setStyle", params);
    if editing {
        app.session.end_group();
    }
}

fn set_paragraphs(app: &mut App, id: NodeId, span: Option<(usize, usize)>, mut params: Value) {
    params["id"] = json!(id);
    if let Some((first, last)) = span {
        params["first"] = json!(first);
        params["last"] = json!(last);
    }
    let editing = app.editing.as_ref().is_some_and(|editing| editing.id == id);
    if editing {
        app.session.begin_group("Paragraph list");
        app.commit_editing();
    }
    app.dispatch("text.setParagraphs", params);
    if editing {
        app.session.end_group();
    }
}

fn set_typography(app: &mut App, id: NodeId, params: Value, change: impl FnMut(&mut TextStyle)) {
    if selected_text_style(app, id).is_some() {
        if let Some(editing) = &mut app.editing {
            if let Err(message) = editing.editor.modify_selected_style(change) {
                app.status = Some((message, true));
            }
        }
    } else {
        set_layer_text_style(app, id, params);
    }
}

fn edit_typography(
    app: &mut App,
    resp: &egui::Response,
    label: &str,
    id: NodeId,
    params: Value,
    change: impl FnMut(&mut TextStyle),
) {
    if selected_text_style(app, id).is_some() {
        if resp.changed() {
            set_typography(app, id, params, change);
        }
    } else {
        if resp.changed() && app.editing.as_ref().is_some_and(|editing| editing.id == id) {
            if !app.drag_group_open {
                app.session.begin_group(label);
                app.drag_group_open = true;
            }
            app.commit_editing();
        }
        edit(app, resp, label, "text.setStyle", params);
    }
}

fn text_section(app: &mut App, ui: &mut egui::Ui, id: NodeId, node: &Node) {
    let Some(t) = node.text().cloned() else {
        return;
    };
    let style = selected_text_style(app, id).unwrap_or_else(|| t.style.clone());
    let families = app.font_families();
    let mut settings = None;
    widgets::section(
        ui,
        app.tr("Typography"),
        |ui| settings = Some(icons::button(ui, Icon::Sliders, false, "Type settings")),
        |ui| {
            let full = widgets::half(ui) * 2.0 + 8.0;
            let half = widgets::half(ui);
            let fam = widgets::select(ui, full, &style.font_family);
            widgets::popover(&fam, "Fonts").show(|ui| {
                ui.set_width(240.0);
                if widgets::popover_title(ui, "Fonts") {
                    ui.close();
                }
                if let Some(f) = font_list(app, ui, &families, Some(&style.font_family)) {
                    set_typography(app, id, json!({ "ids": [id], "fontFamily": f }), |style| {
                        style.font_family = f.clone()
                    });
                    ui.close();
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let wname = WEIGHTS
                    .iter()
                    .find(|(w, _)| *w == style.font_weight)
                    .map_or_else(|| style.font_weight.to_string(), |(_, n)| (*n).to_owned());
                let wr = widgets::select(ui, half, &wname);
                crate::widgets::menu(&wr).show(|ui| {
                    for (w, n) in WEIGHTS {
                        if widgets::menu_item(ui, Some(w == style.font_weight), None, n, "", true)
                            .clicked()
                        {
                            set_typography(
                                app,
                                id,
                                json!({ "ids": [id], "fontWeight": w }),
                                |style| style.font_weight = w,
                            );
                            ui.close();
                        }
                    }
                });
                let mut size = style.font_size;
                let resp = widgets::number(ui, Lead::None, &mut size, 0.5, "", half);
                let size = size.max(0.5);
                edit_typography(
                    app,
                    &resp,
                    "Font size",
                    id,
                    json!({ "ids": [id], "fontSize": size.max(0.5) }),
                    |style| style.font_size = size,
                );
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (mut lh, lh_suffix) = match style.line_height {
                    deesainer_doc::LineHeight::Auto => (0.0, ""),
                    deesainer_doc::LineHeight::Pixels(p) => (p, ""),
                    deesainer_doc::LineHeight::Percent(p) => (p, "%"),
                };
                if matches!(style.line_height, LineHeight::Auto) {
                    widgets::field(ui, half, |ui| {
                        icons::glyph(ui, Icon::AutoHeight, crate::theme::TOKENS.text_secondary);
                        ui.label("Auto");
                    });
                } else {
                    let resp = widgets::number(
                        ui,
                        Lead::Icon(Icon::AutoHeight),
                        &mut lh,
                        0.5,
                        lh_suffix,
                        half,
                    );
                    let value = if lh_suffix.is_empty() {
                        json!(lh.max(0.0))
                    } else {
                        json!(format!("{}%", lh.max(0.0)))
                    };
                    let line_height = if lh_suffix.is_empty() {
                        LineHeight::Pixels(lh.max(0.1))
                    } else {
                        LineHeight::Percent(lh.max(0.1))
                    };
                    edit_typography(
                        app,
                        &resp,
                        "Line height",
                        id,
                        json!({ "ids": [id], "lineHeight": value }),
                        |style| style.line_height = line_height,
                    );
                }
                let mut ls = style.letter_spacing;
                let resp =
                    widgets::number(ui, Lead::Icon(Icon::AutoWidth), &mut ls, 0.1, "%", half);
                edit_typography(
                    app,
                    &resp,
                    "Letter spacing",
                    id,
                    json!({ "ids": [id], "letterSpacing": ls }),
                    |style| style.letter_spacing = ls,
                );
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let h_keys = ["left", "center", "right", "justified"];
                let cur_h = enum_str(t.align_h);
                let items = [
                    (Icon::TextLeft, "Align left"),
                    (Icon::TextCenter, "Align center"),
                    (Icon::TextRight, "Align right"),
                    (Icon::TextJustify, "Justify"),
                ];
                let cell = ((half - 4.0) / 4.0).floor();
                if let Some(i) =
                    widgets::segmented(ui, &items, h_keys.iter().position(|k| *k == cur_h), cell)
                {
                    set_layer_text_style(app, id, json!({ "ids": [id], "align": h_keys[i] }));
                }
                let v_keys = ["top", "center", "bottom"];
                let cur_v = enum_str(t.align_v);
                let items = [
                    (Icon::AlignTop, "Align top"),
                    (Icon::AlignVCenter, "Align middle"),
                    (Icon::AlignBottom, "Align bottom"),
                ];
                let cell = ((half - 4.0) / 3.0).floor();
                if let Some(i) =
                    widgets::segmented(ui, &items, v_keys.iter().position(|k| *k == cur_v), cell)
                {
                    set_layer_text_style(
                        app,
                        id,
                        json!({ "ids": [id], "alignVertical": v_keys[i] }),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let mut ps = t.paragraph_spacing;
                let resp = widgets::number(
                    ui,
                    Lead::Icon(Icon::ParagraphSpacing),
                    &mut ps,
                    1.0,
                    "",
                    half,
                );
                edit(
                    app,
                    &resp,
                    "Paragraph spacing",
                    "text.setStyle",
                    json!({ "ids": [id], "paragraphSpacing": ps.clamp(0.0, 10_000.0) }),
                );
                let span = app
                    .editing
                    .as_ref()
                    .filter(|e| e.id == id)
                    .map(|e| e.editor.selected_paragraphs());
                let keys = ["none", "bullet", "number"];
                let cur = match span {
                    Some((first, _)) => Some(enum_str(t.paragraph_format(first).list)),
                    None if t.paragraphs.is_empty() => Some(enum_str(t.list)),
                    None => None,
                };
                let items = [
                    (Icon::ListNone, "No list"),
                    (Icon::ListBullet, "Bulleted list"),
                    (Icon::ListNumber, "Numbered list"),
                ];
                let cell = ((half - 4.0) / 3.0).floor();
                let picked = widgets::segmented(
                    ui,
                    &items,
                    cur.and_then(|c| keys.iter().position(|k| *k == c)),
                    cell,
                );
                if let Some(i) = picked {
                    set_paragraphs(app, id, span, json!({ "list": keys[i] }));
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let span = app
                    .editing
                    .as_ref()
                    .filter(|e| e.id == id)
                    .map(|e| e.editor.selected_paragraphs());
                if icons::button(ui, Icon::Outdent, false, "Decrease indent").clicked() {
                    set_paragraphs(app, id, span, json!({ "indent": -1 }));
                }
                if icons::button(ui, Icon::Indent, false, "Increase indent").clicked() {
                    set_paragraphs(app, id, span, json!({ "indent": 1 }));
                }
            });
        },
    );
    if let Some(resp) = settings {
        widgets::popover(&resp, "Type settings").show(|ui| {
            ui.set_width(240.0);
            if widgets::popover_title(ui, "Type settings") {
                ui.close();
            }
            let deco = enum_str(t.style.decoration);
            let items = [
                (Icon::Minus, "None"),
                (Icon::Underline, "Underline"),
                (Icon::Strike, "Strikethrough"),
            ];
            let keys = ["none", "underline", "strikethrough"];
            widgets::labelled(ui, "Decoration", |ui| {
                if let Some(i) =
                    widgets::segmented(ui, &items, keys.iter().position(|k| *k == deco), 32.0)
                {
                    let next = match keys[i] {
                        "underline" => deesainer_doc::TextDecoration::Underline,
                        "strikethrough" => deesainer_doc::TextDecoration::Strikethrough,
                        _ => deesainer_doc::TextDecoration::None,
                    };
                    set_typography(
                        app,
                        id,
                        json!({ "ids": [id], "decoration": keys[i] }),
                        |style| style.decoration = next,
                    );
                }
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("OpenType features").color(crate::theme::TOKENS.text_secondary));
            let features = t.style.features.clone();
            let value = |tag: &str| features.iter().find(|f| f.tag == tag).map(|f| f.value);
            let rows = [
                ("liga", "Ligatures", true),
                ("calt", "Contextual alternates", true),
                ("dlig", "Discretionary ligatures", false),
                ("smcp", "Small caps", false),
                ("tnum", "Tabular figures", false),
                ("onum", "Oldstyle figures", false),
                ("zero", "Slashed zero", false),
                ("frac", "Fractions", false),
                ("ss01", "Stylistic set 1", false),
                ("ss02", "Stylistic set 2", false),
            ];
            for (tag, label, default_on) in rows {
                let mut on = value(tag).map_or(default_on, |v| v > 0);
                if ui.checkbox(&mut on, label).changed() {
                    let mut next: Vec<serde_json::Value> = features
                        .iter()
                        .filter(|f| f.tag != tag)
                        .map(|f| json!({ "tag": f.tag, "value": f.value }))
                        .collect();
                    if on != default_on {
                        next.push(json!({ "tag": tag, "value": u32::from(on) }));
                    }
                    set_layer_text_style(app, id, json!({ "ids": [id], "features": next }));
                }
            }
            let axes: Vec<deesainer_text::FontAxis> = deesainer_text::font_axes(&t.style)
                .into_iter()
                .filter(|a| a.tag != "wght" && a.max > a.min)
                .collect();
            if !axes.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Variable axes")
                        .color(crate::theme::TOKENS.text_secondary),
                );
                let vars = t.style.variations.clone();
                for axis in axes {
                    let current = vars
                        .iter()
                        .find(|v| v.tag == axis.tag)
                        .map_or(axis.default, |v| v.value);
                    let mut value = f64::from(current);
                    let resp = widgets::labelled(ui, &axis.name, |ui| {
                        widgets::number(
                            ui,
                            widgets::Lead::None,
                            &mut value,
                            f64::from(axis.max - axis.min) / 200.0,
                            "",
                            110.0,
                        )
                    });
                    let value = value.clamp(f64::from(axis.min), f64::from(axis.max)) as f32;
                    let mut next: Vec<serde_json::Value> = vars
                        .iter()
                        .filter(|v| v.tag != axis.tag)
                        .map(|v| json!({ "tag": v.tag, "value": v.value }))
                        .collect();
                    if (value - axis.default).abs() > 1e-3 {
                        next.push(json!({ "tag": axis.tag, "value": value }));
                    }
                    edit(
                        app,
                        &resp,
                        "Variable axis",
                        "text.setStyle",
                        json!({ "ids": [id], "variations": next }),
                    );
                }
            }
            if selected_text_style(app, id).is_some() {
                use deesainer_doc::{ColorStop, Paint, PaintKind};
                let current = style.fill.clone();
                let base = current
                    .as_ref()
                    .and_then(Paint::primary_color)
                    .unwrap_or(deesainer_color::Rgba::BLACK);
                let mode = match current.as_ref().map(|p| &p.kind) {
                    None => 0,
                    Some(PaintKind::Solid { .. }) => 1,
                    Some(PaintKind::Image { .. }) => 3,
                    Some(_) => 2,
                };
                widgets::labelled(ui, "Fill", |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, label) in ["Layer", "Solid", "Gradient", "Image"].iter().enumerate() {
                        if widgets::tab(ui, label, mode == i).clicked() && mode != i {
                            let paint = match i {
                                0 => None,
                                1 => Some(Paint::solid(base)),
                                3 => {
                                    let first = app.session.doc().assets().next().map(|(id, _)| id.clone());
                                    match first {
                                        Some(asset) => Some(Paint {
                                            kind: PaintKind::Image {
                                                asset,
                                                scale_mode: deesainer_doc::ImageScaleMode::Fill,
                                                adjust: Default::default(),
                                            },
                                            ..Paint::solid(base)
                                        }),
                                        None => {
                                            app.info("Place an image on the canvas first, then pick it here.");
                                            continue;
                                        }
                                    }
                                }
                                _ => Some(Paint::linear(
                                    deesainer_geom::Point::new(0.0, 0.5),
                                    deesainer_geom::Point::new(1.0, 0.5),
                                    vec![
                                        ColorStop {
                                            position: 0.0,
                                            color: base,
                                        },
                                        ColorStop {
                                            position: 1.0,
                                            color: deesainer_color::Rgba::WHITE,
                                        },
                                    ],
                                )),
                            };
                            set_typography(app, id, json!({}), |s| s.fill = paint.clone());
                        }
                    }
                });
                match current.as_ref().map(|p| &p.kind) {
                    Some(PaintKind::Image { asset, scale_mode, .. }) => {
                        let assets: Vec<deesainer_doc::AssetId> = app
                            .session
                            .doc()
                            .assets()
                            .map(|(id, _)| id.clone())
                            .collect();
                        let short = |a: &deesainer_doc::AssetId| a.0.chars().take(8).collect::<String>();
                        let mut picked = None;
                        widgets::labelled(ui, "Image", |ui| {
                            egui::ComboBox::from_id_salt(("range-image", id.0))
                                .selected_text(short(asset))
                                .show_ui(ui, |ui| {
                                    for a in &assets {
                                        if ui.selectable_label(a == asset, short(a)).clicked() {
                                            picked = Some(a.clone());
                                        }
                                    }
                                });
                        });
                        if let Some(new_asset) = picked {
                            let mode = *scale_mode;
                            set_typography(app, id, json!({}), |s| {
                                s.fill = Some(Paint {
                                    kind: PaintKind::Image {
                                        asset: new_asset.clone(),
                                        scale_mode: mode,
                                        adjust: Default::default(),
                                    },
                                    ..Paint::solid(base)
                                });
                            });
                        }
                    }
                    Some(PaintKind::Solid { color }) => {
                        let mut c = color32(*color);
                        widgets::labelled(ui, "Color", |ui| {
                            ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                            if ui.color_edit_button_srgba(&mut c).changed() {
                                let v = rgba(c);
                                set_typography(app, id, json!({}), |s| {
                                    s.fill = Some(Paint::solid(v))
                                });
                            }
                            ui.label(widgets::hex(c));
                        });
                    }
                    Some(
                        PaintKind::LinearGradient { stops, .. }
                        | PaintKind::RadialGradient { stops, .. },
                    ) => {
                        let (first, last) = (
                            stops.first().map(|s| s.color),
                            stops.last().map(|s| s.color),
                        );
                        widgets::labelled(ui, "Stops", |ui| {
                            ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                            for (slot, color) in [(0usize, first), (1, last)] {
                                let Some(color) = color else { continue };
                                let mut c = color32(color);
                                if ui.color_edit_button_srgba(&mut c).changed() {
                                    let v = rgba(c);
                                    set_typography(app, id, json!({}), |s| {
                                        if let Some(p) = &mut s.fill {
                                            if let PaintKind::LinearGradient { stops, .. }
                                            | PaintKind::RadialGradient { stops, .. } =
                                                &mut p.kind
                                            {
                                                let at = if slot == 0 {
                                                    0
                                                } else {
                                                    stops.len().saturating_sub(1)
                                                };
                                                if let Some(stop) = stops.get_mut(at) {
                                                    stop.color = v;
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        });
                    }
                    _ => {}
                }
            }
            let case = enum_str(t.style.case);
            let keys = ["original", "upper", "lower", "title"];
            widgets::labelled(ui, "Case", |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (k, label) in keys.iter().zip(["-", "AG", "ag", "Ag"]) {
                    if widgets::tab(ui, label, case == *k).clicked() {
                        set_layer_text_style(app, id, json!({ "ids": [id], "case": k }));
                    }
                }
            });
            let mut italic = style.italic;
            widgets::labelled(ui, "Style", |ui| {
                if ui.checkbox(&mut italic, "Italic").changed() {
                    set_typography(app, id, json!({ "ids": [id], "italic": italic }), |style| {
                        style.italic = italic
                    });
                }
            });
            let align = enum_str(t.align_h);
            let keys = ["left", "center", "right", "justified"];
            let items = [
                (Icon::TextLeft, "Left"),
                (Icon::TextCenter, "Center"),
                (Icon::TextRight, "Right"),
                (Icon::TextJustify, "Justified"),
            ];
            widgets::labelled(ui, "Alignment", |ui| {
                if let Some(i) =
                    widgets::segmented(ui, &items, keys.iter().position(|k| *k == align), 32.0)
                {
                    set_layer_text_style(app, id, json!({ "ids": [id], "align": keys[i] }));
                }
            });
        });
    }
}

#[cfg(test)]
mod typography_tests {
    use super::*;
    use deesainer_text::EditAction;

    #[test]
    fn selected_panel_typography_commits_with_text_as_one_document_step() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let id = app
            .session
            .dispatch(
                "node.create.text",
                &json!({"x": 0, "y": 0, "content": "Hello", "fontFamily": "sans-serif"}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let id = NodeId(id);
        app.start_editing(id);
        app.editing
            .as_mut()
            .unwrap()
            .editor
            .action(EditAction::Insert("!".into()))
            .unwrap();
        app.editing
            .as_mut()
            .unwrap()
            .editor
            .action(EditAction::SelectAll)
            .unwrap();
        set_typography(
            &mut app,
            id,
            json!({"ids": [id], "fontSize": 30.0}),
            |style| style.font_size = 30.0,
        );
        assert_eq!(selected_text_style(&app, id).unwrap().font_size, 30.0);
        assert!(app
            .session
            .doc()
            .get(id)
            .unwrap()
            .text()
            .unwrap()
            .runs
            .is_empty());
        app.commit_editing();
        let text = app.session.doc().get(id).unwrap().text().unwrap();
        assert_eq!(text.content, "Hello!");
        assert_eq!(text.runs[0].style.font_size, 30.0);
        app.session.dispatch("edit.undo", &json!({})).unwrap();
        let text = app.session.doc().get(id).unwrap().text().unwrap();
        assert_eq!(text.content, "Hello");
        assert!(text.runs.is_empty());
    }

    #[test]
    fn panel_typography_without_selected_range_updates_layer() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let id = app
            .session
            .dispatch(
                "node.create.text",
                &json!({"x": 0, "y": 0, "content": "Hello", "fontFamily": "sans-serif"}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let id = NodeId(id);
        set_typography(
            &mut app,
            id,
            json!({"ids": [id], "fontSize": 30.0}),
            |style| style.font_size = 30.0,
        );
        assert_eq!(
            app.session
                .doc()
                .get(id)
                .unwrap()
                .text()
                .unwrap()
                .style
                .font_size,
            30.0
        );
    }

    #[test]
    fn global_case_commits_selected_draft_and_groups_undo() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let id = app
            .session
            .dispatch(
                "node.create.text",
                &json!({"x": 0, "y": 0, "content": "Hello", "fontFamily": "sans-serif"}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let id = NodeId(id);
        app.start_editing(id);
        app.editing
            .as_mut()
            .unwrap()
            .editor
            .action(EditAction::Insert("!".into()))
            .unwrap();
        app.editing
            .as_mut()
            .unwrap()
            .editor
            .action(EditAction::SelectAll)
            .unwrap();
        set_typography(
            &mut app,
            id,
            json!({"ids": [id], "fontSize": 30.0}),
            |style| style.font_size = 30.0,
        );
        set_layer_text_style(&mut app, id, json!({"ids": [id], "case": "upper"}));
        assert!(app.editing.is_none());
        let text = app.session.doc().get(id).unwrap().text().unwrap();
        assert_eq!(text.content, "Hello!");
        assert_eq!(text.runs[0].style.font_size, 30.0);
        assert_eq!(text.runs[0].style.case, deesainer_doc::TextCase::Upper);
        app.session.dispatch("edit.undo", &json!({})).unwrap();
        let text = app.session.doc().get(id).unwrap().text().unwrap();
        assert_eq!(text.content, "Hello");
        assert!(text.runs.is_empty());
        assert_eq!(text.style.case, deesainer_doc::TextCase::Original);
    }
}
