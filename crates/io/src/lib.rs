mod fig_import;
mod figma_rest;
pub mod files;
mod pdf;
mod svg_export;
mod svg_import;

use std::collections::BTreeMap;

use deesainer_doc::{Document, Node, NodeId, NodeKind, PropValue, VectorPath};

pub use fig_import::{import_fig, FigReport};
pub use figma_rest::{figma_file_key, import_figma_json, ImageBytes};
pub use pdf::export_pdf;
pub use svg_export::export_svg;
pub use svg_import::{import_svg, Imported};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("no node with id {0}")]
    UnknownNode(NodeId),
    #[error("render failed: {0}")]
    Render(String),
    #[error("invalid SVG: {0}")]
    BadSvg(String),
    #[error("{0}")]
    Other(String),
}

pub struct TextOutlines;

impl deesainer_render::TextPainter for TextOutlines {
    fn outlines(&self, node: &Node) -> Vec<VectorPath> {
        deesainer_text::node_outlines(node)
    }

    fn parts(&self, node: &Node) -> (Vec<VectorPath>, Vec<(deesainer_doc::Paint, VectorPath)>) {
        let (base, colored) = deesainer_text::node_parts(node);
        (vec![base], colored)
    }
}

pub static TEXT: TextOutlines = TextOutlines;

pub fn export_png(doc: &Document, id: NodeId, scale: f64) -> Result<Vec<u8>, IoError> {
    let node = doc.get(id).ok_or(IoError::UnknownNode(id))?;
    let opts = deesainer_render::Options {
        background: matches!(node.kind, NodeKind::Page),
        text: Some(&TEXT),
        hidden: &[],
    };
    let (pix, _) = deesainer_render::render_node(doc, id, scale, opts)
        .map_err(|e| IoError::Render(e.to_string()))?;
    deesainer_render::encode_png(&pix).map_err(|e| IoError::Render(e.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterFormat {
    Jpeg(u8),
    WebP,
}

pub struct Raster {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn export_raster(
    doc: &Document,
    id: NodeId,
    scale: f64,
    format: RasterFormat,
) -> Result<Raster, IoError> {
    use image::{ExtendedColorType, ImageEncoder};
    let node = doc.get(id).ok_or(IoError::UnknownNode(id))?;
    let opts = deesainer_render::Options {
        background: matches!(node.kind, NodeKind::Page),
        text: Some(&TEXT),
        hidden: &[],
    };
    let (pix, _) = deesainer_render::render_node(doc, id, scale, opts)
        .map_err(|e| IoError::Render(e.to_string()))?;
    let (w, h) = (pix.width(), pix.height());
    let mut out = Vec::new();
    match format {
        RasterFormat::Jpeg(q) => {
            let mut rgb = Vec::with_capacity(pix.data().len() / 4 * 3);
            for px in pix.data().as_chunks::<4>().0.iter() {
                let inv = 255 - u16::from(px[3]);
                for c in &px[..3] {
                    rgb.push((u16::from(*c) + inv).min(255) as u8);
                }
            }
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q.clamp(1, 100))
                .write_image(&rgb, w, h, ExtendedColorType::Rgb8)
                .map_err(|e| IoError::Render(e.to_string()))?;
        }
        RasterFormat::WebP => {
            let mut rgba = Vec::with_capacity(pix.data().len());
            for px in pix.data().as_chunks::<4>().0.iter() {
                let a = u32::from(px[3]);
                match 255u32.checked_div(a) {
                    None => rgba.extend_from_slice(&[0, 0, 0, 0]),
                    Some(_) => {
                        for c in &px[..3] {
                            rgba.push(((u32::from(*c) * 255 + a / 2) / a).min(255) as u8);
                        }
                        rgba.push(px[3]);
                    }
                }
            }
            image::codecs::webp::WebPEncoder::new_lossless(&mut out)
                .write_image(&rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(|e| IoError::Render(e.to_string()))?;
        }
    }
    Ok(Raster {
        bytes: out,
        width: w,
        height: h,
    })
}

pub(crate) struct Resolver<'a> {
    pub doc: &'a Document,
    stack: Vec<BTreeMap<NodeId, Vec<PropValue>>>,
    slots: Vec<BTreeMap<NodeId, NodeId>>,
}

impl<'a> Resolver<'a> {
    pub fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            stack: Vec::new(),
            slots: Vec::new(),
        }
    }

    pub fn node(&self, id: NodeId) -> Option<Node> {
        let mut n = self.doc.get(id)?.clone();
        for map in self.stack.iter().rev() {
            if let Some(values) = map.get(&id) {
                for v in values {
                    n.set(v.clone());
                }
            }
        }
        Some(n)
    }

    pub fn children(&mut self, node: &Node) -> (Vec<NodeId>, bool) {
        if let Some(content) = self.slots.last().and_then(|m| m.get(&node.id)) {
            let kids = self
                .doc
                .get(*content)
                .map(|c| c.children.clone())
                .unwrap_or_default();
            return (kids, false);
        }
        match &node.kind {
            NodeKind::BooleanOp { .. } => (Vec::new(), false),
            NodeKind::Instance {
                component,
                overrides,
            } => {
                if self.stack.len() >= 32 {
                    return (Vec::new(), false);
                }
                let Some(main) = self.doc.get(*component) else {
                    return (Vec::new(), false);
                };
                let mut map: BTreeMap<NodeId, Vec<PropValue>> = BTreeMap::new();
                for (cid, rect) in deesainer_render::instance_child_rects(
                    self.doc,
                    main,
                    node.rect.width,
                    node.rect.height,
                ) {
                    map.entry(cid).or_default().push(PropValue::Rect(rect));
                }
                for o in overrides {
                    map.entry(o.target).or_default().push(o.value.clone());
                }
                self.stack.push(map);
                self.slots
                    .push(self.doc.slot_fills_from(node.id, *component));
                (main.children.clone(), true)
            }
            _ => (node.children.clone(), false),
        }
    }

    pub fn pop(&mut self) {
        self.stack.pop();
        self.slots.pop();
    }

    pub fn text_colored(&self, node: &Node) -> Vec<(deesainer_doc::Paint, VectorPath)> {
        deesainer_text::node_parts(node).1
    }

    pub fn outline(&self, node: &Node) -> Option<VectorPath> {
        match &node.kind {
            NodeKind::Text(_) => Some(deesainer_text::node_parts(node).0),
            _ => deesainer_vector::resolved_outline(self.doc, node),
        }
    }
}

pub(crate) fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(0.123456), "0.123");
        assert_eq!(num(f64::NAN), "0");
    }
}

#[cfg(test)]
mod raster_tests {
    use super::*;
    use deesainer_color::Rgba;

    fn doc() -> (Document, NodeId) {
        let doc = Document::with_frame(40.0, 30.0, Rgba::new(1.0, 0.0, 0.0, 1.0)).unwrap();
        let frame = doc.get(doc.first_page()).unwrap().children[0];
        (doc, frame)
    }

    #[test]
    fn jpeg_and_webp_round_trip_with_the_right_size_and_colour() {
        let (doc, id) = doc();
        let jpg = export_raster(&doc, id, 2.0, RasterFormat::Jpeg(90))
            .unwrap()
            .bytes;
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        let img = image::load_from_memory(&jpg).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (80, 60));
        let px = img.get_pixel(40, 30);
        assert!(px[0] > 240 && px[1] < 20 && px[2] < 20, "{px:?}");
        let webp = export_raster(&doc, id, 1.0, RasterFormat::WebP)
            .unwrap()
            .bytes;
        assert_eq!(&webp[..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
        let img = image::load_from_memory(&webp).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (40, 30));
        assert_eq!(img.get_pixel(10, 10).0, [255, 0, 0, 255]);
    }
}
