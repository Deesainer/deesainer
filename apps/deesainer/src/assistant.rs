use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use deesainer_automation::ai::{self, AgentEvent, AiConfig, ToolOutcome, Turn, PRESETS};
use deesainer_automation::McpServer;
use deesainer_engine::Session;
use deesainer_ui::{Extension, ExtensionView};
use serde_json::Value;

enum Entry {
    User(String),
    Assistant(String),
    Tool(String),
    Error(String),
}

pub struct Assistant {
    config: AiConfig,
    turns: Vec<Turn>,
    log: Vec<Entry>,
    input: String,
    running: Option<(Receiver<AgentEvent>, Arc<AtomicBool>)>,
    pending: Option<(String, String)>,
    group_open: bool,
    settings_draft: Option<(AiConfig, bool)>,
    status: String,
}

impl Assistant {
    pub fn new() -> Self {
        Self::with_config(AiConfig::load())
    }

    fn with_config(config: AiConfig) -> Self {
        let settings_draft = (!config.is_local() && config.effective_key().is_empty())
            .then(|| (config.clone(), false));
        Self {
            config,
            turns: Vec::new(),
            log: Vec::new(),
            input: String::new(),
            running: None,
            pending: None,
            group_open: false,
            settings_draft,
            status: String::new(),
        }
    }

    fn open_settings(&mut self) {
        self.settings_draft = Some((self.config.clone(), !self.config.api_key.is_empty()));
        self.status.clear();
    }

    fn close_settings(&mut self) {
        self.settings_draft = None;
        self.status.clear();
    }

    fn send(&mut self) {
        let text = self.input.trim().to_owned();
        if text.is_empty() || self.running.is_some() {
            return;
        }
        self.input.clear();
        self.log.push(Entry::User(text.clone()));
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cfg = self.config.clone();
        let history = self.turns.clone();
        let stop = Arc::clone(&cancel);
        let msg = text.clone();
        std::thread::spawn(move || ai::run(&cfg, &history, &msg, &tx, &stop));
        self.running = Some((rx, cancel));
        self.pending = Some((text, String::new()));
    }

    fn finish(&mut self, session: &mut Session) {
        if self.group_open {
            session.end_group();
            self.group_open = false;
        }
        if let Some((user, reply)) = self.pending.take() {
            self.turns.push(Turn {
                user: true,
                text: user,
            });
            if !reply.is_empty() {
                self.turns.push(Turn {
                    user: false,
                    text: reply,
                });
            }
        }
        self.running = None;
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let Some((draft, remember_key)) = self.settings_draft.as_mut() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        ui.label(egui::RichText::new("Provider").strong());
        let current = draft.provider.clone();
        egui::ComboBox::from_id_salt("ai-provider")
            .selected_text(&current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for p in PRESETS {
                    if ui.selectable_label(p.name == current, p.name).clicked() {
                        if draft.provider != p.name {
                            *remember_key = false;
                        }
                        draft.use_preset(p);
                    }
                }
            });
        ui.add_space(6.0);
        ui.label("Endpoint");
        ui.add(egui::TextEdit::singleline(&mut draft.base_url).desired_width(f32::INFINITY));
        ui.label("Model");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut draft.model)
                    .desired_width(ui.available_width() - 64.0),
            );
            let models = PRESETS
                .iter()
                .find(|p| p.name == draft.provider)
                .map(|p| p.models)
                .unwrap_or(&[]);
            ui.menu_button("Models", |ui| {
                for m in models {
                    if ui.button(*m).clicked() {
                        draft.model = (*m).to_owned();
                        ui.close();
                    }
                }
            });
        });
        ui.label("API key");
        ui.add(
            egui::TextEdit::singleline(&mut draft.api_key)
                .password(true)
                .hint_text("Paste a provider API key")
                .desired_width(f32::INFINITY),
        );
        if let Some(preset) = PRESETS.iter().find(|p| p.name == draft.provider) {
            if !preset.key_env.is_empty() {
                ui.label(
                    egui::RichText::new(format!("Leave empty to use {}.", preset.key_env))
                        .small()
                        .color(egui::Color32::from_gray(120)),
                );
            }
        }
        ui.checkbox(remember_key, "Remember the key on this computer");
        ui.label(
            egui::RichText::new("Otherwise the pasted key is used only until Deesainer closes.")
                .small()
                .color(egui::Color32::from_gray(120)),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
        if !self.status.is_empty() {
            ui.label(egui::RichText::new(&self.status).small());
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "Anthropic uses its Messages API; every other provider uses the OpenAI-compatible chat API. The assistant edits the document with the same tools agents get over MCP.",
            )
            .small()
            .color(egui::Color32::from_gray(120)),
        );
        if cancel {
            self.close_settings();
        } else if save {
            if let Some((mut next, keep_key)) = self.settings_draft.clone() {
                next.api_key = next.api_key.trim().to_owned();
                match next.save(keep_key) {
                    Ok(()) => {
                        self.config = next;
                        self.settings_draft = None;
                        self.status = "Settings saved.".into();
                    }
                    Err(e) => self.status = format!("Could not save settings: {e}"),
                }
            }
        }
    }
}

fn short(args: &Value) -> String {
    if let Some(c) = args.get("command").and_then(Value::as_str) {
        return c.to_owned();
    }
    if let Some(steps) = args.get("steps").and_then(Value::as_array) {
        return format!("{} steps", steps.len());
    }
    String::new()
}

impl Extension for Assistant {
    fn name(&self) -> &str {
        "AI assistant"
    }

    fn sidebar_title(&self) -> Option<&str> {
        Some("AI")
    }

    fn poll(&mut self, view: &mut ExtensionView<'_>) -> bool {
        let Some((rx, _)) = &self.running else {
            return false;
        };
        let mut changed = false;
        let mut done = false;
        while let Ok(ev) = rx.try_recv() {
            changed = true;
            match ev {
                AgentEvent::Text(t) => {
                    if let Some((_, reply)) = &mut self.pending {
                        if !reply.is_empty() {
                            reply.push('\n');
                        }
                        reply.push_str(&t);
                    }
                    self.log.push(Entry::Assistant(t));
                }
                AgentEvent::Tool { name, args, reply } => {
                    if !self.group_open {
                        view.session.begin_group("Assistant");
                        self.group_open = true;
                    }
                    self.log.push(Entry::Tool(
                        format!("{name} {}", short(&args)).trim().to_owned(),
                    ));
                    let outcome = match McpServer::new(&mut *view.session, "assistant")
                        .call_tool(&name, &args)
                    {
                        Ok(v) => ToolOutcome::from_mcp(&v),
                        Err(e) => ToolOutcome {
                            text: e.message,
                            is_error: true,
                            ..Default::default()
                        },
                    };
                    let _ = reply.send(outcome);
                }
                AgentEvent::Done => done = true,
                AgentEvent::Error(e) => {
                    self.log.push(Entry::Error(e));
                    done = true;
                }
            }
        }
        if done {
            self.finish(view.session);
        }
        changed
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let _ = session;
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("AI").strong().size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Settings").clicked() {
                    if self.settings_draft.is_some() {
                        self.close_settings();
                    } else {
                        self.open_settings();
                    }
                }
                if ui.small_button("New chat").clicked() && self.running.is_none() {
                    self.turns.clear();
                    self.log.clear();
                }
            });
        });
        ui.label(
            egui::RichText::new(format!("{} · {}", self.config.provider, self.config.model))
                .small()
                .color(egui::Color32::from_gray(120)),
        );
        if !self.config.is_local() && self.config.effective_key().is_empty() {
            ui.horizontal(|ui| {
                ui.label("No API key configured");
                if ui.small_button("Add key").clicked() {
                    self.open_settings();
                }
            });
        }
        ui.separator();
        if self.settings_draft.is_some() {
            self.settings_ui(ui);
            return;
        }
        let input_h = 96.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - input_h)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.log.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("What's next?").strong().size(14.0));
                        ui.label(
                            egui::RichText::new("Ask for changes to the open file: \"make the button blue\", \"add a card with a title and two lines\", \"align the selection\".")
                                .color(egui::Color32::from_gray(110)),
                        );
                    });
                }
                for e in &self.log {
                    match e {
                        Entry::User(t) => {
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(242, 242, 242))
                                .corner_radius(10)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.add(egui::Label::new(t).wrap());
                                });
                        }
                        Entry::Assistant(t) => {
                            ui.add(egui::Label::new(t).wrap());
                        }
                        Entry::Tool(t) => {
                            ui.label(egui::RichText::new(format!("▸ {t}")).small().color(egui::Color32::from_gray(130)));
                        }
                        Entry::Error(t) => {
                            ui.colored_label(egui::Color32::from_rgb(220, 60, 60), t);
                        }
                    }
                    ui.add_space(6.0);
                }
                if self.running.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Working…").color(egui::Color32::from_gray(120)));
                    });
                }
            });
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(225)))
            .corner_radius(10)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .hint_text("Ask for changes")
                        .desired_rows(2)
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY),
                );
                let enter = r.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                if enter {
                    while self.input.ends_with('\n') {
                        self.input.pop();
                    }
                }
                ui.horizontal(|ui| {
                    if !self.status.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.status)
                                .small()
                                .color(egui::Color32::from_gray(120)),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some((_, cancel)) = &self.running {
                            if ui.button("Stop").clicked() {
                                cancel.store(true, Ordering::Relaxed);
                            }
                        } else {
                            let send = egui::Button::new(
                                egui::RichText::new("↑").color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(13, 153, 255))
                            .corner_radius(12);
                            if ui
                                .add_enabled(!self.input.trim().is_empty(), send)
                                .clicked()
                                || enter
                            {
                                self.send();
                            }
                        }
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_settings_keeps_active_provider_and_key() {
        let original = AiConfig {
            provider: "Ollama (local)".into(),
            protocol: PRESETS[3].protocol,
            base_url: "http://localhost:11434/v1".into(),
            api_key: "private-example-key".into(),
            ..AiConfig::default()
        };
        let mut assistant = Assistant::with_config(original.clone());
        assistant.open_settings();
        if let Some((draft, _)) = assistant.settings_draft.as_mut() {
            draft.use_preset(&PRESETS[1]);
        }
        assistant.close_settings();
        assert_eq!(assistant.config, original);
        assert!(assistant.settings_draft.is_none());
    }
}
