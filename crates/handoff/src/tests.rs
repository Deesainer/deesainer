use super::*;
use crate::code::{code, Language};
use deesainer_doc::{
    AutoLayout, ColorStop, CornerRadius, Padding, Style, TextData, TextStyle, Variable,
};
use deesainer_geom::Point;

fn add(doc: &mut Document, parent: NodeId, id: u64, kind: NodeKind, rect: Rect) -> NodeId {
    let at = doc.get(parent).map_or(0, |p| p.children.len());
    doc.insert(
        Node::new(NodeId(id), format!("Layer {id}"), kind, rect),
        parent,
        at,
    )
    .unwrap();
    NodeId(id)
}

fn blue() -> Paint {
    Paint::solid(Rgba::new(0.0, 0.4, 1.0, 1.0))
}

fn css(doc: &Document, id: NodeId) -> String {
    code(doc, id, Language::Css).unwrap()
}

#[test]
fn a_card_with_a_style_and_a_variable() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let card = add(
        &mut doc,
        page,
        10,
        NodeKind::Rectangle,
        Rect::new(20.0, 30.0, 200.0, 120.0),
    );
    doc.set_style(
        StyleId(1),
        Some(Style {
            origin: None,
            name: "Brand/Primary".into(),
            description: String::new(),
            kind: StyleKind::Paint(vec![blue()]),
        }),
    );
    doc.set_variable(
        VariableId(1),
        Some(Variable {
            name: "Radius/Card".into(),
            values: vec![VarValue::Number(12.0)],
            ..Default::default()
        }),
    )
    .unwrap();
    {
        let n = doc.get_mut(card).unwrap();
        n.fills = vec![Paint::solid(Rgba::new(1.0, 0.0, 0.0, 1.0))];
        n.styles.fill = Some(StyleId(1));
        n.corner_radius = CornerRadius::uniform(12.0);
        n.bindings.insert(BindTarget::CornerRadius, VariableId(1));
        n.opacity = 0.5;
        n.effects = vec![Effect::DropShadow {
            color: Rgba::new(0.0, 0.0, 0.0, 0.25),
            offset: Point::new(0.0, 4.0),
            blur: 16.0,
            spread: 0.0,
            visible: true,
        }];
    }
    let s = spec(&doc, card).unwrap();
    assert_eq!(s.fills[0].color.as_deref(), Some("#0066FF"));
    assert_eq!(
        s.styles,
        vec![StyleUse {
            property: "fill",
            name: "Brand/Primary".into()
        }]
    );
    assert_eq!(s.variables[0].name, "Radius/Card");
    assert_eq!(s.variables[0].value.as_deref(), Some("12"));
    assert_eq!(s.corner_radius, Some([12.0; 4]));
    let css = css(&doc, card);
    for line in [
        "/* fill style: Brand/Primary */",
        "position: absolute;",
        "left: 20px;",
        "top: 30px;",
        "width: 200px;",
        "height: 120px;",
        "border-radius: var(--radius-card, 12px);",
        "background: #0066FF;",
        "box-shadow: 0px 4px 16px 0px rgba(0, 0, 0, 0.25);",
        "opacity: 0.5;",
    ] {
        assert!(css.contains(line), "missing {line:?} in\n{css}");
    }
}

#[test]
fn gradients_map_to_the_css_gradient_line() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let r = add(
        &mut doc,
        page,
        10,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 200.0, 100.0),
    );
    let stops = vec![
        ColorStop {
            position: 0.0,
            color: Rgba::new(1.0, 0.0, 0.0, 1.0),
        },
        ColorStop {
            position: 1.0,
            color: Rgba::new(0.0, 0.0, 1.0, 1.0),
        },
    ];
    doc.get_mut(r).unwrap().fills = vec![Paint::linear(
        Point::new(0.0, 0.5),
        Point::new(1.0, 0.5),
        stops.clone(),
    )];
    assert!(
        css(&doc, r).contains("background: linear-gradient(90deg, #FF0000 0%, #0000FF 100%);"),
        "{}",
        css(&doc, r)
    );
    doc.get_mut(r).unwrap().fills = vec![Paint::linear(
        Point::new(0.25, 0.5),
        Point::new(0.75, 0.5),
        stops.clone(),
    )];
    assert!(
        css(&doc, r).contains("linear-gradient(90deg, #FF0000 25%, #0000FF 75%)"),
        "{}",
        css(&doc, r)
    );
    doc.get_mut(r).unwrap().fills = vec![Paint::linear(
        Point::new(0.5, 0.0),
        Point::new(0.5, 1.0),
        stops,
    )];
    assert!(
        css(&doc, r).contains("linear-gradient(180deg"),
        "{}",
        css(&doc, r)
    );
}

#[test]
fn angular_gradients_map_to_css_conic_gradients() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let r = add(
        &mut doc,
        page,
        10,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 200.0, 100.0),
    );
    doc.get_mut(r).unwrap().fills = vec![Paint {
        kind: deesainer_doc::PaintKind::AngularGradient {
            center: Point::new(0.25, 0.5),
            angle: 0.0,
            stops: vec![
                ColorStop {
                    position: 0.0,
                    color: Rgba::new(1.0, 0.0, 0.0, 1.0),
                },
                ColorStop {
                    position: 1.0,
                    color: Rgba::new(0.0, 0.0, 1.0, 1.0),
                },
            ],
        },
        ..blue()
    }];
    assert!(
        css(&doc, r).contains("conic-gradient(from 90deg at 25% 50%, #FF0000 0%, #0000FF 100%)"),
        "{}",
        css(&doc, r)
    );
}

#[test]
fn auto_layout_and_children_in_the_flow() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let row = add(
        &mut doc,
        page,
        10,
        NodeKind::Frame,
        Rect::new(0.0, 0.0, 300.0, 60.0),
    );
    doc.get_mut(row).unwrap().layout = Some(AutoLayout {
        direction: LayoutDirection::Horizontal,
        gap: 8.0,
        padding: Padding {
            top: 12.0,
            right: 16.0,
            bottom: 12.0,
            left: 16.0,
        },
        primary_align: PrimaryAlign::SpaceBetween,
        counter_align: CounterAlign::Center,
        ..AutoLayout::default()
    });
    let child = add(
        &mut doc,
        row,
        11,
        NodeKind::Rectangle,
        Rect::new(16.0, 12.0, 40.0, 36.0),
    );
    doc.get_mut(child).unwrap().layout_child.horizontal = Sizing::Fill;
    let css_row = css(&doc, row);
    for line in [
        "display: flex;",
        "flex-direction: row;",
        "padding: 12px 16px 12px 16px;",
        "justify-content: space-between;",
        "align-items: center;",
    ] {
        assert!(css_row.contains(line), "missing {line:?} in\n{css_row}");
    }
    assert!(!css_row.contains("gap:"), "{css_row}");
    let css_child = css(&doc, child);
    assert!(css_child.contains("flex: 1 0 0;"), "{css_child}");
    assert!(
        !css_child.contains("position: absolute;"),
        "a flowed child has no position: {css_child}"
    );
    let swift = code(&doc, row, Language::SwiftUi).unwrap();
    assert!(
        swift.contains("HStack(alignment: .center, spacing: 8)"),
        "{swift}"
    );
    let kotlin = code(&doc, row, Language::Compose).unwrap();
    assert!(kotlin.contains("Row("), "{kotlin}");
    assert!(
        kotlin.contains("horizontalArrangement = Arrangement.SpaceBetween"),
        "{kotlin}"
    );
    assert!(
        kotlin.contains("verticalAlignment = Alignment.CenterVertically"),
        "{kotlin}"
    );
}

#[test]
fn text_typography() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let data = TextData {
        content: "Hello \"world\"".into(),
        style: TextStyle {
            font_family: "Inter".into(),
            font_size: 18.0,
            font_weight: 600,
            letter_spacing: 0.5,
            line_height: LineHeight::Percent(150.0),
            decoration: TextDecoration::Underline,
            case: TextCase::Upper,
            ..TextStyle::default()
        },
        align_h: TextAlignH::Center,
        ..TextData::default()
    };
    let t = add(
        &mut doc,
        page,
        10,
        NodeKind::Text(data),
        Rect::new(0.0, 0.0, 120.0, 27.0),
    );
    doc.get_mut(t).unwrap().fills = vec![blue()];
    let s = spec(&doc, t).unwrap();
    assert_eq!(s.text.as_ref().map(|t| t.line_height), Some(Some(27.0)));
    let css = css(&doc, t);
    for line in [
        "font-family: \"Inter\";",
        "font-size: 18px;",
        "font-weight: 600;",
        "line-height: 27px;",
        "letter-spacing: 0.5px;",
        "text-align: center;",
        "text-decoration: underline;",
        "text-transform: uppercase;",
        "color: #0066FF;",
    ] {
        assert!(css.contains(line), "missing {line:?} in\n{css}");
    }
    assert!(!css.contains("background:"), "{css}");
    let swift = code(&doc, t, Language::SwiftUi).unwrap();
    assert!(
        swift.contains("Text(\"Hello \\\"world\\\"\")"),
        "quotes are escaped: {swift}"
    );
    assert!(swift.contains(".weight(.semibold)"), "{swift}");
    let kotlin = code(&doc, t, Language::Compose).unwrap();
    assert!(kotlin.contains("fontWeight = FontWeight(600)"), "{kotlin}");
    assert!(kotlin.contains("textAlign = TextAlign.Center"), "{kotlin}");
}

#[test]
fn instances_name_their_component_and_variant() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let set = add(
        &mut doc,
        page,
        10,
        NodeKind::ComponentSet,
        Rect::new(0.0, 0.0, 300.0, 100.0),
    );
    doc.get_mut(set).unwrap().name = "Button".into();
    let main = add(
        &mut doc,
        set,
        11,
        NodeKind::Component {
            description: String::new(),
            properties: Vec::new(),
        },
        Rect::new(0.0, 0.0, 100.0, 40.0),
    );
    doc.get_mut(main).unwrap().name = "Size=Large, State=Hover".into();
    let inst = add(
        &mut doc,
        page,
        12,
        NodeKind::Instance {
            component: main,
            overrides: Vec::new(),
        },
        Rect::new(0.0, 200.0, 100.0, 40.0),
    );
    let c = spec(&doc, inst).unwrap().component.unwrap();
    assert_eq!(c.main, "Button");
    assert_eq!(
        c.variant,
        vec![
            ("Size".into(), "Large".into()),
            ("State".into(), "Hover".into())
        ]
    );
}

#[test]
fn redlines_between_layers() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let a = add(
        &mut doc,
        page,
        10,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 50.0, 50.0),
    );
    let b = add(
        &mut doc,
        page,
        11,
        NodeKind::Rectangle,
        Rect::new(80.0, 10.0, 50.0, 50.0),
    );
    let m = measure(&doc, a, b).unwrap();
    assert_eq!(
        (m.relation, m.horizontal_gap, m.vertical_gap),
        ("apart", 30.0, 0.0)
    );
    let outer = add(
        &mut doc,
        page,
        12,
        NodeKind::Frame,
        Rect::new(-10.0, -20.0, 200.0, 100.0),
    );
    let m = measure(&doc, a, outer).unwrap();
    assert_eq!(m.relation, "inside");
    assert_eq!(
        (m.left, m.top, m.right, m.bottom),
        (10.0, 20.0, 140.0, 30.0)
    );
    let m = measure(&doc, outer, a).unwrap();
    assert_eq!(m.relation, "contains");
    assert!(measure(&doc, a, NodeId(999)).is_none());
}

#[test]
fn unknown_layers_and_odd_values_never_panic() {
    let mut doc = Document::new();
    let page = doc.first_page();
    assert!(spec(&doc, NodeId(999)).is_none());
    assert!(code(&doc, NodeId(999), Language::Css).is_none());
    let r = add(
        &mut doc,
        page,
        10,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 0.0, 0.0),
    );
    doc.get_mut(r).unwrap().fills = vec![Paint::linear(
        Point::new(0.5, 0.5),
        Point::new(0.5, 0.5),
        Vec::new(),
    )];
    for lang in [Language::Css, Language::SwiftUi, Language::Compose] {
        let out = code(&doc, r, lang).unwrap();
        assert!(!out.contains("NaN") && !out.contains("inf"), "{out}");
    }
}

#[test]
fn css_variable_names() {
    assert_eq!(
        crate::code::css_var_for_tests("Brand/Primary 500"),
        "--brand-primary-500"
    );
    assert_eq!(
        crate::code::css_var_for_tests("  spacing//xl "),
        "--spacing-xl"
    );
}

#[test]
fn size_limits_become_css_min_and_max() {
    let mut doc = Document::new();
    let page = doc.first_page();
    let r = add(
        &mut doc,
        page,
        10,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 120.0, 40.0),
    );
    doc.get_mut(r).unwrap().size_limits = deesainer_doc::SizeLimits {
        min_width: Some(80.0),
        max_width: Some(240.0),
        ..Default::default()
    };
    let out = css(&doc, r);
    assert!(out.contains("min-width: 80px;"), "{out}");
    assert!(out.contains("max-width: 240px;"), "{out}");
    assert!(!out.contains("min-height"), "{out}");
}
