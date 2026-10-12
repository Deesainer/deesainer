use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deesainer_doc::{AssetId, Document};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT: &str = "deesainer";
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "deesainer";

pub const MAX_ENTRIES: usize = 100_000;
pub const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_DOCUMENT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a Deesainer file: {0}")]
    NotDeesainer(String),
    #[error("this file was written by a newer Deesainer (format {0}); update to open it")]
    TooNew(u32),
    #[error("the file is damaged: {0}")]
    Corrupt(String),
    #[error("the file is too large: {0}")]
    TooLarge(String),
    #[error("could not write the file: {0}")]
    Write(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub assets: Vec<AssetId>,
    #[serde(default)]
    pub has_thumbnail: bool,
}

pub fn asset_id(bytes: &[u8]) -> AssetId {
    AssetId(
        blake3::hash(bytes)
            .to_hex()
            .as_str()
            .chars()
            .take(32)
            .collect(),
    )
}

fn zip_err(e: impl std::fmt::Display) -> FormatError {
    FormatError::Corrupt(e.to_string())
}

pub const MAX_VERSIONS: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    pub id: u64,
    pub label: String,
    pub saved_at: String,
    pub author: String,
    pub document: Arc<Document>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionMeta {
    id: u64,
    label: String,
    #[serde(default)]
    saved_at: String,
    #[serde(default)]
    author: String,
}

pub fn write(doc: &Document, thumbnail: Option<&[u8]>) -> Result<Vec<u8>, FormatError> {
    write_bundle(doc, thumbnail, &[])
}

pub fn write_bundle(
    doc: &Document,
    thumbnail: Option<&[u8]>,
    versions: &[Version],
) -> Result<Vec<u8>, FormatError> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflate = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut all: BTreeMap<AssetId, Arc<Vec<u8>>> = BTreeMap::new();
    for d in std::iter::once(doc).chain(versions.iter().map(|v| v.document.as_ref())) {
        for (id, bytes) in d.assets() {
            all.entry(id.clone()).or_insert_with(|| Arc::clone(bytes));
        }
    }
    let assets: Vec<AssetId> = all.keys().cloned().collect();
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        app: format!("deesainer {}", env!("CARGO_PKG_VERSION")),
        assets: assets.clone(),
        has_thumbnail: thumbnail.is_some(),
    };
    let w = |e: zip::result::ZipError| FormatError::Write(e.to_string());
    let io = |e: std::io::Error| FormatError::Write(e.to_string());
    zip.start_file("manifest.json", deflate).map_err(w)?;
    zip.write_all(
        &serde_json::to_vec_pretty(&manifest).map_err(|e| FormatError::Write(e.to_string()))?,
    )
    .map_err(io)?;
    zip.start_file("document.json", deflate).map_err(w)?;
    zip.write_all(&serde_json::to_vec(doc).map_err(|e| FormatError::Write(e.to_string()))?)
        .map_err(io)?;
    for (id, bytes) in &all {
        zip.start_file(format!("assets/{id}"), stored).map_err(w)?;
        zip.write_all(bytes).map_err(io)?;
    }
    if !versions.is_empty() {
        let enc = |e: serde_json::Error| FormatError::Write(e.to_string());
        let index: Vec<VersionMeta> = versions
            .iter()
            .map(|v| VersionMeta {
                id: v.id,
                label: v.label.clone(),
                saved_at: v.saved_at.clone(),
                author: v.author.clone(),
            })
            .collect();
        zip.start_file("history/index.json", deflate).map_err(w)?;
        zip.write_all(&serde_json::to_vec_pretty(&index).map_err(enc)?)
            .map_err(io)?;
        for v in versions {
            zip.start_file(format!("history/{}.json", v.id), deflate)
                .map_err(w)?;
            zip.write_all(&serde_json::to_vec(v.document.as_ref()).map_err(enc)?)
                .map_err(io)?;
        }
    }
    if let Some(png) = thumbnail {
        zip.start_file("thumbnail.png", stored).map_err(w)?;
        zip.write_all(png).map_err(io)?;
    }
    let cursor = zip.finish().map_err(w)?;
    Ok(cursor.into_inner())
}

fn read_entry(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    limit: u64,
    budget: &mut u64,
) -> Result<Vec<u8>, FormatError> {
    let entry = archive
        .by_name(name)
        .map_err(|e| FormatError::Corrupt(format!("{name}: {e}")))?;
    let size = entry.size();
    if size > limit || size > *budget {
        return Err(FormatError::TooLarge(format!("{name} is {size} bytes")));
    }
    let mut out = Vec::with_capacity(size as usize);
    entry
        .take(limit.min(*budget) + 1)
        .read_to_end(&mut out)
        .map_err(zip_err)?;
    if out.len() as u64 > limit.min(*budget) {
        return Err(FormatError::TooLarge(format!(
            "{name} expands beyond its declared size"
        )));
    }
    *budget -= out.len() as u64;
    Ok(out)
}

fn migrate(doc: Value, from: u32) -> Result<Value, FormatError> {
    match from {
        VERSION => Ok(doc),
        0 => Err(FormatError::Corrupt("unknown format version 0".into())),
        v => Err(FormatError::TooNew(v)),
    }
}

pub fn read(bytes: &[u8]) -> Result<Document, FormatError> {
    read_with_thumbnail(bytes).map(|(d, _)| d)
}

pub fn read_with_thumbnail(bytes: &[u8]) -> Result<(Document, Option<Vec<u8>>), FormatError> {
    read_bundle(bytes).map(|(d, t, _)| (d, t))
}

pub type Bundle = (Document, Option<Vec<u8>>, Vec<Version>);

pub fn read_bundle(bytes: &[u8]) -> Result<Bundle, FormatError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| FormatError::NotDeesainer(e.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(FormatError::TooLarge(format!("{} entries", archive.len())));
    }
    let mut budget = MAX_TOTAL_BYTES;
    let manifest_bytes = read_entry(&mut archive, "manifest.json", 1024 * 1024, &mut budget)
        .map_err(|_| FormatError::NotDeesainer("no manifest.json".into()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| FormatError::NotDeesainer(format!("bad manifest: {e}")))?;
    if manifest.format != FORMAT {
        return Err(FormatError::NotDeesainer(format!(
            "format is {:?}",
            manifest.format
        )));
    }
    if manifest.version > VERSION {
        return Err(FormatError::TooNew(manifest.version));
    }
    let doc_bytes = read_entry(
        &mut archive,
        "document.json",
        MAX_DOCUMENT_BYTES,
        &mut budget,
    )?;
    let value: Value = serde_json::from_slice(&doc_bytes).map_err(zip_err)?;
    let value = migrate(value, manifest.version)?;
    let mut doc: Document = serde_json::from_value(value).map_err(zip_err)?;
    doc.validate().map_err(zip_err)?;
    let mut assets: BTreeMap<AssetId, Arc<Vec<u8>>> = BTreeMap::new();
    for id in &manifest.assets {
        if id.0.is_empty() || !id.0.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(FormatError::Corrupt(format!("bad asset name {id}")));
        }
        let data = read_entry(
            &mut archive,
            &format!("assets/{id}"),
            MAX_TOTAL_BYTES,
            &mut budget,
        )?;
        if asset_id(&data) != *id {
            return Err(FormatError::Corrupt(format!(
                "asset {id} does not match its content"
            )));
        }
        assets.insert(id.clone(), Arc::new(data));
    }
    for (id, data) in &assets {
        doc.insert_asset(id.clone(), Arc::clone(data));
    }
    let thumbnail = if manifest.has_thumbnail {
        read_entry(&mut archive, "thumbnail.png", 64 * 1024 * 1024, &mut budget).ok()
    } else {
        None
    };
    let mut versions = Vec::new();
    if archive.index_for_name("history/index.json").is_some() {
        let raw = read_entry(
            &mut archive,
            "history/index.json",
            16 * 1024 * 1024,
            &mut budget,
        )?;
        let index: Vec<VersionMeta> = serde_json::from_slice(&raw).map_err(zip_err)?;
        if index.len() > MAX_VERSIONS {
            return Err(FormatError::TooLarge(format!("{} versions", index.len())));
        }
        for meta in index {
            let raw = read_entry(
                &mut archive,
                &format!("history/{}.json", meta.id),
                MAX_DOCUMENT_BYTES,
                &mut budget,
            )?;
            let value: Value = serde_json::from_slice(&raw).map_err(zip_err)?;
            let value = migrate(value, manifest.version)?;
            let mut vdoc: Document = serde_json::from_value(value).map_err(zip_err)?;
            vdoc.validate().map_err(zip_err)?;
            for (id, data) in &assets {
                vdoc.insert_asset(id.clone(), Arc::clone(data));
            }
            versions.push(Version {
                id: meta.id,
                label: meta.label,
                saved_at: meta.saved_at,
                author: meta.author,
                document: Arc::new(vdoc),
            });
        }
    }
    Ok((doc, thumbnail, versions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_color::Rgba;

    #[test]
    fn round_trip_with_assets() {
        let mut doc = Document::with_frame(100.0, 50.0, Rgba::WHITE).unwrap();
        let data = b"not really a png but bytes".to_vec();
        doc.insert_asset(asset_id(&data), Arc::new(data));
        let bytes = write(&doc, Some(b"thumb")).unwrap();
        let (back, thumb) = read_with_thumbnail(&bytes).unwrap();
        assert_eq!(back, doc);
        assert_eq!(thumb.as_deref(), Some(&b"thumb"[..]));
        assert_eq!(write(&back, Some(b"thumb")).unwrap().len(), bytes.len());
    }

    #[test]
    fn numbers_survive_save_and_open_exactly() {
        let mut doc = Document::with_frame(100.0, 50.0, Rgba::WHITE).unwrap();
        let id = doc
            .pages()
            .first()
            .and_then(|p| doc.get(*p))
            .map(|p| p.children[0])
            .unwrap();
        if let Some(n) = doc.get_mut(id) {
            n.rect.x = 0.380_298_092_389_822_76;
            n.rect.width = 3.661_666_870_117_187_5;
            n.rotation = 0.1 + 0.2;
        }
        let back = read(&write(&doc, None).unwrap()).unwrap();
        let n = back.get(id).unwrap();
        assert_eq!(n.rect.x.to_bits(), 0.380_298_092_389_822_76_f64.to_bits());
        assert_eq!(
            n.rect.width.to_bits(),
            3.661_666_870_117_187_5_f64.to_bits()
        );
        assert_eq!(n.rotation.to_bits(), (0.1_f64 + 0.2).to_bits());
        assert_eq!(back, doc);
    }

    #[test]
    fn rejects_garbage_and_tampering() {
        assert!(matches!(read(b"hello"), Err(FormatError::NotDeesainer(_))));
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("x.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        let other = zip.finish().unwrap().into_inner();
        assert!(matches!(read(&other), Err(FormatError::NotDeesainer(_))));
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"format":"deesainer","version":99}"#)
            .unwrap();
        let newer = zip.finish().unwrap().into_inner();
        assert!(matches!(read(&newer), Err(FormatError::TooNew(99))));
    }

    #[test]
    fn versions_round_trip_with_their_assets() {
        let mut old = Document::with_frame(10.0, 10.0, Rgba::WHITE).unwrap();
        let data = b"an image only the old version uses".to_vec();
        let aid = asset_id(&data);
        old.insert_asset(aid.clone(), Arc::new(data));
        let now = Document::with_frame(20.0, 20.0, Rgba::BLACK).unwrap();
        let v = Version {
            id: 7,
            label: "Before the redesign".into(),
            saved_at: "2026-10-11T10:00:00Z".into(),
            author: "ada".into(),
            document: Arc::new(old.clone()),
        };
        let bytes = write_bundle(&now, None, std::slice::from_ref(&v)).unwrap();
        let (doc, _, versions) = read_bundle(&bytes).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].label, "Before the redesign");
        assert!(versions[0].document.asset(&aid).is_some());
        assert_eq!(
            versions[0].document.get(versions[0].document.first_page()),
            old.get(old.first_page())
        );
        assert_eq!(doc.get(doc.first_page()), now.get(now.first_page()));
        assert!(read(&bytes).is_ok());
    }
}
