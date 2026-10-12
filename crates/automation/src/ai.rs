use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
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
            max_steps: 24,
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
        let body = std::fs::read(path).ok()?;
        serde_json::from_slice(&body).ok()
    }

    pub fn save(&self, keep_key: bool) -> Result<(), String> {
        let path = config_path().ok_or("no home directory")?;
        self.save_to(&path, keep_key)
    }

    fn save_to(&self, path: &Path, keep_key: bool) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut c = self.clone();
        if !keep_key {
            c.api_key.clear();
        } else {
            c.api_key = c.api_key.trim().to_owned();
        }
        let body = serde_json::to_vec_pretty(&c).map_err(|e| e.to_string())?;
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
            f.write_all(&body).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
            replace_config_file(&tmp, path, &suffix).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
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

#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub user: bool,
    pub text: String,
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

pub enum AgentEvent {
    Text(String),
    Tool {
        name: String,
        args: Value,
        reply: Sender<ToolOutcome>,
    },
    Done,
    Error(String),
}

const SYSTEM: &str = "You are the design assistant inside Deesainer, an interface design tool (frames, shapes, text, auto layout, components, styles, variables, prototypes). You edit the user's open document through tools. Work like this: call doc_inspect to see pages, layers and the selection; use command_list (with a filter) and describe_command to learn a command's parameters; make changes with command_run or, for several steps, command_batch (one undo step); after visual changes call render_preview to check the result. Coordinates are in points, y grows downward, and a child's x/y are relative to its parent. Prefer editing the selection when the user refers to 'this' or 'selected'. Keep replies short and say what you changed.";

fn agent(cfg: &AiConfig) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
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

fn post(cfg: &AiConfig, path: &str, body: &Value) -> Result<Value, String> {
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
    let mut raw = Vec::new();
    resp.into_body()
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
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
    Ok(v)
}

pub fn run(
    cfg: &AiConfig,
    history: &[Turn],
    message: &str,
    events: &Sender<AgentEvent>,
    cancel: &Arc<AtomicBool>,
) {
    let result = match cfg.protocol {
        Protocol::Anthropic => run_anthropic(cfg, history, message, events, cancel),
        Protocol::OpenAi => run_openai(cfg, history, message, events, cancel),
    };
    let _ = events.send(match result {
        Ok(()) => AgentEvent::Done,
        Err(e) => AgentEvent::Error(e),
    });
}

fn call_tool(events: &Sender<AgentEvent>, name: &str, args: Value) -> Result<ToolOutcome, String> {
    let (tx, rx): (Sender<ToolOutcome>, Receiver<ToolOutcome>) = std::sync::mpsc::channel();
    events
        .send(AgentEvent::Tool {
            name: name.to_owned(),
            args,
            reply: tx,
        })
        .map_err(|_| "the app closed the conversation".to_owned())?;
    rx.recv_timeout(Duration::from_secs(120))
        .map_err(|_| "the app did not answer the tool call".to_owned())
}

fn run_anthropic(
    cfg: &AiConfig,
    history: &[Turn],
    message: &str,
    events: &Sender<AgentEvent>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let tools: Vec<Value> = crate::tool_specs()
        .into_iter()
        .map(|(n, d, s)| json!({ "name": n, "description": d, "input_schema": s }))
        .collect();
    let mut messages: Vec<Value> = history
        .iter()
        .map(|t| json!({ "role": if t.user { "user" } else { "assistant" }, "content": t.text }))
        .collect();
    messages.push(json!({ "role": "user", "content": message }));
    for _ in 0..cfg.max_steps.max(1) {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped.".into());
        }
        let body = json!({
            "model": cfg.model,
            "max_tokens": 8192,
            "system": SYSTEM,
            "tools": tools,
            "messages": messages,
        });
        let reply = post(cfg, "messages", &body)?;
        let content = reply.get("content").cloned().unwrap_or(json!([]));
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
                        name,
                        block.get("input").cloned().unwrap_or(json!({})),
                    )?;
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
    let _ = events.send(AgentEvent::Text("(Stopped after the step limit.)".into()));
    Ok(())
}

fn run_openai(
    cfg: &AiConfig,
    history: &[Turn],
    message: &str,
    events: &Sender<AgentEvent>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let tools: Vec<Value> = crate::tool_specs()
        .into_iter()
        .map(|(n, d, s)| json!({ "type": "function", "function": { "name": n, "description": d, "parameters": s } }))
        .collect();
    let mut messages = vec![json!({ "role": "system", "content": SYSTEM })];
    messages.extend(
        history.iter().map(
            |t| json!({ "role": if t.user { "user" } else { "assistant" }, "content": t.text }),
        ),
    );
    messages.push(json!({ "role": "user", "content": message }));
    for _ in 0..cfg.max_steps.max(1) {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped.".into());
        }
        let body = json!({ "model": cfg.model, "messages": messages, "tools": tools });
        let reply = post(cfg, "chat/completions", &body)?;
        let msg = reply
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or(json!({}));
        if let Some(t) = msg.get("content").and_then(Value::as_str) {
            if !t.trim().is_empty() {
                let _ = events.send(AgentEvent::Text(t.to_owned()));
            }
        }
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
            let out = call_tool(events, name, args)?;
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
    let _ = events.send(AgentEvent::Text("(Stopped after the step limit.)".into()));
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
