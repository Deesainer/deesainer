use std::collections::BTreeMap;

use deesainer_doc::{Document, NodeId, NodeKind, PropValue};
use deesainer_geom::Rect;
use deesainer_ops::Op;

pub type Pass = fn(&Document) -> Vec<Op>;

pub const PASSES: &[Pass] = &[
    apply_styles,
    apply_variables,
    crate::components::sync_instances,
    crate::slots::sync_slot_content,
    text_autosize,
    fit_groups,
    auto_layout,
    fit_groups,
];

const EPS: f64 = 1e-9;

fn differs(a: f64, b: f64) -> bool {
    (a - b).abs() > EPS
}

pub fn fit_groups(doc: &Document) -> Vec<Op> {
    let mut groups: Vec<(usize, NodeId)> = doc
        .nodes()
        .filter(|n| matches!(n.kind, NodeKind::Group | NodeKind::BooleanOp { .. }))
        .map(|n| (doc.ancestors(n.id).len(), n.id))
        .collect();
    groups.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut rects: BTreeMap<NodeId, Rect> = BTreeMap::new();
    let mut deleted: Vec<NodeId> = Vec::new();
    let mut ops = Vec::new();
    for (_, gid) in groups {
        let Some(group) = doc.get(gid) else { continue };
        let live: Vec<NodeId> = group
            .children
            .iter()
            .copied()
            .filter(|c| !deleted.contains(c))
            .collect();
        if live.is_empty() {
            ops.push(Op::DeleteNode { id: gid });
            deleted.push(gid);
            continue;
        }
        if group.rotation != 0.0 {
            continue;
        }
        let mut bounds: Option<Rect> = None;
        for c in &live {
            let Some(child) = doc.get(*c) else { continue };
            let mut child = child.clone();
            if let Some(r) = rects.get(c) {
                child.rect = *r;
            }
            let local = Rect::new(0.0, 0.0, child.rect.width, child.rect.height);
            let b = Document::local_transform(&child).transform_rect_bounds(&local);
            bounds = Some(bounds.map_or(b, |acc| acc.union(b)));
        }
        let Some(b) = bounds else { continue };
        let current = rects.get(&gid).copied().unwrap_or(group.rect);
        let moved = differs(b.x, 0.0) || differs(b.y, 0.0);
        let resized = differs(b.width, current.width) || differs(b.height, current.height);
        if !moved && !resized {
            continue;
        }
        let new_rect = Rect::new(current.x + b.x, current.y + b.y, b.width, b.height);
        rects.insert(gid, new_rect);
        ops.push(Op::set(gid, PropValue::Rect(new_rect)));
        if moved {
            for c in &live {
                let Some(child) = doc.get(*c) else { continue };
                let r = rects.get(c).copied().unwrap_or(child.rect);
                let shifted = r.translate(-b.x, -b.y);
                rects.insert(*c, shifted);
                ops.push(Op::set(*c, PropValue::Rect(shifted)));
            }
        }
    }
    ops
}

pub fn text_autosize(doc: &Document) -> Vec<Op> {
    let mut ops = Vec::new();
    for node in doc.nodes() {
        let NodeKind::Text(data) = &node.kind else {
            continue;
        };
        let width_owned_by_layout = node.layout_child.horizontal == deesainer_doc::Sizing::Fill
            && !node.layout_child.absolute
            && node
                .parent
                .and_then(|p| doc.get(p))
                .is_some_and(|p| p.layout.is_some());
        let fixed_width = match data.auto_resize {
            deesainer_doc::TextAutoResize::None => continue,
            _ if width_owned_by_layout => Some(node.rect.width),
            deesainer_doc::TextAutoResize::Height => Some(node.rect.width),
            deesainer_doc::TextAutoResize::WidthAndHeight => None,
        };
        let (w, h) = crate::text_size(node, data, fixed_width);
        if !(w.is_finite() && h.is_finite()) {
            continue;
        }
        if differs(w, node.rect.width) || differs(h, node.rect.height) {
            ops.push(Op::set(node.id, PropValue::Rect(node.rect.with_size(w, h))));
        }
    }
    ops
}

pub fn apply_styles(doc: &Document) -> Vec<Op> {
    use deesainer_doc::StyleKind;
    let mut ops = Vec::new();
    for node in doc.nodes() {
        let refs = node.styles;
        if let Some(StyleKind::Paint(p)) = refs.fill.and_then(|s| doc.style(s)).map(|s| &s.kind) {
            if &node.fills != p {
                ops.push(Op::set(node.id, PropValue::Fills(p.clone())));
            }
        }
        if let Some(StyleKind::Paint(p)) = refs.stroke.and_then(|s| doc.style(s)).map(|s| &s.kind) {
            if &node.strokes != p {
                ops.push(Op::set(node.id, PropValue::Strokes(p.clone())));
            }
        }
        if let Some(StyleKind::Grid(g)) = refs.grid.and_then(|s| doc.style(s)).map(|s| &s.kind) {
            if &node.layout_grids != g {
                ops.push(Op::set(node.id, PropValue::LayoutGrids(g.clone())));
            }
        }
        if let Some(StyleKind::Effect(e)) = refs.effect.and_then(|s| doc.style(s)).map(|s| &s.kind)
        {
            if &node.effects != e {
                ops.push(Op::set(node.id, PropValue::Effects(e.clone())));
            }
        }
        if let (Some(StyleKind::Text(ts)), NodeKind::Text(data)) = (
            refs.text.and_then(|s| doc.style(s)).map(|s| &s.kind),
            &node.kind,
        ) {
            if &data.style != ts {
                let mut d = data.clone();
                d.style = ts.clone();
                ops.push(Op::set(node.id, PropValue::Kind(NodeKind::Text(d))));
            }
        }
    }
    ops
}

pub fn apply_variables(doc: &Document) -> Vec<Op> {
    use deesainer_doc::{BindTarget, CornerRadius, Padding, Paint, PaintKind, VarValue};
    let mut ops = Vec::new();
    for node in doc.nodes() {
        for (target, var) in &node.bindings {
            let Some(value) = doc.resolve_variable_at(*var, node.id) else {
                continue;
            };
            let set_color = |paints: &[Paint], c: deesainer_color::Rgba| -> Option<Vec<Paint>> {
                let mut out = paints.to_vec();
                match out
                    .iter_mut()
                    .find(|p| matches!(p.kind, PaintKind::Solid { .. }))
                {
                    Some(p) => {
                        if p.kind == (PaintKind::Solid { color: c }) {
                            return None;
                        }
                        p.kind = PaintKind::Solid { color: c };
                    }
                    None => out.push(Paint::solid(c)),
                }
                Some(out)
            };
            match (target, value) {
                (BindTarget::FillColor, VarValue::Color(c)) => {
                    if let Some(f) = set_color(&node.fills, *c) {
                        ops.push(Op::set(node.id, PropValue::Fills(f)));
                    }
                }
                (BindTarget::StrokeColor, VarValue::Color(c)) => {
                    if let Some(f) = set_color(&node.strokes, *c) {
                        ops.push(Op::set(node.id, PropValue::Strokes(f)));
                    }
                }
                (BindTarget::Opacity, VarValue::Number(n)) => {
                    let o = if *n > 1.0 { n / 100.0 } else { *n }.clamp(0.0, 1.0) as f32;
                    if (o - node.opacity).abs() > 1e-6 {
                        ops.push(Op::set(node.id, PropValue::Opacity(o)));
                    }
                }
                (BindTarget::CornerRadius, VarValue::Number(n)) if *n >= 0.0 => {
                    let r = CornerRadius::uniform(*n);
                    if r != node.corner_radius {
                        ops.push(Op::set(node.id, PropValue::CornerRadius(r)));
                    }
                }
                (BindTarget::Width, VarValue::Number(n))
                    if *n >= 0.0 && differs(*n, node.rect.width) =>
                {
                    ops.push(Op::set(
                        node.id,
                        PropValue::Rect(node.rect.with_size(*n, node.rect.height)),
                    ));
                }
                (BindTarget::Height, VarValue::Number(n))
                    if *n >= 0.0 && differs(*n, node.rect.height) =>
                {
                    ops.push(Op::set(
                        node.id,
                        PropValue::Rect(node.rect.with_size(node.rect.width, *n)),
                    ));
                }
                (BindTarget::Gap, VarValue::Number(n)) => {
                    if let Some(mut l) = node.layout {
                        if differs(l.gap, *n) {
                            l.gap = *n;
                            ops.push(Op::set(node.id, PropValue::Layout(Some(l))));
                        }
                    }
                }
                (BindTarget::Padding, VarValue::Number(n)) => {
                    if let Some(mut l) = node.layout {
                        let pad = Padding::uniform(*n);
                        if l.padding != pad {
                            l.padding = pad;
                            ops.push(Op::set(node.id, PropValue::Layout(Some(l))));
                        }
                    }
                }
                (BindTarget::TextContent, v) => {
                    let text = match v {
                        VarValue::String(s) => s.clone(),
                        VarValue::Number(n) => format!("{n}"),
                        VarValue::Bool(b) => format!("{b}"),
                        _ => continue,
                    };
                    if let NodeKind::Text(data) = &node.kind {
                        if data.content != text {
                            let mut d = data.clone();
                            d.content = text;
                            ops.push(Op::set(node.id, PropValue::Kind(NodeKind::Text(d))));
                        }
                    }
                }
                (BindTarget::Visible, VarValue::Bool(b)) if *b != node.visible => {
                    ops.push(Op::set(node.id, PropValue::Visible(*b)));
                }
                _ => {}
            }
        }
    }
    ops
}

pub fn auto_layout(doc: &Document) -> Vec<Op> {
    let mut frames: Vec<(usize, NodeId)> = doc
        .nodes()
        .filter(|n| {
            n.layout.is_some()
                && n.kind.is_frame_like()
                && !matches!(n.kind, NodeKind::Instance { .. })
        })
        .map(|n| (doc.ancestors(n.id).len(), n.id))
        .collect();
    if frames.is_empty() {
        return Vec::new();
    }
    frames.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let deepest_first: Vec<NodeId> = frames.iter().map(|(_, id)| *id).collect();
    let mut rects: BTreeMap<NodeId, Rect> = BTreeMap::new();
    let rect_of = |rects: &BTreeMap<NodeId, Rect>, id: NodeId| -> Option<Rect> {
        rects
            .get(&id)
            .copied()
            .or_else(|| doc.get(id).map(|n| n.rect))
    };
    for _ in 0..6 {
        let mut changed = false;
        let order: Vec<NodeId> = deepest_first
            .iter()
            .copied()
            .chain(deepest_first.iter().rev().copied())
            .collect();
        for fid in order {
            let Some(frame) = doc.get(fid) else { continue };
            let Some(layout) = frame.layout else { continue };
            let Some(frect) = rect_of(&rects, fid) else {
                continue;
            };
            let kids: Vec<&deesainer_doc::Node> =
                frame.children.iter().filter_map(|c| doc.get(*c)).collect();
            let children: Vec<deesainer_algo::Child> = kids
                .iter()
                .map(|c| {
                    let r = rect_of(&rects, c.id).unwrap_or(c.rect);
                    let (hw, hh) = match &c.kind {
                        NodeKind::Text(data) => match data.auto_resize {
                            deesainer_doc::TextAutoResize::WidthAndHeight => {
                                crate::text_size(c, data, None)
                            }
                            _ => (r.width, r.height),
                        },
                        _ => (r.width, r.height),
                    };
                    let (horizontal, vertical) = match &c.kind {
                        NodeKind::Text(data) => {
                            use deesainer_doc::{Sizing, TextAutoResize};
                            let h = match (c.layout_child.horizontal, data.auto_resize) {
                                (Sizing::Fill, _) => Sizing::Fill,
                                (_, TextAutoResize::WidthAndHeight) => Sizing::Hug,
                                (other, _) => other,
                            };
                            let v = match (c.layout_child.vertical, data.auto_resize) {
                                (Sizing::Fill, _) => Sizing::Fill,
                                (_, TextAutoResize::None) => c.layout_child.vertical,
                                _ => Sizing::Hug,
                            };
                            (h, v)
                        }
                        _ => (c.layout_child.horizontal, c.layout_child.vertical),
                    };
                    deesainer_algo::Child {
                        width: r.width,
                        height: r.height,
                        horizontal,
                        vertical,
                        hug_width: hw,
                        hug_height: hh,
                        skip: !c.visible || c.layout_child.absolute,
                        limits: c.size_limits,
                    }
                })
                .collect();
            let reflow = |i: usize, w: f64| -> f64 {
                match kids.get(i).map(|c| (*c, &c.kind)) {
                    Some((c, NodeKind::Text(data))) => crate::text_size(c, data, Some(w)).1,
                    Some((c, _)) => rect_of(&rects, c.id).map_or(0.0, |r| r.height),
                    None => 0.0,
                }
            };
            let sol = deesainer_algo::solve_limited(
                &layout,
                frect.width,
                frect.height,
                &children,
                frame.size_limits,
                &reflow,
            );
            let new_frame = frect.with_size(sol.width, sol.height);
            if new_frame.is_valid() && new_frame != frect {
                rects.insert(fid, new_frame);
                changed = true;
            }
            for (c, r) in kids.iter().zip(sol.rects) {
                let Some(r) = r else { continue };
                let current = rect_of(&rects, c.id).unwrap_or(c.rect);
                if r.is_valid()
                    && (differs(r.x, current.x)
                        || differs(r.y, current.y)
                        || differs(r.width, current.width)
                        || differs(r.height, current.height))
                {
                    rects.insert(c.id, r);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut ops = Vec::new();
    for (id, r) in rects {
        let Some(node) = doc.get(id) else { continue };
        if node.rect == r {
            continue;
        }
        ops.push(Op::set(id, PropValue::Rect(r)));
        if let Ok(children) = crate::constraints::child_ops(doc, id, node.rect, r) {
            ops.extend(children);
        }
    }
    ops
}
