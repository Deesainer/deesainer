#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use deesainer_automation::sync_client::SyncClient;
use deesainer_engine::Session;
use serde_json::json;

struct Relay(Child);

impl Drop for Relay {
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

#[test]
fn two_editors_converge_through_the_relay() {
    let dir = std::env::temp_dir().join(format!("deesainer-relay-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("shared.deesainer");
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let _relay = Relay(
        Command::new(env!("CARGO_BIN_EXE_deesainer-relay"))
            .args([
                "--file",
                file.to_str().unwrap(),
                "--bind",
                &addr,
                "--autosave",
                "1",
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut a = Session::new().unwrap();
    let mut b = Session::new().unwrap();
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    let mut ca = SyncClient::connect(&addr, "Ada", "");
    let mut cb = SyncClient::connect(&addr, "Bob", "");
    assert!(
        wait_until(|| {
            ca.poll(&mut a, None);
            cb.poll(&mut b, None);
            ca.is_connected() && cb.is_connected()
        }),
        "not connected: {} / {}",
        ca.status,
        cb.status
    );
    let id = a
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 10, "y": 10, "width": 50, "height": 50}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    assert!(wait_until(|| {
        ca.poll(&mut a, Some(deesainer_geom::Point::new(5.0, 5.0)));
        cb.poll(&mut b, None);
        b.doc().contains(deesainer_engine::doc::NodeId(id))
    }));
    b.dispatch("node.move", &json!({"ids": [id], "dx": 100, "dy": 0}))
        .unwrap();
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        a.doc()
            .get(deesainer_engine::doc::NodeId(id))
            .is_some_and(|n| n.rect.x == 110.0)
    }));
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        ca.pending() == 0 && cb.pending() == 0 && a.doc() == b.doc()
    }));
    assert!(cb.peers().iter().any(|p| p.name == "Ada"));
    assert!(wait_until(|| deesainer_automation::files::load_document(
        &file
    )
    .is_ok_and(|d| d.contains(deesainer_engine::doc::NodeId(id)))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn editors_connect_over_tls() {
    let dir = std::env::temp_dir().join(format!("deesainer-relay-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.cert.pem()).unwrap();
    std::fs::write(&key_path, cert.key_pair.serialize_pem()).unwrap();
    let file = dir.join("shared.deesainer");
    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let _relay = Relay(
        Command::new(env!("CARGO_BIN_EXE_deesainer-relay"))
            .args([
                "--file",
                file.to_str().unwrap(),
                "--bind",
                &bind,
                "--tls-cert",
                cert_path.to_str().unwrap(),
                "--tls-key",
                key_path.to_str().unwrap(),
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(wait_until(|| std::net::TcpStream::connect(&bind).is_ok()));
    let mut refused = Session::new().unwrap();
    let mut bad = SyncClient::connect(&format!("tls://localhost:{port}"), "Mallory", "");
    std::thread::sleep(Duration::from_millis(800));
    bad.poll(&mut refused, None);
    assert!(!bad.is_connected());
    std::env::set_var("DEESAINER_TLS_CA", &cert_path);
    let mut a = Session::new().unwrap();
    let mut b = Session::new().unwrap();
    let addr = format!("tls://localhost:{port}");
    let mut ca = SyncClient::connect(&addr, "Ada", "");
    let mut cb = SyncClient::connect(&addr, "Bob", "");
    assert!(
        wait_until(|| {
            ca.poll(&mut a, None);
            cb.poll(&mut b, None);
            ca.is_connected() && cb.is_connected()
        }),
        "not connected: {} / {}",
        ca.status,
        cb.status
    );
    let id = a
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 1, "y": 2, "width": 30, "height": 40}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        b.doc().contains(deesainer_engine::doc::NodeId(id))
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn viewer_sees_edits_but_cannot_make_them() {
    let dir = std::env::temp_dir().join(format!("deesainer-relay-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let edit_tok = dir.join("edit.tok");
    let view_tok = dir.join("view.tok");
    std::fs::write(&edit_tok, "editor-secret").unwrap();
    std::fs::write(&view_tok, "viewer-secret").unwrap();
    let file = dir.join("shared.deesainer");
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let _relay = Relay(
        Command::new(env!("CARGO_BIN_EXE_deesainer-relay"))
            .args([
                "--file",
                file.to_str().unwrap(),
                "--bind",
                &addr,
                "--token-file",
                edit_tok.to_str().unwrap(),
                "--viewer-token-file",
                view_tok.to_str().unwrap(),
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    let mut a = Session::new().unwrap();
    let mut v = Session::new().unwrap();
    let mut ca = SyncClient::connect(&addr, "Ada", "editor-secret");
    let mut cv = SyncClient::connect(&addr, "Vic", "viewer-secret");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cv.poll(&mut v, None);
        ca.is_connected() && cv.is_connected()
    }));
    assert!(cv.status.contains("view only"));
    let id = a
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 1, "y": 2, "width": 30, "height": 40}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cv.poll(&mut v, None);
        v.doc().contains(deesainer_engine::doc::NodeId(id))
    }));
    v.dispatch("node.move", &json!({"ids": [id], "dx": 500, "dy": 0}))
        .unwrap();
    let x = |s: &Session| {
        s.doc()
            .get(deesainer_engine::doc::NodeId(id))
            .map(|n| n.rect.x)
    };
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cv.poll(&mut v, None);
        x(&v) == Some(1.0)
    }));
    for _ in 0..30 {
        ca.poll(&mut a, None);
        cv.poll(&mut v, None);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(x(&a), Some(1.0));
    assert_eq!(x(&v), Some(1.0));
    let mut other = Session::new().unwrap();
    let mut bad = SyncClient::connect(&addr, "Eve", "guess");
    std::thread::sleep(Duration::from_millis(500));
    bad.poll(&mut other, None);
    assert!(!bad.is_connected());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn versions_are_saved_restored_and_persisted() {
    let dir = std::env::temp_dir().join(format!("deesainer-relay-ver-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("shared.deesainer");
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let spawn = || {
        Relay(
            Command::new(env!("CARGO_BIN_EXE_deesainer-relay"))
                .args(["--file", file.to_str().unwrap(), "--bind", &addr])
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let relay = spawn();
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    let mut a = Session::new().unwrap();
    let mut b = Session::new().unwrap();
    let mut ca = SyncClient::connect(&addr, "Ada", "");
    let mut cb = SyncClient::connect(&addr, "Bob", "");
    assert!(
        wait_until(|| {
            ca.poll(&mut a, None);
            cb.poll(&mut b, None);
            ca.is_connected() && cb.is_connected()
        }),
        "not connected: {} / {}",
        ca.status,
        cb.status
    );
    let id = a
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 1, "y": 2, "width": 30, "height": 40}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let node = deesainer_engine::doc::NodeId(id);
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        b.doc().contains(node)
    }));
    ca.save_version("With rectangle");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        cb.versions.len() == 1 && ca.versions.len() == 1
    }));
    assert_eq!(cb.versions[0].label, "With rectangle");
    assert_eq!(cb.versions[0].author, "Ada");
    b.dispatch("edit.delete", &json!({"ids": [id]})).unwrap();
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        !a.doc().contains(node)
    }));
    ca.restore_version(cb.versions[0].id);
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        a.doc().contains(node) && b.doc().contains(node) && ca.versions.len() == 2
    }));
    assert_eq!(ca.versions[0].label, "Before restore");
    drop(relay);
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_err()));
    let _relay = spawn();
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    let mut c = Session::new().unwrap();
    let mut cc = SyncClient::connect(&addr, "Cy", "");
    assert!(wait_until(|| {
        cc.poll(&mut c, None);
        cc.versions.len() == 2
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn branches_evolve_apart_merge_back_and_persist() {
    let dir = std::env::temp_dir().join(format!("deesainer-relay-branch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("shared.deesainer");
    let addr = format!("127.0.0.1:{}", free_port());
    let spawn = || {
        Relay(
            Command::new(env!("CARGO_BIN_EXE_deesainer-relay"))
                .args(["--file", file.to_str().unwrap(), "--bind", &addr])
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let relay = spawn();
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    let mut a = Session::new().unwrap();
    let mut ca = SyncClient::connect(&addr, "Ada", "");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        ca.is_connected()
    }));
    ca.create_branch("redesign");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        ca.branches.len() == 1
    }));
    let mut b = Session::new().unwrap();
    let mut cb = SyncClient::connect_branch(&addr, "Bob", "", "redesign");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        cb.is_connected()
    }));
    assert_eq!(cb.branch, "redesign");
    let on_branch = b
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 1, "y": 2, "width": 30, "height": 40}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let on_main = a
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 100, "y": 2, "width": 30, "height": 40}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    for _ in 0..40 {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        std::thread::sleep(Duration::from_millis(20));
    }
    let branch_node = deesainer_engine::doc::NodeId(on_branch);
    let main_node = deesainer_engine::doc::NodeId(on_main);
    assert!(on_branch >= 1 << 32);
    assert!(on_main < 1 << 32);
    assert!(!a.doc().contains(branch_node));
    assert!(!b.doc().contains(main_node));
    ca.preview_merge("redesign");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        ca.merge_preview.as_ref().is_some_and(|p| p.0 == "redesign")
    }));
    let preview = ca.merge_preview.clone().unwrap();
    assert!(
        preview.1.iter().any(|l| l.contains("layer(s) added")),
        "{preview:?}"
    );
    assert!(!a.doc().contains(branch_node), "a preview changes nothing");
    let merged = ca
        .merge_preview_doc
        .clone()
        .expect("the preview carries the merged main");
    assert!(merged.contains(branch_node) && merged.contains(main_node));
    let c = deesainer_engine::compare::documents(a.doc(), &merged, 0, 1.0).unwrap();
    assert!(c.changed >= 1000, "{}", c.changed);
    ca.merge_branch("redesign", false);
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        a.doc().contains(branch_node) && a.doc().contains(main_node)
    }));
    assert_eq!(ca.last_merge.as_ref().map(|m| m.1), Some(1));
    cb.save_version("branch checkpoint");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        cb.poll(&mut b, None);
        cb.versions.len() == 1
    }));
    assert!(ca.versions.is_empty());
    drop(relay);
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_err()));
    let _relay = spawn();
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    let mut c = Session::new().unwrap();
    let mut cc = SyncClient::connect_branch(&addr, "Cy", "", "redesign");
    assert!(wait_until(|| {
        cc.poll(&mut c, None);
        cc.is_connected() && c.doc().contains(branch_node) && cc.versions.len() == 1
    }));
    assert_eq!(cc.versions[0].label, "branch checkpoint");
    cc.delete_branch("redesign");
    let versions_dir = dir.join("shared.deesainer.branches");
    assert!(wait_until(|| std::fs::read_dir(&versions_dir)
        .map(|d| d
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".versions")))
        .unwrap_or(true)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn roles_accounts_revocation_and_audit() {
    let dir = std::env::temp_dir().join(format!("deesainer-relay-roles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let users_path = dir.join("users.json");
    let users = |with_ada: bool| {
        let mut list =
            vec![json!({"name": "Bob", "token": "bob-token-0123456789", "role": "editor"})];
        if with_ada {
            list.push(json!({"name": "Ada", "token": "ada-token-0123456789", "role": "commenter"}));
        }
        std::fs::write(&users_path, serde_json::to_string(&list).unwrap()).unwrap();
    };
    users(true);
    let file = dir.join("shared.deesainer");
    let addr = format!("127.0.0.1:{}", free_port());
    let _relay = Relay(
        Command::new(env!("CARGO_BIN_EXE_deesainer-relay"))
            .args([
                "--file",
                file.to_str().unwrap(),
                "--bind",
                &addr,
                "--users-file",
                users_path.to_str().unwrap(),
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    let mut nobody = Session::new().unwrap();
    let mut anon = SyncClient::connect(&addr, "Eve", "");
    std::thread::sleep(Duration::from_millis(500));
    anon.poll(&mut nobody, None);
    assert!(!anon.is_connected());
    let mut b = Session::new().unwrap();
    let mut a = Session::new().unwrap();
    let mut cb = SyncClient::connect(&addr, "ignored", "bob-token-0123456789");
    let mut ca = SyncClient::connect(&addr, "ignored", "ada-token-0123456789");
    assert!(wait_until(|| {
        cb.poll(&mut b, None);
        ca.poll(&mut a, None);
        cb.is_connected() && ca.is_connected()
    }));
    assert!(ca.status.contains("comment only"), "{}", ca.status);
    assert!(
        cb.peers().iter().any(|p| p.name == "Ada"),
        "accounts name their peers"
    );
    let id = b
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 1, "y": 2, "width": 30, "height": 40}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let node = deesainer_engine::doc::NodeId(id);
    assert!(wait_until(|| {
        cb.poll(&mut b, None);
        ca.poll(&mut a, None);
        a.doc().contains(node)
    }));
    a.dispatch("comment.add", &json!({"x": 5, "y": 5, "text": "Too tight"}))
        .unwrap();
    a.dispatch("node.move", &json!({"ids": [id], "dx": 300, "dy": 0}))
        .unwrap();
    assert!(wait_until(|| {
        cb.poll(&mut b, None);
        ca.poll(&mut a, None);
        b.doc().comments().count() == 1 && a.doc().get(node).is_some_and(|n| n.rect.x == 1.0)
    }));
    assert_eq!(b.doc().get(node).unwrap().rect.x, 1.0);
    users(false);
    assert!(wait_until(|| {
        cb.poll(&mut b, None);
        ca.poll(&mut a, None);
        !ca.is_connected()
    }));
    std::thread::sleep(Duration::from_millis(2500));
    ca.poll(&mut a, None);
    assert!(!ca.is_connected(), "{}", ca.status);
    let log = std::fs::read_to_string(dir.join("shared.deesainer.audit.log")).unwrap();
    for needle in [
        "\"event\":\"join\"",
        "\"event\":\"edit\"",
        "\"event\":\"revoked\"",
        "\"user\":\"Ada\"",
        "\"role\":\"commenter\"",
    ] {
        assert!(log.contains(needle), "missing {needle} in {log}");
    }
    assert!(!log.contains("token"), "tokens never reach the log");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn browsers_join_over_websocket_from_allowed_origins() {
    use tungstenite::client::IntoClientRequest;
    let dir = std::env::temp_dir().join(format!("deesainer-relay-ws-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("shared.deesainer");
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let _relay = Relay(
        Command::new(env!("CARGO_BIN_EXE_deesainer-relay"))
            .args([
                "--file",
                file.to_str().unwrap(),
                "--bind",
                &addr,
                "--allow-origin",
                "https://app.example",
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(wait_until(|| std::net::TcpStream::connect(&addr).is_ok()));
    #[allow(clippy::result_large_err)]
    let open = |origin: Option<&str>| {
        let mut req = format!("ws://{addr}/").into_client_request().unwrap();
        if let Some(o) = origin {
            req.headers_mut().insert("Origin", o.parse().unwrap());
        }
        tungstenite::connect(req)
    };
    assert!(open(Some("https://evil.example")).is_err());
    let (mut ws, _) = open(Some("https://app.example")).unwrap();
    let hello = serde_json::to_string(&deesainer_sync::client::hello("Web", "", "", "")).unwrap();
    ws.send(tungstenite::Message::text(hello)).unwrap();
    let welcome = loop {
        match ws.read().unwrap() {
            tungstenite::Message::Text(t) => break t.to_string(),
            _ => continue,
        }
    };
    let msg: deesainer_sync::ServerMsg = serde_json::from_str(&welcome).unwrap();
    assert!(
        matches!(msg, deesainer_sync::ServerMsg::Welcome { .. }),
        "{welcome}"
    );
    let mut a = Session::new().unwrap();
    let mut ca = SyncClient::connect(&addr, "Ada", "");
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        ca.is_connected()
    }));
    let mut state = deesainer_sync::ClientState::from_welcome(&msg).unwrap();
    let page = state.confirmed.first_page();
    let batch = state.local(vec![deesainer_engine::ops::Op::set(
        page,
        deesainer_engine::doc::PropValue::Name("From the browser".into()),
    )]);
    ws.send(tungstenite::Message::text(
        serde_json::to_string(&batch).unwrap(),
    ))
    .unwrap();
    assert!(wait_until(|| {
        ca.poll(&mut a, None);
        a.doc()
            .get(page)
            .is_some_and(|p| p.name == "From the browser")
    }));
    let _ = std::fs::remove_dir_all(&dir);
}
