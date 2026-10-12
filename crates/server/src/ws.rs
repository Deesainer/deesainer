use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Error, Message};

pub fn is_websocket(stream: &TcpStream) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = [0u8; 4];
    let mut seen = 0;
    for _ in 0..50 {
        match stream.peek(&mut head) {
            Ok(n) if n >= 4 => {
                seen = n;
                break;
            }
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen = n;
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    let _ = stream.set_read_timeout(None);
    seen >= 4 && &head == b"GET "
}

pub fn bridge(stream: TcpStream, inner: SocketAddr, origins: &[String]) {
    #[allow(clippy::result_large_err)]
    let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let origin = req.headers().get("origin").and_then(|o| o.to_str().ok());
        match origin {
            Some(o) if !origins.iter().any(|a| a == o) => {
                let mut refusal = ErrorResponse::new(Some(format!(
                    "origin {o} is not allowed (start the relay with --allow-origin {o})"
                )));
                *refusal.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
                Err(refusal)
            }
            _ => Ok(resp),
        }
    };
    let Ok(mut ws) = tungstenite::accept_hdr(stream, check) else {
        return;
    };
    let Ok(mut plain) = TcpStream::connect(inner) else {
        return;
    };
    let _ = plain.set_nodelay(true);
    if plain.set_nonblocking(true).is_err()
        || ws
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(20)))
            .is_err()
    {
        return;
    }
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let mut line = text.as_bytes().to_vec();
                line.push(b'\n');
                if write_all(&mut plain, &line).is_err() {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        match plain.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => pending.extend_from_slice(buf.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        if pending.len() > 64 * 1024 * 1024 {
            break;
        }
        while let Some(at) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=at).collect();
            let text = String::from_utf8_lossy(line.get(..at).unwrap_or_default()).into_owned();
            if ws.send(Message::text(text)).is_err() {
                return;
            }
        }
    }
    let _ = plain.shutdown(std::net::Shutdown::Both);
    let _ = ws.close(None);
}

fn write_all(w: &mut TcpStream, mut bytes: &[u8]) -> std::io::Result<()> {
    while !bytes.is_empty() {
        match w.write(bytes) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => bytes = bytes.get(n..).unwrap_or_default(),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
