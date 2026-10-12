use egui::{vec2, Color32, Pos2};

use crate::theme::TOKENS;
use crate::App;

const LINGER: f64 = 4.0;

const MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Default)]
pub struct CursorChat {
    pub text: String,
    pub typing: bool,
    pub until: Option<f64>,
}

pub fn open(app: &mut App) {
    app.chat = Some(CursorChat {
        typing: true,
        ..CursorChat::default()
    });
}

pub fn bubble(painter: &egui::Painter, at: Pos2, text: &str, color: Color32) {
    let galley = painter.layout(
        text.to_owned(),
        egui::FontId::proportional(13.0),
        Color32::WHITE,
        220.0,
    );
    let r = egui::Rect::from_min_size(at, galley.size() + vec2(16.0, 10.0));
    painter.rect_filled(r, 12.0, color);
    painter.galley(r.min + vec2(8.0, 5.0), galley, Color32::WHITE);
}

pub fn show(app: &mut App, ui: &mut egui::Ui, origin: Pos2) {
    let now = ui.input(|i| i.time);
    let Some(chat) = app.chat.as_mut() else {
        return;
    };
    if !chat.typing && chat.until.is_some_and(|t| now > t) {
        app.chat = None;
        return;
    }
    let Some(at) = app.pointer_page.map(|p| app.camera.to_screen(origin, p)) else {
        return;
    };
    let anchor = at + vec2(14.0, 18.0);
    if chat.typing {
        let mut close = false;
        egui::Area::new(egui::Id::new("cursor-chat"))
            .fixed_pos(anchor)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(TOKENS.selection)
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        let edit = egui::TextEdit::singleline(&mut chat.text)
                            .char_limit(MAX_CHARS)
                            .hint_text("Say something")
                            .text_color(Color32::WHITE)
                            .frame(egui::Frame::NONE)
                            .desired_width(200.0);
                        let resp = ui.add(edit);
                        resp.request_focus();
                        let (enter, escape) = ui.input(|i| {
                            (
                                i.key_pressed(egui::Key::Enter),
                                i.key_pressed(egui::Key::Escape),
                            )
                        });
                        if escape {
                            close = true;
                        } else if enter {
                            chat.typing = false;
                            chat.until = Some(now + LINGER);
                        }
                    });
            });
        if close || (chat.text.is_empty() && !chat.typing) {
            app.chat = None;
        }
    } else {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("cursor-chat-sent"),
        ));
        bubble(&painter, anchor, &chat.text, TOKENS.selection);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}
