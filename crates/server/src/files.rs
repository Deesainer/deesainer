use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deesainer_sync::Role;
use serde::{Deserialize, Serialize};

use crate::accounts::UserId;

pub type FileId = u64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: FileId,
    pub name: String,
    pub owner: UserId,
    pub created: u64,
    #[serde(default)]
    pub members: BTreeMap<UserId, Role>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    files: Vec<FileInfo>,
    next_id: FileId,
}

pub struct Files {
    root: PathBuf,
    index: Index,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err("a file name needs 1 to 120 characters".into());
    }
    Ok(name.to_owned())
}

impl Files {
    pub fn open(root: &Path) -> Result<Self, String> {
        let index = match std::fs::read(root.join("files.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("files.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Index::default(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Self {
            root: root.to_owned(),
            index,
        })
    }

    fn save(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&self.index).map_err(|e| e.to_string())?;
        let path = self.root.join("files.json");
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    pub fn document_path(&self, id: FileId) -> PathBuf {
        self.root
            .join("files")
            .join(id.to_string())
            .join("doc.deesainer")
    }

    pub fn get(&self, id: FileId) -> Option<&FileInfo> {
        self.index.files.iter().find(|f| f.id == id)
    }

    pub fn role(&self, user: UserId, admin: bool, id: FileId) -> Option<Role> {
        let f = self.get(id)?;
        if admin || f.owner == user {
            return Some(Role::Editor);
        }
        f.members.get(&user).copied()
    }

    pub fn visible_to(&self, user: UserId, admin: bool) -> Vec<(&FileInfo, Role)> {
        let mut out: Vec<(&FileInfo, Role)> = self
            .index
            .files
            .iter()
            .filter_map(|f| Some((f, self.role(user, admin, f.id)?)))
            .collect();
        out.sort_by(|a, b| b.0.created.cmp(&a.0.created).then(b.0.id.cmp(&a.0.id)));
        out
    }

    pub fn create(
        &mut self,
        owner: UserId,
        name: &str,
        document: Option<&[u8]>,
    ) -> Result<FileId, String> {
        let name = clean_name(name)?;
        let doc = match document {
            Some(bytes) => {
                deesainer_format::read(bytes).map_err(|e| format!("not a Deesainer file: {e}"))?
            }
            None => deesainer_engine::doc::Document::new(),
        };
        self.index.next_id = self.index.next_id.max(1);
        let id = self.index.next_id;
        let path = self.document_path(id);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        deesainer_io::files::save_document(&doc, &path)?;
        self.index.next_id += 1;
        self.index.files.push(FileInfo {
            id,
            name,
            owner,
            created: now(),
            members: BTreeMap::new(),
        });
        self.save()?;
        Ok(id)
    }

    pub fn can_manage(&self, user: UserId, admin: bool, id: FileId) -> bool {
        admin || self.get(id).is_some_and(|f| f.owner == user)
    }

    pub fn set_member(
        &mut self,
        id: FileId,
        member: UserId,
        role: Option<Role>,
    ) -> Result<(), String> {
        let f = self
            .index
            .files
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or("no such file")?;
        if f.owner == member {
            return Err("the owner always edits".into());
        }
        match role {
            Some(r) => {
                f.members.insert(member, r);
            }
            None => {
                f.members.remove(&member);
            }
        }
        self.save()
    }

    pub fn rename(&mut self, id: FileId, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        let f = self
            .index
            .files
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or("no such file")?;
        f.name = name;
        self.save()
    }

    pub fn delete(&mut self, id: FileId) -> Result<(), String> {
        self.index.files.retain(|f| f.id != id);
        self.save()?;
        let dir = self.root.join("files").join(id.to_string());
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_have_owners_members_and_folders() {
        let root = std::env::temp_dir().join(format!("deesainer-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut files = Files::open(&root).unwrap();
        let (ada, bob, eve) = (1, 2, 3);
        let id = files.create(ada, "Landing page", None).unwrap();
        assert!(files.document_path(id).exists());
        assert!(files.create(ada, "  ", None).is_err());
        assert!(files.create(ada, "Bad", Some(b"not a bundle")).is_err());
        assert_eq!(files.role(ada, false, id), Some(Role::Editor));
        assert_eq!(files.role(bob, false, id), None);
        assert_eq!(
            files.role(eve, true, id),
            Some(Role::Editor),
            "admins see everything"
        );
        files.set_member(id, bob, Some(Role::Viewer)).unwrap();
        assert_eq!(files.role(bob, false, id), Some(Role::Viewer));
        assert!(files.set_member(id, ada, Some(Role::Viewer)).is_err());
        assert_eq!(files.visible_to(bob, false).len(), 1);
        assert!(files.visible_to(eve, false).is_empty());
        assert!(files.can_manage(ada, false, id) && !files.can_manage(bob, false, id));
        let mut again = Files::open(&root).unwrap();
        assert_eq!(
            again.get(id).unwrap().members.get(&bob),
            Some(&Role::Viewer)
        );
        again.delete(id).unwrap();
        assert!(!again.document_path(id).exists());
        assert!(again.get(id).is_none());
    }
}
