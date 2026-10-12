use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use deesainer_engine::Session;
use deesainer_sync::client::{Incoming, RoomClient};
use deesainer_sync::{ClientMsg, ServerMsg};

pub struct SyncClient {
    core: RoomClient,
}

impl std::ops::Deref for SyncClient {
    type Target = RoomClient;
    fn deref(&self) -> &RoomClient {
        &self.core
    }
}

impl std::ops::DerefMut for SyncClient {
    fn deref_mut(&mut self) -> &mut RoomClient {
        &mut self.core
    }
}

fn connect_once(
    addr: &str,
    name: &str,
    token: &str,
    branch: &str,
    file: &str,
    inbox: &Sender<Incoming>,
    out: &Receiver<ClientMsg>,
) -> Result<(), String> {
    let stream = match addr.strip_prefix("tls://") {
        Some(target) => {
            let local = crate::tls::forward(target)?;
            TcpStream::connect(local).map_err(|e| e.to_string())?
        }
        None => TcpStream::connect(addr).map_err(|e| e.to_string())?,
    };
    let _ = stream.set_nodelay(true);
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let hello = deesainer_sync::client::hello(name, token, branch, file);
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&hello).map_err(|e| e.to_string())?
    )
    .map_err(|e| e.to_string())?;
    let reader_inbox = inbox.clone();
    let reader = std::thread::spawn(move || {
        let r = BufReader::new(stream);
        for line in r.lines() {
            let Ok(line) = line else { break };
            match serde_json::from_str::<ServerMsg>(&line) {
                Ok(m) => {
                    if reader_inbox.send(Incoming::Msg(Box::new(m))).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = reader_inbox
                        .send(Incoming::Disconnected(format!("bad server message: {e}")));
                    break;
                }
            }
        }
    });
    loop {
        if reader.is_finished() {
            return Err("connection closed".into());
        }
        match out.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => {
                let line = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
                writeln!(writer, "{line}").map_err(|e| e.to_string())?;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

impl SyncClient {
    pub fn connect(addr: &str, name: &str, token: &str) -> Self {
        Self::connect_branch(addr, name, token, "")
    }

    pub fn connect_branch(addr: &str, name: &str, token: &str, branch: &str) -> Self {
        Self::connect_file(addr, name, token, branch, "")
    }

    pub fn connect_file(addr: &str, name: &str, token: &str, branch: &str, file: &str) -> Self {
        let (out_tx, out_rx) = channel::<ClientMsg>();
        let (in_tx, in_rx) = channel::<Incoming>();
        let (a, n, t, b, f) = (
            addr.to_owned(),
            name.to_owned(),
            token.to_owned(),
            branch.to_owned(),
            file.to_owned(),
        );
        std::thread::spawn(move || loop {
            let result = connect_once(&a, &n, &t, &b, &f, &in_tx, &out_rx);
            let reason = match result {
                Ok(()) => return,
                Err(e) => e,
            };
            if in_tx.send(Incoming::Disconnected(reason)).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(2));
        });
        let mut core = RoomClient::new(addr, name, token, branch, out_tx, in_rx);
        core.file = file.to_owned();
        Self { core }
    }

    pub fn on_branch(&self, branch: &str) -> Self {
        Self::connect_file(&self.addr, &self.name, &self.token, branch, &self.file)
    }

    pub fn poll(&mut self, session: &mut Session, cursor: Option<deesainer_geom::Point>) -> bool {
        self.core.poll(session, cursor)
    }
}
