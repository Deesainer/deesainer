use deesainer_doc::{Document, NodeId, NodeKind};
use egui::{vec2, Sense};
use serde_json::json;

use crate::theme::{self, TOKENS};
use crate::widgets;
use crate::App;

#[derive(Clone, Copy)]
pub(crate) struct AssetDrag(pub NodeId);

struct Asset {
    id: NodeId,
    name: String,
    group: String,
}

fn assets(doc: &Document, query: &str) -> Vec<(String, Vec<Asset>)> {
    let q = query.trim().to_lowercase();
    let mut by_source: std::collections::BTreeMap<(bool, String), Vec<Asset>> = Default::default();
    for n in doc.nodes() {
        if !matches!(n.kind, NodeKind::Component { .. }) {
            continue;
        }
        let set = n
            .parent
            .and_then(|p| doc.get(p))
            .filter(|p| matches!(p.kind, NodeKind::ComponentSet));
        if set.is_some_and(|s| s.children.first() != Some(&n.id)) {
            continue;
        }
        let name = set.map_or(n.name.clone(), |s| s.name.clone());
        if !q.is_empty() && !name.to_lowercase().contains(&q) {
            continue;
        }
        let source = n
            .origin
            .as_ref()
            .map_or((false, "Local components".to_owned()), |o| {
                (true, o.library.clone())
            });
        let group = name
            .split_once('/')
            .map(|(g, _)| g.trim().to_owned())
            .unwrap_or_default();
        by_source.entry(source).or_default().push(Asset {
            id: n.id,
            name,
            group,
        });
    }
    by_source
        .into_iter()
        .map(|((_, name), mut list)| {
            list.sort_by(|a, b| {
                (a.group.as_str(), a.name.as_str()).cmp(&(b.group.as_str(), b.name.as_str()))
            });
            (name, list)
        })
        .collect()
}

fn thumbnail(app: &mut App, ctx: &egui::Context, id: NodeId) -> Option<egui::TextureHandle> {
    let rev = app.session.revision();
    if let Some((r, tex)) = app.asset_thumbs.get(&id) {
        if *r == rev {
            return tex.clone();
        }
    }
    let doc = app.session.doc();
    let b = doc.world_bounds(id)?;
    let scale = (208.0 / b.width.max(1.0))
        .min(128.0 / b.height.max(1.0))
        .min(4.0);
    let opts = deesainer_render::Options {
        background: false,
        text: Some(deesainer_engine::text_painter()),
        hidden: &[],
    };
    let tex = deesainer_render::render_node(doc, id, scale, opts)
        .ok()
        .map(|(pix, _)| {
            let img = egui::ColorImage::from_rgba_premultiplied(
                [pix.width() as usize, pix.height() as usize],
                pix.data(),
            );
            ctx.load_texture(format!("asset:{}", id.0), img, egui::TextureOptions::LINEAR)
        });
    if app.asset_thumbs.len() > 2000 {
        app.asset_thumbs.clear();
    }
    app.asset_thumbs.insert(id, (rev, tex.clone()));
    tex
}

pub(crate) fn place(app: &mut App, component: NodeId, at: deesainer_geom::Point) {
    let (w, h) = app
        .session
        .doc()
        .get(component)
        .map_or((0.0, 0.0), |n| (n.rect.width, n.rect.height));
    app.dispatch(
        "component.instantiate",
        json!({
            "component": component,
            "x": (at.x - w / 2.0).round(),
            "y": (at.y - h / 2.0).round(),
        }),
    );
}

pub fn panel(app: &mut App, ui: &mut egui::Ui) {
    widgets::divider(ui);
    let key = egui::Id::new("asset-search");
    let mut q: String = ui.data_mut(|d| d.get_temp(key).unwrap_or_default());
    ui.add_space(8.0);
    widgets::field(ui, ui.available_width() - 16.0, |ui| {
        crate::icons::glyph(ui, crate::icons::Icon::Search, TOKENS.text_secondary);
        ui.add(
            egui::TextEdit::singleline(&mut q)
                .frame(egui::Frame::NONE)
                .hint_text("Search assets")
                .desired_width(f32::INFINITY),
        );
    });
    ui.data_mut(|d| d.insert_temp(key, q.clone()));
    ui.add_space(8.0);
    let doc = app.session.snapshot();
    let sources = assets(&doc, &q);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if sources.is_empty() {
                let text = if q.trim().is_empty() {
                    "No components yet. Select layers and press ⌥⌘K."
                } else {
                    "No components match."
                };
                ui.label(egui::RichText::new(text).color(TOKENS.text_secondary));
                return;
            }
            if sources.iter().any(|(name, _)| name == "Local components") {
                publish_row(app, ui);
            }
            for (source, list) in sources {
                widgets::header(ui, &source, |_| {});
                let mut group: Option<&str> = None;
                let tile = vec2(((ui.available_width() - 8.0) / 2.0).floor(), 96.0);
                let mut row: Vec<&Asset> = Vec::new();
                let flush = |ui: &mut egui::Ui, app: &mut App, row: &mut Vec<&Asset>| {
                    if row.is_empty() {
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        for a in row.iter() {
                            tile_ui(app, ui, a, tile);
                        }
                    });
                    ui.add_space(8.0);
                    row.clear();
                };
                for a in &list {
                    if group != Some(a.group.as_str()) {
                        flush(ui, app, &mut row);
                        group = Some(a.group.as_str());
                        if !a.group.is_empty() {
                            ui.label(egui::RichText::new(&a.group).color(TOKENS.text_secondary));
                        }
                    }
                    row.push(a);
                    if row.len() == 2 {
                        flush(ui, app, &mut row);
                    }
                }
                flush(ui, app, &mut row);
            }
        });
}

fn publish_row(app: &mut App, ui: &mut egui::Ui) {
    let key = egui::Id::new("publish-note");
    let mut note: String = ui.data_mut(|d| d.get_temp(key).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut note)
                .hint_text("What changed")
                .desired_width(ui.available_width() - 72.0),
        );
        if ui
            .button("Publish")
            .on_hover_text("Publish these components as a library")
            .clicked()
        {
            app.dispatch("library.publish", json!({ "note": note.trim() }));
            app.info("Published: files using this library can review and update");
            note.clear();
        }
    });
    ui.data_mut(|d| d.insert_temp(key, note));
    ui.add_space(8.0);
}

fn tile_ui(app: &mut App, ui: &mut egui::Ui, a: &Asset, size: egui::Vec2) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let fill = if resp.hovered() {
        TOKENS.row_hover
    } else {
        TOKENS.field
    };
    painter.rect_filled(rect, 6.0, fill);
    let pic = egui::Rect::from_min_max(rect.min + vec2(6.0, 6.0), rect.max - vec2(6.0, 24.0));
    if let Some(tex) = thumbnail(app, ui.ctx(), a.id) {
        let t = tex.size_vec2();
        let k = (pic.width() / t.x).min(pic.height() / t.y).min(0.5);
        let r = egui::Rect::from_center_size(pic.center(), t * k);
        painter.image(
            tex.id(),
            r,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    let label = a.name.rsplit('/').next().unwrap_or(&a.name).trim();
    painter.text(
        egui::pos2(rect.min.x + 8.0, rect.max.y - 12.0),
        egui::Align2::LEFT_CENTER,
        label,
        theme::body(),
        TOKENS.text,
    );
    if resp.drag_started() {
        resp.dnd_set_drag_payload(AssetDrag(a.id));
    }
    let resp = resp.on_hover_text(format!(
        "{}: click to place, or drag onto the canvas",
        a.name
    ));
    if resp.clicked() {
        let s = app.canvas.last_size;
        let center = app
            .camera
            .to_page(egui::Pos2::ZERO, egui::pos2(s.x / 2.0, s.y / 2.0));
        place(app, a.id, center);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::{LibraryRef, Node};
    use deesainer_geom::Rect;

    #[test]
    fn assets_group_by_source_and_name_and_filter() {
        let mut doc = Document::new();
        let page = doc.first_page();
        for (name, lib) in [
            ("Buttons/Primary", None),
            ("Buttons/Ghost", None),
            ("Avatar", None),
            ("Icon/Close", Some("Icons")),
        ] {
            let mut n = Node::new(
                doc.next_free_id(),
                name,
                NodeKind::Component {
                    description: String::new(),
                    properties: Vec::new(),
                },
                Rect::new(0.0, 0.0, 10.0, 10.0),
            );
            n.origin = lib.map(|l| LibraryRef {
                library: l.to_owned(),
                source: NodeId(1),
            });
            let at = doc.get(page).map_or(0, |p| p.children.len());
            doc.insert(n, page, at).unwrap();
        }
        let all = assets(&doc, "");
        assert_eq!(all[0].0, "Local components");
        assert_eq!(all[1].0, "Icons");
        let local: Vec<(&str, &str)> = all[0]
            .1
            .iter()
            .map(|a| (a.group.as_str(), a.name.as_str()))
            .collect();
        assert_eq!(
            local,
            [
                ("", "Avatar"),
                ("Buttons", "Buttons/Ghost"),
                ("Buttons", "Buttons/Primary")
            ]
        );
        let found = assets(&doc, "ghost");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.len(), 1);
    }
}
