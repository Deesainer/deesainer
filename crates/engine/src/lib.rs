mod commands;
pub mod compare;
pub mod components;
pub mod constraints;
mod derive;
mod error;
pub mod lint;
mod params;
pub mod preview;
mod registry;
mod session;
pub mod slots;

pub use error::EngineError;
pub use params::Params;
pub use registry::{
    always, has_selection, CommandKind, CommandRegistry, CommandSpec, Context, Effect, Outcome,
    ParamSpec, ParamType, RunFn,
};
pub use session::Session;

pub use deesainer_doc as doc;
pub use deesainer_ops as ops;

struct ShapedText;

impl deesainer_render::TextPainter for ShapedText {
    fn outlines(&self, node: &deesainer_doc::Node) -> Vec<deesainer_doc::VectorPath> {
        deesainer_text::node_outlines(node)
    }

    fn parts(
        &self,
        node: &deesainer_doc::Node,
    ) -> (
        Vec<deesainer_doc::VectorPath>,
        Vec<(deesainer_doc::Paint, deesainer_doc::VectorPath)>,
    ) {
        let (base, colored) = deesainer_text::node_parts(node);
        (vec![base], colored)
    }
}

pub fn text_painter() -> &'static dyn deesainer_render::TextPainter {
    static PAINTER: ShapedText = ShapedText;
    &PAINTER
}

pub fn text_size(
    _node: &deesainer_doc::Node,
    data: &deesainer_doc::TextData,
    width: Option<f64>,
) -> (f64, f64) {
    deesainer_text::measure(data, width)
}

pub fn text_preview(
    doc: &deesainer_doc::Document,
    id: deesainer_doc::NodeId,
    data: &deesainer_doc::TextData,
) -> Result<deesainer_doc::Document, String> {
    let node = doc.get(id).ok_or("text layer no longer exists")?;
    if node.text().is_none() {
        return Err("layer is no longer text".into());
    }
    if data.content.len() > 1_000_000 {
        return Err("text is too large".into());
    }
    let mut preview = doc.clone();
    let node = preview.get_mut(id).ok_or("text layer no longer exists")?;
    let width = match data.auto_resize {
        deesainer_doc::TextAutoResize::WidthAndHeight => None,
        _ => Some(node.rect.width),
    };
    let (w, h) = deesainer_text::measure(data, width);
    if data.auto_resize == deesainer_doc::TextAutoResize::WidthAndHeight {
        node.rect.width = w;
    }
    if data.auto_resize != deesainer_doc::TextAutoResize::None {
        node.rect.height = h;
    }
    node.kind = deesainer_doc::NodeKind::Text(data.clone());
    Ok(preview)
}

pub fn instance_text_preview(
    doc: &deesainer_doc::Document,
    instance: deesainer_doc::NodeId,
    target: deesainer_doc::NodeId,
    data: &deesainer_doc::TextData,
) -> Result<deesainer_doc::Document, String> {
    use deesainer_doc::{NodeKind, Override, PropKey, PropValue};
    if data.content.len() > 1_000_000 {
        return Err("text is too large".into());
    }
    let mut preview = doc.clone();
    let node = preview
        .get_mut(instance)
        .ok_or("instance no longer exists")?;
    let NodeKind::Instance { overrides, .. } = &mut node.kind else {
        return Err("layer is no longer an instance".into());
    };
    overrides.retain(|o| !(o.target == target && o.value.key() == PropKey::Kind));
    overrides.push(Override {
        target,
        value: PropValue::Kind(NodeKind::Text(data.clone())),
    });
    Ok(preview)
}
