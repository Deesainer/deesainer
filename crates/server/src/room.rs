use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deesainer_io::files;
use deesainer_sync::{
    BranchInfo, ClientId, ClientMsg, Hub, Role, ServerMsg, ServerState, Snapshot, StoredBranch,
    VersionInfo,
};

const MAX_LINE: usize = 64 * 1024 * 1024;

pub type Clients = Arc<Mutex<BTreeMap<(String, ClientId), Conn>>>;

pub struct Conn {
    pub tx: Sender<String>,
    pub stream: Option<TcpStream>,
}

pub type SaveState = (u64, BTreeMap<String, u64>);

pub struct RoomService {
    pub state: Arc<Mutex<Hub>>,
    pub clients: Clients,
    pub file: PathBuf,
    versions: VersionDir,
    branch_dir: VersionDir,
    pub audit: PathBuf,
}

pub fn audit(
    path: &std::path::Path,
    event: &str,
    room: &str,
    who: (&str, &str),
    detail: serde_json::Value,
) {
    use std::io::Write as _;
    let line = serde_json::json!({
        "at": unix_now(),
        "event": event,
        "room": room,
        "user": who.0,
        "role": who.1,
        "detail": detail,
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

pub fn who(hub: &Hub, room: &str, id: ClientId) -> (String, &'static str) {
    hub.room(room).map_or_else(
        || (String::new(), ""),
        |r| (r.peer_name(id), r.role_of(id).map_or("", Role::name)),
    )
}

type VersionDir = Arc<PathBuf>;

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn version_dir_for(file: &std::path::Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".versions");
    PathBuf::from(name)
}

fn load_versions(dir: &std::path::Path) -> Vec<Snapshot> {
    let Ok(index) = std::fs::read(dir.join("index.json")) else {
        return Vec::new();
    };
    let infos: Vec<VersionInfo> = serde_json::from_slice(&index).unwrap_or_default();
    infos
        .into_iter()
        .filter_map(|info| {
            let doc = files::load_document(&dir.join(format!("{}.deesainer", info.id))).ok()?;
            Some(Snapshot { info, doc })
        })
        .collect()
}

pub fn persist_versions(dir: &std::path::Path, snapshots: &[Snapshot]) {
    if std::fs::create_dir_all(dir).is_err() {
        eprintln!("deesainer: cannot create {}", dir.display());
        return;
    }
    for s in snapshots {
        let path = dir.join(format!("{}.deesainer", s.info.id));
        if !path.exists() {
            if let Err(e) = files::save_document(&s.doc, &path) {
                eprintln!("deesainer: cannot store version {}: {e}", s.info.id);
            }
        }
    }
    let infos: Vec<&VersionInfo> = snapshots.iter().map(|s| &s.info).collect();
    if let Ok(json) = serde_json::to_vec_pretty(&infos) {
        let _ = std::fs::write(dir.join("index.json"), json);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let keep = name == "index.json"
                || name
                    .strip_suffix(".deesainer")
                    .and_then(|id| id.parse::<u64>().ok())
                    .is_some_and(|id| snapshots.iter().any(|s| s.info.id == id));
            if !keep && name.ends_with(".deesainer") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

pub fn send_all(clients: &Clients, room: &str, msg: &ServerMsg, except: Option<ClientId>) {
    let Ok(line) = serde_json::to_string(msg) else {
        return;
    };
    if let Ok(map) = clients.lock() {
        for ((r, id), conn) in map.iter() {
            if r == room && Some(*id) != except {
                let _ = conn.tx.send(line.clone());
            }
        }
    }
}

pub fn send_everywhere(clients: &Clients, msg: &ServerMsg) {
    let Ok(line) = serde_json::to_string(msg) else {
        return;
    };
    if let Ok(map) = clients.lock() {
        for conn in map.values() {
            let _ = conn.tx.send(line.clone());
        }
    }
}

fn branch_dir_for(file: &std::path::Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".branches");
    PathBuf::from(name)
}

fn branch_versions_dir(dir: &std::path::Path, slot: u64) -> PathBuf {
    dir.join(format!("{slot}.versions"))
}

fn load_branches(dir: &std::path::Path) -> Vec<StoredBranch> {
    let Ok(index) = std::fs::read(dir.join("index.json")) else {
        return Vec::new();
    };
    let infos: Vec<BranchInfo> = serde_json::from_slice(&index).unwrap_or_default();
    infos
        .into_iter()
        .filter_map(|info| {
            let base =
                files::load_document(&dir.join(format!("{}.base.deesainer", info.slot))).ok()?;
            let doc = files::load_document(&dir.join(format!("{}.deesainer", info.slot))).ok()?;
            let snapshots = load_versions(&branch_versions_dir(dir, info.slot));
            Some(StoredBranch {
                info,
                base,
                doc,
                snapshots,
            })
        })
        .collect()
}

pub fn persist_branches(dir: &std::path::Path, hub: &Hub) {
    let rooms: Vec<_> = hub.branch_rooms().collect();
    if rooms.is_empty() && !dir.exists() {
        return;
    }
    if std::fs::create_dir_all(dir).is_err() {
        eprintln!("deesainer: cannot create {}", dir.display());
        return;
    }
    for b in &rooms {
        for (suffix, doc) in [("", &b.state.doc), (".base", &b.base)] {
            let path = dir.join(format!("{}{suffix}.deesainer", b.info.slot));
            if let Err(e) = files::save_document(doc, &path) {
                eprintln!("deesainer: cannot store branch {}: {e}", b.info.name);
            }
        }
    }
    let infos: Vec<&BranchInfo> = rooms.iter().map(|b| &b.info).collect();
    if let Ok(json) = serde_json::to_vec_pretty(&infos) {
        let _ = std::fs::write(dir.join("index.json"), json);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let slot = name
                .strip_suffix(".base.deesainer")
                .or_else(|| name.strip_suffix(".deesainer"))
                .and_then(|s| s.parse::<u64>().ok());
            if let Some(slot) = slot {
                if !rooms.iter().any(|b| b.info.slot == slot) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            let stale_versions = name
                .strip_suffix(".versions")
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|slot| !rooms.iter().any(|b| b.info.slot == slot));
            if stale_versions {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

impl RoomService {
    pub fn open(
        file: &Path,
        audit: PathBuf,
        configure: impl FnOnce(deesainer_engine::doc::Document) -> ServerState,
    ) -> Result<Self, String> {
        let doc = if file.exists() {
            files::load_document(file)?
        } else {
            deesainer_engine::doc::Document::new()
        };
        let mut server = configure(doc);
        let versions: VersionDir = Arc::new(version_dir_for(file));
        server.load_snapshots(load_versions(&versions));
        let mut hub = Hub::new(server);
        let branch_dir: VersionDir = Arc::new(branch_dir_for(file));
        hub.load_branches(load_branches(&branch_dir));
        Ok(Self {
            state: Arc::new(Mutex::new(hub)),
            clients: Arc::new(Mutex::new(BTreeMap::new())),
            file: file.to_owned(),
            versions,
            branch_dir,
            audit,
        })
    }

    pub fn save_if_changed(&self, saved: &mut SaveState) {
        let snapshot = self
            .state
            .lock()
            .ok()
            .map(|hub| (hub.main.version, hub.main.doc.clone()));
        if let Some((version, doc)) = snapshot {
            if version != saved.0 {
                match files::save_document(&doc, &self.file) {
                    Ok(()) => saved.0 = version,
                    Err(e) => {
                        eprintln!("deesainer: autosave of {} failed: {e}", self.file.display())
                    }
                }
            }
        }
        if let Ok(hub) = self.state.lock() {
            let now: BTreeMap<String, u64> = hub
                .branch_rooms()
                .map(|b| (b.info.name.clone(), b.state.version))
                .collect();
            if now != saved.1 {
                persist_branches(&self.branch_dir, &hub);
                saved.1 = now;
            }
        }
    }

    pub fn client_count(&self) -> usize {
        self.clients.lock().map_or(0, |m| m.len())
    }

    pub fn kick(&self, room: &str, id: ClientId, message: &str) {
        if let Ok(hub) = self.state.lock() {
            let (user, role) = who(&hub, room, id);
            audit(
                &self.audit,
                "revoked",
                room,
                (&user, role),
                serde_json::json!({}),
            );
        }
        let conn = self
            .clients
            .lock()
            .ok()
            .and_then(|mut m| m.remove(&(room.to_owned(), id)));
        if let Some(conn) = conn {
            if let Ok(line) = serde_json::to_string(&ServerMsg::Error {
                message: message.to_owned(),
            }) {
                let _ = conn.tx.send(line);
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(stream) = conn.stream {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    pub fn handle(&self, mut reader: BufReader<TcpStream>, first: Option<String>) {
        let state = &self.state;
        let clients = &self.clients;
        let versions = &self.versions;
        let branch_dir = &self.branch_dir;
        let audit_log = &self.audit;
        let stream = reader.get_ref();
        let Ok(write_half) = stream.try_clone() else {
            return;
        };
        let kill = stream.try_clone().ok();
        let (tx, rx) = channel::<String>();
        std::thread::spawn(move || {
            let mut w = write_half;
            for line in rx {
                if writeln!(w, "{line}").and_then(|()| w.flush()).is_err() {
                    break;
                }
            }
        });
        let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
        let mut first = first;
        let mut client: Option<ClientId> = None;
        let mut room = String::new();
        let reply = |msg: &ServerMsg| {
            if let Ok(line) = serde_json::to_string(msg) {
                let _ = tx.send(line);
            }
        };
        let mut line = String::new();
        loop {
            line.clear();
            let read = match first.take() {
                Some(l) => {
                    line = l;
                    Ok(line.len().max(1))
                }
                None => reader
                    .by_ref()
                    .take(MAX_LINE as u64 + 1)
                    .read_line(&mut line),
            };
            match read {
                Ok(0) => break,
                Ok(n) if n > MAX_LINE => {
                    reply(&ServerMsg::Error {
                        message: "message too large".into(),
                    });
                    break;
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if client.is_none() {
                        break;
                    }
                    continue;
                }
                Err(_) => break,
            }
            let msg: ClientMsg = match serde_json::from_str(line.trim()) {
                Ok(m) => m,
                Err(e) => {
                    reply(&ServerMsg::Error {
                        message: format!("bad message: {e}"),
                    });
                    continue;
                }
            };
            let Ok(mut st) = state.lock() else { break };
            match (msg, client) {
                (
                    ClientMsg::Hello {
                        protocol,
                        name,
                        token,
                        branch,
                        ..
                    },
                    None,
                ) => match st.join(&branch, &name, &token, protocol) {
                    Ok((id, welcome)) => {
                        client = Some(id);
                        room = branch;
                        if let Ok(mut map) = clients.lock() {
                            map.insert(
                                (room.clone(), id),
                                Conn {
                                    tx: tx.clone(),
                                    stream: kill.as_ref().and_then(|s| s.try_clone().ok()),
                                },
                            );
                        }
                        let (user, role) = who(&st, &room, id);
                        audit(
                            audit_log,
                            "join",
                            &room,
                            (&user, role),
                            serde_json::json!({}),
                        );
                        reply(&welcome);
                        reply(&st.branches());
                        let arrived = st
                            .room(&room)
                            .and_then(|r| r.peers().find(|p| p.id == id).cloned());
                        drop(st);
                        if let Some(peer) = arrived {
                            send_all(
                                clients,
                                &room,
                                &ServerMsg::Presence {
                                    peer: Box::new(peer),
                                },
                                Some(id),
                            );
                        }
                        eprintln!(
                            "deesainer: {name} joined {:?} of {} as {id}",
                            room,
                            self.file.display()
                        );
                    }
                    Err(err) => {
                        audit(
                            audit_log,
                            "denied",
                            &branch,
                            (&name, ""),
                            serde_json::json!({}),
                        );
                        reply(&err);
                        break;
                    }
                },
                (ClientMsg::Submit { seq, ops }, Some(id)) => {
                    let count = ops.len();
                    match st.submit(&room, id, seq, ops) {
                        Ok(msg) => {
                            let (user, role) = who(&st, &room, id);
                            let version = match &msg {
                                ServerMsg::Ops { version, .. } => *version,
                                _ => 0,
                            };
                            audit(
                                audit_log,
                                "edit",
                                &room,
                                (&user, role),
                                serde_json::json!({ "ops": count, "version": version }),
                            );
                            drop(st);
                            send_all(clients, &room, &msg, None);
                        }
                        Err(msg) => reply(&msg),
                    }
                }
                (ClientMsg::Presence { presence }, Some(id)) => {
                    if let Some(msg) = st.presence(&room, id, presence) {
                        drop(st);
                        send_all(clients, &room, &msg, Some(id));
                    }
                }
                (ClientMsg::ListVersions, Some(_)) => {
                    if let Some(r) = st.room(&room) {
                        reply(&r.versions());
                    }
                }
                (ClientMsg::ListBranches, Some(_)) => reply(&st.branches()),
                (ClientMsg::SaveVersion { label }, Some(id)) => {
                    let saved = st
                        .room_mut(&room)
                        .map(|r| r.save_version(id, &label, unix_now()));
                    match saved {
                        Some(Ok(_)) => {
                            let Some(r) = st.room(&room) else { continue };
                            let target = if room.is_empty() {
                                Some(PathBuf::clone(versions))
                            } else {
                                st.slot_of(&room)
                                    .map(|slot| branch_versions_dir(branch_dir, slot))
                            };
                            if let Some(dir) = target {
                                persist_versions(&dir, r.snapshots());
                            }
                            let list = r.versions();
                            let (user, role) = who(&st, &room, id);
                            audit(
                                audit_log,
                                "saveVersion",
                                &room,
                                (&user, role),
                                serde_json::json!({ "label": label }),
                            );
                            drop(st);
                            send_all(clients, &room, &list, None);
                        }
                        Some(Err(msg)) => reply(&msg),
                        None => {}
                    }
                }
                (ClientMsg::RestoreVersion { id: version }, Some(id)) => {
                    let restored = st
                        .room_mut(&room)
                        .map(|r| r.restore_version(id, version, unix_now()));
                    match restored {
                        Some(Ok((restored, _))) => {
                            let Some(r) = st.room(&room) else { continue };
                            let target = if room.is_empty() {
                                Some(PathBuf::clone(versions))
                            } else {
                                st.slot_of(&room)
                                    .map(|slot| branch_versions_dir(branch_dir, slot))
                            };
                            if let Some(dir) = target {
                                persist_versions(&dir, r.snapshots());
                            }
                            let list = r.versions();
                            let (user, role) = who(&st, &room, id);
                            audit(
                                audit_log,
                                "restoreVersion",
                                &room,
                                (&user, role),
                                serde_json::json!({ "version": version }),
                            );
                            drop(st);
                            send_all(clients, &room, &restored, None);
                            send_all(clients, &room, &list, None);
                        }
                        Some(Err(msg)) => reply(&msg),
                        None => {}
                    }
                }
                (ClientMsg::CreateBranch { name }, Some(id)) => {
                    match st.create_branch(&room, id, &name, unix_now()) {
                        Ok(_) => {
                            persist_branches(branch_dir, &st);
                            let list = st.branches();
                            let (user, role) = who(&st, &room, id);
                            audit(
                                audit_log,
                                "createBranch",
                                &room,
                                (&user, role),
                                serde_json::json!({ "name": name }),
                            );
                            drop(st);
                            send_everywhere(clients, &list);
                        }
                        Err(msg) => reply(&msg),
                    }
                }
                (ClientMsg::PreviewMerge { name }, Some(_)) => match st.preview_merge(&name) {
                    Ok(preview) => reply(&preview),
                    Err(msg) => reply(&msg),
                },
                (
                    ClientMsg::MergeBranch {
                        name,
                        prefer_branch,
                    },
                    Some(id),
                ) => match st.merge_branch(&room, id, &name, prefer_branch) {
                    Ok(out) => {
                        persist_branches(branch_dir, &st);
                        let (user, role) = who(&st, &room, id);
                        audit(
                            audit_log,
                            "mergeBranch",
                            &room,
                            (&user, role),
                            serde_json::json!({
                                "name": name,
                                "applied": out.applied,
                                "conflicts": out.conflicts.len(),
                                "preferBranch": prefer_branch,
                            }),
                        );
                        drop(st);
                        if let Some(ops) = out.ops {
                            send_all(clients, "", &ops, None);
                        }
                        send_everywhere(
                            clients,
                            &ServerMsg::Merged {
                                branch: name,
                                applied: out.applied,
                                conflicts: out.conflicts,
                            },
                        );
                    }
                    Err(msg) => reply(&msg),
                },
                (ClientMsg::DeleteBranch { name }, Some(id)) => {
                    match st.delete_branch(&room, id, &name) {
                        Ok(()) => {
                            persist_branches(branch_dir, &st);
                            let list = st.branches();
                            let (user, role) = who(&st, &room, id);
                            audit(
                                audit_log,
                                "deleteBranch",
                                &room,
                                (&user, role),
                                serde_json::json!({ "name": name }),
                            );
                            drop(st);
                            send_all(
                                clients,
                                &name,
                                &ServerMsg::Error {
                                    message: format!("branch {name:?} was deleted"),
                                },
                                None,
                            );
                            send_everywhere(clients, &list);
                        }
                        Err(msg) => reply(&msg),
                    }
                }
                (ClientMsg::Ping, _) => reply(&ServerMsg::Pong),
                (_, None) => {
                    reply(&ServerMsg::Error {
                        message: "say hello first".into(),
                    });
                    break;
                }
                (ClientMsg::Hello { .. }, Some(_)) => reply(&ServerMsg::Error {
                    message: "already joined".into(),
                }),
            }
        }
        if let Some(id) = client {
            if let Ok(st) = state.lock() {
                let (user, role) = who(&st, &room, id);
                audit(
                    audit_log,
                    "leave",
                    &room,
                    (&user, role),
                    serde_json::json!({}),
                );
            }
            if let Ok(mut map) = clients.lock() {
                map.remove(&(room.clone(), id));
            }
            let left = state.lock().map(|mut st| st.leave(&room, id));
            if let Ok(Some(msg)) = left {
                send_all(clients, &room, &msg, None);
            }
            eprintln!("deesainer: client {id} left {}", self.file.display());
        }
    }
}
