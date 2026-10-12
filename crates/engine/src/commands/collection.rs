use deesainer_doc::{CollectionId, Modes, NodeKind, PropValue, VariableId};
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::style::{collection_param, scopes_param};
use super::{layer_ids, param, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

const MAX_COLLECTIONS: usize = 256;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("collection.create", "Create collection", create)
            .doc("Creates a variable collection with its own modes (default one mode, \"Mode 1\"). Returns {id}.")
            .params(const {
                &[
                    param("name", ParamType::String, true, "Collection name."),
                    param("modes", ParamType::Json, false, "Mode names, e.g. [\"Light\", \"Dark\"]; at most 40."),
                ]
            }),
        CommandSpec::new("collection.rename", "Rename collection", rename)
            .doc("Renames a collection (0 is the default collection).")
            .params(const {
                &[
                    param("collection", ParamType::Integer, true, "Collection id."),
                    param("name", ParamType::String, true, "New name."),
                ]
            }),
        CommandSpec::new("collection.delete", "Delete collection", delete)
            .doc("Deletes a collection other than the default one with its variables; bound layers keep their values and are unbound, and frames stop choosing its modes.")
            .params(const { &[param("collection", ParamType::Integer, true, "Collection id (not 0).")] }),
        CommandSpec::new("variable.setScopes", "Variable scopes", set_scopes)
            .doc("Sets which properties a variable can be bound to; an empty list allows all.")
            .params(const {
                &[
                    param("id", ParamType::Integer, true, "Variable id."),
                    param("scopes", ParamType::Json, true, "Bind targets, e.g. [\"cornerRadius\"]."),
                ]
            }),
        CommandSpec::new("node.setVariableMode", "Variable mode", set_variable_mode)
            .doc("Makes layers (and their descendants) use one mode of a collection, whatever its default; mode null goes back to the inherited mode.")
            .params(const {
                &[
                    IDS,
                    param("collection", ParamType::Integer, false, "Collection id; default 0."),
                    param("mode", ParamType::Json, false, "Mode index, or null (required)."),
                ]
            }),
    ]
}

fn create(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    if ctx.doc.collections().count() > MAX_COLLECTIONS {
        return Err(p.invalid(format!("at most {MAX_COLLECTIONS} collections")));
    }
    let names: Vec<String> = match p.json::<Vec<String>>("modes")? {
        Some(n) if n.is_empty() || n.len() > 40 => {
            return Err(p.bad("modes", "needs 1 to 40 names"))
        }
        Some(n) => n,
        None => vec!["Mode 1".to_owned()],
    };
    let id = CollectionId(ctx.doc.next_free_id().0);
    let modes = Modes {
        name: p.req_str("name")?.to_owned(),
        names,
        active: 0,
    };
    Ok(Outcome::apply(
        Transaction::new(
            "Create collection",
            vec![Op::SetCollection {
                id,
                modes: Some(modes),
            }],
        ),
        json!({ "id": id }),
    ))
}

fn rename(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, mut modes) = collection_param(ctx, p)?;
    modes.name = p.req_str("name")?.to_owned();
    Ok(Outcome::apply(
        Transaction::new(
            "Rename collection",
            vec![super::style::set_modes_op(id, modes)],
        ),
        json!({}),
    ))
}

fn delete(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, _) = collection_param(ctx, p)?;
    if id.is_default() {
        return Err(p.bad("collection", "the default collection cannot be deleted"));
    }
    let doomed: Vec<VariableId> = ctx.doc.collection_variables(id).into_keys().collect();
    let mut ops = Vec::new();
    for node in ctx.doc.nodes() {
        if node.bindings.values().any(|v| doomed.contains(v)) {
            let mut b = node.bindings.clone();
            b.retain(|_, v| !doomed.contains(v));
            ops.push(Op::set(node.id, PropValue::Bindings(b)));
        }
        if node.variable_modes.contains_key(&id) {
            let mut m = node.variable_modes.clone();
            m.remove(&id);
            ops.push(Op::set(node.id, PropValue::VariableModes(m)));
        }
    }
    ops.push(Op::SetCollection { id, modes: None });
    Ok(Outcome::apply(
        Transaction::new("Delete collection", ops),
        json!({}),
    ))
}

fn set_scopes(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let raw = p
        .raw("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| p.missing("id"))?;
    let id = VariableId(raw);
    let mut var = ctx
        .doc
        .variable(id)
        .cloned()
        .ok_or_else(|| p.bad("id", format!("variable {raw} does not exist")))?;
    var.scopes = scopes_param(p)?.ok_or_else(|| p.missing("scopes"))?;
    Ok(Outcome::apply(
        Transaction::new(
            "Variable scopes",
            vec![Op::SetVariable {
                id,
                variable: Some(var),
            }],
        ),
        json!({}),
    ))
}

fn set_variable_mode(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let (collection, modes) = collection_param(ctx, p)?;
    let mode = match p.given("mode") {
        Some(Value::Null) => None,
        Some(v) => {
            let m = v
                .as_u64()
                .filter(|m| (*m as usize) < modes.names.len())
                .ok_or_else(|| p.bad("mode", "must be a mode index of the collection, or null"))?;
            Some(m as usize)
        }
        None => return Err(p.missing("mode")),
    };
    let mut ops = Vec::new();
    for id in ids {
        let node = ctx
            .doc
            .get(id)
            .ok_or_else(|| p.bad("ids", format!("{id} does not exist")))?;
        if matches!(node.kind, NodeKind::Document) {
            return Err(p.bad("ids", "the document has no mode"));
        }
        let mut m = node.variable_modes.clone();
        match mode {
            Some(v) => m.insert(collection, v),
            None => m.remove(&collection),
        };
        if m != node.variable_modes {
            ops.push(Op::set(id, PropValue::VariableModes(m)));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Variable mode", ops),
        json!({}),
    ))
}
