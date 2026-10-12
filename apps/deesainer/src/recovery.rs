use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use deesainer_automation::files;
use deesainer_doc::Document;
use serde_json::{json, Value};

pub struct Recovery {
    dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub saved_at: SystemTime,
    pub original: Option<PathBuf>,
}

impl Recovery {
    pub fn in_home() -> Option<Self> {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        Self::at(PathBuf::from(home).join(".deesainer"))
    }

    pub fn at(dir: PathBuf) -> Option<Self> {
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self { dir })
    }

    pub fn document_path(&self) -> PathBuf {
        self.dir.join("recovery.deesainer")
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join("recovery.json")
    }

    pub fn snapshot(&self, doc: &Document, original: Option<&Path>) -> Result<(), String> {
        files::save_document(doc, &self.document_path())?;
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let meta = json!({
            "savedAt": secs,
            "original": original.map(|p| p.display().to_string()),
        });
        std::fs::write(self.meta_path(), meta.to_string()).map_err(|e| e.to_string())
    }

    pub fn pending(&self) -> Option<Pending> {
        let doc = std::fs::metadata(self.document_path()).ok()?;
        let meta: Value = std::fs::read_to_string(self.meta_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let saved_at = meta["savedAt"]
            .as_u64()
            .map(|s| UNIX_EPOCH + Duration::from_secs(s))
            .or_else(|| doc.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        Some(Pending {
            saved_at,
            original: meta["original"].as_str().map(PathBuf::from),
        })
    }

    pub fn load(&self) -> Result<Document, String> {
        files::load_document(&self.document_path())
    }

    pub fn discard(&self) {
        let _ = std::fs::remove_file(self.document_path());
        let _ = std::fs::remove_file(self.meta_path());
    }
}

pub fn ago(at: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(at).map_or(0, |d| d.as_secs());
    let unit = |n: u64, one: &str| {
        if n == 1 {
            format!("1 {one} ago")
        } else {
            format!("{n} {one}s ago")
        }
    };
    match secs {
        0..=59 => "just now".to_owned(),
        60..=3599 => unit(secs / 60, "minute"),
        3600..=86_399 => unit(secs / 3600, "hour"),
        _ => unit(secs / 86_400, "day"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> Recovery {
        let dir =
            std::env::temp_dir().join(format!("deesainer-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Recovery::at(dir).unwrap()
    }

    #[test]
    fn a_snapshot_is_pending_until_discarded() {
        let r = temp("cycle");
        assert_eq!(r.pending(), None);
        let doc = Document::new();
        r.snapshot(&doc, Some(Path::new("/work/card.deesainer")))
            .unwrap();
        let p = r.pending().unwrap();
        assert_eq!(
            p.original.as_deref(),
            Some(Path::new("/work/card.deesainer"))
        );
        assert!(SystemTime::now().duration_since(p.saved_at).unwrap() < Duration::from_secs(60));
        assert!(r.load().is_ok());
        r.discard();
        assert_eq!(r.pending(), None);
    }

    #[test]
    fn a_snapshot_without_metadata_is_still_offered() {
        let r = temp("bare");
        r.snapshot(&Document::new(), None).unwrap();
        std::fs::remove_file(r.meta_path()).unwrap();
        let p = r.pending().unwrap();
        assert_eq!(p.original, None);
        r.discard();
    }

    #[test]
    fn ages_read_naturally() {
        let t = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let at = |s| ago(t, t + Duration::from_secs(s));
        assert_eq!(at(5), "just now");
        assert_eq!(at(60), "1 minute ago");
        assert_eq!(at(7200), "2 hours ago");
        assert_eq!(at(3 * 86_400), "3 days ago");
    }
}
