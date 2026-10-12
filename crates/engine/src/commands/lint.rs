use deesainer_ops::Transaction;
use serde_json::json;

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

const RULES: &[&str] = &[
    "detachedPaint",
    "detachedText",
    "rawValue",
    "offPixelGrid",
    "hiddenLayer",
    "emptyFrame",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("lint.run", "Design checks", run)
            .read_only()
            .doc("Checks the current page (or a layer): layers matching styles or variables they do not use, values off the pixel grid, hidden layers, empty frames. Returns [{rule, id, name, message, fix?}].")
            .params(const { &[param("id", ParamType::NodeId, false, "Layer to check; default the current page.")] }),
        CommandSpec::new("lint.fix", "Fix design checks", fix)
            .doc("Applies the fixes of the current page's issues (use the style or variable, round to whole pixels), optionally only for some layers or rules, as one undo step. Returns {fixed}.")
            .params(const {
                &[
                    param("ids", ParamType::NodeIds, false, "Only these layers."),
                    param("rules", ParamType::Json, false, "Only these rules."),
                ]
            }),
    ]
}

fn run(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let root = p.node_id("id")?.unwrap_or(ctx.current_page);
    if !ctx.doc.contains(root) {
        return Err(p.bad("id", format!("{root} does not exist")));
    }
    let issues = crate::lint::issues(ctx.doc, root);
    Ok(Outcome::read(json!(issues)))
}

fn fix(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let only_ids = p.node_ids("ids")?;
    let rules: Option<Vec<String>> = p.json("rules")?;
    if let Some(r) = &rules {
        if r.iter().any(|x| !RULES.contains(&x.as_str())) {
            return Err(p.bad("rules", format!("expected some of {}", RULES.join(", "))));
        }
    }
    let mut ops = Vec::new();
    let mut fixed = 0;
    let mut work = ctx.doc.clone();
    for issue in crate::lint::issues(ctx.doc, ctx.current_page) {
        if only_ids
            .as_ref()
            .is_some_and(|ids| !ids.contains(&issue.id))
        {
            continue;
        }
        let name = serde_json::to_value(issue.rule)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        if rules.as_ref().is_some_and(|r| !r.contains(&name)) {
            continue;
        }
        let more = crate::lint::fix_ops(&work, &issue);
        if !more.is_empty() {
            fixed += 1;
            for op in &more {
                let _ = op.clone().apply(&mut work);
            }
            ops.extend(more);
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Fix design checks", ops),
        json!({ "fixed": fixed }),
    ))
}
