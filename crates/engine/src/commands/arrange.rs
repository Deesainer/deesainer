use deesainer_doc::{Document, Node, NodeId, NodeKind, PropValue};
use deesainer_geom::{Affine, Point, Rect};
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::{layer_ids, param, top_level, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType, RunFn};

pub fn specs() -> Vec<CommandSpec> {
    let ids_only = const { &[IDS] };
    let simple = |id: &'static str,
                  label: &'static str,
                  menu: &'static str,
                  key: &'static str,
                  doc: &'static str,
                  run: RunFn| {
        CommandSpec::new(id, label, run)
            .menu(menu)
            .key(key)
            .doc(doc)
            .params(ids_only)
    };
    vec![
        simple("object.group", "Group selection", "Object/Group selection", "Cmd+G", "Groups sibling layers. The group's box always fits its children. Returns {id}.", group),
        simple("object.ungroup", "Ungroup", "Object/Ungroup", "Shift+Cmd+G", "Moves the children of groups (or boolean groups) into the group's parent and removes the groups. Returns {ids} of the freed children.", ungroup),
        simple("object.frameSelection", "Frame selection", "Object/Frame selection", "Alt+Cmd+G", "Wraps sibling layers in a new frame sized to their bounds. Returns {id}.", frame_selection),
        simple("object.bringToFront", "Bring to front", "Object/Bring to front", "Alt+Cmd+]", "Moves layers to the top of their parent.", |c, p| reorder(c, p, Reorder::Front)),
        simple("object.bringForward", "Bring forward", "Object/Bring forward", "Cmd+]", "Moves layers one step up.", |c, p| reorder(c, p, Reorder::Forward)),
        simple("object.sendBackward", "Send backward", "Object/Send backward", "Cmd+[", "Moves layers one step down.", |c, p| reorder(c, p, Reorder::Backward)),
        simple("object.sendToBack", "Send to back", "Object/Send to back", "Alt+Cmd+[", "Moves layers to the bottom of their parent.", |c, p| reorder(c, p, Reorder::Back)),
        simple("object.toggleVisible", "Show/Hide selection", "Object/Show/Hide selection", "Shift+Cmd+H", "Toggles visibility (all become hidden unless all were hidden).", toggle_visible),
        simple("object.toggleLock", "Lock/Unlock selection", "Object/Lock/Unlock selection", "Shift+Cmd+L", "Toggles locking (locked layers cannot be selected on the canvas).", toggle_lock),
        simple("align.left", "Align left", "Object/Align/Left", "Alt+A", "Aligns left edges to the selection bounds (or a single layer to its parent frame).", |c, p| align(c, p, Align::Left)),
        simple("align.horizontalCenter", "Align horizontal centers", "Object/Align/Horizontal centers", "Alt+H", "Aligns horizontal centres.", |c, p| align(c, p, Align::HCenter)),
        simple("align.right", "Align right", "Object/Align/Right", "Alt+D", "Aligns right edges.", |c, p| align(c, p, Align::Right)),
        simple("align.top", "Align top", "Object/Align/Top", "Alt+W", "Aligns top edges.", |c, p| align(c, p, Align::Top)),
        simple("align.verticalCenter", "Align vertical centers", "Object/Align/Vertical centers", "Alt+V", "Aligns vertical centres.", |c, p| align(c, p, Align::VCenter)),
        simple("align.bottom", "Align bottom", "Object/Align/Bottom", "Alt+S", "Aligns bottom edges.", |c, p| align(c, p, Align::Bottom)),
        simple("distribute.horizontal", "Distribute horizontal spacing", "Object/Align/Distribute horizontal spacing", "Ctrl+Alt+H", "Equalizes the horizontal gaps between 3 or more layers.", |c, p| distribute(c, p, true)),
        simple("distribute.vertical", "Distribute vertical spacing", "Object/Align/Distribute vertical spacing", "Ctrl+Alt+V", "Equalizes the vertical gaps between 3 or more layers.", |c, p| distribute(c, p, false)),
        CommandSpec::new("node.reparent", "Move to", reparent)
            .doc("Moves a layer into another container at an index (0 = bottom), keeping its position on the page. Returns {}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer to move."),
                    param("parent", ParamType::NodeId, true, "New container (page, frame, group, component)."),
                    param("index", ParamType::Integer, false, "Z-index in the new parent; default top."),
                ]
            }),
    ]
}

fn common_parent(ctx: &Context<'_>, p: &Params<'_>, ids: &[NodeId]) -> Result<NodeId, EngineError> {
    let mut parent = None;
    for id in ids {
        let pid = ctx.doc.node(*id)?.parent;
        match (parent, pid) {
            (None, Some(x)) => parent = Some(x),
            (Some(a), Some(b)) if a == b => {}
            _ => return Err(p.invalid("select layers that share the same parent")),
        }
    }
    parent.ok_or_else(|| p.invalid("nothing to arrange"))
}

pub(crate) fn bounds_in_parent(node: &Node) -> Rect {
    let local = Rect::new(0.0, 0.0, node.rect.width, node.rect.height);
    Document::local_transform(node).transform_rect_bounds(&local)
}

fn sorted_by_z(ctx: &Context<'_>, ids: &[NodeId]) -> Result<Vec<(usize, NodeId)>, EngineError> {
    let mut v: Vec<(usize, NodeId)> = ids
        .iter()
        .map(|id| Ok((ctx.doc.index_in_parent(*id)?, *id)))
        .collect::<Result<_, EngineError>>()?;
    v.sort();
    Ok(v)
}

fn wrap(
    ctx: &Context<'_>,
    p: &Params<'_>,
    kind: NodeKind,
    label: &str,
) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    let parent = common_parent(ctx, p, &ids)?;
    let z = sorted_by_z(ctx, &ids)?;
    let mut bounds: Option<Rect> = None;
    for (_, id) in &z {
        let b = bounds_in_parent(ctx.doc.node(*id)?);
        bounds = Some(bounds.map_or(b, |acc| acc.union(b)));
    }
    let bounds = bounds.ok_or_else(|| p.invalid("nothing to group"))?;
    let gid = ctx.doc.next_free_id();
    let name = match kind {
        NodeKind::Frame => "Frame",
        _ => "Group",
    };
    let mut group = Node::new(gid, name, kind, bounds);
    group.clips_content = false;
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
    Ok(Outcome::apply(Transaction::new(label, ops), json!({ "id": gid })).selecting(vec![gid]))
}

fn group(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    wrap(ctx, p, NodeKind::Group, "Group")
}

fn frame_selection(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    wrap(ctx, p, NodeKind::Frame, "Frame selection")
}

fn rehome_rect(child: &Node, to_new_parent: Affine, rotation_add: f64) -> (Rect, f64) {
    let center_local = Point::new(child.rect.width / 2.0, child.rect.height / 2.0);
    let center_in_container = Document::local_transform(child).apply(center_local);
    let center = to_new_parent.apply(center_in_container);
    (
        Rect::new(
            center.x - child.rect.width / 2.0,
            center.y - child.rect.height / 2.0,
            child.rect.width,
            child.rect.height,
        ),
        normalize_degrees(child.rotation + rotation_add),
    )
}

pub(crate) fn normalize_degrees(d: f64) -> f64 {
    let r = d.rem_euclid(360.0);
    if r > 180.0 {
        r - 360.0
    } else {
        r
    }
}

fn ungroup(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let mut ops = Vec::new();
    let mut freed = Vec::new();
    let mut scratch = ctx.doc.clone();
    for gid in ids {
        let g = scratch.node(gid)?.clone();
        if !matches!(g.kind, NodeKind::Group | NodeKind::BooleanOp { .. }) {
            continue;
        }
        let parent = g.parent.ok_or_else(|| p.invalid("group without parent"))?;
        let first_index = scratch.index_in_parent(gid)?;
        let to_parent = Document::local_transform(&g);
        let mut step = Vec::new();
        for (index, c) in (first_index..).zip(g.children.iter()) {
            let child = scratch.node(*c)?;
            let (rect, rotation) = rehome_rect(child, to_parent, g.rotation);
            step.push(Op::MoveNode {
                id: *c,
                parent,
                index,
            });
            step.push(Op::set(*c, PropValue::Rect(rect)));
            if rotation != child.rotation {
                step.push(Op::set(*c, PropValue::Rotation(rotation)));
            }
            freed.push(*c);
        }
        step.push(Op::DeleteNode { id: gid });
        Transaction::new("", step.clone()).apply(&mut scratch)?;
        ops.extend(step);
    }
    if ops.is_empty() {
        return Err(p.invalid("select one or more groups"));
    }
    Ok(Outcome::apply(Transaction::new("Ungroup", ops), json!({ "ids": freed })).selecting(freed))
}

#[derive(Clone, Copy)]
enum Reorder {
    Front,
    Forward,
    Backward,
    Back,
}

fn reorder(ctx: &Context<'_>, p: &Params<'_>, how: Reorder) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let parent = common_parent(ctx, p, &ids)?;
    let current = ctx.doc.node(parent)?.children.clone();
    let mut order = current.clone();
    let selected = |id: &NodeId| ids.contains(id);
    match how {
        Reorder::Front => {
            order.retain(|id| !selected(id));
            order.extend(current.iter().copied().filter(|id| selected(id)));
        }
        Reorder::Back => {
            let mut front: Vec<NodeId> =
                current.iter().copied().filter(|id| selected(id)).collect();
            front.extend(current.iter().copied().filter(|id| !selected(id)));
            order = front;
        }
        Reorder::Forward => {
            for i in (0..order.len().saturating_sub(1)).rev() {
                if selected(&order[i]) && !selected(&order[i + 1]) {
                    order.swap(i, i + 1);
                }
            }
        }
        Reorder::Backward => {
            for i in 1..order.len() {
                if selected(&order[i]) && !selected(&order[i - 1]) {
                    order.swap(i, i - 1);
                }
            }
        }
    }
    let mut sim = current;
    let mut ops = Vec::new();
    for (i, target) in order.iter().enumerate() {
        if sim.get(i) != Some(target) {
            if let Some(pos) = sim.iter().position(|x| x == target) {
                let id = sim.remove(pos);
                sim.insert(i, id);
                ops.push(Op::MoveNode {
                    id: *target,
                    parent,
                    index: i,
                });
            }
        }
    }
    Ok(Outcome::apply(Transaction::new("Reorder", ops), json!({})))
}

fn toggle_visible(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let all_hidden = ids
        .iter()
        .all(|id| ctx.doc.get(*id).is_some_and(|n| !n.visible));
    let ops = ids
        .iter()
        .map(|id| Op::set(*id, PropValue::Visible(all_hidden)))
        .collect();
    Ok(Outcome::apply(
        Transaction::new("Show/Hide", ops),
        json!({ "visible": all_hidden }),
    ))
}

fn toggle_lock(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let all_locked = ids
        .iter()
        .all(|id| ctx.doc.get(*id).is_some_and(|n| n.locked));
    let ops = ids
        .iter()
        .map(|id| Op::set(*id, PropValue::Locked(!all_locked)))
        .collect();
    Ok(Outcome::apply(
        Transaction::new("Lock/Unlock", ops),
        json!({ "locked": !all_locked }),
    ))
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

fn align(ctx: &Context<'_>, p: &Params<'_>, how: Align) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    let world: Vec<(NodeId, Rect)> = ids
        .iter()
        .filter_map(|id| ctx.doc.world_bounds(*id).map(|b| (*id, b)))
        .collect();
    let target = if world.len() == 1 {
        let id = world[0].0;
        let parent = ctx.doc.node(id)?.parent;
        match parent.and_then(|pid| ctx.doc.get(pid)) {
            Some(pn) if !matches!(pn.kind, NodeKind::Page | NodeKind::Document) => {
                ctx.doc.world_bounds(pn.id).unwrap_or(world[0].1)
            }
            _ => return Ok(Outcome::apply(Transaction::new("Align", vec![]), json!({}))),
        }
    } else {
        world
            .iter()
            .map(|(_, b)| *b)
            .reduce(Rect::union)
            .ok_or_else(|| p.invalid("nothing to align"))?
    };
    let mut ops = Vec::new();
    for (id, b) in &world {
        let (dx, dy) = match how {
            Align::Left => (target.x - b.x, 0.0),
            Align::HCenter => (target.center().x - b.center().x, 0.0),
            Align::Right => (target.max_x() - b.max_x(), 0.0),
            Align::Top => (0.0, target.y - b.y),
            Align::VCenter => (0.0, target.center().y - b.center().y),
            Align::Bottom => (0.0, target.max_y() - b.max_y()),
        };
        if dx.abs() > 1e-9 || dy.abs() > 1e-9 {
            let node = ctx.doc.node(*id)?;
            let (ldx, ldy) = world_delta_to_parent(ctx.doc, node, dx, dy);
            ops.push(Op::set(*id, PropValue::Rect(node.rect.translate(ldx, ldy))));
        }
    }
    Ok(Outcome::apply(Transaction::new("Align", ops), json!({})))
}

pub(crate) fn world_delta_to_parent(doc: &Document, node: &Node, dx: f64, dy: f64) -> (f64, f64) {
    let Some(parent) = node.parent else {
        return (dx, dy);
    };
    match doc.world_transform(parent).invert() {
        Some(inv) => {
            let a = inv.apply(Point::new(0.0, 0.0));
            let b = inv.apply(Point::new(dx, dy));
            (b.x - a.x, b.y - a.y)
        }
        None => (dx, dy),
    }
}

fn distribute(ctx: &Context<'_>, p: &Params<'_>, horizontal: bool) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    let mut items: Vec<(NodeId, Rect)> = ids
        .iter()
        .filter_map(|id| ctx.doc.world_bounds(*id).map(|b| (*id, b)))
        .collect();
    if items.len() < 3 {
        return Err(p.invalid("select at least 3 layers to distribute"));
    }
    let key = |r: &Rect| if horizontal { r.x } else { r.y };
    let size = |r: &Rect| if horizontal { r.width } else { r.height };
    items.sort_by(|a, b| key(&a.1).total_cmp(&key(&b.1)));
    let first = items[0].1;
    let last = items[items.len() - 1].1;
    let span = key(&last) + size(&last) - key(&first);
    let total: f64 = items.iter().map(|(_, r)| size(r)).sum();
    let gap = (span - total) / (items.len() - 1) as f64;
    let mut cursor = key(&first);
    let mut ops = Vec::new();
    for (id, r) in &items {
        let delta = cursor - key(r);
        if delta.abs() > 1e-9 {
            let node = ctx.doc.node(*id)?;
            let (dx, dy) = if horizontal {
                (delta, 0.0)
            } else {
                (0.0, delta)
            };
            let (ldx, ldy) = world_delta_to_parent(ctx.doc, node, dx, dy);
            ops.push(Op::set(*id, PropValue::Rect(node.rect.translate(ldx, ldy))));
        }
        cursor += size(r) + gap;
    }
    Ok(Outcome::apply(
        Transaction::new("Distribute", ops),
        json!({}),
    ))
}

fn reparent(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let parent = p.req_node_id("parent")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    if matches!(node.kind, NodeKind::Page | NodeKind::Document) {
        return Err(p.bad("id", "pages cannot be moved this way"));
    }
    let pnode = ctx
        .doc
        .get(parent)
        .ok_or_else(|| p.bad("parent", format!("{parent} does not exist")))?;
    if !pnode.kind.is_container() || matches!(pnode.kind, NodeKind::Document) {
        return Err(p.bad("parent", "must be a page or container"));
    }
    if id == parent || ctx.doc.is_ancestor(id, parent)? {
        return Err(p.bad("parent", "cannot move a layer into itself"));
    }
    let same = node.parent == Some(parent);
    let max = if same {
        pnode.children.len().saturating_sub(1)
    } else {
        pnode.children.len()
    };
    let index = match p.u32("index")? {
        Some(i) => (i as usize).min(max),
        None => max,
    };
    let old_parent = node.parent.unwrap_or(ctx.current_page);
    let to_world = ctx.doc.world_transform(old_parent);
    let from_world = ctx
        .doc
        .world_transform(parent)
        .invert()
        .unwrap_or(Affine::IDENTITY);
    let old_rot = world_rotation(ctx.doc, old_parent);
    let new_rot = world_rotation(ctx.doc, parent);
    let (rect, rotation) = rehome_rect(node, from_world.then(to_world), old_rot - new_rot);
    let mut ops = vec![Op::MoveNode { id, parent, index }];
    if rect != node.rect {
        ops.push(Op::set(id, PropValue::Rect(rect)));
    }
    if (rotation - node.rotation).abs() > 1e-9 {
        ops.push(Op::set(id, PropValue::Rotation(rotation)));
    }
    Ok(Outcome::apply(Transaction::new("Move to", ops), json!({})))
}

fn world_rotation(doc: &Document, id: NodeId) -> f64 {
    let mut r = doc.get(id).map(|n| n.rotation).unwrap_or(0.0);
    for a in doc.ancestors(id) {
        r += doc.get(a).map(|n| n.rotation).unwrap_or(0.0);
    }
    r
}
