use std::collections::BTreeMap;
use std::sync::Arc;

use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{
    AutoLayout, BoolOp, ColorStop, Constraint, Constraints, CornerRadius, CounterAlign, Document,
    Effect, FillRule, ImageScaleMode, LayoutDirection, LineHeight, Node, NodeId, NodeKind, Padding,
    Paint, PaintKind, PathCmd, PrimaryAlign, Sizing, StrokeAlign, StrokeCap, StrokeJoin,
    TextAlignH, TextAlignV, TextAutoResize, TextCase, TextData, TextDecoration, VectorPath,
};
use deesainer_geom::{Point, Rect};
use serde_json::Value;

use crate::{FigReport, IoError};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn num(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(d)
}

fn color(c: &Value) -> Rgba {
    Rgba::new(
        num(c, "r", 0.0) as f32,
        num(c, "g", 0.0) as f32,
        num(c, "b", 0.0) as f32,
        num(c, "a", 1.0) as f32,
    )
}

fn blend(name: &str) -> BlendMode {
    match name {
        "PASS_THROUGH" => BlendMode::PassThrough,
        "DARKEN" => BlendMode::Darken,
        "MULTIPLY" => BlendMode::Multiply,
        "COLOR_BURN" => BlendMode::ColorBurn,
        "LIGHTEN" => BlendMode::Lighten,
        "SCREEN" => BlendMode::Screen,
        "COLOR_DODGE" => BlendMode::ColorDodge,
        "OVERLAY" => BlendMode::Overlay,
        "SOFT_LIGHT" => BlendMode::SoftLight,
        "HARD_LIGHT" => BlendMode::HardLight,
        "DIFFERENCE" => BlendMode::Difference,
        "EXCLUSION" => BlendMode::Exclusion,
        "HUE" => BlendMode::Hue,
        "SATURATION" => BlendMode::Saturation,
        "COLOR" => BlendMode::Color,
        "LUMINOSITY" => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

pub type ImageBytes = BTreeMap<String, Vec<u8>>;

struct Ctx<'a> {
    images: &'a ImageBytes,
    ids: BTreeMap<String, NodeId>,
    report: FigReport,
}

pub fn import_figma_json(
    file: &Value,
    images: &ImageBytes,
) -> Result<(Document, FigReport), IoError> {
    let root = file
        .get("document")
        .ok_or_else(|| IoError::Other("Figma JSON: no document".into()))?;
    let mut doc = Document::new();
    let mut ctx = Ctx {
        images,
        ids: BTreeMap::new(),
        report: FigReport::default(),
    };
    let mut next = doc.next_free_id().0;
    assign_ids(root, &mut ctx.ids, &mut next, 0);
    let first = doc.first_page();
    let pages: Vec<&Value> = root
        .get("children")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter(|p| s(p, "type") == "CANVAS").collect())
        .unwrap_or_default();
    for (i, page) in pages.iter().enumerate() {
        let page_id = if i == 0 {
            first
        } else {
            let id = ctx.ids.get(s(page, "id")).copied().unwrap_or(NodeId(next));
            next += 1;
            let at = doc.pages().len();
            doc.insert(
                Node::new(id, "", NodeKind::Page, Rect::default()),
                doc.root(),
                at,
            )
            .map_err(|e| IoError::Other(e.to_string()))?;
            id
        };
        if let Some(p) = doc.get_mut(page_id) {
            p.name = s(page, "name").to_owned();
            if let Some(c) = page.get("backgroundColor") {
                p.fills = vec![Paint::solid(color(c))];
            }
        }
        ctx.report.pages += 1;
        for child in page
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            add(&mut ctx, &mut doc, child, page_id, None, 0)?;
        }
    }
    for bytes in images.values() {
        doc.insert_asset(deesainer_format::asset_id(bytes), Arc::new(bytes.clone()));
    }
    Ok((doc, ctx.report))
}

fn assign_ids(v: &Value, ids: &mut BTreeMap<String, NodeId>, next: &mut u64, depth: usize) {
    if depth > 256 {
        return;
    }
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        if s(v, "type") != "DOCUMENT" {
            ids.insert(id.to_owned(), NodeId(*next));
            *next += 1;
        }
    }
    for c in v
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        assign_ids(c, ids, next, depth + 1);
    }
}

fn paint(ctx: &Ctx<'_>, p: &Value) -> Option<Paint> {
    let stops = || -> Vec<ColorStop> {
        p.get("gradientStops")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|st| ColorStop {
                position: num(st, "position", 0.0) as f32,
                color: st.get("color").map_or(Rgba::BLACK, color),
            })
            .collect()
    };
    let handle = |i: usize, d: (f64, f64)| -> Point {
        p.get("gradientHandlePositions")
            .and_then(Value::as_array)
            .and_then(|a| a.get(i))
            .map_or(Point::new(d.0, d.1), |h| {
                Point::new(num(h, "x", d.0), num(h, "y", d.1))
            })
    };
    let kind = match s(p, "type") {
        "SOLID" => PaintKind::Solid {
            color: p.get("color").map_or(Rgba::BLACK, color),
        },
        "GRADIENT_LINEAR" => PaintKind::LinearGradient {
            start: handle(0, (0.5, 0.0)),
            end: handle(1, (0.5, 1.0)),
            stops: stops(),
        },
        kind @ ("GRADIENT_RADIAL" | "GRADIENT_ANGULAR" | "GRADIENT_DIAMOND") => {
            let c = handle(0, (0.5, 0.5));
            let a = handle(1, (1.0, 0.5));
            let b = handle(2, (0.5, 1.0));
            let radius = Point::new(
                ((a.x - c.x).powi(2) + (a.y - c.y).powi(2)).sqrt(),
                ((b.x - c.x).powi(2) + (b.y - c.y).powi(2)).sqrt(),
            );
            match kind {
                "GRADIENT_ANGULAR" => PaintKind::AngularGradient {
                    center: c,
                    angle: (a.y - c.y).atan2(a.x - c.x).to_degrees().rem_euclid(360.0),
                    stops: stops(),
                },
                "GRADIENT_DIAMOND" => PaintKind::DiamondGradient {
                    center: c,
                    radius,
                    stops: stops(),
                },
                _ => PaintKind::RadialGradient {
                    center: c,
                    radius,
                    stops: stops(),
                },
            }
        }
        "IMAGE" => {
            let bytes = ctx.images.get(s(p, "imageRef"))?;
            PaintKind::Image {
                asset: deesainer_format::asset_id(bytes),
                scale_mode: match s(p, "scaleMode") {
                    "FIT" => ImageScaleMode::Fit,
                    "TILE" => ImageScaleMode::Tile,
                    "STRETCH" => ImageScaleMode::Stretch,
                    _ => ImageScaleMode::Fill,
                },
                adjust: Default::default(),
            }
        }
        _ => return None,
    };
    Some(Paint {
        kind,
        opacity: num(p, "opacity", 1.0) as f32,
        visible: p.get("visible").and_then(Value::as_bool).unwrap_or(true),
        blend: blend(s(p, "blendMode")),
    })
}

fn paints(ctx: &Ctx<'_>, v: &Value, key: &str) -> Vec<Paint> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| paint(ctx, p))
        .collect()
}

fn effects(v: &Value) -> Vec<Effect> {
    v.get("effects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let visible = e.get("visible").and_then(Value::as_bool).unwrap_or(true);
            let c = e.get("color").map_or(Rgba::new(0.0, 0.0, 0.0, 0.25), color);
            let offset = e.get("offset").map_or(Point::new(0.0, 0.0), |o| {
                Point::new(num(o, "x", 0.0), num(o, "y", 0.0))
            });
            let blur = num(e, "radius", 0.0);
            let spread = num(e, "spread", 0.0);
            Some(match s(e, "type") {
                "DROP_SHADOW" => Effect::DropShadow {
                    color: c,
                    offset,
                    blur,
                    spread,
                    visible,
                },
                "INNER_SHADOW" => Effect::InnerShadow {
                    color: c,
                    offset,
                    blur,
                    spread,
                    visible,
                },
                "LAYER_BLUR" => Effect::LayerBlur {
                    radius: blur,
                    visible,
                },
                "BACKGROUND_BLUR" => Effect::BackgroundBlur {
                    radius: blur,
                    visible,
                },
                "GLASS" => Effect::BackgroundBlur {
                    radius: 20.0,
                    visible,
                },
                _ => return None,
            })
        })
        .collect()
}

fn svg_path(d: &str) -> Vec<PathCmd> {
    let mut out = Vec::new();
    let mut nums: Vec<f64> = Vec::new();
    let mut cmd = ' ';
    let mut cur = Point::new(0.0, 0.0);
    let mut start = cur;
    let mut invalid_number = false;
    let flush = |cmd: char,
                 nums: &mut Vec<f64>,
                 out: &mut Vec<PathCmd>,
                 cur: &mut Point,
                 start: &mut Point,
                 invalid: &mut bool| {
        let relative = cmd.is_ascii_lowercase();
        let command = cmd.to_ascii_uppercase();
        let n = match command {
            'M' | 'L' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'Q' => 4,
            _ => 0,
        };
        if n == 0 {
            nums.clear();
            return;
        }
        let mut first = true;
        for c in nums.chunks(n) {
            if c.len() < n {
                break;
            }
            let cmdv = if command == 'M' && !first {
                'L'
            } else {
                command
            };
            first = false;
            let anchor = *cur;
            let point = |x, y| {
                let p = Point::new(x, y);
                if relative {
                    anchor + p
                } else {
                    p
                }
            };
            match cmdv {
                'M' => {
                    let p = point(c[0], c[1]);
                    if !p.is_finite() {
                        *invalid = true;
                        break;
                    }
                    *cur = p;
                    *start = p;
                    out.push(PathCmd::MoveTo { to: *cur });
                }
                'L' => {
                    let p = point(c[0], c[1]);
                    if !p.is_finite() {
                        *invalid = true;
                        break;
                    }
                    *cur = p;
                    out.push(PathCmd::LineTo { to: *cur });
                }
                'H' => {
                    let x = if relative { cur.x + c[0] } else { c[0] };
                    if !x.is_finite() {
                        *invalid = true;
                        break;
                    }
                    *cur = Point::new(x, cur.y);
                    out.push(PathCmd::LineTo { to: *cur });
                }
                'V' => {
                    let y = if relative { cur.y + c[0] } else { c[0] };
                    if !y.is_finite() {
                        *invalid = true;
                        break;
                    }
                    *cur = Point::new(cur.x, y);
                    out.push(PathCmd::LineTo { to: *cur });
                }
                'C' => {
                    let ctrl1 = point(c[0], c[1]);
                    let ctrl2 = point(c[2], c[3]);
                    let to = point(c[4], c[5]);
                    if !ctrl1.is_finite() || !ctrl2.is_finite() || !to.is_finite() {
                        *invalid = true;
                        break;
                    }
                    *cur = to;
                    out.push(PathCmd::CubicTo {
                        ctrl1,
                        ctrl2,
                        to: *cur,
                    });
                }
                _ => {
                    let ctrl = point(c[0], c[1]);
                    let to = point(c[2], c[3]);
                    if !ctrl.is_finite() || !to.is_finite() {
                        *invalid = true;
                        break;
                    }
                    *cur = to;
                    out.push(PathCmd::QuadTo { ctrl, to: *cur });
                }
            }
        }
        nums.clear();
    };
    let mut token = String::new();
    let push_token = |token: &mut String, nums: &mut Vec<f64>, invalid: &mut bool| {
        if !token.is_empty() {
            match token.parse::<f64>() {
                Ok(v) if v.is_finite() => nums.push(v),
                _ => *invalid = true,
            }
            token.clear();
        }
    };
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() && ch != 'e' && ch != 'E' {
            push_token(&mut token, &mut nums, &mut invalid_number);
            flush(
                cmd,
                &mut nums,
                &mut out,
                &mut cur,
                &mut start,
                &mut invalid_number,
            );
            cmd = ch;
            if cmd.eq_ignore_ascii_case(&'Z') {
                out.push(PathCmd::Close);
                cur = start;
            }
        } else if ch == ',' || ch.is_whitespace() {
            push_token(&mut token, &mut nums, &mut invalid_number);
        } else if matches!(ch, '-' | '+') && !token.is_empty() && !token.ends_with(['e', 'E']) {
            push_token(&mut token, &mut nums, &mut invalid_number);
            token.push(ch);
        } else {
            token.push(ch);
        }
    }
    push_token(&mut token, &mut nums, &mut invalid_number);
    flush(
        cmd,
        &mut nums,
        &mut out,
        &mut cur,
        &mut start,
        &mut invalid_number,
    );
    if invalid_number {
        Vec::new()
    } else {
        out
    }
}

fn geometry(v: &Value, key: &str) -> Vec<PathCmd> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|g| svg_path(s(g, "path")))
        .collect()
}

fn constraint(name: &str) -> Constraint {
    match name {
        "RIGHT" | "BOTTOM" => Constraint::Max,
        "CENTER" => Constraint::Center,
        "LEFT_RIGHT" | "TOP_BOTTOM" => Constraint::Stretch,
        "SCALE" => Constraint::Scale,
        _ => Constraint::Min,
    }
}

fn text(v: &Value) -> TextData {
    let mut t = TextData {
        content: s(v, "characters").to_owned(),
        ..TextData::default()
    };
    let st = v.get("style").cloned().unwrap_or(Value::Null);
    let size = num(&st, "fontSize", 12.0);
    t.style.font_size = size;
    if let Some(f) = st.get("fontFamily").and_then(Value::as_str) {
        t.style.font_family = f.to_owned();
    }
    t.style.font_weight = num(&st, "fontWeight", 400.0).clamp(100.0, 900.0) as u16;
    t.style.italic = st.get("italic").and_then(Value::as_bool).unwrap_or(false);
    t.style.line_height = match s(&st, "lineHeightUnit") {
        "PIXELS" => LineHeight::Pixels(num(&st, "lineHeightPx", size * 1.2)),
        "FONT_SIZE_%" => LineHeight::Percent(num(&st, "lineHeightPercentFontSize", 100.0)),
        _ => LineHeight::Auto,
    };
    let ls = num(&st, "letterSpacing", 0.0);
    t.style.letter_spacing = if size > 0.0 { ls / size * 100.0 } else { 0.0 };
    t.style.decoration = match s(&st, "textDecoration") {
        "UNDERLINE" => TextDecoration::Underline,
        "STRIKETHROUGH" => TextDecoration::Strikethrough,
        _ => TextDecoration::None,
    };
    t.style.case = match s(&st, "textCase") {
        "UPPER" => TextCase::Upper,
        "LOWER" => TextCase::Lower,
        "TITLE" => TextCase::Title,
        _ => TextCase::Original,
    };
    t.align_h = match s(&st, "textAlignHorizontal") {
        "CENTER" => TextAlignH::Center,
        "RIGHT" => TextAlignH::Right,
        "JUSTIFIED" => TextAlignH::Justified,
        _ => TextAlignH::Left,
    };
    t.align_v = match s(&st, "textAlignVertical") {
        "CENTER" => TextAlignV::Center,
        "BOTTOM" => TextAlignV::Bottom,
        _ => TextAlignV::Top,
    };
    t.auto_resize = match s(&st, "textAutoResize") {
        "WIDTH_AND_HEIGHT" => TextAutoResize::WidthAndHeight,
        "HEIGHT" => TextAutoResize::Height,
        _ => TextAutoResize::None,
    };
    t
}

fn bbox(v: &Value) -> Option<Rect> {
    let b = v.get("absoluteBoundingBox")?;
    Some(Rect::new(
        num(b, "x", 0.0),
        num(b, "y", 0.0),
        num(b, "width", 0.0),
        num(b, "height", 0.0),
    ))
}

fn add(
    ctx: &mut Ctx<'_>,
    doc: &mut Document,
    v: &Value,
    parent: NodeId,
    parent_abs: Option<Rect>,
    depth: usize,
) -> Result<(), IoError> {
    if depth > 200 {
        return Ok(());
    }
    let Some(id) = v
        .get("id")
        .and_then(Value::as_str)
        .and_then(|i| ctx.ids.get(i).copied())
    else {
        return Ok(());
    };
    let ty = s(v, "type");
    let abs = bbox(v);
    let (w, h) = match v.get("size") {
        Some(sz) => (num(sz, "x", 0.0), num(sz, "y", 0.0)),
        None => abs.map_or((0.0, 0.0), |b| (b.width, b.height)),
    };
    let in_group = doc
        .get(parent)
        .is_some_and(|p| matches!(p.kind, NodeKind::Group | NodeKind::BooleanOp { .. }));
    let transform = if in_group {
        None
    } else {
        v.get("relativeTransform").and_then(Value::as_array)
    };
    let (rect, rotation) = match transform {
        Some(rows) if rows.len() == 2 => {
            let r0: Vec<f64> = rows[0]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_f64)
                .collect();
            let r1: Vec<f64> = rows[1]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_f64)
                .collect();
            if r0.len() == 3 && r1.len() == 3 {
                let cx = r0[0] * w / 2.0 + r0[1] * h / 2.0 + r0[2];
                let cy = r1[0] * w / 2.0 + r1[1] * h / 2.0 + r1[2];
                (
                    Rect::new(cx - w / 2.0, cy - h / 2.0, w, h),
                    r1[0].atan2(r0[0]).to_degrees(),
                )
            } else {
                (Rect::new(0.0, 0.0, w, h), 0.0)
            }
        }
        _ => {
            let (px, py) = parent_abs.map_or((0.0, 0.0), |p| (p.x, p.y));
            let b = abs.unwrap_or(Rect::new(0.0, 0.0, w, h));
            (Rect::new(b.x - px, b.y - py, w, h), 0.0)
        }
    };
    let fills = paints(ctx, v, "fills");
    let kind = match ty {
        "FRAME" | "SECTION" => NodeKind::Frame,
        "GROUP" => NodeKind::Group,
        "COMPONENT" => NodeKind::Component {
            description: s(v, "description").to_owned(),
            properties: Vec::new(),
        },
        "COMPONENT_SET" => NodeKind::ComponentSet,
        "INSTANCE" => match ctx.ids.get(s(v, "componentId")).copied() {
            Some(component) => NodeKind::Instance {
                component,
                overrides: Vec::new(),
            },
            None => NodeKind::Frame,
        },
        "RECTANGLE" => NodeKind::Rectangle,
        "ELLIPSE" => {
            let arc = v.get("arcData").cloned().unwrap_or(Value::Null);
            NodeKind::Ellipse {
                arc_start: num(&arc, "startingAngle", 0.0).to_degrees(),
                arc_end: num(&arc, "endingAngle", std::f64::consts::TAU).to_degrees(),
                inner_ratio: num(&arc, "innerRadius", 0.0),
            }
        }
        "LINE" => NodeKind::Line,
        "TEXT" => NodeKind::Text(text(v)),
        "BOOLEAN_OPERATION" => NodeKind::BooleanOp {
            op: match s(v, "booleanOperation") {
                "SUBTRACT" => BoolOp::Subtract,
                "INTERSECT" => BoolOp::Intersect,
                "EXCLUDE" => BoolOp::Exclude,
                _ => BoolOp::Union,
            },
        },
        "VECTOR" | "STAR" | "REGULAR_POLYGON" => {
            let mut cmds = geometry(v, "fillGeometry");
            if cmds.is_empty() {
                cmds = geometry(v, "strokeGeometry");
            }
            NodeKind::Vector {
                path: VectorPath {
                    cmds,
                    fill_rule: FillRule::NonZero,
                },
            }
        }
        other => {
            *ctx.report.skipped.entry(other.to_owned()).or_default() += 1;
            return Ok(());
        }
    };
    let mut node = Node::new(id, s(v, "name"), kind, rect);
    node.rotation = if rotation.abs() < 1e-6 { 0.0 } else { rotation };
    node.visible = v.get("visible").and_then(Value::as_bool).unwrap_or(true);
    node.locked = v.get("locked").and_then(Value::as_bool).unwrap_or(false);
    node.opacity = num(v, "opacity", 1.0) as f32;
    node.blend = blend(s(v, "blendMode"));
    if !matches!(node.kind, NodeKind::Group) {
        node.fills = fills;
    }
    node.strokes = paints(ctx, v, "strokes");
    node.stroke.weight = num(v, "strokeWeight", 1.0);
    node.stroke.align = match s(v, "strokeAlign") {
        "INSIDE" => StrokeAlign::Inside,
        "OUTSIDE" => StrokeAlign::Outside,
        _ => StrokeAlign::Center,
    };
    node.stroke.cap = match s(v, "strokeCap") {
        "ROUND" => StrokeCap::Round,
        "SQUARE" => StrokeCap::Square,
        _ => StrokeCap::None,
    };
    node.stroke.join = match s(v, "strokeJoin") {
        "ROUND" => StrokeJoin::Round,
        "BEVEL" => StrokeJoin::Bevel,
        _ => StrokeJoin::Miter,
    };
    node.stroke.dash = v
        .get("strokeDashes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
    node.effects = effects(v);
    node.corner_radius = match v.get("rectangleCornerRadii").and_then(Value::as_array) {
        Some(r) if r.len() == 4 => CornerRadius {
            top_left: r[0].as_f64().unwrap_or(0.0),
            top_right: r[1].as_f64().unwrap_or(0.0),
            bottom_right: r[2].as_f64().unwrap_or(0.0),
            bottom_left: r[3].as_f64().unwrap_or(0.0),
        },
        _ => CornerRadius::uniform(num(v, "cornerRadius", 0.0)),
    };
    if let Some(c) = v.get("constraints") {
        node.constraints = Constraints {
            horizontal: constraint(s(c, "horizontal")),
            vertical: constraint(s(c, "vertical")),
        };
    }
    if node.kind.is_frame_like() {
        node.clips_content = v
            .get("clipsContent")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let direction = match s(v, "layoutMode") {
            "HORIZONTAL" => Some(LayoutDirection::Horizontal),
            "VERTICAL" => Some(LayoutDirection::Vertical),
            _ => None,
        };
        node.layout = direction.map(|direction| AutoLayout {
            direction,
            wrap: s(v, "layoutWrap") == "WRAP",
            gap: num(v, "itemSpacing", 0.0),
            counter_gap: num(v, "counterAxisSpacing", 0.0),
            padding: Padding {
                top: num(v, "paddingTop", 0.0),
                right: num(v, "paddingRight", 0.0),
                bottom: num(v, "paddingBottom", 0.0),
                left: num(v, "paddingLeft", 0.0),
            },
            primary_align: match s(v, "primaryAxisAlignItems") {
                "CENTER" => PrimaryAlign::Center,
                "MAX" => PrimaryAlign::End,
                "SPACE_BETWEEN" => PrimaryAlign::SpaceBetween,
                _ => PrimaryAlign::Start,
            },
            counter_align: match s(v, "counterAxisAlignItems") {
                "CENTER" => CounterAlign::Center,
                "MAX" => CounterAlign::End,
                _ => CounterAlign::Start,
            },
            primary_sizing: if s(v, "primaryAxisSizingMode") == "AUTO" {
                Sizing::Hug
            } else {
                Sizing::Fixed
            },
            counter_sizing: if s(v, "counterAxisSizingMode") == "AUTO" {
                Sizing::Hug
            } else {
                Sizing::Fixed
            },
            ..AutoLayout::default()
        });
    }
    let sz = |k: &str| match s(v, k) {
        "HUG" => Some(Sizing::Hug),
        "FILL" => Some(Sizing::Fill),
        "FIXED" => Some(Sizing::Fixed),
        _ => None,
    };
    if let Some(h) = sz("layoutSizingHorizontal") {
        node.layout_child.horizontal = h;
    }
    if let Some(vv) = sz("layoutSizingVertical") {
        node.layout_child.vertical = vv;
    }
    node.layout_child.absolute = s(v, "layoutPositioning") == "ABSOLUTE";
    node.is_mask = v.get("isMask").and_then(Value::as_bool).unwrap_or(false);
    let container = node.kind.is_container();
    let at = doc.get(parent).map_or(0, |p| p.children.len());
    doc.insert(node, parent, at)
        .map_err(|e| IoError::Other(e.to_string()))?;
    ctx.report.layers += 1;
    if container {
        for c in v
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            add(ctx, doc, c, id, abs, depth + 1)?;
        }
    }
    Ok(())
}

pub fn figma_file_key(url_or_key: &str) -> Option<String> {
    let t = url_or_key.trim();
    let valid = |k: &str| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric());
    if valid(t) {
        return Some(t.to_owned());
    }
    let mut parts = t.split('/');
    while let Some(p) = parts.next() {
        if matches!(p, "file" | "design" | "proto" | "board" | "slides") {
            let k = parts.next()?.split(['?', '#']).next()?;
            return valid(k).then(|| k.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_from_urls() {
        assert_eq!(
            figma_file_key("https://www.figma.com/design/AbC123/Name?node-id=1-2").as_deref(),
            Some("AbC123")
        );
        assert_eq!(
            figma_file_key("https://figma.com/file/XyZ9/x").as_deref(),
            Some("XyZ9")
        );
        assert_eq!(figma_file_key("XyZ9").as_deref(), Some("XyZ9"));
        assert_eq!(figma_file_key("https://example.com/nothing"), None);
    }

    #[test]
    fn svg_paths_parse() {
        let c = svg_path("M0 0L10 0L10 10C10 15 5 20 0 20Z");
        assert_eq!(c.len(), 5);
        assert!(matches!(c[3], PathCmd::CubicTo { .. }));
        assert_eq!(svg_path("M1-2L3,4").len(), 2);
        assert_eq!(svg_path("M1+2L3+4").len(), 2);
        assert!(svg_path("M1e309 0L0 0").is_empty());
        assert!(svg_path("M1e308 0 l1e308 0").is_empty());
    }

    #[test]
    fn svg_relative_paths_follow_current_point_and_closed_subpath() {
        let c = svg_path("m10 20 5 0 h5 v5 q5 5 10 0 z m2 3 l4 0");
        assert_eq!(
            c,
            vec![
                PathCmd::MoveTo {
                    to: Point::new(10.0, 20.0),
                },
                PathCmd::LineTo {
                    to: Point::new(15.0, 20.0),
                },
                PathCmd::LineTo {
                    to: Point::new(20.0, 20.0),
                },
                PathCmd::LineTo {
                    to: Point::new(20.0, 25.0),
                },
                PathCmd::QuadTo {
                    ctrl: Point::new(25.0, 30.0),
                    to: Point::new(30.0, 25.0),
                },
                PathCmd::Close,
                PathCmd::MoveTo {
                    to: Point::new(12.0, 23.0),
                },
                PathCmd::LineTo {
                    to: Point::new(16.0, 23.0),
                },
            ]
        );
        assert_eq!(
            svg_path("M10 20c1 2 3 4 5 6"),
            vec![
                PathCmd::MoveTo {
                    to: Point::new(10.0, 20.0),
                },
                PathCmd::CubicTo {
                    ctrl1: Point::new(11.0, 22.0),
                    ctrl2: Point::new(13.0, 24.0),
                    to: Point::new(15.0, 26.0),
                },
            ]
        );
    }

    #[test]
    fn imports_relative_svg_geometry_as_editable_vector() {
        let file = json!({
            "document": {"id": "0:0", "type": "DOCUMENT", "children": [
                {"id": "0:1", "type": "CANVAS", "children": [
                    {"id": "1:2", "type": "VECTOR", "name": "Contour",
                     "size": {"x": 20, "y": 20},
                     "absoluteBoundingBox": {"x": 0, "y": 0, "width": 20, "height": 20},
                     "fillGeometry": [{"path": "M5 5 l10 0 l0 10 z"}]}
                ]}
            ]}
        });
        let (doc, _) = import_figma_json(&file, &ImageBytes::new()).unwrap();
        let vector = doc.nodes().find(|node| node.name == "Contour").unwrap();
        let NodeKind::Vector { path } = &vector.kind else {
            panic!("expected editable vector");
        };
        assert_eq!(
            path.cmds,
            vec![
                PathCmd::MoveTo {
                    to: Point::new(5.0, 5.0),
                },
                PathCmd::LineTo {
                    to: Point::new(15.0, 5.0),
                },
                PathCmd::LineTo {
                    to: Point::new(15.0, 15.0),
                },
                PathCmd::Close,
            ]
        );
    }

    #[test]
    fn converts_a_small_file() {
        let file = json!({
            "document": {"id": "0:0", "type": "DOCUMENT", "children": [
                {"id": "0:1", "type": "CANVAS", "name": "Home", "backgroundColor": {"r": 1, "g": 1, "b": 1, "a": 1}, "children": [
                    {"id": "1:2", "type": "FRAME", "name": "Card", "absoluteBoundingBox": {"x": 100, "y": 50, "width": 200, "height": 120},
                     "fills": [{"type": "SOLID", "color": {"r": 1, "g": 0, "b": 0, "a": 1}}],
                     "layoutMode": "VERTICAL", "itemSpacing": 8, "paddingLeft": 16, "clipsContent": true,
                     "children": [
                        {"id": "1:3", "type": "TEXT", "name": "Title", "characters": "Hi",
                         "absoluteBoundingBox": {"x": 116, "y": 66, "width": 20, "height": 16},
                         "style": {"fontFamily": "Inter", "fontWeight": 700, "fontSize": 14}},
                        {"id": "1:4", "type": "COMPONENT", "name": "Btn", "absoluteBoundingBox": {"x": 116, "y": 90, "width": 40, "height": 20}},
                        {"id": "1:5", "type": "INSTANCE", "name": "Btn 2", "componentId": "1:4", "absoluteBoundingBox": {"x": 160, "y": 90, "width": 40, "height": 20}}
                     ]}
                ]}
            ]}
        });
        let (doc, report) = import_figma_json(&file, &ImageBytes::new()).unwrap();
        assert_eq!(report.pages, 1);
        assert_eq!(report.layers, 4);
        let card = doc.nodes().find(|n| n.name == "Card").unwrap();
        assert_eq!(card.rect, Rect::new(100.0, 50.0, 200.0, 120.0));
        assert!(card.layout.is_some() && card.clips_content);
        let title = doc.nodes().find(|n| n.name == "Title").unwrap();
        assert_eq!(title.rect.x, 16.0);
        assert_eq!(title.text().unwrap().style.font_weight, 700);
        let inst = doc.nodes().find(|n| n.name == "Btn 2").unwrap();
        let comp = doc.nodes().find(|n| n.name == "Btn").unwrap();
        assert!(matches!(inst.kind, NodeKind::Instance { component, .. } if component == comp.id));
    }
}
