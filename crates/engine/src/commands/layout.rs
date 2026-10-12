use deesainer_doc::{
    AutoLayout, Constraint, CounterAlign, LayoutDirection, Node, NodeId, NodeKind, Padding,
    PrimaryAlign, PropValue, Sizing,
};
use deesainer_geom::Rect;
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::arrange::bounds_in_parent;
use super::{layer_ids, param, top_level, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

const SIZING: &[&str] = &["fixed", "hug", "fill"];
const H_CONSTRAINTS: &[&str] = &["left", "right", "center", "leftRight", "scale"];
const V_CONSTRAINTS: &[&str] = &["top", "bottom", "center", "topBottom", "scale"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("layout.setAutoLayout", "Auto layout", set_auto_layout)
            .doc("Turns auto-layout on (direction horizontal|vertical), off (none) or edits it. Children are stacked with gap and padding; the frame can hug its content. Omitted keys keep their values.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Frame or component."),
                    param("direction", ParamType::Enum(&["horizontal", "vertical", "grid", "none"]), false, "Stacking direction, a grid, or none to remove auto-layout."),
                    param("gridColumns", ParamType::Integer, false, "Grid: columns, 1..=100 (default 2)."),
                    param("gridRows", ParamType::Integer, false, "Grid: rows of a fixed-height grid, 0..=100 (0: as many as needed)."),
                    param("gap", ParamType::Number, false, "Space between children."),
                    param("padding", ParamType::Json, false, "Number, or {top, right, bottom, left}."),
                    param("primaryAlign", ParamType::Enum(&["start", "center", "end", "spaceBetween"]), false, "Alignment along the direction."),
                    param("counterAlign", ParamType::Enum(&["start", "center", "end"]), false, "Alignment across the direction."),
                    param("primarySizing", ParamType::Enum(&["fixed", "hug"]), false, "Frame size along the direction."),
                    param("counterSizing", ParamType::Enum(&["fixed", "hug"]), false, "Frame size across the direction."),
                    param("wrap", ParamType::Bool, false, "Wrap children onto new lines (horizontal only)."),
                    param("counterGap", ParamType::Number, false, "Space between wrapped lines."),
                ]
            }),
        CommandSpec::new("layout.addAutoLayout", "Add auto layout", add_auto_layout)
            .menu("Object/Add auto layout")
            .key("Shift+A")
            .params(const { &[IDS] })
            .doc("On a frame without auto-layout, turns it on. Otherwise wraps the selected siblings in a new hugging auto-layout frame, inferring direction and gap from their arrangement. Returns {id}."),
        CommandSpec::new("layout.setChild", "Resizing", set_child)
            .doc("How layers size inside an auto-layout parent: fixed, hug (content) or fill (share the free space); absolute takes a layer out of the flow.")
            .params(const {
                &[
                    IDS,
                    param("horizontal", ParamType::Enum(SIZING), false, "Width behaviour."),
                    param("vertical", ParamType::Enum(SIZING), false, "Height behaviour."),
                    param("absolute", ParamType::Bool, false, "Ignore auto-layout (absolute position)."),
                ]
            }),
        CommandSpec::new("node.setConstraints", "Constraints", set_constraints)
            .doc("How layers react when their (non auto-layout) parent frame is resized.")
            .params(const {
                &[
                    IDS,
                    param("horizontal", ParamType::Enum(H_CONSTRAINTS), false, "left, right, center, leftRight (stretch) or scale."),
                    param("vertical", ParamType::Enum(V_CONSTRAINTS), false, "top, bottom, center, topBottom (stretch) or scale."),
                ]
            }),
    ]
}

fn parse_padding(p: &Params<'_>) -> Result<Option<Padding>, EngineError> {
    let Some(v) = p.raw("padding") else {
        return Ok(None);
    };
    let bad = || {
        p.bad(
            "padding",
            "must be a number >= 0 or {top, right, bottom, left}",
        )
    };
    let pad = match v {
        Value::Number(n) => Padding::uniform(
            n.as_f64()
                .filter(|f| f.is_finite() && *f >= 0.0)
                .ok_or_else(bad)?,
        ),
        Value::Object(_) => serde_json::from_value::<Padding>(v.clone()).map_err(|_| bad())?,
        _ => return Err(bad()),
    };
    if [pad.top, pad.right, pad.bottom, pad.left]
        .iter()
        .any(|x| !x.is_finite() || *x < 0.0)
    {
        return Err(bad());
    }
    Ok(Some(pad))
}

fn set_auto_layout(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    if !matches!(
        node.kind,
        NodeKind::Frame | NodeKind::Component { .. } | NodeKind::ComponentSet
    ) {
        return Err(p.bad("id", "auto-layout needs a frame or component"));
    }
    let mut layout = node.layout.unwrap_or(AutoLayout {
        primary_sizing: Sizing::Fixed,
        counter_sizing: Sizing::Fixed,
        ..AutoLayout::default()
    });
    match p.choice("direction", &["horizontal", "vertical", "grid", "none"])? {
        Some("none") => {
            return Ok(Outcome::apply(
                Transaction::new(
                    "Remove auto layout",
                    vec![Op::set(id, PropValue::Layout(None))],
                ),
                json!({}),
            ))
        }
        Some("horizontal") => layout.direction = LayoutDirection::Horizontal,
        Some("vertical") => layout.direction = LayoutDirection::Vertical,
        Some("grid") => layout.direction = LayoutDirection::Grid,
        _ => {}
    }
    if let Some(c) = p.u32("gridColumns")? {
        if !(1..=100).contains(&c) {
            return Err(p.bad("gridColumns", "must be 1 to 100"));
        }
        layout.grid_columns = c;
    }
    if let Some(r) = p.u32("gridRows")? {
        if r > 100 {
            return Err(p.bad("gridRows", "must be 0 to 100"));
        }
        layout.grid_rows = r;
    }
    if let Some(g) = p.non_negative("gap")? {
        layout.gap = g;
    }
    if let Some(g) = p.non_negative("counterGap")? {
        layout.counter_gap = g;
    }
    if let Some(pad) = parse_padding(p)? {
        layout.padding = pad;
    }
    match p.choice("primaryAlign", &["start", "center", "end", "spaceBetween"])? {
        Some("start") => layout.primary_align = PrimaryAlign::Start,
        Some("center") => layout.primary_align = PrimaryAlign::Center,
        Some("end") => layout.primary_align = PrimaryAlign::End,
        Some("spaceBetween") => layout.primary_align = PrimaryAlign::SpaceBetween,
        _ => {}
    }
    match p.choice("counterAlign", &["start", "center", "end"])? {
        Some("start") => layout.counter_align = CounterAlign::Start,
        Some("center") => layout.counter_align = CounterAlign::Center,
        Some("end") => layout.counter_align = CounterAlign::End,
        _ => {}
    }
    let sizing = |s: &str| {
        if s == "hug" {
            Sizing::Hug
        } else {
            Sizing::Fixed
        }
    };
    if let Some(s) = p.choice("primarySizing", &["fixed", "hug"])? {
        layout.primary_sizing = sizing(s);
    }
    if let Some(s) = p.choice("counterSizing", &["fixed", "hug"])? {
        layout.counter_sizing = sizing(s);
    }
    if let Some(w) = p.bool("wrap")? {
        layout.wrap = w;
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Auto layout",
            vec![Op::set(id, PropValue::Layout(Some(layout)))],
        ),
        json!({}),
    ))
}

fn add_auto_layout(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    if let [one] = ids.as_slice() {
        let node = ctx.doc.node(*one)?;
        if matches!(node.kind, NodeKind::Frame | NodeKind::Component { .. })
            && node.layout.is_none()
        {
            let layout = infer_layout(ctx, &node.children);
            return Ok(Outcome::apply(
                Transaction::new(
                    "Add auto layout",
                    vec![Op::set(*one, PropValue::Layout(Some(layout)))],
                ),
                json!({ "id": one }),
            ));
        }
    }
    let parent = ctx
        .doc
        .node(ids[0])?
        .parent
        .ok_or_else(|| p.invalid("layer without parent"))?;
    if ids
        .iter()
        .any(|id| ctx.doc.get(*id).and_then(|n| n.parent) != Some(parent))
    {
        return Err(p.invalid("select layers that share the same parent"));
    }
    let mut layout = infer_layout(ctx, &ids);
    layout.primary_sizing = Sizing::Hug;
    layout.counter_sizing = Sizing::Hug;
    let mut ordered: Vec<(f64, NodeId)> = ids
        .iter()
        .filter_map(|id| {
            ctx.doc.get(*id).map(|n| {
                let b = bounds_in_parent(n);
                (
                    if layout.direction == LayoutDirection::Horizontal {
                        b.x
                    } else {
                        b.y
                    },
                    *id,
                )
            })
        })
        .collect();
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
    let bounds = ids
        .iter()
        .filter_map(|id| ctx.doc.get(*id).map(bounds_in_parent))
        .reduce(Rect::union)
        .ok_or_else(|| p.invalid("nothing to wrap"))?;
    let fid = ctx.doc.next_free_id();
    let mut frame = Node::new(fid, "Frame", NodeKind::Frame, bounds);
    frame.layout = Some(layout);
    frame.clips_content = false;
    let top = ids
        .iter()
        .filter_map(|id| ctx.doc.index_in_parent(*id).ok())
        .max()
        .unwrap_or(0);
    let mut ops = vec![Op::CreateNode {
        parent,
        index: top + 1,
        node: Box::new(frame),
    }];
    for (k, (_, id)) in ordered.iter().enumerate() {
        ops.push(Op::MoveNode {
            id: *id,
            parent: fid,
            index: k,
        });
    }
    Ok(Outcome::apply(
        Transaction::new("Add auto layout", ops),
        json!({ "id": fid }),
    )
    .selecting(vec![fid]))
}

fn infer_layout(ctx: &Context<'_>, ids: &[NodeId]) -> AutoLayout {
    let rects: Vec<Rect> = ids
        .iter()
        .filter_map(|id| ctx.doc.get(*id).map(bounds_in_parent))
        .collect();
    let horizontal = match (
        rects.iter().map(|r| r.center().x).reduce(f64::max),
        rects.iter().map(|r| r.center().x).reduce(f64::min),
        rects.iter().map(|r| r.center().y).reduce(f64::max),
        rects.iter().map(|r| r.center().y).reduce(f64::min),
    ) {
        (Some(x1), Some(x0), Some(y1), Some(y0)) => (x1 - x0) >= (y1 - y0),
        _ => false,
    };
    let mut sorted = rects.clone();
    sorted.sort_by(|a, b| {
        if horizontal {
            a.x.total_cmp(&b.x)
        } else {
            a.y.total_cmp(&b.y)
        }
    });
    let gaps: Vec<f64> = sorted
        .windows(2)
        .map(|w| {
            if horizontal {
                w[1].x - w[0].max_x()
            } else {
                w[1].y - w[0].max_y()
            }
        })
        .collect();
    let gap = if gaps.is_empty() {
        10.0
    } else {
        (gaps.iter().sum::<f64>() / gaps.len() as f64)
            .max(0.0)
            .round()
    };
    AutoLayout {
        direction: if horizontal {
            LayoutDirection::Horizontal
        } else {
            LayoutDirection::Vertical
        },
        gap,
        padding: Padding::uniform(0.0),
        primary_sizing: Sizing::Hug,
        counter_sizing: Sizing::Hug,
        ..AutoLayout::default()
    }
}

fn set_child(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let parse = |s: &str| match s {
        "hug" => Sizing::Hug,
        "fill" => Sizing::Fill,
        _ => Sizing::Fixed,
    };
    let h = p.choice("horizontal", SIZING)?.map(parse);
    let v = p.choice("vertical", SIZING)?.map(parse);
    let abs = p.bool("absolute")?;
    let mut ops = Vec::new();
    for id in ids {
        let node = ctx.doc.node(id)?;
        let mut lc = node.layout_child;
        let hug_ok = node.text().is_some() || node.layout.is_some();
        if let Some(h) = h {
            if h == Sizing::Hug && !hug_ok {
                return Err(p.bad(
                    "horizontal",
                    "hug needs a text layer or an auto-layout frame",
                ));
            }
            lc.horizontal = h;
        }
        if let Some(v) = v {
            if v == Sizing::Hug && !hug_ok {
                return Err(p.bad("vertical", "hug needs a text layer or an auto-layout frame"));
            }
            lc.vertical = v;
        }
        if let Some(a) = abs {
            lc.absolute = a;
        }
        ops.push(Op::set(id, PropValue::LayoutChild(lc)));
        if let Some(mut l) = node.layout {
            let horizontal = l.direction == LayoutDirection::Horizontal;
            let map = |s: Sizing| {
                if s == Sizing::Hug {
                    Sizing::Hug
                } else {
                    Sizing::Fixed
                }
            };
            if let Some(h) = h {
                if horizontal {
                    l.primary_sizing = map(h)
                } else {
                    l.counter_sizing = map(h)
                }
            }
            if let Some(v) = v {
                if horizontal {
                    l.counter_sizing = map(v)
                } else {
                    l.primary_sizing = map(v)
                }
            }
            ops.push(Op::set(id, PropValue::Layout(Some(l))));
        }
    }
    Ok(Outcome::apply(Transaction::new("Resizing", ops), json!({})))
}

fn set_constraints(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let h = p.choice("horizontal", H_CONSTRAINTS)?.map(|s| match s {
        "right" => Constraint::Max,
        "center" => Constraint::Center,
        "leftRight" => Constraint::Stretch,
        "scale" => Constraint::Scale,
        _ => Constraint::Min,
    });
    let v = p.choice("vertical", V_CONSTRAINTS)?.map(|s| match s {
        "bottom" => Constraint::Max,
        "center" => Constraint::Center,
        "topBottom" => Constraint::Stretch,
        "scale" => Constraint::Scale,
        _ => Constraint::Min,
    });
    let ops = ids
        .iter()
        .filter_map(|id| ctx.doc.get(*id))
        .map(|n| {
            let mut c = n.constraints;
            if let Some(h) = h {
                c.horizontal = h;
            }
            if let Some(v) = v {
                c.vertical = v;
            }
            Op::set(n.id, PropValue::Constraints(c))
        })
        .collect();
    Ok(Outcome::apply(
        Transaction::new("Constraints", ops),
        json!({}),
    ))
}
