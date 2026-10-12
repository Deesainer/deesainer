use std::collections::BTreeMap;

use deesainer_doc::{Document, Node, NodeId, NodeKind, Override, PropKey, PropValue};
use deesainer_ops::{Op, Transaction};

pub const SYNCED: [PropKey; 9] = [
    PropKey::Fills,
    PropKey::Strokes,
    PropKey::Stroke,
    PropKey::Effects,
    PropKey::CornerRadius,
    PropKey::Opacity,
    PropKey::Blend,
    PropKey::ClipsContent,
    PropKey::Layout,
];

pub fn capture_overrides(doc: &Document, tx: &Transaction) -> Transaction {
    let mut pending: BTreeMap<NodeId, (NodeId, Vec<Override>)> = BTreeMap::new();
    let kind_set: Vec<NodeId> = tx
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::SetProp {
                id,
                value: PropValue::Kind(_),
            } => Some(*id),
            _ => None,
        })
        .collect();
    for op in &tx.ops {
        let Op::SetProp { id, value } = op else {
            continue;
        };
        if !SYNCED.contains(&value.key()) || kind_set.contains(id) {
            continue;
        }
        let Some(NodeKind::Instance {
            component,
            overrides,
        }) = doc.get(*id).map(|n| &n.kind)
        else {
            continue;
        };
        let entry = pending
            .entry(*id)
            .or_insert_with(|| (*component, overrides.clone()));
        let comp = entry.0;
        entry
            .1
            .retain(|o| !(o.target == comp && o.value.key() == value.key()));
        entry.1.push(Override {
            target: comp,
            value: value.clone(),
        });
    }
    if pending.is_empty() {
        return tx.clone();
    }
    let mut out = tx.clone();
    for (id, (component, overrides)) in pending {
        out.ops.push(Op::set(
            id,
            PropValue::Kind(NodeKind::Instance {
                component,
                overrides,
            }),
        ));
    }
    out
}

pub fn sync_instances(doc: &Document) -> Vec<Op> {
    let mut ops = Vec::new();
    for node in doc.nodes() {
        let NodeKind::Instance {
            component,
            overrides,
        } = &node.kind
        else {
            continue;
        };
        let Some(main) = doc.get(*component) else {
            continue;
        };
        for key in SYNCED {
            let desired = overrides
                .iter()
                .rev()
                .find(|o| o.target == *component && o.value.key() == key)
                .map(|o| o.value.clone())
                .unwrap_or_else(|| main.get(key));
            if node.get(key) != desired {
                ops.push(Op::set(node.id, desired));
            }
        }
    }
    ops
}

pub fn main_sizes(doc: &Document) -> BTreeMap<NodeId, (f64, f64)> {
    doc.nodes()
        .filter(|n| matches!(n.kind, NodeKind::Component { .. }))
        .map(|n| (n.id, (n.rect.width, n.rect.height)))
        .collect()
}

pub fn follow_main_size(before: &BTreeMap<NodeId, (f64, f64)>, doc: &Document) -> Vec<Op> {
    const EPS: f64 = 1e-6;
    let mut ops = Vec::new();
    for node in doc.nodes() {
        let NodeKind::Instance { component, .. } = &node.kind else {
            continue;
        };
        let (Some((w, h)), Some(main)) = (before.get(component), doc.get(*component)) else {
            continue;
        };
        let resized = (main.rect.width - w).abs() > EPS || (main.rect.height - h).abs() > EPS;
        let matched = (node.rect.width - w).abs() <= EPS && (node.rect.height - h).abs() <= EPS;
        if resized && matched {
            let mut r = node.rect;
            r.width = main.rect.width;
            r.height = main.rect.height;
            ops.push(Op::set(node.id, PropValue::Rect(r)));
        }
    }
    ops
}

pub fn variant_props(name: &str) -> BTreeMap<String, String> {
    name.split(',')
        .filter_map(|part| {
            let (k, v) = part.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().to_owned()))
        })
        .collect()
}

pub fn variant_name(props: &BTreeMap<String, String>) -> String {
    props
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn remap_overrides(
    doc: &Document,
    from: NodeId,
    to: NodeId,
    overrides: &[Override],
) -> Vec<Override> {
    let names_in = |root: NodeId| -> Vec<(NodeId, String)> {
        doc.subtree_pre_order(root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| doc.get(id).map(|n| (id, n.name.clone())))
            .collect()
    };
    let new_nodes = names_in(to);
    overrides
        .iter()
        .filter_map(|o| {
            if o.target == from {
                return Some(Override {
                    target: to,
                    value: o.value.clone(),
                });
            }
            let name = doc.get(o.target)?.name.clone();
            let target = new_nodes.iter().find(|(id, n)| *n == name && *id != to)?.0;
            Some(Override {
                target,
                value: o.value.clone(),
            })
        })
        .collect()
}

pub fn materialize(doc: &Document, instance: &Node) -> Option<BTreeMap<NodeId, Node>> {
    let NodeKind::Instance {
        component,
        overrides,
    } = &instance.kind
    else {
        return None;
    };
    let main = doc.get(*component)?;
    let mut nodes: BTreeMap<NodeId, Node> = BTreeMap::new();
    for id in doc.subtree_pre_order(*component).ok()? {
        nodes.insert(id, doc.get(id)?.clone());
    }
    for (cid, rect) in
        deesainer_render::instance_child_rects(doc, main, instance.rect.width, instance.rect.height)
    {
        if let Some(n) = nodes.get_mut(&cid) {
            n.rect = rect;
        }
    }
    for o in overrides {
        if o.target == *component {
            continue;
        }
        if let Some(n) = nodes.get_mut(&o.target) {
            n.set(o.value.clone());
        }
    }
    let root = nodes.get_mut(component)?;
    let mut frame = instance.clone();
    frame.id = root.id;
    frame.kind = NodeKind::Frame;
    frame.children = root.children.clone();
    frame.parent = root.parent;
    *root = frame;
    Some(nodes)
}
