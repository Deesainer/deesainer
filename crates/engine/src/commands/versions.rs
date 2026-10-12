use base64::Engine as _;
use deesainer_doc::{Document, NodeId, PropKey};
use serde_json::{json, Value};

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Effect, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("version.save", "Save to version history", save)
            .doc("Saves the current document as a named version in the file. Returns {id}.")
            .params(const { &[param("label", ParamType::String, false, "Name, default \"Untitled version\".")] }),
        CommandSpec::new("version.list", "Version history", list)
            .read_only()
            .doc("Lists the saved versions, newest first: [{id, label, savedAt, author}]."),
        CommandSpec::new("version.preview", "Preview version", preview)
            .read_only()
            .doc("Renders a saved version's page (default the first) as a PNG: {mimeType, data, width, height}.")
            .params(const {
                &[
                    param("id", ParamType::Integer, true, "Version id."),
                    param("page", ParamType::Integer, false, "Page index, default 0."),
                    param("scale", ParamType::Number, false, "0.05..=4, default 1."),
                ]
            }),
        CommandSpec::new("version.diff", "Compare with version", diff)
            .read_only()
            .doc("What restoring a version would change: {added, removed, changed: [{id, name, props}]} (added: layers the version has and the current document does not).")
            .params(const { &[param("id", ParamType::Integer, true, "Version id.")] }),
        CommandSpec::new("version.compare", "Visual diff with version", compare)
            .read_only()
            .doc("Renders a page of a saved version and of the current document over the same area and marks every pixel that differs. Returns {before, after, diff (PNGs, base64; diff shows changes in magenta over a faded current page), changedPixels, width, height}.")
            .params(const {
                &[
                    param("id", ParamType::Integer, true, "Version id."),
                    param("page", ParamType::Integer, false, "Page index, default 0 (pages are matched by index)."),
                    param("scale", ParamType::Number, false, "0.05..=4, default 1; the result is at most 4096 px a side."),
                ]
            }),
        CommandSpec::new("version.restore", "Restore version", restore)
            .doc("Replaces the document with a saved version. The current document is saved as a version first, so nothing is lost. Clears undo.")
            .params(const { &[param("id", ParamType::Integer, true, "Version id.")] }),
        CommandSpec::new("version.delete", "Delete version", delete)
            .doc("Removes a saved version from the file.")
            .params(const { &[param("id", ParamType::Integer, true, "Version id.")] }),
    ]
}

fn version<'a>(
    ctx: &'a Context<'_>,
    p: &Params<'_>,
) -> Result<&'a deesainer_format::Version, EngineError> {
    let id = p
        .raw("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| p.missing("id"))?;
    ctx.versions
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| p.bad("id", format!("version {id} does not exist")))
}

fn save(_: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let label = p.str("label")?.unwrap_or("Untitled version").trim();
    if label.chars().count() > 200 {
        return Err(p.bad("label", "at most 200 characters"));
    }
    let label = if label.is_empty() {
        "Untitled version"
    } else {
        label
    };
    Ok(Outcome::effect(Effect::SaveVersion {
        label: label.to_owned(),
    }))
}

fn list(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let list: Vec<Value> = ctx
        .versions
        .iter()
        .rev()
        .map(|v| json!({ "id": v.id, "label": v.label, "savedAt": v.saved_at, "author": v.author }))
        .collect();
    Ok(Outcome::read(json!(list)))
}

fn preview(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let v = version(ctx, p)?;
    let doc = v.document.as_ref();
    let index = p.u32("page")?.unwrap_or(0) as usize;
    let page = *doc
        .pages()
        .get(index)
        .ok_or_else(|| p.bad("page", "no such page in that version"))?;
    let scale = p.f64("scale")?.unwrap_or(1.0);
    if !(0.05..=4.0).contains(&scale) {
        return Err(p.bad("scale", "must be between 0.05 and 4"));
    }
    let png = deesainer_io::export_png(doc, page, scale).map_err(|e| p.invalid(e.to_string()))?;
    let (w, h) = deesainer_render::images::dimensions(&png).unwrap_or((0, 0));
    Ok(Outcome::read(json!({
        "mimeType": "image/png",
        "data": base64::engine::general_purpose::STANDARD.encode(png),
        "width": w,
        "height": h,
    })))
}

fn changed_props(a: &deesainer_doc::Node, b: &deesainer_doc::Node) -> Vec<String> {
    PropKey::ALL
        .iter()
        .filter(|k| a.get(**k) != b.get(**k))
        .map(|k| {
            serde_json::to_value(k)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{k:?}"))
        })
        .collect()
}

fn summary(doc: &Document, id: NodeId) -> Value {
    json!({ "id": id, "name": doc.get(id).map(|n| n.name.clone()).unwrap_or_default() })
}

fn diff(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    const LIMIT: usize = 500;
    let v = version(ctx, p)?;
    let (then, now) = (v.document.as_ref(), ctx.doc);
    let mut added = Vec::new();
    let mut changed = Vec::new();
    for n in then.nodes() {
        match now.get(n.id) {
            None => added.push(summary(then, n.id)),
            Some(cur) => {
                let props = changed_props(cur, n);
                let moved = cur.parent != n.parent || cur.children != n.children;
                if !props.is_empty() || moved {
                    let mut s = summary(then, n.id);
                    s["props"] = json!(props);
                    if moved {
                        s["moved"] = json!(true);
                    }
                    changed.push(s);
                }
            }
        }
    }
    let removed: Vec<Value> = now
        .nodes()
        .filter(|n| !then.contains(n.id))
        .map(|n| summary(now, n.id))
        .collect();
    let total = added.len() + removed.len() + changed.len();
    added.truncate(LIMIT);
    changed.truncate(LIMIT);
    let removed: Vec<Value> = removed.into_iter().take(LIMIT).collect();
    Ok(Outcome::read(json!({
        "added": added,
        "removed": removed,
        "changed": changed,
        "total": total,
    })))
}

fn restore(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let v = version(ctx, p)?;
    Ok(Outcome::effect(Effect::RestoreVersion(v.id)))
}

fn delete(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let v = version(ctx, p)?;
    Ok(Outcome::effect(Effect::DeleteVersion(v.id)))
}

fn compare(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let v = version(ctx, p)?;
    let index = p.u32("page")?.unwrap_or(0) as usize;
    let scale = p.f64("scale")?.unwrap_or(1.0);
    if !(0.05..=4.0).contains(&scale) {
        return Err(p.bad("scale", "must be between 0.05 and 4"));
    }
    let c = crate::compare::documents(v.document.as_ref(), ctx.doc, index, scale)
        .map_err(|e| p.invalid(e))?;
    let png = |pix: &deesainer_render::Pixmap| -> Result<String, EngineError> {
        deesainer_render::encode_png(pix)
            .map(|b| base64::engine::general_purpose::STANDARD.encode(b))
            .map_err(|e| p.invalid(e.to_string()))
    };
    Ok(Outcome::read(json!({
        "before": png(&c.before)?,
        "after": png(&c.after)?,
        "diff": png(&c.diff)?,
        "changedPixels": c.changed,
        "width": c.before.width(),
        "height": c.before.height(),
    })))
}
