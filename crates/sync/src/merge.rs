use std::collections::{BTreeMap, BTreeSet};

use deesainer_doc::{Document, NodeId, PropKey};
use deesainer_ops::{Op, Transaction};

fn depth(doc: &Document, id: NodeId) -> usize {
    let mut d = 0;
    let mut cur = doc.get(id).and_then(|n| n.parent);
    while let Some(p) = cur {
        d += 1;
        if d > deesainer_doc::MAX_DEPTH {
            break;
        }
        cur = doc.get(p).and_then(|n| n.parent);
    }
    d
}

pub fn summarize(ops: &[Op]) -> Vec<String> {
    let count = |f: &dyn Fn(&Op) -> bool| ops.iter().filter(|o| f(o)).count();
    let mut lines = Vec::new();
    let mut add = |n: usize, what: &str| {
        if n > 0 {
            lines.push(format!("{n} {what}"));
        }
    };
    add(
        count(&|o| matches!(o, Op::CreateNode { .. })),
        "layer(s) added",
    );
    add(
        count(&|o| matches!(o, Op::DeleteNode { .. })),
        "layer(s) removed",
    );
    add(
        count(&|o| matches!(o, Op::MoveNode { .. })),
        "move(s) or reorder(s)",
    );
    add(
        count(&|o| matches!(o, Op::SetProp { .. })),
        "property change(s)",
    );
    add(
        count(&|o| matches!(o, Op::SetStyle { .. } | Op::SetVariable { .. })),
        "style or variable change(s)",
    );
    add(
        count(&|o| matches!(o, Op::SetComment { .. })),
        "comment change(s)",
    );
    add(
        count(&|o| matches!(o, Op::SetAsset { .. })),
        "image(s) copied",
    );
    lines
}

fn merge_keyed<K, V>(
    what: &str,
    base: &BTreeMap<K, &V>,
    theirs: &BTreeMap<K, &V>,
    ours: &BTreeMap<K, &V>,
    op: impl Fn(K, Option<V>) -> Op,
    prefer_theirs: bool,
    conflicts: &mut Vec<String>,
) -> (Vec<Op>, Vec<Op>)
where
    K: Ord + Copy + std::fmt::Debug,
    V: PartialEq + Clone,
{
    let keys: BTreeSet<K> = base.keys().chain(theirs.keys()).copied().collect();
    let (mut sets, mut deletes) = (Vec::new(), Vec::new());
    for k in keys {
        let (b, t, o) = (base.get(&k), theirs.get(&k), ours.get(&k));
        if t == b || t == o {
            continue;
        }
        if o != b {
            conflicts.push(format!("{what} {k:?} changed on both sides"));
        }
        if o == b || prefer_theirs {
            match t {
                Some(v) => sets.push(op(k, Some((*v).clone()))),
                None => deletes.push(op(k, None)),
            }
        }
    }
    (sets, deletes)
}

fn restricted(order: &[NodeId], keep: &BTreeSet<NodeId>) -> Vec<NodeId> {
    order
        .iter()
        .copied()
        .filter(|id| keep.contains(id))
        .collect()
}

pub fn merge(base: &Document, theirs: &Document, ours: &Document) -> (Vec<Op>, Vec<String>) {
    merge_with(base, theirs, ours, false)
}

pub fn merge_with(
    base: &Document,
    theirs: &Document,
    ours: &Document,
    prefer_theirs: bool,
) -> (Vec<Op>, Vec<String>) {
    let mut conflicts = Vec::new();
    let mut ops: Vec<Op> = theirs
        .assets()
        .filter(|(id, _)| ours.asset(id).is_none())
        .map(|(id, data)| Op::SetAsset {
            id: id.clone(),
            data: Some(data.clone()),
        })
        .collect();

    let mut created: Vec<NodeId> = theirs
        .nodes()
        .filter(|n| !base.contains(n.id))
        .map(|n| n.id)
        .collect();
    created.sort_by_key(|id| (depth(theirs, *id), theirs.index_in_parent(*id).unwrap_or(0)));
    let mut have: BTreeSet<NodeId> = ours.nodes().map(|n| n.id).collect();
    let mut counts: BTreeMap<NodeId, usize> =
        ours.nodes().map(|n| (n.id, n.children.len())).collect();
    for id in created {
        let Some(node) = theirs.get(id) else { continue };
        if have.contains(&id) {
            conflicts.push(format!(
                "{}: the same id was created on both sides",
                node.name
            ));
            continue;
        }
        let Some(parent) = node.parent.filter(|p| have.contains(p)) else {
            conflicts.push(format!("{}: its parent no longer exists", node.name));
            continue;
        };
        let count = counts.entry(parent).or_insert(0);
        let index = theirs.index_in_parent(id).unwrap_or(0).min(*count);
        *count += 1;
        let mut copy = node.clone();
        copy.children.clear();
        copy.parent = None;
        have.insert(id);
        counts.insert(id, 0);
        ops.push(Op::CreateNode {
            parent,
            index,
            node: Box::new(copy),
        });
    }

    let mut sets: Vec<Op> = Vec::new();
    for n in theirs.nodes() {
        let (Some(b), Some(o)) = (base.get(n.id), ours.get(n.id)) else {
            continue;
        };
        if n.parent != b.parent {
            match n.parent.filter(|p| have.contains(p)) {
                Some(parent) if o.parent == b.parent => {
                    let index = theirs
                        .index_in_parent(n.id)
                        .unwrap_or(0)
                        .min(counts.get(&parent).copied().unwrap_or(0));
                    *counts.entry(parent).or_insert(0) += 1;
                    ops.push(Op::MoveNode {
                        id: n.id,
                        parent,
                        index,
                    });
                }
                _ => conflicts.push(format!("{}: moved on both sides", n.name)),
            }
        }
        for key in PropKey::ALL {
            let (tv, bv, ov) = (n.get(key), b.get(key), o.get(key));
            if tv == bv || tv == ov {
                continue;
            }
            if ov == bv {
                sets.push(Op::set(n.id, tv));
            } else {
                conflicts.push(format!("{}: {key:?} changed on both sides", n.name));
                if prefer_theirs {
                    sets.push(Op::set(n.id, tv));
                }
            }
        }
    }
    ops.extend(sets);

    let mut sim = ours.clone();
    if Transaction::new("merge", ops.clone())
        .apply(&mut sim)
        .is_ok()
    {
        for parent in theirs.nodes() {
            let (Some(b), Some(s)) = (base.get(parent.id), sim.get(parent.id)) else {
                continue;
            };
            let common: BTreeSet<NodeId> = b
                .children
                .iter()
                .copied()
                .filter(|c| parent.children.contains(c) && s.children.contains(c))
                .collect();
            let (bo, to, so) = (
                restricted(&b.children, &common),
                restricted(&parent.children, &common),
                restricted(&s.children, &common),
            );
            if to == bo {
                continue;
            }
            if so != bo {
                if so != to {
                    conflicts.push(format!("{}: children reordered on both sides", parent.name));
                    if !prefer_theirs {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            let mut next = to.iter();
            let target: Vec<NodeId> = s
                .children
                .iter()
                .map(|c| {
                    if common.contains(c) {
                        next.next().copied().unwrap_or(*c)
                    } else {
                        *c
                    }
                })
                .collect();
            let mut current = s.children.clone();
            for (i, id) in target.iter().enumerate() {
                let Some(pos) = current.iter().position(|c| c == id) else {
                    continue;
                };
                if pos != i {
                    current.remove(pos);
                    current.insert(i, *id);
                    ops.push(Op::MoveNode {
                        id: *id,
                        parent: parent.id,
                        index: i,
                    });
                }
            }
        }
    }

    let gone: BTreeSet<NodeId> = base
        .nodes()
        .map(|n| n.id)
        .filter(|id| !theirs.contains(*id))
        .collect();
    let mut order: Vec<NodeId> = gone.iter().copied().collect();
    order.sort_by_key(|id| std::cmp::Reverse(depth(base, *id)));
    let mut deleted: BTreeSet<NodeId> = BTreeSet::new();
    for id in order {
        let Some(o) = ours.get(id) else { continue };
        if o.children
            .iter()
            .all(|c| deleted.contains(c) || theirs.get(*c).is_some_and(|t| t.parent != Some(id)))
        {
            deleted.insert(id);
            ops.push(Op::DeleteNode { id });
        } else {
            conflicts.push(format!(
                "{}: deleted on one side, extended on the other",
                o.name
            ));
        }
    }
    let (style_sets, style_deletes) = merge_keyed(
        "style",
        &base.styles().collect(),
        &theirs.styles().collect(),
        &ours.styles().collect(),
        |id, style| Op::SetStyle { id, style },
        prefer_theirs,
        &mut conflicts,
    );
    let (var_sets, var_deletes) = merge_keyed(
        "variable",
        &base.variables().collect(),
        &theirs.variables().collect(),
        &ours.variables().collect(),
        |id, variable| Op::SetVariable { id, variable },
        prefer_theirs,
        &mut conflicts,
    );
    let (comment_sets, comment_deletes) = merge_keyed(
        "comment",
        &base.comments().collect(),
        &theirs.comments().collect(),
        &ours.comments().collect(),
        |id, comment| Op::SetComment { id, comment },
        prefer_theirs,
        &mut conflicts,
    );
    let mut all: Vec<Op> = style_sets.into_iter().chain(var_sets).collect();
    all.append(&mut ops);
    all.extend(style_deletes);
    all.extend(var_deletes);
    all.extend(comment_sets);
    all.extend(comment_deletes);
    (all, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::{Node, NodeKind, PropValue};
    use deesainer_geom::{Point, Rect};
    use deesainer_ops::Transaction;

    fn rect(doc: &mut Document, id: u64, name: &str) -> NodeId {
        let page = doc.first_page();
        let n = Node::new(
            NodeId(id),
            name,
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let at = doc.get(page).map_or(0, |p| p.children.len());
        doc.insert(n, page, at).unwrap();
        NodeId(id)
    }

    fn apply(doc: &mut Document, ops: Vec<Op>) {
        Transaction::new("merge", ops).apply(doc).unwrap();
    }

    #[test]
    fn independent_edits_combine_and_clashes_keep_ours() {
        let mut base = Document::new();
        let a = rect(&mut base, 10, "A");
        let b = rect(&mut base, 11, "B");
        let c = rect(&mut base, 12, "C");
        let mut theirs = base.clone();
        let mut ours = base.clone();
        theirs.reserve_ids(1 << 32);
        apply(
            &mut theirs,
            vec![Op::set(a, PropValue::Name("A theirs".into()))],
        );
        apply(
            &mut theirs,
            vec![Op::set(
                b,
                PropValue::Rect(Rect::new(50.0, 0.0, 10.0, 10.0)),
            )],
        );
        let d = rect(&mut theirs, (1 << 32) + 1, "D");
        apply(&mut theirs, vec![Op::DeleteNode { id: c }]);
        apply(
            &mut ours,
            vec![Op::set(b, PropValue::Name("B ours".into()))],
        );
        let e = rect(&mut ours, 13, "E");
        let (ops, conflicts) = merge(&base, &theirs, &ours);
        assert!(conflicts.is_empty(), "{conflicts:?}");
        apply(&mut ours, ops);
        assert_eq!(ours.get(a).unwrap().name, "A theirs");
        assert_eq!(ours.get(b).unwrap().name, "B ours");
        assert_eq!(ours.get(b).unwrap().rect.x, 50.0);
        assert!(ours.contains(d) && ours.contains(e));
        assert!(!ours.contains(c));
    }

    #[test]
    fn the_same_property_changed_on_both_sides_is_a_conflict() {
        let mut base = Document::new();
        let a = rect(&mut base, 10, "A");
        let mut theirs = base.clone();
        let mut ours = base.clone();
        apply(
            &mut theirs,
            vec![Op::set(a, PropValue::Name("Theirs".into()))],
        );
        apply(&mut ours, vec![Op::set(a, PropValue::Name("Ours".into()))]);
        let (ops, conflicts) = merge(&base, &theirs, &ours);
        assert_eq!(conflicts.len(), 1);
        apply(&mut ours, ops);
        assert_eq!(ours.get(a).unwrap().name, "Ours");
        let (ops, _) = merge(&theirs, &theirs, &ours);
        assert!(ops.is_empty());
    }

    fn order(doc: &Document) -> Vec<NodeId> {
        doc.get(doc.first_page()).unwrap().children.clone()
    }

    #[test]
    fn a_reorder_on_one_side_is_taken_and_on_both_is_a_conflict() {
        let mut base = Document::new();
        let (a, b, c) = (
            rect(&mut base, 10, "A"),
            rect(&mut base, 11, "B"),
            rect(&mut base, 12, "C"),
        );
        let page = base.first_page();
        let mut theirs = base.clone();
        let mut ours = base.clone();
        apply(
            &mut theirs,
            vec![Op::MoveNode {
                id: a,
                parent: page,
                index: 2,
            }],
        );
        assert_eq!(order(&theirs), vec![b, c, a]);
        let d = rect(&mut ours, 13, "D");
        let (ops, conflicts) = merge(&base, &theirs, &ours);
        assert!(conflicts.is_empty(), "{conflicts:?}");
        apply(&mut ours, ops);
        assert_eq!(order(&ours), vec![b, c, a, d]);
        let mut ours = base.clone();
        apply(
            &mut ours,
            vec![Op::MoveNode {
                id: c,
                parent: page,
                index: 0,
            }],
        );
        let (ops, conflicts) = merge(&base, &theirs, &ours);
        assert_eq!(conflicts.len(), 1);
        apply(&mut ours, ops);
        assert_eq!(order(&ours), vec![c, a, b]);
    }

    #[test]
    fn styles_variables_and_comments_merge_by_key() {
        use deesainer_doc::{Comment, Style, StyleId, StyleKind, VarValue, Variable, VariableId};
        let mut base = Document::new();
        let page = base.first_page();
        let style = |name: &str| Style {
            name: name.into(),
            origin: None,
            description: String::new(),
            kind: StyleKind::Paint(vec![deesainer_doc::Paint::solid(
                deesainer_color::Rgba::BLACK,
            )]),
        };
        let shared = StyleId(50);
        base.set_style(shared, Some(style("Shared")));
        let mut theirs = base.clone();
        let mut ours = base.clone();
        theirs.set_style(StyleId(51), Some(style("New")));
        theirs.set_style(shared, Some(style("Shared renamed")));
        theirs
            .set_variable(
                VariableId(60),
                Some(Variable {
                    name: "gap".into(),
                    values: vec![VarValue::Number(8.0)],
                    ..Default::default()
                }),
            )
            .unwrap();
        let note = Comment {
            author: "Bob".into(),
            text: "Check this".into(),
            created: "2026-10-10T10:00:00Z".into(),
            page,
            position: Point::new(1.0, 2.0),
            node: None,
            resolved: false,
            replies: Vec::new(),
        };
        theirs.set_comment(deesainer_doc::CommentId(70), Some(note));
        ours.set_style(StyleId(52), Some(style("Ours")));
        let (ops, conflicts) = merge(&base, &theirs, &ours);
        assert!(conflicts.is_empty(), "{conflicts:?}");
        apply(&mut ours, ops);
        assert_eq!(ours.style(shared).unwrap().name, "Shared renamed");
        assert!(ours.style(StyleId(51)).is_some() && ours.style(StyleId(52)).is_some());
        assert!(ours.variable(VariableId(60)).is_some());
        assert!(ours.comment(deesainer_doc::CommentId(70)).is_some());
        let mut ours2 = base.clone();
        ours2.set_style(shared, Some(style("Shared ours")));
        let (ops, conflicts) = merge(&base, &theirs, &ours2);
        assert_eq!(conflicts.len(), 1);
        apply(&mut ours2, ops);
        assert_eq!(ours2.style(shared).unwrap().name, "Shared ours");
        let mut gone = base.clone();
        gone.set_style(shared, None);
        let mut ours3 = base.clone();
        let (ops, _) = merge(&base, &gone, &ours3);
        apply(&mut ours3, ops);
        assert!(ours3.style(shared).is_none());
    }

    #[test]
    fn preferring_theirs_applies_their_value_and_still_reports() {
        let mut base = Document::new();
        let a = rect(&mut base, 10, "A");
        let mut theirs = base.clone();
        let mut ours = base.clone();
        apply(
            &mut theirs,
            vec![Op::set(a, PropValue::Name("Theirs".into()))],
        );
        apply(&mut ours, vec![Op::set(a, PropValue::Name("Ours".into()))]);
        let (ops, conflicts) = merge_with(&base, &theirs, &ours, true);
        assert_eq!(conflicts.len(), 1);
        let preview = summarize(&ops);
        assert_eq!(preview, vec!["1 property change(s)".to_string()]);
        apply(&mut ours, ops);
        assert_eq!(ours.get(a).unwrap().name, "Theirs");
    }

    #[test]
    fn deleting_a_container_that_gained_children_is_refused() {
        let mut base = Document::new();
        let frame = {
            let page = base.first_page();
            let n = Node::new(
                NodeId(10),
                "F",
                NodeKind::Frame,
                Rect::new(0.0, 0.0, 50.0, 50.0),
            );
            base.insert(n, page, 0).unwrap();
            NodeId(10)
        };
        let mut theirs = base.clone();
        let mut ours = base.clone();
        apply(&mut theirs, vec![Op::DeleteNode { id: frame }]);
        let n = Node::new(
            NodeId(11),
            "Inside",
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 5.0, 5.0),
        );
        apply(
            &mut ours,
            vec![Op::CreateNode {
                parent: frame,
                index: 0,
                node: Box::new(n),
            }],
        );
        let (ops, conflicts) = merge(&base, &theirs, &ours);
        assert_eq!(conflicts.len(), 1);
        apply(&mut ours, ops);
        assert!(ours.contains(frame));
    }
}
