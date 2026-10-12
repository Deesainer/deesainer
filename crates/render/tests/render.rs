#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_color::Rgba;
use deesainer_doc::{
    BoolOp, ColorStop, Document, Effect, Node, NodeId, NodeKind, Override, Paint, PropValue,
    StrokeAlign,
};
use deesainer_geom::{Affine, Point, Rect};
use deesainer_render::{pixel_rgba, render_node, render_page, Options, Viewport};

fn add(doc: &mut Document, parent: NodeId, mut node: Node) -> NodeId {
    node.id = doc.next_free_id();
    let id = node.id;
    let index = doc.get(parent).unwrap().children.len();
    doc.insert(node, parent, index).unwrap();
    id
}

fn rect_node(x: f64, y: f64, w: f64, h: f64, color: Rgba) -> Node {
    let mut n = Node::new(NodeId(0), "R", NodeKind::Rectangle, Rect::new(x, y, w, h));
    n.fills.push(Paint::solid(color));
    n
}

fn render(doc: &Document, w: u32, h: u32) -> deesainer_render::Pixmap {
    let vp = Viewport {
        transform: Affine::IDENTITY,
        width: w,
        height: h,
    };
    render_page(
        doc,
        doc.first_page(),
        &vp,
        Options {
            background: true,
            ..Options::default()
        },
    )
    .unwrap()
    .0
}

const RED: Rgba = Rgba::new(1.0, 0.0, 0.0, 1.0);
const BLUE: Rgba = Rgba::new(0.0, 0.0, 1.0, 1.0);

#[test]
fn solid_fill_and_background() {
    let mut doc = Document::new();
    let page = doc.first_page();
    add(&mut doc, page, rect_node(10.0, 10.0, 20.0, 20.0, RED));
    let pix = render(&doc, 50, 50);
    assert_eq!(pixel_rgba(&pix, 20, 20), Some([255, 0, 0, 255]));
    assert_eq!(pixel_rgba(&pix, 5, 5), Some([0xF5, 0xF5, 0xF5, 255]));
}

#[test]
fn frames_clip_children() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut frame = Node::new(
        NodeId(0),
        "F",
        NodeKind::Frame,
        Rect::new(0.0, 0.0, 20.0, 20.0),
    );
    frame.fills.push(Paint::solid(Rgba::WHITE));
    let f = add(&mut doc, page, frame);
    add(&mut doc, f, rect_node(10.0, 10.0, 30.0, 30.0, BLUE));
    let pix = render(&doc, 50, 50);
    assert_eq!(pixel_rgba(&pix, 15, 15), Some([0, 0, 255, 255]));
    assert_eq!(pixel_rgba(&pix, 30, 30), Some([0xF5, 0xF5, 0xF5, 255]));
}

#[test]
fn opacity_blends_with_background() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut r = rect_node(0.0, 0.0, 10.0, 10.0, Rgba::BLACK);
    r.opacity = 0.5;
    add(&mut doc, page, r);
    let pix = render(&doc, 10, 10);
    let [v, _, _, a] = pixel_rgba(&pix, 5, 5).unwrap();
    assert_eq!(a, 255);
    assert!((118..=127).contains(&v), "{v}");
}

#[test]
fn linear_gradient_runs_left_to_right() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut r = rect_node(0.0, 0.0, 100.0, 10.0, Rgba::BLACK);
    r.fills = vec![Paint::linear(
        Point::new(0.0, 0.5),
        Point::new(1.0, 0.5),
        vec![
            ColorStop {
                position: 0.0,
                color: Rgba::BLACK,
            },
            ColorStop {
                position: 1.0,
                color: Rgba::WHITE,
            },
        ],
    )];
    add(&mut doc, page, r);
    let pix = render(&doc, 100, 10);
    let left = pixel_rgba(&pix, 2, 5).unwrap()[0];
    let mid = pixel_rgba(&pix, 50, 5).unwrap()[0];
    let right = pixel_rgba(&pix, 97, 5).unwrap()[0];
    assert!(left < mid && mid < right, "{left} {mid} {right}");
}

#[test]
fn inside_and_outside_strokes() {
    for (align, inside_px, outside_px) in [
        (
            StrokeAlign::Inside,
            [255, 0, 0, 255],
            [0xF5, 0xF5, 0xF5, 255],
        ),
        (StrokeAlign::Outside, [0, 0, 255, 255], [255, 0, 0, 255]),
    ] {
        let mut doc = Document::new();
        let page = doc.first_page();
        let mut r = rect_node(10.0, 10.0, 20.0, 20.0, BLUE);
        r.strokes.push(Paint::solid(RED));
        r.stroke.weight = 4.0;
        r.stroke.align = align;
        add(&mut doc, page, r);
        let pix = render(&doc, 40, 40);
        assert_eq!(
            pixel_rgba(&pix, 12, 20),
            Some(inside_px),
            "{align:?} inside"
        );
        assert_eq!(
            pixel_rgba(&pix, 8, 20),
            Some(outside_px),
            "{align:?} outside"
        );
    }
}

#[test]
fn drop_shadow_paints_outside_the_shape() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut r = rect_node(10.0, 10.0, 20.0, 20.0, Rgba::WHITE);
    r.effects.push(Effect::DropShadow {
        color: Rgba::BLACK,
        offset: Point::new(6.0, 6.0),
        blur: 0.0,
        spread: 0.0,
        visible: true,
    });
    add(&mut doc, page, r);
    let pix = render(&doc, 50, 50);
    assert_eq!(pixel_rgba(&pix, 33, 33), Some([0, 0, 0, 255]));
    assert_eq!(pixel_rgba(&pix, 20, 20), Some([255, 255, 255, 255]));
}

#[test]
fn mask_clips_siblings_above() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut mask = Node::new(
        NodeId(0),
        "M",
        NodeKind::ellipse(),
        Rect::new(0.0, 0.0, 40.0, 40.0),
    );
    mask.fills.push(Paint::solid(Rgba::BLACK));
    mask.is_mask = true;
    add(&mut doc, page, mask);
    add(&mut doc, page, rect_node(0.0, 0.0, 40.0, 40.0, RED));
    let pix = render(&doc, 40, 40);
    assert_eq!(pixel_rgba(&pix, 20, 20), Some([255, 0, 0, 255]));
    assert_eq!(pixel_rgba(&pix, 1, 1), Some([0xF5, 0xF5, 0xF5, 255]));
}

#[test]
fn instances_render_their_component_with_overrides() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let comp = Node::new(
        NodeId(0),
        "Button",
        NodeKind::Component {
            description: String::new(),
            properties: Vec::new(),
        },
        Rect::new(0.0, 0.0, 20.0, 20.0),
    );
    let c = add(&mut doc, page, comp);
    let inner = add(&mut doc, c, rect_node(0.0, 0.0, 20.0, 20.0, RED));
    let inst = Node::new(
        NodeId(0),
        "Button",
        NodeKind::Instance {
            component: c,
            overrides: vec![Override {
                target: inner,
                value: PropValue::Fills(vec![Paint::solid(BLUE)]),
            }],
        },
        Rect::new(30.0, 0.0, 20.0, 20.0),
    );
    add(&mut doc, page, inst);
    let pix = render(&doc, 60, 20);
    assert_eq!(pixel_rgba(&pix, 10, 10), Some([255, 0, 0, 255]));
    assert_eq!(pixel_rgba(&pix, 40, 10), Some([0, 0, 255, 255]));
}

#[test]
fn boolean_subtract_renders_hole() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut b = Node::new(
        NodeId(0),
        "Bool",
        NodeKind::BooleanOp {
            op: BoolOp::Subtract,
        },
        Rect::new(0.0, 0.0, 40.0, 40.0),
    );
    b.fills.push(Paint::solid(RED));
    let bid = add(&mut doc, page, b);
    add(&mut doc, bid, rect_node(0.0, 0.0, 40.0, 40.0, Rgba::BLACK));
    add(
        &mut doc,
        bid,
        rect_node(10.0, 10.0, 20.0, 20.0, Rgba::BLACK),
    );
    let pix = render(&doc, 40, 40);
    assert_eq!(pixel_rgba(&pix, 5, 5), Some([255, 0, 0, 255]));
    assert_eq!(pixel_rgba(&pix, 20, 20), Some([0xF5, 0xF5, 0xF5, 255]));
}

#[test]
fn render_node_crops_and_scales() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let id = add(&mut doc, page, rect_node(100.0, 100.0, 10.0, 5.0, RED));
    let (pix, bounds) = render_node(&doc, id, 2.0, Options::default()).unwrap();
    assert_eq!(bounds, Rect::new(100.0, 100.0, 10.0, 5.0));
    assert_eq!((pix.width(), pix.height()), (20, 10));
    assert_eq!(pixel_rgba(&pix, 10, 5), Some([255, 0, 0, 255]));
    let png = deesainer_render::encode_png(&pix).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

#[test]
fn rotation_and_zoom() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut r = rect_node(0.0, 0.0, 20.0, 4.0, RED);
    r.rotation = 90.0;
    add(&mut doc, page, r);
    let vp = Viewport {
        transform: Affine::translate(20.0, 20.0).then(Affine::scale(2.0, 2.0)),
        width: 80,
        height: 80,
    };
    let (pix, _) = render_page(&doc, page, &vp, Options::default()).unwrap();
    assert_eq!(pixel_rgba(&pix, 40, 40), Some([255, 0, 0, 255]));
    assert_eq!(pixel_rgba(&pix, 30, 40).map(|p| p[3]), Some(0));
}

#[test]
fn hostile_sizes_are_errors_not_panics() {
    let doc = Document::new();
    let vp = Viewport {
        transform: Affine::IDENTITY,
        width: 0,
        height: 10,
    };
    assert!(render_page(&doc, doc.first_page(), &vp, Options::default()).is_err());
    let vp = Viewport {
        transform: Affine::scale(1e300, 1e300),
        width: 10,
        height: 10,
    };
    let mut doc = Document::new();
    let page = doc.first_page();
    add(&mut doc, page, rect_node(0.0, 0.0, 1e300, 1e300, RED));
    assert!(render_page(&doc, page, &vp, Options::default()).is_ok());
}

#[test]
fn inner_shadow_spread_does_not_darken_twice() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut n = rect_node(0.0, 0.0, 100.0, 100.0, Rgba::new(1.0, 1.0, 1.0, 1.0));
    n.effects = vec![Effect::InnerShadow {
        color: Rgba::new(0.0, 0.0, 0.0, 0.5),
        offset: Point::new(10.0, 10.0),
        blur: 0.0,
        spread: 4.0,
        visible: true,
    }];
    add(&mut doc, page, n);
    let pix = render(&doc, 100, 100);
    let [r, ..] = pixel_rgba(&pix, 8, 50).unwrap();
    assert!(
        (i32::from(r) - 128).abs() <= 2,
        "{r}: the band is darkened twice"
    );
    let [r, ..] = pixel_rgba(&pix, 60, 60).unwrap();
    assert_eq!(r, 255);
}

fn grad_stops() -> Vec<ColorStop> {
    vec![
        ColorStop {
            position: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 1.0),
        },
        ColorStop {
            position: 1.0,
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        },
    ]
}

#[test]
fn diamond_gradients_are_exact_and_seamless() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut n = rect_node(0.0, 0.0, 200.0, 200.0, RED);
    n.fills = vec![Paint {
        kind: deesainer_doc::PaintKind::DiamondGradient {
            center: Point::new(0.5, 0.5),
            radius: Point::new(0.5, 0.5),
            stops: grad_stops(),
        },
        ..Paint::solid(RED)
    }];
    add(&mut doc, page, n);
    let pix = render(&doc, 200, 200);
    let grey = |x: u32, y: u32| i32::from(pixel_rgba(&pix, x, y).unwrap()[0]);
    assert!(grey(100, 100) <= 3);
    for (x, y) in [(150, 100), (100, 150), (50, 100), (125, 125), (75, 75)] {
        assert!(
            (grey(x, y) - 128).abs() <= 4,
            "({x}, {y}) is {}",
            grey(x, y)
        );
    }
    let (a, b, c) = (grey(140, 99), grey(140, 100), grey(140, 101));
    assert!((a - b).abs() <= 4 && (b - c).abs() <= 4, "{a} {b} {c}");
}

#[test]
fn angular_gradients_sweep_from_their_start_angle() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut n = rect_node(0.0, 0.0, 200.0, 200.0, RED);
    n.fills = vec![Paint {
        kind: deesainer_doc::PaintKind::AngularGradient {
            center: Point::new(0.5, 0.5),
            angle: 90.0,
            stops: grad_stops(),
        },
        ..Paint::solid(RED)
    }];
    add(&mut doc, page, n);
    let pix = render(&doc, 200, 200);
    let grey = |x: u32, y: u32| i32::from(pixel_rgba(&pix, x, y).unwrap()[0]);
    assert!(grey(97, 180) <= 12, "{}", grey(97, 180));
    assert!((grey(20, 100) - 64).abs() <= 6, "{}", grey(20, 100));
    assert!((grey(100, 20) - 128).abs() <= 6, "{}", grey(100, 20));
    assert!((grey(180, 100) - 191).abs() <= 6, "{}", grey(180, 100));
}

#[test]
fn per_side_strokes_draw_only_their_sides() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let mut n = rect_node(20.0, 20.0, 100.0, 60.0, RED);
    n.strokes = vec![Paint::solid(BLUE)];
    n.stroke.sides = Some([0.0, 0.0, 6.0, 2.0]);
    n.stroke.weight = 6.0;
    add(&mut doc, page, n);
    let pix = render(&doc, 160, 120);
    let at = |x: u32, y: u32| pixel_rgba(&pix, x, y).unwrap();
    assert_eq!(at(70, 77), [0, 0, 255, 255], "bottom band");
    assert_eq!(at(21, 50), [0, 0, 255, 255], "left band");
    assert_eq!(at(70, 21), [255, 0, 0, 255], "no top band");
    assert_eq!(at(118, 50), [255, 0, 0, 255], "no right band");
    assert_eq!(at(70, 72), [255, 0, 0, 255], "the band is 6 thick");
}

#[test]
fn image_adjustments_change_what_is_drawn() {
    let mut img = deesainer_render::Pixmap::new(4, 4).unwrap();
    img.fill(tiny_skia::Color::from_rgba8(128, 128, 128, 255));
    let png = deesainer_render::encode_png(&img).unwrap();
    let mut doc = Document::new();
    let page = doc.first_page();
    let id = deesainer_doc::AssetId("abc123".into());
    doc.insert_asset(id.clone(), std::sync::Arc::new(png));
    let mut n = rect_node(0.0, 0.0, 40.0, 40.0, RED);
    n.fills = vec![Paint {
        kind: deesainer_doc::PaintKind::Image {
            asset: id,
            scale_mode: deesainer_doc::ImageScaleMode::Stretch,
            adjust: deesainer_doc::ImageAdjust {
                exposure: 0.5,
                ..Default::default()
            },
        },
        ..Paint::solid(RED)
    }];
    add(&mut doc, page, n);
    let pix = render(&doc, 40, 40);
    let c = pixel_rgba(&pix, 20, 20).unwrap();
    assert!(c[0] > 230 && c[0] == c[1] && c[1] == c[2], "{c:?}");
}
