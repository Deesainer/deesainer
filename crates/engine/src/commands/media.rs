use std::sync::Arc;

use base64::Engine as _;
use deesainer_doc::{AssetId, ImageScaleMode, Node, NodeKind, Paint, PaintKind, PropValue};
use deesainer_geom::Rect;
use deesainer_ops::{Op, Transaction};
use serde_json::json;

use super::{layer_ids, param, target_parent, IDS};
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("image.place", "Place image", place_image)
            .menu("File/Place image")
            .key("Shift+Cmd+K")
            .doc("Adds a PNG or JPEG (base64 data) as a rectangle with an image fill, at its natural size unless width/height are given. Returns {id, asset}.")
            .params(const {
                &[
                    param("data", ParamType::String, true, "Base64-encoded PNG or JPEG bytes."),
                    param("parent", ParamType::NodeId, false, "Container; defaults to the current page."),
                    param("x", ParamType::Number, false, "Left edge, default 0."),
                    param("y", ParamType::Number, false, "Top edge, default 0."),
                    param("width", ParamType::Number, false, "Width; default natural width (keeping aspect with height)."),
                    param("height", ParamType::Number, false, "Height."),
                    param("name", ParamType::String, false, "Layer name, default \"Image\"."),
                ]
            }),
        CommandSpec::new("image.setFill", "Image fill", set_image_fill)
            .doc("Replaces the fills of layers with an image (base64 PNG/JPEG) using a scale mode. Returns {asset}.")
            .params(const {
                &[
                    IDS,
                    param("data", ParamType::String, true, "Base64-encoded PNG or JPEG bytes."),
                    param("scaleMode", ParamType::Enum(&["fill", "fit", "stretch", "tile"]), false, "Default fill (cover)."),
                ]
            }),
    ]
}

pub(crate) type ImageData = (AssetId, Arc<Vec<u8>>, (u32, u32));

pub(crate) fn read_image(p: &Params<'_>) -> Result<ImageData, EngineError> {
    let b64 = p.req_str("data")?;
    if b64.len() > MAX_ASSET_BYTES * 4 / 3 + 16 {
        return Err(p.bad("data", "is larger than 32 MB"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| p.bad("data", format!("is not valid base64: {e}")))?;
    let dims = deesainer_render::images::dimensions(&bytes)
        .ok_or_else(|| p.bad("data", "is not a PNG or JPEG image"))?;
    let id = asset_id(&bytes);
    Ok((id, Arc::new(bytes), dims))
}

pub(crate) fn asset_id(bytes: &[u8]) -> AssetId {
    let hash = blake3::hash(bytes);
    AssetId(hash.to_hex().as_str().chars().take(32).collect())
}

fn asset_op(ctx: &Context<'_>, id: &AssetId, bytes: Arc<Vec<u8>>) -> Option<Op> {
    ctx.doc.asset(id).is_none().then(|| Op::SetAsset {
        id: id.clone(),
        data: Some(bytes),
    })
}

fn place_image(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let parent = target_parent(ctx, p)?;
    let (asset, bytes, (iw, ih)) = read_image(p)?;
    let (iw, ih) = (f64::from(iw), f64::from(ih));
    let (w, h) = match (p.non_negative("width")?, p.non_negative("height")?) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ih / iw.max(1.0)),
        (None, Some(h)) => (h * iw / ih.max(1.0), h),
        (None, None) => (iw, ih),
    };
    let rect = Rect::new(p.f64("x")?.unwrap_or(0.0), p.f64("y")?.unwrap_or(0.0), w, h);
    if !rect.is_valid() {
        return Err(p.invalid("the image box is out of range"));
    }
    let id = ctx.doc.next_free_id();
    let mut node = Node::new(
        id,
        p.str("name")?.unwrap_or("Image"),
        NodeKind::Rectangle,
        rect,
    );
    node.fills.push(Paint {
        kind: PaintKind::Image {
            asset: asset.clone(),
            scale_mode: ImageScaleMode::Fill,
            adjust: Default::default(),
        },
        ..Paint::solid(deesainer_color::Rgba::WHITE)
    });
    let mut ops: Vec<Op> = asset_op(ctx, &asset, bytes).into_iter().collect();
    ops.push(Op::CreateNode {
        parent,
        index: ctx.doc.node(parent)?.children.len(),
        node: Box::new(node),
    });
    Ok(Outcome::apply(
        Transaction::new("Place image", ops),
        json!({ "id": id, "asset": asset }),
    )
    .selecting(vec![id]))
}

fn set_image_fill(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let ids = layer_ids(ctx, p)?;
    let (asset, bytes, _) = read_image(p)?;
    let scale_mode = match p.choice("scaleMode", &["fill", "fit", "stretch", "tile"])? {
        Some("fit") => ImageScaleMode::Fit,
        Some("stretch") => ImageScaleMode::Stretch,
        Some("tile") => ImageScaleMode::Tile,
        _ => ImageScaleMode::Fill,
    };
    let paint = Paint {
        kind: PaintKind::Image {
            asset: asset.clone(),
            scale_mode,
            adjust: Default::default(),
        },
        ..Paint::solid(deesainer_color::Rgba::WHITE)
    };
    let mut ops: Vec<Op> = asset_op(ctx, &asset, bytes).into_iter().collect();
    for id in ids {
        ops.push(Op::set(id, PropValue::Fills(vec![paint.clone()])));
    }
    Ok(Outcome::apply(
        Transaction::new("Image fill", ops),
        json!({ "asset": asset }),
    ))
}
