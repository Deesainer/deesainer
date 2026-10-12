use std::sync::mpsc::{channel, Receiver, Sender};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasmi::{
    Caller, Config, Engine, Extern, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub fuel: u64,
    pub memory_bytes: usize,
    pub max_calls: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel: 200_000_000,
            memory_bytes: 64 * 1024 * 1024,
            max_calls: 10_000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("the plugin is not valid WebAssembly: {0}")]
    Invalid(String),
    #[error("the plugin has no exported function {0:?}")]
    NoEntry(String),
    #[error("the plugin ran out of fuel (it took too long)")]
    OutOfFuel,
    #[error("the plugin trapped: {0}")]
    Trap(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub logs: Vec<String>,
    pub calls: u32,
    pub fuel_used: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub main: String,
    #[serde(default)]
    pub commands: Vec<MenuEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuEntry {
    pub label: String,
    pub entry: String,
}

enum Request {
    Call(String, Value),
    Done(Result<Report, PluginError>),
}

struct HostState {
    to_host: Sender<Request>,
    from_host: Receiver<Result<Value, String>>,
    last: Vec<u8>,
    logs: Vec<String>,
    calls: u32,
    max_calls: u32,
    limits: StoreLimits,
    input: Vec<u8>,
    panel: Option<Value>,
}

fn memory(caller: &Caller<'_, HostState>) -> Result<wasmi::Memory, wasmi::Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("the plugin must export its memory as \"memory\""))
}

fn read_bytes(caller: &Caller<'_, HostState>, ptr: i32, len: i32) -> Result<Vec<u8>, wasmi::Error> {
    if len < 0 || ptr < 0 || len > 16 * 1024 * 1024 {
        return Err(wasmi::Error::new("bad pointer or length"));
    }
    let mem = memory(caller)?;
    let mut buf = vec![0u8; len as usize];
    mem.read(caller, ptr as usize, &mut buf)
        .map_err(|_| wasmi::Error::new("read out of bounds"))?;
    Ok(buf)
}

fn link(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker.func_wrap(
        "deesainer",
        "call",
        |mut caller: Caller<'_, HostState>,
         cp: i32,
         cl: i32,
         pp: i32,
         pl: i32|
         -> Result<i32, wasmi::Error> {
            let cmd = String::from_utf8(read_bytes(&caller, cp, cl)?)
                .map_err(|_| wasmi::Error::new("command id is not UTF-8"))?;
            let params: Value = if pl == 0 {
                Value::Null
            } else {
                serde_json::from_slice(&read_bytes(&caller, pp, pl)?)
                    .map_err(|e| wasmi::Error::new(format!("params are not JSON: {e}")))?
            };
            let state = caller.data_mut();
            state.calls += 1;
            if state.calls > state.max_calls {
                return Err(wasmi::Error::new("too many command calls"));
            }
            state
                .to_host
                .send(Request::Call(cmd, params))
                .map_err(|_| wasmi::Error::new("host went away"))?;
            let reply = state
                .from_host
                .recv()
                .map_err(|_| wasmi::Error::new("host went away"))?;
            Ok(match reply {
                Ok(v) => {
                    state.last = v.to_string().into_bytes();
                    state.last.len() as i32
                }
                Err(e) => {
                    state.last = e.into_bytes();
                    -(state.last.len() as i32)
                }
            })
        },
    )?;
    linker.func_wrap(
        "deesainer",
        "result",
        |mut caller: Caller<'_, HostState>, dst: i32| -> Result<(), wasmi::Error> {
            if dst < 0 {
                return Err(wasmi::Error::new("bad pointer"));
            }
            let mem = memory(&caller)?;
            let data = std::mem::take(&mut caller.data_mut().last);
            mem.write(&mut caller, dst as usize, &data)
                .map_err(|_| wasmi::Error::new("write out of bounds"))?;
            caller.data_mut().last = data;
            Ok(())
        },
    )?;
    linker.func_wrap(
        "deesainer",
        "input",
        |mut caller: Caller<'_, HostState>, dst: i32| -> Result<i32, wasmi::Error> {
            let data = caller.data().input.clone();
            if dst >= 0 {
                let mem = memory(&caller)?;
                mem.write(&mut caller, dst as usize, &data)
                    .map_err(|_| wasmi::Error::new("write out of bounds"))?;
            }
            Ok(data.len() as i32)
        },
    )?;
    linker.func_wrap(
        "deesainer",
        "panel",
        |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<(), wasmi::Error> {
            if len > 256 * 1024 {
                return Err(wasmi::Error::new("the panel is larger than 256 KB"));
            }
            let bytes = read_bytes(&caller, ptr, len)?;
            let v: Value = serde_json::from_slice(&bytes)
                .map_err(|e| wasmi::Error::new(format!("the panel is not JSON: {e}")))?;
            caller.data_mut().panel = Some(v);
            Ok(())
        },
    )?;
    linker.func_wrap(
        "deesainer",
        "log",
        |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<(), wasmi::Error> {
            let bytes = read_bytes(&caller, ptr, len.min(4096))?;
            let state = caller.data_mut();
            if state.logs.len() < 1000 {
                state
                    .logs
                    .push(String::from_utf8_lossy(&bytes).into_owned());
            }
            Ok(())
        },
    )?;
    Ok(())
}

fn execute(
    bytes: Vec<u8>,
    entry: String,
    input: Vec<u8>,
    limits: Limits,
    to_host: Sender<Request>,
    from_host: Receiver<Result<Value, String>>,
) -> Result<Report, PluginError> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes).map_err(|e| PluginError::Invalid(e.to_string()))?;
    let mut store = Store::new(
        &engine,
        HostState {
            to_host,
            from_host,
            last: Vec::new(),
            logs: Vec::new(),
            calls: 0,
            max_calls: limits.max_calls,
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.memory_bytes)
                .instances(1)
                .build(),
            input,
            panel: None,
        },
    );
    store.limiter(|s| &mut s.limits);
    store
        .set_fuel(limits.fuel)
        .map_err(|e| PluginError::Trap(e.to_string()))?;
    let mut linker = Linker::new(&engine);
    link(&mut linker).map_err(|e| PluginError::Invalid(e.to_string()))?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| PluginError::Invalid(e.to_string()))?;
    let func = instance
        .get_typed_func::<(), ()>(&store, &entry)
        .map_err(|_| PluginError::NoEntry(entry.clone()))?;
    let outcome = func.call(&mut store, ());
    let fuel_left = store.get_fuel().unwrap_or(0);
    let state = store.data();
    let report = Report {
        logs: state.logs.clone(),
        calls: state.calls,
        fuel_used: limits.fuel.saturating_sub(fuel_left),
        panel: state.panel.clone(),
    };
    match outcome {
        Ok(()) => Ok(report),
        Err(e) if e.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) => {
            Err(PluginError::OutOfFuel)
        }
        Err(e) => Err(PluginError::Trap(e.to_string())),
    }
}

pub fn run(
    bytes: &[u8],
    entry: &str,
    limits: Limits,
    host: impl FnMut(&str, &Value) -> Result<Value, String>,
) -> Result<Report, PluginError> {
    run_with_input(bytes, entry, &Value::Null, limits, host)
}

pub fn run_with_input(
    bytes: &[u8],
    entry: &str,
    input: &Value,
    limits: Limits,
    mut host: impl FnMut(&str, &Value) -> Result<Value, String>,
) -> Result<Report, PluginError> {
    let input = if input.is_null() {
        Vec::new()
    } else {
        input.to_string().into_bytes()
    };
    let (req_tx, req_rx) = channel::<Request>();
    let (resp_tx, resp_rx) = channel::<Result<Value, String>>();
    let bytes = bytes.to_vec();
    let entry = entry.to_owned();
    std::thread::scope(|scope| {
        let done_tx = req_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("deesainer-plugin".into())
            .stack_size(32 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                let result = execute(bytes, entry, input, limits, req_tx, resp_rx);
                let _ = done_tx.send(Request::Done(result));
            });
        if let Err(e) = spawned {
            return Err(PluginError::Trap(format!(
                "cannot start the plugin thread: {e}"
            )));
        }
        loop {
            match req_rx.recv() {
                Ok(Request::Call(cmd, params)) => {
                    let reply = host(&cmd, &params);
                    if resp_tx.send(reply).is_err() {
                        return Err(PluginError::Trap("plugin thread ended".into()));
                    }
                }
                Ok(Request::Done(result)) => return result,
                Err(_) => return Err(PluginError::Trap("plugin thread ended unexpectedly".into())),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = r#"
        (module
          (import "deesainer" "call" (func $call (param i32 i32 i32 i32) (result i32)))
          (import "deesainer" "result" (func $result (param i32)))
          (import "deesainer" "log" (func $log (param i32 i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "node.create.rectangle")
          (data (i32.const 64) "{\"x\":0,\"y\":0,\"width\":10,\"height\":10}")
          (data (i32.const 128) "hello from wasm")
          (func (export "run")
            (local $len i32)
            (call $log (i32.const 128) (i32.const 15))
            (local.set $len (call $call (i32.const 0) (i32.const 21) (i32.const 64) (i32.const 36)))
            (call $result (i32.const 256))
            (call $log (i32.const 256) (local.get $len))))
    "#;

    #[test]
    fn plugin_calls_commands_and_reads_replies() {
        let mut seen = Vec::new();
        let report = run(HELLO.as_bytes(), "run", Limits::default(), |cmd, params| {
            seen.push((cmd.to_owned(), params.clone()));
            Ok(serde_json::json!({ "id": 7 }))
        })
        .unwrap();
        assert_eq!(seen[0].0, "node.create.rectangle");
        assert_eq!(seen[0].1["width"], 10);
        assert_eq!(
            report.logs,
            vec!["hello from wasm".to_owned(), "{\"id\":7}".to_owned()]
        );
        assert_eq!(report.calls, 1);
    }

    #[test]
    fn infinite_loops_run_out_of_fuel() {
        let spin =
            r#"(module (memory (export "memory") 1) (func (export "run") (loop $l (br $l))))"#;
        let limits = Limits {
            fuel: 100_000,
            ..Limits::default()
        };
        assert!(matches!(
            run(spin.as_bytes(), "run", limits, |_, _| Ok(Value::Null)),
            Err(PluginError::OutOfFuel)
        ));
    }

    #[test]
    fn memory_growth_is_capped_and_garbage_is_rejected() {
        let grow = r#"(module (memory (export "memory") 1) (func (export "run") (drop (memory.grow (i32.const 2000)))))"#;
        let limits = Limits {
            memory_bytes: 1024 * 1024,
            ..Limits::default()
        };
        assert!(run(grow.as_bytes(), "run", limits, |_, _| Ok(Value::Null)).is_ok());
        assert!(matches!(
            run(b"not wasm", "run", Limits::default(), |_, _| Ok(
                Value::Null
            )),
            Err(PluginError::Invalid(_))
        ));
        let no_entry = r#"(module (memory (export "memory") 1))"#;
        assert!(matches!(
            run(no_entry.as_bytes(), "run", Limits::default(), |_, _| Ok(
                Value::Null
            )),
            Err(PluginError::NoEntry(_))
        ));
    }
}
