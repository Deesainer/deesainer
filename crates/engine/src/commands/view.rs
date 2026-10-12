use base64::Engine as _;
use deesainer_doc::NodeKind;
use deesainer_geom::Point;
use deesainer_render::{encode_png, render_node, Options};
use serde_json::json;

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("view.renderPreview", "Render preview", render_preview)
            .read_only()
            .doc("Renders a layer (or the current page) to PNG and returns {mimeType, data (base64), width, height, bounds}. Use it to look at your work. The image is downscaled to fit maxSize.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, false, "Layer or page to render; defaults to the current page."),
                    param("scale", ParamType::Number, false, "Pixels per point, 0.01..=8, default 1."),
                    param("maxSize", ParamType::Integer, false, "Longest side cap in pixels, default 1600, max 8192."),
                    param("background", ParamType::Bool, false, "Paint the page background behind the layer, default true for pages, false for layers."),
                ]
            }),
        CommandSpec::new("view.hitTest", "Hit test", hit_test)
            .read_only()
            .doc("Returns the layer a click at page point (x, y) selects: {id, path}. With deep, the innermost layer (like Cmd+click).")
            .params(const {
                &[
                    param("x", ParamType::Number, true, "Page x."),
                    param("y", ParamType::Number, true, "Page y."),
                    param("deep", ParamType::Bool, false, "Select the innermost layer."),
                ]
            }),
    ]
}

fn render_preview(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let id = p.node_id("id")?.unwrap_or(ctx.current_page);
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    if matches!(node.kind, NodeKind::Document) {
        return Err(p.bad("id", "render a page or a layer"));
    }
    let is_page = matches!(node.kind, NodeKind::Page);
    let mut scale = p.f64("scale")?.unwrap_or(1.0);
    if !(0.01..=8.0).contains(&scale) {
        return Err(p.bad("scale", "must be between 0.01 and 8"));
    }
    let max = f64::from(p.u32("maxSize")?.unwrap_or(1600).clamp(16, 8192));
    let bounds = deesainer_render::render_bounds(ctx.doc, id)
        .ok_or_else(|| p.bad("id", "nothing to render"))?;
    let page_bounds = if is_page {
        node.children
            .iter()
            .filter_map(|c| deesainer_render::render_bounds(ctx.doc, *c))
            .reduce(deesainer_geom::Rect::union)
    } else {
        Some(bounds)
    };
    let Some(b) = page_bounds else {
        return Err(p.invalid("the page is empty"));
    };
    let longest = (b.width.max(b.height) * scale).max(1.0);
    if longest > max {
        scale *= max / longest;
    }
    let background = p.bool("background")?.unwrap_or(is_page);
    let text = crate::text_painter();
    let opts = Options {
        background,
        text: Some(text),
        hidden: &[],
    };
    let (pix, bounds) = if is_page {
        render_page_area(ctx, id, b, scale, opts)?
    } else {
        render_node(ctx.doc, id, scale, opts).map_err(|e| p.invalid(e.to_string()))?
    };
    let png = encode_png(&pix).map_err(|e| EngineError::Internal(e.to_string()))?;
    Ok(Outcome::read(json!({
        "mimeType": "image/png",
        "data": base64::engine::general_purpose::STANDARD.encode(png),
        "width": pix.width(),
        "height": pix.height(),
        "bounds": bounds,
        "scale": scale,
    })))
}

fn render_page_area(
    ctx: &Context<'_>,
    page: deesainer_doc::NodeId,
    area: deesainer_geom::Rect,
    scale: f64,
    opts: Options<'_>,
) -> Result<(deesainer_render::Pixmap, deesainer_geom::Rect), EngineError> {
    let w = (area.width * scale).ceil().max(1.0) as u32;
    let h = (area.height * scale).ceil().max(1.0) as u32;
    let vp = deesainer_render::Viewport {
        transform: deesainer_geom::Affine::scale(scale, scale)
            .then(deesainer_geom::Affine::translate(-area.x, -area.y)),
        width: w,
        height: h,
    };
    let (pix, _) = deesainer_render::render_page(ctx.doc, page, &vp, opts).map_err(|e| {
        EngineError::Invalid {
            command: "view.renderPreview".into(),
            reason: e.to_string(),
        }
    })?;
    Ok((pix, area))
}

fn hit_test(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let pt = Point::new(p.req_f64("x")?, p.req_f64("y")?);
    let deep = p.bool("deep")?.unwrap_or(false);
    let path = deesainer_vector::hit_path(ctx.doc, ctx.current_page, pt, 0.0);
    let hit = deesainer_vector::hit_test(ctx.doc, ctx.current_page, pt, 0.0, deep);
    Ok(Outcome::read(json!({ "id": hit, "path": path })))
}
