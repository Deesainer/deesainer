use serde_json::{json, Map, Value};

use crate::theme::TOKENS;
use crate::App;

pub const MARKER: char = '\u{2}';

pub(crate) struct PluginPanel {
    module: Value,
    spec: Value,
    values: Map<String, Value>,
    log: Vec<String>,
}

const MAX_CONTROLS: usize = 64;

fn defaults(spec: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    for c in spec["controls"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_CONTROLS)
    {
        let Some(id) = c["id"].as_str() else { continue };
        let v = match c["type"].as_str() {
            Some("checkbox") => json!(c["default"].as_bool().unwrap_or(false)),
            Some("number") => json!(c["default"].as_f64().unwrap_or(0.0)),
            Some("select") => c["default"]
                .as_str()
                .map(|s| json!(s))
                .or_else(|| c["options"].get(0).cloned())
                .unwrap_or(Value::Null),
            _ => json!(c["default"].as_str().unwrap_or("")),
        };
        out.insert(id.to_owned(), v);
    }
    out
}

pub(crate) fn open_from_message(app: &mut App, message: &str) -> bool {
    let Some(rest) = message.strip_prefix(MARKER) else {
        return false;
    };
    let Some((module, spec)) = rest.split_once(MARKER) else {
        return false;
    };
    let (Ok(module), Ok(spec)) = (
        serde_json::from_str::<Value>(module),
        serde_json::from_str::<Value>(spec),
    ) else {
        app.status = Some(("The plugin's panel is not valid JSON".into(), true));
        return true;
    };
    let values = defaults(&spec);
    app.plugin_panel = Some(PluginPanel {
        module,
        spec,
        values,
        log: Vec::new(),
    });
    true
}

fn run(app: &mut App, entry: &str) {
    let Some(panel) = app.plugin_panel.as_mut() else {
        return;
    };
    let mut params = panel.module.clone();
    params["entry"] = json!(entry);
    params["input"] = Value::Object(panel.values.clone());
    match app.session.dispatch("plugin.run", &params) {
        Ok(report) => {
            if let Some(p) = app.plugin_panel.as_mut() {
                p.log = report["logs"]
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if report["panel"].is_object() {
                    let mut values = defaults(&report["panel"]);
                    for (k, v) in &p.values {
                        if values.contains_key(k) {
                            values.insert(k.clone(), v.clone());
                        }
                    }
                    p.spec = report["panel"].clone();
                    p.values = values;
                }
            }
        }
        Err(e) => app.status = Some((e.to_string(), true)),
    }
}

pub(crate) fn window(app: &mut App, ctx: &egui::Context) {
    let Some(panel) = app.plugin_panel.as_mut() else {
        return;
    };
    let title = panel.spec["title"]
        .as_str()
        .unwrap_or("Plugin")
        .chars()
        .take(60)
        .collect::<String>();
    let mut open = true;
    let mut pressed: Option<String> = None;
    egui::Window::new(title)
        .open(&mut open)
        .default_width(280.0)
        .show(ctx, |ui| {
            let controls = panel.spec["controls"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for (i, c) in controls.iter().take(MAX_CONTROLS).enumerate() {
                let label = c["label"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect::<String>();
                let id = c["id"].as_str().unwrap_or("").to_owned();
                ui.push_id(i, |ui| match c["type"].as_str() {
                    Some("text") => {
                        ui.label(label);
                    }
                    Some("input") => {
                        let mut s = panel
                            .values
                            .get(&id)
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        ui.horizontal(|ui| {
                            ui.label(&label);
                            if ui.text_edit_singleline(&mut s).changed() {
                                panel.values.insert(id.clone(), json!(s));
                            }
                        });
                    }
                    Some("number") => {
                        let mut v = panel.values.get(&id).and_then(Value::as_f64).unwrap_or(0.0);
                        let (lo, hi) = (
                            c["min"].as_f64().unwrap_or(f64::MIN),
                            c["max"].as_f64().unwrap_or(f64::MAX),
                        );
                        ui.horizontal(|ui| {
                            ui.label(&label);
                            if ui
                                .add(egui::DragValue::new(&mut v).range(lo..=hi))
                                .changed()
                            {
                                panel.values.insert(id.clone(), json!(v));
                            }
                        });
                    }
                    Some("checkbox") => {
                        let mut on = panel
                            .values
                            .get(&id)
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        if ui.checkbox(&mut on, &label).changed() {
                            panel.values.insert(id.clone(), json!(on));
                        }
                    }
                    Some("select") => {
                        let options: Vec<String> = c["options"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|o| o.as_str().map(str::to_owned))
                            .take(100)
                            .collect();
                        let current = panel
                            .values
                            .get(&id)
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        ui.horizontal(|ui| {
                            ui.label(&label);
                            egui::ComboBox::from_id_salt(("plugin-select", i))
                                .selected_text(&current)
                                .show_ui(ui, |ui| {
                                    for o in &options {
                                        if ui.selectable_label(*o == current, o).clicked() {
                                            panel.values.insert(id.clone(), json!(o));
                                        }
                                    }
                                });
                        });
                    }
                    Some("button") if ui.button(&label).clicked() => {
                        pressed = c["entry"].as_str().map(str::to_owned);
                    }
                    _ => {}
                });
            }
            if !panel.log.is_empty() {
                ui.separator();
                for l in panel.log.iter().take(20) {
                    ui.label(egui::RichText::new(l).color(TOKENS.text_secondary));
                }
            }
        });
    if let Some(entry) = pressed {
        run(app, &entry);
    }
    if !open {
        app.plugin_panel = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL_PLUGIN: &str = r#"
        (module
          (import "deesainer" "call" (func $call (param i32 i32 i32 i32) (result i32)))
          (import "deesainer" "input" (func $input (param i32) (result i32)))
          (import "deesainer" "panel" (func $panel (param i32 i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "{\"title\":\"Grid maker\",\"controls\":[{\"type\":\"number\",\"id\":\"count\",\"label\":\"Count\",\"default\":2},{\"type\":\"button\",\"label\":\"Make\",\"entry\":\"make\"}]}")
          (data (i32.const 400) "node.create.rectangle")
          (data (i32.const 440) "{\"x\":0,\"y\":0,\"width\":10,\"height\":10}")
          (func (export "run") (call $panel (i32.const 0) (i32.const 142)))
          (func (export "make")
            (local $n i32)
            ;; The input is {"count":N.0}: read it and make one rectangle per
            ;; call while the first digit allows (tests keep N small).
            (drop (call $input (i32.const 1000)))
            (local.set $n (i32.sub (i32.load8_u (i32.const 1009)) (i32.const 48)))
            (block $done (loop $more
              (br_if $done (i32.le_s (local.get $n) (i32.const 0)))
              (drop (call $call (i32.const 400) (i32.const 21) (i32.const 440) (i32.const 36)))
              (local.set $n (i32.sub (local.get $n) (i32.const 1)))
              (br $more)))))
    "#;

    #[test]
    fn a_plugin_panel_opens_and_its_buttons_run_with_the_values() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        let report = app
            .session
            .dispatch("plugin.run", &json!({ "wat": PANEL_PLUGIN }))
            .unwrap();
        let spec = report["panel"].clone();
        assert_eq!(spec["title"], json!("Grid maker"));
        let module = json!({ "wat": PANEL_PLUGIN });
        assert!(open_from_message(
            &mut app,
            &format!("{MARKER}{module}{MARKER}{spec}")
        ));
        app.plugin_panel
            .as_mut()
            .unwrap()
            .values
            .insert("count".into(), json!(3));
        run(&mut app, "make");
        let made = app
            .session
            .doc()
            .nodes()
            .filter(|n| matches!(n.kind, deesainer_doc::NodeKind::Rectangle))
            .count();
        assert_eq!(made, 3);
    }
}
