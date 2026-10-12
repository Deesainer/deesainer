use deesainer_doc::{Annotation, NodeKind, PropValue};
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

const MAX_PER_LAYER: usize = 64;
const MAX_TEXT: usize = 4000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("annotation.add", "Add annotation", add)
            .doc("Adds a developer note to a layer, optionally calling out some of its properties. Returns {index}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer."),
                    param("text", ParamType::String, true, "The note."),
                    param("props", ParamType::Json, false, "Property names it highlights, e.g. [\"width\", \"gap\"]."),
                ]
            }),
        CommandSpec::new("annotation.update", "Edit annotation", update)
            .doc("Changes an annotation's text or properties.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer."),
                    param("index", ParamType::Integer, true, "Which annotation."),
                    param("text", ParamType::String, false, "New text."),
                    param("props", ParamType::Json, false, "New property names."),
                ]
            }),
        CommandSpec::new("annotation.remove", "Remove annotation", remove)
            .doc("Removes one annotation (by index) or all of a layer's.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer."),
                    param("index", ParamType::Integer, false, "Which one; omit for all."),
                ]
            }),
        CommandSpec::new("annotation.list", "List annotations", list)
            .read_only()
            .doc("Every annotation on the current page: [{id, name, index, text, props}]."),
    ]
}

fn props_param(p: &Params<'_>) -> Result<Option<Vec<String>>, EngineError> {
    let Some(list) = p.json::<Vec<String>>("props")? else {
        return Ok(None);
    };
    if list.len() > 32 || list.iter().any(|s| s.is_empty() || s.len() > 64) {
        return Err(p.bad("props", "at most 32 names of 1 to 64 characters"));
    }
    Ok(Some(list))
}

fn text_param(p: &Params<'_>, key: &str) -> Result<Option<String>, EngineError> {
    let Some(t) = p.str(key)? else {
        return Ok(None);
    };
    let t = t.trim();
    if t.is_empty() || t.chars().count() > MAX_TEXT {
        return Err(p.bad(key, format!("needs 1 to {MAX_TEXT} characters")));
    }
    Ok(Some(t.to_owned()))
}

fn layer<'a>(ctx: &'a Context<'_>, p: &Params<'_>) -> Result<&'a deesainer_doc::Node, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx.doc.node(id)?;
    if matches!(node.kind, NodeKind::Document | NodeKind::Page) {
        return Err(p.bad("id", "annotate a layer, not a page"));
    }
    Ok(node)
}

fn add(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let node = layer(ctx, p)?;
    if node.annotations.len() >= MAX_PER_LAYER {
        return Err(p.invalid(format!("at most {MAX_PER_LAYER} annotations per layer")));
    }
    let text = text_param(p, "text")?.ok_or_else(|| p.missing("text"))?;
    let mut list = node.annotations.clone();
    list.push(Annotation {
        text,
        props: props_param(p)?.unwrap_or_default(),
    });
    let index = list.len() - 1;
    Ok(Outcome::apply(
        Transaction::new(
            "Add annotation",
            vec![Op::set(node.id, PropValue::Annotations(list))],
        ),
        json!({ "index": index }),
    ))
}

fn update(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let node = layer(ctx, p)?;
    let i = p.u32("index")?.ok_or_else(|| p.missing("index"))? as usize;
    let mut list = node.annotations.clone();
    let a = list
        .get_mut(i)
        .ok_or_else(|| p.bad("index", "no such annotation"))?;
    if let Some(t) = text_param(p, "text")? {
        a.text = t;
    }
    if let Some(props) = props_param(p)? {
        a.props = props;
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Edit annotation",
            vec![Op::set(node.id, PropValue::Annotations(list))],
        ),
        json!({}),
    ))
}

fn remove(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let node = layer(ctx, p)?;
    let mut list = node.annotations.clone();
    match p.u32("index")? {
        Some(i) if (i as usize) < list.len() => {
            list.remove(i as usize);
        }
        Some(_) => return Err(p.bad("index", "no such annotation")),
        None => list.clear(),
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Remove annotation",
            vec![Op::set(node.id, PropValue::Annotations(list))],
        ),
        json!({}),
    ))
}

fn list(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let mut out: Vec<Value> = Vec::new();
    for id in ctx.doc.subtree_pre_order(ctx.current_page)? {
        let Some(n) = ctx.doc.get(id) else { continue };
        for (i, a) in n.annotations.iter().enumerate() {
            out.push(
                json!({ "id": id, "name": n.name, "index": i, "text": a.text, "props": a.props }),
            );
        }
    }
    Ok(Outcome::read(json!(out)))
}
