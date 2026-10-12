use std::io::{BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use deesainer_automation::tls;
use deesainer_server::team::Team;
use deesainer_server::{api, ws};
use serde_json::json;

enum Kind {
    WebSocket,
    Http,
    Protocol,
}

fn kind(stream: &TcpStream) -> Kind {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut head = [0u8; 2048];
    let mut seen = 0;
    for _ in 0..100 {
        match stream.peek(&mut head) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen = n;
                if head[..n].windows(4).any(|w| w == b"\r\n\r\n") || (n >= 8 && head[0] == b'{') {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    let _ = stream.set_read_timeout(None);
    let head = &head[..seen];
    if !api::looks_like_http(head) {
        return Kind::Protocol;
    }
    let text = String::from_utf8_lossy(head).to_lowercase();
    if text.starts_with("get ") && text.contains("upgrade: websocket") {
        Kind::WebSocket
    } else {
        Kind::Http
    }
}

fn serve(stream: TcpStream, team: &Team, inner: std::net::SocketAddr, origins: &[String]) {
    match kind(&stream) {
        Kind::WebSocket => ws::bridge(stream, inner, origins),
        Kind::Http => {
            let Ok(mut out) = stream.try_clone() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let req = match api::read_request(&mut reader) {
                Ok(r) => r,
                Err(e) => return api::respond(&mut out, 400, &json!({ "error": e }), None),
            };
            let origin = req.header("origin").map(str::to_owned);
            let cors = origin.as_deref().filter(|o| origins.iter().any(|a| a == o));
            if origin.is_some() && cors.is_none() {
                return api::respond(
                    &mut out,
                    403,
                    &json!({ "error": "origin not allowed" }),
                    None,
                );
            }
            if req.method == "OPTIONS" {
                return api::respond(&mut out, 204, &json!({}), cors);
            }
            let (status, body) = api::handle(team, &req);
            api::respond(&mut out, status, &body, cors);
        }
        Kind::Protocol => {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            let read = reader.by_ref().take(1024 * 1024).read_line(&mut line);
            if matches!(read, Ok(n) if n > 0) {
                team.serve_protocol(reader, line);
            }
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let usage = "usage: deesainer-server --data DIR [--bind ADDR] [--tls-cert F --tls-key F] [--allow-origin ORIGIN]... | --data DIR admin EMAIL NAME";
    let data = PathBuf::from(flag("--data").ok_or(usage)?);
    let team = Arc::new(Team::open(&data)?);
    if let Some(i) = args.iter().position(|a| a == "admin") {
        let (Some(email), Some(name)) = (args.get(i + 1), args.get(i + 2)) else {
            return Err(usage.into());
        };
        let password = std::env::var("DEESAINER_ADMIN_PASSWORD")
            .map_err(|_| "set DEESAINER_ADMIN_PASSWORD to the new administrator's password")?;
        let id = team
            .accounts
            .lock()
            .map_err(|_| "busy")?
            .bootstrap_admin(email, name, &password)
            .map_err(|e| e.to_string())?;
        println!("administrator {email} created (id {id})");
        return Ok(());
    }
    let bind = flag("--bind").unwrap_or_else(|| "127.0.0.1:7800".into());
    let tls = match (flag("--tls-cert"), flag("--tls-key")) {
        (Some(cert), Some(key)) => {
            let cert = std::fs::read(cert).map_err(|e| format!("cannot read certificate: {e}"))?;
            let key = std::fs::read(key).map_err(|e| format!("cannot read private key: {e}"))?;
            Some(tls::server_config(&cert, &key)?)
        }
        (None, None) => None,
        _ => return Err("--tls-cert and --tls-key go together".into()),
    };
    let loopback =
        bind.starts_with("127.") || bind.starts_with("localhost") || bind.starts_with("[::1]");
    if tls.is_none() && !loopback {
        return Err("beyond loopback the server only listens with --tls-cert and --tls-key".into());
    }
    let origins: Arc<Vec<String>> = Arc::new(
        args.windows(2)
            .filter(|w| w[0] == "--allow-origin")
            .map(|w| w[1].clone())
            .collect(),
    );
    {
        let team = Arc::clone(&team);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(10));
            team.tick();
        });
    }
    let inner = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let inner_addr = inner.local_addr().map_err(|e| e.to_string())?;
    let spawn = {
        let team = Arc::clone(&team);
        let origins = Arc::clone(&origins);
        move |stream: TcpStream| {
            let team = Arc::clone(&team);
            let origins = Arc::clone(&origins);
            std::thread::spawn(move || serve(stream, &team, inner_addr, &origins));
        }
    };
    {
        let spawn = spawn.clone();
        std::thread::spawn(move || {
            for stream in inner.incoming().flatten() {
                spawn(stream);
            }
        });
    }
    let public = TcpListener::bind(&bind).map_err(|e| format!("cannot listen on {bind}: {e}"))?;
    eprintln!(
        "deesainer-server: {} on {bind}{}",
        data.display(),
        if tls.is_some() { " (TLS)" } else { "" }
    );
    for stream in public.incoming() {
        match stream {
            Ok(s) => match &tls {
                Some(config) => {
                    let config = Arc::clone(config);
                    std::thread::spawn(move || tls::terminate(config, s, inner_addr));
                }
                None => spawn(s),
            },
            Err(e) => eprintln!("deesainer-server: accept failed: {e}"),
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("deesainer-server: {e}");
            ExitCode::FAILURE
        }
    }
}
