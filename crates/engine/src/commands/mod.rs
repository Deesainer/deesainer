mod annotation;
mod arrange;
mod collection;
mod comment;
mod component;
mod create;
mod edit;
mod file;
mod fonts;
mod handoff;
mod inspect;
mod io;
mod layout;
mod library;
mod library_assets;
mod lint;
mod media;
mod network;
mod page;
mod plugin;
mod props;
mod proto;
mod select;
mod slot;
mod style;
mod text;
mod vector;
mod versions;
mod view;

use std::collections::BTreeMap;

use deesainer_doc::{Action, Document, Node, NodeId, NodeKind, Override, Paint, PropValue};
use deesainer_ops::Op;

use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, ParamSpec, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    let mut all = Vec::new();
    all.extend(file::specs());
    all.extend(inspect::specs());
    all.extend(handoff::specs());
    all.extend(select::specs());
    all.extend(page::specs());
    all.extend(create::specs());
    all.extend(edit::specs());
    all.extend(arrange::specs());
    all.extend(props::specs());
    all.extend(text::specs());
    all.extend(fonts::specs());
    all.extend(vector::specs());
    all.extend(style::specs());
    all.extend(collection::specs());
    all.extend(annotation::specs());
    all.extend(lint::specs());
    all.extend(versions::specs());
    all.extend(layout::specs());
    all.extend(component::specs());
    all.extend(library::specs());
    all.extend(slot::specs());
    all.extend(network::specs());
    all.extend(media::specs());
    all.extend(io::specs());
    all.extend(comment::specs());
    all.extend(proto::specs());
    all.extend(plugin::specs());
    all.extend(view::specs());
    all
}

pub(crate) const fn param(
    name: &'static str,
    ty: ParamType,
    required: bool,
    doc: &'static str,
) -> ParamSpec {
    ParamSpec {
        name,
        ty,
        required,
        doc,
    }
}

pub(crate) const IDS: ParamSpec = param(
    "ids",
    ParamType::NodeIds,
    false,
    "Nodes to act on; defaults to the current selection.",
);

pub(crate) fn layer_ids(ctx: &Context<'_>, p: &Params<'_>) -> Result<Vec<NodeId>, EngineError> {
    let ids = p.ids_or_selection(ctx)?;
    for id in &ids {
        let node = ctx.doc.node(*id)?;
        if matches!(node.kind, NodeKind::Document | NodeKind::Page) {
            return Err(p.bad("ids", format!("{id} is a page; use the page.* commands")));
        }
    }
    Ok(ids)
}

pub(crate) fn top_level(doc: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    let mut out: Vec<NodeId> = Vec::new();
    for id in ids {
        if out.contains(id) {
            continue;
        }
        let covered = ids
            .iter()
            .any(|other| other != id && doc.is_ancestor(*other, *id).unwrap_or(false));
        if !covered {
            out.push(*id);
        }
    }
    out
}

pub(crate) fn validate_paints(
    p: &Params<'_>,
    key: &str,
    paints: &[Paint],
) -> Result<(), EngineError> {
    for paint in paints {
        if !(0.0..=1.0).contains(&paint.opacity) {
            return Err(p.bad(key, "paint opacity must be between 0 and 1"));
        }
        let finite = paint.is_finite();
        if !finite {
            return Err(p.bad(key, "paints must contain only finite numbers"));
        }
    }
    Ok(())
}

pub(crate) fn clone_subtrees(
    target: &Document,
    nodes: &BTreeMap<NodeId, Node>,
    roots: &[NodeId],
    parent: NodeId,
    index: usize,
    offset: (f64, f64),
) -> Result<(Vec<Op>, Vec<NodeId>), EngineError> {
    let (ops, roots, _) = clone_subtrees_mapped(target, nodes, roots, parent, index, offset)?;
    Ok((ops, roots))
}

pub(crate) type Cloned = (Vec<Op>, Vec<NodeId>, BTreeMap<NodeId, NodeId>);

pub(crate) fn clone_subtrees_mapped(
    target: &Document,
    nodes: &BTreeMap<NodeId, Node>,
    roots: &[NodeId],
    parent: NodeId,
    index: usize,
    offset: (f64, f64),
) -> Result<Cloned, EngineError> {
    let mut order = Vec::new();
    for root in roots {
        let mut stack = vec![(*root, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            if depth > deesainer_doc::MAX_DEPTH || order.len() > nodes.len() {
                return Err(EngineError::Internal("clipboard tree is cyclic".into()));
            }
            let node = nodes
                .get(&id)
                .ok_or_else(|| EngineError::Internal(format!("missing node {id} in copy")))?;
            order.push(id);
            for c in node.children.iter().rev() {
                stack.push((*c, depth + 1));
            }
        }
    }
    let fresh = target.fresh_ids(order.len());
    let map: BTreeMap<NodeId, NodeId> = order.iter().copied().zip(fresh).collect();
    let mut child_count: BTreeMap<NodeId, usize> = BTreeMap::new();
    let mut ops = Vec::with_capacity(order.len());
    let mut new_roots = Vec::new();
    for old in &order {
        let Some(src) = nodes.get(old) else { continue };
        let new_id = map.get(old).copied().unwrap_or(*old);
        let mut node = remap_node(src, &map);
        node.id = new_id;
        node.children.clear();
        let (p, idx) = if roots.contains(old) {
            node.rect = node.rect.translate(offset.0, offset.1);
            let idx = index + new_roots.len();
            new_roots.push(new_id);
            (parent, idx)
        } else {
            let np = src
                .parent
                .and_then(|pp| map.get(&pp).copied())
                .ok_or_else(|| EngineError::Internal(format!("orphan {old} in copy")))?;
            let count = child_count.entry(np).or_insert(0);
            let idx = *count;
            *count += 1;
            (np, idx)
        };
        ops.push(Op::CreateNode {
            parent: p,
            index: idx,
            node: Box::new(node),
        });
    }
    Ok((ops, new_roots, map))
}

fn remap_node(src: &Node, map: &BTreeMap<NodeId, NodeId>) -> Node {
    let m = |id: NodeId| map.get(&id).copied().unwrap_or(id);
    let mut node = src.clone();
    node.parent = None;
    node.origin = None;
    if let NodeKind::Instance {
        component,
        overrides,
    } = &mut node.kind
    {
        *component = m(*component);
        for Override { target, value } in overrides.iter_mut() {
            *target = m(*target);
            if let PropValue::Interactions(list) = value {
                remap_interactions(list, &m);
            }
        }
    }
    if let NodeKind::Component { properties, .. } = &mut node.kind {
        for prop in properties.iter_mut() {
            prop.target = m(prop.target);
            if let deesainer_doc::ComponentPropKind::InstanceSwap { default } = &mut prop.kind {
                *default = m(*default);
            }
        }
    }
    remap_interactions(&mut node.interactions, &m);
    node
}

fn remap_interactions(list: &mut [deesainer_doc::Interaction], m: &impl Fn(NodeId) -> NodeId) {
    for it in list {
        remap_action(&mut it.action, m);
    }
}

fn remap_action(action: &mut Action, m: &impl Fn(NodeId) -> NodeId) {
    match action {
        Action::Navigate { to } => *to = m(*to),
        Action::OpenOverlay { frame } => *frame = m(*frame),
        Action::Conditional {
            then, otherwise, ..
        } => {
            for a in then.iter_mut().chain(otherwise.iter_mut()) {
                remap_action(a, m);
            }
        }
        Action::CloseOverlay
        | Action::Back
        | Action::OpenUrl { .. }
        | Action::SetVariable { .. } => {}
    }
}

pub(crate) fn collect_subtrees(
    doc: &Document,
    roots: &[NodeId],
) -> Result<BTreeMap<NodeId, Node>, EngineError> {
    let mut out = BTreeMap::new();
    for root in roots {
        for id in doc.subtree_pre_order(*root)? {
            out.insert(id, doc.node(id)?.clone());
        }
    }
    Ok(out)
}

pub(crate) fn target_parent(ctx: &Context<'_>, p: &Params<'_>) -> Result<NodeId, EngineError> {
    let parent = p.node_id("parent")?.unwrap_or(ctx.current_page);
    let node = ctx
        .doc
        .get(parent)
        .ok_or_else(|| p.bad("parent", format!("{parent} does not exist")))?;
    if !node.kind.is_container() || matches!(node.kind, NodeKind::Document) {
        return Err(p.bad(
            "parent",
            format!("{parent} cannot contain layers (use a page, frame, group or component)"),
        ));
    }
    Ok(parent)
}
