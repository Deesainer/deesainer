use std::collections::{BTreeMap, BTreeSet};

use deesainer_doc::{Node, NodeId, NodeKind};
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::{clone_subtrees, collect_subtrees, layer_ids, param, top_level, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Effect, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("edit.undo", "Undo", |_, _| Ok(Outcome::effect(Effect::Undo)))
            .menu("Edit/Undo")
            .key("Cmd+Z")
            .enabled(|ctx| ctx.can_undo)
            .doc("Undoes the last history step. Returns {undone: label}."),
        CommandSpec::new("edit.redo", "Redo", |_, _| Ok(Outcome::effect(Effect::Redo)))
            .menu("Edit/Redo")
            .key("Shift+Cmd+Z")
            .enabled(|ctx| ctx.can_redo)
            .doc("Redoes the last undone step. Returns {redone: label}."),
        CommandSpec::new("edit.delete", "Delete", edit_delete)
            .menu("Edit/Delete")
            .key("Delete")
            .params(const { &[IDS] })
            .doc("Deletes layers and everything inside them. Pages are deleted with page.delete."),
        CommandSpec::new("edit.duplicate", "Duplicate", edit_duplicate)
            .menu("Edit/Duplicate")
            .key("Cmd+D")
            .params(const {
                &[
                    IDS,
                    param("dx", ParamType::Number, false, "Horizontal offset of the copies, default 0."),
                    param("dy", ParamType::Number, false, "Vertical offset of the copies, default 0."),
                ]
            })
            .doc("Duplicates layers right above the originals and selects the copies. Returns {ids}."),
        CommandSpec::new("edit.copy", "Copy", edit_copy)
            .menu("Edit/Copy")
            .key("Cmd+C")
            .params(const { &[IDS] })
            .doc("Copies layers to the session clipboard. Returns the clipboard JSON (portable between sessions via edit.paste {data})."),
        CommandSpec::new("edit.cut", "Cut", edit_cut)
            .menu("Edit/Cut")
            .key("Cmd+X")
            .params(const { &[IDS] })
            .doc("Copies layers to the clipboard and deletes them."),
        CommandSpec::new("edit.paste", "Paste", edit_paste)
            .menu("Edit/Paste")
            .key("Cmd+V")
            .params(const {
                &[
                    param("parent", ParamType::NodeId, false, "Where to paste; defaults to the selected container, else the current page."),
                    param("data", ParamType::Json, false, "Clipboard JSON from edit.copy; defaults to the session clipboard."),
                    param("dx", ParamType::Number, false, "Horizontal offset, default 0."),
                    param("dy", ParamType::Number, false, "Vertical offset, default 0."),
                ]
            })
            .doc("Pastes copied layers with fresh ids at their original positions (plus the offset). Returns {ids}."),
        CommandSpec::new("node.rename", "Rename", node_rename)
            .key("Cmd+R")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer."),
                    param("name", ParamType::String, true, "New name."),
                ]
            })
            .doc("Renames a layer."),
    ]
}

pub(crate) fn delete_ops(ctx: &Context<'_>, ids: &[NodeId]) -> Result<Vec<Op>, EngineError> {
    let mut seen = BTreeSet::new();
    let mut ops = Vec::new();
    for id in top_level(ctx.doc, ids) {
        for victim in ctx.doc.subtree_post_order(id)? {
            if seen.insert(victim) {
                ops.push(Op::DeleteNode { id: victim });
            }
        }
    }
    Ok(ops)
}

fn edit_delete(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let ops = delete_ops(ctx, &ids)?;
    Ok(Outcome::apply(Transaction::new("Delete", ops), json!({})).selecting(Vec::new()))
}

fn edit_duplicate(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    let offset = (p.f64("dx")?.unwrap_or(0.0), p.f64("dy")?.unwrap_or(0.0));
    let nodes = collect_subtrees(ctx.doc, &ids)?;
    let mut ops = Vec::new();
    let mut new_ids = Vec::new();
    let mut sorted: Vec<(NodeId, usize, NodeId)> = ids
        .iter()
        .map(|id| {
            let parent = ctx.doc.node(*id)?.parent.unwrap_or(ctx.current_page);
            Ok((parent, ctx.doc.index_in_parent(*id)?, *id))
        })
        .collect::<Result<_, EngineError>>()?;
    sorted.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut scratch = ctx.doc.clone();
    for (parent, index, id) in sorted {
        let (sub_ops, roots) = clone_subtrees(&scratch, &nodes, &[id], parent, index + 1, offset)?;
        Transaction::new("", sub_ops.clone()).apply(&mut scratch)?;
        ops.extend(sub_ops);
        new_ids.extend(roots);
    }
    Ok(Outcome::apply(
        Transaction::new("Duplicate", ops),
        json!({ "ids": new_ids }),
    )
    .selecting(new_ids))
}

fn clipboard_json(ctx: &Context<'_>, ids: &[NodeId]) -> Result<Value, EngineError> {
    let roots = top_level(ctx.doc, ids);
    let nodes: Vec<Node> = collect_subtrees(ctx.doc, &roots)?.into_values().collect();
    Ok(json!({ "deesainer": 1, "roots": roots, "nodes": nodes }))
}

fn edit_copy(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let clip = clipboard_json(ctx, &ids)?;
    let mut out = Outcome::read(clip.clone());
    out.clipboard = Some(clip);
    Ok(out)
}

fn edit_cut(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let clip = clipboard_json(ctx, &ids)?;
    let ops = delete_ops(ctx, &ids)?;
    let mut out = Outcome::apply(Transaction::new("Cut", ops), clip.clone()).selecting(Vec::new());
    out.clipboard = Some(clip);
    Ok(out)
}

fn edit_paste(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let data = match p.raw("data") {
        Some(v) => v.clone(),
        None => ctx
            .clipboard
            .cloned()
            .ok_or_else(|| p.invalid("the clipboard is empty"))?,
    };
    let roots: Vec<NodeId> =
        serde_json::from_value(data.get("roots").cloned().unwrap_or(Value::Null))
            .map_err(|e| p.bad("data", format!("roots: {e}")))?;
    let list: Vec<Node> = serde_json::from_value(data.get("nodes").cloned().unwrap_or(Value::Null))
        .map_err(|e| p.bad("data", format!("nodes: {e}")))?;
    let nodes: BTreeMap<NodeId, Node> = list.into_iter().map(|n| (n.id, n)).collect();
    for r in &roots {
        match nodes.get(r).map(|n| &n.kind) {
            Some(NodeKind::Page | NodeKind::Document) | None => {
                return Err(p.bad("data", "roots must be layers present in nodes"))
            }
            _ => {}
        }
    }
    let parent = match p.node_id("parent")? {
        Some(id) => id,
        None => match ctx.selection {
            [one]
                if ctx.doc.get(*one).is_some_and(|n| {
                    n.kind.is_container() && !matches!(n.kind, NodeKind::Document)
                }) =>
            {
                *one
            }
            _ => ctx.current_page,
        },
    };
    let pnode = ctx
        .doc
        .get(parent)
        .ok_or_else(|| p.bad("parent", format!("{parent} does not exist")))?;
    if !pnode.kind.is_container() || matches!(pnode.kind, NodeKind::Document) {
        return Err(p.bad("parent", "must be a page or container"));
    }
    let offset = (p.f64("dx")?.unwrap_or(0.0), p.f64("dy")?.unwrap_or(0.0));
    let (ops, new_ids) = clone_subtrees(
        ctx.doc,
        &nodes,
        &roots,
        parent,
        pnode.children.len(),
        offset,
    )?;
    Ok(
        Outcome::apply(Transaction::new("Paste", ops), json!({ "ids": new_ids }))
            .selecting(new_ids),
    )
}

fn node_rename(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    if !ctx.doc.contains(id) {
        return Err(p.bad("id", format!("{id} does not exist")));
    }
    let name = p.req_str("name")?.to_owned();
    Ok(Outcome::apply(
        Transaction::new(
            "Rename",
            vec![Op::set(id, deesainer_doc::PropValue::Name(name))],
        ),
        json!({}),
    ))
}
