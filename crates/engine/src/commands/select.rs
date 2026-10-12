use deesainer_doc::{NodeId, NodeKind};
use serde_json::json;

use super::{layer_ids, param};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{has_selection, CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("selection.set", "Select", selection_set)
            .doc("Replaces the selection. Pass an empty array to clear. Switches to the page of the first node.")
            .params(const { &[param("ids", ParamType::NodeIds, true, "Layers to select.")] }),
        CommandSpec::new("selection.get", "Get selection", selection_get)
            .read_only()
            .doc("Returns {selection, currentPage}."),
        CommandSpec::new("selection.selectAll", "Select all", select_all)
            .menu("Edit/Select all")
            .key("Cmd+A")
            .doc("Selects the siblings of the current selection, or every top-level layer of the current page."),
        CommandSpec::new("selection.clear", "Deselect", select_none)
            .key("Escape")
            .doc("Clears the selection."),
        CommandSpec::new("selection.selectChildren", "Select children", select_children)
            .key("Enter")
            .enabled(has_selection)
            .doc("Selects the children of the selected containers."),
        CommandSpec::new("selection.selectParent", "Select parent", select_parent)
            .key("Shift+Enter")
            .enabled(has_selection)
            .doc("Selects the parents of the selected layers (never a page)."),
        CommandSpec::new("selection.selectInverse", "Select inverse", select_inverse)
            .menu("Edit/Select inverse")
            .key("Shift+Cmd+A")
            .enabled(has_selection)
            .doc("Selects the siblings that are not selected."),
    ]
}

fn reply(sel: &[NodeId]) -> serde_json::Value {
    json!({ "selection": sel })
}

fn selection_set(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = p.req_node_ids("ids")?;
    if ids.is_empty() {
        return Ok(Outcome::read(reply(&[])).selecting(Vec::new()));
    }
    let ids = layer_ids(ctx, p)?;
    let page = ctx.doc.page_of(ids[0]).unwrap_or(ctx.current_page);
    Ok(Outcome::read(reply(&ids)).selecting(ids).on_page(page))
}

fn selection_get(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    Ok(Outcome::read(json!({
        "selection": ctx.selection,
        "currentPage": ctx.current_page,
    })))
}

fn select_all(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let parent = ctx
        .selection
        .first()
        .and_then(|id| ctx.doc.get(*id))
        .and_then(|n| n.parent)
        .unwrap_or(ctx.current_page);
    let ids = ctx.doc.node(parent)?.children.clone();
    Ok(Outcome::read(reply(&ids)).selecting(ids))
}

fn select_none(_: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    Ok(Outcome::read(reply(&[])).selecting(Vec::new()))
}

fn select_children(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let mut ids = Vec::new();
    for id in ctx.selection {
        if let Some(n) = ctx.doc.get(*id) {
            ids.extend(n.children.iter().copied());
        }
    }
    if ids.is_empty() {
        ids = ctx.selection.to_vec();
    }
    Ok(Outcome::read(reply(&ids)).selecting(ids))
}

fn select_parent(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let mut ids: Vec<NodeId> = Vec::new();
    for id in ctx.selection {
        let parent = ctx.doc.get(*id).and_then(|n| n.parent);
        let target = match parent.and_then(|pid| ctx.doc.get(pid)) {
            Some(pn) if !matches!(pn.kind, NodeKind::Page | NodeKind::Document) => pn.id,
            _ => *id,
        };
        if !ids.contains(&target) {
            ids.push(target);
        }
    }
    Ok(Outcome::read(reply(&ids)).selecting(ids))
}

fn select_inverse(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let parent = ctx
        .selection
        .first()
        .and_then(|id| ctx.doc.get(*id))
        .and_then(|n| n.parent)
        .unwrap_or(ctx.current_page);
    let ids: Vec<NodeId> = ctx
        .doc
        .node(parent)?
        .children
        .iter()
        .copied()
        .filter(|c| !ctx.selection.contains(c))
        .collect();
    Ok(Outcome::read(reply(&ids)).selecting(ids))
}
