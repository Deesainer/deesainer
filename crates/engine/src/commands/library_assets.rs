use std::collections::{BTreeMap, BTreeSet};

use deesainer_doc::{
    CollectionId, Document, LibraryLink, Modes, Node, StyleId, VarValue, Variable, VariableId,
};
use deesainer_ops::Op;

#[derive(Default)]
pub(super) struct Carried {
    pub styles: BTreeMap<StyleId, StyleId>,
    pub variables: BTreeMap<VariableId, VariableId>,
    pub ops: Vec<Op>,
}

impl Carried {
    pub fn relink(&self, node: &mut Node) {
        let map = |s: &mut Option<StyleId>| *s = s.and_then(|id| self.styles.get(&id).copied());
        map(&mut node.styles.fill);
        map(&mut node.styles.stroke);
        map(&mut node.styles.effect);
        map(&mut node.styles.text);
        map(&mut node.styles.grid);
        node.bindings = node
            .bindings
            .iter()
            .filter_map(|(t, v)| Some((*t, *self.variables.get(v)?)))
            .collect();
    }
}

pub(super) fn carry<'a>(
    doc: &Document,
    foreign: &Document,
    library: &str,
    nodes: impl IntoIterator<Item = &'a Node>,
    next: &mut u64,
) -> Carried {
    let mut out = Carried::default();
    let mut styles = BTreeSet::new();
    let mut vars = BTreeSet::new();
    for n in nodes {
        let r = n.styles;
        styles.extend(
            [r.fill, r.stroke, r.effect, r.text, r.grid]
                .into_iter()
                .flatten(),
        );
        vars.extend(n.bindings.values().copied());
    }
    let mut queue: Vec<VariableId> = vars.iter().copied().collect();
    while let Some(id) = queue.pop() {
        let Some(v) = foreign.variable(id) else {
            continue;
        };
        for value in &v.values {
            if let VarValue::Alias(to) = value {
                if vars.len() < 10_000 && vars.insert(*to) {
                    queue.push(*to);
                }
            }
        }
    }
    let linked = |link: &Option<LibraryLink>| {
        link.as_ref()
            .filter(|l| l.library == library)
            .map(|l| l.source)
    };
    let mut fresh = || {
        let id = *next;
        *next = next.saturating_add(1);
        id
    };
    let local_styles: BTreeMap<u64, StyleId> = doc
        .styles()
        .filter_map(|(id, s)| Some((linked(&s.origin)?, id)))
        .collect();
    for id in styles {
        let Some(style) = foreign.style(id) else {
            continue;
        };
        let local = local_styles
            .get(&id.0)
            .copied()
            .unwrap_or_else(|| StyleId(fresh()));
        let mut copy = style.clone();
        copy.origin = Some(LibraryLink {
            library: library.to_owned(),
            source: id.0,
        });
        if doc.style(local) != Some(&copy) {
            out.ops.push(Op::SetStyle {
                id: local,
                style: Some(copy),
            });
        }
        out.styles.insert(id, local);
    }
    let mut collections: BTreeMap<CollectionId, (CollectionId, Modes)> = BTreeMap::new();
    let mut created: Vec<(CollectionId, Modes)> = Vec::new();
    for v in vars.iter().filter_map(|id| foreign.variable(*id)) {
        if collections.contains_key(&v.collection) {
            continue;
        }
        let Some(theirs) = foreign.collection(v.collection) else {
            continue;
        };
        let existing = doc
            .collections()
            .map(|(id, m)| (id, m.clone()))
            .chain(created.iter().cloned())
            .find(|(_, m)| m.name == theirs.name);
        let target = match existing {
            Some(found) => found,
            None => {
                let id = CollectionId(fresh());
                let modes = Modes {
                    active: 0,
                    ..theirs.clone()
                };
                out.ops.push(Op::SetCollection {
                    id,
                    modes: Some(modes.clone()),
                });
                created.push((id, modes.clone()));
                (id, modes)
            }
        };
        collections.insert(v.collection, target);
    }
    let local_vars: BTreeMap<u64, VariableId> = doc
        .variables()
        .filter_map(|(id, v)| Some((linked(&v.origin)?, id)))
        .collect();
    for id in &vars {
        if foreign.variable(*id).is_some() {
            let local = local_vars
                .get(&id.0)
                .copied()
                .unwrap_or_else(|| VariableId(fresh()));
            out.variables.insert(*id, local);
        }
    }
    for (id, local) in &out.variables {
        let (Some(v), Some((collection, modes))) = (
            foreign.variable(*id),
            foreign
                .variable(*id)
                .and_then(|v| collections.get(&v.collection)),
        ) else {
            continue;
        };
        let theirs = foreign.collection(v.collection);
        let values: Vec<VarValue> = modes
            .names
            .iter()
            .enumerate()
            .filter_map(|(i, name)| {
                let at = theirs
                    .and_then(|t| t.names.iter().position(|n| n == name))
                    .unwrap_or(i);
                let value = v.values.get(at).or_else(|| v.values.first())?;
                Some(match value {
                    VarValue::Alias(to) => VarValue::Alias(*out.variables.get(to)?),
                    other => other.clone(),
                })
            })
            .collect();
        if values.len() != modes.names.len() {
            continue;
        }
        let copy = Variable {
            name: v.name.clone(),
            values,
            collection: *collection,
            scopes: v.scopes.clone(),
            origin: Some(LibraryLink {
                library: library.to_owned(),
                source: id.0,
            }),
        };
        if doc.variable(*local) != Some(&copy) {
            out.ops.push(Op::SetVariable {
                id: *local,
                variable: Some(copy),
            });
        }
    }
    out
}
