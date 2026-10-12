use deesainer_doc::{
    BindTarget, Document, NodeId, NodeKind, PaintKind, PropValue, StyleId, StyleKind, VarValue,
    VariableId,
};
use deesainer_ops::Op;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Rule {
    DetachedPaint,
    DetachedText,
    RawValue,
    OffPixelGrid,
    HiddenLayer,
    EmptyFrame,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Fix {
    Style {
        target: &'static str,
        style: StyleId,
    },
    Variable {
        target: BindTarget,
        variable: VariableId,
    },
    Round,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub rule: Rule,
    pub id: NodeId,
    pub name: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

fn off_grid(v: f64) -> bool {
    (v - v.round()).abs() > 0.01
}

pub fn issues(doc: &Document, root: NodeId) -> Vec<Issue> {
    let mut out = Vec::new();
    let Ok(ids) = doc.subtree_pre_order(root) else {
        return out;
    };
    let styles: Vec<(StyleId, &StyleKind, &str)> = doc
        .styles()
        .map(|(id, s)| (id, &s.kind, s.name.as_str()))
        .collect();
    let vars: Vec<(VariableId, &str, Option<&VarValue>)> = doc
        .variables()
        .map(|(id, v)| (id, v.name.as_str(), doc.resolve_variable(id)))
        .collect();
    for id in ids.into_iter().filter(|i| *i != root) {
        let Some(n) = doc.get(id) else { continue };
        let mut push = |rule, message: String, fix| {
            out.push(Issue {
                rule,
                id,
                name: n.name.clone(),
                message,
                fix,
            })
        };
        if !n.visible {
            push(Rule::HiddenLayer, "Hidden layer".into(), None);
        }
        if matches!(n.kind, NodeKind::Frame)
            && n.children.is_empty()
            && n.fills.is_empty()
            && n.strokes.is_empty()
        {
            push(
                Rule::EmptyFrame,
                "Empty frame with nothing to show".into(),
                None,
            );
        }
        for (target, paints, linked) in [
            ("fill", &n.fills, n.styles.fill),
            ("stroke", &n.strokes, n.styles.stroke),
        ] {
            if linked.is_some() || paints.is_empty() {
                continue;
            }
            let bound = n.bindings.contains_key(if target == "fill" {
                &BindTarget::FillColor
            } else {
                &BindTarget::StrokeColor
            });
            if bound {
                continue;
            }
            if let Some((sid, _, name)) = styles
                .iter()
                .find(|(_, k, _)| matches!(k, StyleKind::Paint(p) if p == paints))
            {
                push(
                    Rule::DetachedPaint,
                    format!("The {target} matches the style \"{name}\""),
                    Some(Fix::Style {
                        target,
                        style: *sid,
                    }),
                );
                continue;
            }
            if let [p] = paints.as_slice() {
                if let PaintKind::Solid { color } = p.kind {
                    if let Some((vid, name, _)) = vars
                        .iter()
                        .find(|(_, _, v)| matches!(v, Some(VarValue::Color(c)) if *c == color))
                    {
                        let t = if target == "fill" {
                            BindTarget::FillColor
                        } else {
                            BindTarget::StrokeColor
                        };
                        push(
                            Rule::RawValue,
                            format!("The {target} colour is the variable \"{name}\""),
                            Some(Fix::Variable {
                                target: t,
                                variable: *vid,
                            }),
                        );
                    }
                }
            }
        }
        if let NodeKind::Text(data) = &n.kind {
            if n.styles.text.is_none() && data.runs.is_empty() {
                if let Some((sid, _, name)) = styles
                    .iter()
                    .find(|(_, k, _)| matches!(k, StyleKind::Text(t) if *t == data.style))
                {
                    push(
                        Rule::DetachedText,
                        format!("The text matches the style \"{name}\""),
                        Some(Fix::Style {
                            target: "text",
                            style: *sid,
                        }),
                    );
                }
            }
        }
        let r = n.corner_radius;
        let mut numbers = Vec::new();
        if r.top_left > 0.0
            && [r.top_right, r.bottom_right, r.bottom_left]
                .iter()
                .all(|v| *v == r.top_left)
        {
            numbers.push((BindTarget::CornerRadius, "corner radius", r.top_left));
        }
        if let Some(l) = n.layout {
            if l.gap > 0.0 {
                numbers.push((BindTarget::Gap, "gap", l.gap));
            }
            let p = l.padding;
            if p.top > 0.0 && [p.right, p.bottom, p.left].iter().all(|v| *v == p.top) {
                numbers.push((BindTarget::Padding, "padding", p.top));
            }
        }
        for (target, what, value) in numbers {
            if n.bindings.contains_key(&target) {
                continue;
            }
            if let Some((vid, name, _)) = vars.iter().find(
                |(_, _, v)| matches!(v, Some(VarValue::Number(x)) if (*x - value).abs() < 1e-9),
            ) {
                push(
                    Rule::RawValue,
                    format!("The {what} is the variable \"{name}\""),
                    Some(Fix::Variable {
                        target,
                        variable: *vid,
                    }),
                );
            }
        }
        let laid_out = n
            .parent
            .and_then(|p| doc.get(p))
            .is_some_and(|p| p.layout.is_some() && !n.layout_child.absolute);
        let rect = n.rect;
        if !laid_out
            && n.rotation == 0.0
            && !matches!(
                n.kind,
                NodeKind::Page | NodeKind::Document | NodeKind::Text(_)
            )
            && [rect.x, rect.y, rect.width, rect.height]
                .into_iter()
                .any(off_grid)
        {
            push(
                Rule::OffPixelGrid,
                format!(
                    "Not on whole pixels ({} × {} at {}, {})",
                    trim(rect.width),
                    trim(rect.height),
                    trim(rect.x),
                    trim(rect.y)
                ),
                Some(Fix::Round),
            );
        }
    }
    out
}

fn trim(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    format!("{r}")
}

pub fn fix_ops(doc: &Document, issue: &Issue) -> Vec<Op> {
    let Some(n) = doc.get(issue.id) else {
        return Vec::new();
    };
    match &issue.fix {
        Some(Fix::Style { target, style }) => {
            let mut refs = n.styles;
            match *target {
                "fill" => refs.fill = Some(*style),
                "stroke" => refs.stroke = Some(*style),
                _ => refs.text = Some(*style),
            }
            vec![Op::set(n.id, PropValue::Styles(refs))]
        }
        Some(Fix::Variable { target, variable }) => {
            let mut b = n.bindings.clone();
            b.insert(*target, *variable);
            vec![Op::set(n.id, PropValue::Bindings(b))]
        }
        Some(Fix::Round) => {
            let r = n.rect;
            let x0 = r.x.round();
            let y0 = r.y.round();
            let w = (r.x + r.width).round() - x0;
            let h = (r.y + r.height).round() - y0;
            vec![Op::set(
                n.id,
                PropValue::Rect(deesainer_geom::Rect::new(x0, y0, w.max(1.0), h.max(1.0))),
            )]
        }
        None => Vec::new(),
    }
}
