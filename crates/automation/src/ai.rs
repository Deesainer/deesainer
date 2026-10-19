use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    #[default]
    Anthropic,
    OpenAi,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiConfig {
    pub provider: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub max_steps: u32,
    pub debug_window: bool,
    pub vision: bool,
    pub reasoning: Reasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Reasoning {
    #[default]
    Default,
    PlanFirst,
    Off,
}

impl Default for AiConfig {
    fn default() -> Self {
        let p = &PRESETS[0];
        Self {
            provider: p.name.to_owned(),
            protocol: p.protocol,
            base_url: p.base_url.to_owned(),
            model: p.models[0].to_owned(),
            api_key: String::new(),
            max_steps: 100,
            debug_window: false,
            vision: true,
            reasoning: Reasoning::Default,
        }
    }
}

pub struct Preset {
    pub name: &'static str,
    pub protocol: Protocol,
    pub base_url: &'static str,
    pub models: &'static [&'static str],
    pub key_env: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Anthropic",
        protocol: Protocol::Anthropic,
        base_url: "https://api.anthropic.com/v1",
        models: &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
        key_env: "ANTHROPIC_API_KEY",
    },
    Preset {
        name: "OpenAI",
        protocol: Protocol::OpenAi,
        base_url: "https://api.openai.com/v1",
        models: &["gpt-4.1", "gpt-4.1-mini"],
        key_env: "OPENAI_API_KEY",
    },
    Preset {
        name: "OpenRouter",
        protocol: Protocol::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        models: &["anthropic/claude-opus-5-5", "openai/gpt-4.1"],
        key_env: "OPENROUTER_API_KEY",
    },
    Preset {
        name: "Ollama (local)",
        protocol: Protocol::OpenAi,
        base_url: "http://localhost:11434/v1",
        models: &["qwen3", "llama3.3"],
        key_env: "",
    },
    Preset {
        name: "LM Studio (local)",
        protocol: Protocol::OpenAi,
        base_url: "http://localhost:1234/v1",
        models: &["local-model"],
        key_env: "",
    },
    Preset {
        name: "Custom (OpenAI-compatible)",
        protocol: Protocol::OpenAi,
        base_url: "https://",
        models: &[""],
        key_env: "",
    },
];

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".deesainer").join("ai.json"))
}

fn read_private(path: &Path) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if !metadata.file_type().is_file() {
            return None;
        }
        if metadata.permissions().mode() & 0o777 != 0o600 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok()?;
        }
    }
    std::fs::read(path).ok()
}

fn write_private(path: &Path, body: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let mut tmp_name = path.as_os_str().to_os_string();
    tmp_name.push(format!(".{suffix}.tmp"));
    let tmp = PathBuf::from(tmp_name);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| -> Result<(), String> {
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(body).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        replace_config_file(&tmp, path, &suffix).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn replace_config_file(tmp: &Path, path: &Path, suffix: &str) -> std::io::Result<()> {
    #[cfg(not(windows))]
    {
        let _ = suffix;
        std::fs::rename(tmp, path)
    }
    #[cfg(windows)]
    {
        match std::fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return std::fs::rename(tmp, path);
            }
            Err(e) => return Err(e),
        }
        let mut backup_name = path.as_os_str().to_os_string();
        backup_name.push(format!(".{suffix}.backup"));
        let backup = PathBuf::from(backup_name);
        std::fs::rename(path, &backup)?;
        match std::fs::rename(tmp, path) {
            Ok(()) => std::fs::remove_file(backup),
            Err(e) => {
                std::fs::rename(&backup, path)?;
                Err(e)
            }
        }
    }
}

impl AiConfig {
    pub fn load() -> Self {
        config_path()
            .and_then(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    fn load_from(path: &Path) -> Option<Self> {
        serde_json::from_slice(&read_private(path)?).ok()
    }

    pub fn save(&self, keep_key: bool) -> Result<(), String> {
        let path = config_path().ok_or("no home directory")?;
        self.save_to(&path, keep_key)
    }

    fn save_to(&self, path: &Path, keep_key: bool) -> Result<(), String> {
        let mut c = self.clone();
        if !keep_key {
            c.api_key.clear();
        } else {
            c.api_key = c.api_key.trim().to_owned();
        }
        let body = serde_json::to_vec_pretty(&c).map_err(|e| e.to_string())?;
        write_private(path, &body)
    }

    pub fn use_preset(&mut self, preset: &Preset) {
        if self.provider != preset.name {
            self.api_key.clear();
        }
        self.provider = preset.name.to_owned();
        self.protocol = preset.protocol;
        self.base_url = preset.base_url.to_owned();
        self.model = preset
            .models
            .first()
            .copied()
            .unwrap_or_default()
            .to_owned();
    }

    pub fn effective_key(&self) -> String {
        if !self.api_key.trim().is_empty() {
            return self.api_key.trim().to_owned();
        }
        PRESETS
            .iter()
            .find(|p| {
                p.name == self.provider
                    && p.base_url.trim_end_matches('/') == self.base_url.trim_end_matches('/')
            })
            .filter(|p| !p.key_env.is_empty())
            .and_then(|p| std::env::var(p.key_env).ok())
            .unwrap_or_default()
    }

    pub fn is_local(&self) -> bool {
        let Ok(uri) = self.base_url.parse::<ureq::http::Uri>() else {
            return false;
        };
        uri.scheme_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("http"))
            && uri.host().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || matches!(host, "127.0.0.1" | "::1" | "[::1]")
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub user: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SavedEntry {
    User { text: String },
    Assistant { text: String },
    Plan { text: String },
    Tool { label: String, failed: bool },
    Error { text: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatHistory {
    pub prompts: Vec<String>,
    pub turns: Vec<Turn>,
    pub log: Vec<SavedEntry>,
}

const MAX_PROMPTS: usize = 200;
const MAX_SAVED_ENTRIES: usize = 400;
const MAX_SAVED_TURNS: usize = 60;

fn history_path() -> Option<PathBuf> {
    config_path().map(|p| p.with_file_name("ai-history.json"))
}

impl ChatHistory {
    pub fn load() -> Self {
        history_path()
            .and_then(|p| read_private(&p))
            .and_then(|body| serde_json::from_slice(&body).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = history_path().ok_or("no home directory")?;
        let body = serde_json::to_vec(&self.trimmed()).map_err(|e| e.to_string())?;
        write_private(&path, &body)
    }

    fn trimmed(&self) -> Self {
        fn last<T: Clone>(items: &[T], max: usize) -> Vec<T> {
            items
                .iter()
                .skip(items.len().saturating_sub(max))
                .cloned()
                .collect()
        }
        Self {
            prompts: last(&self.prompts, MAX_PROMPTS),
            turns: last(&self.turns, MAX_SAVED_TURNS),
            log: last(&self.log, MAX_SAVED_ENTRIES),
        }
    }

    pub fn remember_prompt(&mut self, prompt: &str) {
        let prompt = prompt.trim();
        if prompt.is_empty() || self.prompts.last().is_some_and(|p| p == prompt) {
            return;
        }
        self.prompts.push(prompt.to_owned());
        self.prompts = self.trimmed().prompts;
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutcome {
    pub text: String,
    pub images: Vec<(String, String)>,
    pub is_error: bool,
}

impl ToolOutcome {
    pub fn from_mcp(result: &Value) -> Self {
        let mut out = ToolOutcome {
            is_error: result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ..Default::default()
        };
        for block in result
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if !out.text.is_empty() {
                        out.text.push('\n');
                    }
                    out.text
                        .push_str(block.get("text").and_then(Value::as_str).unwrap_or(""));
                }
                Some("image") => out.images.push((
                    block
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png")
                        .to_owned(),
                    block
                        .get("data")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                )),
                _ => {}
            }
        }
        if out.text.len() > 60_000 {
            out.text.truncate(60_000);
            out.text.push_str("\n…(truncated)");
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stream {
    Text(String),
    Thinking(String),
    ToolStart(String),
    ToolInput(String),
}

pub enum AgentEvent {
    Text(String),
    Stream(Stream),
    Usage {
        input: u64,
        output: u64,
        millis: u64,
        stop: String,
    },
    Tool {
        name: String,
        args: Value,
        reply: Sender<ToolOutcome>,
    },
    Done,
    Error(String),
}

const SYSTEM: &str = "You are the design assistant inside Deesainer, an interface design tool (frames, shapes, text, auto layout, components, styles, variables, prototypes). You edit the user's open document through tools. Work like this: call doc_inspect to see pages, layers and the selection; use command_list (with a filter) and describe_command to learn a command's parameters; make changes with command_run or, for several steps, command_batch (one undo step); after visual changes call render_preview to check the result. The command index at the end of this prompt names every command with its params, so do not call command_list to find names; use describe_command (or command_list with a filter) only for the docs of a command whose params are unclear, and do not repeat calls you have already made. text.fonts lists the installed fonts. In command_batch a param \"$N\" is the id step N returned: create a frame, its children (parent \"$0\") and their alignment or layout in one batch instead of one call each. Coordinates are in points, y grows downward, and a child's x/y are relative to its parent. Text without a width sizes itself to its glyphs, so you cannot know its box in advance: never centre a label, glyph or icon inside a shape by guessing x/y. Create it in the same parent as the shape, starting at the shape's top-left so it lies within the shape (align uses the bounds of all ids, so text sticking out would move the shape too), then run align.horizontalCenter and align.verticalCenter with ids [shape, text] (both in one command_batch), or put the content in an auto layout frame with centred alignment. Read in bulk: doc_inspect (or document.tree for another page) gives every layer's id, type and rect at once; do not walk layers one by one with node.inspect or inspect.spec, keep those for the few layers whose styling you really need. Put independent tool calls in the same reply. Rectangles and frames take cornerRadius when created. For a theme or recolour (dark mode, rebrand): duplicate the page with page.duplicate {id}, call color.list {ids: [newPage]}, then one color.replace {ids: [newPage], fill: {\"#F8FAFC\": \"#0F172A\", \"#FFFFFF\": \"#1E293B\"}, text: {\"#0F172A\": \"#F8FAFC\"}, stroke: {...}} covering every colour (backgrounds, surfaces, text, borders, accents). Use the fill, text and stroke maps rather than one map when a colour plays two roles, e.g. white cards and white text on accent buttons. Decide the map in a few lines and apply it; then call color.list again and fix the few leftovers with node.setProps on the layer ids it reports. If the document already holds what the user asks for (a page with that name and content), say so in one line and ask whether to redo it instead of re-checking it layer by layer. Prefer editing the selection when the user refers to 'this' or 'selected'. Keep replies short and say what you changed. Replies are shown as Markdown (headings, lists, **bold**); write colours in backticks, like `#0B1F3A`, so they show as swatches.";

const NO_VISION: &str = " This model cannot see images, so there is no render_preview: check your work with color.list, doc_inspect and node.inspect on the few layers that matter.";

const PLAN_MODE: &str = " PLAN MODE: you cannot change the document in this message; edits are refused. Look at what you need with read-only tools (doc_inspect, color.list, inspect.spec, node.inspect, command_list), then reply with a concise numbered plan: what you will create or change and where (pages, frames, sizes), colours and type, and the batches you will run. Do not ask whether to proceed: the user reviews the plan and presses Execute.";
const EXECUTE_PLAN: &str = " The plan in your previous reply is approved: carry it out now without re-planning. Prefer few large command_batch calls using \"$N\" references.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Auto,
    Plan,
    ExecutePlan,
}

fn system_prompt(cfg: &AiConfig, mode: RunMode) -> String {
    let mut s = SYSTEM.replace(
        "after visual changes call render_preview to check the result",
        if cfg.vision {
            "after visual changes call render_preview to check the result"
        } else {
            "after changes check them with color.list or doc_inspect"
        },
    );
    if !cfg.vision {
        s.push_str(NO_VISION);
    }
    match mode {
        RunMode::Auto => {}
        RunMode::Plan => s.push_str(PLAN_MODE),
        RunMode::ExecutePlan => s.push_str(EXECUTE_PLAN),
    }
    let index = crate::command_index();
    if !index.is_empty() {
        s.push_str("\n\nEvery command, as id(params) (? = optional; run them with command_run or command_batch):\n");
        s.push_str(index);
    }
    s
}

fn user_message(message: &str, mode: RunMode) -> String {
    match mode {
        RunMode::Plan => format!("[Plan mode: read only, reply with a plan]\n{message}"),
        RunMode::Auto | RunMode::ExecutePlan => message.to_owned(),
    }
}

fn thinking(cfg: &AiConfig, mode: RunMode, step: u32) -> Option<Value> {
    let off = match (cfg.reasoning, mode) {
        (_, RunMode::Plan) | (Reasoning::Default, _) => false,
        (Reasoning::Off, _) => true,
        (Reasoning::PlanFirst, RunMode::ExecutePlan) => true,
        (Reasoning::PlanFirst, RunMode::Auto) => step > 0,
    };
    off.then(|| json!({ "type": "disabled" }))
}

fn for_model(cfg: &AiConfig, mut out: ToolOutcome) -> ToolOutcome {
    if !cfg.vision && !out.images.is_empty() {
        out.images.clear();
        out.text
            .push_str("\n(Image not sent: this model cannot see images.)");
    }
    out
}

fn agent_tools(cfg: &AiConfig, mode: RunMode) -> Vec<(&'static str, &'static str, Value)> {
    crate::tool_specs()
        .into_iter()
        .filter(|(name, _, _)| cfg.vision || *name != "render_preview")
        .filter(|(name, _, _)| mode != RunMode::Plan || *name != "command_batch")
        .collect()
}

fn agent(cfg: &AiConfig) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(300)))
        .timeout_recv_body(Some(Duration::from_secs(30 * 60)))
        .https_only(!cfg.is_local())
        .http_status_as_error(false)
        .build()
        .into()
}

fn redact_key(message: String, key: &str) -> String {
    if key.is_empty() {
        message
    } else {
        message.replace(key, "[redacted]")
    }
}

fn post(
    cfg: &AiConfig,
    path: &str,
    body: &Value,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(&Value) -> Result<(), String>,
) -> Result<Option<Value>, String> {
    let url = format!("{}/{path}", cfg.base_url.trim_end_matches('/'));
    let key = cfg.effective_key();
    let mut req = agent(cfg)
        .post(&url)
        .header("content-type", "application/json");
    match cfg.protocol {
        Protocol::Anthropic => {
            if key.is_empty() {
                return Err(
                    "No API key: add one in the assistant settings or set ANTHROPIC_API_KEY."
                        .into(),
                );
            }
            req = req
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01");
        }
        Protocol::OpenAi => {
            if !key.is_empty() {
                req = req.header("authorization", &format!("Bearer {key}"));
            }
        }
    }
    let resp = req
        .send(body.to_string().as_bytes())
        .map_err(|e| redact_key(format!("Cannot reach {}: {e}", cfg.base_url), &key))?;
    let status = resp.status().as_u16();
    let streamed = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.contains("text/event-stream"));
    let mut reader = resp.into_body().into_reader().take(256 * 1024 * 1024);
    if streamed && status < 400 {
        read_events(&mut std::io::BufReader::new(reader), cancel, on_event)?;
        return Ok(None);
    }
    let mut raw = Vec::new();
    reader.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&raw)
        .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&raw) }));
    if status >= 400 {
        let msg = v
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| v.get("raw").and_then(Value::as_str))
            .unwrap_or("request failed");
        return Err(redact_key(
            format!("{} replied {status}: {msg}", cfg.provider),
            &key,
        ));
    }
    Ok(Some(v))
}

fn read_events(
    reader: &mut impl std::io::BufRead,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(&Value) -> Result<(), String>,
) -> Result<(), String> {
    let mut data = String::new();
    let mut line = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped.".into());
        }
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let text = line.trim_end_matches(['\r', '\n']);
        if n == 0 || text.is_empty() {
            if data == "[DONE]" {
                return Ok(());
            }
            if !data.is_empty() {
                if let Ok(ev) = serde_json::from_str::<Value>(&data) {
                    on_event(&ev)?;
                }
                data.clear();
            }
            if n == 0 {
                return Ok(());
            }
        } else if let Some(d) = text.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.strip_prefix(' ').unwrap_or(d));
        }
    }
}

fn push_str(block: &mut Value, key: &str, more: &str) {
    if let Some(obj) = block.as_object_mut() {
        let cur = obj
            .entry(key)
            .or_insert_with(|| Value::String(String::new()));
        match cur {
            Value::String(s) => s.push_str(more),
            other => *other = Value::String(more.to_owned()),
        }
    }
}

#[derive(Default)]
struct AnthropicStream {
    blocks: Vec<Value>,
    partial: Vec<String>,
    stop_reason: Option<String>,
    input: u64,
    output: u64,
}

impl AnthropicStream {
    fn feed(&mut self, ev: &Value, out: &mut dyn FnMut(Stream)) -> Result<(), String> {
        let index = ev.get("index").and_then(Value::as_u64).map(|i| i as usize);
        match ev.get("type").and_then(Value::as_str) {
            Some("message_start") => self.usage(ev.pointer("/message/usage")),
            Some("content_block_start") => {
                let block = ev.get("content_block").cloned().unwrap_or(json!({}));
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    out(Stream::ToolStart(name.to_owned()));
                }
                self.blocks.push(block);
                self.partial.push(String::new());
            }
            Some("content_block_delta") => {
                let i = index.unwrap_or(self.blocks.len().saturating_sub(1));
                let d = ev.get("delta").cloned().unwrap_or(json!({}));
                let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
                let (Some(block), Some(partial)) =
                    (self.blocks.get_mut(i), self.partial.get_mut(i))
                else {
                    return Ok(());
                };
                match d.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let t = s("text");
                        push_str(block, "text", &t);
                        out(Stream::Text(t));
                    }
                    Some("thinking_delta") => {
                        let t = s("thinking");
                        push_str(block, "thinking", &t);
                        out(Stream::Thinking(t));
                    }
                    Some("signature_delta") => push_str(block, "signature", &s("signature")),
                    Some("input_json_delta") => {
                        let t = s("partial_json");
                        partial.push_str(&t);
                        out(Stream::ToolInput(t));
                    }
                    _ => {}
                }
            }
            Some("content_block_stop") => {
                let i = index.unwrap_or(self.blocks.len().saturating_sub(1));
                if let (Some(block), Some(partial)) = (self.blocks.get_mut(i), self.partial.get(i))
                {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let input = if partial.trim().is_empty() {
                            json!({})
                        } else {
                            serde_json::from_str(partial).unwrap_or(json!({}))
                        };
                        if let Some(obj) = block.as_object_mut() {
                            obj.insert("input".into(), input);
                        }
                    }
                }
            }
            Some("message_delta") => {
                if let Some(r) = ev.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(r.to_owned());
                }
                self.usage(ev.get("usage"));
            }
            Some("error") => {
                return Err(ev
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("the provider reported an error")
                    .to_owned())
            }
            _ => {}
        }
        Ok(())
    }

    fn usage(&mut self, u: Option<&Value>) {
        let Some(u) = u else { return };
        let n = |k: &str| u.get(k).and_then(Value::as_u64);
        if let Some(i) = n("input_tokens") {
            self.input = i
                + n("cache_creation_input_tokens").unwrap_or(0)
                + n("cache_read_input_tokens").unwrap_or(0);
        }
        if let Some(o) = n("output_tokens") {
            self.output = o;
        }
    }

    fn finish(self) -> Value {
        json!({
            "content": self.blocks,
            "stop_reason": self.stop_reason,
            "usage": { "input_tokens": self.input, "output_tokens": self.output },
        })
    }
}

#[derive(Default)]
struct OpenAiStream {
    text: String,
    calls: Vec<(String, String, String)>,
    finish_reason: Option<String>,
    usage: Option<Value>,
}

impl OpenAiStream {
    fn feed(&mut self, ev: &Value, out: &mut dyn FnMut(Stream)) -> Result<(), String> {
        if let Some(e) = ev.get("error") {
            return Err(e
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("the provider reported an error")
                .to_owned());
        }
        if let Some(u) = ev.get("usage").filter(|u| u.is_object()) {
            self.usage = Some(u.clone());
        }
        if let Some(r) = ev
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
        {
            self.finish_reason = Some(r.to_owned());
        }
        let Some(d) = ev.pointer("/choices/0/delta") else {
            return Ok(());
        };
        if let Some(t) = d.get("content").and_then(Value::as_str) {
            self.text.push_str(t);
            out(Stream::Text(t.to_owned()));
        }
        if let Some(t) = d
            .get("reasoning_content")
            .or_else(|| d.get("reasoning"))
            .and_then(Value::as_str)
        {
            out(Stream::Thinking(t.to_owned()));
        }
        for tc in d
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let i = tc
                .get("index")
                .and_then(Value::as_u64)
                .map_or(self.calls.len().saturating_sub(1), |i| i as usize);
            if i >= 128 {
                continue;
            }
            while self.calls.len() <= i {
                self.calls.push(Default::default());
            }
            let Some(call) = self.calls.get_mut(i) else {
                continue;
            };
            if let Some(id) = tc.get("id").and_then(Value::as_str) {
                call.0 = id.to_owned();
            }
            if let Some(name) = tc.pointer("/function/name").and_then(Value::as_str) {
                if call.1.is_empty() {
                    out(Stream::ToolStart(name.to_owned()));
                }
                call.1.push_str(name);
            }
            if let Some(a) = tc.pointer("/function/arguments").and_then(Value::as_str) {
                call.2.push_str(a);
                out(Stream::ToolInput(a.to_owned()));
            }
        }
        Ok(())
    }

    fn finish(self) -> Value {
        let mut msg = json!({ "role": "assistant", "content": self.text });
        if !self.calls.is_empty() {
            let calls: Vec<Value> = self
                .calls
                .into_iter()
                .map(|(id, name, args)| json!({ "id": id, "type": "function", "function": { "name": name, "arguments": args } }))
                .collect();
            msg["tool_calls"] = Value::Array(calls);
            if self.text.is_empty() {
                msg["content"] = Value::Null;
            }
        }
        json!({
            "choices": [{ "message": msg, "finish_reason": self.finish_reason }],
            "usage": self.usage,
        })
    }
}

fn usage_of(reply: &Value) -> (u64, u64) {
    let n = |k: &str| {
        reply
            .pointer(&format!("/usage/{k}"))
            .and_then(Value::as_u64)
    };
    let input = n("input_tokens")
        .map(|i| {
            i + n("cache_creation_input_tokens").unwrap_or(0)
                + n("cache_read_input_tokens").unwrap_or(0)
        })
        .or_else(|| n("prompt_tokens"))
        .unwrap_or(0);
    let output = n("output_tokens")
        .or_else(|| n("completion_tokens"))
        .unwrap_or(0);
    (input, output)
}

fn request(
    cfg: &AiConfig,
    body: &Value,
    events: &Sender<AgentEvent>,
    cancel: &AtomicBool,
) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let mut emit = |s: Stream| {
        let _ = events.send(AgentEvent::Stream(s));
    };
    let reply = match cfg.protocol {
        Protocol::Anthropic => {
            let mut acc = AnthropicStream::default();
            let direct = post(cfg, "messages", body, cancel, &mut |ev| {
                acc.feed(ev, &mut emit)
            })?;
            direct.unwrap_or_else(|| acc.finish())
        }
        Protocol::OpenAi => {
            let mut acc = OpenAiStream::default();
            let direct = post(cfg, "chat/completions", body, cancel, &mut |ev| {
                acc.feed(ev, &mut emit)
            })?;
            direct.unwrap_or_else(|| acc.finish())
        }
    };
    let (input, output) = usage_of(&reply);
    let stop = reply
        .get("stop_reason")
        .or_else(|| reply.pointer("/choices/0/finish_reason"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let _ = events.send(AgentEvent::Usage {
        input,
        output,
        millis: started.elapsed().as_millis() as u64,
        stop,
    });
    Ok(reply)
}

pub fn run(
    cfg: &AiConfig,
    mode: RunMode,
    history: &[Turn],
    message: &str,
    events: &Sender<AgentEvent>,
    cancel: &Arc<AtomicBool>,
) {
    let result = match cfg.protocol {
        Protocol::Anthropic => run_anthropic(cfg, mode, history, message, events, cancel),
        Protocol::OpenAi => run_openai(cfg, mode, history, message, events, cancel),
    };
    let _ = events.send(match result {
        Ok(()) => AgentEvent::Done,
        Err(e) => AgentEvent::Error(e),
    });
}

const KEEP_FULL_ROUNDS: usize = 6;
const OLD_RESULT_CHARS: usize = 1500;
const OLD_INPUT_BYTES: usize = 4000;
const SHORTENED: &str =
    "\n…(older result shortened to save context; run the tool again if you need it)";

fn shorten(text: &str) -> Option<String> {
    let (cut, _) = text.char_indices().nth(OLD_RESULT_CHARS)?;
    let head = text.get(..cut)?;
    Some(format!("{head}{SHORTENED}"))
}

fn omitted_input(size: usize) -> Value {
    json!({ "omitted": format!("older call input of {size} bytes, already applied") })
}

fn is_tool_round(m: &Value) -> bool {
    match m.get("role").and_then(Value::as_str) {
        Some("tool") => true,
        Some("user") => m
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts.iter().any(|p| {
                    matches!(
                        p.get("type").and_then(Value::as_str),
                        Some("tool_result" | "image_url")
                    )
                })
            }),
        _ => false,
    }
}

fn compact_old_rounds(messages: &mut [Value]) {
    let rounds: Vec<usize> = (0..messages.len())
        .filter(|&i| messages.get(i).is_some_and(is_tool_round))
        .collect();
    let Some(&first_kept) = rounds.iter().rev().nth(KEEP_FULL_ROUNDS - 1) else {
        return;
    };
    for m in messages.iter_mut().take(first_kept) {
        let role = m
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        match role.as_str() {
            "tool" => {
                if let Some(s) = m.get("content").and_then(Value::as_str).and_then(shorten) {
                    m["content"] = Value::String(s);
                }
            }
            "user" => {
                let Some(parts) = m.get_mut("content").and_then(Value::as_array_mut) else {
                    continue;
                };
                for part in parts.iter_mut() {
                    match part.get("type").and_then(Value::as_str) {
                        Some("tool_result") => {
                            let Some(inner) = part.get_mut("content").and_then(Value::as_array_mut)
                            else {
                                continue;
                            };
                            for block in inner.iter_mut() {
                                match block.get("type").and_then(Value::as_str) {
                                    Some("text") => {
                                        if let Some(s) = block
                                            .get("text")
                                            .and_then(Value::as_str)
                                            .and_then(shorten)
                                        {
                                            block["text"] = Value::String(s);
                                        }
                                    }
                                    Some("image") => {
                                        *block = json!({ "type": "text", "text": "(image from an older result omitted)" });
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some("image_url") => {
                            *part = json!({ "type": "text", "text": "(image from an older result omitted)" });
                        }
                        _ => {}
                    }
                }
            }
            "assistant" => {
                if let Some(blocks) = m.get_mut("content").and_then(Value::as_array_mut) {
                    for b in blocks.iter_mut() {
                        if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                            let size = b.get("input").map_or(0, |i| i.to_string().len());
                            if size > OLD_INPUT_BYTES {
                                b["input"] = omitted_input(size);
                            }
                        }
                    }
                }
                if let Some(calls) = m.get_mut("tool_calls").and_then(Value::as_array_mut) {
                    for c in calls.iter_mut() {
                        let size = c
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .map_or(0, str::len);
                        if size > OLD_INPUT_BYTES {
                            if let Some(f) = c.get_mut("function").and_then(Value::as_object_mut) {
                                f.insert(
                                    "arguments".into(),
                                    Value::String(omitted_input(size).to_string()),
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

const MAX_OUTPUT_TOKENS: u32 = 16384;
const CUT_OFF_NUDGE: &str = "Your last reply hit the output limit and was cut off before any tool call ran. Decide in a few lines and make the tool call now.";
const MAX_CUT_OFFS: u32 = 2;

fn cut_off(events: &Sender<AgentEvent>, so_far: u32) -> Result<u32, String> {
    let n = so_far + 1;
    if n > MAX_CUT_OFFS {
        return Err(format!(
            "The model hit its output limit {n} times in a row without acting. Try a smaller request."
        ));
    }
    let _ = events.send(AgentEvent::Text(
        "(The reply hit the output limit; asking the model to continue briefly.)".into(),
    ));
    Ok(n)
}

fn step_limit_note(cfg: &AiConfig) -> String {
    format!(
        "(Stopped after {} steps, the limit per message. Say \"continue\" to go on, or raise \"Max steps\" in Settings.)",
        cfg.max_steps.max(1)
    )
}

fn call_tool(
    events: &Sender<AgentEvent>,
    cancel: &AtomicBool,
    name: &str,
    args: Value,
) -> Result<ToolOutcome, String> {
    let (tx, rx): (Sender<ToolOutcome>, Receiver<ToolOutcome>) = std::sync::mpsc::channel();
    events
        .send(AgentEvent::Tool {
            name: name.to_owned(),
            args,
            reply: tx,
        })
        .map_err(|_| "the app closed the conversation".to_owned())?;
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(out) => return Ok(out),
            Err(RecvTimeoutError::Timeout) if cancel.load(Ordering::Relaxed) => {
                return Err("Stopped.".into())
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err("the app closed the conversation".into())
            }
        }
    }
}

fn run_anthropic(
    cfg: &AiConfig,
    mode: RunMode,
    history: &[Turn],
    message: &str,
    events: &Sender<AgentEvent>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let tools: Vec<Value> = agent_tools(cfg, mode)
        .into_iter()
        .map(|(n, d, s)| json!({ "name": n, "description": d, "input_schema": s }))
        .collect();
    let mut messages: Vec<Value> = history
        .iter()
        .map(|t| json!({ "role": if t.user { "user" } else { "assistant" }, "content": t.text }))
        .collect();
    messages.push(json!({ "role": "user", "content": user_message(message, mode) }));
    let mut cut_offs = 0;
    for step in 0..cfg.max_steps.max(1) {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped.".into());
        }
        compact_old_rounds(&mut messages);
        let mut body = json!({
            "model": cfg.model,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "system": system_prompt(cfg, mode),
            "tools": tools,
            "messages": messages,
            "stream": true,
        });
        if let (Some(t), Some(obj)) = (thinking(cfg, mode, step), body.as_object_mut()) {
            obj.insert("thinking".into(), t);
        }
        let reply = request(cfg, &body, events, cancel)?;
        let content = reply.get("content").cloned().unwrap_or(json!([]));
        if reply.get("stop_reason").and_then(Value::as_str) == Some("max_tokens") {
            cut_offs = cut_off(events, cut_offs)?;
            let text: Vec<Value> = content
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .cloned()
                .collect();
            let text = if text.is_empty() {
                vec![json!({ "type": "text", "text": "(cut off)" })]
            } else {
                text
            };
            messages.push(json!({ "role": "assistant", "content": text }));
            messages.push(json!({ "role": "user", "content": CUT_OFF_NUDGE }));
            continue;
        }
        cut_offs = 0;
        let mut results = Vec::new();
        for block in content.as_array().into_iter().flatten() {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let t = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if !t.trim().is_empty() {
                        let _ = events.send(AgentEvent::Text(t.to_owned()));
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    let id = block.get("id").and_then(Value::as_str).unwrap_or("");
                    let out = call_tool(
                        events,
                        cancel,
                        name,
                        block.get("input").cloned().unwrap_or(json!({})),
                    )?;
                    let out = for_model(cfg, out);
                    let mut parts = vec![
                        json!({ "type": "text", "text": if out.text.is_empty() { "ok".to_owned() } else { out.text.clone() } }),
                    ];
                    for (mime, data) in &out.images {
                        parts.push(json!({ "type": "image", "source": { "type": "base64", "media_type": mime, "data": data } }));
                    }
                    results.push(json!({ "type": "tool_result", "tool_use_id": id, "content": parts, "is_error": out.is_error }));
                }
                _ => {}
            }
        }
        messages.push(json!({ "role": "assistant", "content": content }));
        if results.is_empty()
            || reply.get("stop_reason").and_then(Value::as_str) != Some("tool_use")
        {
            return Ok(());
        }
        messages.push(json!({ "role": "user", "content": results }));
    }
    let _ = events.send(AgentEvent::Text(step_limit_note(cfg)));
    Ok(())
}

fn run_openai(
    cfg: &AiConfig,
    mode: RunMode,
    history: &[Turn],
    message: &str,
    events: &Sender<AgentEvent>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let tools: Vec<Value> = agent_tools(cfg, mode)
        .into_iter()
        .map(|(n, d, s)| json!({ "type": "function", "function": { "name": n, "description": d, "parameters": s } }))
        .collect();
    let mut messages = vec![json!({ "role": "system", "content": system_prompt(cfg, mode) })];
    messages.extend(
        history.iter().map(
            |t| json!({ "role": if t.user { "user" } else { "assistant" }, "content": t.text }),
        ),
    );
    messages.push(json!({ "role": "user", "content": user_message(message, mode) }));
    let mut cut_offs = 0;
    for _ in 0..cfg.max_steps.max(1) {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped.".into());
        }
        compact_old_rounds(&mut messages);
        let body = json!({
            "model": cfg.model,
            "messages": messages,
            "tools": tools,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        let reply = request(cfg, &body, events, cancel)?;
        let msg = reply
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or(json!({}));
        let text = msg.get("content").and_then(Value::as_str).unwrap_or("");
        if !text.trim().is_empty() {
            let _ = events.send(AgentEvent::Text(text.to_owned()));
        }
        if reply
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            == Some("length")
        {
            cut_offs = cut_off(events, cut_offs)?;
            let text = if text.trim().is_empty() {
                "(cut off)"
            } else {
                text
            };
            messages.push(json!({ "role": "assistant", "content": text }));
            messages.push(json!({ "role": "user", "content": CUT_OFF_NUDGE }));
            continue;
        }
        cut_offs = 0;
        let calls = msg
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        messages.push(msg.clone());
        if calls.is_empty() {
            return Ok(());
        }
        let mut images = Vec::new();
        for c in calls {
            let id = c.get("id").and_then(Value::as_str).unwrap_or("");
            let name = c
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let args: Value = c
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .and_then(|a| serde_json::from_str(a).ok())
                .unwrap_or(json!({}));
            let out = for_model(cfg, call_tool(events, cancel, name, args)?);
            let mut text = if out.text.is_empty() {
                "ok".to_owned()
            } else {
                out.text.clone()
            };
            if out.is_error {
                text = format!("Error: {text}");
            }
            if !out.images.is_empty() {
                text.push_str("\n(The rendered image is attached in the next message.)");
                images.extend(out.images);
            }
            messages.push(json!({ "role": "tool", "tool_call_id": id, "content": text }));
        }
        if !images.is_empty() {
            let parts: Vec<Value> = images
                .iter()
                .map(|(mime, data)| json!({ "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{data}") } }))
                .collect();
            messages.push(json!({ "role": "user", "content": parts }));
        }
    }
    let _ = events.send(AgentEvent::Text(step_limit_note(cfg)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_results_become_tool_outcomes() {
        let r = json!({"content": [{"type": "text", "text": "a"}, {"type": "image", "mimeType": "image/png", "data": "QQ=="}], "isError": false});
        let o = ToolOutcome::from_mcp(&r);
        assert_eq!(o.text, "a");
        assert_eq!(o.images.len(), 1);
        assert!(!o.is_error);
    }

    fn events_of(sse: &str) -> Vec<Value> {
        let mut got = Vec::new();
        read_events(
            &mut std::io::Cursor::new(sse.as_bytes()),
            &AtomicBool::new(false),
            &mut |ev| {
                got.push(ev.clone());
                Ok(())
            },
        )
        .unwrap();
        got
    }

    #[test]
    fn anthropic_stream_rebuilds_text_thinking_and_tool_calls() {
        let sse = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":20,\"output_tokens\":1}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"plan\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\r\n\r\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"doc_inspect\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"pa\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"ge\\\": 3}\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":42}}\n\n",
        );
        let mut acc = AnthropicStream::default();
        let mut live = Vec::new();
        for ev in events_of(sse) {
            acc.feed(&ev, &mut |s| live.push(s)).unwrap();
        }
        let reply = acc.finish();
        assert_eq!(reply["content"][0]["thinking"], "plan");
        assert_eq!(reply["content"][0]["signature"], "sig");
        assert_eq!(reply["content"][1]["text"], "Hello");
        assert_eq!(reply["content"][2]["input"], json!({"page": 3}));
        assert_eq!(reply["stop_reason"], "tool_use");
        assert_eq!(usage_of(&reply), (120, 42));
        assert!(live.contains(&Stream::ToolStart("doc_inspect".into())));
        assert!(live.contains(&Stream::Text("lo".into())));
        let err = AnthropicStream::default()
            .feed(
                &json!({"type": "error", "error": {"message": "overloaded"}}),
                &mut |_| {},
            )
            .unwrap_err();
        assert_eq!(err, "overloaded");
    }

    #[test]
    fn openai_stream_rebuilds_tool_calls_and_usage() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"command_run\",\"arguments\":\"{\\\"comm\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"and\\\":\\\"x\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n",
            "data: {\"never\":\"read\"}\n\n",
        );
        let mut acc = OpenAiStream::default();
        let mut live = Vec::new();
        for ev in events_of(sse) {
            acc.feed(&ev, &mut |s| live.push(s)).unwrap();
        }
        let reply = acc.finish();
        let call = &reply["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(call["id"], "c1");
        assert_eq!(call["function"]["name"], "command_run");
        assert_eq!(call["function"]["arguments"], "{\"command\":\"x\"}");
        assert_eq!(reply["choices"][0]["message"]["content"], Value::Null);
        assert_eq!(reply["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(usage_of(&reply), (7, 5));
        assert_eq!(live[0], Stream::Thinking("hmm".into()));
    }

    #[test]
    fn older_tool_rounds_are_compacted_and_recent_ones_kept() {
        let big = "x".repeat(10_000);
        let mut anthropic = vec![json!({"role": "user", "content": "design"})];
        for i in 0..KEEP_FULL_ROUNDS + 2 {
            anthropic.push(json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": format!("t{i}"), "name": "command_batch", "input": {"steps": big}}
            ]}));
            anthropic.push(json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": format!("t{i}"), "content": [
                    {"type": "text", "text": big},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "QQ=="}}
                ]}
            ]}));
        }
        compact_old_rounds(&mut anthropic);
        let once = anthropic.clone();
        compact_old_rounds(&mut anthropic);
        assert_eq!(anthropic, once, "compaction is idempotent");
        let old = &anthropic[2]["content"][0]["content"];
        assert!(old[0]["text"].as_str().unwrap().ends_with(SHORTENED));
        assert_eq!(old[1]["type"], "text");
        assert!(anthropic[1]["content"][0]["input"].get("omitted").is_some());
        let last = anthropic.len() - 1;
        assert_eq!(anthropic[last]["content"][0]["content"][0]["text"], big);
        assert_eq!(anthropic[last]["content"][0]["content"][1]["type"], "image");
        assert_eq!(anthropic[last - 1]["content"][0]["input"]["steps"], big);
        assert_eq!(anthropic[0]["content"], "design");

        let mut openai = vec![json!({"role": "system", "content": "s"})];
        for i in 0..KEEP_FULL_ROUNDS + 1 {
            openai.push(json!({"role": "assistant", "content": null, "tool_calls": [
                {"id": format!("c{i}"), "type": "function", "function": {"name": "command_batch", "arguments": big}}
            ]}));
            openai.push(json!({"role": "tool", "tool_call_id": format!("c{i}"), "content": big}));
        }
        compact_old_rounds(&mut openai);
        assert!(openai[2]["content"].as_str().unwrap().ends_with(SHORTENED));
        let args = openai[1]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(serde_json::from_str::<Value>(args)
            .unwrap()
            .get("omitted")
            .is_some());
        assert_eq!(openai[openai.len() - 1]["content"], big);
        let mut short = once[..3].to_vec();
        let before = short.clone();
        compact_old_rounds(&mut short);
        assert_eq!(short, before);
    }

    #[test]
    fn plan_mode_reads_and_reasons_execution_acts_quickly() {
        let cfg = |reasoning| AiConfig {
            reasoning,
            ..AiConfig::default()
        };
        let off = Some(json!({ "type": "disabled" }));
        for mode in [RunMode::Auto, RunMode::Plan, RunMode::ExecutePlan] {
            assert_eq!(thinking(&cfg(Reasoning::Default), mode, 3), None);
        }
        let pf = cfg(Reasoning::PlanFirst);
        assert_eq!(thinking(&pf, RunMode::Auto, 0), None);
        assert_eq!(thinking(&pf, RunMode::Auto, 1), off);
        assert_eq!(thinking(&pf, RunMode::Plan, 5), None);
        assert_eq!(thinking(&pf, RunMode::ExecutePlan, 0), off);
        assert_eq!(thinking(&cfg(Reasoning::Off), RunMode::Auto, 0), off);
        assert_eq!(thinking(&cfg(Reasoning::Off), RunMode::Plan, 0), None);
        let base = AiConfig::default();
        assert!(agent_tools(&base, RunMode::Plan)
            .iter()
            .all(|(n, _, _)| *n != "command_batch"));
        assert!(agent_tools(&base, RunMode::Auto)
            .iter()
            .any(|(n, _, _)| *n == "command_batch"));
        assert!(system_prompt(&base, RunMode::Plan).contains("PLAN MODE"));
        assert!(system_prompt(&base, RunMode::ExecutePlan).contains("approved"));
        assert!(!system_prompt(&base, RunMode::Auto).contains("PLAN MODE"));
        let old: AiConfig = serde_json::from_value(json!({"provider": "Anthropic"})).unwrap();
        assert_eq!(old.reasoning, Reasoning::Default);
    }

    #[test]
    fn the_prompt_names_every_command_and_plan_mode_is_in_the_message() {
        let index = crate::command_index();
        assert!(index.lines().count() > 50, "{index}");
        assert!(index.contains("node.create.frame(parent?: nodeId, x: number"));
        assert!(index.contains("color.replace("));
        let prompt = system_prompt(&AiConfig::default(), RunMode::Auto);
        assert!(prompt.ends_with(index));
        assert!(user_message("make it green", RunMode::Plan).starts_with("[Plan mode"));
        assert_eq!(
            user_message("make it green", RunMode::Auto),
            "make it green"
        );
    }

    #[test]
    fn chat_history_keeps_recent_prompts_and_round_trips() {
        let mut h = ChatHistory::default();
        h.remember_prompt("  make it green ");
        h.remember_prompt("make it green");
        h.remember_prompt("");
        assert_eq!(h.prompts, vec!["make it green".to_owned()]);
        for i in 0..MAX_PROMPTS + 5 {
            h.remember_prompt(&format!("p{i}"));
        }
        assert_eq!(h.prompts.len(), MAX_PROMPTS);
        assert_eq!(h.prompts.last().map(String::as_str), Some("p204"));
        h.turns = vec![Turn {
            user: true,
            text: "hi".into(),
        }];
        h.log = vec![
            SavedEntry::User { text: "hi".into() },
            SavedEntry::Tool {
                label: "doc_inspect".into(),
                failed: false,
            },
            SavedEntry::Error {
                text: "Stopped.".into(),
            },
        ];
        let back: ChatHistory = serde_json::from_slice(&serde_json::to_vec(&h).unwrap()).unwrap();
        assert_eq!(back, h);
        let partial: ChatHistory = serde_json::from_value(json!({"prompts": ["a"]})).unwrap();
        assert_eq!(partial.prompts, vec!["a".to_owned()]);
        assert!(partial.log.is_empty());
    }

    #[test]
    fn cut_off_replies_are_retried_a_few_times_then_reported() {
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(cut_off(&tx, 0), Ok(1));
        assert_eq!(cut_off(&tx, 1), Ok(2));
        assert!(cut_off(&tx, 2).unwrap_err().contains("output limit"));
        assert!(matches!(rx.try_recv(), Ok(AgentEvent::Text(t)) if t.contains("output limit")));
    }

    #[test]
    fn text_only_models_get_no_image_tool_or_images() {
        let blind = AiConfig {
            vision: false,
            ..AiConfig::default()
        };
        assert!(agent_tools(&blind, RunMode::Auto)
            .iter()
            .all(|(n, _, _)| *n != "render_preview"));
        assert!(agent_tools(&AiConfig::default(), RunMode::Auto)
            .iter()
            .any(|(n, _, _)| *n == "render_preview"));
        assert!(system_prompt(&blind, RunMode::Auto).contains("cannot see images"));
        assert!(!system_prompt(&blind, RunMode::Auto).contains("call render_preview"));
        assert!(system_prompt(&AiConfig::default(), RunMode::Auto).contains("call render_preview"));
        let shot = ToolOutcome {
            text: "ok".into(),
            images: vec![("image/png".into(), "QQ==".into())],
            is_error: false,
        };
        let out = for_model(&blind, shot.clone());
        assert!(out.images.is_empty() && out.text.contains("Image not sent"));
        assert_eq!(for_model(&AiConfig::default(), shot).images.len(), 1);
        let old: AiConfig = serde_json::from_value(json!({"provider": "Anthropic"})).unwrap();
        assert!(old.vision);
    }

    #[test]
    fn stop_interrupts_a_stream() {
        let err = read_events(
            &mut std::io::Cursor::new(b"data: {}\n\n".as_slice()),
            &AtomicBool::new(true),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(err, "Stopped.");
    }

    #[test]
    fn tool_calls_wait_for_a_late_app_and_honour_stop() {
        let (tx, rx) = std::sync::mpsc::channel();
        let app = std::thread::spawn(move || {
            if let Ok(AgentEvent::Tool { reply, .. }) = rx.recv() {
                std::thread::sleep(Duration::from_millis(700));
                let _ = reply.send(ToolOutcome {
                    text: "late".into(),
                    ..Default::default()
                });
            }
        });
        let out = call_tool(&tx, &AtomicBool::new(false), "doc_inspect", json!({})).unwrap();
        assert_eq!(out.text, "late");
        app.join().unwrap();
        let (tx, _app_rx) = std::sync::mpsc::channel();
        let err = call_tool(&tx, &AtomicBool::new(true), "doc_inspect", json!({})).unwrap_err();
        assert_eq!(err, "Stopped.");
        let (tx, rx) = std::sync::mpsc::channel();
        let app = std::thread::spawn(move || drop(rx.recv()));
        let err = call_tool(&tx, &AtomicBool::new(false), "doc_inspect", json!({})).unwrap_err();
        assert_eq!(err, "the app closed the conversation");
        app.join().unwrap();
    }

    #[test]
    fn config_round_trips_and_defaults_to_a_preset() {
        let c = AiConfig::default();
        assert_eq!(c.protocol, Protocol::Anthropic);
        let back: AiConfig = serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        let local = AiConfig {
            base_url: "http://localhost:11434/v1".into(),
            ..AiConfig::default()
        };
        assert!(local.is_local());
        assert!(AiConfig {
            base_url: "http://127.0.0.1:1234/v1".into(),
            ..c.clone()
        }
        .is_local());
        assert!(AiConfig {
            base_url: "http://[::1]:1234/v1".into(),
            ..c.clone()
        }
        .is_local());
        assert!(!AiConfig {
            base_url: "http://localhost.example.com/v1".into(),
            ..c.clone()
        }
        .is_local());
        assert!(!AiConfig {
            base_url: "http://localhost@other.example/v1".into(),
            ..c
        }
        .is_local());
    }

    #[test]
    fn changing_provider_discards_previous_api_key() {
        let mut cfg = AiConfig {
            api_key: "private-example-key".into(),
            ..AiConfig::default()
        };
        cfg.use_preset(&PRESETS[1]);
        assert_eq!(cfg.provider, "OpenAI");
        assert!(cfg.api_key.is_empty());
        assert_eq!(cfg.base_url, PRESETS[1].base_url);
    }

    #[test]
    fn provider_errors_do_not_echo_a_key() {
        let message = redact_key(
            "provider said private-example-key is invalid".into(),
            "private-example-key",
        );
        assert_eq!(message, "provider said [redacted] is invalid");
    }

    #[test]
    fn saved_key_is_private_and_opt_in() {
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "deesainer-ai-settings-{}-{}",
            std::process::id(),
            u64::from_le_bytes(nonce)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ai.json");
        let cfg = AiConfig {
            api_key: "private-example-key".into(),
            ..AiConfig::default()
        };
        std::fs::write(&path, b"old settings").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        cfg.save_to(&path, true).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(AiConfig::load_from(&path), Some(cfg.clone()));
        cfg.save_to(&path, false).unwrap();
        assert!(AiConfig::load_from(&path).unwrap().api_key.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn loading_legacy_config_repairs_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "deesainer-ai-legacy-{}-{}",
            std::process::id(),
            u64::from_le_bytes(nonce)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ai.json");
        let cfg = AiConfig {
            api_key: "private-example-key".into(),
            ..AiConfig::default()
        };
        std::fs::write(&path, serde_json::to_vec(&cfg).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(AiConfig::load_from(&path), Some(cfg));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tool_specs_have_schemas() {
        let specs = crate::tool_specs();
        assert!(specs
            .iter()
            .any(|(n, _, s)| *n == "command_run" && s["type"] == "object"));
    }
}
