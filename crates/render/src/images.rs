use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use deesainer_doc::{AssetId, Document};
use tiny_skia::Pixmap;

const MAX_PIXELS: u64 = 64 * 1024 * 1024;

fn cache() -> &'static Mutex<HashMap<AssetId, Option<Arc<Pixmap>>>> {
    static CACHE: OnceLock<Mutex<HashMap<AssetId, Option<Arc<Pixmap>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn decode(bytes: &[u8]) -> Option<Pixmap> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS || w == 0 || h == 0 {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        for c in px.iter_mut().take(3) {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)
}

pub fn get(doc: &Document, id: &AssetId) -> Option<Arc<Pixmap>> {
    if let Ok(map) = cache().lock() {
        if let Some(hit) = map.get(id) {
            return hit.clone();
        }
    }
    let decoded = doc.asset(id).and_then(|b| decode(b)).map(Arc::new);
    if let Ok(mut map) = cache().lock() {
        if map.len() > 512 {
            map.clear();
        }
        map.insert(id.clone(), decoded.clone());
    }
    decoded
}

type AdjustedCache = Mutex<HashMap<(AssetId, u64), Option<Arc<Pixmap>>>>;

fn adjusted_cache() -> &'static AdjustedCache {
    static CACHE: OnceLock<AdjustedCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn adjust_pixmap(img: &Pixmap, adjust: &deesainer_doc::ImageAdjust) -> Option<Pixmap> {
    let mut out = img.clone();
    for px in out.data_mut().as_chunks_mut::<4>().0 {
        let a = f32::from(px[3]) / 255.0;
        if a <= 0.0 {
            continue;
        }
        let un = |c: u8| (f32::from(c) / 255.0 / a).min(1.0);
        let [r, g, b] = adjust.apply([un(px[0]), un(px[1]), un(px[2])]);
        let q = |v: f32| (v * a * 255.0 + 0.5) as u8;
        px[0] = q(r);
        px[1] = q(g);
        px[2] = q(b);
    }
    Some(out)
}

pub fn get_adjusted(
    doc: &Document,
    id: &AssetId,
    adjust: &deesainer_doc::ImageAdjust,
) -> Option<Arc<Pixmap>> {
    let key = adjust.key();
    if key == 0 {
        return get(doc, id);
    }
    if let Ok(map) = adjusted_cache().lock() {
        if let Some(hit) = map.get(&(id.clone(), key)) {
            return hit.clone();
        }
    }
    let made = get(doc, id)
        .and_then(|img| adjust_pixmap(&img, adjust))
        .map(Arc::new);
    if let Ok(mut map) = adjusted_cache().lock() {
        if map.len() > 64 {
            map.clear();
        }
        map.insert((id.clone(), key), made.clone());
    }
    made
}

pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}
