use deesainer_color::Rgba;
use deesainer_doc::Document;
use deesainer_geom::Rect;
use serde_json::json;

use super::param;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Effect, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("file.new", "New design file", file_new)
        .menu("File/New design file")
        .key("Cmd+N")
        .doc("Replaces the document with a new one. With width/height, the first page holds one frame of that size. Clears history and selection.")
        .params(const {
            &[
                param("width", ParamType::Number, false, "Frame width; omit both sizes for an empty page."),
                param("height", ParamType::Number, false, "Frame height."),
                param("background", ParamType::Color, false, "Frame fill, default #FFFFFF."),
            ]
        })]
}

fn file_new(_: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let doc = match (p.f64("width")?, p.f64("height")?) {
        (None, None) => Document::new(),
        (w, h) => {
            let (w, h) = (w.unwrap_or(1440.0), h.unwrap_or(1024.0));
            if !Rect::new(0.0, 0.0, w, h).is_valid() {
                return Err(p.bad("width/height", "must be >= 0"));
            }
            let background = p.color("background")?.unwrap_or(Rgba::WHITE);
            Document::with_frame(w, h, background)?
        }
    };
    let page = doc.first_page();
    let mut outcome = Outcome::effect(Effect::Replace(Box::new(doc), Vec::new()))
        .on_page(page)
        .selecting(Vec::new());
    outcome.reply = json!({ "page": page });
    Ok(outcome)
}
