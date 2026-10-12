use serde_json::{json, Value};

use crate::Backend;

pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BATCH_STEPS: usize = 256;

pub const METHODS: &[&str] = &[
    "engine.commands",
    "engine.describe",
    "engine.execute",
    "engine.batch",
];

fn error_reply(id: &Value, message: &str) -> Value {
    json!({ "id": id, "error": { "message": message } })
}

pub fn handle_line<B: Backend>(backend: &mut B, line: &str) -> Value {
    if line.len() > MAX_REQUEST_BYTES {
        return error_reply(&Value::Null, "request exceeds 1 MiB");
    }
    let request: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(err) => return error_reply(&Value::Null, &format!("parse error: {err}")),
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str);
    let result = handle(backend, method, &params);
    let reply = match result {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(message) => error_reply(&id, &message),
    };
    if reply.to_string().len() > MAX_REPLY_BYTES {
        return error_reply(
            &id,
            "reply exceeds 8 MiB; request less (e.g. a smaller render)",
        );
    }
    reply
}

fn handle<B: Backend>(
    backend: &mut B,
    method: Option<&str>,
    params: &Value,
) -> Result<Value, String> {
    match method {
        Some("engine.commands") => backend.commands(),
        Some("engine.describe") => {
            let command = params.get("command").and_then(Value::as_str).unwrap_or("");
            backend
                .commands()?
                .as_array()
                .and_then(|l| l.iter().find(|c| c["id"] == command).cloned())
                .ok_or_else(|| format!("unknown command {command:?}"))
        }
        Some("engine.execute") => match params.get("command").and_then(Value::as_str) {
            Some(command) => {
                let args = params.get("params").cloned().unwrap_or(Value::Null);
                backend.dispatch(command, &args)
            }
            None => Err("engine.execute needs params.command".to_owned()),
        },
        Some("engine.batch") => {
            let steps: Vec<(String, Value)> = params
                .get("steps")
                .and_then(Value::as_array)
                .ok_or("engine.batch needs params.steps")?
                .iter()
                .map(|s| {
                    (
                        s.get("command")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        s.get("params").cloned().unwrap_or(Value::Null),
                    )
                })
                .collect();
            if steps.len() > MAX_BATCH_STEPS {
                return Err(format!("at most {MAX_BATCH_STEPS} steps per batch"));
            }
            let label = params
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("Batch");
            backend.batch(label, &steps).map(Value::Array)
        }
        Some(other) => Err(format!(
            "unknown method {other:?}; accepted: {}",
            METHODS.join(", ")
        )),
        None => Err("missing method".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_engine::Session;

    #[test]
    fn survives_garbage_and_executes_commands() {
        let mut s = Session::new().unwrap();
        assert!(handle_line(&mut s, "not json").get("error").is_some());
        let reply = handle_line(
            &mut s,
            r#"{"id":7,"method":"engine.execute","params":{"command":"node.create.rectangle","params":{"x":0,"y":0,"width":10,"height":10}}}"#,
        );
        assert_eq!(reply["id"], 7);
        assert!(reply["result"]["id"].is_u64());
        let batch = handle_line(
            &mut s,
            r#"{"id":8,"method":"engine.batch","params":{"steps":[{"command":"edit.undo"}]}}"#,
        );
        assert!(batch["result"].is_array(), "{batch}");
    }
}
