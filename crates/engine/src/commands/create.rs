use deesainer_color::Rgba;
use deesainer_doc::{Node, NodeId, NodeKind, Paint, TextAutoResize, TextData, TextStyle};
use deesainer_geom::{Point, Rect};
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::{param, target_parent};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandKind, CommandSpec, Context, Outcome, ParamSpec, ParamType, RunFn};

pub const SHAPE_FILL: Rgba = Rgba::new(0.851, 0.851, 0.851, 1.0);

const BOX: [ParamSpec; 7] = [
    param(
        "parent",
        ParamType::NodeId,
        false,
        "Container to insert into (page, frame, group, component); defaults to the current page.",
    ),
    param(
        "x",
        ParamType::Number,
        true,
        "Left edge in the parent's space, logical points.",
    ),
    param(
        "y",
        ParamType::Number,
        true,
        "Top edge in the parent's space, logical points.",
    ),
    param("width", ParamType::Number, true, "Width >= 0."),
    param("height", ParamType::Number, true, "Height >= 0."),
    param(
        "fill",
        ParamType::Color,
        false,
        "Solid fill, #RRGGBB or #RRGGBBAA.",
    ),
    param(
        "name",
        ParamType::String,
        false,
        "Layer name; defaults to the shape type.",
    ),
];

const POLY: [ParamSpec; 8] = [
    BOX[0],
    BOX[1],
    BOX[2],
    BOX[3],
    BOX[4],
    BOX[5],
    BOX[6],
    param(
        "count",
        ParamType::Integer,
        false,
        "Polygon sides (default 3) or star points (default 5), 3..=60.",
    ),
];

pub fn specs() -> Vec<CommandSpec> {
    let shape = |id: &'static str,
                 label: &'static str,
                 menu: &'static str,
                 key: &'static str,
                 doc: &'static str,
                 run: RunFn| {
        CommandSpec::new(id, label, run)
            .menu(menu)
            .key(key)
            .doc(doc)
            .params(&BOX)
            .kind(CommandKind::Interactive)
    };
    vec![
        shape("node.create.frame", "Frame", "Tools/Frame", "F", "Creates a frame (white fill, clips content). Returns {id} and selects it.", create_frame),
        shape("node.create.rectangle", "Rectangle", "Tools/Rectangle", "R", "Creates a rectangle. Returns {id} and selects it.", create_rectangle),
        shape("node.create.ellipse", "Ellipse", "Tools/Ellipse", "O", "Creates an ellipse inscribed in the box. Returns {id}.", create_ellipse),
        CommandSpec::new("node.create.polygon", "Polygon", create_polygon)
            .menu("Tools/Polygon")
            .doc("Creates a regular polygon inscribed in the box. Returns {id}.")
            .params(&POLY)
            .kind(CommandKind::Interactive),
        CommandSpec::new("node.create.star", "Star", create_star)
            .menu("Tools/Star")
            .doc("Creates a star inscribed in the box (inner radius 38%). Returns {id}.")
            .params(&POLY)
            .kind(CommandKind::Interactive),
        CommandSpec::new("node.create.line", "Line", create_line)
            .menu("Tools/Line")
            .key("L")
            .doc("Creates a straight line from (x, y) to (x2, y2) with a 1pt black stroke. Returns {id}.")
            .params(const {
                &[
                    param("parent", ParamType::NodeId, false, "Container; defaults to the current page."),
                    param("x", ParamType::Number, true, "Start x."),
                    param("y", ParamType::Number, true, "Start y."),
                    param("x2", ParamType::Number, true, "End x."),
                    param("y2", ParamType::Number, true, "End y."),
                    param("stroke", ParamType::Color, false, "Stroke colour, default #000000."),
                    param("name", ParamType::String, false, "Layer name."),
                ]
            })
            .kind(CommandKind::Interactive),
        CommandSpec::new("node.create.text", "Text", create_text)
            .menu("Tools/Text")
            .key("T")
            .doc("Creates a text layer. Without width it auto-sizes to the text; with width the text wraps and the height grows. Returns {id}.")
            .params(const {
                &[
                    param("parent", ParamType::NodeId, false, "Container; defaults to the current page."),
                    param("x", ParamType::Number, true, "Left edge."),
                    param("y", ParamType::Number, true, "Top edge."),
                    param("content", ParamType::String, true, "The text. Newlines start paragraphs."),
                    param("width", ParamType::Number, false, "Fixed width (wraps text)."),
                    param("fontFamily", ParamType::String, false, "Default \"Inter\" (falls back to an available sans-serif)."),
                    param("fontSize", ParamType::Number, false, "Points, > 0, default 16."),
                    param("fontWeight", ParamType::Integer, false, "100..=900, default 400."),
                    param("fill", ParamType::Color, false, "Text colour, default #000000."),
                    param("name", ParamType::String, false, "Layer name; defaults to the text."),
                ]
            })
            .kind(CommandKind::Interactive),
    ]
}

fn read_box(p: &Params<'_>) -> Result<Rect, EngineError> {
    let rect = Rect::new(
        p.req_f64("x")?,
        p.req_f64("y")?,
        p.req_f64("width")?,
        p.req_f64("height")?,
    );
    if !rect.is_valid() {
        return Err(p.bad("width/height", "must be >= 0"));
    }
    Ok(rect)
}

pub(crate) fn insert_top(
    ctx: &Context<'_>,
    parent: NodeId,
    node: Node,
    label: &str,
) -> Result<Outcome, EngineError> {
    let index = ctx.doc.node(parent)?.children.len();
    let id = node.id;
    let tx = Transaction::new(
        label,
        vec![Op::CreateNode {
            parent,
            index,
            node: Box::new(node),
        }],
    );
    Ok(Outcome::apply(tx, json!({ "id": id })).selecting(vec![id]))
}

fn simple_shape(
    ctx: &Context<'_>,
    p: &Params<'_>,
    kind: NodeKind,
    default_name: &str,
    default_fill: Rgba,
) -> Result<Outcome, EngineError> {
    let parent = target_parent(ctx, p)?;
    let rect = read_box(p)?;
    let name = p.str("name")?.unwrap_or(default_name);
    let mut node = Node::new(ctx.doc.next_free_id(), name, kind, rect);
    node.fills
        .push(Paint::solid(p.color("fill")?.unwrap_or(default_fill)));
    insert_top(
        ctx,
        parent,
        node,
        &format!("Create {}", default_name.to_lowercase()),
    )
}

fn create_frame(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    simple_shape(ctx, p, NodeKind::Frame, "Frame", Rgba::WHITE)
}

fn create_rectangle(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    simple_shape(ctx, p, NodeKind::Rectangle, "Rectangle", SHAPE_FILL)
}

fn create_ellipse(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    simple_shape(ctx, p, NodeKind::ellipse(), "Ellipse", SHAPE_FILL)
}

fn count(p: &Params<'_>, default: u32) -> Result<u32, EngineError> {
    let n = p.u32("count")?.unwrap_or(default);
    if !(3..=60).contains(&n) {
        return Err(p.bad("count", "must be between 3 and 60"));
    }
    Ok(n)
}

fn create_polygon(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let sides = count(p, 3)?;
    simple_shape(ctx, p, NodeKind::Polygon { sides }, "Polygon", SHAPE_FILL)
}

fn create_star(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let points = count(p, 5)?;
    let kind = NodeKind::Star {
        points,
        inner_ratio: 0.382,
    };
    simple_shape(ctx, p, kind, "Star", SHAPE_FILL)
}

fn create_line(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let parent = target_parent(ctx, p)?;
    let a = Point::new(p.req_f64("x")?, p.req_f64("y")?);
    let b = Point::new(p.req_f64("x2")?, p.req_f64("y2")?);
    let len = a.distance(b);
    if !len.is_finite() {
        return Err(p.invalid("line endpoints are out of range"));
    }
    let angle = (b.y - a.y).atan2(b.x - a.x).to_degrees();
    let mid = a.lerp(b, 0.5);
    let rect = Rect::new(mid.x - len / 2.0, mid.y, len, 0.0);
    let name = p.str("name")?.unwrap_or("Line");
    let mut node = Node::new(ctx.doc.next_free_id(), name, NodeKind::Line, rect);
    node.rotation = if angle.abs() < 1e-9 { 0.0 } else { angle };
    node.strokes
        .push(Paint::solid(p.color("stroke")?.unwrap_or(Rgba::BLACK)));
    node.stroke.align = deesainer_doc::StrokeAlign::Center;
    insert_top(ctx, parent, node, "Create line")
}

pub(crate) fn estimate_text_box(data: &TextData, width: Option<f64>) -> (f64, f64) {
    let size = data.style.font_size;
    let line_h = data.style.line_height_px();
    let paragraphs: Vec<&str> = data.content.split('\n').collect();
    match width {
        Some(w) => {
            let per_line = (w / (size * 0.55)).max(1.0);
            let lines: f64 = paragraphs
                .iter()
                .map(|para| (para.chars().count() as f64 / per_line).ceil().max(1.0))
                .sum();
            (w, lines * line_h)
        }
        None => {
            let longest = paragraphs
                .iter()
                .map(|para| para.chars().count())
                .max()
                .unwrap_or(0) as f64;
            (longest * size * 0.55, paragraphs.len() as f64 * line_h)
        }
    }
}

fn create_text(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let parent = target_parent(ctx, p)?;
    let content = p.req_str("content")?;
    let mut style = TextStyle::default();
    if let Some(size) = p.f64("fontSize")? {
        if size <= 0.0 || size > 10_000.0 {
            return Err(p.bad("fontSize", "must be > 0 and <= 10000"));
        }
        style.font_size = size;
    }
    if let Some(family) = p.str("fontFamily")? {
        style.font_family = family.to_owned();
    }
    if let Some(w) = p.u32("fontWeight")? {
        if !(1..=1000).contains(&w) {
            return Err(p.bad("fontWeight", "must be between 1 and 1000"));
        }
        style.font_weight = w as u16;
    }
    let width = p.non_negative("width")?;
    let data = TextData {
        content: content.to_owned(),
        style,
        auto_resize: if width.is_some() {
            TextAutoResize::Height
        } else {
            TextAutoResize::WidthAndHeight
        },
        ..TextData::default()
    };
    let (w, h) = estimate_text_box(&data, width);
    let rect = Rect::new(p.req_f64("x")?, p.req_f64("y")?, w, h);
    if !rect.is_valid() {
        return Err(p.invalid("the text box is out of range"));
    }
    let default_name: String = content.chars().take(40).collect();
    let name = p
        .str("name")?
        .map(str::to_owned)
        .unwrap_or(if default_name.is_empty() {
            "Text".to_owned()
        } else {
            default_name
        });
    let mut node = Node::new(ctx.doc.next_free_id(), name, NodeKind::Text(data), rect);
    node.fills
        .push(Paint::solid(p.color("fill")?.unwrap_or(Rgba::BLACK)));
    insert_top(ctx, parent, node, "Create text")
}
