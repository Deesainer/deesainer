use base64::Engine as _;
use serde_json::{json, Value};

use crate::theme::TOKENS;
use crate::{widgets, App};

#[derive(Default)]
pub(crate) struct HistoryView {
    pub open: bool,
    label: String,
    picked: Option<u64>,
    preview: Option<egui::TextureHandle>,
    diff: Option<Value>,
    visual: Option<(egui::TextureHandle, u64)>,
    show_visual: bool,
}

fn texture(ctx: &egui::Context, name: String, b64: &str) -> Option<egui::TextureHandle> {
    let png = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = deesainer_render::images::decode(&png)?;
    let image = egui::ColorImage::from_rgba_premultiplied(
        [img.width() as usize, img.height() as usize],
        img.data(),
    );
    Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
}

fn load_visual(app: &mut App, ctx: &egui::Context, id: u64) -> Option<(egui::TextureHandle, u64)> {
    let reply = app
        .session
        .dispatch("version.compare", &json!({ "id": id, "scale": 0.5 }))
        .ok()?;
    let tex = texture(ctx, format!("version-diff:{id}"), reply["diff"].as_str()?)?;
    Some((tex, reply["changedPixels"].as_u64().unwrap_or(0)))
}

fn load_preview(app: &mut App, ctx: &egui::Context, id: u64) -> Option<egui::TextureHandle> {
    let reply = app
        .session
        .dispatch("version.preview", &json!({ "id": id, "scale": 0.5 }))
        .ok()?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(reply["data"].as_str()?)
        .ok()?;
    let img = deesainer_render::images::decode(&png)?;
    let image = egui::ColorImage::from_rgba_premultiplied(
        [img.width() as usize, img.height() as usize],
        img.data(),
    );
    Some(ctx.load_texture(format!("version:{id}"), image, egui::TextureOptions::LINEAR))
}

fn diff_summary(d: &Value) -> String {
    let n = |k: &str| d[k].as_array().map_or(0, Vec::len);
    let (a, r, c) = (n("added"), n("removed"), n("changed"));
    if a + r + c == 0 {
        return "Same as now.".to_owned();
    }
    let mut parts = Vec::new();
    if a > 0 {
        parts.push(format!("{a} layer(s) come back"));
    }
    if r > 0 {
        parts.push(format!("{r} layer(s) go away"));
    }
    if c > 0 {
        parts.push(format!("{c} layer(s) change"));
    }
    format!("Restoring it: {}.", parts.join(", "))
}

pub(crate) fn window(app: &mut App, ctx: &egui::Context) {
    if !app.history.open {
        return;
    }
    let mut open = true;
    let mut action: Option<(&str, Value)> = None;
    let mut pick: Option<u64> = None;
    let list = app
        .session
        .dispatch("version.list", &json!({}))
        .unwrap_or_else(|_| json!([]));
    egui::Window::new("Version history")
        .open(&mut open)
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.history.label)
                        .hint_text("Version name")
                        .desired_width(200.0),
                );
                if widgets::primary(ui, "Save version").clicked() {
                    let label = std::mem::take(&mut app.history.label);
                    action = Some(("version.save", json!({ "label": label })));
                }
            });
            ui.separator();
            let versions = list.as_array().cloned().unwrap_or_default();
            if versions.is_empty() {
                ui.label(
                    egui::RichText::new("No saved versions. Save one to come back to it later.")
                        .color(TOKENS.text_secondary),
                );
            }
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for v in &versions {
                        let id = v["id"].as_u64().unwrap_or(0);
                        let on = app.history.picked == Some(id);
                        let when = v["savedAt"].as_str().unwrap_or("");
                        let who = v["author"].as_str().unwrap_or("");
                        let text = format!(
                            "{}\n{}{}",
                            v["label"].as_str().unwrap_or(""),
                            when.get(..16).unwrap_or(when).replace('T', " "),
                            if who.is_empty() {
                                String::new()
                            } else {
                                format!(" · {who}")
                            }
                        );
                        if ui.selectable_label(on, text).clicked() {
                            pick = Some(id);
                        }
                    }
                });
            if let Some(id) = app.history.picked {
                ui.separator();
                ui.checkbox(&mut app.history.show_visual, "Show differences");
                let shown = if app.history.show_visual {
                    app.history.visual.as_ref().map(|(t, _)| t)
                } else {
                    app.history.preview.as_ref()
                };
                if let Some(tex) = shown {
                    let size = tex.size_vec2();
                    let k = (ui.available_width() / size.x).min(200.0 / size.y).min(1.0);
                    ui.image((tex.id(), size * k));
                }
                if let (true, Some((_, n))) = (app.history.show_visual, &app.history.visual) {
                    ui.label(
                        egui::RichText::new(if *n == 0 {
                            "No visible difference on this page.".to_owned()
                        } else {
                            format!("{n} pixel(s) differ (in magenta).")
                        })
                        .color(TOKENS.text_secondary),
                    );
                }
                if let Some(d) = &app.history.diff {
                    ui.label(diff_summary(d));
                }
                ui.horizontal(|ui| {
                    if widgets::primary(ui, "Restore").clicked() {
                        action = Some(("version.restore", json!({ "id": id })));
                    }
                    if ui.button("Delete").clicked() {
                        action = Some(("version.delete", json!({ "id": id })));
                    }
                });
            }
        });
    if let Some(id) = pick {
        app.history.picked = Some(id);
        app.history.preview = load_preview(app, ctx, id);
        app.history.diff = app
            .session
            .dispatch("version.diff", &json!({ "id": id }))
            .ok();
        app.history.visual = load_visual(app, ctx, id);
    }
    if let Some((cmd, params)) = action {
        let restoring = cmd == "version.restore";
        app.dispatch(cmd, params);
        if restoring || cmd == "version.delete" {
            app.history.picked = None;
            app.history.preview = None;
            app.history.diff = None;
            app.history.visual = None;
        }
        if restoring {
            app.fitted = false;
            app.info("Restored the version; the previous state is saved in the history");
        }
    }
    if !open {
        app.history = HistoryView::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_reads_as_what_restoring_does() {
        assert_eq!(
            diff_summary(&json!({"added": [], "removed": [], "changed": []})),
            "Same as now."
        );
        assert_eq!(
            diff_summary(&json!({"added": [1], "removed": [], "changed": [1, 2]})),
            "Restoring it: 1 layer(s) come back, 2 layer(s) change."
        );
    }
}
