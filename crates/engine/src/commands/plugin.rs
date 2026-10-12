use base64::Engine as _;
use serde_json::json;

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Effect, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("plugin.run", "Run plugin", run)
        .menu("Plugins/Run plugin")
        .doc("Runs a sandboxed WebAssembly plugin (base64 .wasm in data, or WAT text in wat). It can only call commands (no files, network or clock); its edits are one undo step. Returns {logs, calls, fuelUsed, panel?}: a panel the plugin declared is drawn by the app.")
        .params(const {
            &[
                param("data", ParamType::String, false, "Base64-encoded .wasm module."),
                param("wat", ParamType::String, false, "WebAssembly text (for development)."),
                param("entry", ParamType::String, false, "Exported function to call, default \"run\"."),
                param("input", ParamType::Json, false, "JSON the plugin reads with input() (its panel's values)."),
            ]
        })]
}

fn run(_: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let module = match (p.str("data")?, p.str("wat")?) {
        (Some(b64), None) => base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| p.bad("data", format!("is not valid base64: {e}")))?,
        (None, Some(wat)) => wat.as_bytes().to_vec(),
        _ => return Err(p.invalid("pass exactly one of data or wat")),
    };
    if module.len() > 32 * 1024 * 1024 {
        return Err(p.invalid("the module is larger than 32 MB"));
    }
    let entry = p.str("entry")?.unwrap_or("run").to_owned();
    let input = p.raw("input").cloned().unwrap_or(serde_json::Value::Null);
    if input.to_string().len() > 1024 * 1024 {
        return Err(p.bad("input", "at most 1 MB of JSON"));
    }
    let mut out = Outcome::effect(Effect::RunPlugin {
        module,
        entry,
        input,
    });
    out.reply = json!({});
    Ok(out)
}
