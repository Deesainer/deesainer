use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

use base64::Engine as _;
use deesainer_sync::Role;
use serde_json::{json, Value};

use crate::accounts::{AccountError, User, INVITE_SECS};
use crate::team::Team;

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024;
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn bearer(&self) -> &str {
        self.header("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("")
            .trim()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

pub fn looks_like_http(head: &[u8]) -> bool {
    ["GET ", "POST", "PUT ", "DELE", "OPTI", "PATC", "HEAD"]
        .iter()
        .any(|m| head.starts_with(m.as_bytes()))
}

pub fn read_request(reader: &mut BufReader<TcpStream>) -> Result<Request, String> {
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take((MAX_HEAD - head.len().min(MAX_HEAD)) as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("connection closed".into());
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEAD {
            return Err("request head too large".into());
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(&head) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err("bad request".into()),
    }
    let method = req.method.unwrap_or("").to_owned();
    let path = req.path.unwrap_or("/").to_owned();
    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_owned(),
                String::from_utf8_lossy(h.value).into_owned(),
            )
        })
        .collect();
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err("body too large".into());
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

pub fn respond(stream: &mut TcpStream, status: u16, body: &Value, cors: Option<&str>) {
    let text = body.to_string();
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        text.len()
    );
    if let Some(origin) = cors {
        head.push_str(&format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nVary: Origin\r\n"
        ));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.flush();
}

fn user_json(u: &User) -> Value {
    json!({ "id": u.id, "email": u.email, "name": u.name, "admin": u.admin })
}

fn err(status: u16, message: impl std::fmt::Display) -> (u16, Value) {
    (status, json!({ "error": message.to_string() }))
}

fn account_err(e: AccountError) -> (u16, Value) {
    let status = match e {
        AccountError::BadLogin => 401,
        AccountError::Locked => 429,
        AccountError::Taken => 409,
        AccountError::Io(_) => 500,
        _ => 400,
    };
    err(status, e)
}

fn role_name(r: Role) -> &'static str {
    r.name()
}

fn parse_role(v: &Value) -> Result<Option<Role>, String> {
    match v.as_str() {
        None if v.is_null() => Ok(None),
        Some("editor") => Ok(Some(Role::Editor)),
        Some("commenter") => Ok(Some(Role::Commenter)),
        Some("viewer") => Ok(Some(Role::Viewer)),
        _ => Err("role must be editor, commenter, viewer or null".into()),
    }
}

pub fn handle(team: &Team, req: &Request) -> (u16, Value) {
    let body = req.json();
    let path = req.path.split('?').next().unwrap_or("");
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (req.method.as_str(), parts.as_slice()) {
        ("POST", ["api", "login"]) => {
            let Ok(mut a) = team.accounts.lock() else {
                return err(500, "busy");
            };
            let email = body["email"].as_str().unwrap_or("");
            let password = body["password"].as_str().unwrap_or("");
            return match a.login(email, password) {
                Ok(token) => {
                    let user = a.session(&token).map(user_json).unwrap_or(Value::Null);
                    (200, json!({ "token": token, "user": user }))
                }
                Err(e) => account_err(e),
            };
        }
        ("POST", ["api", "accept"]) => {
            let Ok(mut a) = team.accounts.lock() else {
                return err(500, "busy");
            };
            let s = |k: &str| body[k].as_str().unwrap_or("");
            return match a.accept(s("code"), s("email"), s("name"), s("password")) {
                Ok(_) => match a.login(s("email"), s("password")) {
                    Ok(token) => {
                        let user = a.session(&token).map(user_json).unwrap_or(Value::Null);
                        (200, json!({ "token": token, "user": user }))
                    }
                    Err(e) => account_err(e),
                },
                Err(e) => account_err(e),
            };
        }
        _ => {}
    }
    let token = req.bearer().to_owned();
    let me = team
        .accounts
        .lock()
        .ok()
        .and_then(|a| a.session(&token).cloned());
    let Some(me) = me else {
        return err(401, "sign in first");
    };
    match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["api", "me"]) => (200, user_json(&me)),
        ("POST", ["api", "logout"]) => match team.accounts.lock() {
            Ok(mut a) => match a.logout(&token) {
                Ok(()) => (200, json!({})),
                Err(e) => account_err(e),
            },
            Err(_) => err(500, "busy"),
        },
        ("POST", ["api", "invites"]) => {
            let email = body["email"].as_str().filter(|e| !e.is_empty());
            let admin = body["admin"].as_bool().unwrap_or(false);
            let secs = body["days"]
                .as_u64()
                .map_or(INVITE_SECS, |d| d.saturating_mul(86_400));
            match team.accounts.lock() {
                Ok(mut a) => match a.invite(me.id, email, admin, secs) {
                    Ok(code) => (200, json!({ "code": code })),
                    Err(e) => account_err(e),
                },
                Err(_) => err(500, "busy"),
            }
        }
        ("GET", ["api", "users"]) if me.admin => {
            let list: Vec<Value> = team
                .accounts
                .lock()
                .map(|a| a.users().iter().map(user_json).collect())
                .unwrap_or_default();
            (200, Value::Array(list))
        }
        ("GET", ["api", "files"]) => {
            let Ok(files) = team.files.lock() else {
                return err(500, "busy");
            };
            let list: Vec<Value> = files
                .visible_to(me.id, me.admin)
                .into_iter()
                .map(|(f, role)| {
                    json!({
                        "id": f.id,
                        "name": f.name,
                        "owner": f.owner,
                        "created": f.created,
                        "role": role_name(role),
                    })
                })
                .collect();
            (200, Value::Array(list))
        }
        ("POST", ["api", "files"]) => {
            let name = body["name"].as_str().unwrap_or("Untitled");
            let data = match body["data"].as_str() {
                Some(b) => match B64.decode(b) {
                    Ok(bytes) => Some(bytes),
                    Err(_) => return err(400, "data must be base64"),
                },
                None => None,
            };
            match team.files.lock() {
                Ok(mut f) => match f.create(me.id, name, data.as_deref()) {
                    Ok(id) => (200, json!({ "id": id })),
                    Err(e) => err(400, e),
                },
                Err(_) => err(500, "busy"),
            }
        }
        ("GET", ["api", "files", id, "document"]) => {
            let Ok(id) = id.parse() else {
                return err(404, "no such file");
            };
            if team.role_for(me.id, me.admin, id).is_none() {
                return err(404, "no such file");
            }
            let doc = match team.room(id) {
                Ok(room) => room.state.lock().ok().map(|h| h.main.doc.clone()),
                Err(_) => None,
            };
            match doc.and_then(|d| deesainer_format::write(&d, None).ok()) {
                Some(bytes) => (200, json!({ "data": B64.encode(bytes) })),
                None => err(500, "cannot read the file"),
            }
        }
        ("POST", ["api", "files", id, "members"]) => {
            let Ok(id) = id.parse() else {
                return err(404, "no such file");
            };
            let role = match parse_role(&body["role"]) {
                Ok(r) => r,
                Err(e) => return err(400, e),
            };
            let email = body["email"].as_str().unwrap_or("").trim().to_lowercase();
            let member = team
                .accounts
                .lock()
                .ok()
                .and_then(|a| a.users().iter().find(|u| u.email == email).map(|u| u.id));
            let Some(member) = member else {
                return err(404, "no account with that email");
            };
            let changed = match team.files.lock() {
                Ok(mut f) => {
                    if !f.can_manage(me.id, me.admin, id) {
                        return err(403, "only the owner shares this file");
                    }
                    f.set_member(id, member, role)
                }
                Err(_) => return err(500, "busy"),
            };
            match changed {
                Ok(()) => {
                    team.refresh_access(id);
                    (200, json!({}))
                }
                Err(e) => err(400, e),
            }
        }
        ("DELETE", ["api", "files", id]) => {
            let Ok(id) = id.parse() else {
                return err(404, "no such file");
            };
            match team.files.lock() {
                Ok(mut f) => {
                    if !f.can_manage(me.id, me.admin, id) {
                        return err(403, "only the owner deletes this file");
                    }
                    match f.delete(id) {
                        Ok(()) => {
                            drop(f);
                            team.close_room(id);
                            (200, json!({}))
                        }
                        Err(e) => err(500, e),
                    }
                }
                Err(_) => err(500, "busy"),
            }
        }
        _ => err(404, "unknown request"),
    }
}
