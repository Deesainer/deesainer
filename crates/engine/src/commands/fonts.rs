use std::collections::BTreeMap;

use deesainer_doc::{NodeKind, PropValue};
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("text.missingFonts", "Missing fonts", missing)
            .read_only()
            .doc("Font families the document's text uses (layers and styled ranges) that are not installed, with how many text layers use each. Their text draws in a fallback font until replaced. Returns [{family, layers}].")
            .params(const { &[] }),
        CommandSpec::new("text.replaceFont", "Replace font", replace)
            .doc("Replaces a font family everywhere in the document's text (layers, styled ranges and text styles) with another. Returns {layers}.")
            .params(const {
                &[
                    param("from", ParamType::String, true, "Family to replace (case does not matter)."),
                    param("to", ParamType::String, true, "Family to use instead."),
                ]
            }),
    ]
}

const GENERIC: &[&str] = &["sans-serif", "serif", "monospace", "cursive", "fantasy"];

fn installed(families: &[String], family: &str) -> bool {
    GENERIC.iter().any(|g| g.eq_ignore_ascii_case(family))
        || families.iter().any(|f| f.eq_ignore_ascii_case(family))
}

fn missing(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let families = deesainer_text::families();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for node in ctx.doc.nodes() {
        let NodeKind::Text(data) = &node.kind else {
            continue;
        };
        let mut used: Vec<&str> = vec![data.style.font_family.as_str()];
        used.extend(data.runs.iter().map(|r| r.style.font_family.as_str()));
        used.sort_unstable();
        used.dedup();
        for f in used {
            if !installed(&families, f) {
                *counts.entry(f.to_owned()).or_default() += 1;
            }
        }
    }
    let out: Vec<Value> = counts
        .into_iter()
        .map(|(family, layers)| json!({ "family": family, "layers": layers }))
        .collect();
    Ok(Outcome::read(Value::Array(out)))
}

fn replace(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let from = p.req_str("from")?.trim();
    let to = p.req_str("to")?.trim();
    if from.is_empty() || to.is_empty() || to.len() > 256 {
        return Err(p.invalid("from and to need a family name (to: at most 256 bytes)"));
    }
    let same = |f: &str| f.eq_ignore_ascii_case(from);
    let mut ops = Vec::new();
    let mut layers = 0usize;
    for node in ctx.doc.nodes() {
        let NodeKind::Text(data) = &node.kind else {
            continue;
        };
        let mut d = data.clone();
        if same(&d.style.font_family) {
            d.style.font_family = to.to_owned();
        }
        for run in &mut d.runs {
            if same(&run.style.font_family) {
                run.style.font_family = to.to_owned();
            }
        }
        if &d != data {
            layers += 1;
            ops.push(Op::set(node.id, PropValue::Kind(NodeKind::Text(d))));
        }
    }
    for (id, style) in ctx.doc.styles() {
        if let deesainer_doc::StyleKind::Text(t) = &style.kind {
            if same(&t.font_family) {
                let mut s = style.clone();
                if let deesainer_doc::StyleKind::Text(t) = &mut s.kind {
                    t.font_family = to.to_owned();
                }
                ops.push(Op::SetStyle { id, style: Some(s) });
            }
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Replace font", ops),
        json!({ "layers": layers }),
    ))
}
