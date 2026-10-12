mod access;
pub mod client;
mod hub;
mod merge;
pub use access::{Access, Role, User};
pub use hub::{BranchInfo, BranchRoom, Hub, MergeOutcome, StoredBranch, MAX_BRANCHES};
pub use merge::{merge, merge_with, summarize};

use std::collections::{BTreeMap, VecDeque};

use base64::Engine as _;
use deesainer_doc::{Document, NodeId};
use deesainer_geom::Point;
use deesainer_ops::{Op, Transaction};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: u32 = 1;

pub type ClientId = u32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    pub page: Option<NodeId>,
    pub cursor: Option<Point>,
    #[serde(default)]
    pub selection: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
}

pub const MAX_CHAT: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub id: ClientId,
    pub name: String,
    pub color: String,
    pub presence: Presence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientMsg {
    Hello {
        protocol: u32,
        name: String,
        #[serde(default)]
        token: String,
        #[serde(default)]
        branch: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        file: String,
    },
    Submit {
        seq: u64,
        ops: Vec<Op>,
    },
    Presence {
        presence: Presence,
    },
    SaveVersion {
        label: String,
    },
    ListVersions,
    CreateBranch {
        name: String,
    },
    MergeBranch {
        name: String,
        #[serde(default)]
        prefer_branch: bool,
    },
    PreviewMerge {
        name: String,
    },
    DeleteBranch {
        name: String,
    },
    ListBranches,
    RestoreVersion {
        id: u64,
    },
    Ping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: u64,
    pub label: String,
    pub author: String,
    pub version: u64,
    pub at: u64,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub info: VersionInfo,
    pub doc: Document,
}

pub const MAX_VERSIONS: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerMsg {
    Welcome {
        client: ClientId,
        version: u64,
        document: String,
        peers: Vec<Peer>,
        color: String,
        #[serde(default)]
        read_only: bool,
        #[serde(default)]
        role: Role,
        #[serde(default)]
        branch: String,
    },
    Ops {
        version: u64,
        author: ClientId,
        seq: u64,
        ops: Vec<Op>,
    },
    Reject {
        seq: u64,
        reason: String,
    },
    Presence {
        peer: Box<Peer>,
    },
    Versions {
        versions: Vec<VersionInfo>,
    },
    Branches {
        branches: Vec<BranchInfo>,
    },
    MergePreview {
        branch: String,
        changes: Vec<String>,
        conflicts: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        merged: Option<String>,
    },
    Merged {
        branch: String,
        applied: usize,
        conflicts: Vec<String>,
    },
    Restored {
        version: u64,
        document: String,
    },
    Left {
        client: ClientId,
    },
    Error {
        message: String,
    },
    Pong,
}

const COLORS: [&str; 8] = [
    "#F24E1E", "#0ACF83", "#A259FF", "#1ABCFE", "#FF7262", "#FFC700", "#14AE5C", "#9747FF",
];

pub fn encode_document(doc: &Document) -> Result<String, String> {
    let bytes = deesainer_format::write(doc, None).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub fn decode_document(b64: &str) -> Result<Document, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    deesainer_format::read(&bytes).map_err(|e| e.to_string())
}

pub struct ServerState {
    pub doc: Document,
    pub version: u64,
    peers: BTreeMap<ClientId, Peer>,
    next_client: ClientId,
    access: Access,
    members: BTreeMap<ClientId, (Role, Option<String>)>,
    snapshots: Vec<Snapshot>,
    next_snapshot: u64,
}

impl ServerState {
    pub fn new(doc: Document, token: &str) -> Self {
        Self {
            doc,
            version: 0,
            peers: BTreeMap::new(),
            next_client: 1,
            access: Access::new(token),
            members: BTreeMap::new(),
            snapshots: Vec::new(),
            next_snapshot: 1,
        }
    }

    pub fn sibling(&self, doc: Document) -> Self {
        let mut room = Self::new(doc, "");
        room.access = self.access.clone();
        room
    }

    pub fn apply_server(&mut self, ops: Vec<Op>) -> Result<ServerMsg, String> {
        Transaction::new("merge", ops.clone())
            .apply(&mut self.doc)
            .map_err(|e| e.to_string())?;
        self.version += 1;
        Ok(ServerMsg::Ops {
            version: self.version,
            author: 0,
            seq: 0,
            ops,
        })
    }

    pub fn load_snapshots(&mut self, mut snapshots: Vec<Snapshot>) {
        snapshots.sort_by_key(|s| s.info.id);
        self.next_snapshot = snapshots.last().map_or(1, |s| s.info.id + 1);
        self.snapshots = snapshots;
    }

    pub fn snapshots(&self) -> &[Snapshot] {
        &self.snapshots
    }

    pub fn versions(&self) -> ServerMsg {
        ServerMsg::Versions {
            versions: self
                .snapshots
                .iter()
                .rev()
                .map(|s| s.info.clone())
                .collect(),
        }
    }

    pub(crate) fn require_editor(&self, client: ClientId) -> Result<(), ServerMsg> {
        match self.role_of(client) {
            Some(Role::Editor) | None => Ok(()),
            Some(role) => Err(ServerMsg::Error {
                message: format!("{}-only access: this needs an editor", role.name()),
            }),
        }
    }

    pub fn save_version(
        &mut self,
        client: ClientId,
        label: &str,
        at: u64,
    ) -> Result<VersionInfo, ServerMsg> {
        self.require_editor(client)?;
        let author = self
            .peers
            .get(&client)
            .map_or_else(String::new, |p| p.name.clone());
        let label: String = label.trim().chars().take(120).collect();
        let info = VersionInfo {
            id: self.next_snapshot,
            label: if label.is_empty() {
                format!("Version {}", self.next_snapshot)
            } else {
                label
            },
            author,
            version: self.version,
            at,
        };
        self.next_snapshot += 1;
        self.snapshots.push(Snapshot {
            info: info.clone(),
            doc: self.doc.clone(),
        });
        if self.snapshots.len() > MAX_VERSIONS {
            self.snapshots.remove(0);
        }
        Ok(info)
    }

    pub fn restore_version(
        &mut self,
        client: ClientId,
        id: u64,
        at: u64,
    ) -> Result<(ServerMsg, VersionInfo), ServerMsg> {
        self.require_editor(client)?;
        let target = self
            .snapshots
            .iter()
            .find(|s| s.info.id == id)
            .map(|s| s.doc.clone())
            .ok_or_else(|| ServerMsg::Error {
                message: format!("no version {id}"),
            })?;
        let backup = self.save_version(client, "Before restore", at)?;
        self.doc = target;
        self.version += 1;
        let document =
            encode_document(&self.doc).map_err(|message| ServerMsg::Error { message })?;
        Ok((
            ServerMsg::Restored {
                version: self.version,
                document,
            },
            backup,
        ))
    }

    pub fn with_viewer_token(mut self, token: &str) -> Self {
        self.access.set_secret(Role::Viewer, token);
        self
    }

    pub fn with_commenter_token(mut self, token: &str) -> Self {
        self.access.set_secret(Role::Commenter, token);
        self
    }

    pub fn with_users(mut self, users: Vec<User>) -> Self {
        self.access.set_users(users);
        self
    }

    pub fn role_of(&self, client: ClientId) -> Option<Role> {
        self.members.get(&client).map(|m| m.0)
    }

    pub fn user_of(&self, client: ClientId) -> Option<&str> {
        self.members.get(&client).and_then(|m| m.1.as_deref())
    }

    pub fn set_users(&mut self, users: Vec<User>) -> Vec<ClientId> {
        self.access.set_users(users);
        self.members
            .iter()
            .filter(|(_, (role, user))| {
                user.as_ref().is_some_and(|name| {
                    !self
                        .access
                        .users()
                        .iter()
                        .any(|u| &u.name == name && u.role == *role)
                })
            })
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn peer_name(&self, client: ClientId) -> String {
        self.peers
            .get(&client)
            .map_or_else(String::new, |p| p.name.clone())
    }

    pub fn peers(&self) -> impl Iterator<Item = &Peer> {
        self.peers.values()
    }

    pub fn join(
        &mut self,
        name: &str,
        token: &str,
        protocol: u32,
    ) -> Result<(ClientId, ServerMsg), ServerMsg> {
        if protocol != PROTOCOL {
            return Err(ServerMsg::Error {
                message: format!("protocol {protocol} is not supported (server speaks {PROTOCOL})"),
            });
        }
        let Some((role, user)) = self.access.authenticate(token) else {
            return Err(ServerMsg::Error {
                message: "wrong token".into(),
            });
        };
        let id = self.next_client;
        self.next_client = self.next_client.wrapping_add(1).max(1);
        let color = COLORS[(id as usize) % COLORS.len()].to_owned();
        let name: String = user
            .clone()
            .unwrap_or_else(|| name.to_owned())
            .chars()
            .take(64)
            .collect();
        let peer = Peer {
            id,
            name: if name.trim().is_empty() {
                format!("Guest {id}")
            } else {
                name
            },
            color: color.clone(),
            presence: Presence {
                page: None,
                cursor: None,
                selection: Vec::new(),
                chat: None,
            },
        };
        let peers = self.peers.values().cloned().collect();
        self.peers.insert(id, peer);
        self.members.insert(id, (role, user));
        let document =
            encode_document(&self.doc).map_err(|message| ServerMsg::Error { message })?;
        Ok((
            id,
            ServerMsg::Welcome {
                client: id,
                version: self.version,
                document,
                peers,
                color,
                read_only: role != Role::Editor,
                role,
                branch: String::new(),
            },
        ))
    }

    pub fn leave(&mut self, client: ClientId) -> ServerMsg {
        self.peers.remove(&client);
        self.members.remove(&client);
        ServerMsg::Left { client }
    }

    pub fn submit(
        &mut self,
        client: ClientId,
        seq: u64,
        ops: Vec<Op>,
    ) -> Result<ServerMsg, ServerMsg> {
        match self.role_of(client) {
            Some(Role::Viewer) => {
                return Err(ServerMsg::Reject {
                    seq,
                    reason: "view-only access: edits are not accepted".into(),
                });
            }
            Some(Role::Commenter) if ops.iter().any(|o| !matches!(o, Op::SetComment { .. })) => {
                return Err(ServerMsg::Reject {
                    seq,
                    reason: "comment-only access: only comments can change".into(),
                });
            }
            _ => {}
        }
        if ops.len() > 100_000 {
            return Err(ServerMsg::Reject {
                seq,
                reason: "batch too large".into(),
            });
        }
        match Transaction::new("remote", ops.clone()).apply(&mut self.doc) {
            Ok(_) => {
                self.version += 1;
                Ok(ServerMsg::Ops {
                    version: self.version,
                    author: client,
                    seq,
                    ops,
                })
            }
            Err(e) => Err(ServerMsg::Reject {
                seq,
                reason: e.to_string(),
            }),
        }
    }

    pub fn presence(&mut self, client: ClientId, presence: Presence) -> Option<ServerMsg> {
        let peer = self.peers.get_mut(&client)?;
        let mut presence = presence;
        presence.chat = presence
            .chat
            .map(|c| c.chars().take(MAX_CHAT).collect::<String>())
            .filter(|c| !c.trim().is_empty());
        peer.presence = presence;
        Some(ServerMsg::Presence {
            peer: Box::new(peer.clone()),
        })
    }
}

pub struct ClientState {
    pub client: ClientId,
    pub version: u64,
    pub confirmed: Document,
    pending: VecDeque<(u64, Vec<Op>)>,
    next_seq: u64,
    pub peers: BTreeMap<ClientId, Peer>,
    pub color: String,
    pub read_only: bool,
    pub role: Role,
    pub branch: String,
}

impl ClientState {
    pub fn from_welcome(msg: &ServerMsg) -> Result<Self, String> {
        let ServerMsg::Welcome {
            client,
            version,
            document,
            peers,
            color,
            read_only,
            role,
            branch,
        } = msg
        else {
            return Err("expected a welcome message".into());
        };
        Ok(Self {
            branch: branch.clone(),
            role: *role,
            read_only: *read_only,
            client: *client,
            version: *version,
            confirmed: decode_document(document)?,
            pending: VecDeque::new(),
            next_seq: 1,
            peers: peers.iter().map(|p| (p.id, p.clone())).collect(),
            color: color.clone(),
        })
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn local(&mut self, ops: Vec<Op>) -> ClientMsg {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.pending.push_back((seq, ops.clone()));
        ClientMsg::Submit { seq, ops }
    }

    pub fn visible(&mut self) -> Document {
        let mut doc = self.confirmed.clone();
        self.pending.retain(|(_, ops)| {
            Transaction::new("pending", ops.clone())
                .apply(&mut doc)
                .is_ok()
        });
        doc
    }

    pub fn receive(&mut self, msg: &ServerMsg) -> Option<Document> {
        match msg {
            ServerMsg::Ops {
                version,
                author,
                seq,
                ops,
            } => {
                if *version <= self.version {
                    return None;
                }
                self.version = *version;
                let own =
                    *author == self.client && self.pending.front().is_some_and(|(s, _)| s == seq);
                if own {
                    self.pending.pop_front();
                }
                let _ = Transaction::new("confirmed", ops.clone()).apply(&mut self.confirmed);
                if own {
                    None
                } else {
                    Some(self.visible())
                }
            }
            ServerMsg::Reject { seq, .. } => {
                self.pending.retain(|(s, _)| s != seq);
                Some(self.visible())
            }
            ServerMsg::Presence { peer } => {
                if peer.id != self.client {
                    self.peers.insert(peer.id, (**peer).clone());
                }
                None
            }
            ServerMsg::Left { client } => {
                self.peers.remove(client);
                None
            }
            ServerMsg::Restored { version, document } => {
                let doc = decode_document(document).ok()?;
                self.version = *version;
                self.confirmed = doc;
                self.pending.clear();
                Some(self.confirmed.clone())
            }
            ServerMsg::Welcome { .. }
            | ServerMsg::Versions { .. }
            | ServerMsg::Branches { .. }
            | ServerMsg::Merged { .. }
            | ServerMsg::MergePreview { .. }
            | ServerMsg::Error { .. }
            | ServerMsg::Pong => None,
        }
    }

    pub fn rejoin(&mut self, welcome: &ServerMsg) -> Result<(Document, Vec<ClientMsg>), String> {
        let fresh = ClientState::from_welcome(welcome)?;
        let pending: Vec<Vec<Op>> = self.pending.drain(..).map(|(_, ops)| ops).collect();
        self.client = fresh.client;
        self.version = fresh.version;
        self.confirmed = fresh.confirmed;
        self.peers = fresh.peers;
        self.color = fresh.color;
        let msgs = pending.into_iter().map(|ops| self.local(ops)).collect();
        Ok((self.visible(), msgs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::{Node, NodeKind, PropValue};
    use deesainer_geom::Rect;

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }

        fn below(&mut self, n: u64) -> u64 {
            if n == 0 {
                0
            } else {
                self.next() % n
            }
        }
    }

    fn random_ops(doc: &Document, rng: &mut Lcg, client: u32) -> Vec<Op> {
        let page = doc.first_page();
        let layers: Vec<NodeId> = doc
            .get(page)
            .map(|p| p.children.clone())
            .unwrap_or_default();
        match rng.below(4) {
            0 | 1 if layers.is_empty() || rng.below(3) == 0 => {
                let id =
                    NodeId(1_000_000 * u64::from(client) + doc.next_free_id().0 + rng.below(1000));
                let node = Node::new(
                    id,
                    "R",
                    NodeKind::Rectangle,
                    Rect::new(0.0, 0.0, 10.0, 10.0),
                );
                let index = layers.len();
                vec![Op::CreateNode {
                    parent: page,
                    index,
                    node: Box::new(node),
                }]
            }
            0 | 1 => {
                let id = layers[rng.below(layers.len() as u64) as usize];
                vec![Op::set(
                    id,
                    PropValue::Rect(Rect::new(rng.below(500) as f64, 0.0, 10.0, 10.0)),
                )]
            }
            2 if !layers.is_empty() => {
                let id = layers[rng.below(layers.len() as u64) as usize];
                vec![Op::DeleteNode { id }]
            }
            _ if layers.len() > 1 => {
                let id = layers[rng.below(layers.len() as u64) as usize];
                vec![Op::MoveNode {
                    id,
                    parent: page,
                    index: rng.below(layers.len() as u64 - 1) as usize,
                }]
            }
            _ => vec![Op::set(page, PropValue::Name(format!("P{}", rng.below(9))))],
        }
    }

    #[test]
    fn concurrent_clients_converge_to_the_server() {
        for seed in 0..40u64 {
            let mut rng = Lcg(seed + 1);
            let mut server = ServerState::new(Document::new(), "");
            let mut clients = Vec::new();
            let mut visible = Vec::new();
            for i in 0..3 {
                let (_, welcome) = server.join(&format!("c{i}"), "", PROTOCOL).unwrap();
                let c = ClientState::from_welcome(&welcome).unwrap();
                visible.push(c.confirmed.clone());
                clients.push(c);
            }
            let mut to_server: VecDeque<(usize, ClientMsg)> = VecDeque::new();
            let mut to_client: Vec<VecDeque<ServerMsg>> = vec![VecDeque::new(); 3];
            for _step in 0..200 {
                match rng.below(3) {
                    0 => {
                        let i = rng.below(3) as usize;
                        let ops = random_ops(&visible[i], &mut rng, i as u32 + 1);
                        let mut doc = visible[i].clone();
                        if Transaction::new("local", ops.clone())
                            .apply(&mut doc)
                            .is_ok()
                        {
                            visible[i] = doc;
                            to_server.push_back((i, clients[i].local(ops)));
                        }
                    }
                    1 => {
                        if let Some((i, ClientMsg::Submit { seq, ops })) = to_server.pop_front() {
                            let id = clients[i].client;
                            match server.submit(id, seq, ops) {
                                Ok(msg) => {
                                    to_client.iter_mut().for_each(|q| q.push_back(msg.clone()))
                                }
                                Err(msg) => to_client[i].push_back(msg),
                            }
                        }
                    }
                    _ => {
                        let i = rng.below(3) as usize;
                        if let Some(msg) = to_client[i].pop_front() {
                            if let Some(doc) = clients[i].receive(&msg) {
                                visible[i] = doc;
                            }
                        }
                    }
                }
            }
            while let Some((i, ClientMsg::Submit { seq, ops })) = to_server.pop_front() {
                let id = clients[i].client;
                match server.submit(id, seq, ops) {
                    Ok(msg) => to_client.iter_mut().for_each(|q| q.push_back(msg.clone())),
                    Err(msg) => to_client[i].push_back(msg),
                }
            }
            for i in 0..3 {
                while let Some(msg) = to_client[i].pop_front() {
                    if let Some(doc) = clients[i].receive(&msg) {
                        visible[i] = doc;
                    }
                }
                assert_eq!(clients[i].pending_len(), 0, "seed {seed} client {i}");
                assert_eq!(
                    clients[i].visible(),
                    server.doc,
                    "seed {seed} client {i} diverged"
                );
                server.doc.validate().unwrap();
            }
        }
    }

    #[test]
    fn offline_queue_resubmits_after_rejoin() {
        let mut server = ServerState::new(Document::new(), "secret");
        assert!(server.join("x", "wrong", PROTOCOL).is_err());
        let (_, welcome) = server.join("a", "secret", PROTOCOL).unwrap();
        let mut a = ClientState::from_welcome(&welcome).unwrap();
        let page = a.confirmed.first_page();
        let _ = a.local(vec![Op::set(page, PropValue::Name("Offline edit".into()))]);
        let (_, welcome) = server.join("a", "secret", PROTOCOL).unwrap();
        let (visible, msgs) = a.rejoin(&welcome).unwrap();
        assert_eq!(visible.get(page).unwrap().name, "Offline edit");
        for m in msgs {
            if let ClientMsg::Submit { seq, ops } = m {
                let echo = server.submit(a.client, seq, ops).unwrap();
                a.receive(&echo);
            }
        }
        assert_eq!(server.doc.get(page).unwrap().name, "Offline edit");
        assert_eq!(a.pending_len(), 0);
    }

    #[test]
    fn viewers_read_but_cannot_edit() {
        let mut server = ServerState::new(Document::new(), "edit").with_viewer_token("view");
        assert!(server.join("x", "other", PROTOCOL).is_err());
        let (v, welcome) = server.join("v", "view", PROTOCOL).unwrap();
        let viewer = ClientState::from_welcome(&welcome).unwrap();
        assert!(viewer.read_only);
        let page = viewer.confirmed.first_page();
        let ops = vec![Op::set(page, PropValue::Name("Nope".into()))];
        assert!(matches!(
            server.submit(v, 1, ops.clone()),
            Err(ServerMsg::Reject { .. })
        ));
        let (e, welcome) = server.join("e", "edit", PROTOCOL).unwrap();
        assert!(!ClientState::from_welcome(&welcome).unwrap().read_only);
        assert!(server.submit(e, 1, ops).is_ok());
        assert_eq!(server.doc.get(page).unwrap().name, "Nope");
        server.leave(v);
        assert!(server.role_of(v).is_none());
    }

    #[test]
    fn commenters_change_only_comments_and_users_sign_in_by_name() {
        use deesainer_doc::{Comment, CommentId};
        let users = vec![
            User {
                name: "Ada".into(),
                token: "ada-secret".into(),
                role: Role::Commenter,
            },
            User {
                name: "Bob".into(),
                token: "bob-secret".into(),
                role: Role::Editor,
            },
        ];
        let mut server = ServerState::new(Document::new(), "edit")
            .with_commenter_token("comment")
            .with_users(users);
        let (ada, welcome) = server
            .join("whatever the client says", "ada-secret", PROTOCOL)
            .unwrap();
        let state = ClientState::from_welcome(&welcome).unwrap();
        assert_eq!(state.role, Role::Commenter);
        assert!(state.read_only);
        assert_eq!(server.peer_name(ada), "Ada", "the account name wins");
        assert_eq!(server.user_of(ada), Some("Ada"));
        let page = state.confirmed.first_page();
        let note = Comment {
            author: "Ada".into(),
            text: "Looks off".into(),
            created: "2026-10-10T10:00:00Z".into(),
            page,
            position: Point::new(1.0, 1.0),
            node: None,
            resolved: false,
            replies: Vec::new(),
        };
        let comment = vec![Op::SetComment {
            id: CommentId(40),
            comment: Some(note),
        }];
        assert!(server.submit(ada, 1, comment.clone()).is_ok());
        assert!(server.doc.comment(CommentId(40)).is_some());
        let edit = Op::set(page, PropValue::Name("Nope".into()));
        assert!(matches!(
            server.submit(ada, 2, vec![edit.clone()]),
            Err(ServerMsg::Reject { .. })
        ));
        let mut mixed = comment;
        mixed.push(edit);
        assert!(matches!(
            server.submit(ada, 3, mixed),
            Err(ServerMsg::Reject { .. })
        ));
        assert!(server.save_version(ada, "x", 1).is_err());
        let (bob, _) = server.join("b", "bob-secret", PROTOCOL).unwrap();
        assert!(server.save_version(bob, "x", 1).is_ok());
        let (shared, _) = server.join("c", "comment", PROTOCOL).unwrap();
        assert_eq!(server.role_of(shared), Some(Role::Commenter));
        let kept = vec![User {
            name: "Bob".into(),
            token: "bob-secret".into(),
            role: Role::Editor,
        }];
        assert_eq!(server.set_users(kept), vec![ada]);
        server.leave(ada);
        let (ada2, _) = server.join("a", "bob-secret", PROTOCOL).unwrap();
        assert_eq!(
            server
                .set_users(vec![User {
                    name: "Bob".into(),
                    token: "bob-secret".into(),
                    role: Role::Viewer,
                }])
                .len(),
            2
        );
        let _ = ada2;
        assert!(server.join("x", "ada-secret", PROTOCOL).is_err());
    }

    #[test]
    fn versions_are_saved_and_restored() {
        let mut server = ServerState::new(Document::new(), "edit").with_viewer_token("view");
        let (e, welcome) = server.join("Ada", "edit", PROTOCOL).unwrap();
        let mut ada = ClientState::from_welcome(&welcome).unwrap();
        let (v, _) = server.join("Vic", "view", PROTOCOL).unwrap();
        let page = ada.confirmed.first_page();
        let rename = |name: &str| vec![Op::set(page, PropValue::Name(name.into()))];
        let echo = server.submit(e, 1, rename("First")).unwrap();
        ada.receive(&echo);
        let saved = server.save_version(e, "  Milestone  ", 100).unwrap();
        assert_eq!(saved.label, "Milestone");
        assert_eq!(saved.author, "Ada");
        let echo = server.submit(e, 2, rename("Second")).unwrap();
        ada.receive(&echo);
        assert!(server.save_version(v, "x", 1).is_err());
        assert!(server.restore_version(v, saved.id, 1).is_err());
        assert!(server.restore_version(e, 99, 1).is_err());
        let (msg, backup) = server.restore_version(e, saved.id, 200).unwrap();
        assert_eq!(backup.label, "Before restore");
        assert_eq!(server.doc.get(page).unwrap().name, "First");
        let ServerMsg::Versions { versions } = server.versions() else {
            panic!("not a list");
        };
        assert_eq!(versions.first().unwrap().id, backup.id);
        let _ = ada.local(rename("Pending"));
        let visible = ada.receive(&msg).unwrap();
        assert_eq!(visible.get(page).unwrap().name, "First");
        assert_eq!(ada.pending_len(), 0);
        assert_eq!(ada.version, server.version);
    }

    #[test]
    fn old_snapshots_are_dropped_past_the_cap() {
        let mut server = ServerState::new(Document::new(), "");
        let (c, _) = server.join("a", "", PROTOCOL).unwrap();
        for i in 0..(MAX_VERSIONS + 5) {
            server.save_version(c, &format!("v{i}"), i as u64).unwrap();
        }
        assert_eq!(server.snapshots().len(), MAX_VERSIONS);
        assert_eq!(server.snapshots().first().unwrap().info.label, "v5");
    }

    #[test]
    fn cursor_chat_is_relayed_capped_and_dropped_when_blank() {
        let mut server = ServerState::new(Document::new(), "");
        let (id, _) = server.join("Ada", "", PROTOCOL).unwrap();
        let mut said = |chat: &str| {
            let msg = server.presence(
                id,
                Presence {
                    page: None,
                    cursor: Some(Point::new(1.0, 1.0)),
                    selection: Vec::new(),
                    chat: Some(chat.to_owned()),
                },
            );
            match msg {
                Some(ServerMsg::Presence { peer }) => peer.presence.chat,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(said("Look here").as_deref(), Some("Look here"));
        assert_eq!(said(&"é".repeat(500)).unwrap().chars().count(), MAX_CHAT);
        assert_eq!(said("   "), None);
    }

    #[test]
    fn messages_round_trip_as_json() {
        let msg = ClientMsg::Presence {
            presence: Presence {
                page: Some(NodeId(2)),
                cursor: Some(Point::new(1.0, 2.0)),
                selection: vec![NodeId(3)],
                chat: Some("Over here".into()),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"presence\""));
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }
}
