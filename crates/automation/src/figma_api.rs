use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use deesainer_doc::Document;
use deesainer_io::{FigReport, ImageBytes};
use serde_json::Value;

const API: &str = "https://api.figma.com/v1";
const MAX_BODY: u64 = 256 * 1024 * 1024;

fn token_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".deesainer").join("figma-token"))
}

pub fn token() -> Option<String> {
    if let Ok(t) = std::env::var("FIGMA_TOKEN") {
        if !t.trim().is_empty() {
            return Some(t.trim().to_owned());
        }
    }
    let t = std::fs::read_to_string(token_path()?).ok()?;
    let t = t.trim().to_owned();
    (!t.is_empty()).then_some(t)
}

pub fn save_token(token: &str) -> Result<(), String> {
    let path = token_path().ok_or("no home directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, token.trim().as_bytes()).map_err(|e| e.to_string())
}

pub fn clear_token() {
    if let Some(p) = token_path() {
        let _ = std::fs::remove_file(p);
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .https_only(true)
        .build()
        .into()
}

fn read_body(resp: ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    resp.into_body()
        .into_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    if out.len() as u64 > MAX_BODY {
        return Err("response larger than 256 MB".into());
    }
    Ok(out)
}

fn get_json(agent: &ureq::Agent, url: &str, token: &str) -> Result<Value, String> {
    let resp = agent
        .get(url)
        .header("X-Figma-Token", token)
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(403) => {
                "Figma refused the token (403): check it and its file access".to_owned()
            }
            ureq::Error::StatusCode(404) => "Figma file not found (404)".to_owned(),
            ureq::Error::StatusCode(429) => {
                "Figma rate limit reached (429): try again later".to_owned()
            }
            e => format!("Figma API: {e}"),
        })?;
    let body = read_body(resp)?;
    serde_json::from_slice(&body).map_err(|e| format!("Figma API returned invalid JSON: {e}"))
}

pub fn import(url_or_key: &str, token: &str) -> Result<(Document, FigReport, String), String> {
    let key = deesainer_io::figma_file_key(url_or_key).ok_or("not a Figma file link or key")?;
    let agent = agent();
    let file = get_json(&agent, &format!("{API}/files/{key}?geometry=paths"), token)?;
    let name = file
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Figma file")
        .to_owned();
    let mut images = ImageBytes::new();
    if let Ok(list) = get_json(&agent, &format!("{API}/files/{key}/images"), token) {
        if let Some(map) = list.pointer("/meta/images").and_then(Value::as_object) {
            for (r, u) in map {
                let Some(u) = u.as_str() else { continue };
                if !u.starts_with("https://") {
                    continue;
                }
                if let Ok(resp) = agent.get(u).call() {
                    if let Ok(bytes) = read_body(resp) {
                        images.insert(r.clone(), bytes);
                    }
                }
            }
        }
    }
    let (doc, report) =
        deesainer_io::import_figma_json(&file, &images).map_err(|e| e.to_string())?;
    Ok((doc, report, name))
}
