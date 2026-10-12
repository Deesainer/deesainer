use deesainer_doc::{
    LineHeight, NodeKind, PropValue, TextAlignH, TextAlignV, TextAutoResize, TextCase,
    TextDecoration, TextRun, TextStyle,
};
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::{layer_ids, param, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandKind, CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("text.setContent", "Edit text", set_content)
            .kind(CommandKind::Interactive)
            .doc("Replaces a text layer's characters. Auto-sizing text boxes grow or shrink to fit.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Text layer."),
                    param("content", ParamType::String, true, "New text; newlines start paragraphs."),
                    param("runs", ParamType::Json, false, "Optional sorted UTF-8 style ranges for the new content."),
                ]
            }),
        CommandSpec::new("text.setRangeStyle", "Style text range", set_range_style)
            .kind(CommandKind::Interactive)
            .doc("Sets typography on a non-empty UTF-8 byte range of one text layer. The complete style object is required; ranges stay sorted and non-overlapping.")
            .params(const { &[
                param("id", ParamType::NodeId, true, "Text layer."),
                param("start", ParamType::Integer, true, "Inclusive UTF-8 byte offset."),
                param("end", ParamType::Integer, true, "Exclusive UTF-8 byte offset."),
                param("style", ParamType::Json, true, "Complete TextStyle object (decoration and an optional fill colour may differ per range); case must match the layer."),
            ] }),
        CommandSpec::new("text.setStyle", "Text style", set_style)
            .kind(CommandKind::Dialog)
            .doc("Sets typography on text layers. Omitted keys are unchanged. lineHeight: number (pixels), \"auto\", or \"150%\".")
            .params(const {
                &[
                    IDS,
                    param("fontFamily", ParamType::String, false, "Font family name."),
                    param("fontSize", ParamType::Number, false, "Points, > 0."),
                    param("fontWeight", ParamType::Integer, false, "100..=900."),
                    param("italic", ParamType::Bool, false, "Italic."),
                    param("lineHeight", ParamType::Json, false, "Pixels (number), \"auto\", or a percentage string like \"140%\"."),
                    param("letterSpacing", ParamType::Number, false, "Percent of the font size."),
                    param("align", ParamType::Enum(&["left", "center", "right", "justified"]), false, "Horizontal alignment."),
                    param("alignVertical", ParamType::Enum(&["top", "center", "bottom"]), false, "Vertical alignment in a fixed box."),
                    param("autoResize", ParamType::Enum(&["widthAndHeight", "height", "none"]), false, "Box sizing."),
                    param("decoration", ParamType::Enum(&["none", "underline", "strikethrough"]), false, "Underline or strikethrough."),
                    param("case", ParamType::Enum(&["original", "upper", "lower", "title"]), false, "Letter case transform."),
                    param("paragraphSpacing", ParamType::Number, false, "Extra space after each paragraph, 0..=10000 points."),
                    param("features", ParamType::Json, false, "OpenType features [{tag: \"liga\", value: 0}, {tag: \"tnum\", value: 1}]; replaces the list."),
                    param("variations", ParamType::Json, false, "Variable font axes [{tag: \"wdth\", value: 75}, {tag: \"opsz\", value: 24}]; replaces the list (weight follows fontWeight unless wght is given)."),
                    param("list", ParamType::Enum(&["none", "bullet", "number"]), false, "Show every paragraph as a bulleted or numbered list (nesting levels are kept)."),
                ]
            }),
        CommandSpec::new("text.setParagraphs", "Paragraph list", set_paragraphs)
            .kind(CommandKind::Dialog)
            .doc("Sets the list format of some paragraphs of a text layer: bulleted, numbered or plain, and their nesting level. Paragraphs are counted from 0 by line break; omit first and last for all of them.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Text layer."),
                    param("first", ParamType::Integer, false, "First paragraph (default 0)."),
                    param("last", ParamType::Integer, false, "Last paragraph, inclusive (default the last one)."),
                    param("list", ParamType::Enum(&["none", "bullet", "number"]), false, "Marker kind."),
                    param("level", ParamType::Integer, false, "Nesting level, 0..=8."),
                    param("indent", ParamType::Number, false, "Levels to add (negative to outdent), clamped to 0..=8."),
                ]
            }),
    ]
}

fn list_kind(name: &str) -> deesainer_doc::TextList {
    match name {
        "bullet" => deesainer_doc::TextList::Bullet,
        "number" => deesainer_doc::TextList::Number,
        _ => deesainer_doc::TextList::None,
    }
}

fn set_paragraphs(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx.doc.node(id)?;
    let NodeKind::Text(data) = &node.kind else {
        return Err(p.bad("id", "is not a text layer"));
    };
    let count = data.paragraph_count();
    let first = p.u32("first")?.map_or(0, |v| v as usize);
    let last = p
        .u32("last")?
        .map_or(count.saturating_sub(1), |v| v as usize);
    if first > last || last >= count {
        return Err(p.bad("last", format!("paragraphs run 0..={}", count - 1)));
    }
    let list = p
        .choice("list", &["none", "bullet", "number"])?
        .map(list_kind);
    let level = p.u32("level")?;
    if level.is_some_and(|l| l > u32::from(deesainer_doc::TextData::MAX_LIST_LEVEL)) {
        return Err(p.bad("level", "must be 0..=8"));
    }
    let indent = p.f64("indent")?.map(|v| v.round().clamp(-8.0, 8.0) as i32);
    let mut d = data.clone();
    d.set_paragraph_format(first, last, |f| {
        if let Some(l) = list {
            f.list = l;
        }
        if let Some(l) = level {
            f.level = l as u8;
        }
        if let Some(by) = indent {
            f.level = (i32::from(f.level) + by).clamp(0, 8) as u8;
        }
    })
    .map_err(|e| p.invalid(e))?;
    let ops = if &d == data {
        Vec::new()
    } else {
        vec![Op::set(id, PropValue::Kind(NodeKind::Text(d)))]
    };
    Ok(Outcome::apply(
        Transaction::new("Paragraph list", ops),
        json!({ "paragraphs": count }),
    ))
}

fn set_content(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    let NodeKind::Text(data) = &node.kind else {
        return Err(p.bad("id", "not a text layer"));
    };
    let content = p.req_str("content")?;
    if content.len() > 1_000_000 {
        return Err(p.bad("content", "is longer than 1 MB"));
    }
    let mut data = data.clone();
    if let Some(raw) = p.raw("runs") {
        if raw
            .as_array()
            .is_none_or(|runs| runs.len() > deesainer_doc::TextData::MAX_RUNS)
        {
            return Err(p.bad("runs", "must be an array of at most 4096 ranges"));
        }
        data.content = content.to_owned();
        data.runs = serde_json::from_value::<Vec<TextRun>>(raw.clone())
            .map_err(|_| p.bad("runs", "invalid text style ranges"))?;
        data.validate_runs().map_err(|e| p.bad("runs", e))?;
    } else {
        data.replace_content(content.to_owned())
            .map_err(|e| p.bad("content", e))?;
    }
    let mut ops = vec![Op::set(id, PropValue::Kind(NodeKind::Text(data)))];
    let old_default: String = node
        .text()
        .map(|t| t.content.chars().take(40).collect())
        .unwrap_or_default();
    if node.name == old_default || node.name == "Text" {
        let name: String = content.chars().take(40).collect();
        if !name.is_empty() {
            ops.push(Op::set(id, PropValue::Name(name)));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Edit text", ops),
        json!({}),
    ))
}

fn set_range_style(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let start = p.u32("start")?.ok_or_else(|| p.bad("start", "required"))? as usize;
    let end = p.u32("end")?.ok_or_else(|| p.bad("end", "required"))? as usize;
    let raw = p.raw("style").ok_or_else(|| p.bad("style", "required"))?;
    let style = serde_json::from_value::<TextStyle>(raw.clone())
        .map_err(|_| p.bad("style", "invalid TextStyle object"))?;
    let node = ctx.doc.node(id)?;
    let NodeKind::Text(data) = &node.kind else {
        return Err(p.bad("id", "not a text layer"));
    };
    let mut data = data.clone();
    data.set_range_style(start, end, style)
        .map_err(|e| p.bad("style", e))?;
    Ok(Outcome::apply(
        Transaction::new(
            "Style text range",
            vec![Op::set(id, PropValue::Kind(NodeKind::Text(data)))],
        ),
        json!({}),
    ))
}

fn parse_line_height(p: &Params<'_>) -> Result<Option<LineHeight>, EngineError> {
    let Some(v) = p.raw("lineHeight") else {
        return Ok(None);
    };
    if let Some(n) = v.as_f64() {
        if n.is_finite() && n > 0.0 {
            return Ok(Some(LineHeight::Pixels(n)));
        }
    }
    if let Some(s) = v.as_str() {
        if s.eq_ignore_ascii_case("auto") {
            return Ok(Some(LineHeight::Auto));
        }
        if let Some(pct) = s
            .strip_suffix('%')
            .and_then(|n| n.trim().parse::<f64>().ok())
        {
            if pct.is_finite() && pct > 0.0 {
                return Ok(Some(LineHeight::Percent(pct)));
            }
        }
    }
    Err(p.bad(
        "lineHeight",
        "must be a positive number, \"auto\" or \"N%\"",
    ))
}

fn set_style(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let size = p.f64("fontSize")?;
    if size.is_some_and(|s| s <= 0.0 || s > 10_000.0) {
        return Err(p.bad("fontSize", "must be > 0 and <= 10000"));
    }
    let weight = p.u32("fontWeight")?;
    if weight.is_some_and(|w| !(1..=1000).contains(&w)) {
        return Err(p.bad("fontWeight", "must be between 1 and 1000"));
    }
    let spacing = p.f64("letterSpacing")?;
    let line_height = parse_line_height(p)?;
    let mut ops = Vec::new();
    for id in ids {
        let node = ctx.doc.node(id)?;
        let NodeKind::Text(data) = &node.kind else {
            continue;
        };
        let mut d = data.clone();
        if let Some(f) = p.str("fontFamily")? {
            d.style.font_family = f.to_owned();
        }
        if let Some(s) = size {
            d.style.font_size = s;
        }
        if let Some(w) = weight {
            d.style.font_weight = w as u16;
        }
        if let Some(i) = p.bool("italic")? {
            d.style.italic = i;
        }
        if let Some(lh) = line_height {
            d.style.line_height = lh;
        }
        if let Some(s) = spacing {
            d.style.letter_spacing = s;
        }
        match p.choice("align", &["left", "center", "right", "justified"])? {
            Some("left") => d.align_h = TextAlignH::Left,
            Some("center") => d.align_h = TextAlignH::Center,
            Some("right") => d.align_h = TextAlignH::Right,
            Some("justified") => d.align_h = TextAlignH::Justified,
            _ => {}
        }
        match p.choice("alignVertical", &["top", "center", "bottom"])? {
            Some("top") => d.align_v = TextAlignV::Top,
            Some("center") => d.align_v = TextAlignV::Center,
            Some("bottom") => d.align_v = TextAlignV::Bottom,
            _ => {}
        }
        if let Some(features) = p.json::<Vec<deesainer_doc::FontFeature>>("features")? {
            if features.len() > 32 || features.iter().any(|f| !f.is_valid()) {
                return Err(p.bad(
                    "features",
                    "at most 32 features of four letters or digits, values 0..=99",
                ));
            }
            d.style.features = features.clone();
            for run in &mut d.runs {
                run.style.features = features.clone();
            }
        }
        if let Some(variations) = p.json::<Vec<deesainer_doc::FontVariation>>("variations")? {
            if variations.len() > 32 || variations.iter().any(|v| !v.is_valid()) {
                return Err(p.bad(
                    "variations",
                    "at most 32 axes of four letters or digits with finite values",
                ));
            }
            d.style.variations = variations.clone();
            for run in &mut d.runs {
                run.style.variations = variations.clone();
            }
        }
        if let Some(v) = p.f64("paragraphSpacing")? {
            if !(0.0..=10_000.0).contains(&v) {
                return Err(p.bad("paragraphSpacing", "must be between 0 and 10000"));
            }
            d.paragraph_spacing = v;
        }
        if let Some(kind) = p.choice("list", &["none", "bullet", "number"])? {
            let last = d.paragraph_count() - 1;
            d.set_paragraph_format(0, last, |f| f.list = list_kind(kind))
                .map_err(|e| p.invalid(e))?;
        }
        match p.choice("autoResize", &["widthAndHeight", "height", "none"])? {
            Some("widthAndHeight") => d.auto_resize = TextAutoResize::WidthAndHeight,
            Some("height") => d.auto_resize = TextAutoResize::Height,
            Some("none") => d.auto_resize = TextAutoResize::None,
            _ => {}
        }
        match p.choice("decoration", &["none", "underline", "strikethrough"])? {
            Some("none") => d.style.decoration = TextDecoration::None,
            Some("underline") => d.style.decoration = TextDecoration::Underline,
            Some("strikethrough") => d.style.decoration = TextDecoration::Strikethrough,
            _ => {}
        }
        match p.choice("case", &["original", "upper", "lower", "title"])? {
            Some("original") => d.style.case = TextCase::Original,
            Some("upper") => d.style.case = TextCase::Upper,
            Some("lower") => d.style.case = TextCase::Lower,
            Some("title") => d.style.case = TextCase::Title,
            _ => {}
        }
        let decoration_given = p
            .choice("decoration", &["none", "underline", "strikethrough"])?
            .is_some();
        for run in &mut d.runs {
            run.style.case = d.style.case;
            if decoration_given {
                run.style.decoration = d.style.decoration;
            }
        }
        if &d != data {
            ops.push(Op::set(id, PropValue::Kind(NodeKind::Text(d))));
        }
    }
    Ok(Outcome::apply(
        Transaction::new("Text style", ops),
        json!({}),
    ))
}
