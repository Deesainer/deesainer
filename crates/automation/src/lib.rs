pub mod ai;
pub mod control;
pub mod control_server;
pub mod figma_api;
pub mod files;
pub mod sync_client;
pub mod team_client;
pub mod tls;

pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub fn os_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "You".to_owned())
}

use std::io::{BufRead, Write};

use deesainer_engine::{CommandSpec, EngineError, Session};
use serde_json::{json, Map, Value};

pub struct SyncedSession {
    pub session: Session,
    pub client: sync_client::SyncClient,
}

impl SyncedSession {
    pub fn join(
        addr: &str,
        name: &str,
        token: &str,
        timeout: std::time::Duration,
    ) -> Result<Self, String> {
        let mut session = Session::new().map_err(|e| e.to_string())?;
        session.set_user(name);
        session.set_clock(now_rfc3339);
        let mut client = sync_client::SyncClient::connect(addr, name, token);
        let start = std::time::Instant::now();
        while !client.is_connected() {
            client.poll(&mut session, None);
            if start.elapsed() > timeout {
                return Err(format!("could not join {addr}: {}", client.status));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Ok(Self { session, client })
    }

    fn sync(&mut self) {
        self.client.poll(&mut self.session, None);
    }
}

impl Backend for SyncedSession {
    fn dispatch(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        self.sync();
        let out = Backend::dispatch(&mut self.session, command, params);
        self.sync();
        out
    }

    fn batch(&mut self, label: &str, steps: &[(String, Value)]) -> Result<Vec<Value>, String> {
        self.sync();
        let out = Backend::batch(&mut self.session, label, steps);
        self.sync();
        out
    }

    fn commands(&mut self) -> Result<Value, String> {
        Backend::commands(&mut self.session)
    }
}

pub const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub trait Backend {
    fn dispatch(&mut self, command: &str, params: &Value) -> Result<Value, String>;
    fn batch(&mut self, label: &str, steps: &[(String, Value)]) -> Result<Vec<Value>, String>;
    fn commands(&mut self) -> Result<Value, String>;
    fn bridge_call(&mut self, _method: &str, _params: &Value) -> Option<Result<Value, String>> {
        None
    }
    fn is_bridge(&self) -> bool {
        false
    }
}

impl<B: Backend + ?Sized> Backend for &mut B {
    fn dispatch(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        (**self).dispatch(command, params)
    }
    fn batch(&mut self, label: &str, steps: &[(String, Value)]) -> Result<Vec<Value>, String> {
        (**self).batch(label, steps)
    }
    fn commands(&mut self) -> Result<Value, String> {
        (**self).commands()
    }
    fn bridge_call(&mut self, method: &str, params: &Value) -> Option<Result<Value, String>> {
        (**self).bridge_call(method, params)
    }
    fn is_bridge(&self) -> bool {
        (**self).is_bridge()
    }
}

fn param_signatures(c: &Value) -> Vec<String> {
    c["params"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let req = if p["required"].as_bool() == Some(true) {
                ""
            } else {
                "?"
            };
            format!(
                "{}{}: {}",
                p["name"].as_str().unwrap_or(""),
                req,
                p["type"].as_str().unwrap_or("")
            )
        })
        .collect()
}

fn command_line(c: &Value) -> String {
    format!(
        "{}({})",
        c["id"].as_str().unwrap_or(""),
        param_signatures(c).join(", ")
    )
}

pub fn command_index() -> &'static str {
    static INDEX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let Ok(mut session) = Session::new() else {
            return String::new();
        };
        Backend::commands(&mut session)
            .ok()
            .and_then(|all| {
                all.as_array()
                    .map(|l| l.iter().map(command_line).collect::<Vec<_>>().join("\n"))
            })
            .unwrap_or_default()
    })
}

fn short_number(v: &Value) -> Value {
    match v.as_f64() {
        Some(n) if n.is_finite() => {
            let r = (n * 10.0).round() / 10.0;
            if r.fract() == 0.0 && r.abs() < 1e15 {
                json!(r as i64)
            } else {
                json!(r)
            }
        }
        _ => v.clone(),
    }
}

fn compact_layer(n: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("id".into(), n["id"].clone());
    out.insert("type".into(), n["type"].clone());
    let text = n.get("text").and_then(Value::as_str);
    if n.get("name").and_then(Value::as_str) != text {
        out.insert("name".into(), n["name"].clone());
    }
    if let Some(t) = text {
        out.insert("text".into(), json!(t));
    }
    if let Some(r) = n.get("rect").filter(|r| r.is_object()) {
        let rect: Vec<Value> = ["x", "y", "width", "height"]
            .iter()
            .map(|k| short_number(&r[*k]))
            .collect();
        out.insert("rect".into(), Value::Array(rect));
    }
    if n.get("visible") == Some(&Value::Bool(false)) {
        out.insert("hidden".into(), Value::Bool(true));
    }
    if let Some(rot) = n.get("rotation") {
        out.insert("rotation".into(), short_number(rot));
    }
    if let Some(children) = n.get("children").and_then(Value::as_array) {
        out.insert(
            "children".into(),
            Value::Array(children.iter().map(compact_layer).collect()),
        );
    }
    Value::Object(out)
}

fn compact_tree(v: &Value) -> Value {
    let mut out = v.clone();
    if let (Some(tree), Some(obj)) = (v.get("tree"), out.as_object_mut()) {
        obj.insert("tree".into(), compact_layer(tree));
    }
    out
}

pub fn tool_specs() -> Vec<(&'static str, &'static str, Value)> {
    tools()
        .into_iter()
        .map(|t| (t.name, t.description, (t.schema)()))
        .collect()
}

impl Backend for Session {
    fn dispatch(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        Session::dispatch(self, command, params).map_err(|e| e.to_string())
    }

    fn batch(&mut self, label: &str, steps: &[(String, Value)]) -> Result<Vec<Value>, String> {
        self.dispatch_batch(label, steps)
            .map_err(|(i, e): (usize, EngineError)| {
                format!("step {i} failed: {e} (earlier steps stay applied)")
            })
    }

    fn commands(&mut self) -> Result<Value, String> {
        Ok(Value::Array(
            self.registry()
                .iter()
                .map(|spec: &CommandSpec| {
                    let mut v = spec.to_json();
                    v["inputSchema"] = spec.input_schema();
                    v
                })
                .collect(),
        ))
    }
}

struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
}

fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "command_list",
            title: "List commands",
            description: "Lists Deesainer commands. Every feature is a command; start here. Without a filter: one line per command, \"id(param: type, optional?: type)\". With a filter (a substring of ids and labels, e.g. \"align\" or \"text.\"): the matching commands with label, doc, readOnly and params.",
            schema: || json!({"type": "object", "properties": {"filter": {"type": "string", "description": "Substring of the id or label."}}, "additionalProperties": false}),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "describe_command",
            title: "Describe a command",
            description: "Returns one command's documentation and the JSON Schema of its params.",
            schema: || json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"], "additionalProperties": false}),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "command_run",
            title: "Run a command",
            description: "Runs one command with params (an object). Each mutating command is one undo step. Unknown params are errors listing the accepted ones.",
            schema: || json!({"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"], "additionalProperties": false}),
            read_only: false,
            destructive: true,
            idempotent: false,
        },
        Tool {
            name: "command_batch",
            title: "Run several commands",
            description: "Runs commands in order as a single undo step. A param value \"$N\" is the id step N (from 0) returned, so one batch can create a frame, add children with parent \"$0\" and align them with ids [\"$1\", \"$2\"]. Stops at the first failure; earlier steps stay applied, so inspect state before retrying.",
            schema: || json!({"type": "object", "properties": {
                "label": {"type": "string", "description": "Undo label."},
                "steps": {"type": "array", "items": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]}}
            }, "required": ["steps"], "additionalProperties": false}),
            read_only: false,
            destructive: true,
            idempotent: false,
        },
        Tool {
            name: "doc_inspect",
            title: "Inspect the document",
            description: "Returns pages, the selection and a page's layer tree, compact: each layer is {id, type, name, text?, rect: [x, y, width, height] in its parent's space, hidden?, rotation?, children?}; name is left out when it equals the text. Use node ids from here in commands.",
            schema: || json!({"type": "object", "properties": {"page": {"type": "integer", "description": "Page id; default current page."}}, "additionalProperties": false}),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "render_preview",
            title: "Render a preview",
            description: "Renders the current page or a layer to a PNG you can look at. Use it after edits to verify the result visually.",
            schema: || json!({"type": "object", "properties": {
                "id": {"type": "integer", "description": "Layer or page id; default current page."},
                "scale": {"type": "number", "description": "Pixels per point, default 1."},
                "maxSize": {"type": "integer", "description": "Longest side cap, default 1600."}
            }, "additionalProperties": false}),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
    ]
}

fn bridge_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "ui_screenshot",
            title: "Screenshot the app",
            description: "Returns a PNG of what the app's canvas shows right now (current page, zoom and pan).",
            schema: || json!({"type": "object", "properties": {"scale": {"type": "number"}}, "additionalProperties": false}),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "ui_state",
            title: "App state",
            description: "Returns the app's current page, selection, zoom, tool and file.",
            schema: || json!({"type": "object", "properties": {}, "additionalProperties": false}),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "app_open",
            title: "Open a file",
            description: "Opens a .deesainer file from the automation read root (relative, forward-slash path).",
            schema: || json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": false}),
            read_only: false,
            destructive: true,
            idempotent: false,
        },
        Tool {
            name: "app_save",
            title: "Save the file",
            description: "Saves the document as .deesainer into the automation write root.",
            schema: || json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": false}),
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "app_export",
            title: "Export to a file",
            description: "Exports a layer or the page (by extension: .png, .svg, .pdf) into the automation write root.",
            schema: || json!({"type": "object", "properties": {"path": {"type": "string"}, "id": {"type": "integer"}, "scale": {"type": "number"}}, "required": ["path"], "additionalProperties": false}),
            read_only: false,
            destructive: false,
            idempotent: true,
        },
    ]
}

fn tool_json(t: &Tool) -> Value {
    json!({
        "name": t.name,
        "title": t.title,
        "description": t.description,
        "inputSchema": (t.schema)(),
        "annotations": {
            "title": t.title,
            "readOnlyHint": t.read_only,
            "destructiveHint": t.destructive,
            "idempotentHint": t.idempotent,
            "openWorldHint": false,
        },
    })
}

#[derive(Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }
}

fn strict_args<'a>(
    args: &'a Value,
    accepted: &[&str],
    tool: &str,
) -> Result<&'a Map<String, Value>, RpcError> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    let map = match args {
        Value::Null => EMPTY.get_or_init(Map::new),
        Value::Object(m) => m,
        _ => {
            return Err(RpcError::invalid_params(format!(
                "{tool}: arguments must be an object"
            )))
        }
    };
    for key in map.keys() {
        if !accepted.contains(&key.as_str()) {
            return Err(RpcError::invalid_params(format!(
                "{tool}: unknown argument {key:?}; accepted: {}",
                accepted.join(", ")
            )));
        }
    }
    Ok(map)
}

pub struct McpServer<B: Backend> {
    backend: B,
    name: String,
}

impl<B: Backend> McpServer<B> {
    pub fn new(backend: B, name: &str) -> Self {
        Self {
            backend,
            name: name.to_owned(),
        }
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(
                    json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})
                        .to_string(),
                )
            }
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let id = id?;
        let reply = match self.handle(method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": e.code, "message": e.message}})
            }
        };
        Some(reply.to_string())
    }

    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "initialize" => {
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let version = if SUPPORTED_PROTOCOLS.contains(&requested) {
                    requested
                } else {
                    SUPPORTED_PROTOCOLS[0]
                };
                let mut instructions = "Deesainer is an interface design tool. Every feature is a command: call command_list, then command_run. Use doc_inspect for node ids and render_preview to see the result. Coordinates are logical points; x/y are in the parent's space."
                    .to_owned();
                if self.backend.is_bridge() {
                    let open = self
                        .backend
                        .bridge_call("ui_state", &Value::Null)
                        .and_then(|r| r.ok())
                        .and_then(|v| v.get("file").cloned())
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .filter(|name| !name.is_empty());
                    match &open {
                        Some(name) => instructions.push_str(&format!(
                            " The connected app has the document \"{name}\" open. When asked to create or edit a design, ask the user whether to work on this document or start a new one (file.new)."
                        )),
                        None => instructions.push_str(
                            " The connected app has a document open. When asked to create or edit a design, ask the user whether to work on it or start a new one (file.new).",
                        ),
                    }
                }
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}},
                    "serverInfo": {"name": self.name, "title": "Deesainer", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": instructions,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let mut list: Vec<Value> = tools().iter().map(tool_json).collect();
                if self.backend.is_bridge() {
                    list.extend(bridge_tools().iter().map(tool_json));
                }
                Ok(json!({ "tools": list }))
            }
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::invalid_params("tools/call needs a name"))?;
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                self.call_tool(name, &args)
            }
            "resources/list" => Ok(json!({"resources": [
                {"uri": "deesainer://document", "name": "document", "title": "Current document tree", "mimeType": "application/json"},
                {"uri": "deesainer://commands", "name": "commands", "title": "Command registry", "mimeType": "application/json"},
            ]})),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                let value = match uri {
                    "deesainer://document" => self.backend.dispatch("document.tree", &Value::Null),
                    "deesainer://commands" => self.backend.commands(),
                    _ => {
                        return Err(RpcError::invalid_params(format!(
                            "unknown resource {uri:?}"
                        )))
                    }
                }
                .map_err(|e| RpcError {
                    code: -32603,
                    message: e,
                })?;
                Ok(
                    json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": value.to_string()}]}),
                )
            }
            other => Err(RpcError {
                code: -32601,
                message: format!("method not found: {other}"),
            }),
        }
    }

    fn is_command(&mut self, name: &str) -> bool {
        name.contains('.')
            && self.backend.commands().is_ok_and(|all| {
                all.as_array()
                    .is_some_and(|l| l.iter().any(|c| c["id"] == name))
            })
    }

    pub fn call_tool(&mut self, name: &str, args: &Value) -> Result<Value, RpcError> {
        let text = |v: &Value| json!({"content": [{"type": "text", "text": v.to_string()}], "structuredContent": wrap(v)});
        let err = |m: String| json!({"content": [{"type": "text", "text": m}], "isError": true});
        match name {
            "command_list" => {
                let a = strict_args(args, &["filter"], name)?;
                let filter = a
                    .get("filter")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_lowercase();
                let all = self.backend.commands().map_err(|e| RpcError {
                    code: -32603,
                    message: e,
                })?;
                let list: Vec<Value> = all
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|c| {
                        filter.is_empty()
                            || c["id"]
                                .as_str()
                                .is_some_and(|s| s.to_lowercase().contains(&filter))
                            || c["label"]
                                .as_str()
                                .is_some_and(|s| s.to_lowercase().contains(&filter))
                    })
                    .map(|c| {
                        if filter.is_empty() {
                            return json!(command_line(c));
                        }
                        let params = param_signatures(c);
                        json!({
                            "id": c["id"],
                            "label": c["label"],
                            "doc": c["doc"],
                            "readOnly": c["readOnly"],
                            "params": params,
                        })
                    })
                    .collect();
                Ok(text(&Value::Array(list)))
            }
            "describe_command" => {
                let a = strict_args(args, &["command"], name)?;
                let cmd = a
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::invalid_params("describe_command needs command"))?;
                let all = self.backend.commands().map_err(|e| RpcError {
                    code: -32603,
                    message: e,
                })?;
                match all
                    .as_array()
                    .and_then(|l| l.iter().find(|c| c["id"] == cmd))
                {
                    Some(spec) => Ok(text(spec)),
                    None => Ok(err(format!("unknown command {cmd:?}; call command_list"))),
                }
            }
            "command_run" => {
                let a = strict_args(args, &["command", "params"], name)?;
                let cmd = a
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::invalid_params("command_run needs command"))?;
                let params = a.get("params").cloned().unwrap_or(Value::Null);
                match self.backend.dispatch(cmd, &params) {
                    Ok(v) if cmd == "view.renderPreview" => Ok(image_result(&v)),
                    Ok(v) => Ok(text(&v)),
                    Err(e) => Ok(err(e)),
                }
            }
            "command_batch" => {
                let a = strict_args(args, &["steps", "label"], name)?;
                let label = a.get("label").and_then(Value::as_str).unwrap_or("Batch");
                let steps: Vec<(String, Value)> = a
                    .get("steps")
                    .and_then(Value::as_array)
                    .ok_or_else(|| RpcError::invalid_params("command_batch needs steps"))?
                    .iter()
                    .map(|s| {
                        let cmd = s
                            .get("command")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        (cmd, s.get("params").cloned().unwrap_or(Value::Null))
                    })
                    .collect();
                if steps.len() > 256 {
                    return Ok(err("at most 256 steps per batch".into()));
                }
                match self.backend.batch(label, &steps) {
                    Ok(replies) => Ok(text(&Value::Array(replies))),
                    Err(e) => Ok(err(e)),
                }
            }
            "doc_inspect" => {
                let a = strict_args(args, &["page"], name)?;
                let params = match a.get("page") {
                    Some(p) => json!({"page": p}),
                    None => Value::Null,
                };
                match self.backend.dispatch("document.tree", &params) {
                    Ok(v) => Ok(text(&compact_tree(&v))),
                    Err(e) => Ok(err(e)),
                }
            }
            "render_preview" => {
                let a = strict_args(args, &["id", "scale", "maxSize"], name)?;
                match self
                    .backend
                    .dispatch("view.renderPreview", &Value::Object(a.clone()))
                {
                    Ok(v) => Ok(image_result(&v)),
                    Err(e) => Ok(err(e)),
                }
            }
            bridge @ ("ui_screenshot" | "ui_state" | "app_open" | "app_save" | "app_export")
                if self.backend.is_bridge() =>
            {
                let method = match bridge {
                    "ui_screenshot" => "ui.screenshot",
                    "ui_state" => "ui.state",
                    "app_open" => "app.open",
                    "app_save" => "app.save",
                    _ => "app.export",
                };
                let accepted: &[&str] = match bridge {
                    "ui_screenshot" => &["scale"],
                    "ui_state" => &[],
                    "app_export" => &["path", "id", "scale"],
                    _ => &["path"],
                };
                let a = strict_args(args, accepted, name)?;
                match self.backend.bridge_call(method, &Value::Object(a.clone())) {
                    Some(Ok(v)) if bridge == "ui_screenshot" => Ok(image_result(&v)),
                    Some(Ok(v)) => Ok(text(&v)),
                    Some(Err(e)) => Ok(err(e)),
                    None => Ok(err("not connected to an app".into())),
                }
            }
            cmd if self.is_command(cmd) => {
                let params = if args.as_object().is_some_and(|o| !o.is_empty()) {
                    args.clone()
                } else {
                    Value::Null
                };
                match self.backend.dispatch(cmd, &params) {
                    Ok(v) if cmd == "view.renderPreview" => Ok(image_result(&v)),
                    Ok(v) => Ok(text(&v)),
                    Err(e) => Ok(err(e)),
                }
            }
            other => Err(RpcError::invalid_params(format!(
                "unknown tool {other:?}; tools: {}",
                tools()
                    .iter()
                    .map(|t| t.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    pub fn run_stdio(&mut self) -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let mut out = std::io::stdout().lock();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if line.len() > control::MAX_REQUEST_BYTES {
                writeln!(
                    out,
                    "{}",
                    json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "request exceeds 1 MiB"}})
                )?;
                out.flush()?;
                continue;
            }
            if let Some(reply) = self.handle_line(&line) {
                writeln!(out, "{reply}")?;
                out.flush()?;
            }
        }
        Ok(())
    }
}

fn wrap(v: &Value) -> Value {
    match v {
        Value::Object(_) => v.clone(),
        other => json!({ "result": other }),
    }
}

fn image_result(v: &Value) -> Value {
    let data = v["data"].as_str().unwrap_or("");
    let meta = json!({"width": v["width"], "height": v["height"], "bounds": v["bounds"], "scale": v["scale"]});
    json!({"content": [
        {"type": "image", "data": data, "mimeType": "image/png"},
        {"type": "text", "text": meta.to_string()},
    ]})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> McpServer<Session> {
        McpServer::new(Session::new().unwrap(), "deesainer")
    }

    fn call(s: &mut McpServer<Session>, msg: Value) -> Value {
        serde_json::from_str(&s.handle_line(&msg.to_string()).unwrap()).unwrap()
    }

    #[test]
    fn initialize_list_and_call() {
        let mut s = server();
        let init = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(s
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
        let list = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        );
        let tools = list["result"]["tools"].as_array().unwrap();
        assert!(tools
            .iter()
            .all(|t| t["annotations"]["readOnlyHint"].is_boolean() && t["title"].is_string()));
        let run = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
                "name": "command_run",
                "arguments": {"command": "node.create.rectangle", "params": {"x": 0, "y": 0, "width": 50, "height": 20, "fill": "#1E90FF"}}
            }}),
        );
        assert!(run["result"]["isError"].is_null(), "{run}");
        let shot = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "render_preview", "arguments": {}}}),
        );
        assert_eq!(shot["result"]["content"][0]["type"], "image");
        let tree = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "doc_inspect", "arguments": {}}}),
        );
        assert!(tree["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("rectangle"));
    }

    #[test]
    fn strict_arguments_and_errors() {
        let mut s = server();
        let bad = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "doc_inspect", "arguments": {"nope": 1}}}),
        );
        assert_eq!(bad["error"]["code"], -32602);
        assert!(bad["error"]["message"]
            .as_str()
            .unwrap()
            .contains("accepted: page"));
        let failing = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "command_run", "arguments": {"command": "edit.undo"}}}),
        );
        assert_eq!(failing["result"]["isError"], true);
        let unknown = call(&mut s, json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}));
        assert_eq!(unknown["error"]["code"], -32601);
        let garbage = s.handle_line("{not json").unwrap();
        assert!(garbage.contains("-32700"));
    }

    #[test]
    fn batch_and_resources() {
        let mut s = server();
        let batch = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "command_batch", "arguments": {"steps": [
                {"command": "node.create.frame", "params": {"x": 0, "y": 0, "width": 100, "height": 100}},
                {"command": "node.create.ellipse", "params": {"x": 10, "y": 10, "width": 10, "height": 10}}
            ]}}}),
        );
        assert!(batch["result"]["isError"].is_null(), "{batch}");
        let res = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": {"uri": "deesainer://document"}}),
        );
        assert!(res["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("frame"));
    }

    #[test]
    fn doc_inspect_is_a_compact_layer_tree() {
        let mut s = server();
        let made = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "command_batch", "arguments": {"steps": [
                {"command": "node.create.frame", "params": {"x": 10, "y": 20.25, "width": 390, "height": 844}},
                {"command": "node.create.text", "params": {"x": 0, "y": 0, "content": "Hello"}},
                {"command": "node.create.rectangle", "params": {"x": 0, "y": 0, "width": 4, "height": 4, "name": "dot"}}
            ]}}}),
        );
        assert!(made["result"]["isError"].is_null(), "{made}");
        let reply = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "doc_inspect", "arguments": {}}}),
        );
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        let layers = v["tree"]["children"].as_array().unwrap();
        let frame = layers.iter().find(|l| l["type"] == "frame").unwrap();
        assert_eq!(frame["rect"], json!([10, 20.3, 390, 844]));
        let hello = layers.iter().find(|l| l["type"] == "text").unwrap();
        assert_eq!(hello["text"], "Hello");
        assert!(
            hello.get("name").is_none(),
            "name equal to the text is dropped"
        );
        let dot = layers.iter().find(|l| l["type"] == "rectangle").unwrap();
        assert_eq!(dot["name"], "dot");
        assert!(!text.contains("\"visible\""));
        let full = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "document.tree", "arguments": {}}}),
        );
        let full = full["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.len() * 10 < full.len() * 8,
            "{} vs {}",
            text.len(),
            full.len()
        );
    }

    #[test]
    fn command_ids_called_as_tools_run_as_commands() {
        let mut s = server();
        let tree = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "document.tree", "arguments": {}}}),
        );
        assert!(tree["result"]["isError"].is_null(), "{tree}");
        let made = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "node.create.rectangle", "arguments": {"x": 0, "y": 0, "width": 40, "height": 20, "cornerRadius": 8}}}),
        );
        assert!(made["result"]["isError"].is_null(), "{made}");
        let bad = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "node.create.rectangle", "arguments": {"x": 0}}}),
        );
        assert_eq!(bad["result"]["isError"], true, "{bad}");
        let unknown = call(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "no.such.thing", "arguments": {}}}),
        );
        assert!(unknown["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }
}
