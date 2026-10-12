#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_color::Rgba;
use deesainer_doc::{Document, Effect, Node, NodeId, NodeKind, Paint};
use deesainer_geom::{Point, Rect};

fn sample() -> (Document, NodeId) {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut frame = Node::new(
        doc.next_free_id(),
        "Card",
        NodeKind::Frame,
        Rect::new(10.0, 10.0, 200.0, 100.0),
    );
    frame.fills.push(Paint::solid(Rgba::WHITE));
    frame.clips_content = true;
    let f = frame.id;
    doc.insert(frame, page, 0).unwrap();
    let mut r = Node::new(
        doc.next_free_id(),
        "Dot",
        NodeKind::ellipse(),
        Rect::new(20.0, 20.0, 40.0, 40.0),
    );
    r.fills.push(Paint::solid(Rgba::new(1.0, 0.0, 0.0, 0.5)));
    r.strokes.push(Paint::solid(Rgba::BLACK));
    r.effects.push(Effect::DropShadow {
        color: Rgba::BLACK,
        offset: Point::new(0.0, 2.0),
        blur: 4.0,
        spread: 0.0,
        visible: true,
    });
    doc.insert(r, f, 0).unwrap();
    let mut t = Node::new(
        doc.next_free_id(),
        "Title",
        NodeKind::Text(deesainer_doc::TextData {
            content: "Hello".into(),
            ..Default::default()
        }),
        Rect::new(80.0, 20.0, 60.0, 20.0),
    );
    t.fills.push(Paint::solid(Rgba::BLACK));
    doc.insert(t, f, 1).unwrap();
    (doc, f)
}

#[test]
fn svg_export_is_well_formed_and_reimports() {
    let (doc, f) = sample();
    let svg = deesainer_io::export_svg(&doc, f).unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("feDropShadow"));
    assert!(svg.contains("clip-path"));
    let imported = deesainer_io::import_svg(&svg, "Card").unwrap();
    assert!(imported.nodes.len() > 2, "{}", imported.nodes.len());
    assert_eq!(imported.size, (200.0, 100.0));
}

#[test]
fn angular_and_diamond_gradients_export_to_svg_as_images() {
    let (mut doc, f) = sample();
    let stops = vec![
        deesainer_doc::ColorStop {
            position: 0.0,
            color: Rgba::BLACK,
        },
        deesainer_doc::ColorStop {
            position: 1.0,
            color: Rgba::WHITE,
        },
    ];
    doc.get_mut(f).unwrap().fills = vec![Paint {
        kind: deesainer_doc::PaintKind::AngularGradient {
            center: Point::new(0.5, 0.5),
            angle: 45.0,
            stops: stops.clone(),
        },
        ..Paint::solid(Rgba::WHITE)
    }];
    let svg = deesainer_io::export_svg(&doc, f).unwrap();
    assert!(svg.contains("<pattern"), "{svg}");
    assert!(svg.contains("data:image/png;base64,"));
    doc.get_mut(f).unwrap().fills = vec![Paint {
        kind: deesainer_doc::PaintKind::DiamondGradient {
            center: Point::new(0.5, 0.5),
            radius: Point::new(0.5, 0.5),
            stops,
        },
        ..Paint::solid(Rgba::WHITE)
    }];
    let svg = deesainer_io::export_svg(&doc, f).unwrap();
    assert!(svg.contains("<pattern"));
    assert!(deesainer_io::import_svg(&svg, "Card").is_ok());
}

#[test]
fn pdf_export_produces_a_pdf() {
    let (doc, f) = sample();
    let pdf = deesainer_io::export_pdf(&doc, &[f, f]).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 2"));
}

#[test]
fn png_export() {
    let (doc, f) = sample();
    let png = deesainer_io::export_png(&doc, f, 2.0).unwrap();
    let img = deesainer_render::images::decode(&png).unwrap();
    assert!(img.width() >= 400);
}

#[test]
fn svg_import_paths_gradients_and_rejects_garbage() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">
      <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
      <g transform="translate(10 5)"><rect id="box" width="40" height="20" fill="url(#g)" stroke="#000" stroke-width="2"/></g>
      <circle cx="80" cy="25" r="10" fill="#0f0"/>
    </svg>"##;
    let imp = deesainer_io::import_svg(svg, "Icon").unwrap();
    let boxed = imp.nodes.values().find(|n| n.name == "box").unwrap();
    assert_eq!(boxed.rect, Rect::new(10.0, 5.0, 40.0, 20.0));
    assert!(matches!(
        boxed.fills[0].kind,
        deesainer_doc::PaintKind::LinearGradient { .. }
    ));
    assert_eq!(boxed.stroke.weight, 2.0);
    assert!(deesainer_io::import_svg("<not svg", "x").is_err());
}
