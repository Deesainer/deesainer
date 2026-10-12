use deesainer_color::Rgba;
use deesainer_doc::{
    BoolOp, Document, FillRule, Node, NodeId, NodeKind, Paint, PaintKind, PathCmd, PropValue,
    VectorPath,
};
use deesainer_geom::{Point, Rect};
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::arrange::bounds_in_parent;
use super::create::{insert_top, SHAPE_FILL};
use super::{layer_ids, param, target_parent, top_level, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandKind, CommandSpec, Context, Outcome, ParamType, RunFn};

pub fn specs() -> Vec<CommandSpec> {
    let ids_only = const { &[IDS] };
    let boolean = |id: &'static str, label: &'static str, key: &'static str, run: RunFn| {
        CommandSpec::new(id, label, run)
            .menu(match id {
                "object.booleanUnion" => "Object/Boolean/Union selection",
                "object.booleanSubtract" => "Object/Boolean/Subtract selection",
                "object.booleanIntersect" => "Object/Boolean/Intersect selection",
                _ => "Object/Boolean/Exclude selection",
            })
            .key(key)
            .params(ids_only)
            .doc("Combines sibling layers into a boolean group (children stay editable). Subtract removes upper layers from the bottom one. Returns {id}.")
    };
    vec![
        CommandSpec::new("node.create.vector", "Vector", create_vector)
            .menu("Tools/Pen")
            .key("P")
            .kind(CommandKind::Interactive)
            .doc("Creates a vector layer from path commands [{cmd: moveTo|lineTo|quadTo|cubicTo|close, to:{x,y}, ctrl, ctrl1, ctrl2}]. The layer's box is fitted to the path. Open paths get a 1pt black stroke, closed ones a grey fill. Returns {id}.")
            .params(const {
                &[
                    param("parent", ParamType::NodeId, false, "Container; defaults to the current page."),
                    param("cmds", ParamType::Json, true, "Path commands."),
                    param("space", ParamType::Enum(&["parent", "page"]), false, "Coordinate space of the points, default parent."),
                    param("fillRule", ParamType::Enum(&["nonZero", "evenOdd"]), false, "Fill rule, default nonZero."),
                    param("name", ParamType::String, false, "Layer name, default \"Vector\"."),
                ]
            }),
        CommandSpec::new("vector.setPath", "Edit path", set_path)
            .kind(CommandKind::Interactive)
            .doc("Replaces a vector layer's path (points in the layer's local box). The box is refitted so the layer does not move on the page.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("cmds", ParamType::Json, true, "Path commands in local coordinates."),
                ]
            }),
        boolean("object.booleanUnion", "Union selection", "Alt+Shift+U", |c, p| boolean_op(c, p, BoolOp::Union)),
        boolean("object.booleanSubtract", "Subtract selection", "Alt+Shift+S", |c, p| boolean_op(c, p, BoolOp::Subtract)),
        boolean("object.booleanIntersect", "Intersect selection", "Alt+Shift+I", |c, p| boolean_op(c, p, BoolOp::Intersect)),
        boolean("object.booleanExclude", "Exclude selection", "Alt+Shift+E", |c, p| boolean_op(c, p, BoolOp::Exclude)),
        CommandSpec::new("object.flatten", "Flatten selection", flatten)
            .menu("Object/Flatten selection")
            .key("Cmd+E")
            .params(ids_only)
            .doc("Replaces layers with one vector layer of their combined outline (rotation baked in). Fills and strokes come from the top layer. Returns {id}."),
        CommandSpec::new("object.outlineStroke", "Outline stroke", outline_stroke)
            .menu("Object/Outline stroke")
            .key("Alt+Cmd+O")
            .params(ids_only)
            .doc("Turns each layer's stroke into a filled vector layer placed above it, and removes the stroke. Returns {ids}."),
        CommandSpec::new("object.flipHorizontal", "Flip horizontal", |c, p| flip(c, p, true))
            .menu("Object/Flip horizontal")
            .key("Shift+H")
            .params(ids_only)
            .doc("Mirrors layers left-right inside their own box (paths, gradients, children, rotation)."),
        CommandSpec::new("object.flipVertical", "Flip vertical", |c, p| flip(c, p, false))
            .menu("Object/Flip vertical")
            .key("Shift+V")
            .params(ids_only)
            .doc("Mirrors layers top-bottom inside their own box."),
        CommandSpec::new("node.setShape", "Shape options", set_shape)
            .doc("Shape parameters: polygon/star point count, star inner ratio, ellipse arc start/end (degrees) and inner ratio (donut).")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Polygon, star or ellipse."),
                    param("count", ParamType::Integer, false, "Sides or points, 3..=60."),
                    param("innerRatio", ParamType::Number, false, "Star inner radius or ellipse hole, 0..1."),
                    param("arcStart", ParamType::Number, false, "Ellipse arc start in degrees."),
                    param("arcEnd", ParamType::Number, false, "Ellipse arc end in degrees."),
                ]
            }),
    ]
}

pub(crate) fn fit_path(path: &VectorPath) -> Option<(Rect, VectorPath)> {
    let b = deesainer_vector::path_bounds(path)?;
    if !b.is_valid() {
        return None;
    }
    let local = path.map_points(|q| Point::new(q.x - b.x, q.y - b.y));
    Some((b, local))
}

fn is_closed(path: &VectorPath) -> bool {
    path.cmds.iter().any(|c| matches!(c, PathCmd::Close))
}

fn read_path(p: &Params<'_>, key: &str) -> Result<VectorPath, EngineError> {
    let cmds: Vec<PathCmd> = p.req_json(key)?;
    if cmds.len() > 100_000 {
        return Err(p.bad(key, "has more than 100000 commands"));
    }
    let path = VectorPath {
        cmds,
        fill_rule: FillRule::NonZero,
    };
    if !path.is_finite() {
        return Err(p.bad(key, "points must be finite"));
    }
    if !matches!(path.cmds.first(), Some(PathCmd::MoveTo { .. })) {
        return Err(p.bad(key, "must start with moveTo"));
    }
    Ok(path)
}

fn create_vector(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let parent = target_parent(ctx, p)?;
    let mut path = read_path(p, "cmds")?;
    if p.str("space")? == Some("page") {
        if let Some(inv) = ctx.doc.world_transform(parent).invert() {
            path = path.map_points(|q| inv.apply(q));
        }
    }
    if p.str("fillRule")? == Some("evenOdd") {
        path.fill_rule = FillRule::EvenOdd;
    }
    let (rect, local) = fit_path(&path).ok_or_else(|| p.bad("cmds", "the path has no extent"))?;
    let name = p.str("name")?.unwrap_or("Vector");
    let mut node = Node::new(
        ctx.doc.next_free_id(),
        name,
        NodeKind::Vector {
            path: local.clone(),
        },
        rect,
    );
    if is_closed(&local) {
        node.fills.push(Paint::solid(SHAPE_FILL));
    } else {
        node.strokes.push(Paint::solid(Rgba::BLACK));
        node.stroke.align = deesainer_doc::StrokeAlign::Center;
    }
    insert_top(ctx, parent, node, "Create vector")
}

fn set_path(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    let NodeKind::Vector { path: old } = &node.kind else {
        return Err(p.bad("id", "not a vector layer"));
    };
    let mut path = read_path(p, "cmds")?;
    path.fill_rule = old.fill_rule;
    let (b, local) = fit_path(&path).ok_or_else(|| p.bad("cmds", "the path has no extent"))?;
    let rect = Rect::new(node.rect.x + b.x, node.rect.y + b.y, b.width, b.height);
    Ok(Outcome::apply(
        Transaction::new(
            "Edit path",
            vec![
                Op::set(id, PropValue::Kind(NodeKind::Vector { path: local })),
                Op::set(id, PropValue::Rect(rect)),
                Op::set(id, PropValue::Network(None)),
            ],
        ),
        json!({}),
    ))
}

fn boolean_op(ctx: &Context<'_>, p: &Params<'_>, op: BoolOp) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    if ids.is_empty() {
        return Err(p.invalid("select layers to combine"));
    }
    let parent = {
        let first = ctx.doc.node(ids[0])?.parent;
        if ids
            .iter()
            .any(|id| ctx.doc.get(*id).and_then(|n| n.parent) != first)
        {
            return Err(p.invalid("select layers that share the same parent"));
        }
        first.ok_or_else(|| p.invalid("layers need a parent"))?
    };
    let mut z: Vec<(usize, NodeId)> = ids
        .iter()
        .map(|id| Ok((ctx.doc.index_in_parent(*id)?, *id)))
        .collect::<Result<_, EngineError>>()?;
    z.sort();
    let bounds = z
        .iter()
        .filter_map(|(_, id)| ctx.doc.get(*id).map(bounds_in_parent))
        .reduce(Rect::union)
        .ok_or_else(|| p.invalid("nothing to combine"))?;
    let gid = ctx.doc.next_free_id();
    let name = match op {
        BoolOp::Union => "Union",
        BoolOp::Subtract => "Subtract",
        BoolOp::Intersect => "Intersect",
        BoolOp::Exclude => "Exclude",
    };
    let mut group = Node::new(gid, name, NodeKind::BooleanOp { op }, bounds);
    if let Some(bottom) = z.first().and_then(|(_, id)| ctx.doc.get(*id)) {
        group.fills = bottom.fills.clone();
        group.strokes = bottom.strokes.clone();
        group.stroke = bottom.stroke.clone();
    }
    let top_index = z.last().map(|(i, _)| *i + 1).unwrap_or(0);
    let mut ops = vec![Op::CreateNode {
        parent,
        index: top_index,
        node: Box::new(group),
    }];
    for (k, (_, id)) in z.iter().enumerate() {
        let node = ctx.doc.node(*id)?;
        ops.push(Op::MoveNode {
            id: *id,
            parent: gid,
            index: k,
        });
        ops.push(Op::set(
            *id,
            PropValue::Rect(node.rect.translate(-bounds.x, -bounds.y)),
        ));
    }
    Ok(Outcome::apply(Transaction::new(name, ops), json!({ "id": gid })).selecting(vec![gid]))
}

fn outline_in_parent(doc: &Document, node: &Node) -> Option<VectorPath> {
    let local = match &node.kind {
        NodeKind::Text(_) => {
            let glyphs = crate::text_painter().outlines(node);
            let mut all = VectorPath::default();
            for g in glyphs {
                all.cmds.extend(g.cmds);
            }
            (!all.cmds.is_empty()).then_some(all)
        }
        NodeKind::Group | NodeKind::Frame | NodeKind::Component { .. }
            if !node.children.is_empty() && matches!(node.kind, NodeKind::Group) =>
        {
            let mut all = VectorPath::default();
            for c in &node.children {
                if let Some(child) = doc.get(*c) {
                    if let Some(path) = outline_in_parent(doc, child) {
                        all.cmds.extend(path.cmds);
                    }
                }
            }
            (!all.cmds.is_empty()).then_some(all)
        }
        _ => deesainer_vector::resolved_outline(doc, node),
    }?;
    let t = Document::local_transform(node);
    Some(local.map_points(|q| t.apply(q)))
}

fn flatten(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
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
    let mut z: Vec<(usize, NodeId)> = ids
        .iter()
        .map(|id| Ok((ctx.doc.index_in_parent(*id)?, *id)))
        .collect::<Result<_, EngineError>>()?;
    z.sort();
    let mut combined = VectorPath::default();
    let mut even_odd = false;
    for (_, id) in &z {
        let node = ctx.doc.node(*id)?;
        if let Some(path) = outline_in_parent(ctx.doc, node) {
            even_odd |= path.fill_rule == FillRule::EvenOdd;
            combined.cmds.extend(path.cmds);
        }
    }
    if even_odd {
        combined.fill_rule = FillRule::EvenOdd;
    }
    let (rect, local) =
        fit_path(&combined).ok_or_else(|| p.invalid("nothing to flatten (empty outlines)"))?;
    let (top_index, top_id) = z
        .last()
        .copied()
        .ok_or_else(|| p.invalid("nothing to flatten"))?;
    let top = ctx.doc.node(top_id)?;
    let new_id = ctx.doc.next_free_id();
    let mut node = Node::new(
        new_id,
        top.name.clone(),
        NodeKind::Vector { path: local },
        rect,
    );
    node.fills = top.fills.clone();
    node.strokes = top.strokes.clone();
    node.stroke = top.stroke.clone();
    node.effects = top.effects.clone();
    node.opacity = top.opacity;
    node.blend = top.blend;
    let mut ops = vec![Op::CreateNode {
        parent,
        index: top_index + 1,
        node: Box::new(node),
    }];
    ops.extend(super::edit::delete_ops(ctx, &ids)?);
    Ok(
        Outcome::apply(Transaction::new("Flatten", ops), json!({ "id": new_id }))
            .selecting(vec![new_id]),
    )
}

fn outline_stroke(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let mut ops = Vec::new();
    let mut created = Vec::new();
    let mut scratch = ctx.doc.clone();
    for id in ids {
        let node = scratch.node(id)?.clone();
        if node.strokes.iter().all(|s| !s.visible) || node.stroke.weight <= 0.0 {
            continue;
        }
        let Some(path) = deesainer_vector::resolved_outline(&scratch, &node) else {
            continue;
        };
        let width = match node.stroke.align {
            deesainer_doc::StrokeAlign::Center => node.stroke.weight,
            _ => node.stroke.weight * 2.0,
        };
        let Some(stroke_path) = deesainer_render::stroke_outline(&path, &node.stroke, width) else {
            continue;
        };
        let stroke_path = match node.stroke.align {
            deesainer_doc::StrokeAlign::Center => stroke_path,
            align => {
                let a = deesainer_vector::flatten(&stroke_path, 0.1);
                let b = deesainer_vector::flatten(&path, 0.1);
                let op = if align == deesainer_doc::StrokeAlign::Inside {
                    BoolOp::Intersect
                } else {
                    BoolOp::Subtract
                };
                deesainer_vector::boolean::apply(op, &[a, b])
            }
        };
        let t = Document::local_transform(&node);
        let in_parent = stroke_path.map_points(|q| t.apply(q));
        let Some((rect, local)) = fit_path(&in_parent) else {
            continue;
        };
        let parent = node
            .parent
            .ok_or_else(|| p.invalid("layer without parent"))?;
        let index = scratch.index_in_parent(id)? + 1;
        let new_id = scratch.next_free_id();
        let mut v = Node::new(
            new_id,
            format!("{} stroke", node.name),
            NodeKind::Vector { path: local },
            rect,
        );
        v.fills = node
            .strokes
            .iter()
            .filter(|s| !matches!(s.kind, PaintKind::Image { .. }))
            .cloned()
            .collect();
        let step = vec![
            Op::CreateNode {
                parent,
                index,
                node: Box::new(v),
            },
            Op::set(id, PropValue::Strokes(Vec::new())),
        ];
        Transaction::new("", step.clone()).apply(&mut scratch)?;
        ops.extend(step);
        created.push(new_id);
    }
    if ops.is_empty() {
        return Err(p.invalid("select layers with a visible stroke"));
    }
    Ok(Outcome::apply(
        Transaction::new("Outline stroke", ops),
        json!({ "ids": created }),
    )
    .selecting(created))
}

fn flip_ops(
    doc: &Document,
    id: NodeId,
    horizontal: bool,
    depth: usize,
    ops: &mut Vec<Op>,
) -> Result<(), EngineError> {
    if depth > deesainer_doc::MAX_DEPTH {
        return Ok(());
    }
    let node = doc.node(id)?;
    let (w, h) = (node.rect.width, node.rect.height);
    let mirror = |q: Point| {
        if horizontal {
            Point::new(w - q.x, q.y)
        } else {
            Point::new(q.x, h - q.y)
        }
    };
    let new_kind = match &node.kind {
        NodeKind::Vector { path } => Some(NodeKind::Vector {
            path: path.map_points(mirror),
        }),
        NodeKind::Polygon { .. } | NodeKind::Star { .. } => {
            deesainer_vector::outline(node).map(|path| NodeKind::Vector {
                path: path.map_points(mirror),
            })
        }
        NodeKind::Ellipse {
            arc_start,
            arc_end,
            inner_ratio,
        } if (arc_end - arc_start).abs() < 360.0 => {
            let (s, e) = if horizontal {
                (180.0 - arc_end, 180.0 - arc_start)
            } else {
                (-arc_end, -arc_start)
            };
            Some(NodeKind::Ellipse {
                arc_start: s,
                arc_end: e,
                inner_ratio: *inner_ratio,
            })
        }
        _ => None,
    };
    if let Some(k) = new_kind {
        ops.push(Op::set(id, PropValue::Kind(k)));
    }
    if node.rotation != 0.0 {
        ops.push(Op::set(id, PropValue::Rotation(-node.rotation)));
    }
    if node
        .fills
        .iter()
        .any(|f| !matches!(f.kind, PaintKind::Solid { .. }))
    {
        ops.push(Op::set(
            id,
            PropValue::Fills(deesainer_doc::mirror_paints(&node.fills, horizontal)),
        ));
    }
    if !matches!(node.kind, NodeKind::Instance { .. }) {
        for c in &node.children {
            let child = doc.node(*c)?;
            let r = child.rect;
            let moved = if horizontal {
                r.with_origin(w - r.x - r.width, r.y)
            } else {
                r.with_origin(r.x, h - r.y - r.height)
            };
            if moved != r {
                ops.push(Op::set(*c, PropValue::Rect(moved)));
            }
            flip_ops(doc, *c, horizontal, depth + 1, ops)?;
        }
    }
    Ok(())
}

fn flip(ctx: &Context<'_>, p: &Params<'_>, horizontal: bool) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    let mut ops = Vec::new();
    for id in ids {
        flip_ops(ctx.doc, id, horizontal, 0, &mut ops)?;
    }
    let label = if horizontal {
        "Flip horizontal"
    } else {
        "Flip vertical"
    };
    Ok(Outcome::apply(Transaction::new(label, ops), json!({})))
}

fn set_shape(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    let count = p.u32("count")?;
    if count.is_some_and(|c| !(3..=60).contains(&c)) {
        return Err(p.bad("count", "must be between 3 and 60"));
    }
    let ratio = p.f64("innerRatio")?;
    if ratio.is_some_and(|r| !(0.0..=1.0).contains(&r)) {
        return Err(p.bad("innerRatio", "must be between 0 and 1"));
    }
    let kind = match &node.kind {
        NodeKind::Polygon { sides } => NodeKind::Polygon {
            sides: count.unwrap_or(*sides),
        },
        NodeKind::Star {
            points,
            inner_ratio,
        } => NodeKind::Star {
            points: count.unwrap_or(*points),
            inner_ratio: ratio.unwrap_or(*inner_ratio),
        },
        NodeKind::Ellipse {
            arc_start,
            arc_end,
            inner_ratio,
        } => NodeKind::Ellipse {
            arc_start: p.f64("arcStart")?.unwrap_or(*arc_start),
            arc_end: p.f64("arcEnd")?.unwrap_or(*arc_end),
            inner_ratio: ratio.unwrap_or(*inner_ratio).min(0.999),
        },
        _ => return Err(p.bad("id", "not a polygon, star or ellipse")),
    };
    Ok(Outcome::apply(
        Transaction::new("Shape options", vec![Op::set(id, PropValue::Kind(kind))]),
        json!({}),
    ))
}
