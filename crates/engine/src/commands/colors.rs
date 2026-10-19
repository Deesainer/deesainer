use std::collections::{BTreeMap, BTreeSet};

use deesainer_color::Rgba;
use deesainer_doc::{Effect, Node, NodeId, NodeKind, Paint, PaintKind, PropValue};
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamSpec, ParamType};

const SCOPE: ParamSpec = param(
    "ids",
    ParamType::NodeIds,
    false,
    "Layers or pages whose whole subtrees are scanned; default the selection, or the current page (including its background) when nothing is selected.",
);

const MAX_RULES: usize = 256;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("color.list", "Selection colors", list)
            .read_only()
            .doc("Every colour the scanned layers use: fills (a page's fill is its background), strokes, gradient stops, shadow colours and styled text ranges. Properties linked to a shared style are left out (they follow the style). Returns [{color, uses, fill, stroke, effect, text, layers}], most used first; colours are #RRGGBB, or #RRGGBBAA when translucent; layers holds the ids of up to 20 layers using the colour, for fixing a few directly.")
            .params(const { &[SCOPE] }),
        CommandSpec::new("color.replace", "Replace colors", replace)
            .doc("Replaces colours throughout the scanned layers as one undo step, e.g. to turn a light design dark. map applies to every use; fill, stroke, effect and text apply only to that kind of use (text = text layers and styled ranges) and win over map, so white cards can turn dark while white text stays white. A 6-digit key matches that colour at any opacity and keeps the opacity unless the new colour has 8 digits; an 8-digit key matches exactly. Each colour is replaced at most once, so swaps work. Properties linked to a shared style are left to the style. Returns {layers, changes}.")
            .params(const {
                &[
                    SCOPE,
                    param("map", ParamType::Json, false, "Old to new colour for every kind of use, both hex: {\"#F8FAFC\": \"#0F172A\"}."),
                    param("fill", ParamType::Json, false, "Old to new colour for shape and frame fills only (page backgrounds included): {\"#FFFFFF\": \"#1E293B\"}."),
                    param("stroke", ParamType::Json, false, "Old to new colour for strokes only: {\"#E2E8F0\": \"#334155\"}."),
                    param("effect", ParamType::Json, false, "Old to new colour for shadow colours only."),
                    param("text", ParamType::Json, false, "Old to new colour for text only: {\"#0F172A\": \"#F8FAFC\"}."),
                ]
            }),
    ]
}

#[derive(Clone, Copy)]
enum Slot {
    Fill = 0,
    Stroke = 1,
    Effect = 2,
    Text = 3,
}

fn paints(list: &mut [Paint], slot: Slot, f: &mut dyn FnMut(Slot, &mut Rgba)) {
    for paint in list {
        match &mut paint.kind {
            PaintKind::Solid { color } => f(slot, color),
            PaintKind::LinearGradient { stops, .. }
            | PaintKind::RadialGradient { stops, .. }
            | PaintKind::AngularGradient { stops, .. }
            | PaintKind::DiamondGradient { stops, .. } => {
                for stop in stops {
                    f(slot, &mut stop.color);
                }
            }
            PaintKind::Image { .. } => {}
        }
    }
}

fn visit(node: &Node, f: &mut dyn FnMut(Slot, &mut Rgba)) -> Vec<Op> {
    let mut ops = Vec::new();
    let is_text = matches!(node.kind, NodeKind::Text(_));
    if node.styles.fill.is_none() {
        let mut fills = node.fills.clone();
        paints(&mut fills, if is_text { Slot::Text } else { Slot::Fill }, f);
        if fills != node.fills {
            ops.push(Op::set(node.id, PropValue::Fills(fills)));
        }
    }
    if node.styles.stroke.is_none() {
        let mut strokes = node.strokes.clone();
        paints(&mut strokes, Slot::Stroke, f);
        if strokes != node.strokes {
            ops.push(Op::set(node.id, PropValue::Strokes(strokes)));
        }
    }
    if node.styles.effect.is_none() {
        let mut effects = node.effects.clone();
        for effect in &mut effects {
            if let Effect::DropShadow { color, .. } | Effect::InnerShadow { color, .. } = effect {
                f(Slot::Effect, color);
            }
        }
        if effects != node.effects {
            ops.push(Op::set(node.id, PropValue::Effects(effects)));
        }
    }
    if let NodeKind::Text(data) = &node.kind {
        let mut d = data.clone();
        if node.styles.text.is_none() {
            if let Some(p) = &mut d.style.fill {
                paints(std::slice::from_mut(p), Slot::Text, f);
            }
        }
        for run in &mut d.runs {
            if let Some(p) = &mut run.style.fill {
                paints(std::slice::from_mut(p), Slot::Text, f);
            }
        }
        if &d != data {
            ops.push(Op::set(node.id, PropValue::Kind(NodeKind::Text(d))));
        }
    }
    ops
}

fn scope(ctx: &Context<'_>, p: &Params<'_>) -> Result<Vec<NodeId>, EngineError> {
    let roots = match p.node_ids("ids")? {
        Some(ids) => ids,
        None if !ctx.selection.is_empty() => ctx.selection.to_vec(),
        None => vec![ctx.current_page],
    };
    let mut all = BTreeSet::new();
    for root in roots {
        all.extend(ctx.doc.subtree_pre_order(root)?);
    }
    Ok(all.into_iter().collect())
}

const MAX_LAYER_IDS: usize = 20;

#[derive(Default)]
struct Usage {
    slots: [usize; 4],
    layers: Vec<NodeId>,
}

fn list(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let mut counts: BTreeMap<String, Usage> = BTreeMap::new();
    for id in scope(ctx, p)? {
        let node = ctx.doc.node(id)?;
        visit(node, &mut |slot, c| {
            let u = counts.entry(c.to_hex()).or_default();
            u.slots[slot as usize] += 1;
            if u.layers.last() != Some(&id) && u.layers.len() < MAX_LAYER_IDS {
                u.layers.push(id);
            }
        });
    }
    let mut out: Vec<(String, Usage)> = counts.into_iter().collect();
    out.sort_by_key(|(_, u)| std::cmp::Reverse(u.slots.iter().sum::<usize>()));
    let out: Vec<Value> = out
        .into_iter()
        .map(
            |(
                color,
                Usage {
                    slots: [fill, stroke, effect, text],
                    layers,
                },
            )| {
                json!({
                    "color": color,
                    "uses": fill + stroke + effect + text,
                    "fill": fill,
                    "stroke": stroke,
                    "effect": effect,
                    "text": text,
                    "layers": layers,
                })
            },
        )
        .collect();
    Ok(Outcome::read(Value::Array(out)))
}

struct Rule {
    from: [u8; 4],
    exact: bool,
    to: Rgba,
    to_alpha: bool,
}

fn has_alpha(hex: &str) -> bool {
    hex.trim().trim_start_matches('#').len() == 8
}

const MAPS: [&str; 5] = ["map", "fill", "stroke", "effect", "text"];

struct Rules {
    general: Vec<Rule>,
    slots: [Vec<Rule>; 4],
}

impl Rules {
    fn find(&self, slot: Slot, q: [u8; 4]) -> Option<&Rule> {
        let hit = |r: &&Rule| {
            if r.exact {
                q == r.from
            } else {
                q[..3] == r.from[..3]
            }
        };
        self.slots
            .get(slot as usize)
            .and_then(|own| own.iter().find(hit))
            .or_else(|| self.general.iter().find(hit))
    }
}

fn rule_list(p: &Params<'_>, key: &str) -> Result<Vec<Rule>, EngineError> {
    let Some(map) = p.json::<BTreeMap<String, String>>(key)? else {
        return Ok(Vec::new());
    };
    let parse = |hex: &str| {
        Rgba::from_hex(hex.trim())
            .map_err(|_| p.bad(key, format!("{hex:?} is not a hex colour like \"#1A2B3C\"")))
    };
    map.iter()
        .map(|(from, to)| {
            Ok(Rule {
                from: parse(from)?.to_rgba8(),
                exact: has_alpha(from),
                to: parse(to)?,
                to_alpha: has_alpha(to),
            })
        })
        .collect()
}

fn rules(p: &Params<'_>) -> Result<Rules, EngineError> {
    let [general, fill, stroke, effect, text] = MAPS.map(|k| rule_list(p, k));
    let rules = Rules {
        general: general?,
        slots: [fill?, stroke?, effect?, text?],
    };
    let total = rules.general.len() + rules.slots.iter().map(Vec::len).sum::<usize>();
    if total == 0 || total > MAX_RULES {
        return Err(p.invalid(format!(
            "give 1 to {MAX_RULES} colours in map, fill, stroke, effect or text"
        )));
    }
    Ok(rules)
}

fn replace(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let rules = rules(p)?;
    let mut ops = Vec::new();
    let mut layers = 0usize;
    let mut changes = 0usize;
    for id in scope(ctx, p)? {
        let node = ctx.doc.node(id)?;
        let node_ops = visit(node, &mut |slot, c| {
            let hit = rules.find(slot, c.to_rgba8());
            if let Some(r) = hit {
                let next = r.to.with_alpha(if r.to_alpha { r.to.a } else { c.a });
                if next != *c {
                    *c = next;
                    changes += 1;
                }
            }
        });
        if !node_ops.is_empty() {
            layers += 1;
            ops.extend(node_ops);
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Replace colors", ops),
        json!({ "layers": layers, "changes": changes }),
    ))
}
