mod code;

use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{
    BindTarget, CounterAlign, Document, Effect, ImageScaleMode, LayoutDirection, LineHeight, Node,
    NodeId, NodeKind, Paint, PaintKind, PrimaryAlign, Sizing, StrokeAlign, StyleId, StyleKind,
    TextAlignH, TextCase, TextDecoration, VarValue, VariableId,
};
use deesainer_geom::Rect;
use serde::Serialize;

pub use code::{code, Language};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub id: NodeId,
    pub name: String,
    pub kind: &'static str,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub size_limits: deesainer_doc::SizeLimits,
    pub page_bounds: Rect,
    pub rotation: f64,
    pub opacity: f32,
    pub blend: Option<&'static str>,
    pub corner_radius: Option<[f64; 4]>,
    pub fills: Vec<PaintSpec>,
    pub strokes: Vec<PaintSpec>,
    pub stroke: Option<StrokeSpec>,
    pub effects: Vec<EffectSpec>,
    pub text: Option<TextSpec>,
    pub layout: Option<LayoutSpec>,
    pub in_layout: Option<InLayout>,
    pub constraints: Option<[&'static str; 2]>,
    pub clips_content: bool,
    pub styles: Vec<StyleUse>,
    pub variables: Vec<VariableUse>,
    pub component: Option<ComponentSpec>,
    pub assets: Vec<AssetSpec>,
    pub annotations: Vec<deesainer_doc::Annotation>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintSpec {
    pub kind: &'static str,
    pub color: Option<String>,
    pub opacity: f32,
    pub stops: Vec<(f32, String)>,
    pub start: Option<[f64; 2]>,
    pub end: Option<[f64; 2]>,
    pub center: Option<[f64; 2]>,
    pub radius: Option<[f64; 2]>,
    pub angle: Option<f64>,
    pub scale_mode: Option<&'static str>,
    pub asset: Option<String>,
    pub blend: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeSpec {
    pub weight: f64,
    pub align: &'static str,
    pub dash: Vec<f64>,
    pub sides: Option<[f64; 4]>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectSpec {
    pub kind: &'static str,
    pub color: Option<String>,
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub spread: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpec {
    pub content: String,
    pub font_family: String,
    pub font_size: f64,
    pub font_weight: u16,
    pub italic: bool,
    pub line_height: Option<f64>,
    pub letter_spacing: f64,
    pub align: &'static str,
    pub decoration: Option<&'static str>,
    pub case: Option<&'static str>,
    pub styled_ranges: usize,
    pub features: Vec<(String, u32)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variations: Vec<(String, f32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSpec {
    pub direction: &'static str,
    pub wrap: bool,
    pub gap: f64,
    pub counter_gap: f64,
    pub padding: [f64; 4],
    pub primary_align: &'static str,
    pub counter_align: &'static str,
    pub primary_sizing: &'static str,
    pub counter_sizing: &'static str,
    pub grid: Option<(usize, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InLayout {
    pub direction: &'static str,
    pub horizontal: &'static str,
    pub vertical: &'static str,
    pub absolute: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleUse {
    pub property: &'static str,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableUse {
    pub property: &'static str,
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSpec {
    pub main: String,
    pub variant: Vec<(String, String)>,
    pub properties: Vec<String>,
    pub overrides: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSpec {
    pub asset: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measure {
    pub relation: &'static str,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub horizontal_gap: f64,
    pub vertical_gap: f64,
}

fn blend_name(b: BlendMode) -> Option<&'static str> {
    Some(match b {
        BlendMode::PassThrough | BlendMode::Normal => return None,
        BlendMode::Darken => "darken",
        BlendMode::Multiply => "multiply",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::Lighten => "lighten",
        BlendMode::Screen => "screen",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::Overlay => "overlay",
        BlendMode::SoftLight => "soft-light",
        BlendMode::HardLight => "hard-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
    })
}

fn sizing(s: Sizing) -> &'static str {
    match s {
        Sizing::Fixed => "fixed",
        Sizing::Hug => "hug",
        Sizing::Fill => "fill",
    }
}

fn hex(c: Rgba) -> String {
    c.to_hex()
}

pub fn effective_fills<'a>(doc: &'a Document, node: &'a Node) -> &'a [Paint] {
    linked_paints(doc, node.styles.fill).unwrap_or(&node.fills)
}

pub fn effective_strokes<'a>(doc: &'a Document, node: &'a Node) -> &'a [Paint] {
    linked_paints(doc, node.styles.stroke).unwrap_or(&node.strokes)
}

pub fn effective_effects<'a>(doc: &'a Document, node: &'a Node) -> &'a [Effect] {
    match node
        .styles
        .effect
        .and_then(|id| doc.style(id))
        .map(|s| &s.kind)
    {
        Some(StyleKind::Effect(e)) => e,
        _ => &node.effects,
    }
}

fn linked_paints(doc: &Document, id: Option<StyleId>) -> Option<&[Paint]> {
    match id.and_then(|id| doc.style(id)).map(|s| &s.kind) {
        Some(StyleKind::Paint(p)) => Some(p),
        _ => None,
    }
}

fn paint_spec(p: &Paint) -> PaintSpec {
    let mut spec = PaintSpec {
        kind: "solid",
        color: None,
        opacity: p.opacity,
        stops: Vec::new(),
        start: None,
        end: None,
        center: None,
        radius: None,
        angle: None,
        scale_mode: None,
        asset: None,
        blend: blend_name(p.blend),
    };
    match &p.kind {
        PaintKind::Solid { color } => spec.color = Some(hex(*color)),
        PaintKind::LinearGradient { start, end, stops } => {
            spec.kind = "linearGradient";
            spec.stops = stops.iter().map(|s| (s.position, hex(s.color))).collect();
            spec.start = Some([start.x, start.y]);
            spec.end = Some([end.x, end.y]);
        }
        PaintKind::RadialGradient {
            center,
            radius,
            stops,
        }
        | PaintKind::DiamondGradient {
            center,
            radius,
            stops,
        } => {
            spec.kind = if matches!(p.kind, PaintKind::RadialGradient { .. }) {
                "radialGradient"
            } else {
                "diamondGradient"
            };
            spec.stops = stops.iter().map(|s| (s.position, hex(s.color))).collect();
            spec.center = Some([center.x, center.y]);
            spec.radius = Some([radius.x, radius.y]);
        }
        PaintKind::AngularGradient {
            center,
            angle,
            stops,
        } => {
            spec.kind = "angularGradient";
            spec.stops = stops.iter().map(|s| (s.position, hex(s.color))).collect();
            spec.center = Some([center.x, center.y]);
            spec.angle = Some(*angle);
        }
        PaintKind::Image {
            asset, scale_mode, ..
        } => {
            spec.kind = "image";
            spec.asset = Some(asset.0.clone());
            spec.scale_mode = Some(match scale_mode {
                ImageScaleMode::Fill => "fill",
                ImageScaleMode::Fit => "fit",
                ImageScaleMode::Stretch => "stretch",
                ImageScaleMode::Tile => "tile",
            });
        }
    }
    spec
}

fn effect_spec(e: &Effect) -> Option<EffectSpec> {
    if !e.is_visible() {
        return None;
    }
    Some(match e {
        Effect::DropShadow {
            color,
            offset,
            blur,
            spread,
            ..
        }
        | Effect::InnerShadow {
            color,
            offset,
            blur,
            spread,
            ..
        } => EffectSpec {
            kind: if matches!(e, Effect::DropShadow { .. }) {
                "dropShadow"
            } else {
                "innerShadow"
            },
            color: Some(hex(*color)),
            x: offset.x,
            y: offset.y,
            blur: *blur,
            spread: *spread,
        },
        Effect::LayerBlur { radius, .. } | Effect::BackgroundBlur { radius, .. } => EffectSpec {
            kind: if matches!(e, Effect::LayerBlur { .. }) {
                "layerBlur"
            } else {
                "backgroundBlur"
            },
            color: None,
            x: 0.0,
            y: 0.0,
            blur: *radius,
            spread: 0.0,
        },
    })
}

fn text_spec(doc: &Document, node: &Node) -> Option<TextSpec> {
    let data = node.text()?;
    let style = match node
        .styles
        .text
        .and_then(|id| doc.style(id))
        .map(|s| &s.kind)
    {
        Some(StyleKind::Text(t)) => t,
        _ => &data.style,
    };
    Some(TextSpec {
        content: data.content.clone(),
        font_family: style.font_family.clone(),
        font_size: style.font_size,
        font_weight: style.font_weight,
        italic: style.italic,
        line_height: match style.line_height {
            LineHeight::Auto => None,
            LineHeight::Pixels(px) => Some(px),
            LineHeight::Percent(p) => Some(style.font_size * p / 100.0),
        },
        letter_spacing: style.letter_spacing,
        align: match data.align_h {
            TextAlignH::Left => "left",
            TextAlignH::Center => "center",
            TextAlignH::Right => "right",
            TextAlignH::Justified => "justify",
        },
        decoration: match style.decoration {
            TextDecoration::None => None,
            TextDecoration::Underline => Some("underline"),
            TextDecoration::Strikethrough => Some("line-through"),
        },
        case: match style.case {
            TextCase::Original => None,
            TextCase::Upper => Some("uppercase"),
            TextCase::Lower => Some("lowercase"),
            TextCase::Title => Some("capitalize"),
        },
        styled_ranges: data.runs.len(),
        features: data
            .style
            .features
            .iter()
            .filter(|f| f.is_valid())
            .map(|f| (f.tag.clone(), f.value))
            .collect(),
        variations: data
            .style
            .variations
            .iter()
            .filter(|v| v.is_valid())
            .map(|v| (v.tag.clone(), v.value))
            .collect(),
    })
}

fn variable_value(doc: &Document, id: VariableId) -> Option<String> {
    Some(match doc.resolve_variable(id)? {
        VarValue::Color(c) => hex(*c),
        VarValue::Number(n) => format!("{n}"),
        VarValue::String(s) => s.clone(),
        VarValue::Bool(b) => format!("{b}"),
        VarValue::Alias(_) => return None,
    })
}

fn bind_name(t: BindTarget) -> &'static str {
    match t {
        BindTarget::FillColor => "fillColor",
        BindTarget::StrokeColor => "strokeColor",
        BindTarget::Opacity => "opacity",
        BindTarget::CornerRadius => "cornerRadius",
        BindTarget::Width => "width",
        BindTarget::Height => "height",
        BindTarget::Gap => "gap",
        BindTarget::Padding => "padding",
        BindTarget::TextContent => "textContent",
        BindTarget::Visible => "visible",
    }
}

fn component_spec(doc: &Document, node: &Node) -> Option<ComponentSpec> {
    let (main, overrides) = match &node.kind {
        NodeKind::Instance {
            component,
            overrides,
        } => (doc.get(*component)?, overrides.len()),
        NodeKind::Component { .. } => (node, 0),
        _ => return None,
    };
    let in_set = main
        .parent
        .and_then(|p| doc.get(p))
        .is_some_and(|p| matches!(p.kind, NodeKind::ComponentSet));
    let variant = if in_set {
        main.name
            .split(',')
            .filter_map(|part| {
                let (k, v) = part.split_once('=')?;
                Some((k.trim().to_owned(), v.trim().to_owned()))
            })
            .collect()
    } else {
        Vec::new()
    };
    let name = if in_set {
        main.parent
            .and_then(|p| doc.get(p))
            .map_or_else(|| main.name.clone(), |set| set.name.clone())
    } else {
        main.name.clone()
    };
    let properties = match &main.kind {
        NodeKind::Component { properties, .. } => {
            properties.iter().map(|p| p.name.clone()).collect()
        }
        _ => Vec::new(),
    };
    Some(ComponentSpec {
        main: name,
        variant,
        properties,
        overrides,
    })
}

pub fn spec(doc: &Document, id: NodeId) -> Option<Spec> {
    let node = doc.get(id)?;
    let parent = node.parent.and_then(|p| doc.get(p));
    let fills = effective_fills(doc, node);
    let strokes = effective_strokes(doc, node);
    let r = &node.corner_radius;
    let radii = [r.top_left, r.top_right, r.bottom_right, r.bottom_left];
    let rounded = radii.iter().any(|v| *v != 0.0) && !matches!(node.kind, NodeKind::Ellipse { .. });
    let parent_layout = parent.and_then(|p| p.layout.as_ref());
    let mut styles = Vec::new();
    for (property, id) in [
        ("fill", node.styles.fill),
        ("stroke", node.styles.stroke),
        ("effect", node.styles.effect),
        ("text", node.styles.text),
    ] {
        if let Some(style) = id.and_then(|id| doc.style(id)) {
            styles.push(StyleUse {
                property,
                name: style.name.clone(),
            });
        }
    }
    let variables = node
        .bindings
        .iter()
        .filter_map(|(target, id)| {
            Some(VariableUse {
                property: bind_name(*target),
                name: doc.variable(*id)?.name.clone(),
                value: variable_value(doc, *id),
            })
        })
        .collect();
    let assets = fills
        .iter()
        .chain(strokes)
        .filter_map(|p| match &p.kind {
            PaintKind::Image { asset, .. } => Some(asset),
            _ => None,
        })
        .map(|asset| {
            let dims = doc
                .asset(asset)
                .and_then(|bytes| deesainer_render::images::dimensions(bytes));
            AssetSpec {
                asset: asset.0.clone(),
                width: dims.map(|d| d.0),
                height: dims.map(|d| d.1),
            }
        })
        .collect();
    let page_bounds = doc.world_transform(id).transform_rect_bounds(&Rect::new(
        0.0,
        0.0,
        node.rect.width,
        node.rect.height,
    ));
    let has_stroke = strokes.iter().any(|p| p.visible) && node.stroke.weight > 0.0;
    Some(Spec {
        id,
        name: node.name.clone(),
        kind: node.kind.type_name(),
        x: node.rect.x,
        y: node.rect.y,
        width: node.rect.width,
        size_limits: node.size_limits,
        height: node.rect.height,
        page_bounds,
        rotation: node.rotation,
        opacity: node.opacity,
        blend: blend_name(node.blend),
        corner_radius: rounded.then_some(radii),
        fills: fills.iter().filter(|p| p.visible).map(paint_spec).collect(),
        strokes: strokes
            .iter()
            .filter(|p| p.visible)
            .map(paint_spec)
            .collect(),
        stroke: has_stroke.then(|| StrokeSpec {
            weight: node.stroke.weight,
            align: match node.stroke.align {
                StrokeAlign::Inside => "inside",
                StrokeAlign::Center => "center",
                StrokeAlign::Outside => "outside",
            },
            dash: node.stroke.dash.clone(),
            sides: deesainer_render::side_weights(node),
        }),
        effects: effective_effects(doc, node)
            .iter()
            .filter_map(effect_spec)
            .collect(),
        text: text_spec(doc, node),
        layout: node.layout.as_ref().map(|l| LayoutSpec {
            direction: match l.direction {
                LayoutDirection::Horizontal => "row",
                LayoutDirection::Vertical => "column",
                LayoutDirection::Grid => "grid",
            },
            wrap: l.wrap,
            gap: l.gap,
            counter_gap: l.counter_gap,
            padding: [
                l.padding.top,
                l.padding.right,
                l.padding.bottom,
                l.padding.left,
            ],
            primary_align: match l.primary_align {
                PrimaryAlign::Start => "start",
                PrimaryAlign::Center => "center",
                PrimaryAlign::End => "end",
                PrimaryAlign::SpaceBetween => "spaceBetween",
            },
            counter_align: match l.counter_align {
                CounterAlign::Start => "start",
                CounterAlign::Center => "center",
                CounterAlign::End => "end",
            },
            primary_sizing: sizing(l.primary_sizing),
            counter_sizing: sizing(l.counter_sizing),
            grid: (l.direction == LayoutDirection::Grid).then(|| (l.columns(), l.grid_rows)),
        }),
        in_layout: parent_layout.map(|l| InLayout {
            direction: match l.direction {
                LayoutDirection::Horizontal => "row",
                LayoutDirection::Vertical => "column",
                LayoutDirection::Grid => "grid",
            },
            horizontal: sizing(node.layout_child.horizontal),
            vertical: sizing(node.layout_child.vertical),
            absolute: node.layout_child.absolute,
        }),
        constraints: (parent_layout.is_none() && parent.is_some_and(|p| p.kind.is_frame_like()))
            .then(|| {
                let c = |c: deesainer_doc::Constraint, min: &'static str, max: &'static str| match c
                {
                    deesainer_doc::Constraint::Min => min,
                    deesainer_doc::Constraint::Max => max,
                    deesainer_doc::Constraint::Center => "center",
                    deesainer_doc::Constraint::Stretch => "stretch",
                    deesainer_doc::Constraint::Scale => "scale",
                };
                [
                    c(node.constraints.horizontal, "left", "right"),
                    c(node.constraints.vertical, "top", "bottom"),
                ]
            }),
        clips_content: node.clips_content && node.kind.is_frame_like(),
        styles,
        variables,
        component: component_spec(doc, node),
        assets,
        annotations: node.annotations.clone(),
    })
}

pub fn measure(doc: &Document, a: NodeId, b: NodeId) -> Option<Measure> {
    let bounds = |id: NodeId| {
        let n = doc.get(id)?;
        Some(doc.world_transform(id).transform_rect_bounds(&Rect::new(
            0.0,
            0.0,
            n.rect.width,
            n.rect.height,
        )))
    };
    let (p, q) = (bounds(a)?, bounds(b)?);
    let contains = |o: &Rect, i: &Rect| {
        i.x >= o.x && i.y >= o.y && i.max_x() <= o.max_x() && i.max_y() <= o.max_y()
    };
    let margins = |o: &Rect, i: &Rect, relation| Measure {
        relation,
        left: i.x - o.x,
        top: i.y - o.y,
        right: o.max_x() - i.max_x(),
        bottom: o.max_y() - i.max_y(),
        horizontal_gap: 0.0,
        vertical_gap: 0.0,
    };
    if contains(&q, &p) {
        return Some(margins(&q, &p, "inside"));
    }
    if contains(&p, &q) {
        return Some(margins(&p, &q, "contains"));
    }
    let hgap = (q.x - p.max_x()).max(p.x - q.max_x()).max(0.0);
    let vgap = (q.y - p.max_y()).max(p.y - q.max_y()).max(0.0);
    Some(Measure {
        relation: if hgap > 0.0 || vgap > 0.0 {
            "apart"
        } else {
            "overlap"
        },
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        horizontal_gap: hgap,
        vertical_gap: vgap,
    })
}

#[cfg(test)]
mod tests;
