use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use deesainer_automation::ai::{
    self, AgentEvent, AiConfig, ChatHistory, Reasoning, RunMode, SavedEntry, Stream, ToolOutcome,
    Turn, PRESETS,
};
use deesainer_automation::McpServer;
use deesainer_engine::Session;
use deesainer_ui::{Extension, ExtensionView};
use serde_json::Value;

enum Entry {
    User(String),
    Assistant(String),
    Plan(String),
    Tool {
        label: String,
        args: String,
        result: String,
        failed: bool,
    },
    Request {
        step: u32,
        input: u64,
        output: u64,
        millis: u64,
        stop: String,
    },
    Thinking(String),
    Error(String),
}

#[derive(Default)]
struct Live {
    text: String,
    thinking: String,
    tool: Option<(String, String)>,
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
    live: Live,
    tokens: (u64, u64),
    step: u32,
    debug_open: bool,
    plan: bool,
    mode: RunMode,
    plan_ready: bool,
    prompts: Vec<String>,
    recall: Option<usize>,
}

fn edits_document(session: &Session, name: &str, args: &Value) -> bool {
    let command = match name {
        "command_batch" => return true,
        "command_run" => args.get("command").and_then(Value::as_str).unwrap_or(""),
        other if other.contains('.') => other,
        _ => return false,
    };
    session
        .registry()
        .get(command)
        .is_none_or(|spec| !spec.read_only)
}

const RED: egui::Color32 = egui::Color32::from_rgb(220, 60, 60);
const BLUE: egui::Color32 = egui::Color32::from_rgb(90, 120, 200);

impl Assistant {
    pub fn new() -> Self {
        let mut assistant = Self::with_config(AiConfig::load());
        assistant.restore(ChatHistory::load());
        assistant
    }

    fn restore(&mut self, saved: ChatHistory) {
        self.prompts = saved.prompts;
        self.turns = saved.turns;
        self.log = saved
            .log
            .into_iter()
            .map(|e| match e {
                SavedEntry::User { text } => Entry::User(text),
                SavedEntry::Assistant { text } => Entry::Assistant(text),
                SavedEntry::Plan { text } => Entry::Plan(text),
                SavedEntry::Tool { label, failed } => Entry::Tool {
                    label,
                    args: "(not kept between runs)".into(),
                    result: String::new(),
                    failed,
                },
                SavedEntry::Error { text } => Entry::Error(text),
            })
            .collect();
    }

    fn history(&self) -> ChatHistory {
        ChatHistory {
            prompts: self.prompts.clone(),
            turns: self.turns.clone(),
            log: self
                .log
                .iter()
                .filter_map(|e| match e {
                    Entry::User(t) => Some(SavedEntry::User { text: t.clone() }),
                    Entry::Assistant(t) => Some(SavedEntry::Assistant { text: t.clone() }),
                    Entry::Plan(t) => Some(SavedEntry::Plan { text: t.clone() }),
                    Entry::Tool { label, failed, .. } => Some(SavedEntry::Tool {
                        label: label.clone(),
                        failed: *failed,
                    }),
                    Entry::Error(t) => Some(SavedEntry::Error { text: t.clone() }),
                    Entry::Request { .. } | Entry::Thinking(_) => None,
                })
                .collect(),
        }
    }

    fn save_history(&mut self) {
        if let Err(e) = self.history().save() {
            self.status = format!("Could not save the chat history: {e}");
        }
    }

    fn recall_prompt(&mut self, back: bool) {
        let len = self.prompts.len();
        let next = match (self.recall, back) {
            (None, true) if len > 0 => Some(len - 1),
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < len => Some(i + 1),
            (Some(_), false) => None,
            _ => return,
        };
        self.recall = next;
        self.input = next
            .and_then(|i| self.prompts.get(i).cloned())
            .unwrap_or_default();
    }

    fn with_config(config: AiConfig) -> Self {
        let settings_draft = (!config.is_local() && config.effective_key().is_empty())
            .then(|| (config.clone(), false));
        let debug_open = config.debug_window;
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
            live: Live::default(),
            tokens: (0, 0),
            step: 0,
            debug_open,
            plan: false,
            mode: RunMode::Auto,
            plan_ready: false,
            prompts: Vec::new(),
            recall: None,
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
        self.recall = None;
        let mut saved = ChatHistory {
            prompts: std::mem::take(&mut self.prompts),
            ..ChatHistory::default()
        };
        saved.remember_prompt(&text);
        self.prompts = saved.prompts;
        let mode = if self.plan {
            RunMode::Plan
        } else {
            RunMode::Auto
        };
        self.start(text, mode);
        self.save_history();
    }

    fn execute_plan(&mut self) {
        if self.running.is_some() || !self.plan_ready {
            return;
        }
        self.plan = false;
        self.start("Execute the plan.".to_owned(), RunMode::ExecutePlan);
    }

    fn start(&mut self, text: String, mode: RunMode) {
        self.log.push(Entry::User(match mode {
            RunMode::Plan => format!("[Plan] {text}"),
            _ => text.clone(),
        }));
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cfg = self.config.clone();
        let history = self.turns.clone();
        let stop = Arc::clone(&cancel);
        let msg = text.clone();
        std::thread::spawn(move || ai::run(&cfg, mode, &history, &msg, &tx, &stop));
        self.running = Some((rx, cancel));
        self.pending = Some((text, String::new()));
        self.live = Live::default();
        self.step = 0;
        self.mode = mode;
        self.plan_ready = false;
    }

    fn finish(&mut self, session: &mut Session) {
        if self.group_open {
            session.end_group();
            self.group_open = false;
        }
        if let Some((user, reply)) = self.pending.take() {
            self.plan_ready = self.mode == RunMode::Plan
                && !reply.trim().is_empty()
                && !matches!(self.log.last(), Some(Entry::Error(_)));
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
        self.live = Live::default();
        self.save_history();
    }

    fn live_ui(&self, ui: &mut egui::Ui, full: bool) {
        let gray = egui::Color32::from_gray(120);
        let live = &self.live;
        if !live.thinking.is_empty() && live.text.is_empty() && live.tool.is_none() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new(format!("Thinking… {}", size(live.thinking.len())))
                        .color(gray),
                );
            });
            if full {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(tail(&live.thinking, 1500))
                            .small()
                            .italics()
                            .color(gray),
                    )
                    .wrap(),
                );
            }
            return;
        }
        if !live.text.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&live.text).color(egui::Color32::from_gray(90)),
                )
                .wrap(),
            );
            ui.add_space(4.0);
        }
        match &live.tool {
            Some((name, input)) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new(format!("Writing {name}… {}", size(input.len())))
                            .color(gray),
                    );
                });
                if full && !input.is_empty() {
                    egui::Frame::new()
                        .fill(egui::Color32::from_gray(246))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    mono(tail(input, 2000)).color(egui::Color32::from_gray(70)),
                                )
                                .wrap(),
                            );
                        });
                }
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let what = if live.text.is_empty() {
                        "Waiting for the model…"
                    } else {
                        "Writing…"
                    };
                    ui.label(egui::RichText::new(what).color(gray));
                });
            }
        }
    }

    fn log_text(&self, compact: bool) -> String {
        let cut = |s: &str, max: usize| if compact { clip(s, max) } else { s.to_owned() };
        let mut out = format!(
            "Deesainer AI debug log\nprovider: {} · model: {} · max steps: {} · vision: {}\ntokens: {} in · {} out\n",
            self.config.provider,
            self.config.model,
            self.config.max_steps,
            self.config.vision,
            self.tokens.0,
            self.tokens.1
        );
        for e in &self.log {
            out.push('\n');
            match e {
                Entry::User(t) => out.push_str(&format!("USER: {t}\n")),
                Entry::Assistant(t) => out.push_str(&format!("ASSISTANT: {t}\n")),
                Entry::Plan(t) => out.push_str(&format!("PLAN: {t}\n")),
                Entry::Request {
                    step,
                    input,
                    output,
                    millis,
                    stop,
                } => out.push_str(&format!(
                    "REQUEST #{step}: {input} in · {output} out · {:.1} s · stop {stop}\n",
                    *millis as f64 / 1000.0
                )),
                Entry::Thinking(t) => out.push_str(&format!("REASONING:\n{}\n", cut(t, 1500))),
                Entry::Tool {
                    label,
                    args,
                    result,
                    failed,
                } => out.push_str(&format!(
                    "TOOL {label}{}\nparams:\n{}\nresult:\n{}\n",
                    if *failed { " (FAILED)" } else { "" },
                    cut(args, 1200),
                    cut(result, if *failed { 1500 } else { 600 })
                )),
                Entry::Error(t) => out.push_str(&format!("ERROR: {t}\n")),
            }
        }
        out
    }

    fn debug_ui(&self, ui: &mut egui::Ui) {
        let requests = self
            .log
            .iter()
            .filter(|e| matches!(e, Entry::Request { .. }))
            .count();
        let tools = self
            .log
            .iter()
            .filter(|e| matches!(e, Entry::Tool { .. }))
            .count();
        let failures = self
            .log
            .iter()
            .filter(|e| matches!(e, Entry::Tool { failed: true, .. } | Entry::Error(_)))
            .count();
        egui::Sides::new().shrink_left().show(
            ui,
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!(
                            "{} · {} · {requests} requests · {tools} tool calls · {failures} errors · {} in · {} out",
                            self.config.provider,
                            self.config.model,
                            kilo(self.tokens.0),
                            kilo(self.tokens.1)
                        ))
                        .small(),
                    )
                    .truncate(),
                );
            },
            |ui| {
                if ui
                    .small_button("Copy log")
                    .on_hover_text("Long parameters, results and reasoning shortened, to paste into a chat")
                    .clicked()
                {
                    ui.ctx().copy_text(self.log_text(true));
                }
                if ui
                    .small_button("Full")
                    .on_hover_text("Copy everything, including long parameters, results and reasoning")
                    .clicked()
                {
                    ui.ctx().copy_text(self.log_text(false));
                }
            },
        );
        ui.separator();
        let gray = egui::Color32::from_gray(130);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                for (i, e) in self.log.iter().enumerate() {
                    match e {
                        Entry::User(t) => {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("You").small().strong());
                            copy_menu(ui.add(egui::Label::new(t).wrap().selectable(true)), t);
                        }
                        Entry::Assistant(t) | Entry::Plan(t) => {
                            let who = if matches!(e, Entry::Plan(_)) {
                                "Plan"
                            } else {
                                "Assistant"
                            };
                            ui.label(egui::RichText::new(who).small().strong());
                            copy_menu(ui.add(egui::Label::new(t).wrap().selectable(true)), t);
                        }
                        Entry::Request {
                            step,
                            input,
                            output,
                            millis,
                            stop,
                        } => {
                            ui.add_space(4.0);
                            ui.label(
                                mono(&format!(
                                    "#{step}  {} in · {} out · {:.1} s · {}",
                                    kilo(*input),
                                    kilo(*output),
                                    *millis as f64 / 1000.0,
                                    if stop.is_empty() { "?" } else { stop }
                                ))
                                .color(BLUE),
                            );
                        }
                        Entry::Thinking(t) => {
                            egui::CollapsingHeader::new(
                                egui::RichText::new(format!(
                                    "Reasoning ({} characters)",
                                    t.chars().count()
                                ))
                                .small()
                                .italics()
                                .color(gray),
                            )
                            .id_salt(("ai-debug-thinking", i))
                            .show(ui, |ui| {
                                copy_menu(
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(t).small().italics().color(gray),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    ),
                                    t,
                                );
                            });
                        }
                        Entry::Tool {
                            label,
                            args,
                            result,
                            failed,
                        } => {
                            let color = if *failed { RED } else { gray };
                            egui::CollapsingHeader::new(
                                egui::RichText::new(label).small().color(color),
                            )
                            .id_salt(("ai-debug-tool", i))
                            .default_open(*failed)
                            .show(ui, |ui| {
                                section_title(ui, "Parameters", args);
                                egui::ScrollArea::vertical()
                                    .id_salt(("ai-debug-args", i))
                                    .max_height(260.0)
                                    .show(ui, |ui| {
                                        copy_menu(
                                            ui.add(
                                                egui::Label::new(mono(args))
                                                    .wrap()
                                                    .selectable(true),
                                            ),
                                            args,
                                        );
                                    });
                                section_title(ui, "Result", result);
                                egui::ScrollArea::vertical()
                                    .id_salt(("ai-debug-result", i))
                                    .max_height(200.0)
                                    .show(ui, |ui| {
                                        copy_menu(
                                            ui.add(
                                                egui::Label::new(mono(result).color(color))
                                                    .wrap()
                                                    .selectable(true),
                                            ),
                                            result,
                                        );
                                    });
                            });
                        }
                        Entry::Error(t) => {
                            ui.colored_label(RED, t);
                        }
                    }
                }
                if self.running.is_some() {
                    ui.add_space(6.0);
                    self.live_ui(ui, true);
                }
            });
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
        ui.horizontal(|ui| {
            ui.label("Max steps per message");
            ui.add(egui::DragValue::new(&mut draft.max_steps).range(1..=500))
                .on_hover_text("Model requests the assistant may make for one message before it stops. Large designs need more.");
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
        ui.label("Reasoning");
        let reasoning_label = |r: Reasoning| match r {
            Reasoning::Default => "Model default",
            Reasoning::PlanFirst => "Plan first, then act quickly",
            Reasoning::Off => "Off (except Plan mode)",
        };
        egui::ComboBox::from_id_salt("ai-reasoning")
            .selected_text(reasoning_label(draft.reasoning))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for r in [Reasoning::Default, Reasoning::PlanFirst, Reasoning::Off] {
                    ui.selectable_value(&mut draft.reasoning, r, reasoning_label(r));
                }
            });
        ui.label(
            egui::RichText::new("For reasoning models on the Anthropic protocol (e.g. DeepSeek): long reasoning on every step makes large edits slow. \"Plan first\" reasons on a message's first request and in Plan mode only.")
                .small()
                .color(egui::Color32::from_gray(120)),
        );
        ui.add_space(6.0);
        ui.checkbox(&mut draft.vision, "Model can see images");
        ui.label(
            egui::RichText::new("Turn off for text-only models (e.g. DeepSeek): render previews are then not offered and the model checks its work from the document data.")
                .small()
                .color(egui::Color32::from_gray(120)),
        );
        ui.add_space(6.0);
        ui.checkbox(&mut draft.debug_window, "Debug window");
        ui.label(
            egui::RichText::new("A separate window with every model request (tokens, time, why it stopped), the model's reasoning, each tool call with its parameters and result, and the reply as it streams in.")
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
                        self.debug_open = next.debug_window;
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

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}\n…({} more characters)", &s[..i], s[i..].chars().count()),
        None => s.to_owned(),
    }
}

fn tail(s: &str, max: usize) -> &str {
    let n = s.chars().count();
    match s.char_indices().nth(n.saturating_sub(max)) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}

fn kilo(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn size(bytes: usize) -> String {
    if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn copy_menu(resp: egui::Response, text: &str) {
    resp.context_menu(|ui| {
        if ui.button("Copy").clicked() {
            ui.ctx().copy_text(text.to_owned());
            ui.close();
        }
    });
}

fn section_title(ui: &mut egui::Ui, title: &str, content: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).small().strong());
        if ui.small_button("Copy").clicked() {
            ui.ctx().copy_text(content.to_owned());
        }
    });
}

fn mono(t: &str) -> egui::RichText {
    egui::RichText::new(t).monospace().size(10.5)
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
                    self.log.push(if self.mode == RunMode::Plan {
                        Entry::Plan(t)
                    } else {
                        Entry::Assistant(t)
                    });
                }
                AgentEvent::Stream(s) => match s {
                    Stream::Text(t) => self.live.text.push_str(&t),
                    Stream::Thinking(t) => self.live.thinking.push_str(&t),
                    Stream::ToolStart(name) => self.live.tool = Some((name, String::new())),
                    Stream::ToolInput(t) => {
                        if let Some((_, input)) = &mut self.live.tool {
                            input.push_str(&t);
                        }
                    }
                },
                AgentEvent::Usage {
                    input,
                    output,
                    millis,
                    stop,
                } => {
                    self.step += 1;
                    self.tokens.0 += input;
                    self.tokens.1 += output;
                    self.log.push(Entry::Request {
                        step: self.step,
                        input,
                        output,
                        millis,
                        stop,
                    });
                    let live = std::mem::take(&mut self.live);
                    if !live.thinking.trim().is_empty() {
                        self.log.push(Entry::Thinking(live.thinking));
                    }
                }
                AgentEvent::Tool { name, args, reply } => {
                    if !self.group_open {
                        view.session.begin_group("Assistant");
                        self.group_open = true;
                    }
                    let outcome = if self.mode == RunMode::Plan
                        && edits_document(view.session, &name, &args)
                    {
                        ToolOutcome {
                            text: "Plan mode: this would change the document, so it was not run. Finish the plan; the user presses Execute to carry it out.".into(),
                            is_error: true,
                            ..Default::default()
                        }
                    } else {
                        match McpServer::new(&mut *view.session, "assistant")
                            .call_tool(&name, &args)
                        {
                            Ok(v) => ToolOutcome::from_mcp(&v),
                            Err(e) => ToolOutcome {
                                text: e.message,
                                is_error: true,
                                ..Default::default()
                            },
                        }
                    };
                    let mut result = clip(&outcome.text, 4000);
                    if !outcome.images.is_empty() {
                        result.push_str(&format!("\n({} image(s))", outcome.images.len()));
                    }
                    self.log.push(Entry::Tool {
                        label: format!("{name} {}", short(&args)).trim().to_owned(),
                        args: clip(
                            &serde_json::to_string_pretty(&args).unwrap_or_default(),
                            20_000,
                        ),
                        result,
                        failed: outcome.is_error,
                    });
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

    fn overlay(&mut self, ctx: &egui::Context) {
        if !self.debug_open {
            return;
        }
        let id = egui::ViewportId::from_hash_of("deesainer-ai-debug");
        let builder = egui::ViewportBuilder::default()
            .with_title("Deesainer AI debug")
            .with_inner_size([620.0, 820.0]);
        ctx.show_viewport_immediate(id, builder, |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| self.debug_ui(ui));
            if ctx.input(|i| i.viewport().close_requested()) {
                self.debug_open = false;
            }
        });
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
                    self.tokens = (0, 0);
                    self.plan_ready = false;
                    self.save_history();
                }
                if self.config.debug_window
                    && ui
                        .small_button("Debug")
                        .on_hover_text("Open the AI debug window")
                        .clicked()
                {
                    self.debug_open = true;
                }
            });
        });
        let model = format!("{} · {}", self.config.provider, self.config.model);
        let tokens = self.tokens;
        egui::Sides::new().shrink_left().show(
            ui,
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&model)
                            .small()
                            .color(egui::Color32::from_gray(120)),
                    )
                    .truncate(),
                )
                .on_hover_text(&model);
            },
            |ui| {
                if tokens != (0, 0) {
                    ui.label(
                        egui::RichText::new(format!("{} in · {} out", kilo(tokens.0), kilo(tokens.1)))
                            .small()
                            .color(egui::Color32::from_gray(120)),
                    )
                    .on_hover_text("Tokens used by this chat (input counts the whole context of every request)");
                }
            },
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
        let mut execute = false;
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
                let gray = egui::Color32::from_gray(130);
                ui.spacing_mut().item_spacing.y = 2.0;
                for e in &self.log {
                    match e {
                        Entry::User(t) => {
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(242, 242, 242))
                                .corner_radius(10)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    copy_menu(ui.add(egui::Label::new(t).wrap().selectable(true)), t);
                                });
                        }
                        Entry::Assistant(t) => {
                            let r = ui.scope(|ui| crate::markdown::show(ui, t)).response;
                            copy_menu(r.interact(egui::Sense::click()), t);
                        }
                        Entry::Plan(t) => {
                            let r = egui::Frame::new()
                                .fill(egui::Color32::from_rgb(244, 249, 255))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(205, 226, 252)))
                                .corner_radius(10)
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new("PLAN")
                                            .small()
                                            .strong()
                                            .color(egui::Color32::from_rgb(13, 120, 220)),
                                    );
                                    crate::markdown::show(ui, t);
                                })
                                .response;
                            copy_menu(r.interact(egui::Sense::click()), t);
                        }
                        Entry::Tool { label, failed, .. } => {
                            let color = if *failed { RED } else { gray };
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                let h = ui.text_style_height(&egui::TextStyle::Small);
                                crate::markdown::dot(ui, color, h);
                                ui.label(egui::RichText::new(label).small().color(color));
                            });
                            continue;
                        }
                        Entry::Request { .. } | Entry::Thinking(_) => continue,
                        Entry::Error(t) => {
                            ui.colored_label(RED, t);
                        }
                    }
                    ui.add_space(6.0);
                }
                if self.running.is_some() {
                    self.live_ui(ui, false);
                }
                if self.plan_ready && self.running.is_none() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let run = egui::Button::new(
                            egui::RichText::new("Execute plan").color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(13, 153, 255))
                        .corner_radius(8);
                        execute = ui.add(run).on_hover_text("Carry out the plan above").clicked();
                        ui.label(
                            egui::RichText::new("or reply to change it")
                                .small()
                                .color(egui::Color32::from_gray(120)),
                        );
                    });
                }
            });
        if execute {
            self.execute_plan();
        }
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(225)))
            .corner_radius(10)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .hint_text(if self.plan {
                            "Describe what to plan (nothing changes yet)"
                        } else {
                            "Ask for changes"
                        })
                        .desired_rows(2)
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY),
                );
                let enter = r.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                let browsing = self.input.is_empty()
                    || self
                        .recall
                        .is_some_and(|i| self.prompts.get(i) == Some(&self.input));
                if r.has_focus() && browsing {
                    let (up, down) = ui.input(|i| {
                        (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown))
                    });
                    if up || down {
                        self.recall_prompt(up);
                    }
                } else if r.changed() {
                    self.recall = None;
                }
                if enter {
                    while self.input.ends_with('\n') {
                        self.input.pop();
                    }
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.plan, false, egui::RichText::new("Auto").small())
                        .on_hover_text("The assistant edits the document directly");
                    ui.selectable_value(&mut self.plan, true, egui::RichText::new("Plan").small())
                        .on_hover_text("The assistant only looks and proposes a plan; Execute plan carries it out");
                    if !self.prompts.is_empty() {
                        let mut picked = None;
                        ui.menu_button(egui::RichText::new("History").small(), |ui| {
                            for p in self.prompts.iter().rev().take(25) {
                                let line = p.lines().next().unwrap_or("");
                                let short: String = line.chars().take(70).collect();
                                let short = if short.len() < p.len() { format!("{short}…") } else { short };
                                if ui.button(short).on_hover_text(p).clicked() {
                                    picked = Some(p.clone());
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text("Earlier prompts (Up and Down in the empty box step through them too)");
                        if let Some(p) = picked {
                            self.input = p;
                            self.recall = None;
                        }
                    }
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

    #[test]
    fn history_restores_the_chat_and_recalls_prompts() {
        let mut a = Assistant::with_config(AiConfig::default());
        a.restore(ChatHistory {
            prompts: vec!["first".into(), "second".into()],
            turns: vec![Turn {
                user: true,
                text: "first".into(),
            }],
            log: vec![
                SavedEntry::User {
                    text: "first".into(),
                },
                SavedEntry::Tool {
                    label: "command_batch 3 steps".into(),
                    failed: true,
                },
                SavedEntry::Assistant {
                    text: "Done.".into(),
                },
            ],
        });
        assert_eq!(a.log.len(), 3);
        assert!(matches!(&a.log[1], Entry::Tool { failed: true, .. }));
        a.log.push(Entry::Thinking("long".into()));
        assert_eq!(a.history().log.len(), 3);
        assert_eq!(a.history().turns.len(), 1);
        a.recall_prompt(true);
        assert_eq!(a.input, "second");
        a.recall_prompt(true);
        assert_eq!(a.input, "first");
        a.recall_prompt(true);
        assert_eq!(a.input, "first");
        a.recall_prompt(false);
        assert_eq!(a.input, "second");
        a.recall_prompt(false);
        assert_eq!(a.input, "");
        assert!(a.recall.is_none());
    }

    #[test]
    fn plan_mode_tells_edits_from_reads() {
        let s = Session::new().unwrap();
        let none = serde_json::json!({});
        let run = |c: &str| serde_json::json!({"command": c});
        assert!(!edits_document(&s, "doc_inspect", &none));
        assert!(!edits_document(&s, "command_list", &none));
        assert!(!edits_document(&s, "command_run", &run("color.list")));
        assert!(!edits_document(&s, "command_run", &run("node.inspect")));
        assert!(edits_document(&s, "command_run", &run("node.create.frame")));
        assert!(edits_document(&s, "command_run", &run("no.such.command")));
        assert!(edits_document(&s, "command_batch", &none));
        assert!(!edits_document(&s, "document.tree", &none));
        assert!(edits_document(&s, "page.duplicate", &none));
    }

    #[test]
    fn debug_log_carries_requests_tools_and_errors() {
        let mut assistant = Assistant::with_config(AiConfig {
            debug_window: true,
            ..AiConfig::default()
        });
        assert!(assistant.debug_open);
        assistant.log = vec![
            Entry::User("dark mode".into()),
            Entry::Request {
                step: 1,
                input: 1200,
                output: 80,
                millis: 2500,
                stop: "tool_use".into(),
            },
            Entry::Thinking("duplicate the page first".into()),
            Entry::Tool {
                label: "command_run page.duplicate".into(),
                args: "{\"command\": \"page.duplicate\"}".into(),
                result: "unknown param".into(),
                failed: true,
            },
            Entry::Error("Stopped.".into()),
        ];
        assert!(assistant.log_text(true).len() <= assistant.log_text(false).len());
        let log = assistant.log_text(false);
        for part in [
            "USER: dark mode",
            "REQUEST #1: 1200 in · 80 out · 2.5 s · stop tool_use",
            "REASONING:\nduplicate the page first",
            "TOOL command_run page.duplicate (FAILED)",
            "params:\n{\"command\": \"page.duplicate\"}",
            "ERROR: Stopped.",
        ] {
            assert!(log.contains(part), "missing {part:?} in\n{log}");
        }
    }
}
