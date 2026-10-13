use deesainer_doc::CommentId;
use egui::{pos2, vec2, Color32, Pos2, Stroke};
use serde_json::json;

use crate::theme::TOKENS;
use crate::App;

pub fn draw_pins(app: &App, painter: &egui::Painter, origin: Pos2) {
    if !app.show_comments {
        return;
    }
    let doc = app.session.doc();
    let page = app.session.current_page();
    for (i, (id, c)) in doc.comments().filter(|(_, c)| c.page == page).enumerate() {
        if c.resolved {
            continue;
        }
        let anchor = match c.node.and_then(|n| doc.world_bounds(n)) {
            Some(b) => deesainer_geom::Point::new(b.x + c.position.x, b.y + c.position.y),
            None => c.position,
        };
        let s = app.camera.to_screen(origin, anchor);
        let center = s + vec2(10.0, -10.0);
        painter.circle(
            center,
            11.0,
            TOKENS.component,
            Stroke::new(2.0, Color32::WHITE),
        );
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            format!("{}", i + 1),
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
        let _ = id;
    }
    if let Some((p, _)) = &app.comment_draft {
        let s = app.camera.to_screen(origin, *p);
        painter.circle(
            s + vec2(10.0, -10.0),
            11.0,
            TOKENS.selection,
            Stroke::new(2.0, Color32::WHITE),
        );
    }
}

pub fn draw_peers(app: &App, painter: &egui::Painter, origin: Pos2) {
    let doc = app.session.doc();
    let page = app.session.current_page();
    for peer in app.peers.iter().filter(|p| p.page == Some(page)) {
        for id in &peer.selection {
            let Some(node) = doc.get(*id) else { continue };
            let t = doc.world_transform(*id);
            let local = deesainer_geom::Rect::new(0.0, 0.0, node.rect.width, node.rect.height);
            let pts: Vec<Pos2> = local
                .corners()
                .iter()
                .map(|c| app.camera.to_screen(origin, t.apply(*c)))
                .collect();
            painter.add(egui::Shape::closed_line(pts, Stroke::new(2.0, peer.color)));
        }
        if let Some(c) = peer.cursor {
            let s = app.camera.to_screen(origin, c);
            let arrow = vec![
                s,
                s + vec2(0.0, 16.0),
                s + vec2(4.5, 12.0),
                s + vec2(11.0, 11.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                arrow,
                peer.color,
                Stroke::new(1.0, Color32::WHITE),
            ));
            let galley = painter.layout_no_wrap(
                peer.name.clone(),
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
            let bg =
                egui::Rect::from_min_size(s + vec2(12.0, 14.0), galley.size() + vec2(8.0, 4.0));
            painter.rect_filled(bg, 4.0, peer.color);
            painter.galley(bg.min + vec2(4.0, 2.0), galley, Color32::WHITE);
            if let Some(text) = &peer.chat {
                crate::chat::bubble(painter, pos2(bg.min.x, bg.max.y + 4.0), text, peer.color);
            }
        }
    }
}

pub fn comments_window(app: &mut App, ctx: &egui::Context) {
    let Some((pos, mut text)) = app.comment_draft.take() else {
        return;
    };
    let screen = app.camera.to_screen(app.canvas.last_origin, pos) + vec2(26.0, -24.0);
    let mut keep = true;
    let mut post = false;
    egui::Area::new(egui::Id::new("comment-draft"))
        .fixed_pos(screen)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(TOKENS.panel)
                .corner_radius(12)
                .stroke(Stroke::new(1.0, TOKENS.accent))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .shadow(egui::Shadow {
                    offset: [0, 2],
                    blur: 10,
                    spread: 0,
                    color: Color32::from_black_alpha(30),
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .frame(egui::Frame::NONE)
                                .hint_text("Add a comment")
                                .desired_width(220.0),
                        );
                        resp.request_focus();
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            post = true;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            keep = false;
                        }
                        let (r, send) =
                            ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
                        let fill = if text.trim().is_empty() {
                            TOKENS.text_tertiary
                        } else {
                            TOKENS.accent
                        };
                        ui.painter().circle_filled(r.center(), 11.0, fill);
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            "↑",
                            crate::theme::strong(),
                            Color32::WHITE,
                        );
                        if send.clicked() {
                            post = true;
                        }
                    });
                });
        });
    if post && !text.trim().is_empty() {
        app.dispatch(
            "comment.add",
            json!({ "x": pos.x, "y": pos.y, "text": text }),
        );
    } else if keep && !post {
        app.comment_draft = Some((pos, text));
    }
}

pub fn comments_panel(app: &mut App, ui: &mut egui::Ui) {
    crate::widgets::divider(ui);
    ui.add_space(8.0);
    let key = egui::Id::new("comment-search");
    let mut query: String = ui.data_mut(|d| d.get_temp(key).unwrap_or_default());
    crate::widgets::field(ui, ui.available_width(), |ui| {
        crate::icons::glyph(ui, crate::icons::Icon::Search, TOKENS.text_secondary);
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .frame(egui::Frame::NONE)
                .hint_text("Search")
                .desired_width(f32::INFINITY),
        );
    });
    ui.data_mut(|d| d.insert_temp(key, query.clone()));
    ui.add_space(8.0);
    let doc = app.session.snapshot();
    let page = app.session.current_page();
    let q = query.to_lowercase();
    let threads: Vec<(CommentId, deesainer_doc::Comment)> = doc
        .comments()
        .filter(|(_, c)| c.page == page)
        .filter(|(_, c)| {
            q.is_empty()
                || c.text.to_lowercase().contains(&q)
                || c.author.to_lowercase().contains(&q)
        })
        .map(|(id, c)| (id, c.clone()))
        .collect();
    if threads.is_empty() {
        ui.horizontal_top(|ui| {
            crate::icons::glyph(ui, crate::icons::Icon::Comment, TOKENS.text);
            ui.add(
                egui::Label::new(
                    app.tr("Give feedback, ask a question, or just leave a note. Click anywhere on the canvas to leave a comment."),
                )
                .wrap(),
            );
        });
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, (id, c)) in threads.iter().enumerate() {
                ui.push_id(id.0, |ui| {
                    crate::widgets::divider(ui);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(r.center(), 12.0, TOKENS.component);
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{}", i + 1),
                            crate::theme::strong(),
                            Color32::WHITE,
                        );
                        ui.label(egui::RichText::new(&c.author).font(crate::theme::strong()));
                        if !c.created.is_empty() {
                            let when: String = c
                                .created
                                .chars()
                                .take(16)
                                .collect::<String>()
                                .replace('T', " ");
                            ui.label(egui::RichText::new(when).color(TOKENS.text_secondary));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if c.resolved {
                                app.tr("Reopen")
                            } else {
                                app.tr("Resolve")
                            };
                            if crate::icons::button(
                                ui,
                                crate::icons::Icon::Close,
                                false,
                                app.tr("Delete"),
                            )
                            .clicked()
                            {
                                app.dispatch("comment.delete", json!({ "id": id.0 }));
                            }
                            if ui.small_button(label).clicked() {
                                app.dispatch(
                                    "comment.resolve",
                                    json!({ "id": id.0, "resolved": !c.resolved }),
                                );
                            }
                        });
                    });
                    let color = if c.resolved {
                        TOKENS.text_secondary
                    } else {
                        TOKENS.text
                    };
                    ui.add(egui::Label::new(egui::RichText::new(&c.text).color(color)).wrap());
                    for r in &c.replies {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(&r.author).font(crate::theme::strong()));
                            ui.label(&r.text);
                        });
                    }
                    let key = egui::Id::new(("reply", id.0));
                    let mut draft: String = ui.data_mut(|d| d.get_temp(key).unwrap_or_default());
                    crate::widgets::field(ui, ui.available_width(), |ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut draft)
                                .frame(egui::Frame::NONE)
                                .hint_text("Reply")
                                .desired_width(f32::INFINITY),
                        );
                        if resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !draft.trim().is_empty()
                        {
                            app.dispatch("comment.reply", json!({ "id": id.0, "text": draft }));
                            draft.clear();
                        }
                    });
                    ui.data_mut(|d| d.insert_temp(key, draft));
                    if ui.small_button("Show on canvas").clicked() {
                        let size = app.canvas.last_size;
                        app.camera.pan = size / 2.0
                            - vec2(
                                (c.position.x * app.camera.zoom) as f32,
                                (c.position.y * app.camera.zoom) as f32,
                            );
                    }
                    ui.add_space(8.0);
                });
            }
        });
}
