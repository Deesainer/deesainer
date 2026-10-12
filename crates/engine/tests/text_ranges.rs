#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::{doc::NodeId, Session};
use serde_json::json;

#[test]
fn range_style_round_trips_through_commands_and_history() {
    let mut session = Session::new().unwrap();
    let id = session
        .dispatch(
            "node.create.text",
            &json!({
                "x": 0, "y": 0, "content": "aé🙂z", "fontFamily": "sans-serif"
            }),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let id = NodeId(id);
    let base = session.doc().get(id).unwrap().text().unwrap().style.clone();
    let mut bold = base.clone();
    bold.font_weight = 700;
    assert!(session
        .dispatch(
            "text.setRangeStyle",
            &json!({
                "id": id, "start": 1, "end": 2, "style": bold,
            })
        )
        .is_err());
    session
        .dispatch(
            "text.setRangeStyle",
            &json!({
                "id": id, "start": 1, "end": 7, "style": bold,
            }),
        )
        .unwrap();
    let data = session.doc().get(id).unwrap().text().unwrap();
    assert_eq!((data.runs[0].start, data.runs[0].end), (1, 7));
    assert_eq!(data.runs[0].style.font_weight, 700);
    session.dispatch("edit.undo", &json!({})).unwrap();
    assert!(session
        .doc()
        .get(id)
        .unwrap()
        .text()
        .unwrap()
        .runs
        .is_empty());
    session.dispatch("edit.redo", &json!({})).unwrap();
    assert_eq!(session.doc().get(id).unwrap().text().unwrap().runs.len(), 1);
    session
        .dispatch("text.setContent", &json!({ "id": id, "content": "aé!🙂z" }))
        .unwrap();
    let data = session.doc().get(id).unwrap().text().unwrap();
    assert_eq!((data.runs[0].start, data.runs[0].end), (1, 8));
    assert_eq!(data.content, "aé!🙂z");
}

#[test]
fn range_colour_and_decoration_survive_layer_style_changes() {
    let mut session = Session::new().unwrap();
    let id = NodeId(
        session
            .dispatch(
                "node.create.text",
                &json!({ "x": 0, "y": 0, "content": "Hello world", "fontFamily": "sans-serif" }),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap(),
    );
    let mut style = session.doc().get(id).unwrap().text().unwrap().style.clone();
    style.fill = Some(deesainer_engine::doc::Paint::solid(
        deesainer_color::Rgba::new(1.0, 0.0, 0.0, 1.0),
    ));
    style.decoration = deesainer_engine::doc::TextDecoration::Underline;
    session
        .dispatch(
            "text.setRangeStyle",
            &json!({ "id": id, "start": 6, "end": 11, "style": style }),
        )
        .unwrap();
    session
        .dispatch("text.setStyle", &json!({ "ids": [id], "fontSize": 30 }))
        .unwrap();
    let data = session.doc().get(id).unwrap().text().unwrap();
    assert_eq!(data.runs.len(), 1);
    assert_eq!(
        data.runs[0].style.decoration,
        deesainer_engine::doc::TextDecoration::Underline
    );
    assert!(data.runs[0].style.fill.is_some());
    session
        .dispatch(
            "text.setStyle",
            &json!({ "ids": [id], "decoration": "none" }),
        )
        .unwrap();
    let data = session.doc().get(id).unwrap().text().unwrap();
    assert!(data
        .runs
        .iter()
        .all(|r| r.style.decoration == deesainer_engine::doc::TextDecoration::None));
}

#[test]
fn a_range_can_carry_a_gradient_that_exports_and_renders() {
    let mut session = Session::new().unwrap();
    let id = NodeId(
        session
            .dispatch(
                "node.create.text",
                &json!({ "x": 0, "y": 0, "content": "Gradient words", "fontFamily": "sans-serif", "fontSize": 40 }),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap(),
    );
    let mut style = session.doc().get(id).unwrap().text().unwrap().style.clone();
    style.fill = Some(deesainer_engine::doc::Paint::linear(
        deesainer_geom::Point::new(0.0, 0.5),
        deesainer_geom::Point::new(1.0, 0.5),
        vec![
            deesainer_engine::doc::ColorStop {
                position: 0.0,
                color: deesainer_color::Rgba::new(1.0, 0.0, 0.0, 1.0),
            },
            deesainer_engine::doc::ColorStop {
                position: 1.0,
                color: deesainer_color::Rgba::new(0.0, 0.0, 1.0, 1.0),
            },
        ],
    ));
    session
        .dispatch(
            "text.setRangeStyle",
            &json!({ "id": id, "start": 0, "end": 8, "style": style }),
        )
        .unwrap();
    let svg = session
        .dispatch("export.svg", &json!({ "id": id }))
        .unwrap();
    let svg = svg["svg"].as_str().unwrap();
    assert!(svg.contains("linearGradient"), "{svg}");
    let mut bad = style.clone();
    if let Some(p) = &mut bad.fill {
        p.opacity = 7.0;
    }
    assert!(session
        .dispatch(
            "text.setRangeStyle",
            &json!({ "id": id, "start": 0, "end": 4, "style": bad })
        )
        .is_err());
    let png = session
        .dispatch("export.png", &json!({ "id": id, "scale": 1 }))
        .unwrap();
    assert!(png["width"].as_u64().unwrap() > 50);
}

#[test]
fn an_image_on_a_range_fills_the_range_box() {
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";
    let mut session = Session::new().unwrap();
    let placed = session
        .dispatch("image.place", &json!({"data": png, "x": 400, "y": 400}))
        .unwrap();
    let asset = placed["asset"].as_str().unwrap().to_owned();
    let id = NodeId(
        session
            .dispatch(
                "node.create.text",
                &json!({ "x": 0, "y": 0, "content": "Image words", "fontFamily": "sans-serif", "fontSize": 40 }),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap(),
    );
    let mut style = session.doc().get(id).unwrap().text().unwrap().style.clone();
    style.fill = Some(deesainer_engine::doc::Paint {
        kind: deesainer_engine::doc::PaintKind::Image {
            asset: deesainer_engine::doc::AssetId(asset),
            scale_mode: deesainer_engine::doc::ImageScaleMode::Fill,
            adjust: Default::default(),
        },
        ..deesainer_engine::doc::Paint::solid(deesainer_color::Rgba::BLACK)
    });
    session
        .dispatch(
            "text.setRangeStyle",
            &json!({ "id": id, "start": 6, "end": 11, "style": style }),
        )
        .unwrap();
    let svg = session
        .dispatch("export.svg", &json!({ "id": id }))
        .unwrap();
    let svg = svg["svg"].as_str().unwrap();
    let at = svg.find("<image").expect("an image element");
    let x: f64 = svg[at..]
        .split("x=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .and_then(|s| s.parse().ok())
        .unwrap();
    assert!(
        x > 50.0,
        "the image frame starts at the range, not at 0: {x}"
    );
    session
        .dispatch("export.png", &json!({ "id": id }))
        .unwrap();
    session
        .dispatch("export.pdf", &json!({ "ids": [id] }))
        .unwrap();
}
