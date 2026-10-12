use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use deesainer_doc::{Document, LibraryRef, Node, NodeId, NodeKind, Override, PropValue};
use deesainer_geom::Rect;
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::library_assets::carry;
use super::{clone_subtrees_mapped, collect_subtrees, param};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
const MAX_NAME: usize = 80;
const GAP: f64 = 40.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("library.import", "Import library", import)
            .doc("Copies main components (with the layers they need) from another .deesainer file into a page named \"Library: NAME\" and links them to it. Components already imported are skipped; use library.update to refresh them. The styles and variables they use come along, linked to the library (collections are matched by name); prototype links do not. Returns {page, imported, skipped, components}.")
            .params(const {
                &[
                    param("data", ParamType::String, true, "Base64 .deesainer bytes of the library file."),
                    param("library", ParamType::String, true, "Name that identifies the library in this document."),
                    param("components", ParamType::Json, false, "Ids (in the library file) of the components or component sets to import; default all."),
                ]
            }),
        CommandSpec::new("library.update", "Update library", update)
            .doc("Replaces the content of every component linked to the library with the version in the given file, and refreshes the styles and variables shared from it. Instances keep following their main component; their overrides are re-targeted by layer name. Returns {updated, missing, available}.")
            .params(const {
                &[
                    param("data", ParamType::String, true, "Base64 .deesainer bytes of the library file."),
                    param("library", ParamType::String, true, "Name given at import."),
                    param("components", ParamType::Json, false, "Only these components (their ids in the library file, as library.changes lists them); default all."),
                ]
            }),
        CommandSpec::new("library.changes", "Review library updates", changes)
            .read_only()
            .doc("What updating from a library file would change, before doing it: per linked component {source, id, name, status: changed|unchanged|removed, added, removed, changed (layers), instances}, the components the file has that were not imported, and the file's last published note. Returns {components, available, published}.")
            .params(const {
                &[
                    param("data", ParamType::String, true, "Base64 .deesainer bytes of the library file."),
                    param("library", ParamType::String, true, "Name given at import."),
                ]
            }),
        CommandSpec::new("library.publish", "Publish library", publish)
            .doc("Marks this file's components as published, with a note of what changed: a version named \"Published: NOTE\" in the file's history, which library.changes shows to the files that use it. Returns {id}.")
            .params(const { &[param("note", ParamType::String, false, "What changed.")] }),
        CommandSpec::new("library.list", "List libraries", list)
            .read_only()
            .doc("Lists the libraries this document imported from, with the number of linked components."),
    ]
}

fn is_comp(n: &Node) -> bool {
    matches!(n.kind, NodeKind::Component { .. } | NodeKind::ComponentSet)
}

fn library_name(p: &Params<'_>) -> Result<String, EngineError> {
    let name = p.req_str("library")?.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(p.bad("library", format!("needs 1 to {MAX_NAME} characters")));
    }
    Ok(name.to_owned())
}

fn decode(p: &Params<'_>) -> Result<Document, EngineError> {
    let bytes = B64
        .decode(p.req_str("data")?.trim())
        .map_err(|e| p.bad("data", format!("is not valid base64: {e}")))?;
    deesainer_format::read(&bytes).map_err(|e| p.bad("data", e.to_string()))
}

fn has_comp_ancestor(doc: &Document, id: NodeId) -> bool {
    let mut cur = doc.get(id).and_then(|n| n.parent);
    for _ in 0..deesainer_doc::MAX_DEPTH {
        let Some(n) = cur.and_then(|c| doc.get(c)) else {
            return false;
        };
        if is_comp(n) {
            return true;
        }
        cur = n.parent;
    }
    false
}

fn top_level_components(doc: &Document) -> Vec<NodeId> {
    doc.nodes()
        .filter(|n| is_comp(n) && !has_comp_ancestor(doc, n.id))
        .map(|n| n.id)
        .collect()
}

fn set_root(doc: &Document, id: NodeId) -> NodeId {
    doc.get(id)
        .and_then(|n| n.parent)
        .and_then(|p| doc.get(p))
        .filter(|p| matches!(p.kind, NodeKind::ComponentSet))
        .map_or(id, |p| p.id)
}

fn linked(doc: &Document, library: &str) -> BTreeMap<NodeId, NodeId> {
    doc.nodes()
        .filter(|n| is_comp(n))
        .filter_map(|n| {
            n.origin
                .as_ref()
                .filter(|o| o.library == library)
                .map(|o| (o.source, n.id))
        })
        .collect()
}

fn import(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let library = library_name(p)?;
    let foreign = decode(p)?;
    let existing = linked(ctx.doc, &library);
    let requested: Vec<NodeId> = match p.json::<Vec<u64>>("components")? {
        Some(ids) => ids.into_iter().map(NodeId).collect(),
        None => top_level_components(&foreign),
    };
    let mut roots: Vec<NodeId> = Vec::new();
    for id in requested {
        if !foreign.get(id).is_some_and(is_comp) {
            return Err(p.bad(
                "components",
                format!("{id} is not a component or component set in the library"),
            ));
        }
        let root = set_root(&foreign, id);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    let mut covered: BTreeSet<NodeId> = BTreeSet::new();
    let mut i = 0;
    while let Some(&root) = roots.get(i) {
        i += 1;
        let tree = foreign.subtree_pre_order(root).unwrap_or_default();
        covered.extend(tree.iter().copied());
        for id in tree {
            let Some(NodeKind::Instance { component, .. }) = foreign.get(id).map(|n| &n.kind)
            else {
                continue;
            };
            let dep = set_root(&foreign, *component);
            if foreign.get(dep).is_some()
                && !existing.contains_key(component)
                && !covered.contains(component)
                && !roots.contains(&dep)
            {
                roots.push(dep);
            }
        }
        if roots.len() > 1000 {
            return Err(p.invalid("the library has too many components"));
        }
    }
    let copy: Vec<NodeId> = roots
        .iter()
        .copied()
        .filter(|r| !existing.contains_key(r))
        .collect();
    let skipped = roots.len() - copy.len();
    if copy.is_empty() {
        return Ok(Outcome::read(
            json!({ "imported": 0, "skipped": skipped, "components": [] }),
        ));
    }
    let nodes = collect_subtrees(&foreign, &copy)?;
    let mut next = ctx
        .doc
        .next_free_id()
        .0
        .saturating_add(nodes.len() as u64 + 1);
    let carried = carry(ctx.doc, &foreign, &library, nodes.values(), &mut next);
    let page_name = format!("Library: {library}");
    let page = ctx
        .doc
        .pages()
        .iter()
        .copied()
        .find(|id| ctx.doc.get(*id).is_some_and(|n| n.name == page_name));
    let (page_id, index) = match page {
        Some(id) => (id, ctx.doc.get(id).map_or(0, |n| n.children.len())),
        None => (ctx.doc.fresh_ids(nodes.len() + 1)[nodes.len()], 0),
    };
    let (mut clones, new_roots, map) =
        clone_subtrees_mapped(ctx.doc, &nodes, &copy, page_id, index, (0.0, 0.0))?;
    let reverse: BTreeMap<NodeId, NodeId> = map.iter().map(|(old, new)| (*new, *old)).collect();
    let mut cursor = page.and_then(|id| ctx.doc.get(id)).map_or(0.0, |pg| {
        pg.children
            .iter()
            .filter_map(|c| ctx.doc.get(*c))
            .map(|c| c.rect.y + c.rect.height + GAP)
            .fold(0.0, f64::max)
    });
    for op in &mut clones {
        let Op::CreateNode { node, .. } = op else {
            continue;
        };
        carried.relink(node);
        node.interactions.clear();
        if is_comp(node) {
            node.origin = reverse.get(&node.id).map(|source| LibraryRef {
                library: library.clone(),
                source: *source,
            });
        }
        if new_roots.contains(&node.id) {
            node.rect.x = 0.0;
            node.rect.y = cursor;
            cursor += node.rect.height + GAP;
        }
        if matches!(node.kind, NodeKind::Instance { .. }) {
            let source = reverse.get(&node.id).and_then(|old| nodes.get(old));
            if let Some(NodeKind::Instance { component, .. }) = source.map(|s| &s.kind) {
                if !map.contains_key(component) {
                    node.kind = match existing.get(component) {
                        Some(local) => NodeKind::Instance {
                            component: *local,
                            overrides: Vec::new(),
                        },
                        None => NodeKind::Frame,
                    };
                }
            }
        }
    }
    let mut ops: Vec<Op> = foreign
        .assets()
        .filter(|(id, _)| ctx.doc.asset(id).is_none())
        .map(|(id, data)| Op::SetAsset {
            id: id.clone(),
            data: Some(data.clone()),
        })
        .collect();
    if page.is_none() {
        let mut node = Node::new(page_id, page_name, NodeKind::Page, Rect::default());
        if let Some(first) = ctx.doc.get(ctx.doc.first_page()) {
            node.fills = first.fills.clone();
        }
        ops.push(Op::CreateNode {
            parent: ctx.doc.root(),
            index: ctx.doc.pages().len(),
            node: Box::new(node),
        });
    }
    let (styles, variables) = (carried.styles.len(), carried.variables.len());
    ops.extend(carried.ops);
    ops.extend(clones);
    let components: Vec<Value> = copy
        .iter()
        .filter_map(|old| Some(json!({ "source": old, "id": map.get(old)? })))
        .collect();
    Ok(Outcome::apply(
        Transaction::new("Import library", ops),
        json!({
            "page": page_id,
            "imported": copy.len(),
            "skipped": skipped,
            "components": components,
            "styles": styles,
            "variables": variables,
        }),
    )
    .on_page(page_id)
    .selecting(new_roots))
}

fn update(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let library = library_name(p)?;
    let foreign = decode(p)?;
    let links = linked(ctx.doc, &library);
    if links.is_empty() {
        return Err(p.bad("library", "nothing was imported under this name"));
    }
    let mut ops: Vec<Op> = Vec::new();
    let mut updated = 0usize;
    let mut missing: Vec<String> = Vec::new();
    let mut ids = ctx.doc.clone();
    let only: Option<Vec<NodeId>> = p
        .json::<Vec<u64>>("components")?
        .map(|ids| ids.into_iter().map(NodeId).collect());
    let sources: Vec<NodeId> = links
        .keys()
        .copied()
        .filter(|s| only.as_ref().is_none_or(|o| o.contains(s)))
        .filter(|s| {
            foreign
                .get(*s)
                .is_some_and(|n| matches!(n.kind, NodeKind::Component { .. }))
        })
        .collect();
    let all = collect_subtrees(&foreign, &sources)?;
    let mut next = ctx.doc.next_free_id().0;
    let carried = carry(ctx.doc, &foreign, &library, all.values(), &mut next);
    ids.reserve_ids(next);
    ops.extend(carried.ops.iter().cloned());
    for (source, local_id) in &links {
        if only.as_ref().is_some_and(|o| !o.contains(source)) {
            continue;
        }
        let Some(local) = ctx.doc.get(*local_id) else {
            continue;
        };
        if !matches!(local.kind, NodeKind::Component { .. }) {
            continue;
        }
        let Some(src) = foreign
            .get(*source)
            .filter(|n| matches!(n.kind, NodeKind::Component { .. }))
        else {
            missing.push(local.name.clone());
            continue;
        };
        let nodes = collect_subtrees(&foreign, &[src.id])?;
        let (mut clones, roots, map) =
            clone_subtrees_mapped(&ids, &nodes, &[src.id], *local_id, 0, (0.0, 0.0))?;
        let past = map.values().map(|id| id.0).max().unwrap_or(0);
        ids.reserve_ids(past.saturating_add(1));
        let nested = roots
            .first()
            .copied()
            .ok_or_else(|| p.invalid("empty copy"))?;
        let mut head: Option<Node> = None;
        let mut fresh: Vec<(NodeId, String)> = Vec::new();
        let mut kept: Vec<Op> = Vec::new();
        for mut op in clones.drain(..) {
            if let Op::CreateNode { parent, node, .. } = &mut op {
                carried.relink(node);
                node.interactions.clear();
                if node.id == nested {
                    head = Some((**node).clone());
                    continue;
                }
                if *parent == nested {
                    *parent = *local_id;
                }
                fresh.push((node.id, node.name.clone()));
            }
            kept.push(op);
        }
        let Some(head) = head else { continue };
        let old: Vec<NodeId> = ctx.doc.subtree_pre_order(*local_id).unwrap_or_default();
        for id in old.iter().skip(1).rev() {
            ops.push(Op::DeleteNode { id: *id });
        }
        ops.extend(kept);
        ops.push(Op::set(*local_id, PropValue::Name(head.name.clone())));
        ops.push(Op::set(*local_id, PropValue::Kind(head.kind.clone())));
        ops.push(Op::set(
            *local_id,
            PropValue::Rect(Rect::new(
                local.rect.x,
                local.rect.y,
                head.rect.width,
                head.rect.height,
            )),
        ));
        ops.push(Op::set(*local_id, PropValue::Fills(head.fills.clone())));
        ops.push(Op::set(*local_id, PropValue::Strokes(head.strokes.clone())));
        ops.push(Op::set(*local_id, PropValue::Stroke(head.stroke)));
        ops.push(Op::set(*local_id, PropValue::Effects(head.effects.clone())));
        ops.push(Op::set(
            *local_id,
            PropValue::CornerRadius(head.corner_radius),
        ));
        ops.push(Op::set(*local_id, PropValue::Layout(head.layout)));
        ops.push(Op::set(
            *local_id,
            PropValue::ClipsContent(head.clips_content),
        ));
        for inst in ctx.doc.nodes() {
            let NodeKind::Instance {
                component,
                overrides,
            } = &inst.kind
            else {
                continue;
            };
            if component != local_id || overrides.is_empty() {
                continue;
            }
            let retargeted: Vec<Override> = overrides
                .iter()
                .filter_map(|o| {
                    if o.target == *local_id {
                        return Some(o.clone());
                    }
                    let name = &ctx.doc.get(o.target)?.name;
                    let target = fresh.iter().find(|(_, n)| n == name)?.0;
                    Some(Override {
                        target,
                        value: o.value.clone(),
                    })
                })
                .collect();
            ops.push(Op::set(
                inst.id,
                PropValue::Kind(NodeKind::Instance {
                    component: *local_id,
                    overrides: retargeted,
                }),
            ));
        }
        let _ = map;
        updated += 1;
    }
    let available = top_level_components(&foreign)
        .into_iter()
        .filter(|id| !links.contains_key(id) && !links.contains_key(&set_root(&foreign, *id)))
        .count();
    Ok(Outcome::apply(
        Transaction::new("Update library", ops),
        json!({ "updated": updated, "missing": missing, "available": available }),
    ))
}

fn list(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for n in ctx.doc.nodes() {
        if let (Some(o), NodeKind::Component { .. }) = (&n.origin, &n.kind) {
            *counts.entry(o.library.clone()).or_default() += 1;
        }
    }
    let out: Vec<Value> = counts
        .into_iter()
        .map(|(name, components)| json!({ "library": name, "components": components }))
        .collect();
    Ok(Outcome::read(Value::Array(out)))
}

fn fingerprint(doc: &Document, root: NodeId) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Ok(ids) = doc.subtree_pre_order(root) else {
        return out;
    };
    for id in ids {
        let Some(n) = doc.get(id) else { continue };
        let mut path = Vec::new();
        let mut at = Some(id);
        while let Some(cur) = at.filter(|c| *c != root) {
            path.push(doc.get(cur).map(|x| x.name.clone()).unwrap_or_default());
            at = doc.get(cur).and_then(|x| x.parent);
        }
        path.reverse();
        let mut v = serde_json::to_value(n).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            for k in [
                "id",
                "parent",
                "children",
                "styles",
                "bindings",
                "interactions",
                "origin",
            ] {
                o.remove(k);
            }
            if id == root {
                if let Some(r) = o.get_mut("rect").and_then(Value::as_object_mut) {
                    r.remove("x");
                    r.remove("y");
                }
            }
        }
        let mut key = path.join("/");
        let mut i = 2;
        while out.contains_key(&key) {
            key = format!("{}#{i}", path.join("/"));
            i += 1;
        }
        out.insert(key, v);
    }
    out
}

fn changes(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let library = library_name(p)?;
    let foreign = decode_bundle(p)?;
    let (theirs, versions) = (&foreign.0, &foreign.1);
    let links = linked(ctx.doc, &library);
    if links.is_empty() {
        return Err(p.bad("library", "nothing was imported under this name"));
    }
    let mut list = Vec::new();
    for (source, local_id) in &links {
        let Some(local) = ctx.doc.get(*local_id) else {
            continue;
        };
        if !matches!(local.kind, NodeKind::Component { .. }) {
            continue;
        }
        let instances = ctx
            .doc
            .nodes()
            .filter(|n| matches!(&n.kind, NodeKind::Instance { component, .. } if component == local_id))
            .count();
        let src = theirs
            .get(*source)
            .filter(|n| matches!(n.kind, NodeKind::Component { .. }));
        let Some(src) = src else {
            list.push(
                json!({ "source": source, "id": local_id, "name": local.name,
                "status": "removed", "instances": instances }),
            );
            continue;
        };
        let (a, b) = (fingerprint(ctx.doc, *local_id), fingerprint(theirs, src.id));
        let added = b.keys().filter(|k| !a.contains_key(*k)).count();
        let removed = a.keys().filter(|k| !b.contains_key(*k)).count();
        let changed = a
            .iter()
            .filter(|(k, v)| b.get(*k).is_some_and(|w| w != *v))
            .count();
        let status = if added + removed + changed == 0 {
            "unchanged"
        } else {
            "changed"
        };
        list.push(
            json!({ "source": source, "id": local_id, "name": src.name, "status": status,
            "added": added, "removed": removed, "changed": changed, "instances": instances }),
        );
    }
    let available: Vec<Value> = top_level_components(theirs)
        .into_iter()
        .filter(|id| !links.contains_key(id) && !links.contains_key(&set_root(theirs, *id)))
        .map(|id| json!({ "source": id, "name": theirs.get(id).map(|n| n.name.clone()).unwrap_or_default() }))
        .collect();
    let published = versions
        .iter()
        .rev()
        .find(|v| v.label.starts_with("Published"))
        .map(|v| json!({ "label": v.label, "savedAt": v.saved_at, "author": v.author }));
    Ok(Outcome::read(
        json!({ "components": list, "available": available, "published": published }),
    ))
}

fn decode_bundle(
    p: &Params<'_>,
) -> Result<(Document, Vec<deesainer_format::Version>), EngineError> {
    let bytes = B64
        .decode(p.req_str("data")?.trim())
        .map_err(|e| p.bad("data", format!("is not valid base64: {e}")))?;
    let (doc, _, versions) =
        deesainer_format::read_bundle(&bytes).map_err(|e| p.bad("data", e.to_string()))?;
    Ok((doc, versions))
}

fn publish(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    if top_level_components(ctx.doc).is_empty() {
        return Err(p.invalid("this file has no components to publish"));
    }
    let note = p.str("note")?.unwrap_or("").trim();
    if note.chars().count() > 200 {
        return Err(p.bad("note", "at most 200 characters"));
    }
    let label = if note.is_empty() {
        "Published".to_owned()
    } else {
        format!("Published: {note}")
    };
    Ok(Outcome::effect(crate::registry::Effect::SaveVersion {
        label,
    }))
}
