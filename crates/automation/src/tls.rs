use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
};

pub const CA_ENV: &str = "DEESAINER_TLS_CA";

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

pub fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> Result<Arc<ServerConfig>, String> {
    let certs = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("bad certificate: {e}"))?;
    if certs.is_empty() {
        return Err("the certificate file holds no certificate".into());
    }
    let key =
        PrivateKeyDer::from_pem_slice(key_pem).map_err(|e| format!("bad private key: {e}"))?;
    let config = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(config))
}

pub fn client_config(extra_ca_pem: Option<&[u8]>) -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(pem) = extra_ca_pem {
        for cert in CertificateDer::pem_slice_iter(pem) {
            let cert = cert.map_err(|e| format!("bad CA certificate: {e}"))?;
            roots.add(cert).map_err(|e| e.to_string())?;
        }
    }
    let config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn write_all_retry<W: Write>(w: &mut W, mut buf: &[u8]) -> std::io::Result<()> {
    while !buf.is_empty() {
        match w.write(buf) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => buf = buf.get(n..).unwrap_or_default(),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn pump<S: Read + Write>(mut secure: S, mut plain: TcpStream) {
    if plain.set_nonblocking(true).is_err() {
        return;
    }
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let mut idle = true;
        match secure.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                idle = false;
                if write_all_retry(&mut plain, buf.get(..n).unwrap_or_default()).is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        match plain.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                idle = false;
                if write_all_retry(&mut secure, buf.get(..n).unwrap_or_default()).is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        match secure.flush() {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        if idle {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let _ = plain.shutdown(std::net::Shutdown::Both);
}

pub fn terminate(config: Arc<ServerConfig>, socket: TcpStream, inner: SocketAddr) {
    let _ = socket.set_nodelay(true);
    let _ = socket.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = socket.set_write_timeout(Some(HANDSHAKE_TIMEOUT));
    let Ok(mut conn) = ServerConnection::new(config) else {
        return;
    };
    let mut socket = socket;
    while conn.is_handshaking() {
        if conn.complete_io(&mut socket).is_err() {
            return;
        }
    }
    let Ok(plain) = TcpStream::connect(inner) else {
        return;
    };
    let _ = plain.set_nodelay(true);
    let _ = socket.set_read_timeout(None);
    let _ = socket.set_write_timeout(None);
    if socket.set_nonblocking(true).is_err() {
        return;
    }
    pump(StreamOwned::new(conn, socket), plain);
}

pub fn forward(target: &str) -> Result<SocketAddr, String> {
    let ca = match std::env::var_os(CA_ENV) {
        Some(path) => Some(std::fs::read(&path).map_err(|e| format!("cannot read {CA_ENV}: {e}"))?),
        None => None,
    };
    let config = client_config(ca.as_deref())?;
    let host = target
        .rsplit_once(':')
        .map_or(target, |(h, _)| h)
        .trim_matches(|c| c == '[' || c == ']')
        .to_owned();
    let name = ServerName::try_from(host).map_err(|e| format!("bad host name: {e}"))?;
    let mut socket = TcpStream::connect(target).map_err(|e| e.to_string())?;
    let _ = socket.set_nodelay(true);
    let _ = socket.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = socket.set_write_timeout(Some(HANDSHAKE_TIMEOUT));
    let mut conn = ClientConnection::new(config, name).map_err(|e| e.to_string())?;
    while conn.is_handshaking() {
        conn.complete_io(&mut socket)
            .map_err(|e| format!("TLS handshake failed: {e}"))?;
    }
    let _ = socket.set_read_timeout(None);
    let _ = socket.set_write_timeout(None);
    socket.set_nonblocking(true).map_err(|e| e.to_string())?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        if let Ok((plain, _)) = listener.accept() {
            let _ = plain.set_nodelay(true);
            pump(StreamOwned::new(conn, socket), plain);
        }
    });
    Ok(local)
}
