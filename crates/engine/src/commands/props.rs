use deesainer_color::BlendMode;
use deesainer_doc::{
    BindTarget, CornerRadius, Effect, Node, NodeId, NodeKind, Paint, PropValue, StrokeAlign,
    StrokeStyle, StyleRefs,
};
use deesainer_geom::Rect;
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::arrange::normalize_degrees;
use super::{layer_ids, param, validate_paints, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandKind, CommandSpec, Context, Outcome, ParamType};

const BLEND_MODES: &[&str] = &[
    "passThrough",
    "normal",
    "darken",
    "multiply",
    "colorBurn",
    "lighten",
    "screen",
    "colorDodge",
    "overlay",
    "softLight",
    "hardLight",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("node.move", "Move", node_move)
            .kind(CommandKind::Interactive)
            .doc("Translates layers by (dx, dy) in page space. One history step.")
            .params(const {
                &[
                    IDS,
                    param("dx", ParamType::Number, true, "Horizontal offset in page space."),
                    param("dy", ParamType::Number, true, "Vertical offset in page space."),
                ]
            }),
        CommandSpec::new("node.setPosition", "Set position", set_position)
            .doc("Sets a layer's top-left corner in its parent's space (the X/Y fields of the design panel).")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer."),
                    param("x", ParamType::Number, false, "New x; unchanged when omitted."),
                    param("y", ParamType::Number, false, "New y; unchanged when omitted."),
                ]
            }),
        CommandSpec::new("node.resize", "Resize", node_resize)
            .kind(CommandKind::Interactive)
            .doc("Sets width/height keeping the top-left corner (or the given x/y). Vectors scale their path; children follow their constraints.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer."),
                    param("width", ParamType::Number, false, "New width >= 0."),
                    param("height", ParamType::Number, false, "New height >= 0."),
                    param("x", ParamType::Number, false, "New x (for resizing from the left)."),
                    param("y", ParamType::Number, false, "New y (for resizing from the top)."),
                ]
            }),
        CommandSpec::new("node.setRotation", "Rotate", set_rotation)
            .kind(CommandKind::Interactive)
            .doc("Sets the rotation in degrees (clockwise, around the box centre), normalized to -180..180.")
            .params(const {
                &[
                    IDS,
                    param("degrees", ParamType::Number, true, "Absolute rotation."),
                ]
            }),
        CommandSpec::new("node.setProps", "Set properties", set_props)
            .kind(CommandKind::Dialog)
            .doc("Sets simple properties on layers. Omitted keys are unchanged. For gradients, images and multiple paints use node.setFills / node.setStrokes.")
            .params(const {
                &[
                    IDS,
                    param("name", ParamType::String, false, "Layer name."),
                    param("visible", ParamType::Bool, false, "Show or hide."),
                    param("locked", ParamType::Bool, false, "Lock or unlock."),
                    param("opacity", ParamType::Number, false, "Layer opacity 0..=1."),
                    param("blend", ParamType::Enum(BLEND_MODES), false, "Layer blend mode."),
                    param("cornerRadius", ParamType::Number, false, "Uniform corner radius >= 0."),
                    param("cornerRadii", ParamType::Json, false, "Per-corner radii >= 0: [topLeft, topRight, bottomRight, bottomLeft]."),
                    param("clipsContent", ParamType::Bool, false, "Frames: clip children to the frame."),
                    param("fill", ParamType::Color, false, "Replaces the fills with one solid colour."),
                    param("stroke", ParamType::Color, false, "Replaces the strokes with one solid colour."),
                    param("strokeWeight", ParamType::Number, false, "Stroke weight >= 0."),
                    param("strokeAlign", ParamType::Enum(&["inside", "center", "outside"]), false, "Stroke position."),
                    param("strokeSides", ParamType::Json, false, "Rectangles and frames: stroke weights [top, right, bottom, left], or null for one weight."),
                    param("isMask", ParamType::Bool, false, "Use this layer as a mask for the layers above it."),
                    param("layoutGrids", ParamType::Json, false, "Layout guides for frames: [{pattern: grid|columns|rows, size, count, gutter, offset, align: stretch|min|center|max, color, visible}]."),
                    param("minWidth", ParamType::Json, false, "Minimum width in auto layout (a number >= 0), or null for none."),
                    param("maxWidth", ParamType::Json, false, "Maximum width in auto layout, or null for none."),
                    param("minHeight", ParamType::Json, false, "Minimum height in auto layout, or null for none."),
                    param("maxHeight", ParamType::Json, false, "Maximum height in auto layout, or null for none."),
                ]
            }),
        CommandSpec::new("node.setFills", "Set fills", set_fills)
            .doc("Replaces fills with a paint list (bottom first). Paint: {type: solid|linearGradient|radialGradient|angularGradient|diamondGradient|image, color, start, end, center, radius, angle (angular: degrees clockwise from +x), stops:[{position, color}], asset, scaleMode, opacity, visible, blend}. Colours are {r,g,b,a} 0..1. Gradient handles are 0..1 in the box.")
            .params(const {
                &[
                    IDS,
                    param("fills", ParamType::Json, true, "Array of paints."),
                ]
            }),
        CommandSpec::new("node.setStrokes", "Set strokes", set_strokes)
            .doc("Replaces strokes (paint list, same shape as fills) and/or the stroke style {weight, align: inside|center|outside, cap: none|round|square, join: miter|round|bevel, dash: [..]}.")
            .params(const {
                &[
                    IDS,
                    param("strokes", ParamType::Json, false, "Array of paints."),
                    param("style", ParamType::Json, false, "Stroke style object."),
                ]
            }),
        CommandSpec::new("node.setEffects", "Set effects", set_effects)
            .doc("Replaces effects. Effect: {type: dropShadow|innerShadow, color, offset:{x,y}, blur, spread, visible} or {type: layerBlur|backgroundBlur, radius, visible}.")
            .params(const {
                &[
                    IDS,
                    param("effects", ParamType::Json, true, "Array of effects."),
                ]
            }),
    ]
}

fn node_move(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = super::top_level(ctx.doc, &layer_ids(ctx, p)?);
    let (dx, dy) = (p.req_f64("dx")?, p.req_f64("dy")?);
    let mut ops = Vec::with_capacity(ids.len());
    for id in ids {
        let node = ctx.doc.node(id)?;
        let (ldx, ldy) = super::arrange::world_delta_to_parent(ctx.doc, node, dx, dy);
        let rect = node.rect.translate(ldx, ldy);
        if !rect.is_valid() {
            return Err(p.invalid("the offset moves a layer out of range"));
        }
        ops.push(Op::set(id, PropValue::Rect(rect)));
    }
    Ok(Outcome::apply(Transaction::new("Move", ops), json!({})))
}

fn layer(ctx: &Context<'_>, p: &Params<'_>) -> Result<NodeId, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    if matches!(node.kind, NodeKind::Page | NodeKind::Document) {
        return Err(p.bad("id", "pages have no position or size"));
    }
    Ok(id)
}

fn set_position(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = layer(ctx, p)?;
    let node = ctx.doc.node(id)?;
    let x = p.f64("x")?.unwrap_or(node.rect.x);
    let y = p.f64("y")?.unwrap_or(node.rect.y);
    let rect = node.rect.with_origin(x, y);
    Ok(Outcome::apply(
        Transaction::new("Set position", vec![Op::set(id, PropValue::Rect(rect))]),
        json!({}),
    ))
}

pub(crate) fn resize_ops(
    ctx: &Context<'_>,
    id: NodeId,
    rect: Rect,
) -> Result<Vec<Op>, EngineError> {
    let node = ctx.doc.node(id)?;
    let mut ops = vec![Op::set(id, PropValue::Rect(rect))];
    if let NodeKind::Vector { path } = &node.kind {
        let scaled = path.rescale(node.rect.width, node.rect.height, rect.width, rect.height);
        ops.push(Op::set(
            id,
            PropValue::Kind(NodeKind::Vector { path: scaled }),
        ));
    }
    ops.extend(crate::constraints::child_ops(ctx.doc, id, node.rect, rect)?);
    Ok(ops)
}

fn node_resize(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = layer(ctx, p)?;
    let node = ctx.doc.node(id)?;
    let rect = Rect::new(
        p.f64("x")?.unwrap_or(node.rect.x),
        p.f64("y")?.unwrap_or(node.rect.y),
        p.f64("width")?.unwrap_or(node.rect.width),
        p.f64("height")?.unwrap_or(node.rect.height),
    );
    if !rect.is_valid() {
        return Err(p.bad("width/height", "must be >= 0"));
    }
    let ops = resize_ops(ctx, id, rect)?;
    Ok(Outcome::apply(Transaction::new("Resize", ops), json!({})))
}

fn set_rotation(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let deg = normalize_degrees(p.req_f64("degrees")?);
    let ops = ids
        .iter()
        .map(|id| Op::set(*id, PropValue::Rotation(deg)))
        .collect();
    Ok(Outcome::apply(Transaction::new("Rotate", ops), json!({})))
}

fn parse_blend(s: &str) -> Option<BlendMode> {
    serde_json::from_value(json!(s)).ok()
}

fn detach_ops(node: &Node, styles: &[fn(&mut StyleRefs)], targets: &[BindTarget]) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut refs = node.styles;
    for clear in styles {
        clear(&mut refs);
    }
    if refs != node.styles {
        ops.push(Op::set(node.id, PropValue::Styles(refs)));
    }
    if targets.iter().any(|t| node.bindings.contains_key(t)) {
        let mut b = node.bindings.clone();
        for t in targets {
            b.remove(t);
        }
        ops.push(Op::set(node.id, PropValue::Bindings(b)));
    }
    ops
}

fn clear_fill(r: &mut StyleRefs) {
    r.fill = None;
}

fn clear_stroke(r: &mut StyleRefs) {
    r.stroke = None;
}

fn clear_effect(r: &mut StyleRefs) {
    r.effect = None;
}

fn set_props(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let mut ops = Vec::new();
    for id in ids {
        let node = ctx.doc.node(id)?;
        let mut styles: Vec<fn(&mut StyleRefs)> = Vec::new();
        let mut targets: Vec<BindTarget> = Vec::new();
        if let Some(name) = p.str("name")? {
            ops.push(Op::set(id, PropValue::Name(name.to_owned())));
        }
        if let Some(v) = p.bool("visible")? {
            ops.push(Op::set(id, PropValue::Visible(v)));
        }
        if let Some(v) = p.bool("locked")? {
            ops.push(Op::set(id, PropValue::Locked(v)));
        }
        if let Some(o) = p.f64("opacity")? {
            if !(0.0..=1.0).contains(&o) {
                return Err(p.bad("opacity", "must be between 0 and 1"));
            }
            ops.push(Op::set(id, PropValue::Opacity(o as f32)));
            targets.push(BindTarget::Opacity);
        }
        if let Some(b) = p.choice("blend", BLEND_MODES)? {
            let mode = parse_blend(b).ok_or_else(|| p.bad("blend", "unknown blend mode"))?;
            ops.push(Op::set(id, PropValue::Blend(mode)));
        }
        if let Some(r) = p.non_negative("cornerRadius")? {
            ops.push(Op::set(
                id,
                PropValue::CornerRadius(CornerRadius::uniform(r)),
            ));
            targets.push(BindTarget::CornerRadius);
        }
        if let Some(raw) = p.raw("cornerRadii") {
            let values: Option<Vec<f64>> = raw
                .as_array()
                .filter(|a| a.len() == 4)
                .map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect());
            let Some([tl, tr, br, bl]) = values
                .filter(|v| v.iter().all(|r| r.is_finite() && *r >= 0.0))
                .and_then(|v| <[f64; 4]>::try_from(v).ok())
            else {
                return Err(p.bad(
                    "cornerRadii",
                    "must be four radii >= 0: [topLeft, topRight, bottomRight, bottomLeft]",
                ));
            };
            ops.push(Op::set(
                id,
                PropValue::CornerRadius(CornerRadius {
                    top_left: tl,
                    top_right: tr,
                    bottom_right: br,
                    bottom_left: bl,
                }),
            ));
            targets.push(BindTarget::CornerRadius);
        }
        if let Some(c) = p.bool("clipsContent")? {
            ops.push(Op::set(id, PropValue::ClipsContent(c)));
        }
        if let Some(color) = p.color("fill")? {
            ops.push(Op::set(id, PropValue::Fills(vec![Paint::solid(color)])));
            styles.push(clear_fill);
            targets.push(BindTarget::FillColor);
        }
        if let Some(color) = p.color("stroke")? {
            ops.push(Op::set(id, PropValue::Strokes(vec![Paint::solid(color)])));
            styles.push(clear_stroke);
            targets.push(BindTarget::StrokeColor);
        }
        ops.extend(detach_ops(node, &styles, &targets));
        let weight = p.non_negative("strokeWeight")?;
        let align = p.choice("strokeAlign", &["inside", "center", "outside"])?;
        let sides_given = p.given("strokeSides");
        if weight.is_some() || align.is_some() || sides_given.is_some() {
            let mut style: StrokeStyle = node.stroke.clone();
            if let Some(w) = weight {
                style.weight = w;
            }
            match align {
                Some("inside") => style.align = StrokeAlign::Inside,
                Some("center") => style.align = StrokeAlign::Center,
                Some("outside") => style.align = StrokeAlign::Outside,
                _ => {}
            }
            match sides_given {
                Some(serde_json::Value::Null) => style.sides = None,
                Some(v) => {
                    let sides: [f64; 4] = serde_json::from_value(v.clone())
                        .ok()
                        .filter(|s: &[f64; 4]| {
                            s.iter()
                                .all(|x| x.is_finite() && (0.0..=10_000.0).contains(x))
                        })
                        .ok_or_else(|| {
                            p.bad("strokeSides", "needs four weights from 0 to 10000")
                        })?;
                    style.weight = sides.iter().copied().fold(0.0, f64::max);
                    style.sides = Some(sides);
                }
                None => {
                    if weight.is_some() {
                        style.sides = None;
                    }
                }
            }
            ops.push(Op::set(id, PropValue::Stroke(style)));
        }
        if let Some(m) = p.bool("isMask")? {
            ops.push(Op::set(id, PropValue::IsMask(m)));
        }
        if ["minWidth", "maxWidth", "minHeight", "maxHeight"]
            .iter()
            .any(|k| p.given(k).is_some())
        {
            let mut limits = node.size_limits;
            for (key, slot) in [
                ("minWidth", &mut limits.min_width),
                ("maxWidth", &mut limits.max_width),
                ("minHeight", &mut limits.min_height),
                ("maxHeight", &mut limits.max_height),
            ] {
                match p.given(key) {
                    None => {}
                    Some(serde_json::Value::Null) => *slot = None,
                    Some(v) => match v.as_f64() {
                        Some(n) if n.is_finite() && (0.0..=1e6).contains(&n) => *slot = Some(n),
                        _ => return Err(p.bad(key, "must be a number from 0 to 1000000, or null")),
                    },
                }
            }
            let crossed =
                |a: Option<f64>, b: Option<f64>| matches!((a, b), (Some(a), Some(b)) if a > b);
            if crossed(limits.min_width, limits.max_width)
                || crossed(limits.min_height, limits.max_height)
            {
                return Err(p.invalid("a minimum cannot be larger than its maximum"));
            }
            ops.push(Op::set(id, PropValue::SizeLimits(limits)));
        }
        if let Some(g) = p.json::<Vec<deesainer_doc::LayoutGrid>>("layoutGrids")? {
            if g.len() > 64 {
                return Err(EngineError::BadParam {
                    command: p.command().to_owned(),
                    key: "layoutGrids".into(),
                    reason: "allows at most 64 layout grids".into(),
                });
            }
            ops.push(Op::set(id, PropValue::LayoutGrids(g)));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Set properties", ops),
        json!({}),
    ))
}

fn set_fills(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let fills: Vec<Paint> = p.req_json("fills")?;
    validate_paints(p, "fills", &fills)?;
    let mut ops = Vec::new();
    for id in &ids {
        ops.push(Op::set(*id, PropValue::Fills(fills.clone())));
        ops.extend(detach_ops(
            ctx.doc.node(*id)?,
            &[clear_fill],
            &[BindTarget::FillColor],
        ));
    }
    Ok(Outcome::apply(
        Transaction::new("Set fills", ops),
        json!({}),
    ))
}

fn set_strokes(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let strokes: Option<Vec<Paint>> = p.json("strokes")?;
    let mut style: Option<StrokeStyle> = p.json("style")?;
    if strokes.is_none() && style.is_none() {
        return Err(p.invalid("pass strokes and/or style"));
    }
    if let Some(s) = &strokes {
        validate_paints(p, "strokes", s)?;
    }
    if let Some(st) = &style {
        if !st.weight.is_finite()
            || st.weight < 0.0
            || st.dash.iter().any(|d| !d.is_finite() || *d < 0.0)
        {
            return Err(p.bad("style", "weight and dashes must be finite and >= 0"));
        }
    }
    if let Some(st) = &mut style {
        if let Some(sides) = st.sides {
            if sides
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=10_000.0).contains(v))
            {
                return Err(p.bad("style", "sides must be four weights from 0 to 10000"));
            }
            st.weight = sides.iter().copied().fold(0.0, f64::max);
        }
    }
    let mut ops = Vec::new();
    for id in ids {
        if let Some(s) = &strokes {
            ops.push(Op::set(id, PropValue::Strokes(s.clone())));
            ops.extend(detach_ops(
                ctx.doc.node(id)?,
                &[clear_stroke],
                &[BindTarget::StrokeColor],
            ));
        }
        if let Some(st) = &style {
            ops.push(Op::set(id, PropValue::Stroke(st.clone())));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Set strokes", ops),
        json!({}),
    ))
}

fn set_effects(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let effects: Vec<Effect> = p.req_json("effects")?;
    for e in &effects {
        let ok = match e {
            Effect::DropShadow {
                color,
                offset,
                blur,
                spread,
                ..
            }
            | Effect::InnerShadow {
                color,
                offset,
                blur,
                spread,
                ..
            } => {
                color.is_finite()
                    && offset.is_finite()
                    && blur.is_finite()
                    && *blur >= 0.0
                    && spread.is_finite()
            }
            Effect::LayerBlur { radius, .. } | Effect::BackgroundBlur { radius, .. } => {
                radius.is_finite() && *radius >= 0.0 && *radius <= 1000.0
            }
        };
        if !ok {
            return Err(p.bad("effects", "numbers must be finite; blur and radius >= 0"));
        }
    }
    let mut ops = Vec::new();
    for id in &ids {
        ops.push(Op::set(*id, PropValue::Effects(effects.clone())));
        ops.extend(detach_ops(ctx.doc.node(*id)?, &[clear_effect], &[]));
    }
    Ok(Outcome::apply(
        Transaction::new("Set effects", ops),
        json!({}),
    ))
}
