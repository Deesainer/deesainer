use deesainer_doc::{Node, NodeKind, Paint};
use deesainer_geom::Rect;
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::{clone_subtrees, collect_subtrees, param};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("page.create", "Add new page", page_create)
            .menu("Page/Add new page")
            .doc("Adds a page after the last one and switches to it. Returns {id}.")
            .params(
                const {
                    &[param(
                        "name",
                        ParamType::String,
                        false,
                        "Page name; default \"Page N\".",
                    )]
                },
            ),
        CommandSpec::new("page.select", "Go to page", page_select)
            .doc("Switches the current page (view state, not undoable). Clears the selection.")
            .params(const { &[param("id", ParamType::NodeId, true, "Page id.")] }),
        CommandSpec::new("page.delete", "Delete page", page_delete)
            .menu("Page/Delete page")
            .doc("Deletes a page and everything on it. The last page cannot be deleted.")
            .params(
                const {
                    &[param(
                        "id",
                        ParamType::NodeId,
                        false,
                        "Page id; defaults to the current page.",
                    )]
                },
            ),
        CommandSpec::new("page.duplicate", "Duplicate page", page_duplicate)
            .menu("Page/Duplicate page")
            .doc("Copies a page with all its layers, right after it. Returns {id}.")
            .params(
                const {
                    &[param(
                        "id",
                        ParamType::NodeId,
                        false,
                        "Page id; defaults to the current page.",
                    )]
                },
            ),
        CommandSpec::new("page.rename", "Rename page", page_rename)
            .doc("Renames a page.")
            .params(
                const {
                    &[
                        param(
                            "id",
                            ParamType::NodeId,
                            false,
                            "Page id; defaults to the current page.",
                        ),
                        param("name", ParamType::String, true, "New name."),
                    ]
                },
            ),
        CommandSpec::new("page.setBackground", "Page background", page_background)
            .doc("Sets the canvas background colour of a page.")
            .params(
                const {
                    &[
                        param(
                            "id",
                            ParamType::NodeId,
                            false,
                            "Page id; defaults to the current page.",
                        ),
                        param("color", ParamType::Color, true, "Background colour."),
                    ]
                },
            ),
        CommandSpec::new("page.setGuides", "Ruler guides", page_guides)
            .doc("Replaces the ruler guides of a page. Guides are page-space lines that moving layers snap to.")
            .params(
                const {
                    &[
                        param(
                            "id",
                            ParamType::NodeId,
                            false,
                            "Page id; defaults to the current page.",
                        ),
                        param(
                            "guides",
                            ParamType::Json,
                            true,
                            "[{vertical: bool, at: number}], at most 1000.",
                        ),
                    ]
                },
            ),
    ]
}

fn page_param(ctx: &Context<'_>, p: &Params<'_>) -> Result<deesainer_doc::NodeId, EngineError> {
    let id = p.node_id("id")?.unwrap_or(ctx.current_page);
    match ctx.doc.get(id).map(|n| &n.kind) {
        Some(NodeKind::Page) => Ok(id),
        _ => Err(p.bad("id", format!("{id} is not a page"))),
    }
}

fn page_create(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = ctx.doc.next_free_id();
    let count = ctx.doc.pages().len();
    let name = p
        .str("name")?
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Page {}", count + 1));
    let mut page = Node::new(id, name, NodeKind::Page, Rect::default());
    if let Some(first) = ctx.doc.get(ctx.doc.first_page()) {
        page.fills = first.fills.clone();
    }
    let tx = Transaction::new(
        "Add page",
        vec![Op::CreateNode {
            parent: ctx.doc.root(),
            index: count,
            node: Box::new(page),
        }],
    );
    Ok(Outcome::apply(tx, json!({ "id": id }))
        .on_page(id)
        .selecting(Vec::new()))
}

fn page_select(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    if !matches!(ctx.doc.get(id).map(|n| &n.kind), Some(NodeKind::Page)) {
        return Err(p.bad("id", format!("{id} is not a page")));
    }
    Ok(Outcome::read(json!({ "currentPage": id }))
        .on_page(id)
        .selecting(Vec::new()))
}

fn page_delete(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = page_param(ctx, p)?;
    if ctx.doc.pages().len() <= 1 {
        return Err(p.invalid("the last page cannot be deleted"));
    }
    let ops = ctx
        .doc
        .subtree_post_order(id)?
        .into_iter()
        .map(|n| Op::DeleteNode { id: n })
        .collect();
    let next = ctx
        .doc
        .pages()
        .iter()
        .copied()
        .find(|p| *p != id)
        .unwrap_or(ctx.current_page);
    Ok(
        Outcome::apply(Transaction::new("Delete page", ops), json!({}))
            .on_page(next)
            .selecting(Vec::new()),
    )
}

fn page_duplicate(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = page_param(ctx, p)?;
    let nodes = collect_subtrees(ctx.doc, &[id])?;
    let index = ctx.doc.index_in_parent(id)? + 1;
    let (mut ops, roots) =
        clone_subtrees(ctx.doc, &nodes, &[id], ctx.doc.root(), index, (0.0, 0.0))?;
    let new_id = roots.first().copied().unwrap_or(id);
    let name = format!("{} copy", ctx.doc.node(id)?.name);
    ops.push(Op::set(new_id, deesainer_doc::PropValue::Name(name)));
    Ok(Outcome::apply(
        Transaction::new("Duplicate page", ops),
        json!({ "id": new_id }),
    )
    .on_page(new_id)
    .selecting(Vec::new()))
}

fn page_rename(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = page_param(ctx, p)?;
    let name = p.req_str("name")?.to_owned();
    Ok(Outcome::apply(
        Transaction::new(
            "Rename page",
            vec![Op::set(id, deesainer_doc::PropValue::Name(name))],
        ),
        json!({}),
    ))
}

fn page_guides(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = page_param(ctx, p)?;
    let guides: Vec<deesainer_doc::RulerGuide> =
        p.json("guides")?.ok_or_else(|| p.missing("guides"))?;
    if guides.len() > 1000 || guides.iter().any(|g| !g.at.is_finite()) {
        return Err(p.bad("guides", "needs at most 1000 finite positions".to_owned()));
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Ruler guides",
            vec![Op::set(id, deesainer_doc::PropValue::Guides(guides))],
        ),
        json!({}),
    ))
}

fn page_background(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = page_param(ctx, p)?;
    let color = p.color("color")?.ok_or_else(|| p.missing("color"))?;
    Ok(Outcome::apply(
        Transaction::new(
            "Page background",
            vec![Op::set(
                id,
                deesainer_doc::PropValue::Fills(vec![Paint::solid(color)]),
            )],
        ),
        json!({}),
    ))
}
