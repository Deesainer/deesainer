use std::collections::BTreeMap;

use deesainer_doc::Document;
use deesainer_ops::Op;
use serde::{Deserialize, Serialize};

use crate::{merge_with, summarize, ClientId, Presence, ServerMsg, ServerState, Snapshot, User};

pub const MAX_BRANCHES: usize = 20;
const MAX_NAME: usize = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub author: String,
    pub base_version: u64,
    pub at: u64,
    pub slot: u64,
}

pub struct BranchRoom {
    pub info: BranchInfo,
    pub base: Document,
    pub state: ServerState,
}

pub struct StoredBranch {
    pub info: BranchInfo,
    pub base: Document,
    pub doc: Document,
    pub snapshots: Vec<Snapshot>,
}

pub struct MergeOutcome {
    pub ops: Option<ServerMsg>,
    pub applied: usize,
    pub conflicts: Vec<String>,
}

pub struct Hub {
    pub main: ServerState,
    branches: BTreeMap<String, BranchRoom>,
}

fn valid_name(name: &str) -> bool {
    (1..=MAX_NAME).contains(&name.chars().count())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
        && !name.starts_with('.')
        && name.trim() == name
}

fn error(message: impl Into<String>) -> ServerMsg {
    ServerMsg::Error {
        message: message.into(),
    }
}

impl Hub {
    pub fn new(main: ServerState) -> Self {
        Self {
            main,
            branches: BTreeMap::new(),
        }
    }

    pub fn set_users(&mut self, users: Vec<User>) -> Vec<(String, ClientId)> {
        let mut kicked: Vec<(String, ClientId)> = self
            .main
            .set_users(users.clone())
            .into_iter()
            .map(|id| (String::new(), id))
            .collect();
        for (name, b) in &mut self.branches {
            kicked.extend(
                b.state
                    .set_users(users.clone())
                    .into_iter()
                    .map(|id| (name.clone(), id)),
            );
        }
        kicked
    }

    pub fn room(&self, name: &str) -> Option<&ServerState> {
        if name.is_empty() {
            Some(&self.main)
        } else {
            self.branches.get(name).map(|b| &b.state)
        }
    }

    pub fn room_mut(&mut self, name: &str) -> Option<&mut ServerState> {
        if name.is_empty() {
            Some(&mut self.main)
        } else {
            self.branches.get_mut(name).map(|b| &mut b.state)
        }
    }

    pub fn branch_rooms(&self) -> impl Iterator<Item = &BranchRoom> {
        self.branches.values()
    }

    pub fn load_branches(&mut self, rooms: Vec<StoredBranch>) {
        for stored in rooms {
            let mut state = self.main.sibling(stored.doc);
            state.load_snapshots(stored.snapshots);
            self.branches.insert(
                stored.info.name.clone(),
                BranchRoom {
                    info: stored.info,
                    base: stored.base,
                    state,
                },
            );
        }
    }

    pub fn slot_of(&self, name: &str) -> Option<u64> {
        self.branches.get(name).map(|b| b.info.slot)
    }

    pub fn branches(&self) -> ServerMsg {
        ServerMsg::Branches {
            branches: self.branches.values().map(|b| b.info.clone()).collect(),
        }
    }

    pub fn join(
        &mut self,
        room: &str,
        name: &str,
        token: &str,
        protocol: u32,
    ) -> Result<(ClientId, ServerMsg), ServerMsg> {
        let state = self
            .room_mut(room)
            .ok_or_else(|| error(format!("no branch named {room:?}")))?;
        let (id, mut welcome) = state.join(name, token, protocol)?;
        if let ServerMsg::Welcome { branch, .. } = &mut welcome {
            *branch = room.to_owned();
        }
        Ok((id, welcome))
    }

    pub fn leave(&mut self, room: &str, client: ClientId) -> Option<ServerMsg> {
        self.room_mut(room).map(|s| s.leave(client))
    }

    pub fn submit(
        &mut self,
        room: &str,
        client: ClientId,
        seq: u64,
        ops: Vec<Op>,
    ) -> Result<ServerMsg, ServerMsg> {
        self.room_mut(room)
            .ok_or_else(|| error("this branch was deleted"))?
            .submit(client, seq, ops)
    }

    pub fn presence(
        &mut self,
        room: &str,
        client: ClientId,
        presence: Presence,
    ) -> Option<ServerMsg> {
        self.room_mut(room)?.presence(client, presence)
    }

    pub fn create_branch(
        &mut self,
        room: &str,
        client: ClientId,
        name: &str,
        at: u64,
    ) -> Result<BranchInfo, ServerMsg> {
        let author = {
            let state = self.room(room).ok_or_else(|| error("unknown room"))?;
            state.require_editor(client)?;
            state.peer_name(client)
        };
        if !valid_name(name) {
            return Err(error(
                "a branch name has 1 to 60 letters, digits, spaces, dots, dashes or underscores",
            ));
        }
        if self.branches.contains_key(name) {
            return Err(error(format!("a branch named {name:?} exists")));
        }
        if self.branches.len() >= MAX_BRANCHES {
            return Err(error(format!("at most {MAX_BRANCHES} branches")));
        }
        let slot = self
            .branches
            .values()
            .map(|b| b.info.slot)
            .max()
            .unwrap_or(0)
            + 1;
        let base = self.main.doc.clone();
        let mut doc = base.clone();
        doc.reserve_ids(slot << 32);
        let info = BranchInfo {
            name: name.to_owned(),
            author,
            base_version: self.main.version,
            at,
            slot,
        };
        let state = self.main.sibling(doc);
        self.branches.insert(
            name.to_owned(),
            BranchRoom {
                info: info.clone(),
                base,
                state,
            },
        );
        Ok(info)
    }

    pub fn merge_branch(
        &mut self,
        room: &str,
        client: ClientId,
        name: &str,
        prefer_branch: bool,
    ) -> Result<MergeOutcome, ServerMsg> {
        self.room(room)
            .ok_or_else(|| error("unknown room"))?
            .require_editor(client)?;
        let branch = self
            .branches
            .get_mut(name)
            .ok_or_else(|| error(format!("no branch named {name:?}")))?;
        let (ops, conflicts) = merge_with(
            &branch.base,
            &branch.state.doc,
            &self.main.doc,
            prefer_branch,
        );
        let applied = ops.len();
        let msg = if ops.is_empty() {
            None
        } else {
            Some(self.main.apply_server(ops).map_err(error)?)
        };
        branch.base = branch.state.doc.clone();
        Ok(MergeOutcome {
            ops: msg,
            applied,
            conflicts,
        })
    }

    pub fn preview_merge(&self, name: &str) -> Result<ServerMsg, ServerMsg> {
        let branch = self
            .branches
            .get(name)
            .ok_or_else(|| error(format!("no branch named {name:?}")))?;
        let (ops, conflicts) = merge_with(&branch.base, &branch.state.doc, &self.main.doc, false);
        let mut merged = self.main.doc.clone();
        let merged = deesainer_ops::Transaction::new("preview", ops.clone())
            .apply(&mut merged)
            .ok()
            .and_then(|_| crate::encode_document(&merged).ok());
        Ok(ServerMsg::MergePreview {
            branch: name.to_owned(),
            changes: summarize(&ops),
            conflicts,
            merged,
        })
    }

    pub fn delete_branch(
        &mut self,
        room: &str,
        client: ClientId,
        name: &str,
    ) -> Result<(), ServerMsg> {
        self.room(room)
            .ok_or_else(|| error("unknown room"))?
            .require_editor(client)?;
        self.branches
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| error(format!("no branch named {name:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientState, PROTOCOL};
    use deesainer_doc::PropValue;

    fn hub() -> Hub {
        Hub::new(ServerState::new(Document::new(), "edit").with_viewer_token("view"))
    }

    #[test]
    fn a_branch_evolves_apart_and_merges_back() {
        let mut hub = hub();
        let (m, welcome) = hub.join("", "Ada", "edit", PROTOCOL).unwrap();
        let mut ada = ClientState::from_welcome(&welcome).unwrap();
        let page = ada.confirmed.first_page();
        let info = hub.create_branch("", m, "redesign", 10).unwrap();
        assert_eq!(info.author, "Ada");
        assert!(hub.create_branch("", m, "redesign", 11).is_err());
        assert!(hub.create_branch("", m, "../etc", 11).is_err());
        assert!(hub.create_branch("", m, "", 11).is_err());
        let (b, welcome) = hub.join("redesign", "Bob", "edit", PROTOCOL).unwrap();
        let branch_client = ClientState::from_welcome(&welcome).unwrap();
        assert_eq!(branch_client.branch, "redesign");
        let rename = |name: &str| vec![Op::set(page, PropValue::Name(name.into()))];
        hub.submit("redesign", b, 1, rename("On the branch"))
            .unwrap();
        assert_eq!(hub.main.doc.get(page).unwrap().name, "Page 1");
        let ServerMsg::MergePreview {
            changes,
            conflicts,
            merged,
            ..
        } = hub.preview_merge("redesign").unwrap()
        else {
            panic!("not a preview");
        };
        assert_eq!(changes, vec!["1 property change(s)".to_string()]);
        assert!(conflicts.is_empty());
        let merged = crate::decode_document(&merged.unwrap()).unwrap();
        assert_ne!(merged.get(page).unwrap().name, "Page 1");
        assert_eq!(hub.main.doc.get(page).unwrap().name, "Page 1");
        let out = hub.merge_branch("", m, "redesign", false).unwrap();
        assert_eq!(out.applied, 1);
        assert!(out.conflicts.is_empty());
        let msg = out.ops.unwrap();
        assert_eq!(hub.main.doc.get(page).unwrap().name, "On the branch");
        let visible = ada.receive(&msg).unwrap();
        assert_eq!(visible.get(page).unwrap().name, "On the branch");
        assert_eq!(
            hub.merge_branch("", m, "redesign", false).unwrap().applied,
            0
        );
        assert!(hub.join("nope", "Eve", "edit", PROTOCOL).is_err());
        hub.delete_branch("", m, "redesign").unwrap();
        assert!(hub.join("redesign", "Eve", "edit", PROTOCOL).is_err());
    }

    #[test]
    fn viewers_cannot_branch_or_merge_and_ids_do_not_clash() {
        let mut hub = hub();
        let (m, _) = hub.join("", "Ada", "edit", PROTOCOL).unwrap();
        let (v, _) = hub.join("", "Vic", "view", PROTOCOL).unwrap();
        assert!(hub.create_branch("", v, "x", 1).is_err());
        hub.create_branch("", m, "x", 1).unwrap();
        assert!(hub.merge_branch("", v, "x", false).is_err());
        assert!(hub.delete_branch("", v, "x").is_err());
        let branch_next = hub.room("x").unwrap().doc.next_free_id().0;
        assert!(branch_next >= 1 << 32);
        assert!(hub.main.doc.next_free_id().0 < 1 << 32);
        for i in 0..MAX_BRANCHES {
            let _ = hub.create_branch("", m, &format!("b{i}"), 1);
        }
        assert!(hub.create_branch("", m, "one more", 1).is_err());
    }
}
