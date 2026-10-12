use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use deesainer_sync::{ClientMsg, Role, ServerMsg, ServerState, User};
use sha2::{Digest, Sha256};

use crate::accounts::{Accounts, UserId};
use crate::files::{FileId, Files};
use crate::room::{RoomService, SaveState};

pub struct Team {
    data: PathBuf,
    secret: String,
    pub accounts: Mutex<Accounts>,
    pub files: Mutex<Files>,
    rooms: Mutex<BTreeMap<FileId, (Arc<RoomService>, SaveState)>>,
}

fn random_secret() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

impl Team {
    pub fn open(data: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
        let key = data.join("secret.key");
        let secret = match std::fs::read_to_string(&key) {
            Ok(s) if s.trim().len() >= 32 => s.trim().to_owned(),
            _ => {
                let s = random_secret()?;
                std::fs::write(&key, &s).map_err(|e| e.to_string())?;
                s
            }
        };
        Ok(Self {
            data: data.to_owned(),
            secret,
            accounts: Mutex::new(
                Accounts::open(&data.join("accounts.json")).map_err(|e| e.to_string())?,
            ),
            files: Mutex::new(Files::open(data)?),
            rooms: Mutex::new(BTreeMap::new()),
        })
    }

    fn room_token(&self, file: FileId, user: UserId) -> String {
        let h = Sha256::digest(format!("{}|{file}|{user}", self.secret).as_bytes());
        h.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn room_users(&self, file: FileId) -> Vec<User> {
        let (Ok(accounts), Ok(files)) = (self.accounts.lock(), self.files.lock()) else {
            return Vec::new();
        };
        let mut taken: BTreeMap<String, usize> = BTreeMap::new();
        accounts
            .users()
            .iter()
            .filter_map(|u| {
                let role = files.role(u.id, u.admin, file)?;
                let n = taken.entry(u.name.clone()).or_default();
                *n += 1;
                let name = if *n == 1 {
                    u.name.clone()
                } else {
                    format!("{} {n}", u.name)
                };
                Some(User {
                    name,
                    token: self.room_token(file, u.id),
                    role,
                })
            })
            .collect()
    }

    pub fn room(&self, file: FileId) -> Result<Arc<RoomService>, String> {
        if let Some((r, _)) = self.rooms.lock().map_err(|_| "busy")?.get(&file) {
            return Ok(Arc::clone(r));
        }
        let path = {
            let files = self.files.lock().map_err(|_| "busy")?;
            if files.get(file).is_none() {
                return Err("no such file".into());
            }
            files.document_path(file)
        };
        let users = self.room_users(file);
        let editor = random_secret()?;
        let service = Arc::new(RoomService::open(
            &path,
            self.data.join("audit.log"),
            |doc| ServerState::new(doc, &editor).with_users(users),
        )?);
        let mut rooms = self.rooms.lock().map_err(|_| "busy")?;
        let entry = rooms
            .entry(file)
            .or_insert_with(|| (Arc::clone(&service), (u64::MAX, BTreeMap::new())));
        Ok(Arc::clone(&entry.0))
    }

    pub fn refresh_access(&self, file: FileId) {
        let room = self
            .rooms
            .lock()
            .ok()
            .and_then(|r| r.get(&file).map(|(s, _)| Arc::clone(s)));
        let Some(room) = room else { return };
        let users = self.room_users(file);
        let kicked = room.state.lock().map(|mut hub| hub.set_users(users));
        for (r, id) in kicked.unwrap_or_default() {
            room.kick(&r, id, "your access to this file was changed or removed");
        }
    }

    pub fn close_room(&self, file: FileId) {
        let room = self.rooms.lock().ok().and_then(|mut r| r.remove(&file));
        let Some((room, _)) = room else { return };
        let ids: Vec<(String, deesainer_sync::ClientId)> = room
            .clients
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        for (r, id) in ids {
            room.kick(&r, id, "this file was deleted");
        }
    }

    pub fn tick(&self) {
        let Ok(mut rooms) = self.rooms.lock() else {
            return;
        };
        let mut idle = Vec::new();
        for (id, (room, saved)) in rooms.iter_mut() {
            room.save_if_changed(saved);
            if room.client_count() == 0 {
                idle.push(*id);
            }
        }
        for id in idle {
            rooms.remove(&id);
        }
    }

    pub fn serve_protocol(&self, reader: BufReader<TcpStream>, hello_line: String) {
        let refuse = |reader: BufReader<TcpStream>, message: &str| {
            use std::io::Write as _;
            let mut s = reader.into_inner();
            if let Ok(line) = serde_json::to_string(&ServerMsg::Error {
                message: message.to_owned(),
            }) {
                let _ = writeln!(s, "{line}");
            }
        };
        let Ok(ClientMsg::Hello {
            protocol,
            name,
            token,
            branch,
            file,
        }) = serde_json::from_str::<ClientMsg>(hello_line.trim())
        else {
            return refuse(reader, "say hello first");
        };
        let Ok(file_id) = file.parse::<FileId>() else {
            return refuse(reader, "name a file to open");
        };
        let user = self
            .accounts
            .lock()
            .ok()
            .and_then(|a| a.session(&token).map(|u| (u.id, u.admin)));
        let Some((user, admin)) = user else {
            return refuse(reader, "sign in first (the session has expired)");
        };
        let role = self
            .files
            .lock()
            .ok()
            .and_then(|f| f.role(user, admin, file_id));
        if role.is_none() {
            return refuse(reader, "you do not have access to this file");
        }
        let room = match self.room(file_id) {
            Ok(r) => r,
            Err(e) => return refuse(reader, &e),
        };
        let hello = ClientMsg::Hello {
            protocol,
            name,
            token: self.room_token(file_id, user),
            branch,
            file,
        };
        let Ok(line) = serde_json::to_string(&hello) else {
            return;
        };
        room.handle(reader, Some(line));
    }

    pub fn role_for(&self, user: UserId, admin: bool, file: FileId) -> Option<Role> {
        self.files.lock().ok()?.role(user, admin, file)
    }
}
