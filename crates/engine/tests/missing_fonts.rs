#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::NodeId;
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

#[test]
fn missing_fonts_are_listed_and_replaced_everywhere() {
    let mut s = Session::new().unwrap();
    let text = |s: &mut Session, family: &str| {
        NodeId(
            run(
                s,
                "node.create.text",
                json!({"x": 0, "y": 0, "content": "Hello", "fontFamily": family}),
            )["id"]
                .as_u64()
                .unwrap(),
        )
    };
    let a = text(&mut s, "Nowhere Sans Zz");
    let b = text(&mut s, "nowhere sans zz");
    let c = text(&mut s, "sans-serif");
    let missing = run(&mut s, "text.missingFonts", json!({}));
    let families: Vec<&str> = missing
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["family"].as_str().unwrap())
        .collect();
    assert!(families
        .iter()
        .any(|f| f.eq_ignore_ascii_case("Nowhere Sans Zz")));
    assert!(!families.contains(&"sans-serif"));
    let out = run(
        &mut s,
        "text.replaceFont",
        json!({"from": "Nowhere Sans Zz", "to": "sans-serif"}),
    );
    assert_eq!(out["layers"], 2);
    for id in [a, b, c] {
        assert_eq!(
            s.doc().get(id).unwrap().text().unwrap().style.font_family,
            "sans-serif"
        );
    }
    assert_eq!(run(&mut s, "text.missingFonts", json!({})), json!([]));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        s.doc().get(a).unwrap().text().unwrap().style.font_family,
        "Nowhere Sans Zz"
    );
}
