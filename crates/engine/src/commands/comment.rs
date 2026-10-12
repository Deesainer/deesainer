use deesainer_doc::{Comment, CommentId, NodeKind, Reply};
use deesainer_geom::Point;
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("comment.add", "Add comment", add)
            .key("C")
            .doc("Pins a comment at page coordinates (x, y) on the current page, optionally attached to a layer. Returns {id}.")
            .params(const {
                &[
                    param("x", ParamType::Number, true, "Page x."),
                    param("y", ParamType::Number, true, "Page y."),
                    param("text", ParamType::String, true, "Comment text."),
                    param("node", ParamType::NodeId, false, "Attach to this layer."),
                ]
            }),
        CommandSpec::new("comment.reply", "Reply", reply)
            .doc("Adds a reply to a comment thread.")
            .params(const {
                &[
                    param("id", ParamType::Integer, true, "Comment id."),
                    param("text", ParamType::String, true, "Reply text."),
                ]
            }),
        CommandSpec::new("comment.resolve", "Resolve comment", resolve)
            .doc("Marks a thread resolved (or reopens it).")
            .params(const {
                &[
                    param("id", ParamType::Integer, true, "Comment id."),
                    param("resolved", ParamType::Bool, false, "Default true."),
                ]
            }),
        CommandSpec::new("comment.delete", "Delete comment", delete)
            .doc("Deletes a comment thread.")
            .params(const { &[param("id", ParamType::Integer, true, "Comment id.")] }),
        CommandSpec::new("comment.list", "List comments", list)
            .read_only()
            .doc("Lists comment threads: [{id, author, text, page, position, node, resolved, replies}]. Unresolved first.")
            .params(const { &[param("page", ParamType::NodeId, false, "Only this page.")] }),
    ]
}

fn text(p: &Params<'_>) -> Result<String, EngineError> {
    let t = p.req_str("text")?.trim();
    if t.is_empty() {
        return Err(p.bad("text", "must not be empty"));
    }
    if t.len() > 20_000 {
        return Err(p.bad("text", "is longer than 20000 bytes"));
    }
    Ok(t.to_owned())
}

fn comment_id(ctx: &Context<'_>, p: &Params<'_>) -> Result<(CommentId, Comment), EngineError> {
    let raw = p
        .raw("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| p.missing("id"))?;
    let id = CommentId(raw);
    let c = ctx
        .doc
        .comment(id)
        .cloned()
        .ok_or_else(|| p.bad("id", format!("comment {raw} does not exist")))?;
    Ok((id, c))
}

fn add(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let node = p.node_id("node")?;
    if let Some(n) = node {
        if !ctx.doc.contains(n) {
            return Err(p.bad("node", format!("{n} does not exist")));
        }
    }
    let page = node
        .and_then(|n| ctx.doc.page_of(n))
        .unwrap_or(ctx.current_page);
    if !matches!(ctx.doc.get(page).map(|n| &n.kind), Some(NodeKind::Page)) {
        return Err(p.invalid("no page to comment on"));
    }
    let id = CommentId(ctx.doc.next_free_id().0);
    let comment = Comment {
        author: ctx.user.to_owned(),
        text: text(p)?,
        created: ctx.now.clone(),
        page,
        position: Point::new(p.req_f64("x")?, p.req_f64("y")?),
        node,
        resolved: false,
        replies: Vec::new(),
    };
    Ok(Outcome::apply(
        Transaction::new(
            "Add comment",
            vec![Op::SetComment {
                id,
                comment: Some(comment),
            }],
        ),
        json!({ "id": id }),
    ))
}

fn reply(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, mut c) = comment_id(ctx, p)?;
    c.replies.push(Reply {
        author: ctx.user.to_owned(),
        text: text(p)?,
        created: ctx.now.clone(),
    });
    Ok(Outcome::apply(
        Transaction::new(
            "Reply",
            vec![Op::SetComment {
                id,
                comment: Some(c),
            }],
        ),
        json!({}),
    ))
}

fn resolve(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, mut c) = comment_id(ctx, p)?;
    c.resolved = p.bool("resolved")?.unwrap_or(true);
    Ok(Outcome::apply(
        Transaction::new(
            "Resolve comment",
            vec![Op::SetComment {
                id,
                comment: Some(c),
            }],
        ),
        json!({}),
    ))
}

fn delete(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, _) = comment_id(ctx, p)?;
    Ok(Outcome::apply(
        Transaction::new("Delete comment", vec![Op::SetComment { id, comment: None }]),
        json!({}),
    ))
}

fn list(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let page = p.node_id("page")?;
    let mut items: Vec<(bool, Value)> = ctx
        .doc
        .comments()
        .filter(|(_, c)| page.is_none_or(|pg| c.page == pg))
        .map(|(id, c)| {
            let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
            v["id"] = json!(id);
            (c.resolved, v)
        })
        .collect();
    items.sort_by_key(|(resolved, _)| *resolved);
    Ok(Outcome::read(Value::Array(
        items.into_iter().map(|(_, v)| v).collect(),
    )))
}
