use deesainer_color::Rgba;
use deesainer_doc::NodeId;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::error::EngineError;
use crate::registry::{Context, ParamSpec};

#[derive(Debug)]
pub struct Params<'a> {
    command: &'a str,
    map: Option<&'a Map<String, Value>>,
}

impl<'a> Params<'a> {
    pub fn parse(
        command: &'a str,
        value: &'a Value,
        specs: &[ParamSpec],
    ) -> Result<Self, EngineError> {
        let map = match value {
            Value::Null => None,
            Value::Object(map) => Some(map),
            _ => {
                return Err(EngineError::ParamsNotObject {
                    command: command.to_owned(),
                })
            }
        };
        if let Some(map) = map {
            for key in map.keys() {
                if !specs.iter().any(|s| s.name == key) {
                    let accepted: Vec<&str> = specs.iter().map(|s| s.name).collect();
                    return Err(EngineError::UnknownParam {
                        command: command.to_owned(),
                        key: key.clone(),
                        accepted: if accepted.is_empty() {
                            "none".to_owned()
                        } else {
                            accepted.join(", ")
                        },
                    });
                }
            }
        }
        let params = Self { command, map };
        for spec in specs.iter().filter(|s| s.required) {
            if params.raw(spec.name).is_none() {
                return Err(params.missing(spec.name));
            }
        }
        Ok(params)
    }

    pub fn command(&self) -> &str {
        self.command
    }

    pub fn raw(&self, key: &str) -> Option<&'a Value> {
        self.map.and_then(|m| m.get(key)).filter(|v| !v.is_null())
    }

    pub fn given(&self, key: &str) -> Option<&'a Value> {
        self.map.and_then(|m| m.get(key))
    }

    pub fn has(&self, key: &str) -> bool {
        self.raw(key).is_some()
    }

    pub fn missing(&self, key: &str) -> EngineError {
        EngineError::MissingParam {
            command: self.command.to_owned(),
            key: key.to_owned(),
        }
    }

    pub fn bad(&self, key: &str, reason: impl Into<String>) -> EngineError {
        EngineError::BadParam {
            command: self.command.to_owned(),
            key: key.to_owned(),
            reason: reason.into(),
        }
    }

    pub fn invalid(&self, reason: impl Into<String>) -> EngineError {
        EngineError::Invalid {
            command: self.command.to_owned(),
            reason: reason.into(),
        }
    }

    pub fn f64(&self, key: &str) -> Result<Option<f64>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        match v.as_f64() {
            Some(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(self.bad(key, "must be a finite number")),
        }
    }

    pub fn req_f64(&self, key: &str) -> Result<f64, EngineError> {
        self.f64(key)?.ok_or_else(|| self.missing(key))
    }

    pub fn non_negative(&self, key: &str) -> Result<Option<f64>, EngineError> {
        match self.f64(key)? {
            Some(n) if n < 0.0 => Err(self.bad(key, "must be >= 0")),
            other => Ok(other),
        }
    }

    pub fn u32(&self, key: &str) -> Result<Option<u32>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        v.as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| self.bad(key, "must be a non-negative integer"))
    }

    pub fn bool(&self, key: &str) -> Result<Option<bool>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        v.as_bool()
            .map(Some)
            .ok_or_else(|| self.bad(key, "must be true or false"))
    }

    pub fn str(&self, key: &str) -> Result<Option<&'a str>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        v.as_str()
            .map(Some)
            .ok_or_else(|| self.bad(key, "must be a string"))
    }

    pub fn req_str(&self, key: &str) -> Result<&'a str, EngineError> {
        self.str(key)?.ok_or_else(|| self.missing(key))
    }

    pub fn choice(&self, key: &str, choices: &[&str]) -> Result<Option<&'a str>, EngineError> {
        match self.str(key)? {
            Some(s) if choices.contains(&s) => Ok(Some(s)),
            Some(_) => Err(self.bad(key, format!("must be one of: {}", choices.join(", ")))),
            None => Ok(None),
        }
    }

    pub fn color(&self, key: &str) -> Result<Option<Rgba>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        if let Some(obj) = v.as_object() {
            let channel = |c: &str, default: Option<f64>| -> Result<f32, EngineError> {
                match obj.get(c).map(Value::as_f64) {
                    Some(Some(x)) if (0.0..=1.0).contains(&x) => Ok(x as f32),
                    None if default.is_some() => Ok(default.unwrap_or(1.0) as f32),
                    _ => Err(self.bad(
                        key,
                        "must be a hex string like \"#1A2B3C\" or {r, g, b, a?} with channels from 0 to 1",
                    )),
                }
            };
            return Ok(Some(Rgba::new(
                channel("r", None)?,
                channel("g", None)?,
                channel("b", None)?,
                channel("a", Some(1.0))?,
            )));
        }
        let s = v.as_str().ok_or_else(|| {
            self.bad(
                key,
                "must be a hex string like \"#1A2B3C\" or {r, g, b, a?} with channels from 0 to 1",
            )
        })?;
        Rgba::from_hex(s)
            .map(Some)
            .map_err(|e| self.bad(key, e.to_string()))
    }

    pub fn node_id(&self, key: &str) -> Result<Option<NodeId>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        v.as_u64()
            .map(|n| Some(NodeId(n)))
            .ok_or_else(|| self.bad(key, "must be a node id (non-negative integer)"))
    }

    pub fn req_node_id(&self, key: &str) -> Result<NodeId, EngineError> {
        self.node_id(key)?.ok_or_else(|| self.missing(key))
    }

    pub fn node_ids(&self, key: &str) -> Result<Option<Vec<NodeId>>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        let items = v
            .as_array()
            .ok_or_else(|| self.bad(key, "must be an array of node ids"))?;
        items
            .iter()
            .map(|item| {
                item.as_u64()
                    .map(NodeId)
                    .ok_or_else(|| self.bad(key, "must contain only node ids"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    pub fn req_node_ids(&self, key: &str) -> Result<Vec<NodeId>, EngineError> {
        self.node_ids(key)?.ok_or_else(|| self.missing(key))
    }

    pub fn ids_or_selection(&self, ctx: &Context<'_>) -> Result<Vec<NodeId>, EngineError> {
        let ids = match self.node_ids("ids")? {
            Some(ids) => ids,
            None => ctx.selection.to_vec(),
        };
        if ids.is_empty() {
            return Err(self.invalid("nothing selected; pass ids or select nodes first"));
        }
        for id in &ids {
            if !ctx.doc.contains(*id) {
                return Err(self.bad("ids", format!("{id} does not exist")));
            }
        }
        Ok(ids)
    }

    pub fn json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, EngineError> {
        let Some(v) = self.raw(key) else {
            return Ok(None);
        };
        let mut v = v.clone();
        hex_colors_to_objects(&mut v, 0);
        serde_json::from_value(v)
            .map(Some)
            .map_err(|e| self.bad(key, format!("has the wrong shape: {e}")))
    }

    pub fn req_json<T: DeserializeOwned>(&self, key: &str) -> Result<T, EngineError> {
        self.json(key)?.ok_or_else(|| self.missing(key))
    }
}

fn hex_colors_to_objects(v: &mut Value, depth: usize) {
    if depth > 32 {
        return;
    }
    match v {
        Value::Object(map) => {
            for (k, child) in map.iter_mut() {
                if k == "color" {
                    if let Some(c) = child.as_str().and_then(|s| Rgba::from_hex(s.trim()).ok()) {
                        *child = serde_json::json!({ "r": c.r, "g": c.g, "b": c.b, "a": c.a });
                        continue;
                    }
                }
                hex_colors_to_objects(child, depth + 1);
            }
        }
        Value::Array(items) => {
            for item in items {
                hex_colors_to_objects(item, depth + 1);
            }
        }
        _ => {}
    }
}
