#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use deesainer_automation::sync_client::SyncClient;
use deesainer_engine::Session;
use serde_json::{json, Value};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(45) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn http(
    addr: &str,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
    origin: Option<&str>,
) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    let text = body.to_string();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        text.len()
    );
    if !token.is_empty() {
        req.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(o) = origin {
        req.push_str(&format!("Origin: {o}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&text);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status: u16 = out.split(' ').nth(1).unwrap().parse().unwrap();
    let body = out.split("\r\n\r\n").nth(1).unwrap_or("null");
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

#[test]
fn teams_sign_in_share_files_and_edit_them_live() {
    let dir = std::env::temp_dir().join(format!("deesainer-team-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = env!("CARGO_BIN_EXE_deesainer-server");
    let made = Command::new(bin)
        .args([
            "--data",
            dir.to_str().unwrap(),
            "admin",
            "ada@example.com",
            "Ada",
        ])
        .env("DEESAINER_ADMIN_PASSWORD", "correct horse battery")
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let open = Command::new(bin)
        .args(["--data", dir.to_str().unwrap(), "--bind", "0.0.0.0:1"])
        .output()
        .unwrap();
    assert!(!open.status.success());
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let _server = Server(
        Command::new(bin)
            .args([
                "--data",
                dir.to_str().unwrap(),
                "--bind",
                &addr,
                "--allow-origin",
                "https://app.example",
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(wait_until(|| TcpStream::connect(&addr).is_ok()));
    let (status, _) = http(
        &addr,
        "POST",
        "/api/login",
        "",
        json!({"email": "ada@example.com", "password": "nope"}),
        None,
    );
    assert_eq!(status, 401);
    let (status, login) = http(
        &addr,
        "POST",
        "/api/login",
        "",
        json!({"email": "ada@example.com", "password": "correct horse battery"}),
        None,
    );
    assert_eq!(status, 200, "{login}");
    let ada = login["token"].as_str().unwrap().to_owned();
    assert_eq!(login["user"]["admin"], true);
    assert_eq!(
        http(&addr, "GET", "/api/files", "", json!(null), None).0,
        401
    );
    let (_, made) = http(
        &addr,
        "POST",
        "/api/files",
        &ada,
        json!({"name": "Landing page"}),
        None,
    );
    let file = made["id"].as_u64().unwrap().to_string();
    let (_, inv) = http(
        &addr,
        "POST",
        "/api/invites",
        &ada,
        json!({"email": "bob@example.com"}),
        None,
    );
    let code = inv["code"].as_str().unwrap();
    let (status, bob_login) = http(
        &addr,
        "POST",
        "/api/accept",
        "",
        json!({"code": code, "email": "bob@example.com", "name": "Bob", "password": "a long password"}),
        None,
    );
    assert_eq!(status, 200, "{bob_login}");
    let bob = bob_login["token"].as_str().unwrap().to_owned();
    assert_eq!(
        http(&addr, "GET", "/api/files", &bob, json!(null), None).1,
        json!([])
    );
    let mut a = Session::new().unwrap();
    let mut ca = SyncClient::connect_file(&addr, "ignored", &ada, "", &file);
    assert!(
        wait_until(|| {
            ca.poll(&mut a, None);
            ca.is_connected()
        }),
        "{}",
        ca.status
    );
    let rect = a
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 1, "y": 2, "width": 30, "height": 40}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let mut b = Session::new().unwrap();
    let mut cb = SyncClient::connect_file(&addr, "Bob", &bob, "", &file);
    std::thread::sleep(Duration::from_millis(800));
    cb.poll(&mut b, None);
    assert!(!cb.is_connected());
    drop(cb);
    let (status, _) = http(
        &addr,
        "POST",
        &format!("/api/files/{file}/members"),
        &ada,
        json!({"email": "bob@example.com", "role": "viewer"}),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(
        http(&addr, "GET", "/api/files", &bob, json!(null), None).1[0]["role"],
        "viewer"
    );
    let mut cb = SyncClient::connect_file(&addr, "Bob", &bob, "", &file);
    let node = deesainer_engine::doc::NodeId(rect);
    assert!(
        wait_until(|| {
            ca.poll(&mut a, None);
            cb.poll(&mut b, None);
            cb.is_connected() && b.doc().contains(node)
        }),
        "{}",
        cb.status
    );
    assert!(cb.is_read_only());
    assert_eq!(
        http(
            &addr,
            "POST",
            &format!("/api/files/{file}/members"),
            &bob,
            json!({"email": "bob@example.com", "role": "editor"}),
            None
        )
        .0,
        403
    );
    let (_, doc) = http(
        &addr,
        "GET",
        &format!("/api/files/{file}/document"),
        &bob,
        json!(null),
        None,
    );
    assert!(doc["data"].as_str().unwrap().len() > 100);
    http(
        &addr,
        "POST",
        &format!("/api/files/{file}/members"),
        &ada,
        json!({"email": "bob@example.com", "role": null}),
        None,
    );
    assert!(wait_until(|| {
        cb.poll(&mut b, None);
        !cb.is_connected()
    }));
    assert_eq!(
        http(
            &addr,
            "GET",
            "/api/me",
            &ada,
            json!(null),
            Some("https://evil.example")
        )
        .0,
        403
    );
    assert_eq!(
        http(
            &addr,
            "GET",
            "/api/me",
            &ada,
            json!(null),
            Some("https://app.example")
        )
        .0,
        200
    );
    let _ = std::fs::remove_dir_all(&dir);
}
