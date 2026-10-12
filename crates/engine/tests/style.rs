#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use base64::Engine as _;
use deesainer_color::Rgba;
use deesainer_engine::doc::{NodeId, PaintKind};
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn rect(s: &mut Session, fill: &str) -> u64 {
    run(
        s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 10, "height": 10, "fill": fill}),
    )["id"]
        .as_u64()
        .unwrap()
}

fn fill_color(s: &Session, id: u64) -> Rgba {
    match &s.snapshot().get(NodeId(id)).unwrap().fills[0].kind {
        PaintKind::Solid { color } => *color,
        other => panic!("not solid: {other:?}"),
    }
}

#[test]
fn paint_styles_propagate_and_detach_on_delete() {
    let mut s = Session::new().unwrap();
    let a = rect(&mut s, "#FF0000");
    let b = rect(&mut s, "#00FF00");
    let style = run(
        &mut s,
        "style.create",
        json!({"name": "Brand/Red", "kind": "paint", "from": a}),
    )["id"]
        .as_u64()
        .unwrap();
    run(&mut s, "style.apply", json!({"ids": [b], "style": style}));
    assert_eq!(fill_color(&s, b).to_hex(), "#FF0000");
    run(
        &mut s,
        "style.update",
        json!({"id": style, "value": [{"type": "solid", "color": {"r": 0, "g": 0, "b": 1, "a": 1}}]}),
    );
    assert_eq!(fill_color(&s, b).to_hex(), "#0000FF");
    assert_eq!(fill_color(&s, a).to_hex(), "#FF0000");
    run(&mut s, "style.delete", json!({"id": style}));
    let doc = s.snapshot();
    assert!(doc.get(NodeId(b)).unwrap().styles.fill.is_none());
    assert_eq!(fill_color(&s, b).to_hex(), "#0000FF");
    let list = run(&mut s, "style.list", Value::Null);
    assert_eq!(list.as_array().unwrap().len(), 0);
}

#[test]
fn variables_follow_modes() {
    let mut s = Session::new().unwrap();
    let r = rect(&mut s, "#FFFFFF");
    let v = run(
        &mut s,
        "variable.create",
        json!({"name": "color/bg", "value": "#FFFFFF"}),
    )["id"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "variable.bind",
        json!({"ids": [r], "target": "fillColor", "variable": v}),
    );
    run(&mut s, "mode.add", json!({"name": "Dark"}));
    run(
        &mut s,
        "variable.set",
        json!({"id": v, "value": "#111111", "mode": 1}),
    );
    assert_eq!(fill_color(&s, r).to_hex(), "#FFFFFF");
    run(&mut s, "mode.setActive", json!({"name": "Dark"}));
    assert_eq!(fill_color(&s, r).to_hex(), "#111111");
    run(&mut s, "edit.undo", Value::Null);
    assert_eq!(fill_color(&s, r).to_hex(), "#FFFFFF");
    let list = run(&mut s, "variable.list", Value::Null);
    assert_eq!(list["modes"]["names"].as_array().unwrap().len(), 2);
    let alias = run(
        &mut s,
        "variable.create",
        json!({"name": "color/surface", "value": "#000000"}),
    )["id"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "variable.set",
        json!({"id": alias, "alias": v, "mode": 0}),
    );
    run(
        &mut s,
        "variable.bind",
        json!({"ids": [r], "target": "fillColor", "variable": alias}),
    );
    assert_eq!(fill_color(&s, r).to_hex(), "#FFFFFF");
}

fn tiny_png(color: [u8; 4]) -> String {
    let mut pix = deesainer_render::Pixmap::new(4, 2).unwrap();
    pix.fill(tiny_skia_color(color));
    let png = deesainer_render::encode_png(&pix).unwrap();
    base64::engine::general_purpose::STANDARD.encode(png)
}

fn tiny_skia_color([r, g, b, a]: [u8; 4]) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

#[test]
fn images_are_assets_and_render() {
    let mut s = Session::new().unwrap();
    let data = tiny_png([0, 200, 0, 255]);
    let reply = run(
        &mut s,
        "image.place",
        json!({"data": data, "x": 10, "y": 10}),
    );
    let id = NodeId(reply["id"].as_u64().unwrap());
    let doc = s.snapshot();
    assert_eq!(doc.get(id).unwrap().rect.width, 4.0);
    assert_eq!(doc.assets().count(), 1);
    let preview = run(
        &mut s,
        "view.renderPreview",
        json!({"id": id.0, "scale": 4}),
    );
    let png = base64::engine::general_purpose::STANDARD
        .decode(preview["data"].as_str().unwrap())
        .unwrap();
    let img = deesainer_render::images::decode(&png).unwrap();
    let px = deesainer_render::pixel_rgba(&img, 8, 4).unwrap();
    assert_eq!(px, [0, 200, 0, 255]);
    run(&mut s, "edit.undo", Value::Null);
    assert_eq!(s.snapshot().assets().count(), 0);
    assert!(s
        .dispatch("image.place", &json!({"data": "bm90IGFuIGltYWdl"}))
        .is_err());
}
