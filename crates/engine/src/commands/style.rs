use deesainer_doc::{
    BindTarget, CollectionId, Modes, NodeKind, PropValue, Style, StyleId, StyleKind, StyleRefs,
    VarValue, Variable, VariableId,
};
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::{layer_ids, param, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

const STYLE_KINDS: &[&str] = &["paint", "text", "effect", "grid"];
const TARGETS: &[&str] = &["fill", "stroke", "text", "effect", "grid"];
pub(super) const BIND_TARGETS: &[&str] = &[
    "fillColor",
    "strokeColor",
    "opacity",
    "cornerRadius",
    "width",
    "height",
    "gap",
    "padding",
    "textContent",
    "visible",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("style.create", "Create style", style_create)
            .doc("Creates a shared style from a layer (fills, text style, effects or layout grids) or from a JSON value. Returns {id}.")
            .params(const {
                &[
                    param("name", ParamType::String, true, "Style name; use slashes to group, e.g. \"Brand/Primary\"."),
                    param("kind", ParamType::Enum(STYLE_KINDS), true, "paint (fills/strokes), text, effect or grid (layout grids)."),
                    param("from", ParamType::NodeId, false, "Take the value from this layer."),
                    param("value", ParamType::Json, false, "Paint list, text style object or effect list."),
                    param("description", ParamType::String, false, "Description."),
                ]
            }),
        CommandSpec::new("style.update", "Edit style", style_update)
            .doc("Renames, describes or changes the value of a style. Every layer using it updates.")
            .params(const {
                &[
                    param("id", ParamType::Integer, true, "Style id."),
                    param("name", ParamType::String, false, "New name."),
                    param("description", ParamType::String, false, "New description."),
                    param("from", ParamType::NodeId, false, "Take the new value from this layer."),
                    param("value", ParamType::Json, false, "New value."),
                ]
            }),
        CommandSpec::new("style.delete", "Delete style", style_delete)
            .doc("Deletes a style. Layers keep their current look and are detached.")
            .params(const { &[param("id", ParamType::Integer, true, "Style id.")] }),
        CommandSpec::new("style.apply", "Apply style", style_apply)
            .doc("Links layers to a style for a target (fill, stroke, text, effect). The layers take the style's value now and whenever it changes.")
            .params(const {
                &[
                    IDS,
                    param("style", ParamType::Integer, true, "Style id."),
                    param("target", ParamType::Enum(TARGETS), false, "Which property; defaults from the style kind (paint -> fill)."),
                ]
            }),
        CommandSpec::new("style.detach", "Detach style", style_detach)
            .doc("Unlinks layers from a style target, keeping their current values.")
            .params(const {
                &[
                    IDS,
                    param("target", ParamType::Enum(TARGETS), true, "Which link to remove."),
                ]
            }),
        CommandSpec::new("style.list", "List styles", style_list)
            .read_only()
            .doc("Lists styles: [{id, name, kind, description, value}]."),
        CommandSpec::new("variable.create", "Create variable", variable_create)
            .doc("Creates a variable. value is a colour (hex string), number, string or bool; it applies to every mode unless values gives one per mode. Returns {id}.")
            .params(const {
                &[
                    param("name", ParamType::String, true, "Name; slashes group, e.g. \"color/bg\"."),
                    param("value", ParamType::Json, false, "Value for all modes."),
                    param("values", ParamType::Json, false, "One value per mode, in mode order."),
                    param("type", ParamType::Enum(&["color", "number", "string", "bool"]), false, "Force a type (colours given as hex need it only when ambiguous)."),
                    param("collection", ParamType::Integer, false, "Collection id; default 0, the default collection."),
                    param("scopes", ParamType::Json, false, "Properties it can be bound to, e.g. [\"fillColor\", \"strokeColor\"]; default all."),
                ]
            }),
        CommandSpec::new("variable.set", "Set variable", variable_set)
            .doc("Sets a variable's value for one mode (default: the active mode), or aliases it to another variable.")
            .params(const {
                &[
                    param("id", ParamType::Integer, true, "Variable id."),
                    param("value", ParamType::Json, false, "New value."),
                    param("alias", ParamType::Integer, false, "Make this mode an alias of another variable."),
                    param("mode", ParamType::Integer, false, "Mode index."),
                    param("name", ParamType::String, false, "Rename."),
                ]
            }),
        CommandSpec::new("variable.delete", "Delete variable", variable_delete)
            .doc("Deletes a variable; bound layers keep their current values and are unbound.")
            .params(const { &[param("id", ParamType::Integer, true, "Variable id.")] }),
        CommandSpec::new("variable.bind", "Bind variable", variable_bind)
            .doc("Binds a property of layers to a variable; the property follows the variable (and the active mode).")
            .params(const {
                &[
                    IDS,
                    param("target", ParamType::Enum(BIND_TARGETS), true, "Property to bind."),
                    param("variable", ParamType::Integer, true, "Variable id."),
                ]
            }),
        CommandSpec::new("variable.unbind", "Unbind variable", variable_unbind)
            .doc("Removes a binding, keeping the current value.")
            .params(const {
                &[
                    IDS,
                    param("target", ParamType::Enum(BIND_TARGETS), true, "Property to unbind."),
                ]
            }),
        CommandSpec::new("variable.list", "List variables", variable_list)
            .read_only()
            .doc("Lists modes (the default collection's) {name, names, active}, collections [{id, name, names, active}] and variables [{id, name, collection, scopes, values, resolved}]."),
        CommandSpec::new("mode.add", "Add mode", mode_add)
            .doc("Adds a variable mode (e.g. Dark). New modes copy the first mode's values.")
            .params(const {
                &[
                    param("name", ParamType::String, true, "Mode name."),
                    param("collection", ParamType::Integer, false, "Collection id; default 0."),
                ]
            }),
        CommandSpec::new("mode.setActive", "Switch mode", mode_set_active)
            .doc("Switches the active mode; every bound property updates.")
            .params(const {
                &[
                    param("index", ParamType::Integer, false, "Mode index."),
                    param("name", ParamType::String, false, "Mode name."),
                    param("collection", ParamType::Integer, false, "Collection id; default 0."),
                ]
            }),
        CommandSpec::new("mode.rename", "Rename mode", mode_rename)
            .doc("Renames a mode.")
            .params(const {
                &[
                    param("index", ParamType::Integer, true, "Mode index."),
                    param("name", ParamType::String, true, "New name."),
                    param("collection", ParamType::Integer, false, "Collection id; default 0."),
                ]
            }),
    ]
}

fn style_id(ctx: &Context<'_>, p: &Params<'_>, key: &str) -> Result<StyleId, EngineError> {
    let raw = p
        .raw(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| p.missing(key))?;
    let id = StyleId(raw);
    if ctx.doc.style(id).is_none() {
        return Err(p.bad(key, format!("style {raw} does not exist")));
    }
    Ok(id)
}

fn style_value(ctx: &Context<'_>, p: &Params<'_>, kind: &str) -> Result<StyleKind, EngineError> {
    if let Some(from) = p.node_id("from")? {
        let node = ctx
            .doc
            .get(from)
            .ok_or_else(|| p.bad("from", format!("{from} does not exist")))?;
        return Ok(match kind {
            "paint" => StyleKind::Paint(node.fills.clone()),
            "effect" => StyleKind::Effect(node.effects.clone()),
            "grid" => StyleKind::Grid(node.layout_grids.clone()),
            _ => match &node.kind {
                NodeKind::Text(t) => StyleKind::Text(t.style.clone()),
                _ => return Err(p.bad("from", "a text style must come from a text layer")),
            },
        });
    }
    let raw = p
        .raw("value")
        .ok_or_else(|| p.invalid("pass from or value"))?;
    let parsed = match kind {
        "paint" => serde_json::from_value(raw.clone()).map(StyleKind::Paint),
        "effect" => serde_json::from_value(raw.clone()).map(StyleKind::Effect),
        "grid" => serde_json::from_value(raw.clone()).map(StyleKind::Grid),
        _ => serde_json::from_value(raw.clone()).map(StyleKind::Text),
    };
    let value = parsed.map_err(|e| p.bad("value", format!("has the wrong shape: {e}")))?;
    if let StyleKind::Paint(paints) = &value {
        super::validate_paints(p, "value", paints)?;
    }
    Ok(value)
}

fn kind_name(k: &StyleKind) -> &'static str {
    match k {
        StyleKind::Paint(_) => "paint",
        StyleKind::Text(_) => "text",
        StyleKind::Effect(_) => "effect",
        StyleKind::Grid(_) => "grid",
    }
}

fn style_create(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let kind = p
        .choice("kind", STYLE_KINDS)?
        .ok_or_else(|| p.missing("kind"))?;
    let value = style_value(ctx, p, kind)?;
    let id = StyleId(ctx.doc.next_free_id().0);
    let style = Style {
        origin: None,
        name: p.req_str("name")?.to_owned(),
        description: p.str("description")?.unwrap_or("").to_owned(),
        kind: value,
    };
    Ok(Outcome::apply(
        Transaction::new(
            "Create style",
            vec![Op::SetStyle {
                id,
                style: Some(style),
            }],
        ),
        json!({ "id": id }),
    ))
}

fn style_update(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = style_id(ctx, p, "id")?;
    let mut style = ctx
        .doc
        .style(id)
        .cloned()
        .ok_or_else(|| p.bad("id", "missing"))?;
    if let Some(n) = p.str("name")? {
        style.name = n.to_owned();
    }
    if let Some(d) = p.str("description")? {
        style.description = d.to_owned();
    }
    if p.has("from") || p.has("value") {
        style.kind = style_value(ctx, p, kind_name(&style.kind))?;
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Edit style",
            vec![Op::SetStyle {
                id,
                style: Some(style),
            }],
        ),
        json!({}),
    ))
}

fn style_delete(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = style_id(ctx, p, "id")?;
    let mut ops = Vec::new();
    for node in ctx.doc.nodes() {
        let mut refs = node.styles;
        for slot in [
            &mut refs.fill,
            &mut refs.stroke,
            &mut refs.text,
            &mut refs.effect,
        ] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        if refs != node.styles {
            ops.push(Op::set(node.id, PropValue::Styles(refs)));
        }
    }
    ops.push(Op::SetStyle { id, style: None });
    Ok(Outcome::apply(
        Transaction::new("Delete style", ops),
        json!({}),
    ))
}

fn set_target(refs: &mut StyleRefs, target: &str, value: Option<StyleId>) {
    match target {
        "fill" => refs.fill = value,
        "stroke" => refs.stroke = value,
        "text" => refs.text = value,
        "grid" => refs.grid = value,
        _ => refs.effect = value,
    }
}

fn style_apply(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let sid = style_id(ctx, p, "style")?;
    let style = ctx
        .doc
        .style(sid)
        .ok_or_else(|| p.bad("style", "missing"))?;
    let default_target = match style.kind {
        StyleKind::Paint(_) => "fill",
        StyleKind::Text(_) => "text",
        StyleKind::Effect(_) => "effect",
        StyleKind::Grid(_) => "grid",
    };
    let target = p.choice("target", TARGETS)?.unwrap_or(default_target);
    let compatible = matches!(
        (&style.kind, target),
        (StyleKind::Paint(_), "fill" | "stroke")
            | (StyleKind::Text(_), "text")
            | (StyleKind::Effect(_), "effect")
            | (StyleKind::Grid(_), "grid")
    );
    if !compatible {
        return Err(p.bad(
            "target",
            format!(
                "a {} style cannot apply to {target}",
                kind_name(&style.kind)
            ),
        ));
    }
    let mut ops = Vec::new();
    for id in ids {
        let node = ctx.doc.node(id)?;
        if target == "text" && node.text().is_none() {
            continue;
        }
        if target == "grid" && !node.kind.is_frame_like() {
            continue;
        }
        let mut refs = node.styles;
        set_target(&mut refs, target, Some(sid));
        ops.push(Op::set(id, PropValue::Styles(refs)));
    }
    Ok(Outcome::apply(
        Transaction::new("Apply style", ops),
        json!({}),
    ))
}

fn style_detach(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let target = p
        .choice("target", TARGETS)?
        .ok_or_else(|| p.missing("target"))?;
    let mut ops = Vec::new();
    for id in ids {
        let node = ctx.doc.node(id)?;
        let mut refs = node.styles;
        set_target(&mut refs, target, None);
        if refs != node.styles {
            ops.push(Op::set(id, PropValue::Styles(refs)));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Detach style", ops),
        json!({}),
    ))
}

fn style_list(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let list: Vec<Value> = ctx
        .doc
        .styles()
        .map(|(id, s)| {
            json!({
                "id": id,
                "name": s.name,
                "kind": kind_name(&s.kind),
                "description": s.description,
                "value": s.kind,
            })
        })
        .collect();
    Ok(Outcome::read(Value::Array(list)))
}

fn parse_var(
    p: &Params<'_>,
    key: &str,
    v: &Value,
    ty: Option<&str>,
) -> Result<VarValue, EngineError> {
    let bad = || p.bad(key, "must be a colour (hex string), number, string or bool");
    match (ty, v) {
        (Some("string"), Value::String(s)) => Ok(VarValue::String(s.clone())),
        (Some("color") | None, Value::String(s)) if deesainer_color::Rgba::from_hex(s).is_ok() => {
            deesainer_color::Rgba::from_hex(s)
                .map(VarValue::Color)
                .map_err(|_| bad())
        }
        (_, Value::String(s)) => Ok(VarValue::String(s.clone())),
        (_, Value::Number(n)) => n
            .as_f64()
            .filter(|f| f.is_finite())
            .map(VarValue::Number)
            .ok_or_else(bad),
        (_, Value::Bool(b)) => Ok(VarValue::Bool(*b)),
        (_, Value::Object(_)) => serde_json::from_value::<deesainer_color::Rgba>(v.clone())
            .map(VarValue::Color)
            .map_err(|_| bad()),
        _ => Err(bad()),
    }
}

fn var_id(ctx: &Context<'_>, p: &Params<'_>, key: &str) -> Result<VariableId, EngineError> {
    let raw = p
        .raw(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| p.missing(key))?;
    let id = VariableId(raw);
    if ctx.doc.variable(id).is_none() {
        return Err(p.bad(key, format!("variable {raw} does not exist")));
    }
    Ok(id)
}

pub(super) fn collection_param(
    ctx: &Context<'_>,
    p: &Params<'_>,
) -> Result<(CollectionId, Modes), EngineError> {
    let id = CollectionId(p.u32("collection")?.map_or(0, u64::from));
    let modes = ctx
        .doc
        .collection(id)
        .cloned()
        .ok_or_else(|| p.bad("collection", format!("collection {id} does not exist")))?;
    Ok((id, modes))
}

pub(super) fn set_modes_op(id: CollectionId, modes: Modes) -> Op {
    if id.is_default() {
        Op::SetModes { modes }
    } else {
        Op::SetCollection {
            id,
            modes: Some(modes),
        }
    }
}

pub(super) fn scopes_param(p: &Params<'_>) -> Result<Option<Vec<BindTarget>>, EngineError> {
    let Some(raw) = p.raw("scopes") else {
        return Ok(None);
    };
    let list = raw
        .as_array()
        .ok_or_else(|| p.bad("scopes", "must be an array"))?;
    let mut out = Vec::new();
    for v in list {
        let name = v.as_str().filter(|n| BIND_TARGETS.contains(n));
        let target: BindTarget = name
            .and_then(|n| serde_json::from_value(json!(n)).ok())
            .ok_or_else(|| {
                p.bad(
                    "scopes",
                    format!("expected one of {}", BIND_TARGETS.join(", ")),
                )
            })?;
        if !out.contains(&target) {
            out.push(target);
        }
    }
    Ok(Some(out))
}

fn variable_create(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (collection, modes) = collection_param(ctx, p)?;
    let n = modes.names.len();
    let ty = p.str("type")?;
    let values: Vec<VarValue> = match (p.raw("values"), p.raw("value")) {
        (Some(Value::Array(list)), _) => {
            if list.len() != n {
                return Err(p.bad("values", format!("needs exactly {n} values (one per mode)")));
            }
            list.iter()
                .map(|v| parse_var(p, "values", v, ty))
                .collect::<Result<_, _>>()?
        }
        (Some(_), _) => return Err(p.bad("values", "must be an array")),
        (None, Some(v)) => vec![parse_var(p, "value", v, ty)?; n],
        (None, None) => return Err(p.invalid("pass value or values")),
    };
    let id = VariableId(ctx.doc.next_free_id().0);
    let var = Variable {
        origin: None,
        name: p.req_str("name")?.to_owned(),
        values,
        collection,
        scopes: scopes_param(p)?.unwrap_or_default(),
    };
    Ok(Outcome::apply(
        Transaction::new(
            "Create variable",
            vec![Op::SetVariable {
                id,
                variable: Some(var),
            }],
        ),
        json!({ "id": id }),
    ))
}

fn variable_set(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = var_id(ctx, p, "id")?;
    let mut var = ctx
        .doc
        .variable(id)
        .cloned()
        .ok_or_else(|| p.bad("id", "missing"))?;
    let active = ctx.doc.collection(var.collection).map_or(0, |m| m.active);
    let mode = p.u32("mode")?.map_or(active, |m| m as usize);
    if mode >= var.values.len() {
        return Err(p.bad("mode", "out of range"));
    }
    if let Some(v) = p.raw("value") {
        let ty = var.values.get(mode).map(VarValue::type_name);
        var.values[mode] = parse_var(p, "value", v, ty)?;
    }
    if let Some(a) = p.raw("alias").and_then(Value::as_u64) {
        let target = VariableId(a);
        if target == id || ctx.doc.variable(target).is_none() {
            return Err(p.bad("alias", "must be another existing variable"));
        }
        var.values[mode] = VarValue::Alias(target);
    }
    if let Some(n) = p.str("name")? {
        var.name = n.to_owned();
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Set variable",
            vec![Op::SetVariable {
                id,
                variable: Some(var),
            }],
        ),
        json!({}),
    ))
}

fn variable_delete(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = var_id(ctx, p, "id")?;
    let mut ops = Vec::new();
    for node in ctx.doc.nodes() {
        if node.bindings.values().any(|v| *v == id) {
            let mut b = node.bindings.clone();
            b.retain(|_, v| *v != id);
            ops.push(Op::set(node.id, PropValue::Bindings(b)));
        }
    }
    ops.push(Op::SetVariable { id, variable: None });
    Ok(Outcome::apply(
        Transaction::new("Delete variable", ops),
        json!({}),
    ))
}

fn bind_target(p: &Params<'_>) -> Result<BindTarget, EngineError> {
    let t = p
        .choice("target", BIND_TARGETS)?
        .ok_or_else(|| p.missing("target"))?;
    serde_json::from_value(json!(t)).map_err(|_| p.bad("target", "unknown"))
}

fn variable_bind(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let target = bind_target(p)?;
    let var = var_id(ctx, p, "variable")?;
    if ctx.doc.variable(var).is_some_and(|v| !v.allows(target)) {
        return Err(p.bad("target", "outside the variable's scopes"));
    }
    let ops = ids
        .iter()
        .filter_map(|id| ctx.doc.get(*id))
        .map(|n| {
            let mut b = n.bindings.clone();
            b.insert(target, var);
            Op::set(n.id, PropValue::Bindings(b))
        })
        .collect();
    Ok(Outcome::apply(
        Transaction::new("Bind variable", ops),
        json!({}),
    ))
}

fn variable_unbind(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let target = bind_target(p)?;
    let ops = ids
        .iter()
        .filter_map(|id| ctx.doc.get(*id))
        .filter(|n| n.bindings.contains_key(&target))
        .map(|n| {
            let mut b = n.bindings.clone();
            b.remove(&target);
            Op::set(n.id, PropValue::Bindings(b))
        })
        .collect();
    Ok(Outcome::apply(
        Transaction::new("Unbind variable", ops),
        json!({}),
    ))
}

fn variable_list(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let vars: Vec<Value> = ctx
        .doc
        .variables()
        .map(|(id, v)| {
            json!({
                "id": id,
                "name": v.name,
                "collection": v.collection,
                "scopes": v.scopes,
                "values": v.values,
                "resolved": ctx.doc.resolve_variable(id),
            })
        })
        .collect();
    let collections: Vec<Value> = ctx
        .doc
        .collections()
        .map(|(id, m)| json!({ "id": id, "name": m.name, "names": m.names, "active": m.active }))
        .collect();
    Ok(Outcome::read(
        json!({ "modes": ctx.doc.modes(), "collections": collections, "variables": vars }),
    ))
}

fn mode_add(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, mut modes) = collection_param(ctx, p)?;
    if modes.names.len() >= 40 {
        return Err(p.invalid("at most 40 modes"));
    }
    let index = modes.names.len();
    modes.names.push(p.req_str("name")?.to_owned());
    Ok(Outcome::apply(
        Transaction::new("Add mode", vec![set_modes_op(id, modes)]),
        json!({ "index": index }),
    ))
}

fn mode_set_active(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, mut modes) = collection_param(ctx, p)?;
    let index = match (p.u32("index")?, p.str("name")?) {
        (Some(i), _) => i as usize,
        (None, Some(n)) => modes
            .names
            .iter()
            .position(|m| m == n)
            .ok_or_else(|| p.bad("name", format!("no mode named {n:?}")))?,
        (None, None) => return Err(p.invalid("pass index or name")),
    };
    if index >= modes.names.len() {
        return Err(p.bad("index", "out of range"));
    }
    modes.active = index;
    Ok(Outcome::apply(
        Transaction::new("Switch mode", vec![set_modes_op(id, modes)]),
        json!({}),
    ))
}

fn mode_rename(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, mut modes) = collection_param(ctx, p)?;
    let i = p.u32("index")?.ok_or_else(|| p.missing("index"))? as usize;
    let slot = modes
        .names
        .get_mut(i)
        .ok_or_else(|| p.bad("index", "out of range"))?;
    *slot = p.req_str("name")?.to_owned();
    Ok(Outcome::apply(
        Transaction::new("Rename mode", vec![set_modes_op(id, modes)]),
        json!({}),
    ))
}
