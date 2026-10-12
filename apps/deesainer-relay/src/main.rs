use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use deesainer_automation::tls;
use deesainer_server::room::RoomService;
use deesainer_server::ws;
use deesainer_sync::{ServerState, User};

fn load_users(path: &std::path::Path) -> Result<Vec<User>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read users file: {e}"))?;
    let users: Vec<User> =
        serde_json::from_str(&text).map_err(|e| format!("bad users file: {e}"))?;
    for (i, u) in users.iter().enumerate() {
        if u.name.trim().is_empty() || u.name.chars().count() > 64 {
            return Err(format!("user {i}: the name needs 1 to 64 characters"));
        }
        if u.token.chars().count() < 16 {
            return Err(format!(
                "user {:?}: the token needs at least 16 characters",
                u.name
            ));
        }
        if users[..i]
            .iter()
            .any(|o| o.name == u.name || o.token == u.token)
        {
            return Err(format!("user {:?}: name and token must be unique", u.name));
        }
    }
    Ok(users)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let file = PathBuf::from(flag("--file").ok_or(
        "usage: deesainer-relay --file F [--bind ADDR] [--token-file F] [--viewer-token-file F] [--commenter-token-file F] [--users-file F] [--audit-log F] [--tls-cert F --tls-key F] [--autosave SECS]",
    )?);
    let bind = flag("--bind").unwrap_or_else(|| "127.0.0.1:7700".into());
    let token = match flag("--token-file") {
        Some(p) => std::fs::read_to_string(p)
            .map_err(|e| e.to_string())?
            .trim()
            .to_owned(),
        None => String::new(),
    };
    let viewer = match flag("--viewer-token-file") {
        Some(p) => std::fs::read_to_string(p)
            .map_err(|e| e.to_string())?
            .trim()
            .to_owned(),
        None => String::new(),
    };
    let commenter = match flag("--commenter-token-file") {
        Some(p) => std::fs::read_to_string(p)
            .map_err(|e| e.to_string())?
            .trim()
            .to_owned(),
        None => String::new(),
    };
    if (!viewer.is_empty() || !commenter.is_empty()) && token.is_empty() {
        return Err(
            "--viewer-token-file and --commenter-token-file need --token-file (the editor token)"
                .into(),
        );
    }
    let users_file = flag("--users-file").map(PathBuf::from);
    let users = match &users_file {
        Some(p) => load_users(p)?,
        None => Vec::new(),
    };
    if token.is_empty()
        && users.is_empty()
        && !bind.starts_with("127.")
        && !bind.starts_with("localhost")
    {
        return Err(
            "refusing to listen beyond loopback without --token-file or --users-file".into(),
        );
    }
    let autosave = flag("--autosave")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(10)
        .max(1);
    let audit = flag("--audit-log").map_or_else(
        || {
            let mut name = file.as_os_str().to_owned();
            name.push(".audit.log");
            PathBuf::from(name)
        },
        PathBuf::from,
    );
    let service = Arc::new(RoomService::open(&file, audit, |doc| {
        ServerState::new(doc, &token)
            .with_viewer_token(&viewer)
            .with_commenter_token(&commenter)
            .with_users(users)
    })?);
    if let Some(path) = users_file {
        let service = Arc::clone(&service);
        std::thread::spawn(move || {
            let modified =
                |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            let mut seen = modified(&path);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let now = modified(&path);
                if now == seen {
                    continue;
                }
                seen = now;
                let users = match load_users(&path) {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("relay: keeping the old users, {e}");
                        continue;
                    }
                };
                let kicked = match service.state.lock() {
                    Ok(mut hub) => hub.set_users(users),
                    Err(_) => continue,
                };
                for (room, id) in kicked {
                    service.kick(&room, id, "your access was changed or removed");
                }
            }
        });
    }
    {
        let service = Arc::clone(&service);
        std::thread::spawn(move || {
            let mut saved = (u64::MAX, BTreeMap::new());
            loop {
                std::thread::sleep(Duration::from_secs(autosave));
                service.save_if_changed(&mut saved);
            }
        });
    }
    let tls = match (flag("--tls-cert"), flag("--tls-key")) {
        (Some(cert), Some(key)) => {
            let cert = std::fs::read(cert).map_err(|e| format!("cannot read certificate: {e}"))?;
            let key = std::fs::read(key).map_err(|e| format!("cannot read private key: {e}"))?;
            Some(tls::server_config(&cert, &key)?)
        }
        (None, None) => None,
        _ => return Err("--tls-cert and --tls-key go together".into()),
    };
    let origins: Arc<Vec<String>> = Arc::new(
        args.windows(2)
            .filter(|w| w[0] == "--allow-origin")
            .map(|w| w[1].clone())
            .collect(),
    );
    let inner = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let inner_addr = inner.local_addr().map_err(|e| e.to_string())?;
    let serve = {
        let service = Arc::clone(&service);
        let origins = Arc::clone(&origins);
        move |stream: TcpStream| {
            let service = Arc::clone(&service);
            let origins = Arc::clone(&origins);
            std::thread::spawn(move || {
                if ws::is_websocket(&stream) {
                    ws::bridge(stream, inner_addr, &origins);
                } else {
                    service.handle(BufReader::new(stream), None);
                }
            });
        }
    };
    {
        let serve = serve.clone();
        std::thread::spawn(move || {
            for stream in inner.incoming().flatten() {
                serve(stream);
            }
        });
    }
    if let Some(config) = tls {
        let public =
            TcpListener::bind(&bind).map_err(|e| format!("cannot listen on {bind}: {e}"))?;
        eprintln!("relay: serving {} on {bind} (TLS)", file.display());
        for stream in public.incoming() {
            match stream {
                Ok(s) => {
                    let config = Arc::clone(&config);
                    std::thread::spawn(move || tls::terminate(config, s, inner_addr));
                }
                Err(e) => eprintln!("relay: accept failed: {e}"),
            }
        }
        return Ok(());
    }
    let listener = TcpListener::bind(&bind).map_err(|e| format!("cannot listen on {bind}: {e}"))?;
    eprintln!("relay: serving {} on {bind}", file.display());
    for stream in listener.incoming() {
        match stream {
            Ok(s) => serve(s),
            Err(e) => eprintln!("relay: accept failed: {e}"),
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("deesainer-relay: {e}");
            ExitCode::FAILURE
        }
    }
}
