use std::collections::BTreeMap;

use deesainer_doc::{AutoLayout, Document, Node, NodeId, NodeKind, PropValue, Sizing};
use deesainer_geom::{Affine, Point, Rect};
use deesainer_ops::Op;

const EPS: f64 = 1e-9;

fn overrides_of(doc: &Document, instance: &Node) -> Vec<BTreeMap<NodeId, Vec<PropValue>>> {
    let NodeKind::Instance {
        component,
        overrides,
    } = &instance.kind
    else {
        return Vec::new();
    };
    let Some(main) = doc.get(*component) else {
        return Vec::new();
    };
    let mut map: BTreeMap<NodeId, Vec<PropValue>> = BTreeMap::new();
    for (cid, rect) in
        deesainer_render::instance_child_rects(doc, main, instance.rect.width, instance.rect.height)
    {
        map.entry(cid).or_default().push(PropValue::Rect(rect));
    }
    for o in overrides {
        map.entry(o.target).or_default().push(o.value.clone());
    }
    vec![map]
}

pub fn slot_box(doc: &Document, instance: &Node, target: NodeId) -> Option<(Rect, f64, Node)> {
    let NodeKind::Instance { component, .. } = &instance.kind else {
        return None;
    };
    let stack = overrides_of(doc, instance);
    let mut path = vec![target];
    let mut at = target;
    while at != *component {
        at = doc.get(at)?.parent?;
        path.push(at);
        if path.len() > deesainer_doc::MAX_DEPTH {
            return None;
        }
    }
    path.pop();
    path.reverse();
    let mut t = Affine::IDENTITY;
    let mut resolved = None;
    for id in path {
        let node = deesainer_render::overridden(doc, &stack, id)?;
        t = t.then(Document::local_transform(&node));
        resolved = Some(node.into_owned());
    }
    let node = resolved?;
    let (w, h) = (node.rect.width, node.rect.height);
    let c = t.apply(Point::new(w / 2.0, h / 2.0));
    let rotation = t.b.atan2(t.a).to_degrees();
    let rotation = if rotation.abs() < EPS { 0.0 } else { rotation };
    Some((
        Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h),
        rotation,
        node,
    ))
}

pub fn layer_at(doc: &Document, instance: &Node, p: Point) -> Option<NodeId> {
    let NodeKind::Instance { component, .. } = &instance.kind else {
        return None;
    };
    let local = doc.world_transform(instance.id).invert()?.apply(p);
    let mut found = None;
    for id in doc.subtree_pre_order(*component).ok()?.into_iter().skip(1) {
        let Some((rect, _, node)) = slot_box(doc, instance, id) else {
            continue;
        };
        let hidden = !node.visible
            || doc
                .ancestors(id)
                .into_iter()
                .take_while(|a| a != component)
                .any(|a| doc.get(a).is_some_and(|n| !n.visible));
        if !hidden && rect.contains(local) {
            found = Some(id);
        }
    }
    found
}

pub fn layer_outline(doc: &Document, instance: &Node, target: NodeId) -> Option<[Point; 4]> {
    let (r, rotation, _) = slot_box(doc, instance, target)?;
    let t = doc.world_transform(instance.id);
    let c = r.center();
    let (s, k) = rotation.to_radians().sin_cos();
    let corner = |dx: f64, dy: f64| {
        let (x, y) = (dx * r.width / 2.0, dy * r.height / 2.0);
        t.apply(Point::new(c.x + x * k - y * s, c.y + x * s + y * k))
    };
    Some([
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ])
}

pub fn content_layout(slot: &Node) -> Option<AutoLayout> {
    slot.layout.map(|l| AutoLayout {
        primary_sizing: Sizing::Fixed,
        counter_sizing: Sizing::Fixed,
        ..l
    })
}

fn rect_differs(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() > EPS
        || (a.y - b.y).abs() > EPS
        || (a.width - b.width).abs() > EPS
        || (a.height - b.height).abs() > EPS
}

pub fn sync_slot_content(doc: &Document) -> Vec<Op> {
    let mut ops = Vec::new();
    for node in doc.nodes() {
        let NodeKind::SlotContent { slot } = &node.kind else {
            continue;
        };
        let Some(instance) = node.parent.and_then(|p| doc.get(p)) else {
            continue;
        };
        let Some(target) = doc.slot_target(instance.id, slot) else {
            continue;
        };
        let Some((rect, rotation, resolved)) = slot_box(doc, instance, target) else {
            continue;
        };
        if rect_differs(&node.rect, &rect) {
            ops.push(Op::set(node.id, PropValue::Rect(rect)));
        }
        if (node.rotation - rotation).abs() > EPS {
            ops.push(Op::set(node.id, PropValue::Rotation(rotation)));
        }
        let layout = content_layout(&resolved);
        if node.layout != layout {
            ops.push(Op::set(node.id, PropValue::Layout(layout)));
        }
        let clips = resolved.clips_content && resolved.kind.is_frame_like();
        if node.clips_content != clips {
            ops.push(Op::set(node.id, PropValue::ClipsContent(clips)));
        }
    }
    ops
}
