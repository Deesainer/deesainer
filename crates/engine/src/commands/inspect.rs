use deesainer_doc::{Document, NodeId};
use serde_json::{json, Value};

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("document.inspect", "Inspect document", document_inspect)
            .read_only()
            .doc("Returns the full document as JSON (nodes, styles, variables, modes) plus currentPage and selection. Large; prefer document.tree for orientation."),
        CommandSpec::new("document.tree", "Layer tree", document_tree)
            .read_only()
            .doc("Compact layer tree of a page: [{id, name, type, rect, visible, children}]. Rects are in the parent's space; worldBounds in page space.")
            .params(const { &[param("page", ParamType::NodeId, false, "Page id; defaults to the current page.")] }),
        CommandSpec::new("node.inspect", "Inspect node", node_inspect)
            .read_only()
            .doc("Returns one node with all its properties, its worldBounds, parent and index.")
            .params(const { &[param("id", ParamType::NodeId, true, "Node id.")] }),
        CommandSpec::new("document.find", "Find layers", document_find)
            .read_only()
            .doc("Finds nodes by exact name and/or type (frame, rectangle, text, ...). Returns [{id, name, type, page}].")
            .params(const {
                &[
                    param("name", ParamType::String, false, "Exact layer name."),
                    param("type", ParamType::String, false, "Node type, e.g. frame, text, instance."),
                ]
            }),
    ]
}

fn document_inspect(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let doc = serde_json::to_value(ctx.doc).map_err(|e| EngineError::Internal(e.to_string()))?;
    Ok(Outcome::read(json!({
        "document": doc,
        "currentPage": ctx.current_page,
        "selection": ctx.selection,
    })))
}

pub(crate) fn tree_json(doc: &Document, id: NodeId, depth: usize) -> Value {
    let Some(node) = doc.get(id) else {
        return Value::Null;
    };
    let children: Vec<Value> = if depth > deesainer_doc::MAX_DEPTH {
        Vec::new()
    } else {
        node.children
            .iter()
            .map(|c| tree_json(doc, *c, depth + 1))
            .collect()
    };
    let mut v = json!({
        "id": node.id,
        "name": node.name,
        "type": node.kind.type_name(),
        "rect": node.rect,
        "visible": node.visible,
    });
    if node.rotation != 0.0 {
        v["rotation"] = json!(node.rotation);
    }
    if let Some(t) = node.text() {
        v["text"] = json!(t.content);
    }
    if !children.is_empty() {
        v["children"] = Value::Array(children);
    }
    v
}

fn document_tree(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let page = p.node_id("page")?.unwrap_or(ctx.current_page);
    if !ctx.doc.contains(page) {
        return Err(p.bad("page", format!("{page} does not exist")));
    }
    let pages: Vec<Value> = ctx
        .doc
        .pages()
        .iter()
        .filter_map(|id| ctx.doc.get(*id))
        .map(|n| json!({"id": n.id, "name": n.name}))
        .collect();
    Ok(Outcome::read(json!({
        "pages": pages,
        "currentPage": ctx.current_page,
        "selection": ctx.selection,
        "tree": tree_json(ctx.doc, page, 0),
    })))
}

fn node_inspect(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    let mut v = serde_json::to_value(node).map_err(|e| EngineError::Internal(e.to_string()))?;
    v["type"] = json!(node.kind.type_name());
    v["worldBounds"] = json!(ctx.doc.world_bounds(id));
    v["index"] = json!(ctx.doc.index_in_parent(id).ok());
    v["page"] = json!(ctx.doc.page_of(id));
    Ok(Outcome::read(v))
}

fn document_find(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let name = p.str("name")?;
    let ty = p.str("type")?;
    if name.is_none() && ty.is_none() {
        return Err(p.invalid("pass name and/or type"));
    }
    let hits: Vec<Value> = ctx
        .doc
        .nodes()
        .filter(|n| name.is_none_or(|w| n.name == w))
        .filter(|n| ty.is_none_or(|w| n.kind.type_name() == w))
        .map(|n| {
            json!({
                "id": n.id,
                "name": n.name,
                "type": n.kind.type_name(),
                "page": ctx.doc.page_of(n.id),
            })
        })
        .collect();
    Ok(Outcome::read(Value::Array(hits)))
}
