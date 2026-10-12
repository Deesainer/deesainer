use std::path::Path;
use std::sync::Arc;

use deesainer_engine::{CommandRegistry, Session};
use deesainer_io::files::write_atomically;
pub use deesainer_io::files::{load_document, save_document, thumbnail};

pub fn is_foreign(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("fig"))
}

pub fn open(path: &Path) -> Result<Session, String> {
    let registry = CommandRegistry::builtin().map_err(|e| e.to_string())?;
    if is_foreign(path) {
        let doc = load_document(path)?;
        return Ok(Session::with_document(doc, Arc::new(registry)));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.starts_with(b"fig-") {
        let doc = load_document(path)?;
        return Ok(Session::with_document(doc, Arc::new(registry)));
    }
    let (doc, _, versions) =
        deesainer_format::read_bundle(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut session = Session::with_document(doc, Arc::new(registry));
    session.set_versions(versions);
    Ok(session)
}

pub fn load_versions(path: &Path) -> Result<Vec<deesainer_format::Version>, String> {
    if is_foreign(path) {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.starts_with(b"fig-") {
        return Ok(Vec::new());
    }
    deesainer_format::read_bundle(&bytes)
        .map(|(_, _, v)| v)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_session(session: &Session, path: &Path) -> Result<(), String> {
    write_atomically(session.doc(), session.versions(), path)
}

pub fn resolve_in_root(
    root: &Path,
    rel: &str,
    for_write: bool,
) -> Result<std::path::PathBuf, String> {
    let bad = |why: &str| Err(format!("path {rel:?} refused: {why}"));
    if rel.is_empty() {
        return bad("empty");
    }
    if rel.starts_with('/') || rel.contains('\\') || rel.contains(':') || rel.contains('\0') {
        return bad("must be a forward-slash path relative to the automation root");
    }
    if rel
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return bad("no empty, . or .. components");
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("automation root {}: {e}", root.display()))?;
    let joined = root.join(rel);
    let check = |p: &Path| -> Result<(), String> {
        let c = p
            .canonicalize()
            .map_err(|e| format!("{}: {e}", p.display()))?;
        if c.starts_with(&root) {
            Ok(())
        } else {
            Err(format!(
                "path {rel:?} refused: it escapes the automation root"
            ))
        }
    };
    if for_write {
        let parent = joined
            .parent()
            .ok_or_else(|| format!("path {rel:?} refused: no parent"))?;
        check(parent)?;
        if joined.exists() {
            check(&joined)?;
        }
    } else {
        check(&joined)?;
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_roots_refuse_escapes() {
        let root = std::env::temp_dir().join(format!("mc-root-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), b"x").unwrap();
        assert!(resolve_in_root(&root, "sub/a.txt", false).is_ok());
        assert!(resolve_in_root(&root, "sub/new.png", true).is_ok());
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "sub/../../x",
            "sub//a.txt",
            "./a",
            "C:/x",
            "sub\\a.txt",
        ] {
            assert!(
                resolve_in_root(&root, bad, false).is_err(),
                "{bad} accepted"
            );
        }
        assert!(resolve_in_root(&root, "missing.txt", false).is_err());
        #[cfg(unix)]
        {
            let outside = std::env::temp_dir();
            let link = root.join("escape");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(resolve_in_root(&root, "escape/file.txt", true).is_err());
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
