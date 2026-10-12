#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

#[test]
fn scrolling_frames_bring_content_into_view_and_fixed_layers_stay() {
    let mut s = Session::new().unwrap();
    let screen = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 200}),
    ));
    let next = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 300, "y": 0, "width": 100, "height": 200}),
    ));
    let other = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 600, "y": 0, "width": 100, "height": 200}),
    ));
    run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": screen, "x": 0, "y": 0, "width": 100, "height": 600, "fill": "#EEEEEE"}),
    );
    let button = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": screen, "x": 10, "y": 400, "width": 80, "height": 40, "fill": "#0D99FF"}),
    ));
    let header = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": screen, "x": 0, "y": 0, "width": 100, "height": 30, "fill": "#333333"}),
    ));
    run(
        &mut s,
        "prototype.addInteraction",
        json!({"id": button, "trigger": "click", "action": "navigate", "to": next}),
    );
    run(
        &mut s,
        "prototype.addInteraction",
        json!({"id": header, "trigger": "click", "action": "navigate", "to": other}),
    );
    run(
        &mut s,
        "prototype.setScrolling",
        json!({"ids": [screen], "overflow": "vertical"}),
    );
    run(
        &mut s,
        "prototype.setScrolling",
        json!({"ids": [header], "fixed": true}),
    );
    let r = run(
        &mut s,
        "prototype.simulate",
        json!({"start": screen, "events": [{"type": "click", "x": 50, "y": 120}]}),
    );
    assert_eq!(r["current"], json!(screen));
    let r = run(
        &mut s,
        "prototype.simulate",
        json!({"start": screen, "events": [
            {"type": "scroll", "x": 50, "y": 100, "dx": 0, "dy": 300},
            {"type": "click", "x": 50, "y": 120}
        ]}),
    );
    assert_eq!(r["current"], json!(next));
    let r = run(
        &mut s,
        "prototype.simulate",
        json!({"start": screen, "events": [{"type": "scroll", "x": 50, "y": 100, "dy": 5000}]}),
    );
    assert_eq!(r["scroll"][0]["y"], json!(400.0));
    let r = run(
        &mut s,
        "prototype.simulate",
        json!({"start": screen, "events": [
            {"type": "scroll", "x": 50, "y": 100, "dy": 300},
            {"type": "click", "x": 50, "y": 10}
        ]}),
    );
    assert_eq!(r["current"], json!(other));
    assert!(s
        .dispatch(
            "prototype.setScrolling",
            &json!({"ids": [button], "overflow": "vertical"})
        )
        .is_err());
}

#[test]
fn pages_choose_the_device_their_prototype_shows_in() {
    let mut s = Session::new().unwrap();
    run(&mut s, "prototype.setDevice", json!({"device": "phone"}));
    let page = s.current_page();
    assert_eq!(
        s.snapshot().get(page).unwrap().device,
        deesainer_doc::Device::Phone
    );
    let data = run(&mut s, "document.serialize", json!({}))["data"].clone();
    let mut t = Session::new().unwrap();
    run(&mut t, "document.load", json!({ "data": data }));
    assert_eq!(
        t.snapshot().get(t.current_page()).unwrap().device,
        deesainer_doc::Device::Phone
    );
    let r = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 10, "height": 10}),
    ));
    assert!(s
        .dispatch(
            "prototype.setDevice",
            &json!({"device": "tablet", "page": r})
        )
        .is_err());
}

#[test]
fn prototypes_set_variables_and_branch_on_them() {
    let mut s = Session::new().unwrap();
    let screen = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 200, "height": 200}),
    ));
    let done = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 300, "y": 0, "width": 200, "height": 200}),
    ));
    let liked = id(&run(
        &mut s,
        "variable.create",
        json!({"name": "liked", "value": false}),
    ));
    let steps = id(&run(
        &mut s,
        "variable.create",
        json!({"name": "steps", "value": 0}),
    ));
    let heart = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": screen, "x": 10, "y": 10, "width": 30, "height": 30, "fill": "#F24E1E"}),
    ));
    run(
        &mut s,
        "variable.bind",
        json!({"ids": [heart], "target": "visible", "variable": liked}),
    );
    let button = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": screen, "x": 10, "y": 100, "width": 80, "height": 40, "fill": "#0D99FF"}),
    ));
    run(
        &mut s,
        "prototype.addInteraction",
        json!({"id": button, "trigger": "click", "action": "conditional",
            "condition": {"variable": liked, "op": "equals", "value": false},
            "then": [{"type": "setVariable", "variable": liked, "value": {"type": "bool", "value": true}}],
            "else": [{"type": "setVariable", "variable": liked, "value": {"type": "bool", "value": false}}]}),
    );
    run(
        &mut s,
        "prototype.addInteraction",
        json!({"id": heart, "trigger": "click", "action": "conditional",
            "condition": {"variable": steps, "op": "greater", "value": 2},
            "then": [{"type": "navigate", "to": done}],
            "else": [{"type": "setVariable", "variable": steps, "value": {"type": "number", "value": 5}}]}),
    );
    let r = run(
        &mut s,
        "prototype.simulate",
        json!({"start": screen, "events": [{"type": "click", "x": 20, "y": 20}]}),
    );
    assert_eq!(
        r["current"],
        json!(screen),
        "the heart is hidden: nothing to click"
    );
    let r = run(
        &mut s,
        "prototype.simulate",
        json!({"start": screen, "events": [{"type": "click", "x": 50, "y": 120}]}),
    );
    assert_eq!(
        r["variables"][0]["value"],
        json!({"type": "bool", "value": true})
    );
    let r = run(
        &mut s,
        "prototype.simulate",
        json!({"start": screen, "events": [
            {"type": "click", "x": 50, "y": 120},
            {"type": "click", "x": 20, "y": 20},
            {"type": "click", "x": 20, "y": 20}
        ]}),
    );
    assert_eq!(r["current"], json!(done), "{r}");
    assert!(
        !s.snapshot()
            .get(deesainer_doc::NodeId(heart))
            .unwrap()
            .visible
    );
    assert!(s
        .dispatch(
            "prototype.addInteraction",
            &json!({"id": button, "trigger": "click", "action": "setVariable", "variable": liked, "value": 3})
        )
        .is_err());
}
