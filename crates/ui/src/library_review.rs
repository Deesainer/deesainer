use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::theme::TOKENS;
use crate::App;

pub const MARKER: char = '\u{1}';

pub(crate) struct LibraryReview {
    library: String,
    data: String,
    changes: Value,
    accepted: BTreeSet<u64>,
}

pub(crate) fn open_from_message(app: &mut App, message: &str) -> bool {
    let Some(rest) = message.strip_prefix(MARKER) else {
        return false;
    };
    let Some((library, data)) = rest.split_once(MARKER) else {
        return false;
    };
    match app.session.dispatch(
        "library.changes",
        &json!({ "library": library, "data": data }),
    ) {
        Ok(changes) => {
            let accepted = changes["components"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter(|c| c["status"] == json!("changed"))
                        .filter_map(|c| c["source"].as_u64())
                        .collect()
                })
                .unwrap_or_default();
            app.library_review = Some(LibraryReview {
                library: library.to_owned(),
                data: data.to_owned(),
                changes,
                accepted,
            });
        }
        Err(e) => app.status = Some((e.to_string(), true)),
    }
    true
}

pub(crate) fn window(app: &mut App, ctx: &egui::Context) {
    let Some(review) = app.library_review.as_mut() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    egui::Window::new(format!("Update {}", review.library))
        .open(&mut open)
        .default_width(360.0)
        .show(ctx, |ui| {
            if let Some(p) = review.changes["published"].as_object() {
                let when = p.get("savedAt").and_then(Value::as_str).unwrap_or("");
                ui.label(format!(
                    "{} · {}",
                    p.get("label").and_then(Value::as_str).unwrap_or(""),
                    when.get(..16).unwrap_or(when).replace('T', " ")
                ));
                ui.separator();
            }
            let list = review.changes["components"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for c in &list {
                let source = c["source"].as_u64().unwrap_or(0);
                let name = c["name"].as_str().unwrap_or("");
                let status = c["status"].as_str().unwrap_or("");
                let uses = c["instances"].as_u64().unwrap_or(0);
                let detail = match status {
                    "changed" => format!(
                        "{} changed, {} added, {} removed layer(s) · {uses} instance(s)",
                        c["changed"], c["added"], c["removed"]
                    ),
                    "removed" => format!("no longer in the library · {uses} instance(s)"),
                    _ => "up to date".to_owned(),
                };
                ui.horizontal(|ui| {
                    let mut on = review.accepted.contains(&source);
                    let enabled = status == "changed";
                    if ui
                        .add_enabled(enabled, egui::Checkbox::new(&mut on, name))
                        .changed()
                    {
                        if on {
                            review.accepted.insert(source);
                        } else {
                            review.accepted.remove(&source);
                        }
                    }
                    ui.label(egui::RichText::new(detail).color(TOKENS.text_secondary));
                });
            }
            if let Some(avail) = review.changes["available"]
                .as_array()
                .filter(|a| !a.is_empty())
            {
                ui.separator();
                let names: Vec<&str> = avail.iter().filter_map(|a| a["name"].as_str()).collect();
                ui.label(
                    egui::RichText::new(format!(
                        "Also in the library, not imported: {}",
                        names.join(", ")
                    ))
                    .color(TOKENS.text_secondary),
                );
            }
            ui.separator();
            let n = review.accepted.len();
            if ui
                .add_enabled(n > 0, egui::Button::new(format!("Update {n} component(s)")))
                .clicked()
            {
                apply = true;
            }
        });
    if apply {
        if let Some(r) = app.library_review.take() {
            let ids: Vec<u64> = r.accepted.into_iter().collect();
            app.dispatch(
                "library.update",
                json!({ "library": r.library, "data": r.data, "components": ids }),
            );
            app.info(format!(
                "Updated {} component(s) from {}",
                ids.len(),
                r.library
            ));
        }
    } else if !open {
        app.library_review = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    #[test]
    fn a_review_request_opens_the_review_with_changed_components_checked() {
        let mut lib = deesainer_engine::Session::new().unwrap();
        let r = lib
            .dispatch(
                "node.create.rectangle",
                &json!({"x": 0, "y": 0, "width": 10, "height": 10, "fill": "#0D99FF"}),
            )
            .unwrap()["id"]
            .clone();
        lib.dispatch("component.create", &json!({"ids": [r]}))
            .unwrap();
        let v1 = lib.dispatch("document.serialize", &json!({})).unwrap()["data"].clone();
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        app.session
            .dispatch("library.import", &json!({"data": v1, "library": "UI"}))
            .unwrap();
        lib.dispatch("node.setProps", &json!({"ids": [r], "fill": "#FF0000"}))
            .unwrap();
        let v2 = lib.dispatch("document.serialize", &json!({})).unwrap()["data"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(base64::engine::general_purpose::STANDARD
            .decode(&v2)
            .is_ok());
        assert!(!open_from_message(&mut app, "Saved a file"));
        assert!(open_from_message(
            &mut app,
            &format!("{MARKER}UI{MARKER}{v2}")
        ));
        let review = app.library_review.as_ref().unwrap();
        assert_eq!(review.accepted.len(), 1);
    }
}
