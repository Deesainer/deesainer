#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::NodeId;
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn rules_for(list: &Value, node: u64) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .filter(|i| i["id"] == json!(node))
        .map(|i| i["rule"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn design_checks_find_raw_values_and_fix_them() {
    let mut s = Session::new().unwrap();
    let brand = id(&run(
        &mut s,
        "variable.create",
        json!({"name": "brand", "value": "#0D99FF", "type": "color"}),
    ));
    run(
        &mut s,
        "variable.create",
        json!({"name": "radius/md", "value": 8}),
    );
    let swatch = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 40, "height": 40, "fill": "#F24E1E"}),
    ));
    let accent = id(&run(
        &mut s,
        "style.create",
        json!({"name": "Accent", "kind": "paint", "from": swatch}),
    ));
    let raw = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 10.5, "y": 100, "width": 40, "height": 40, "fill": "#0D99FF"}),
    ));
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [raw], "cornerRadius": 8}),
    );
    let hidden = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 200, "width": 10, "height": 10, "fill": "#F24E1E"}),
    ));
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [hidden], "visible": false}),
    );
    let empty = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 300, "y": 0, "width": 50, "height": 50}),
    ));
    run(
        &mut s,
        "node.setFills",
        json!({"ids": [empty], "fills": []}),
    );
    let list = run(&mut s, "lint.run", json!({}));
    assert_eq!(rules_for(&list, swatch), ["detachedPaint"]);
    let mut raw_rules = rules_for(&list, raw);
    raw_rules.sort();
    assert_eq!(raw_rules, ["offPixelGrid", "rawValue", "rawValue"]);
    assert!(rules_for(&list, hidden).contains(&"hiddenLayer".to_owned()));
    assert_eq!(rules_for(&list, empty), ["emptyFrame"]);
    let fixed = run(&mut s, "lint.fix", json!({}));
    assert!(fixed["fixed"].as_u64().unwrap() >= 5, "{fixed}");
    let doc = s.snapshot();
    let r = doc.get(NodeId(raw)).unwrap();
    assert_eq!(
        r.bindings.get(&deesainer_doc::BindTarget::FillColor),
        Some(&deesainer_doc::VariableId(brand))
    );
    assert!(r
        .bindings
        .contains_key(&deesainer_doc::BindTarget::CornerRadius));
    assert_eq!(r.rect.x, 11.0);
    assert_eq!(
        doc.get(NodeId(swatch)).unwrap().styles.fill,
        Some(deesainer_doc::StyleId(accent))
    );
    drop(doc);
    let after = run(&mut s, "lint.run", json!({}));
    assert!(rules_for(&after, raw).is_empty(), "{after}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        rules_for(&run(&mut s, "lint.run", json!({})), swatch),
        ["detachedPaint"]
    );
    assert!(s.dispatch("lint.fix", &json!({"rules": ["nope"]})).is_err());
}
