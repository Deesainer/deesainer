use std::path::Path;

use deesainer_doc::Document;

const THUMBNAIL_PX: f64 = 512.0;

pub fn load_document(path: &Path) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let is_fig = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("fig"))
        || bytes.starts_with(b"fig-");
    if is_fig {
        return crate::import_fig(&bytes)
            .map(|(doc, _)| doc)
            .map_err(|e| format!("{}: {e}", path.display()));
    }
    deesainer_format::read(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn thumbnail(doc: &Document) -> Option<Vec<u8>> {
    let page = doc.first_page();
    let bounds = doc
        .get(page)?
        .children
        .iter()
        .filter_map(|c| deesainer_render::render_bounds(doc, *c))
        .reduce(deesainer_geom::Rect::union)?;
    let scale = (THUMBNAIL_PX / bounds.width.max(bounds.height).max(1.0)).min(2.0);
    crate::export_png(doc, page, scale).ok()
}

pub fn save_document(doc: &Document, path: &Path) -> Result<(), String> {
    write_atomically(doc, &[], path)
}

pub fn write_atomically(
    doc: &Document,
    versions: &[deesainer_format::Version],
    path: &Path,
) -> Result<(), String> {
    let bytes = deesainer_format::write_bundle(doc, thumbnail(doc).as_deref(), versions)
        .map_err(|e| e.to_string())?;
    let tmp = path.with_extension("deesainer.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}
