use std::collections::BTreeMap;

use deesainer_doc::{Document, NodeId};
use deesainer_ops::Transaction;
use serde_json::{json, Value};

use crate::error::EngineError;
use crate::params::Params;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Instant,
    Dialog,
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Number,
    Integer,
    Bool,
    String,
    Color,
    NodeId,
    NodeIds,
    Enum(&'static [&'static str]),
    Json,
}

impl ParamType {
    fn as_str(self) -> &'static str {
        match self {
            ParamType::Number => "number",
            ParamType::Integer => "integer",
            ParamType::Bool => "bool",
            ParamType::String => "string",
            ParamType::Color => "color",
            ParamType::NodeId => "nodeId",
            ParamType::NodeIds => "nodeIds",
            ParamType::Enum(_) => "enum",
            ParamType::Json => "json",
        }
    }

    pub fn json_schema(self) -> Value {
        match self {
            ParamType::Number => json!({"type": "number"}),
            ParamType::Integer | ParamType::NodeId => json!({"type": "integer", "minimum": 0}),
            ParamType::Bool => json!({"type": "boolean"}),
            ParamType::String => json!({"type": "string"}),
            ParamType::Color => {
                json!({"type": "string", "pattern": "^#?([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$"})
            }
            ParamType::NodeIds => {
                json!({"type": "array", "items": {"type": "integer", "minimum": 0}})
            }
            ParamType::Enum(choices) => json!({"type": "string", "enum": choices}),
            ParamType::Json => json!({}),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub name: &'static str,
    pub ty: ParamType,
    pub required: bool,
    pub doc: &'static str,
}

pub struct Context<'a> {
    pub doc: &'a Document,
    pub selection: &'a [NodeId],
    pub current_page: NodeId,
    pub can_undo: bool,
    pub can_redo: bool,
    pub clipboard: Option<&'a Value>,
    pub user: &'a str,
    pub now: String,
    pub versions: &'a [deesainer_format::Version],
}

#[derive(Debug)]
pub enum Effect {
    None,
    Apply(Transaction),
    Replace(Box<Document>, Vec<deesainer_format::Version>),
    Undo,
    Redo,
    RunPlugin {
        module: Vec<u8>,
        entry: String,
        input: Value,
    },
    SaveVersion {
        label: String,
    },
    RestoreVersion(u64),
    DeleteVersion(u64),
}

#[derive(Debug)]
pub struct Outcome {
    pub effect: Effect,
    pub reply: Value,
    pub select: Option<Vec<NodeId>>,
    pub page: Option<NodeId>,
    pub clipboard: Option<Value>,
}

impl Outcome {
    pub fn read(reply: Value) -> Self {
        Self {
            effect: Effect::None,
            reply,
            select: None,
            page: None,
            clipboard: None,
        }
    }

    pub fn apply(tx: Transaction, reply: Value) -> Self {
        Self {
            effect: Effect::Apply(tx),
            ..Self::read(reply)
        }
    }

    pub fn effect(effect: Effect) -> Self {
        Self {
            effect,
            ..Self::read(Value::Null)
        }
    }

    pub fn selecting(mut self, ids: Vec<NodeId>) -> Self {
        self.select = Some(ids);
        self
    }

    pub fn on_page(mut self, page: NodeId) -> Self {
        self.page = Some(page);
        self
    }
}

pub type RunFn = fn(&Context<'_>, &Params<'_>) -> Result<Outcome, EngineError>;
pub type EnabledFn = fn(&Context<'_>) -> bool;

#[derive(Clone, Copy)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub menu_path: &'static str,
    pub shortcut: Option<&'static str>,
    pub doc: &'static str,
    pub params: &'static [ParamSpec],
    pub kind: CommandKind,
    pub read_only: bool,
    pub enabled: EnabledFn,
    pub run: RunFn,
}

impl CommandSpec {
    pub const fn new(id: &'static str, label: &'static str, run: RunFn) -> Self {
        Self {
            id,
            label,
            menu_path: "",
            shortcut: None,
            doc: "",
            params: &[],
            kind: CommandKind::Instant,
            read_only: false,
            enabled: always,
            run,
        }
    }

    pub const fn menu(mut self, path: &'static str) -> Self {
        self.menu_path = path;
        self
    }

    pub const fn key(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub const fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    pub const fn params(mut self, params: &'static [ParamSpec]) -> Self {
        self.params = params;
        self
    }

    pub const fn kind(mut self, kind: CommandKind) -> Self {
        self.kind = kind;
        self
    }

    pub const fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub const fn enabled(mut self, f: EnabledFn) -> Self {
        self.enabled = f;
        self
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "label": self.label,
            "menuPath": self.menu_path,
            "shortcut": self.shortcut,
            "doc": self.doc,
            "kind": match self.kind {
                CommandKind::Instant => "instant",
                CommandKind::Dialog => "dialog",
                CommandKind::Interactive => "interactive",
            },
            "readOnly": self.read_only,
            "params": self.params.iter().map(|p| {
                let mut v = json!({
                    "name": p.name,
                    "type": p.ty.as_str(),
                    "required": p.required,
                    "doc": p.doc,
                });
                if let ParamType::Enum(choices) = p.ty {
                    v["choices"] = json!(choices);
                }
                v
            }).collect::<Vec<_>>(),
        })
    }

    pub fn input_schema(&self) -> Value {
        let mut props = serde_json::Map::new();
        for p in self.params {
            let mut schema = p.ty.json_schema();
            if let Value::Object(map) = &mut schema {
                map.insert("description".into(), Value::String(p.doc.to_owned()));
            }
            props.insert(p.name.to_owned(), schema);
        }
        let required: Vec<&str> = self
            .params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name)
            .collect();
        json!({
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false,
        })
    }
}

pub fn always(_: &Context<'_>) -> bool {
    true
}

pub fn has_selection(ctx: &Context<'_>) -> bool {
    !ctx.selection.is_empty()
}

#[derive(Clone, Default)]
pub struct CommandRegistry {
    specs: Vec<CommandSpec>,
    index: BTreeMap<&'static str, usize>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtin() -> Result<Self, EngineError> {
        let mut registry = Self::new();
        for spec in crate::commands::specs() {
            registry.register(spec)?;
        }
        Ok(registry)
    }

    pub fn register(&mut self, spec: CommandSpec) -> Result<(), EngineError> {
        if self.index.contains_key(spec.id) {
            return Err(EngineError::DuplicateCommand(spec.id.to_owned()));
        }
        self.index.insert(spec.id, self.specs.len());
        self.specs.push(spec);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&CommandSpec> {
        self.index.get(id).and_then(|i| self.specs.get(*i))
    }

    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.specs.iter()
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.specs.iter().map(CommandSpec::to_json).collect())
    }
}
