use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::control::MAX_REQUEST_BYTES;

pub const MAX_CONNECTIONS: usize = 16;
pub const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Request {
    pub line: String,
    pub reply: Sender<String>,
}

pub struct ControlServer {
    pub requests: Receiver<Request>,
    pub addr: SocketAddr,
}

pub fn load_or_create_token(path: &Path) -> Result<String, String> {
    if let Ok(t) = std::fs::read_to_string(path) {
        let t = t.trim().to_owned();
        if t.len() >= 32 {
            return Ok(t);
        }
        return Err(format!(
            "token in {} is too short (need 32+ characters)",
            path.display()
        ));
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no secure randomness: {e}"))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    f.write_all(token.as_bytes()).map_err(|e| e.to_string())?;
    Ok(token)
}

fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn read_line_capped(r: &mut BufReader<TcpStream>, line: &mut String) -> std::io::Result<usize> {
    line.clear();
    r.by_ref()
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_line(line)
}

fn send(writer: &mut TcpStream, v: &Value) -> bool {
    writeln!(writer, "{v}")
        .and_then(|()| writer.flush())
        .is_ok()
}

fn serve_connection(stream: TcpStream, token: &str, tx: &Sender<Request>) {
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    match read_line_capped(&mut reader, &mut line) {
        Ok(n) if n > 0 && n <= MAX_REQUEST_BYTES => {}
        _ => return,
    }
    let auth: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
    let ok = auth["method"] == "auth"
        && auth["params"]["token"]
            .as_str()
            .is_some_and(|t| same(t, token));
    if !ok {
        let _ = send(
            &mut writer,
            &json!({"id": auth["id"], "error": {"message": "authentication failed"}}),
        );
        return;
    }
    if !send(
        &mut writer,
        &json!({"id": auth["id"], "result": {"ok": true}}),
    ) {
        return;
    }
    loop {
        match read_line_capped(&mut reader, &mut line) {
            Ok(0) | Err(_) => return,
            Ok(n) if n > MAX_REQUEST_BYTES => {
                let _ = send(
                    &mut writer,
                    &json!({"id": null, "error": {"message": "request exceeds 1 MiB"}}),
                );
                return;
            }
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let (reply_tx, reply_rx) = channel();
        if tx
            .send(Request {
                line: line.trim().to_owned(),
                reply: reply_tx,
            })
            .is_err()
        {
            return;
        }
        let reply = reply_rx.recv_timeout(TIMEOUT).unwrap_or_else(|_| {
            json!({"id": null, "error": {"message": "the app did not answer in time"}}).to_string()
        });
        if writeln!(writer, "{reply}")
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
    }
}

impl ControlServer {
    pub fn start(port: u16, token: String) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let (tx, rx) = channel::<Request>();
        let live = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                if live.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    let _ = writeln!(
                        stream,
                        "{}",
                        json!({"id": null, "error": {"message": "too many connections"}})
                    );
                    continue;
                }
                live.fetch_add(1, Ordering::SeqCst);
                let (tx, token, live) = (tx.clone(), token.clone(), Arc::clone(&live));
                std::thread::spawn(move || {
                    serve_connection(stream, &token, &tx);
                    live.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(Self { requests: rx, addr })
    }
}

pub struct ControlClient {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next: u64,
}

impl ControlClient {
    pub fn connect(addr: &str, token: &str) -> Result<Self, String> {
        let stream =
            TcpStream::connect(addr).map_err(|e| format!("cannot reach the app at {addr}: {e}"))?;
        let _ = stream.set_read_timeout(Some(TIMEOUT + Duration::from_secs(5)));
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut c = Self {
            reader: BufReader::new(stream),
            writer,
            next: 0,
        };
        let reply = c.raw(json!({"id": "auth", "method": "auth", "params": {"token": token}}))?;
        if reply.get("error").is_some() {
            return Err("the app refused the token".into());
        }
        Ok(c)
    }

    fn raw(&mut self, msg: Value) -> Result<Value, String> {
        writeln!(self.writer, "{msg}").map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Err("the app closed the connection".into());
        }
        serde_json::from_str(&line).map_err(|e| e.to_string())
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let reply = self.raw(json!({"id": self.next, "method": method, "params": params}))?;
        match reply.get("error") {
            Some(e) => Err(e["message"].as_str().unwrap_or("error").to_owned()),
            None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
        }
    }
}

impl crate::Backend for ControlClient {
    fn dispatch(&mut self, command: &str, params: &Value) -> Result<Value, String> {
        self.call(
            "engine.execute",
            json!({"command": command, "params": params}),
        )
    }

    fn batch(&mut self, label: &str, steps: &[(String, Value)]) -> Result<Vec<Value>, String> {
        let steps: Vec<Value> = steps
            .iter()
            .map(|(c, p)| json!({"command": c, "params": p}))
            .collect();
        let v = self.call("engine.batch", json!({"label": label, "steps": steps}))?;
        Ok(v.as_array().cloned().unwrap_or_default())
    }

    fn commands(&mut self) -> Result<Value, String> {
        self.call("engine.commands", Value::Null)
    }

    fn bridge_call(&mut self, method: &str, params: &Value) -> Option<Result<Value, String>> {
        Some(self.call(method, params.clone()))
    }

    fn is_bridge(&self) -> bool {
        true
    }
}
