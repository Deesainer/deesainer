#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{BindTarget, NodeId, PaintKind, StyleKind, VarValue};
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn hex(s: &str) -> deesainer_color::Rgba {
    deesainer_color::Rgba::from_hex(s).unwrap()
}

fn paint(c: &str) -> Value {
    json!([deesainer_engine::doc::Paint::solid(hex(c))])
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

#[test]
fn libraries_share_the_styles_and_variables_their_components_use() {
    let mut lib = Session::new().unwrap();
    let theme = id(&run(
        &mut lib,
        "collection.create",
        json!({"name": "Brand", "modes": ["Light", "Dark"]}),
    ));
    let base = id(&run(
        &mut lib,
        "variable.create",
        json!({"name": "blue", "type": "color", "values": ["#0D99FF", "#0A6DC2"], "collection": theme}),
    ));
    let accent = id(&run(
        &mut lib,
        "variable.create",
        json!({"name": "accent", "type": "color", "values": ["#000000", "#000000"], "collection": theme}),
    ));
    run(
        &mut lib,
        "variable.set",
        json!({"id": accent, "alias": base}),
    );
    let r = id(&run(
        &mut lib,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 40, "height": 40, "fill": "#0D99FF"}),
    ));
    let border = id(&run(
        &mut lib,
        "style.create",
        json!({"name": "Border", "kind": "paint", "value": paint("#14AE5C")}),
    ));
    run(
        &mut lib,
        "style.apply",
        json!({"ids": [r], "style": border, "target": "stroke"}),
    );
    run(
        &mut lib,
        "variable.bind",
        json!({"ids": [r], "target": "fillColor", "variable": accent}),
    );
    let c = id(&run(&mut lib, "component.create", json!({"ids": [r]})));
    let v1 = run(&mut lib, "document.serialize", json!({}))["data"].clone();

    let mut app = Session::new().unwrap();
    let out = run(
        &mut app,
        "library.import",
        json!({"data": v1, "library": "UI"}),
    );
    assert_eq!(
        (out["styles"].as_u64(), out["variables"].as_u64()),
        (Some(1), Some(2))
    );
    let doc = app.snapshot();
    let (sid, style) = doc.styles().next().unwrap();
    assert_eq!(style.name, "Border");
    assert_eq!(style.origin.as_ref().unwrap().library, "UI");
    let (cid, brand) = doc.collections().find(|(_, m)| m.name == "Brand").unwrap();
    assert_eq!(brand.names, ["Light", "Dark"]);
    let local = |name: &str| {
        doc.variables()
            .find(|(_, v)| v.name == name)
            .map(|(id, v)| (id, v.clone()))
            .unwrap()
    };
    let (blue_id, blue) = local("blue");
    let (accent_id, accent_v) = local("accent");
    assert_eq!(blue.collection, cid);
    assert_eq!(accent_v.values[0], VarValue::Alias(blue_id));
    let rect = doc
        .nodes()
        .find(|n| {
            n.parent
                .is_some_and(|p| doc.get(p).is_some_and(|c| c.origin.is_some()))
        })
        .unwrap();
    assert_eq!(rect.styles.stroke, Some(sid));
    assert_eq!(rect.bindings.get(&BindTarget::FillColor), Some(&accent_id));

    run(
        &mut lib,
        "style.update",
        json!({"id": border, "value": paint("#F24E1E")}),
    );
    let v2 = run(&mut lib, "document.serialize", json!({}))["data"].clone();
    run(
        &mut app,
        "library.update",
        json!({"data": v2.clone(), "library": "UI"}),
    );
    let doc = app.snapshot();
    assert_eq!(doc.styles().count(), 1);
    assert_eq!(doc.variables().count(), 2);
    let StyleKind::Paint(p) = &doc.style(sid).unwrap().kind else {
        panic!("not a paint style");
    };
    assert_eq!(
        p[0].kind,
        PaintKind::Solid {
            color: hex("#F24E1E")
        }
    );
    let rect = doc
        .nodes()
        .find(|n| {
            n.parent
                .is_some_and(|p| doc.get(p).is_some_and(|c| c.origin.is_some()))
        })
        .unwrap();
    assert_eq!(rect.styles.stroke, Some(sid));
    run(&mut app, "edit.undo", json!({}));
    let doc = app.snapshot();
    let StyleKind::Paint(p) = &doc.style(sid).unwrap().kind else {
        panic!("not a paint style");
    };
    assert_eq!(
        p[0].kind,
        PaintKind::Solid {
            color: hex("#14AE5C")
        }
    );
    let _ = (c, NodeId(0));
}
