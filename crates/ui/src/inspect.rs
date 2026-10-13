use egui::{Align, Layout, RichText, Ui};
use serde_json::{json, Value};

use crate::theme::TOKENS;
use crate::{widgets, App};
use deesainer_doc::NodeId;

const LANGUAGES: &[(&str, &str)] = &[
    ("css", "CSS"),
    ("swiftui", "SwiftUI"),
    ("compose", "Compose"),
];

#[derive(Default)]
pub(crate) struct InspectCache {
    key: Option<(u64, Vec<NodeId>, &'static str)>,
    spec: Value,
    code: String,
    measure: Option<Value>,
}

fn refresh(app: &mut App, ids: &[NodeId]) {
    let key = (app.session.revision(), ids.to_vec(), app.inspect_language);
    if app.inspect.key.as_ref() == Some(&key) {
        return;
    }
    let mut cache = InspectCache {
        key: Some(key),
        ..InspectCache::default()
    };
    if let Some(id) = ids.first() {
        cache.spec = app
            .session
            .dispatch("inspect.spec", &json!({ "id": id }))
            .unwrap_or(Value::Null);
        cache.code = app
            .session
            .dispatch(
                "inspect.code",
                &json!({ "id": id, "language": app.inspect_language }),
            )
            .ok()
            .and_then(|v| v["code"].as_str().map(str::to_owned))
            .unwrap_or_default();
    }
    if let [a, b] = ids {
        cache.measure = app
            .session
            .dispatch("inspect.measure", &json!({ "from": a, "to": b }))
            .ok();
    }
    app.inspect = cache;
}

fn num(v: &Value) -> String {
    v.as_f64().map_or_else(|| "—".to_owned(), widgets::trim)
}

fn row(ui: &mut Ui, key: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(TOKENS.text_secondary));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(value.into()).selectable(true));
        });
    });
}

fn swatch_row(ui: &mut Ui, label: &str, hex: Option<&str>, detail: &str) {
    ui.horizontal(|ui| {
        if let Some(c) = hex.and_then(|h| deesainer_color::Rgba::from_hex(h).ok()) {
            widgets::swatch(ui, crate::theme::color32(c));
        }
        ui.add(egui::Label::new(label.to_owned()).selectable(true));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(detail).color(TOKENS.text_secondary));
        });
    });
}

fn paint_rows(app: &App, ui: &mut Ui, paints: &Value) {
    for p in paints.as_array().into_iter().flatten() {
        let opacity = p["opacity"].as_f64().unwrap_or(1.0);
        let pct = format!("{}%", widgets::trim(opacity * 100.0));
        match p["kind"].as_str() {
            Some("solid") => {
                let hex = p["color"].as_str().unwrap_or("");
                swatch_row(ui, hex, Some(hex), &pct);
            }
            Some(kind @ ("linearGradient" | "radialGradient")) => {
                let name = if kind == "linearGradient" {
                    app.tr("Linear")
                } else {
                    app.tr("Radial")
                };
                swatch_row(ui, &format!("{name} gradient"), None, &pct);
                for s in p["stops"].as_array().into_iter().flatten() {
                    let hex = s[1].as_str().unwrap_or("");
                    let at = s[0]
                        .as_f64()
                        .map_or_else(String::new, |v| format!("{}%", widgets::trim(v * 100.0)));
                    swatch_row(ui, hex, Some(hex), &at);
                }
            }
            Some("image") => {
                let mode = p["scaleMode"].as_str().unwrap_or("fill");
                swatch_row(ui, &format!("Image ({mode})"), None, &pct);
            }
            _ => {}
        }
    }
}

pub(crate) fn panel(app: &mut App, ui: &mut Ui) {
    let ids = app.session.selection().to_vec();
    refresh(app, &ids);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if ids.is_empty() {
                widgets::section(ui, app.tr("Inspect"), |_| {}, |ui| {
                    ui.label(
                        RichText::new(app.tr("Select a layer to see its measurements, styles and code. Select two to measure the distance between them."))
                            .color(TOKENS.text_secondary),
                    );
                });
                return;
            }
            if let Some(m) = app.inspect.measure.clone() {
                widgets::section(ui, "Distance", |_| {}, |ui| match m["relation"].as_str() {
                    Some("apart") => {
                        row(ui, app.tr("Horizontal"), num(&m["horizontalGap"]));
                        row(ui, app.tr("Vertical"), num(&m["verticalGap"]));
                    }
                    Some(rel @ ("inside" | "contains")) => {
                        ui.label(
                            RichText::new(if rel == "inside" {
                                app.tr("First layer inside the second")
                            } else {
                                app.tr("Second layer inside the first")
                            })
                            .color(TOKENS.text_secondary),
                        );
                        for k in ["left", "top", "right", "bottom"] {
                            let label = format!("{}{}", k[..1].to_uppercase(), &k[1..]);
                            row(ui, &label, num(&m[k]));
                        }
                    }
                    _ => {
                        ui.label(RichText::new("The layers overlap.").color(TOKENS.text_secondary));
                    }
                });
            }
            let spec = app.inspect.spec.clone();
            if spec.is_null() {
                return;
            }
            widgets::title_row(ui, spec["name"].as_str().unwrap_or(""), |ui| {
                ui.label(RichText::new(spec["kind"].as_str().unwrap_or("")).color(TOKENS.text_secondary));
            });
            if let [id] = ids.as_slice() {
                annotations(app, ui, *id);
            }
            widgets::section(ui, app.tr("Layout"), |_| {}, |ui| {
                row(ui, app.tr("Width"), num(&spec["width"]));
                row(ui, app.tr("Height"), num(&spec["height"]));
                row(ui, "X", num(&spec["x"]));
                row(ui, "Y", num(&spec["y"]));
                if spec["rotation"].as_f64().is_some_and(|r| r != 0.0) {
                    row(ui, "Rotation", format!("{}°", num(&spec["rotation"])));
                }
                if let Some(r) = spec["cornerRadius"].as_array() {
                    let v: Vec<String> = r.iter().map(num).collect();
                    let same = v.windows(2).all(|w| w.first() == w.last());
                    row(ui, "Radius", if same { v.first().cloned().unwrap_or_default() } else { v.join(" ") });
                }
                if spec["opacity"].as_f64().is_some_and(|o| o < 1.0) {
                    row(ui, "Opacity", format!("{}%", widgets::trim(spec["opacity"].as_f64().unwrap_or(1.0) * 100.0)));
                }
                if let Some(b) = spec["blend"].as_str() {
                    row(ui, app.tr("Blend"), b);
                }
                if let Some(l) = spec["layout"].as_object() {
                    let dir = if l.get("direction").and_then(Value::as_str) == Some("row") { "Horizontal" } else { "Vertical" };
                    row(ui, app.tr("Auto layout"), dir);
                    row(ui, "Gap", l.get("gap").map(num).unwrap_or_default());
                    if let Some(p) = l.get("padding").and_then(Value::as_array) {
                        row(ui, "Padding", p.iter().map(num).collect::<Vec<_>>().join(" "));
                    }
                    let align = format!(
                        "{} · {}",
                        l.get("primaryAlign").and_then(Value::as_str).unwrap_or(""),
                        l.get("counterAlign").and_then(Value::as_str).unwrap_or("")
                    );
                    row(ui, app.tr("Align"), align);
                }
                if let Some(l) = spec["inLayout"].as_object() {
                    let sizing = format!(
                        "{} × {}",
                        l.get("horizontal").and_then(Value::as_str).unwrap_or(""),
                        l.get("vertical").and_then(Value::as_str).unwrap_or("")
                    );
                    row(ui, app.tr("Resizing"), sizing);
                }
                if let Some(c) = spec["constraints"].as_array() {
                    let c: Vec<&str> = c.iter().filter_map(Value::as_str).collect();
                    row(ui, "Constraints", c.join(" · "));
                }
            });
            let styles = spec["styles"].as_array().cloned().unwrap_or_default();
            let variables = spec["variables"].as_array().cloned().unwrap_or_default();
            if !styles.is_empty() || !variables.is_empty() {
                widgets::section(ui, app.tr("Styles and variables"), |_| {}, |ui| {
                    for s in &styles {
                        row(ui, s["property"].as_str().unwrap_or(""), s["name"].as_str().unwrap_or(""));
                    }
                    for v in &variables {
                        let value = v["value"].as_str().map(|v| format!(" = {v}")).unwrap_or_default();
                        row(
                            ui,
                            v["property"].as_str().unwrap_or(""),
                            format!("{}{value}", v["name"].as_str().unwrap_or("")),
                        );
                    }
                });
            }
            if spec["fills"].as_array().is_some_and(|f| !f.is_empty()) {
                widgets::section(ui, app.tr("Fill"), |_| {}, |ui| paint_rows(app, ui, &spec["fills"]));
            }
            if let Some(st) = spec["stroke"].as_object() {
                widgets::section(ui, app.tr("Stroke"), |_| {}, |ui| {
                    paint_rows(app, ui, &spec["strokes"]);
                    row(ui, "Weight", st.get("weight").map(num).unwrap_or_default());
                    row(ui, "Position", st.get("align").and_then(Value::as_str).unwrap_or(""));
                    if let Some(d) = st.get("dash").and_then(Value::as_array).filter(|d| !d.is_empty()) {
                        row(ui, "Dash", d.iter().map(num).collect::<Vec<_>>().join(", "));
                    }
                });
            }
            if spec["effects"].as_array().is_some_and(|e| !e.is_empty()) {
                widgets::section(ui, app.tr("Effects"), |_| {}, |ui| {
                    for e in spec["effects"].as_array().into_iter().flatten() {
                        let kind = e["kind"].as_str().unwrap_or("");
                        let detail = if kind.ends_with("Blur") {
                            format!("blur {}", num(&e["blur"]))
                        } else {
                            format!("{}, {} · blur {} · spread {}", num(&e["x"]), num(&e["y"]), num(&e["blur"]), num(&e["spread"]))
                        };
                        let label = match kind {
                            "dropShadow" => app.tr("Drop shadow"),
                            "innerShadow" => app.tr("Inner shadow"),
                            "layerBlur" => app.tr("Layer blur"),
                            "backgroundBlur" => app.tr("Background blur"),
                            other => other,
                        };
                        swatch_row(ui, label, e["color"].as_str(), &detail);
                    }
                });
            }
            if let Some(t) = spec["text"].as_object() {
                widgets::section(ui, app.tr("Typography"), |_| {}, |ui| {
                    row(ui, "Font", t.get("fontFamily").and_then(Value::as_str).unwrap_or(""));
                    row(ui, "Size", t.get("fontSize").map(num).unwrap_or_default());
                    row(ui, "Weight", t.get("fontWeight").map(num).unwrap_or_default());
                    row(
                        ui,
                        app.tr("Line height"),
                        t.get("lineHeight").filter(|v| !v.is_null()).map_or_else(|| "Auto".to_owned(), num),
                    );
                    row(ui, "Letter spacing", t.get("letterSpacing").map(num).unwrap_or_default());
                    row(ui, "Align", t.get("align").and_then(Value::as_str).unwrap_or(""));
                });
            }
            if let Some(c) = spec["component"].as_object() {
                widgets::section(ui, app.tr("Component"), |_| {}, |ui| {
                    row(ui, "Main", c.get("main").and_then(Value::as_str).unwrap_or(""));
                    for pair in c.get("variant").and_then(Value::as_array).into_iter().flatten() {
                        row(ui, pair[0].as_str().unwrap_or(""), pair[1].as_str().unwrap_or(""));
                    }
                });
            }
            let id = ids.first().copied();
            let assets = spec["assets"].as_array().cloned().unwrap_or_default();
            widgets::section(ui, app.tr("Assets"), |_| {}, |ui| {
                for a in &assets {
                    let asset = a["asset"].as_str().unwrap_or("").to_owned();
                    let size = match (a["width"].as_u64(), a["height"].as_u64()) {
                        (Some(w), Some(h)) => format!("{w} × {h} px"),
                        _ => String::new(),
                    };
                    ui.horizontal(|ui| {
                        ui.label(format!("Image {size}"));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Download").clicked() {
                                crate::menu::run_file_action(app, &format!("saveAsset:{asset}"));
                            }
                        });
                    });
                }
                ui.horizontal(|ui| {
                    let name = spec["name"].as_str().unwrap_or("Export").replace(':', " ");
                    for (label, action) in [("PNG 2x", format!("export:png:2:{name}")), ("SVG", format!("export:svg:1:{name}")), ("PDF", format!("export:pdf:1:{name}"))] {
                        if ui.button(label).clicked() {
                            if let Some(id) = id {
                                let _ = app.session.dispatch("selection.set", &json!({ "ids": [id] }));
                            }
                            crate::menu::run_file_action(app, &action);
                        }
                    }
                });
            });
            let code = app.inspect.code.clone();
            widgets::section(
                ui,
                app.tr("Code"),
                |ui| {
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(code.clone());
                        let label = LANGUAGES
                            .iter()
                            .find(|(v, _)| *v == app.inspect_language)
                            .map_or("code", |(_, l)| *l);
                        app.status = Some((format!("Copied {label}"), true));
                    }
                    if let Some(picked) = widgets::choice(ui, 96.0, app.inspect_language, LANGUAGES) {
                        app.inspect_language = picked;
                    }
                },
                |ui| {
                    egui::Frame::NONE
                        .fill(TOKENS.border.gamma_multiply(0.35))
                        .corner_radius(4.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(code.as_str()).monospace()).selectable(true).wrap());
                        });
                },
            );
        });
}

fn annotations(app: &mut App, ui: &mut Ui, id: NodeId) {
    let list = app
        .session
        .doc()
        .get(id)
        .map(|n| n.annotations.clone())
        .unwrap_or_default();
    widgets::section(
        ui,
        "Annotations",
        |_| {},
        |ui| {
            let mut remove = None;
            for (i, a) in list.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}.", i + 1)).color(TOKENS.text_secondary));
                    ui.vertical(|ui| {
                        ui.label(&a.text);
                        if !a.props.is_empty() {
                            ui.label(
                                RichText::new(a.props.join(", ")).color(TOKENS.text_secondary),
                            );
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::icons::button(ui, crate::icons::Icon::Minus, false, "Remove")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                app.dispatch(
                    "annotation.remove",
                    serde_json::json!({ "id": id, "index": i }),
                );
            }
            let key = egui::Id::new(("annotation-draft", id.0));
            let mut draft: String = ui.data_mut(|d| d.get_temp(key).unwrap_or_default());
            let mut add = false;
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut draft)
                        .hint_text("Add a note for developers")
                        .desired_width(ui.available_width() - 48.0),
                );
                add = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                add |= ui.button("Add").clicked();
            });
            if add && !draft.trim().is_empty() {
                app.dispatch(
                    "annotation.add",
                    serde_json::json!({ "id": id, "text": draft.trim() }),
                );
                draft.clear();
            }
            ui.data_mut(|d| d.insert_temp(key, draft));
        },
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn frame(ctx: &egui::Context, app: &mut App) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            ..Default::default()
        };
        ctx.run_ui(input, |ui| app.ui(ui))
            .drop_without_applying_deltas();
    }

    #[test]
    fn the_inspect_tab_shows_spec_code_and_distance() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let a = app
            .dispatch(
                "node.create.rectangle",
                json!({"x": 0, "y": 0, "width": 80, "height": 40, "fill": "#FF8800"}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let b = app
            .dispatch(
                "node.create.rectangle",
                json!({"x": 100, "y": 0, "width": 20, "height": 20}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        crate::menu::run_ui_action(&mut app, "toggle:inspect");
        assert!(app.right_tab == crate::RightTab::Inspect);
        app.dispatch("selection.set", json!({"ids": [a]})).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(app.inspect.spec["width"], json!(80.0));
        assert!(
            app.inspect.code.contains("background: #FF8800;"),
            "{}",
            app.inspect.code
        );
        app.inspect_language = "compose";
        frame(&ctx, &mut app);
        assert!(app.inspect.code.contains("Box("), "{}", app.inspect.code);
        app.dispatch("selection.set", json!({"ids": [a, b]}))
            .unwrap();
        frame(&ctx, &mut app);
        let m = app.inspect.measure.clone().unwrap();
        assert_eq!(m["horizontalGap"], json!(20.0));
        crate::menu::run_ui_action(&mut app, "toggle:inspect");
        assert!(app.right_tab == crate::RightTab::Design);
    }
}
