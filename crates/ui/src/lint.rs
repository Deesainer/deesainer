use serde_json::{json, Value};

use crate::theme::TOKENS;
use crate::{widgets, App};

#[derive(Default)]
pub(crate) struct LintView {
    pub open: bool,
    cached: Option<(u64, Value)>,
}

fn rule_label(rule: &str) -> &'static str {
    match rule {
        "detachedPaint" => "Paint style not used",
        "detachedText" => "Text style not used",
        "rawValue" => "Raw value",
        "offPixelGrid" => "Off the pixel grid",
        "hiddenLayer" => "Hidden layer",
        "emptyFrame" => "Empty frame",
        _ => "Check",
    }
}

pub(crate) fn window(app: &mut App, ctx: &egui::Context) {
    if !app.lint.open {
        return;
    }
    let rev = app.session.revision();
    if app.lint.cached.as_ref().is_none_or(|(r, _)| *r != rev) {
        let issues = app
            .session
            .dispatch("lint.run", &json!({}))
            .unwrap_or_else(|_| json!([]));
        app.lint.cached = Some((rev, issues));
    }
    let issues = app
        .lint
        .cached
        .as_ref()
        .map(|(_, v)| v.as_array().cloned().unwrap_or_default())
        .unwrap_or_default();
    let mut open = true;
    let mut select = None;
    let mut fix: Option<Value> = None;
    egui::Window::new("Design checks")
        .open(&mut open)
        .default_width(340.0)
        .show(ctx, |ui| {
            if issues.is_empty() {
                ui.label(
                    egui::RichText::new("Nothing to check on this page: it uses its styles and variables, on whole pixels.")
                        .color(TOKENS.text_secondary),
                );
                return;
            }
            let fixable = issues.iter().filter(|i| !i["fix"].is_null()).count();
            ui.horizontal(|ui| {
                ui.label(format!("{} issue(s), {fixable} fixable", issues.len()));
                if fixable > 0 && widgets::primary(ui, "Fix all").clicked() {
                    fix = Some(json!({}));
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, issue) in issues.iter().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            let rule = issue["rule"].as_str().unwrap_or("");
                            let text = format!(
                                "{} · {}\n{}",
                                rule_label(rule),
                                issue["name"].as_str().unwrap_or(""),
                                issue["message"].as_str().unwrap_or("")
                            );
                            if ui.selectable_label(false, text).clicked() {
                                select = issue["id"].as_u64();
                            }
                            if !issue["fix"].is_null() && ui.small_button("Fix").clicked() {
                                fix = Some(json!({ "ids": [issue["id"]], "rules": [rule] }));
                            }
                        });
                    });
                }
            });
        });
    if let Some(id) = select {
        app.dispatch("selection.set", json!({ "ids": [id] }));
        crate::canvas::zoom_to(app, "selection");
    }
    if let Some(params) = fix {
        app.dispatch("lint.fix", params);
    }
    if !open {
        app.lint = LintView::default();
    }
}
