use deesainer_doc::{ComponentProp, ComponentPropKind, Node, NodeId, NodeKind, PropValue};
use deesainer_geom::Rect;
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::arrange::bounds_in_parent;
use super::{clone_subtrees_mapped, collect_subtrees, layer_ids, param, top_level, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("component.addSlot", "Add slot", add_slot)
            .doc("Adds a slot to a main component: a frame (width by height, at x, y) whose children are the default content, bound to a slot property of the same name. Instances fill it with their own layers (instance.fillSlot); its auto layout and clipping set how their content is laid out. Returns {property, frame}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Main component."),
                    param("name", ParamType::String, true, "Slot name, unique among the component's properties."),
                    param("x", ParamType::Number, false, "Left edge in the component, default 0."),
                    param("y", ParamType::Number, false, "Top edge in the component, default 0."),
                    param("width", ParamType::Number, false, "Width, default 120."),
                    param("height", ParamType::Number, false, "Height, default 40."),
                ]
            }),
        CommandSpec::new("component.setSlotPreferred", "Preferred slot content", set_preferred)
            .doc("Sets the components a slot offers as quick content (an empty list offers every component).")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Main component."),
                    param("name", ParamType::String, true, "Slot name."),
                    param("components", ParamType::NodeIds, true, "Main components to offer."),
                ]
            }),
        CommandSpec::new("instance.fillSlot", "Fill slot", fill_slot)
            .doc("Puts layers into a slot of an instance: the given layers (default the selection, minus the instance) move into it, or are copied with copy: true; or, with component, a new instance of that component goes in. The content is the instance's own, editable like any layer, and replaces the slot's default content. Returns {content, layers}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Instance of the component that has the slot."),
                    param("name", ParamType::String, true, "Slot name."),
                    IDS,
                    param("copy", ParamType::Bool, false, "Copy the layers instead of moving them."),
                    param("component", ParamType::NodeId, false, "Insert an instance of this main component instead."),
                ]
            }),
        CommandSpec::new("instance.clearSlot", "Clear slot", clear_slot)
            .doc("Removes an instance's own content from a slot; the slot shows its default content again.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Instance."),
                    param("name", ParamType::String, true, "Slot name."),
                ]
            }),
    ]
}

fn slot_name(p: &Params<'_>) -> Result<String, EngineError> {
    p.str("name")?
        .map(str::trim)
        .filter(|n| !n.is_empty() && n.chars().count() <= 80)
        .map(str::to_owned)
        .ok_or_else(|| p.bad("name", "needs 1 to 80 characters"))
}

fn add_slot(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let Some(NodeKind::Component {
        description,
        properties,
    }) = ctx.doc.get(id).map(|n| &n.kind)
    else {
        return Err(p.bad("id", "not a main component"));
    };
    let name = slot_name(p)?;
    if properties.iter().any(|x| x.name == name) {
        return Err(p.bad("name", "already used by another property"));
    }
    if properties.len() >= ComponentProp::MAX {
        return Err(p.bad("name", "a component takes at most 32 properties"));
    }
    let x = p.f64("x")?.unwrap_or(0.0);
    let y = p.f64("y")?.unwrap_or(0.0);
    let width = p.non_negative("width")?.unwrap_or(120.0).max(1.0);
    let height = p.non_negative("height")?.unwrap_or(40.0).max(1.0);
    let main = ctx.doc.node(id)?;
    let frame_id = ctx.doc.fresh_ids(1)[0];
    let mut frame = Node::new(
        frame_id,
        name.clone(),
        NodeKind::Frame,
        Rect::new(x, y, width, height),
    );
    frame.fills = Vec::new();
    frame.clips_content = true;
    let mut list = properties.clone();
    list.push(ComponentProp {
        name: name.clone(),
        target: frame_id,
        kind: ComponentPropKind::Slot {
            preferred: Vec::new(),
        },
    });
    let ops = vec![
        Op::CreateNode {
            parent: id,
            index: main.children.len(),
            node: Box::new(frame),
        },
        Op::set(
            id,
            PropValue::Kind(NodeKind::Component {
                description: description.clone(),
                properties: list,
            }),
        ),
    ];
    Ok(Outcome::apply(
        Transaction::new("Add slot", ops),
        json!({ "property": name, "frame": frame_id }),
    ))
}

fn set_preferred(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let Some(NodeKind::Component {
        description,
        properties,
    }) = ctx.doc.get(id).map(|n| &n.kind)
    else {
        return Err(p.bad("id", "not a main component"));
    };
    let name = slot_name(p)?;
    let components = p.req_node_ids("components")?;
    for c in &components {
        if !matches!(
            ctx.doc.get(*c).map(|n| &n.kind),
            Some(NodeKind::Component { .. })
        ) {
            return Err(p.bad("components", format!("{c} is not a main component")));
        }
        if *c == id {
            return Err(p.bad("components", "a slot cannot offer its own component"));
        }
    }
    let mut list = properties.clone();
    let Some(prop) = list
        .iter_mut()
        .find(|x| x.name == name && matches!(x.kind, ComponentPropKind::Slot { .. }))
    else {
        return Err(p.bad("name", "no slot with this name on the component"));
    };
    prop.kind = ComponentPropKind::Slot {
        preferred: components,
    };
    let op = Op::set(
        id,
        PropValue::Kind(NodeKind::Component {
            description: description.clone(),
            properties: list,
        }),
    );
    Ok(Outcome::apply(
        Transaction::new("Preferred slot content", vec![op]),
        json!({}),
    ))
}

fn content_of(ctx: &Context<'_>, instance: NodeId, name: &str) -> Option<NodeId> {
    ctx.doc.get(instance)?.children.iter().copied().find(|c| {
        matches!(
            ctx.doc.get(*c).map(|n| &n.kind),
            Some(NodeKind::SlotContent { slot }) if slot == name
        )
    })
}

fn fill_slot(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let instance = ctx.doc.node(id)?;
    let NodeKind::Instance {
        component: main, ..
    } = &instance.kind
    else {
        return Err(p.bad("id", "not an instance"));
    };
    let name = slot_name(p)?;
    let target = ctx
        .doc
        .slot_target(id, &name)
        .ok_or_else(|| p.bad("name", "no slot with this name on the main component"))?;
    let existing = content_of(ctx, id, &name);
    let at = existing
        .and_then(|c| ctx.doc.get(c))
        .map_or(0, |n| n.children.len());
    let insert = p.node_id("component")?;
    let layers = if insert.is_some() {
        Vec::new()
    } else {
        let ids: Vec<NodeId> = layer_ids(ctx, p)?
            .into_iter()
            .filter(|l| *l != id)
            .collect();
        top_level(ctx.doc, &ids)
    };
    if insert.is_none() && layers.is_empty() {
        return Err(p.invalid("select the layers that go into the slot, or pass component"));
    }
    if layers
        .iter()
        .any(|l| ctx.doc.is_ancestor(*l, id).unwrap_or(false))
    {
        return Err(p.invalid("a slot cannot hold the instance it belongs to"));
    }
    let copy = p.bool("copy")?.unwrap_or(false);
    let nodes = if copy {
        collect_subtrees(ctx.doc, &layers)?
    } else {
        Default::default()
    };
    let cloned = nodes.len();
    let needed = cloned + usize::from(existing.is_none()) + usize::from(insert.is_some());
    let fresh = ctx.doc.fresh_ids(needed.max(1));
    let mut ours = fresh.iter().copied().skip(cloned);
    let mut ops = Vec::new();
    let content = match existing {
        Some(c) => c,
        None => {
            let cid = ours.next().ok_or_else(|| p.invalid("no free id"))?;
            let (rect, rotation, slot) = crate::slots::slot_box(ctx.doc, instance, target)
                .ok_or_else(|| p.invalid("the slot is not inside the instance's main component"))?;
            let mut node = Node::new(
                cid,
                name.clone(),
                NodeKind::SlotContent { slot: name.clone() },
                rect,
            );
            node.rotation = rotation;
            node.fills = Vec::new();
            node.layout = crate::slots::content_layout(&slot);
            node.clips_content = slot.clips_content;
            ops.push(Op::CreateNode {
                parent: id,
                index: instance.children.len(),
                node: Box::new(node),
            });
            cid
        }
    };
    let mut placed: Vec<NodeId> = Vec::new();
    if let Some(comp) = insert {
        let main_node = ctx.doc.node(comp)?;
        if !matches!(main_node.kind, NodeKind::Component { .. }) {
            return Err(p.bad("component", "not a main component"));
        }
        if comp == *main || ctx.doc.is_ancestor(comp, *main).unwrap_or(false) {
            return Err(p.bad("component", "a slot cannot hold its own component"));
        }
        let iid = ours.next().ok_or_else(|| p.invalid("no free id"))?;
        let node = Node::new(
            iid,
            main_node.name.clone(),
            NodeKind::Instance {
                component: comp,
                overrides: Vec::new(),
            },
            Rect::new(0.0, 0.0, main_node.rect.width, main_node.rect.height),
        );
        ops.push(Op::CreateNode {
            parent: content,
            index: at,
            node: Box::new(node),
        });
        placed.push(iid);
    } else {
        let parent = ctx.doc.node(layers[0])?.parent;
        if layers
            .iter()
            .any(|l| ctx.doc.get(*l).and_then(|n| n.parent) != parent)
        {
            return Err(p.invalid("select layers that share the same parent"));
        }
        let bounds = layers
            .iter()
            .filter_map(|l| ctx.doc.get(*l).map(bounds_in_parent))
            .reduce(Rect::union)
            .ok_or_else(|| p.invalid("nothing to put in the slot"))?;
        if copy {
            let (clones, roots, _) = clone_subtrees_mapped(
                ctx.doc,
                &nodes,
                &layers,
                content,
                at,
                (-bounds.x, -bounds.y),
            )?;
            ops.extend(clones);
            placed.extend(roots);
        } else {
            for (i, l) in layers.iter().enumerate() {
                let node = ctx.doc.node(*l)?;
                ops.push(Op::MoveNode {
                    id: *l,
                    parent: content,
                    index: at + i,
                });
                let mut rect = node.rect;
                rect.x -= bounds.x;
                rect.y -= bounds.y;
                if rect != node.rect {
                    ops.push(Op::set(*l, PropValue::Rect(rect)));
                }
                placed.push(*l);
            }
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Fill slot", ops),
        json!({ "content": content, "layers": placed }),
    )
    .selecting(placed))
}

fn clear_slot(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    if !matches!(
        ctx.doc.get(id).map(|n| &n.kind),
        Some(NodeKind::Instance { .. })
    ) {
        return Err(p.bad("id", "not an instance"));
    }
    let name = slot_name(p)?;
    let content = content_of(ctx, id, &name)
        .ok_or_else(|| p.bad("name", "the instance has no content in this slot"))?;
    let ops = super::edit::delete_ops(ctx, &[content])?;
    Ok(Outcome::apply(Transaction::new("Clear slot", ops), json!({})).selecting(vec![id]))
}
