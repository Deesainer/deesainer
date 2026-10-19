use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use deesainer_doc::{Document, NodeId};
use deesainer_ops::{History, Op, Transaction};
use serde_json::{json, Value};

use crate::derive;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandRegistry, Context, Effect, Outcome};

pub struct Session {
    doc: Arc<Document>,
    history: History,
    registry: Arc<CommandRegistry>,
    selection: Vec<NodeId>,
    current_page: NodeId,
    clipboard: Option<Value>,
    revision: u64,
    outbox: Vec<Op>,
    record_outbox: bool,
    user: String,
    clock: Option<fn() -> String>,
    versions: Vec<deesainer_format::Version>,
}

impl Session {
    pub fn new() -> Result<Self, EngineError> {
        Ok(Self::with_document(
            Document::new(),
            Arc::new(CommandRegistry::builtin()?),
        ))
    }

    pub fn with_document(doc: Document, registry: Arc<CommandRegistry>) -> Self {
        let current_page = doc.first_page();
        Self {
            doc: Arc::new(doc),
            history: History::new(),
            registry,
            selection: Vec::new(),
            current_page,
            clipboard: None,
            revision: 0,
            outbox: Vec::new(),
            record_outbox: false,
            user: "You".to_owned(),
            clock: None,
            versions: Vec::new(),
        }
    }

    pub fn versions(&self) -> &[deesainer_format::Version] {
        &self.versions
    }

    pub fn set_versions(&mut self, mut versions: Vec<deesainer_format::Version>) {
        versions.truncate(deesainer_format::MAX_VERSIONS);
        if let Some(max) = versions.iter().map(|v| v.id).max() {
            Arc::make_mut(&mut self.doc).reserve_ids(max.saturating_add(1));
        }
        self.versions = versions;
    }

    fn push_version(&mut self, label: String) -> u64 {
        let id = self.doc.next_free_id().0;
        Arc::make_mut(&mut self.doc).reserve_ids(id.saturating_add(1));
        let now = self.clock.map(|c| c()).unwrap_or_default();
        self.versions.push(deesainer_format::Version {
            id,
            label,
            saved_at: now,
            author: self.user.clone(),
            document: Arc::clone(&self.doc),
        });
        if self.versions.len() > deesainer_format::MAX_VERSIONS {
            self.versions.remove(0);
        }
        self.revision += 1;
        id
    }

    pub fn set_user(&mut self, name: &str) {
        self.user = name.to_owned();
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn set_clock(&mut self, clock: fn() -> String) {
        self.clock = Some(clock);
    }

    pub fn snapshot(&self) -> Arc<Document> {
        Arc::clone(&self.doc)
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn registry(&self) -> &CommandRegistry {
        &self.registry
    }

    pub fn registry_arc(&self) -> Arc<CommandRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn selection(&self) -> &[NodeId] {
        &self.selection
    }

    pub fn current_page(&self) -> NodeId {
        self.current_page
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }

    pub fn begin_group(&mut self, label: &str) {
        self.history.begin_group(label);
    }

    pub fn end_group(&mut self) {
        self.history.end_group();
    }

    pub fn set_record_outbox(&mut self, on: bool) {
        self.record_outbox = on;
        self.outbox.clear();
    }

    pub fn take_outbox(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.outbox)
    }

    pub fn replace_document(&mut self, doc: Document, keep_history: bool) {
        self.doc = Arc::new(doc);
        if !keep_history {
            self.history.clear();
        }
        self.fix_view_state();
        self.revision += 1;
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.registry
            .get(id)
            .is_some_and(|spec| (spec.enabled)(&self.context()))
    }

    pub fn clipboard(&self) -> Option<&Value> {
        self.clipboard.as_ref()
    }

    fn context(&self) -> Context<'_> {
        Context {
            doc: &self.doc,
            selection: &self.selection,
            current_page: self.current_page,
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            clipboard: self.clipboard.as_ref(),
            user: &self.user,
            now: self.clock.map(|c| c()).unwrap_or_default(),
            versions: &self.versions,
        }
    }

    pub fn dispatch(&mut self, id: &str, params: &Value) -> Result<Value, EngineError> {
        let registry = Arc::clone(&self.registry);
        let spec = registry
            .get(id)
            .ok_or_else(|| EngineError::UnknownCommand(id.to_owned()))?;
        let outcome = {
            let ctx = self.context();
            if !(spec.enabled)(&ctx) {
                return Err(EngineError::Disabled(id.to_owned()));
            }
            let params = Params::parse(spec.id, params, spec.params)?;
            catch_unwind(AssertUnwindSafe(|| (spec.run)(&ctx, &params)))
                .map_err(|payload| EngineError::Internal(panic_message(&*payload)))??
        };
        self.apply_outcome(outcome)
    }

    pub fn dispatch_batch(
        &mut self,
        label: &str,
        steps: &[(String, Value)],
    ) -> Result<Vec<Value>, (usize, EngineError)> {
        self.history.begin_group(label);
        let mut replies = Vec::with_capacity(steps.len());
        let mut failure = None;
        for (i, (id, params)) in steps.iter().enumerate() {
            let resolved = match resolve_step_refs(id, params, &replies) {
                Ok(v) => v,
                Err(e) => {
                    failure = Some((i, e));
                    break;
                }
            };
            match self.dispatch(id, &resolved) {
                Ok(v) => replies.push(v),
                Err(e) => {
                    failure = Some((i, e));
                    break;
                }
            }
        }
        self.history.end_group();
        match failure {
            Some(f) => Err(f),
            None => Ok(replies),
        }
    }

    fn apply_outcome(&mut self, outcome: Outcome) -> Result<Value, EngineError> {
        let mut reply = outcome.reply;
        match outcome.effect {
            Effect::None => {}
            Effect::Apply(tx) => {
                if !tx.is_empty() {
                    self.commit(&tx)?;
                }
            }
            Effect::Replace(doc, versions) => {
                self.doc = Arc::new(*doc);
                self.versions.clear();
                self.set_versions(versions);
                self.history.clear();
                self.selection.clear();
                self.current_page = self.doc.first_page();
                self.revision += 1;
            }
            Effect::Undo => {
                let tx = self.history.undo(Arc::make_mut(&mut self.doc))?;
                reply = json!({ "undone": self.after_history_step(tx) });
            }
            Effect::RunPlugin {
                module,
                entry,
                input,
            } => {
                reply = self.run_plugin_with(&module, &entry, &input)?;
            }
            Effect::Redo => {
                let tx = self.history.redo(Arc::make_mut(&mut self.doc))?;
                reply = json!({ "redone": self.after_history_step(tx) });
            }
            Effect::SaveVersion { label } => {
                let id = self.push_version(label);
                reply = json!({ "id": id });
            }
            Effect::RestoreVersion(id) => {
                let Some(v) = self.versions.iter().find(|v| v.id == id).cloned() else {
                    return Err(EngineError::Invalid {
                        command: "version.restore".into(),
                        reason: format!("version {id} does not exist"),
                    });
                };
                let before = self.push_version(format!("Before restoring \"{}\"", v.label));
                let mut doc = (*v.document).clone();
                doc.reserve_ids(self.doc.next_free_id().0);
                self.doc = Arc::new(doc);
                self.history.clear();
                self.selection.clear();
                if !self.doc.contains(self.current_page) {
                    self.current_page = self.doc.first_page();
                }
                self.revision += 1;
                reply = json!({ "restored": id, "saved": before });
            }
            Effect::DeleteVersion(id) => {
                let n = self.versions.len();
                self.versions.retain(|v| v.id != id);
                if self.versions.len() == n {
                    return Err(EngineError::Invalid {
                        command: "version.delete".into(),
                        reason: format!("version {id} does not exist"),
                    });
                }
                self.revision += 1;
            }
        }
        if let Some(clip) = outcome.clipboard {
            self.clipboard = Some(clip);
        }
        if let Some(page) = outcome.page {
            if self.doc.contains(page) {
                self.current_page = page;
            }
        }
        if let Some(sel) = outcome.select {
            self.selection = sel;
        }
        self.fix_view_state();
        Ok(reply)
    }

    fn commit(&mut self, tx: &Transaction) -> Result<(), EngineError> {
        let tx = &crate::components::capture_overrides(&self.doc, tx);
        let sizes = crate::components::main_sizes(&self.doc);
        let doc = Arc::make_mut(&mut self.doc);
        self.history.commit(doc, tx)?;
        if self.record_outbox {
            self.outbox.extend(tx.ops.iter().cloned());
        }
        for round in 0..2 {
            for pass in derive::PASSES {
                let ops = pass(doc);
                if self.record_outbox {
                    self.outbox.extend(ops.iter().cloned());
                }
                self.history.amend(doc, ops)?;
            }
            let follow = crate::components::follow_main_size(&sizes, doc);
            if round == 1 || follow.is_empty() {
                break;
            }
            if self.record_outbox {
                self.outbox.extend(follow.iter().cloned());
            }
            self.history.amend(doc, follow)?;
        }
        self.revision += 1;
        Ok(())
    }

    pub fn apply_remote(&mut self, ops: &[Op]) -> Result<(), EngineError> {
        let doc = Arc::make_mut(&mut self.doc);
        Transaction::new("remote", ops.to_vec()).apply(doc)?;
        self.fix_view_state();
        self.revision += 1;
        Ok(())
    }

    pub fn run_plugin(&mut self, module: &[u8], entry: &str) -> Result<Value, EngineError> {
        self.run_plugin_with(module, entry, &Value::Null)
    }

    pub fn run_plugin_with(
        &mut self,
        module: &[u8],
        entry: &str,
        input: &Value,
    ) -> Result<Value, EngineError> {
        self.history.begin_group("Plugin");
        let result = deesainer_plugins::run_with_input(
            module,
            entry,
            input,
            deesainer_plugins::Limits::default(),
            |cmd, params| {
                if cmd.starts_with("plugin.") {
                    return Err("plugins cannot run plugins".to_owned());
                }
                self.dispatch(cmd, params).map_err(|e| e.to_string())
            },
        );
        self.history.end_group();
        match result {
            Ok(report) => Ok(serde_json::to_value(report).unwrap_or(Value::Null)),
            Err(e) => Err(EngineError::Invalid {
                command: "plugin.run".into(),
                reason: e.to_string(),
            }),
        }
    }

    fn after_history_step(&mut self, tx: Option<Transaction>) -> Option<String> {
        let tx = tx?;
        if self.record_outbox {
            self.outbox.extend(tx.ops);
        }
        self.revision += 1;
        Some(tx.label)
    }

    fn fix_view_state(&mut self) {
        let doc = &self.doc;
        if !doc
            .get(self.current_page)
            .is_some_and(|n| matches!(n.kind, deesainer_doc::NodeKind::Page))
        {
            self.current_page = doc.first_page();
        }
        self.selection.retain(|id| doc.contains(*id));
        self.selection.dedup();
    }
}

fn resolve_step_refs(
    command: &str,
    params: &Value,
    replies: &[Value],
) -> Result<Value, EngineError> {
    fn walk(v: &Value, replies: &[Value], depth: usize) -> Result<Value, String> {
        if depth > 64 {
            return Err("params nest too deeply".into());
        }
        match v {
            Value::String(s) => {
                let Some(n) = s.strip_prefix('$').and_then(|d| d.parse::<usize>().ok()) else {
                    return Ok(v.clone());
                };
                let reply = replies
                    .get(n)
                    .ok_or_else(|| format!("{s:?} refers to a step that has not run yet"))?;
                reply
                    .get("id")
                    .cloned()
                    .ok_or_else(|| format!("{s:?}: step {n} returned no id"))
            }
            Value::Array(items) => items
                .iter()
                .map(|i| walk(i, replies, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(map) => map
                .iter()
                .map(|(k, i)| Ok((k.clone(), walk(i, replies, depth + 1)?)))
                .collect::<Result<serde_json::Map<_, _>, String>>()
                .map(Value::Object),
            _ => Ok(v.clone()),
        }
    }
    walk(params, replies, 0).map_err(|reason| EngineError::Invalid {
        command: command.to_owned(),
        reason,
    })
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "command panicked".to_owned()
    }
}

impl deesainer_sync::client::RoomHost for Session {
    fn take_outbox(&mut self) -> Vec<Op> {
        Session::take_outbox(self)
    }

    fn replace_document(&mut self, doc: Document, keep_view: bool) {
        Session::replace_document(self, doc, keep_view);
    }

    fn set_record_outbox(&mut self, on: bool) {
        Session::set_record_outbox(self, on);
    }

    fn current_page(&self) -> NodeId {
        Session::current_page(self)
    }

    fn selection(&self) -> Vec<NodeId> {
        Session::selection(self).to_vec()
    }
}
