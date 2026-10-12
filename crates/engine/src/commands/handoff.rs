use base64::Engine as _;
use deesainer_doc::AssetId;
use deesainer_handoff::Language;
use serde_json::json;

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("inspect.spec", "Inspect layer", inspect_spec)
            .read_only()
            .doc("What a developer needs to build a layer: position and size, rotation, opacity, blend, corner radii, effective fills/strokes/effects (linked styles applied), typography, auto layout and how its parent lays it out, constraints, the styles and variables it uses (by name, variables with their value in the active mode), its component and variant, and its image assets.")
            .params(const { &[param("id", ParamType::NodeId, true, "Layer id.")] }),
        CommandSpec::new("inspect.code", "Copy code", inspect_code)
            .read_only()
            .doc("Code that builds a layer (the layer itself, not its children): CSS, SwiftUI or Jetpack Compose. Bound variables appear as CSS custom properties with their value as fallback. Returns {language, code}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Layer id."),
                    param("language", ParamType::Enum(&["css", "swiftui", "compose"]), false, "Default css."),
                ]
            }),
        CommandSpec::new("inspect.measure", "Measure", inspect_measure)
            .read_only()
            .doc("Distances between two layers on the page. Apart: horizontalGap and verticalGap. One inside the other: the four margins (relation inside or contains). Overlapping: relation overlap.")
            .params(const {
                &[
                    param("from", ParamType::NodeId, true, "First layer."),
                    param("to", ParamType::NodeId, true, "Second layer."),
                ]
            }),
        CommandSpec::new("inspect.asset", "Download asset", inspect_asset)
            .read_only()
            .doc("The original bytes of an image asset (as used by image fills), base64, with its mimeType and pixel size. Returns {asset, mimeType, data, width, height}.")
            .params(const { &[param("asset", ParamType::String, true, "Asset id, as inspect.spec lists it.")] }),
    ]
}

fn inspect_spec(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let spec = deesainer_handoff::spec(ctx.doc, id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    let v = serde_json::to_value(spec).map_err(|e| EngineError::Internal(e.to_string()))?;
    Ok(Outcome::read(v))
}

fn inspect_code(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.req_node_id("id")?;
    let name = p.str("language")?.unwrap_or("css");
    let language = Language::parse(name)
        .ok_or_else(|| p.bad("language", "must be css, swiftui or compose"))?;
    let code = deesainer_handoff::code(ctx.doc, id, language)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    Ok(Outcome::read(json!({ "language": name, "code": code })))
}

fn inspect_measure(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (from, to) = (p.req_node_id("from")?, p.req_node_id("to")?);
    let m = deesainer_handoff::measure(ctx.doc, from, to)
        .ok_or_else(|| p.bad("from", format!("{from} or {to} does not exist")))?;
    let v = serde_json::to_value(m).map_err(|e| EngineError::Internal(e.to_string()))?;
    Ok(Outcome::read(v))
}

fn mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.get(0..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn inspect_asset(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let asset = AssetId(p.req_str("asset")?.to_owned());
    let bytes = ctx
        .doc
        .asset(&asset)
        .ok_or_else(|| p.bad("asset", format!("{} is not in the document", asset.0)))?;
    let dims = deesainer_render::images::dimensions(bytes);
    Ok(Outcome::read(json!({
        "asset": asset.0,
        "mimeType": mime(bytes),
        "data": base64::engine::general_purpose::STANDARD.encode(bytes.as_slice()),
        "width": dims.map(|d| d.0),
        "height": dims.map(|d| d.1),
    })))
}
