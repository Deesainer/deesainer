use deesainer_doc::{
    Action, Direction, Easing, Interaction, NodeId, NodeKind, PropValue, Transition,
    TransitionKind, Trigger,
};
use deesainer_geom::Point;
use deesainer_ops::{Op, Transaction};
use deesainer_tools::proto::{Event, Player};
use serde_json::{json, Value};

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("prototype.addInteraction", "Add interaction", add)
            .doc("Adds an interaction to a layer: when the trigger happens, the action runs with a transition. Returns {index}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer that reacts."),
                    param("trigger", ParamType::Enum(&["click", "hover", "press", "delay"]), true, "What starts it."),
                    param("delay", ParamType::Integer, false, "Milliseconds, for the delay trigger."),
                    param("action", ParamType::Enum(&["navigate", "overlay", "closeOverlay", "back", "url", "setVariable", "conditional"]), true, "What happens."),
                    param("variable", ParamType::Integer, false, "setVariable: the variable."),
                    param("value", ParamType::Json, false, "setVariable: its new value (same type as the variable)."),
                    param("condition", ParamType::Json, false, "conditional: {variable, op: equals|notEquals|greater|less, value}."),
                    param("then", ParamType::Json, false, "conditional: actions when it holds, e.g. [{type: navigate, to: 12}]."),
                    param("else", ParamType::Json, false, "conditional: actions when it does not."),
                    param("to", ParamType::NodeId, false, "Target frame for navigate and overlay."),
                    param("url", ParamType::String, false, "Link for the url action."),
                    param("transition", ParamType::Enum(&["instant", "dissolve", "smartAnimate", "moveIn", "push"]), false, "Default instant."),
                    param("direction", ParamType::Enum(&["left", "right", "up", "down"]), false, "For moveIn and push, default left."),
                    param("duration", ParamType::Integer, false, "Milliseconds, default 300."),
                    param("easing", ParamType::Enum(&["linear", "easeIn", "easeOut", "easeInOut"]), false, "Default easeOut."),
                ]
            }),
        CommandSpec::new("prototype.removeInteraction", "Remove interaction", remove)
            .doc("Removes one interaction (by index) or all of them from a layer.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer."),
                    param("index", ParamType::Integer, false, "Which one; omit to remove all."),
                ]
            }),
        CommandSpec::new("prototype.setScrolling", "Scrolling", set_scrolling)
            .doc("Prototype scrolling: a frame's overflow (its content scrolls when larger than the frame) and whether layers keep their position while their frame scrolls.")
            .params(const {
                &[
                    super::IDS,
                    param("overflow", ParamType::Enum(&["none", "vertical", "horizontal", "both"]), false, "Frames: which way the content scrolls."),
                    param("fixed", ParamType::Bool, false, "Fixed position when the parent frame scrolls."),
                ]
            }),
        CommandSpec::new("prototype.setDevice", "Device", set_device)
            .doc("Sets the device a page's prototype is presented in: none, phone, tablet or browser.")
            .params(const {
                &[
                    param("device", ParamType::Enum(&["none", "phone", "tablet", "browser"]), true, "Device."),
                    param("page", ParamType::NodeId, false, "Page; default the current page."),
                ]
            }),
        CommandSpec::new("prototype.setFlowStart", "Add flow starting point", set_flow_start)
            .doc("Marks a top-level frame as a prototype starting point on its page.")
            .params(const { &[param("frame", ParamType::NodeId, true, "Top-level frame.")] }),
        CommandSpec::new("prototype.removeFlowStart", "Remove flow starting point", remove_flow_start)
            .doc("Removes a starting point.")
            .params(const { &[param("frame", ParamType::NodeId, true, "Frame.")] }),
        CommandSpec::new("prototype.list", "Prototype map", list)
            .read_only()
            .doc("Lists the current page's flow starts and every interaction: {starts, interactions:[{node, frame, trigger, action, transition}]}."),
        CommandSpec::new("prototype.simulate", "Simulate prototype", simulate)
            .read_only()
            .doc("Plays the prototype headlessly: start at a frame, then apply events [{type: click|hover, x, y} (top-frame coordinates) | {type: scroll, x, y, dx, dy} | {type: wait, ms}]. Returns {visited, current, overlays, urls, scroll} and, with preview, a PNG of the final screen.")
            .params(const {
                &[
                    param("start", ParamType::NodeId, false, "Starting frame; default the first flow start."),
                    param("events", ParamType::Json, false, "Event list."),
                    param("preview", ParamType::Bool, false, "Include a PNG of the final frame."),
                ]
            }),
    ]
}

fn layer(ctx: &Context<'_>, p: &Params<'_>, key: &str) -> Result<NodeId, EngineError> {
    let id = p.req_node_id(key)?;
    match ctx.doc.get(id).map(|n| &n.kind) {
        Some(NodeKind::Document | NodeKind::Page) | None => {
            Err(p.bad(key, format!("{id} is not a layer")))
        }
        _ => Ok(id),
    }
}

fn frame(ctx: &Context<'_>, p: &Params<'_>, key: &str) -> Result<NodeId, EngineError> {
    let id = layer(ctx, p, key)?;
    if !ctx.doc.get(id).is_some_and(|n| n.kind.is_frame_like()) {
        return Err(p.bad(key, format!("{id} is not a frame")));
    }
    Ok(id)
}

fn add(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = layer(ctx, p, "id")?;
    let trigger = match p.choice("trigger", &["click", "hover", "press", "delay"])? {
        Some("hover") => Trigger::OnHover,
        Some("press") => Trigger::OnPress,
        Some("delay") => Trigger::AfterDelay {
            ms: p.u32("delay")?.unwrap_or(800).min(3_600_000),
        },
        _ => Trigger::OnClick,
    };
    let action = match p.choice(
        "action",
        &[
            "navigate",
            "overlay",
            "closeOverlay",
            "back",
            "url",
            "setVariable",
            "conditional",
        ],
    )? {
        Some("setVariable") => {
            let variable = deesainer_doc::VariableId(
                p.raw("variable")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| p.missing("variable"))?,
            );
            let raw = p.raw("value").ok_or_else(|| p.missing("value"))?;
            let value = typed_value(ctx, p, variable, raw)?;
            Action::SetVariable { variable, value }
        }
        Some("conditional") => {
            let cond: Value = p.json("condition")?.ok_or_else(|| p.missing("condition"))?;
            let variable = deesainer_doc::VariableId(
                cond["variable"]
                    .as_u64()
                    .ok_or_else(|| p.bad("condition", "needs a variable"))?,
            );
            let op = match cond["op"].as_str() {
                Some("equals") => deesainer_doc::CompareOp::Equals,
                Some("notEquals") => deesainer_doc::CompareOp::NotEquals,
                Some("greater") => deesainer_doc::CompareOp::Greater,
                Some("less") => deesainer_doc::CompareOp::Less,
                _ => {
                    return Err(p.bad("condition", "op must be equals, notEquals, greater or less"))
                }
            };
            let value = typed_value(ctx, p, variable, &cond["value"])?;
            let branch = |key: &str| -> Result<Vec<Action>, EngineError> {
                let list: Vec<Action> = p.json(key)?.unwrap_or_default();
                if list.len() > 32 {
                    return Err(p.bad(key, "at most 32 actions"));
                }
                for a in &list {
                    check_action(ctx, p, key, a, 0)?;
                }
                Ok(list)
            };
            Action::Conditional {
                condition: deesainer_doc::Condition {
                    variable,
                    op,
                    value,
                },
                then: branch("then")?,
                otherwise: branch("else")?,
            }
        }
        Some("navigate") => Action::Navigate {
            to: frame(ctx, p, "to")?,
        },
        Some("overlay") => Action::OpenOverlay {
            frame: frame(ctx, p, "to")?,
        },
        Some("closeOverlay") => Action::CloseOverlay,
        Some("back") => Action::Back,
        Some("url") => {
            let url = p.req_str("url")?;
            if !(url.starts_with("https://")
                || url.starts_with("http://")
                || url.starts_with("mailto:"))
            {
                return Err(p.bad("url", "must start with https://, http:// or mailto:"));
            }
            Action::OpenUrl {
                url: url.to_owned(),
            }
        }
        _ => return Err(p.missing("action")),
    };
    let direction = match p.choice("direction", &["left", "right", "up", "down"])? {
        Some("right") => Direction::Right,
        Some("up") => Direction::Up,
        Some("down") => Direction::Down,
        _ => Direction::Left,
    };
    let kind = match p.choice(
        "transition",
        &["instant", "dissolve", "smartAnimate", "moveIn", "push"],
    )? {
        Some("dissolve") => TransitionKind::Dissolve,
        Some("smartAnimate") => TransitionKind::SmartAnimate,
        Some("moveIn") => TransitionKind::MoveIn { direction },
        Some("push") => TransitionKind::Push { direction },
        _ => TransitionKind::Instant,
    };
    let easing = match p.choice("easing", &["linear", "easeIn", "easeOut", "easeInOut"])? {
        Some("linear") => Easing::Linear,
        Some("easeIn") => Easing::EaseIn,
        Some("easeInOut") => Easing::EaseInOut,
        _ => Easing::EaseOut,
    };
    let interaction = Interaction {
        trigger,
        action,
        transition: Transition {
            kind,
            duration_ms: p.u32("duration")?.unwrap_or(300).min(10_000),
            easing,
        },
    };
    let mut list = ctx.doc.node(id)?.interactions.clone();
    list.push(interaction);
    let index = list.len() - 1;
    Ok(Outcome::apply(
        Transaction::new(
            "Add interaction",
            vec![Op::set(id, PropValue::Interactions(list))],
        ),
        json!({ "index": index }),
    ))
}

fn remove(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = layer(ctx, p, "id")?;
    let mut list = ctx.doc.node(id)?.interactions.clone();
    match p.u32("index")? {
        Some(i) if (i as usize) < list.len() => {
            list.remove(i as usize);
        }
        Some(_) => return Err(p.bad("index", "out of range")),
        None => list.clear(),
    }
    Ok(Outcome::apply(
        Transaction::new(
            "Remove interaction",
            vec![Op::set(id, PropValue::Interactions(list))],
        ),
        json!({}),
    ))
}

fn page_of_frame(ctx: &Context<'_>, p: &Params<'_>, f: NodeId) -> Result<NodeId, EngineError> {
    let parent = ctx
        .doc
        .node(f)?
        .parent
        .ok_or_else(|| p.invalid("frame without parent"))?;
    if !matches!(ctx.doc.get(parent).map(|n| &n.kind), Some(NodeKind::Page)) {
        return Err(p.bad("frame", "flow starts must be top-level frames"));
    }
    Ok(parent)
}

fn set_flow_start(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let f = frame(ctx, p, "frame")?;
    let page = page_of_frame(ctx, p, f)?;
    let mut list = ctx.doc.node(page)?.interactions.clone();
    if list
        .iter()
        .any(|i| i.action == (Action::Navigate { to: f }))
    {
        return Ok(Outcome::read(json!({})));
    }
    list.push(Interaction {
        trigger: Trigger::OnClick,
        action: Action::Navigate { to: f },
        transition: Transition::default(),
    });
    Ok(Outcome::apply(
        Transaction::new(
            "Add flow start",
            vec![Op::set(page, PropValue::Interactions(list))],
        ),
        json!({}),
    ))
}

fn remove_flow_start(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let f = frame(ctx, p, "frame")?;
    let page = page_of_frame(ctx, p, f)?;
    let mut list = ctx.doc.node(page)?.interactions.clone();
    list.retain(|i| i.action != (Action::Navigate { to: f }));
    Ok(Outcome::apply(
        Transaction::new(
            "Remove flow start",
            vec![Op::set(page, PropValue::Interactions(list))],
        ),
        json!({}),
    ))
}

fn list(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let page = ctx.current_page;
    let starts = deesainer_tools::proto::flow_starts(ctx.doc, page);
    let mut out = Vec::new();
    for f in deesainer_tools::proto::frames(ctx.doc, page) {
        for id in ctx.doc.subtree_pre_order(f).unwrap_or_default() {
            let Some(n) = ctx.doc.get(id) else { continue };
            for i in &n.interactions {
                out.push(json!({ "node": id, "frame": f, "trigger": i.trigger, "action": i.action, "transition": i.transition }));
            }
        }
    }
    Ok(Outcome::read(
        json!({ "starts": starts, "interactions": out }),
    ))
}

fn simulate(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let start = p.node_id("start")?;
    let mut player = Player::start(ctx.doc, ctx.current_page, start)
        .ok_or_else(|| p.invalid("this page has no frames to play"))?;
    let events: Vec<Value> = p.json("events")?.unwrap_or_default();
    if events.len() > 1000 {
        return Err(p.bad("events", "at most 1000 events"));
    }
    let mut visited = vec![player.current];
    let mut scroll = deesainer_tools::proto::ScrollOffsets::new();
    let mut scrolled_frame = player.current;
    let mut urls = Vec::new();
    let mut record = |e: Option<Event>, visited: &mut Vec<NodeId>| match e {
        Some(Event::Navigated(f)) => visited.push(f),
        Some(Event::OpenUrl(u)) => urls.push(u),
        _ => {}
    };
    for ev in &events {
        player.tick(ctx.doc, 60.0);
        if player.current != scrolled_frame {
            scroll.clear();
            scrolled_frame = player.current;
        }
        let shown = crate::preview::with_variable_values(ctx.doc, &player.variables);
        let view = deesainer_tools::proto::scrolled_view(&shown, &scroll);
        let pt = || {
            Point::new(
                ev["x"].as_f64().unwrap_or(0.0),
                ev["y"].as_f64().unwrap_or(0.0),
            )
        };
        match ev["type"].as_str() {
            Some("click") => {
                let e = player.click(&view, pt());
                record(e, &mut visited);
            }
            Some("hover") => {
                let e = player.hover(&view, pt());
                record(e, &mut visited);
            }
            Some("scroll") => {
                let top = player.top();
                if let Some(f) =
                    deesainer_tools::proto::scroll_target(ctx.doc, ctx.current_page, top, pt())
                {
                    let d = |k: &str| ev[k].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
                    let o = scroll.entry(f).or_insert((0.0, 0.0));
                    o.0 += d("dx");
                    o.1 += d("dy");
                    deesainer_tools::proto::clamp_scroll(ctx.doc, &mut scroll);
                }
            }
            Some("wait") => {
                let secs = ev["ms"].as_f64().unwrap_or(0.0).clamp(0.0, 3_600_000.0) / 1000.0;
                for e in player.tick(ctx.doc, secs) {
                    record(Some(e), &mut visited);
                }
            }
            _ => {
                return Err(p.bad(
                    "events",
                    "each event needs type click, hover, scroll or wait",
                ))
            }
        }
    }
    player.tick(ctx.doc, 60.0);
    let mut reply = json!({
        "visited": visited,
        "current": player.current,
        "overlays": player.overlays,
        "urls": urls,
    });
    if player.current != scrolled_frame {
        scroll.clear();
    }
    reply["variables"] = json!(player
        .variables
        .iter()
        .map(|(id, v)| json!({ "id": id, "value": v }))
        .collect::<Vec<_>>());
    reply["scroll"] = json!(scroll
        .iter()
        .map(|(f, (x, y))| json!({ "frame": f, "x": x, "y": y }))
        .collect::<Vec<_>>());
    if p.bool("preview")?.unwrap_or(false) {
        let shown = crate::preview::with_variable_values(ctx.doc, &player.variables);
        let view = deesainer_tools::proto::scrolled_view(&shown, &scroll);
        let png = deesainer_io::export_png(&view, player.top(), 1.0)
            .map_err(|e| p.invalid(e.to_string()))?;
        use base64::Engine as _;
        reply["preview"] = json!({ "mimeType": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(png) });
    }
    Ok(Outcome::read(reply))
}

fn set_scrolling(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    use deesainer_doc::Overflow;
    let ids = super::layer_ids(ctx, p)?;
    let overflow = match p.choice("overflow", &["none", "vertical", "horizontal", "both"])? {
        Some("vertical") => Some(Overflow::Vertical),
        Some("horizontal") => Some(Overflow::Horizontal),
        Some("both") => Some(Overflow::Both),
        Some(_) => Some(Overflow::None),
        None => None,
    };
    let fixed = p.bool("fixed")?;
    let mut ops = Vec::new();
    for id in ids {
        let node = ctx
            .doc
            .get(id)
            .ok_or_else(|| p.bad("ids", format!("{id} does not exist")))?;
        let mut s = node.scrolling;
        if let Some(o) = overflow {
            if o != Overflow::None && !node.kind.is_frame_like() {
                return Err(p.bad("overflow", "only frames scroll"));
            }
            s.overflow = o;
        }
        if let Some(f) = fixed {
            s.fixed = f;
        }
        if s != node.scrolling {
            ops.push(Op::set(id, PropValue::Scrolling(s)));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Scrolling", ops),
        json!({}),
    ))
}

fn set_device(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    use deesainer_doc::Device;
    let page = p.node_id("page")?.unwrap_or(ctx.current_page);
    if !ctx
        .doc
        .get(page)
        .is_some_and(|n| matches!(n.kind, NodeKind::Page))
    {
        return Err(p.bad("page", "not a page"));
    }
    let device = match p.choice("device", &["none", "phone", "tablet", "browser"])? {
        Some("phone") => Device::Phone,
        Some("tablet") => Device::Tablet,
        Some("browser") => Device::Browser,
        Some(_) => Device::None,
        None => return Err(p.missing("device")),
    };
    Ok(Outcome::apply(
        Transaction::new("Device", vec![Op::set(page, PropValue::Device(device))]),
        json!({}),
    ))
}

fn typed_value(
    ctx: &Context<'_>,
    p: &Params<'_>,
    variable: deesainer_doc::VariableId,
    raw: &Value,
) -> Result<deesainer_doc::VarValue, EngineError> {
    use deesainer_doc::VarValue;
    let current = ctx
        .doc
        .resolve_variable(variable)
        .ok_or_else(|| p.bad("variable", format!("variable {variable} does not exist")))?;
    let value = match current {
        VarValue::Color(_) => raw
            .as_str()
            .and_then(|h| deesainer_color::Rgba::from_hex(h).ok())
            .map(VarValue::Color),
        VarValue::Number(_) => raw.as_f64().filter(|n| n.is_finite()).map(VarValue::Number),
        VarValue::String(_) => raw.as_str().map(|s| VarValue::String(s.to_owned())),
        VarValue::Bool(_) => raw.as_bool().map(VarValue::Bool),
        VarValue::Alias(_) => None,
    };
    value.ok_or_else(|| {
        p.bad(
            "value",
            format!("must be a {} like the variable", current.type_name()),
        )
    })
}

fn check_action(
    ctx: &Context<'_>,
    p: &Params<'_>,
    key: &str,
    a: &Action,
    depth: usize,
) -> Result<(), EngineError> {
    if depth > 8 {
        return Err(p.bad(key, "conditionals nest at most 8 deep"));
    }
    match a {
        Action::Navigate { to } | Action::OpenOverlay { frame: to } => {
            if !ctx.doc.contains(*to) {
                return Err(p.bad(key, format!("frame {to} does not exist")));
            }
        }
        Action::SetVariable { variable, value } => {
            let ok = ctx
                .doc
                .resolve_variable(*variable)
                .is_some_and(|v| v.type_name() == value.type_name());
            if !ok {
                return Err(p.bad(
                    key,
                    format!("variable {variable} missing or of another type"),
                ));
            }
        }
        Action::Conditional {
            condition,
            then,
            otherwise,
        } => {
            if ctx.doc.variable(condition.variable).is_none() {
                return Err(p.bad(key, "the condition's variable does not exist"));
            }
            for a in then.iter().chain(otherwise) {
                check_action(ctx, p, key, a, depth + 1)?;
            }
        }
        Action::OpenUrl { url } => {
            if !(url.starts_with("https://")
                || url.starts_with("http://")
                || url.starts_with("mailto:"))
            {
                return Err(p.bad(key, "links must start with https://, http:// or mailto:"));
            }
        }
        Action::CloseOverlay | Action::Back => {}
    }
    Ok(())
}
