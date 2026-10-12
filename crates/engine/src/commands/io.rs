use base64::Engine as _;
use deesainer_doc::{ExportFormat, ExportSetting, NodeId, NodeKind, PropValue};
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::{clone_subtrees, param, target_parent};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Effect, Outcome, ParamType};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("export.png", "Export PNG", export_png)
            .read_only()
            .doc("Renders a layer or page to PNG at a scale. Returns {data (base64), width, height}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, false, "Layer or page; defaults to the selection, else the current page."),
                    param("scale", ParamType::Number, false, "0.01..=16, default 1."),
                ]
            }),
        CommandSpec::new("export.jpeg", "Export JPEG", export_jpeg)
            .read_only()
            .doc("Renders a layer or page to JPEG (no transparency: composited on white). Returns {data (base64), width, height}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, false, "Layer or page; defaults to the selection, else the current page."),
                    param("scale", ParamType::Number, false, "0.01..=16, default 1."),
                    param("quality", ParamType::Integer, false, "1..=100, default 90."),
                ]
            }),
        CommandSpec::new("export.webp", "Export WebP", export_webp)
            .read_only()
            .doc("Renders a layer or page to lossless WebP, keeping transparency. Returns {data (base64), width, height}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, false, "Layer or page; defaults to the selection, else the current page."),
                    param("scale", ParamType::Number, false, "0.01..=16, default 1."),
                ]
            }),
        CommandSpec::new("export.svg", "Export SVG", export_svg)
            .read_only()
            .doc("Exports a layer or page as an SVG document (text as outlines). Returns {svg}.")
            .params(const { &[param("id", ParamType::NodeId, false, "Layer or page; defaults to the selection, else the current page.")] }),
        CommandSpec::new("export.pdf", "Export PDF", export_pdf)
            .read_only()
            .doc("Exports layers as a PDF, one page each (vector where possible; effects and gradients are embedded renders). Returns {data (base64)}.")
            .params(const { &[param("ids", ParamType::NodeIds, false, "Layers; defaults to the selection, else the top-level frames of the current page.")] }),
        CommandSpec::new("node.setExportSettings", "Export settings", set_export_settings)
            .doc("Replaces the export settings of layers (or a page): rows of {format: png|jpg|webp|svg|pdf, scale (raster, 0.01..=16, default 1), suffix (added to the name, at most 64 characters)}, at most 32. They are saved with the file; export.settings renders them.")
            .params(const {
                &[
                    param("ids", ParamType::NodeIds, false, "Layers or a page; defaults to the selection, else the current page."),
                    param("settings", ParamType::Json, true, "Array of export settings."),
                ]
            }),
        CommandSpec::new("export.settings", "Export", export_settings)
            .read_only()
            .doc("Exports a layer (or page) with each of its export settings. Returns {files: [{fileName, mimeType, data (base64) | svg, width?, height?}]}; file names are the layer name plus the suffix, made safe and unique.")
            .params(const { &[param("id", ParamType::NodeId, false, "Layer or page; defaults to the selection, else the current page.")] }),
        CommandSpec::new("import.svg", "Import SVG", import_svg)
            .doc("Imports SVG text as a frame of editable vector layers. Returns {id}.")
            .params(const {
                &[
                    param("data", ParamType::String, true, "SVG document text."),
                    param("parent", ParamType::NodeId, false, "Container; defaults to the current page."),
                    param("x", ParamType::Number, false, "Left edge, default 0."),
                    param("y", ParamType::Number, false, "Top edge, default 0."),
                    param("name", ParamType::String, false, "Frame name, default \"SVG\"."),
                ]
            }),
        CommandSpec::new("document.serialize", "Serialize document", serialize)
            .read_only()
            .doc("Returns the whole document as a .deesainer file (base64), for saving or transfer: {data}."),
        CommandSpec::new("document.load", "Load document", load)
            .doc("Replaces the document with a .deesainer file (base64). Clears history.")
            .params(const { &[param("data", ParamType::String, true, "Base64 .deesainer bytes.")] }),
    ]
}

fn default_target(ctx: &Context<'_>, p: &Params<'_>) -> Result<NodeId, EngineError> {
    let id = p
        .node_id("id")?
        .or_else(|| ctx.selection.first().copied())
        .unwrap_or(ctx.current_page);
    if !ctx.doc.contains(id) || matches!(ctx.doc.get(id).map(|n| &n.kind), Some(NodeKind::Document))
    {
        return Err(p.bad("id", format!("{id} is not a layer or page")));
    }
    Ok(id)
}

fn io_err(p: &Params<'_>, e: deesainer_io::IoError) -> EngineError {
    p.invalid(e.to_string())
}

fn export_png(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = default_target(ctx, p)?;
    let scale = p.f64("scale")?.unwrap_or(1.0);
    if !(0.01..=16.0).contains(&scale) {
        return Err(p.bad("scale", "must be between 0.01 and 16"));
    }
    let png = deesainer_io::export_png(ctx.doc, id, scale).map_err(|e| io_err(p, e))?;
    let (w, h) = deesainer_render::images::dimensions(&png).unwrap_or((0, 0));
    Ok(Outcome::read(
        json!({ "mimeType": "image/png", "data": B64.encode(png), "width": w, "height": h }),
    ))
}

fn export_raster(
    ctx: &Context<'_>,
    p: &Params<'_>,
    format: deesainer_io::RasterFormat,
    mime: &str,
) -> Result<Outcome, EngineError> {
    let id = default_target(ctx, p)?;
    let scale = p.f64("scale")?.unwrap_or(1.0);
    if !(0.01..=16.0).contains(&scale) {
        return Err(p.bad("scale", "must be between 0.01 and 16"));
    }
    let r = deesainer_io::export_raster(ctx.doc, id, scale, format).map_err(|e| io_err(p, e))?;
    Ok(Outcome::read(
        json!({ "mimeType": mime, "data": B64.encode(r.bytes), "width": r.width, "height": r.height }),
    ))
}

fn export_jpeg(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let q = p.u32("quality")?.unwrap_or(90);
    if !(1..=100).contains(&q) {
        return Err(p.bad("quality", "must be between 1 and 100"));
    }
    export_raster(
        ctx,
        p,
        deesainer_io::RasterFormat::Jpeg(q as u8),
        "image/jpeg",
    )
}

fn export_webp(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    export_raster(ctx, p, deesainer_io::RasterFormat::WebP, "image/webp")
}

fn export_svg(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = default_target(ctx, p)?;
    let svg = deesainer_io::export_svg(ctx.doc, id).map_err(|e| io_err(p, e))?;
    Ok(Outcome::read(
        json!({ "mimeType": "image/svg+xml", "svg": svg }),
    ))
}

fn export_pdf(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = match p.node_ids("ids")? {
        Some(ids) if !ids.is_empty() => ids,
        _ if !ctx.selection.is_empty() => ctx.selection.to_vec(),
        _ => ctx
            .doc
            .get(ctx.current_page)
            .map(|pg| {
                pg.children
                    .iter()
                    .copied()
                    .filter(|c| ctx.doc.get(*c).is_some_and(|n| n.kind.is_frame_like()))
                    .collect()
            })
            .unwrap_or_default(),
    };
    if ids.is_empty() {
        return Err(p.invalid("nothing to export: select layers or add frames"));
    }
    for id in &ids {
        if !ctx.doc.contains(*id) {
            return Err(p.bad("ids", format!("{id} does not exist")));
        }
    }
    let pdf = deesainer_io::export_pdf(ctx.doc, &ids).map_err(|e| io_err(p, e))?;
    Ok(Outcome::read(
        json!({ "mimeType": "application/pdf", "data": B64.encode(pdf), "pages": ids.len() }),
    ))
}

fn import_svg(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let parent = target_parent(ctx, p)?;
    let data = p.req_str("data")?;
    let name = p.str("name")?.unwrap_or("SVG");
    let mut imp = deesainer_io::import_svg(data, name).map_err(|e| io_err(p, e))?;
    let (x, y) = (p.f64("x")?.unwrap_or(0.0), p.f64("y")?.unwrap_or(0.0));
    if let Some(root) = imp.nodes.get_mut(&imp.root) {
        root.rect = root.rect.with_origin(x, y);
    }
    let mut ops: Vec<Op> = imp
        .assets
        .iter()
        .filter(|(id, _)| ctx.doc.asset(id).is_none())
        .map(|(id, bytes)| Op::SetAsset {
            id: id.clone(),
            data: Some(bytes.clone()),
        })
        .collect();
    let index = ctx.doc.node(parent)?.children.len();
    let (create, roots) =
        clone_subtrees(ctx.doc, &imp.nodes, &[imp.root], parent, index, (0.0, 0.0))?;
    ops.extend(create);
    let id = roots.first().copied().unwrap_or(ctx.current_page);
    Ok(Outcome::apply(Transaction::new("Import SVG", ops), json!({ "id": id })).selecting(roots))
}

fn serialize(ctx: &Context<'_>, _: &Params<'_>) -> Result<Outcome, EngineError> {
    let bytes = deesainer_format::write_bundle(ctx.doc, None, ctx.versions)
        .map_err(|e| EngineError::Internal(e.to_string()))?;
    Ok(Outcome::read(json!({ "data": B64.encode(bytes) })))
}

fn load(_: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let raw = p.req_str("data")?;
    let bytes = B64
        .decode(raw.trim())
        .map_err(|e| p.bad("data", format!("is not valid base64: {e}")))?;
    let (doc, _, versions) =
        deesainer_format::read_bundle(&bytes).map_err(|e| p.bad("data", e.to_string()))?;
    let page = doc.first_page();
    let mut out = Outcome::effect(Effect::Replace(Box::new(doc), versions))
        .on_page(page)
        .selecting(Vec::new());
    out.reply = json!({ "page": page });
    Ok(out)
}

const MAX_EXPORT_SETTINGS: usize = 32;

fn set_export_settings(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let settings: Vec<ExportSetting> = p.json("settings")?.ok_or_else(|| p.missing("settings"))?;
    if settings.len() > MAX_EXPORT_SETTINGS {
        return Err(p.bad(
            "settings",
            format!("allows at most {MAX_EXPORT_SETTINGS} rows"),
        ));
    }
    for s in &settings {
        if !(0.01..=16.0).contains(&s.scale) {
            return Err(p.bad("settings", "scales must be between 0.01 and 16"));
        }
        if s.suffix.chars().count() > 64 || s.suffix.chars().any(char::is_control) {
            return Err(p.bad("settings", "suffixes are at most 64 printable characters"));
        }
    }
    let ids = match p.node_ids("ids")? {
        Some(ids) if !ids.is_empty() => ids,
        _ if !ctx.selection.is_empty() => ctx.selection.to_vec(),
        _ => vec![ctx.current_page],
    };
    let mut ops = Vec::with_capacity(ids.len());
    for id in ids {
        let node = ctx
            .doc
            .get(id)
            .ok_or_else(|| p.bad("ids", format!("{id} does not exist")))?;
        if matches!(node.kind, NodeKind::Document) {
            return Err(p.bad("ids", "the document has no export settings"));
        }
        ops.push(Op::set(id, PropValue::ExportSettings(settings.clone())));
    }
    Ok(Outcome::apply(
        Transaction::new("Export settings", ops),
        json!({}),
    ))
}

fn file_stem(name: &str, suffix: &str) -> String {
    let joined = format!("{name}{suffix}");
    let safe: String = joined
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    let safe = safe.trim().trim_start_matches('.').trim().to_owned();
    if safe.is_empty() {
        "Untitled".to_owned()
    } else {
        safe
    }
}

fn export_settings(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = default_target(ctx, p)?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    if node.export_settings.is_empty() {
        return Err(
            p.invalid("the layer has no export settings: add them with node.setExportSettings")
        );
    }
    let mut used = std::collections::BTreeSet::new();
    let mut files = Vec::with_capacity(node.export_settings.len());
    for s in &node.export_settings {
        let ext = s.format.extension();
        let stem = file_stem(&node.name, &s.suffix);
        let mut file_name = format!("{stem}.{ext}");
        let mut n = 2;
        while !used.insert(file_name.to_lowercase()) {
            file_name = format!("{stem} {n}.{ext}");
            n += 1;
        }
        let file = match s.format {
            ExportFormat::Svg => {
                let svg = deesainer_io::export_svg(ctx.doc, id).map_err(|e| io_err(p, e))?;
                json!({ "fileName": file_name, "mimeType": "image/svg+xml", "svg": svg })
            }
            ExportFormat::Pdf => {
                let pdf = deesainer_io::export_pdf(ctx.doc, &[id]).map_err(|e| io_err(p, e))?;
                json!({ "fileName": file_name, "mimeType": "application/pdf", "data": B64.encode(pdf) })
            }
            ExportFormat::Png => {
                let png =
                    deesainer_io::export_png(ctx.doc, id, s.scale).map_err(|e| io_err(p, e))?;
                let (w, h) = deesainer_render::images::dimensions(&png).unwrap_or((0, 0));
                json!({ "fileName": file_name, "mimeType": "image/png", "data": B64.encode(png), "width": w, "height": h })
            }
            ExportFormat::Jpg | ExportFormat::Webp => {
                let (format, mime) = if s.format == ExportFormat::Jpg {
                    (deesainer_io::RasterFormat::Jpeg(90), "image/jpeg")
                } else {
                    (deesainer_io::RasterFormat::WebP, "image/webp")
                };
                let r = deesainer_io::export_raster(ctx.doc, id, s.scale, format)
                    .map_err(|e| io_err(p, e))?;
                json!({ "fileName": file_name, "mimeType": mime, "data": B64.encode(r.bytes), "width": r.width, "height": r.height })
            }
        };
        files.push(file);
    }
    Ok(Outcome::read(json!({ "files": files })))
}
