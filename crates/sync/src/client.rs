use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use deesainer_doc::{Document, NodeId};
use deesainer_geom::Point;
use deesainer_ops::Op;

use crate::{
    BranchInfo, ClientMsg, ClientState, Peer, Presence, Role, ServerMsg, VersionInfo, PROTOCOL,
};

pub trait RoomHost {
    fn take_outbox(&mut self) -> Vec<Op>;
    fn replace_document(&mut self, doc: Document, keep_view: bool);
    fn set_record_outbox(&mut self, on: bool);
    fn current_page(&self) -> NodeId;
    fn selection(&self) -> Vec<NodeId>;
}

pub enum Incoming {
    Msg(Box<ServerMsg>),
    Disconnected(String),
}

pub fn hello(name: &str, token: &str, branch: &str, file: &str) -> ClientMsg {
    ClientMsg::Hello {
        protocol: PROTOCOL,
        name: name.to_owned(),
        token: token.to_owned(),
        branch: branch.to_owned(),
        file: file.to_owned(),
    }
}

pub struct RoomClient {
    pub addr: String,
    pub name: String,
    pub token: String,
    state: Option<ClientState>,
    out: Sender<ClientMsg>,
    inbox: Receiver<Incoming>,
    pub status: String,
    pub file: String,
    pub chat: Option<String>,
    pub versions: Vec<VersionInfo>,
    pub branch: String,
    pub branches: Vec<BranchInfo>,
    pub last_merge: Option<(String, usize, Vec<String>)>,
    pub merge_preview: Option<(String, Vec<String>, Vec<String>)>,
    pub merge_preview_doc: Option<Document>,
    last_presence: Option<Presence>,
}

impl RoomClient {
    pub fn new(
        addr: &str,
        name: &str,
        token: &str,
        branch: &str,
        out: Sender<ClientMsg>,
        inbox: Receiver<Incoming>,
    ) -> Self {
        Self {
            addr: addr.to_owned(),
            name: name.to_owned(),
            token: token.to_owned(),
            state: None,
            out,
            inbox,
            status: format!("connecting to {addr}…"),
            file: String::new(),
            chat: None,
            versions: Vec::new(),
            branch: branch.to_owned(),
            branches: Vec::new(),
            last_merge: None,
            merge_preview: None,
            merge_preview_doc: None,
            last_presence: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.state.is_some() && self.status.starts_with("connected")
    }

    pub fn peers(&self) -> Vec<Peer> {
        self.state
            .as_ref()
            .map(|s| s.peers.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn save_version(&self, label: &str) {
        let _ = self.out.send(ClientMsg::SaveVersion {
            label: label.to_owned(),
        });
    }

    pub fn restore_version(&self, id: u64) {
        let _ = self.out.send(ClientMsg::RestoreVersion { id });
    }

    pub fn create_branch(&self, name: &str) {
        let _ = self.out.send(ClientMsg::CreateBranch {
            name: name.to_owned(),
        });
    }

    pub fn merge_branch(&self, name: &str, prefer_branch: bool) {
        let _ = self.out.send(ClientMsg::MergeBranch {
            name: name.to_owned(),
            prefer_branch,
        });
    }

    pub fn preview_merge(&self, name: &str) {
        let _ = self.out.send(ClientMsg::PreviewMerge {
            name: name.to_owned(),
        });
    }

    pub fn delete_branch(&self, name: &str) {
        let _ = self.out.send(ClientMsg::DeleteBranch {
            name: name.to_owned(),
        });
    }

    pub fn role(&self) -> Option<Role> {
        self.state.as_ref().map(|s| s.role)
    }

    pub fn is_read_only(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.read_only)
    }

    pub fn pending(&self) -> usize {
        self.state.as_ref().map_or(0, ClientState::pending_len)
    }

    pub fn poll<H: RoomHost>(&mut self, session: &mut H, cursor: Option<Point>) -> bool {
        let mut changed = false;
        let ops = session.take_outbox();
        if !ops.is_empty() {
            if let Some(state) = &mut self.state {
                if state.read_only {
                    let (comments, rest): (Vec<_>, Vec<_>) = ops
                        .into_iter()
                        .partition(|o| matches!(o, Op::SetComment { .. }));
                    if state.role == Role::Commenter && !comments.is_empty() {
                        let _ = self.out.send(state.local(comments));
                    }
                    if !rest.is_empty() {
                        session.replace_document(state.visible(), true);
                        self.status = match state.role {
                            Role::Commenter => {
                                format!("comment only on {}: other edits are not sent", self.addr)
                            }
                            _ => format!("view only on {}: edits are not sent", self.addr),
                        };
                        changed = true;
                    }
                } else {
                    let _ = self.out.send(state.local(ops));
                }
            }
        }
        loop {
            let msg = match self.inbox.try_recv() {
                Ok(Incoming::Msg(m)) => *m,
                Ok(Incoming::Disconnected(reason)) => {
                    self.status = format!("offline ({reason}); retrying…");
                    continue;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            };
            match (&msg, &mut self.state) {
                (ServerMsg::Welcome { .. }, None) => match ClientState::from_welcome(&msg) {
                    Ok(state) => {
                        self.status = if state.role == Role::Commenter {
                            format!("connected to {} (comment only)", self.addr)
                        } else if state.read_only {
                            format!("connected to {} (view only)", self.addr)
                        } else {
                            format!("connected to {}", self.addr)
                        };
                        session.replace_document(state.confirmed.clone(), false);
                        session.set_record_outbox(true);
                        self.state = Some(state);
                        let _ = self.out.send(ClientMsg::ListVersions);
                        changed = true;
                    }
                    Err(e) => self.status = format!("bad welcome: {e}"),
                },
                (ServerMsg::Welcome { .. }, Some(state)) => match state.rejoin(&msg) {
                    Ok((doc, resend)) => {
                        session.replace_document(doc, true);
                        for m in resend {
                            let _ = self.out.send(m);
                        }
                        self.status = format!("connected to {} (resynced)", self.addr);
                        changed = true;
                    }
                    Err(e) => self.status = format!("bad welcome: {e}"),
                },
                (ServerMsg::Versions { versions }, _) => self.versions = versions.clone(),
                (ServerMsg::Branches { branches }, _) => self.branches = branches.clone(),
                (
                    ServerMsg::MergePreview {
                        branch,
                        changes,
                        conflicts,
                        merged,
                    },
                    _,
                ) => {
                    self.merge_preview_doc = merged
                        .as_deref()
                        .and_then(|m| crate::decode_document(m).ok());
                    self.merge_preview = Some((branch.clone(), changes.clone(), conflicts.clone()))
                }
                (
                    ServerMsg::Merged {
                        branch,
                        applied,
                        conflicts,
                    },
                    _,
                ) => self.last_merge = Some((branch.clone(), *applied, conflicts.clone())),
                (ServerMsg::Error { message }, _) => {
                    self.status = format!("relay error: {message}")
                }
                (_, Some(state)) => {
                    if let Some(doc) = state.receive(&msg) {
                        session.replace_document(doc, true);
                        changed = true;
                    }
                }
                (_, None) => {}
            }
        }
        if self.state.is_some() {
            let presence = Presence {
                page: Some(session.current_page()),
                cursor,
                selection: session.selection(),
                chat: self.chat.clone(),
            };
            if self.last_presence.as_ref() != Some(&presence) {
                let _ = self.out.send(ClientMsg::Presence {
                    presence: presence.clone(),
                });
                self.last_presence = Some(presence);
            }
        }
        changed
    }
}
