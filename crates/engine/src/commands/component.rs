use std::collections::BTreeMap;

use deesainer_doc::{Node, NodeId, NodeKind, Override, Paint, PropValue};
use deesainer_geom::Rect;
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::arrange::bounds_in_parent;
use super::{layer_ids, param, target_parent, top_level, IDS};
use crate::components::{materialize, remap_overrides, variant_name, variant_props};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("component.create", "Create component", create)
            .menu("Object/Create component")
            .key("Alt+Cmd+K")
            .params(const { &[IDS] })
            .doc("Turns a frame into a main component, or wraps the selection (shapes, groups, several layers) in a new component. Returns {id}."),
        CommandSpec::new("component.instantiate", "Create instance", instantiate)
            .doc("Places an instance of a main component. It follows the main's changes; edits to it become overrides. Returns {id}.")
            .params(const {
                &[
                    param("component", ParamType::NodeId, true, "Main component id."),
                    param("parent", ParamType::NodeId, false, "Container; defaults to the current page."),
                    param("x", ParamType::Number, false, "Left edge; default right of the main component."),
                    param("y", ParamType::Number, false, "Top edge."),
                ]
            }),
        CommandSpec::new("component.detach", "Detach instance", detach)
            .menu("Object/Detach instance")
            .key("Alt+Cmd+B")
            .params(const { &[IDS] })
            .doc("Replaces instances with plain frames holding copies of their content (overrides applied). Returns {ids}."),
        CommandSpec::new("component.goToMain", "Go to main component", go_to_main)
            .menu("Object/Go to main component")
            .params(const { &[param("id", ParamType::NodeId, false, "Instance; defaults to the selection.")] })
            .doc("Selects an instance's main component (switching page if needed). Returns {id}."),
        CommandSpec::new("component.resetOverrides", "Reset all changes", reset_overrides)
            .menu("Object/Reset all changes")
            .params(const { &[IDS] })
            .doc("Removes every override from instances."),
        CommandSpec::new("instance.setOverride", "Override", set_override)
            .doc("Overrides one property of a layer inside an instance: text (string), fill (hex colour), visible (bool) or opacity (0..1). target is the layer's id in the main component (see component.list or node.inspect on the main).")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Instance."),
                    param("target", ParamType::NodeId, true, "Layer inside the main component."),
                    param("prop", ParamType::Enum(&["text", "fill", "visible", "opacity"]), true, "What to override."),
                    param("value", ParamType::Json, true, "New value."),
                ]
            }),
        CommandSpec::new("component.addProperty", "Add component property", add_property)
            .doc("Adds a property to a main component. kind boolean shows/hides the target layer, text edits a text layer's content, instanceSwap swaps the main component of a nested instance. The default comes from the target's current state. Instances then set it with instance.setProperty.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Main component."),
                    param("name", ParamType::String, true, "Property name, unique within the component."),
                    param("kind", ParamType::Enum(&["boolean", "text", "instanceSwap"]), true, "What the property drives."),
                    param("target", ParamType::NodeId, true, "Layer inside the component (a text layer for text, a nested instance for instanceSwap)."),
                ]
            }),
        CommandSpec::new("component.removeProperty", "Remove component property", remove_property)
            .doc("Removes a property from a main component. Instances keep their overrides on the layers.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Main component."),
                    param("name", ParamType::String, true, "Property name."),
                ]
            }),
        CommandSpec::new("instance.setProperty", "Set component property", set_property)
            .doc("Sets a component property on an instance: a bool for boolean, a string for text, a main component id for instanceSwap. Setting the default clears the override.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Instance."),
                    param("name", ParamType::String, true, "Property name from the main component."),
                    param("value", ParamType::Json, true, "bool, string or component id."),
                ]
            }),
        CommandSpec::new("component.combineAsVariants", "Combine as variants", combine)
            .params(const { &[IDS] })
            .doc("Groups sibling main components into a component set. Names become variant properties (\"Size=Large, State=Hover\"); unnamed ones get Property 1=VariantN. Returns {id}."),
        CommandSpec::new("instance.setVariant", "Set variant", set_variant)
            .doc("Switches instances to the variant matching the given properties (merged with their current ones). Overrides carry over by layer name.")
            .params(const {
                &[
                    IDS,
                    param("properties", ParamType::Json, true, "Object of property -> value, e.g. {\"State\": \"Hover\"}."),
                ]
            }),
        CommandSpec::new("instance.swap", "Swap instance", swap)
            .doc("Points instances at another main component. Overrides carry over by layer name.")
            .params(const {
                &[
                    IDS,
                    param("component", ParamType::NodeId, true, "New main component."),
                ]
            }),
        CommandSpec::new("component.list", "List components", list)
            .read_only()
            .doc("Lists main components and component sets: [{id, name, page, set, variant, instances, layers:[{id, name, type}]}]."),
    ]
}

fn is_main(n: &Node) -> bool {
    matches!(n.kind, NodeKind::Component { .. })
}

fn create(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    if let [one] = ids.as_slice() {
        let node = ctx.doc.node(*one)?;
        if matches!(node.kind, NodeKind::Frame) {
            let mut ops = vec![Op::set(
                *one,
                PropValue::Kind(NodeKind::Component {
                    description: String::new(),
                    properties: Vec::new(),
                }),
            )];
            if node.children.is_empty() && node.fills.is_empty() {
                ops.push(Op::set(
                    *one,
                    PropValue::Fills(vec![Paint::solid(deesainer_color::Rgba::WHITE)]),
                ));
            }
            return Ok(Outcome::apply(
                Transaction::new("Create component", ops),
                json!({ "id": one }),
            )
            .selecting(vec![*one]));
        }
        if is_main(node) {
            return Err(p.invalid("already a main component"));
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
    let mut z: Vec<(usize, NodeId)> = ids
        .iter()
        .map(|id| Ok((ctx.doc.index_in_parent(*id)?, *id)))
        .collect::<Result<_, EngineError>>()?;
    z.sort();
    let bounds = z
        .iter()
        .filter_map(|(_, id)| ctx.doc.get(*id).map(bounds_in_parent))
        .reduce(Rect::union)
        .ok_or_else(|| p.invalid("nothing to wrap"))?;
    let cid = ctx.doc.next_free_id();
    let name = if z.len() == 1 {
        ctx.doc.node(z[0].1)?.name.clone()
    } else {
        "Component".to_owned()
    };
    let mut comp = Node::new(
        cid,
        name,
        NodeKind::Component {
            description: String::new(),
            properties: Vec::new(),
        },
        bounds,
    );
    comp.clips_content = false;
    let mut ops = vec![Op::CreateNode {
        parent,
        index: z.last().map(|(i, _)| *i + 1).unwrap_or(0),
        node: Box::new(comp),
    }];
    for (k, (_, id)) in z.iter().enumerate() {
        let n = ctx.doc.node(*id)?;
        ops.push(Op::MoveNode {
            id: *id,
            parent: cid,
            index: k,
        });
        ops.push(Op::set(
            *id,
            PropValue::Rect(n.rect.translate(-bounds.x, -bounds.y)),
        ));
    }
    Ok(Outcome::apply(
        Transaction::new("Create component", ops),
        json!({ "id": cid }),
    )
    .selecting(vec![cid]))
}

fn main_component(ctx: &Context<'_>, p: &Params<'_>, key: &str) -> Result<NodeId, EngineError> {
    let id = p.req_node_id(key)?;
    match ctx.doc.get(id) {
        Some(n) if is_main(n) => Ok(id),
        Some(_) => Err(p.bad(key, format!("{id} is not a main component"))),
        None => Err(p.bad(key, format!("{id} does not exist"))),
    }
}

fn instantiate(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let cid = main_component(ctx, p, "component")?;
    let main = ctx.doc.node(cid)?;
    let parent = target_parent(ctx, p)?;
    let default_pos = ctx
        .doc
        .world_bounds(cid)
        .map_or((0.0, 0.0), |b| (b.max_x() + 40.0, b.y));
    let rect = Rect::new(
        p.f64("x")?.unwrap_or(default_pos.0),
        p.f64("y")?.unwrap_or(default_pos.1),
        main.rect.width,
        main.rect.height,
    );
    let id = ctx.doc.next_free_id();
    let mut inst = main.clone();
    inst.id = id;
    inst.parent = None;
    inst.children = Vec::new();
    inst.rect = rect;
    inst.rotation = 0.0;
    inst.layout_child = Default::default();
    inst.kind = NodeKind::Instance {
        component: cid,
        overrides: Vec::new(),
    };
    super::create::insert_top(ctx, parent, inst, "Create instance")
}

fn instances_of(ctx: &Context<'_>, p: &Params<'_>) -> Result<Vec<NodeId>, EngineError> {
    let ids: Vec<NodeId> = layer_ids(ctx, p)?
        .into_iter()
        .filter(|id| {
            matches!(
                ctx.doc.get(*id).map(|n| &n.kind),
                Some(NodeKind::Instance { .. })
            )
        })
        .collect();
    if ids.is_empty() {
        return Err(p.invalid("select one or more instances"));
    }
    Ok(ids)
}

fn detach(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = instances_of(ctx, p)?;
    let mut scratch = ctx.doc.clone();
    let mut ops = Vec::new();
    let mut new_ids = Vec::new();
    for id in ids {
        let inst = scratch.node(id)?.clone();
        let NodeKind::Instance { component, .. } = inst.kind else {
            continue;
        };
        let mut nodes = materialize(&scratch, &inst)
            .ok_or_else(|| p.invalid("the main component is missing"))?;
        let fills = scratch.slot_fills(id);
        for target in fills.keys() {
            if let Some(n) = nodes.get_mut(target) {
                n.children.clear();
                n.kind = match &n.kind {
                    NodeKind::Instance { .. } => NodeKind::Frame,
                    other => other.clone(),
                };
            }
        }
        let parent = inst
            .parent
            .ok_or_else(|| p.invalid("instance without parent"))?;
        let index = scratch.index_in_parent(id)?;
        let (mut step, roots, map) = super::clone_subtrees_mapped(
            &scratch,
            &nodes,
            &[component],
            parent,
            index + 1,
            (0.0, 0.0),
        )?;
        for (target, content) in &fills {
            let (Some(new_target), Some(holder)) = (map.get(target), scratch.get(*content)) else {
                continue;
            };
            for (i, child) in holder.children.iter().enumerate() {
                step.push(Op::MoveNode {
                    id: *child,
                    parent: *new_target,
                    index: i,
                });
            }
        }
        for c in inst.children.iter().rev() {
            if fills.values().any(|f| f == c) {
                step.push(Op::DeleteNode { id: *c });
            } else {
                step.extend(delete_subtree(&scratch, *c));
            }
        }
        step.push(Op::DeleteNode { id });
        Transaction::new("", step.clone()).apply(&mut scratch)?;
        ops.extend(step);
        new_ids.extend(roots);
    }
    Ok(Outcome::apply(
        Transaction::new("Detach instance", ops),
        json!({ "ids": new_ids }),
    )
    .selecting(new_ids))
}

fn delete_subtree(doc: &deesainer_doc::Document, id: NodeId) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut stack = vec![(id, false)];
    while let Some((n, expanded)) = stack.pop() {
        if expanded {
            ops.push(Op::DeleteNode { id: n });
            continue;
        }
        stack.push((n, true));
        if let Some(node) = doc.get(n) {
            for c in &node.children {
                stack.push((*c, false));
            }
        }
    }
    ops
}

fn go_to_main(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = match p.node_id("id")? {
        Some(id) => id,
        None => *ctx
            .selection
            .first()
            .ok_or_else(|| p.invalid("select an instance"))?,
    };
    let Some(NodeKind::Instance { component, .. }) = ctx.doc.get(id).map(|n| &n.kind) else {
        return Err(p.bad("id", "not an instance"));
    };
    if !ctx.doc.contains(*component) {
        return Err(p.invalid("the main component is missing (from another file?)"));
    }
    let page = ctx.doc.page_of(*component).unwrap_or(ctx.current_page);
    Ok(Outcome::read(json!({ "id": component }))
        .selecting(vec![*component])
        .on_page(page))
}

fn reset_overrides(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = instances_of(ctx, p)?;
    let ops = ids
        .iter()
        .filter_map(|id| match ctx.doc.get(*id).map(|n| &n.kind) {
            Some(NodeKind::Instance { component, .. }) => Some(Op::set(
                *id,
                PropValue::Kind(NodeKind::Instance {
                    component: *component,
                    overrides: Vec::new(),
                }),
            )),
            _ => None,
        })
        .collect();
    Ok(Outcome::apply(
        Transaction::new("Reset overrides", ops),
        json!({}),
    ))
}

fn set_override(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let Some(NodeKind::Instance {
        component,
        overrides,
    }) = ctx.doc.get(id).map(|n| &n.kind)
    else {
        return Err(p.bad("id", "not an instance"));
    };
    let target = p.req_node_id("target")?;
    let in_main = ctx.doc.is_ancestor(*component, target).unwrap_or(false);
    if !in_main {
        return Err(p.bad(
            "target",
            "must be a layer inside the instance's main component",
        ));
    }
    let tnode = ctx.doc.node(target)?;
    let raw = p.raw("value").ok_or_else(|| p.missing("value"))?;
    let value = match p.choice("prop", &["text", "fill", "visible", "opacity"])? {
        Some("text") => {
            let text = raw
                .as_str()
                .ok_or_else(|| p.bad("value", "must be a string"))?;
            let NodeKind::Text(data) = &tnode.kind else {
                return Err(p.bad("target", "not a text layer"));
            };
            let mut d = data.clone();
            d.content = text.to_owned();
            PropValue::Kind(NodeKind::Text(d))
        }
        Some("fill") => {
            let hex = raw
                .as_str()
                .ok_or_else(|| p.bad("value", "must be a hex colour"))?;
            let c =
                deesainer_color::Rgba::from_hex(hex).map_err(|e| p.bad("value", e.to_string()))?;
            PropValue::Fills(vec![Paint::solid(c)])
        }
        Some("visible") => PropValue::Visible(
            raw.as_bool()
                .ok_or_else(|| p.bad("value", "must be a bool"))?,
        ),
        Some("opacity") => {
            let o = raw
                .as_f64()
                .filter(|o| (0.0..=1.0).contains(o))
                .ok_or_else(|| p.bad("value", "must be 0..1"))?;
            PropValue::Opacity(o as f32)
        }
        _ => return Err(p.missing("prop")),
    };
    let mut list = overrides.clone();
    list.retain(|o| !(o.target == target && o.value.key() == value.key()));
    list.push(Override { target, value });
    Ok(Outcome::apply(
        Transaction::new(
            "Override",
            vec![Op::set(
                id,
                PropValue::Kind(NodeKind::Instance {
                    component: *component,
                    overrides: list,
                }),
            )],
        ),
        json!({}),
    ))
}

fn main_parts(
    ctx: &Context<'_>,
    p: &Params<'_>,
) -> Result<(NodeId, String, Vec<deesainer_doc::ComponentProp>), EngineError> {
    let id = p.req_node_id("id")?;
    match ctx.doc.get(id).map(|n| &n.kind) {
        Some(NodeKind::Component {
            description,
            properties,
        }) => Ok((id, description.clone(), properties.clone())),
        _ => Err(p.bad("id", "not a main component")),
    }
}

fn add_property(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    use deesainer_doc::{ComponentProp, ComponentPropKind};
    let (id, description, mut properties) = main_parts(ctx, p)?;
    let name = p
        .str("name")?
        .map(str::trim)
        .filter(|n| !n.is_empty() && n.len() <= 80)
        .ok_or_else(|| p.bad("name", "needs 1 to 80 characters"))?
        .to_owned();
    if properties.iter().any(|x| x.name == name) {
        return Err(p.bad("name", "already used by another property"));
    }
    if properties.len() >= ComponentProp::MAX {
        return Err(p.bad("name", "a component takes at most 32 properties"));
    }
    let target = p.req_node_id("target")?;
    if target == id || !ctx.doc.is_ancestor(id, target).unwrap_or(false) {
        return Err(p.bad("target", "must be a layer inside the component"));
    }
    let node = ctx.doc.node(target)?;
    let kind = match p.choice("kind", &["boolean", "text", "instanceSwap"])? {
        Some("boolean") => ComponentPropKind::Boolean {
            default: node.visible,
        },
        Some("text") => match &node.kind {
            NodeKind::Text(d) => ComponentPropKind::Text {
                default: d.content.clone(),
            },
            _ => return Err(p.bad("target", "not a text layer")),
        },
        Some("instanceSwap") => match &node.kind {
            NodeKind::Instance { component, .. } => ComponentPropKind::InstanceSwap {
                default: *component,
            },
            _ => return Err(p.bad("target", "not an instance")),
        },
        _ => return Err(p.missing("kind")),
    };
    properties.push(ComponentProp { name, target, kind });
    Ok(Outcome::apply(
        Transaction::new(
            "Add component property",
            vec![Op::set(
                id,
                PropValue::Kind(NodeKind::Component {
                    description,
                    properties,
                }),
            )],
        ),
        json!({}),
    ))
}

fn remove_property(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, description, mut properties) = main_parts(ctx, p)?;
    let name = p.str("name")?.ok_or_else(|| p.missing("name"))?;
    let before = properties.len();
    properties.retain(|x| x.name != name);
    if properties.len() == before {
        return Err(p.bad("name", "no such property"));
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Remove component property",
            vec![Op::set(
                id,
                PropValue::Kind(NodeKind::Component {
                    description,
                    properties,
                }),
            )],
        ),
        json!({}),
    ))
}

fn set_property(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    use deesainer_doc::ComponentPropKind;
    let id = p.req_node_id("id")?;
    let Some(NodeKind::Instance {
        component,
        overrides,
    }) = ctx.doc.get(id).map(|n| &n.kind)
    else {
        return Err(p.bad("id", "not an instance"));
    };
    let Some(NodeKind::Component { properties, .. }) = ctx.doc.get(*component).map(|n| &n.kind)
    else {
        return Err(p.bad("id", "the instance's main component is missing"));
    };
    let name = p.str("name")?.ok_or_else(|| p.missing("name"))?;
    let prop = properties
        .iter()
        .find(|x| x.name == name)
        .ok_or_else(|| p.bad("name", "no such property on the main component"))?;
    let raw = p.raw("value").ok_or_else(|| p.missing("value"))?;
    let tnode = ctx.doc.node(prop.target)?;
    let (value, is_default) = match &prop.kind {
        ComponentPropKind::Boolean { default } => {
            let v = raw
                .as_bool()
                .ok_or_else(|| p.bad("value", "must be a bool"))?;
            (PropValue::Visible(v), v == *default)
        }
        ComponentPropKind::Text { default } => {
            let v = raw
                .as_str()
                .ok_or_else(|| p.bad("value", "must be a string"))?;
            let NodeKind::Text(data) = &tnode.kind else {
                return Err(p.bad("name", "the bound layer is no longer a text layer"));
            };
            let mut d = data.clone();
            d.content = v.to_owned();
            (PropValue::Kind(NodeKind::Text(d)), v == default)
        }
        ComponentPropKind::InstanceSwap { default } => {
            let v = NodeId(
                raw.as_u64()
                    .ok_or_else(|| p.bad("value", "must be a main component id"))?,
            );
            if !matches!(
                ctx.doc.get(v).map(|n| &n.kind),
                Some(NodeKind::Component { .. })
            ) {
                return Err(p.bad("value", "not a main component"));
            }
            if v == *component || ctx.doc.is_ancestor(*component, v).unwrap_or(false) {
                return Err(p.bad("value", "a component cannot contain itself"));
            }
            (
                PropValue::Kind(NodeKind::Instance {
                    component: v,
                    overrides: Vec::new(),
                }),
                v == *default,
            )
        }
        ComponentPropKind::Slot { .. } => {
            return Err(p.bad(
                "name",
                "a slot is filled with layers: use instance.fillSlot or instance.clearSlot",
            ))
        }
    };
    let target = prop.target;
    let mut list = overrides.clone();
    list.retain(|o| !(o.target == target && o.value.key() == value.key()));
    if !is_default {
        list.push(Override { target, value });
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Set component property",
            vec![Op::set(
                id,
                PropValue::Kind(NodeKind::Instance {
                    component: *component,
                    overrides: list,
                }),
            )],
        ),
        json!({}),
    ))
}

fn combine(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = top_level(ctx.doc, &layer_ids(ctx, p)?);
    if ids.iter().any(|id| !ctx.doc.get(*id).is_some_and(is_main)) {
        return Err(p.invalid("select main components only"));
    }
    let parent = ctx
        .doc
        .node(ids[0])?
        .parent
        .ok_or_else(|| p.invalid("component without parent"))?;
    if ids
        .iter()
        .any(|id| ctx.doc.get(*id).and_then(|n| n.parent) != Some(parent))
    {
        return Err(p.invalid("select components that share the same parent"));
    }
    let bounds = ids
        .iter()
        .filter_map(|id| ctx.doc.get(*id).map(bounds_in_parent))
        .reduce(Rect::union)
        .ok_or_else(|| p.invalid("nothing to combine"))?
        .inflate(20.0);
    let sid = ctx.doc.next_free_id();
    let base = ctx
        .doc
        .node(ids[0])?
        .name
        .split(',')
        .next()
        .unwrap_or("Component")
        .split('=')
        .next()
        .unwrap_or("Component")
        .trim()
        .to_owned();
    let mut set = Node::new(sid, base, NodeKind::ComponentSet, bounds);
    set.strokes
        .push(Paint::solid(deesainer_color::Rgba::from_rgba8([
            0x97, 0x47, 0xFF, 0xFF,
        ])));
    set.stroke.dash = vec![4.0, 4.0];
    set.clips_content = false;
    let top = ids
        .iter()
        .filter_map(|id| ctx.doc.index_in_parent(*id).ok())
        .max()
        .unwrap_or(0);
    let mut ops = vec![Op::CreateNode {
        parent,
        index: top + 1,
        node: Box::new(set),
    }];
    for (k, id) in ids.iter().enumerate() {
        let n = ctx.doc.node(*id)?;
        ops.push(Op::MoveNode {
            id: *id,
            parent: sid,
            index: k,
        });
        ops.push(Op::set(
            *id,
            PropValue::Rect(n.rect.translate(-bounds.x, -bounds.y)),
        ));
        if variant_props(&n.name).is_empty() {
            let name = if k == 0 {
                "Property 1=Default".to_owned()
            } else {
                format!("Property 1=Variant{}", k + 1)
            };
            ops.push(Op::set(*id, PropValue::Name(name)));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Combine as variants", ops),
        json!({ "id": sid }),
    )
    .selecting(vec![sid]))
}

fn swap_ops(ctx: &Context<'_>, id: NodeId, to: NodeId) -> Option<Op> {
    let NodeKind::Instance {
        component,
        overrides,
    } = &ctx.doc.get(id)?.kind
    else {
        return None;
    };
    let remapped = remap_overrides(ctx.doc, *component, to, overrides);
    Some(Op::set(
        id,
        PropValue::Kind(NodeKind::Instance {
            component: to,
            overrides: remapped,
        }),
    ))
}

fn set_variant(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = instances_of(ctx, p)?;
    let wanted: BTreeMap<String, String> = p.req_json("properties")?;
    let mut ops = Vec::new();
    for id in ids {
        let Some(NodeKind::Instance { component, .. }) = ctx.doc.get(id).map(|n| &n.kind) else {
            continue;
        };
        let main = ctx.doc.node(*component)?;
        let set = main
            .parent
            .and_then(|s| ctx.doc.get(s))
            .filter(|s| matches!(s.kind, NodeKind::ComponentSet));
        let Some(set) = set else {
            return Err(p.invalid("the instance's component is not part of a variant set"));
        };
        let mut props = variant_props(&main.name);
        props.extend(wanted.clone());
        let target = set
            .children
            .iter()
            .filter_map(|c| ctx.doc.get(*c))
            .find(|c| is_main(c) && variant_props(&c.name) == props)
            .map(|c| c.id)
            .ok_or_else(|| p.invalid(format!("no variant named {:?}", variant_name(&props))))?;
        if let Some(op) = swap_ops(ctx, id, target) {
            ops.push(op);
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Set variant", ops),
        json!({}),
    ))
}

fn swap(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = instances_of(ctx, p)?;
    let to = main_component(ctx, p, "component")?;
    let ops = ids.iter().filter_map(|id| swap_ops(ctx, *id, to)).collect();
    Ok(Outcome::apply(
        Transaction::new("Swap instance", ops),
        json!({}),
    ))
}

fn list(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let mut counts: BTreeMap<NodeId, usize> = BTreeMap::new();
    for n in ctx.doc.nodes() {
        if let NodeKind::Instance { component, .. } = n.kind {
            *counts.entry(component).or_default() += 1;
        }
    }
    let out: Vec<Value> = ctx
        .doc
        .nodes()
        .filter(|n| matches!(n.kind, NodeKind::Component { .. } | NodeKind::ComponentSet))
        .map(|n| {
            let set = n
                .parent
                .and_then(|s| ctx.doc.get(s))
                .filter(|s| matches!(s.kind, NodeKind::ComponentSet))
                .map(|s| s.id);
            let layers: Vec<Value> = ctx
                .doc
                .subtree_pre_order(n.id)
                .unwrap_or_default()
                .into_iter()
                .skip(1)
                .filter_map(|id| ctx.doc.get(id))
                .map(|l| json!({ "id": l.id, "name": l.name, "type": l.kind.type_name() }))
                .collect();
            json!({
                "id": n.id,
                "name": n.name,
                "type": n.kind.type_name(),
                "page": ctx.doc.page_of(n.id),
                "set": set,
                "variant": variant_props(&n.name),
                "instances": counts.get(&n.id).copied().unwrap_or(0),
                "layers": layers,
            })
        })
        .collect();
    Ok(Outcome::read(Value::Array(out)))
}
