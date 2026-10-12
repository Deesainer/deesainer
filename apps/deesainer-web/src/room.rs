use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};

use deesainer_sync::client::{hello, Incoming, RoomClient};
use deesainer_sync::{ClientMsg, ServerMsg};
use deesainer_ui::{Extension, ExtensionView, RemotePeer};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, WebSocket};

pub struct WebRoom {
    ws: WebSocket,
    core: RoomClient,
    out: Receiver<ClientMsg>,
    open: Rc<Cell<bool>>,
    waiting: Vec<String>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_open: Closure<dyn FnMut()>,
    _on_close: Closure<dyn FnMut()>,
}

impl WebRoom {
    pub fn connect(
        url: &str,
        name: &str,
        token: &str,
        branch: &str,
        file: &str,
    ) -> Result<Self, JsValue> {
        let ws = WebSocket::new(url)?;
        let (out_tx, out_rx) = channel::<ClientMsg>();
        let (in_tx, in_rx) = channel::<Incoming>();
        let open = Rc::new(Cell::new(false));
        let inbox = in_tx.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let Some(text) = e.data().as_string() else {
                return;
            };
            let incoming = match serde_json::from_str::<ServerMsg>(&text) {
                Ok(m) => Incoming::Msg(Box::new(m)),
                Err(err) => Incoming::Disconnected(format!("bad server message: {err}")),
            };
            let _ = inbox.send(incoming);
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        let first = serde_json::to_string(&hello(name, token, branch, file))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let (socket, opened) = (ws.clone(), Rc::clone(&open));
        let on_open = Closure::<dyn FnMut()>::new(move || {
            let _ = socket.send_with_str(&first);
            opened.set(true);
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        let closed = Rc::clone(&open);
        let on_close = Closure::<dyn FnMut()>::new(move || {
            closed.set(false);
            let _ = in_tx.send(Incoming::Disconnected("connection closed".into()));
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        let mut core = RoomClient::new(url, name, token, branch, out_tx, in_rx);
        core.file = file.to_owned();
        Ok(Self {
            ws,
            core,
            out: out_rx,
            open,
            waiting: Vec::new(),
            _on_message: on_message,
            _on_open: on_open,
            _on_close: on_close,
        })
    }

    fn flush(&mut self) {
        while let Ok(msg) = self.out.try_recv() {
            if let Ok(line) = serde_json::to_string(&msg) {
                self.waiting.push(line);
            }
        }
        if self.open.get() {
            for line in self.waiting.drain(..) {
                let _ = self.ws.send_with_str(&line);
            }
        }
    }
}

fn color(hex: &str) -> egui::Color32 {
    let [r, g, b, a] = deesainer_color::Rgba::from_hex(hex)
        .unwrap_or(deesainer_color::Rgba::BLACK)
        .to_rgba8();
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl Extension for WebRoom {
    fn name(&self) -> &str {
        "Multiplayer"
    }

    fn poll(&mut self, view: &mut ExtensionView<'_>) -> bool {
        self.core.chat = view.chat.clone();
        let changed = self.core.poll(view.session, view.cursor);
        self.flush();
        *view.peers = self
            .core
            .peers()
            .into_iter()
            .map(|p| RemotePeer {
                name: p.name,
                color: color(&p.color),
                page: p.presence.page,
                cursor: p.presence.cursor,
                selection: p.presence.selection,
                chat: p.presence.chat,
            })
            .collect();
        changed
    }

    fn ui(&mut self, ui: &mut egui::Ui, _session: &mut deesainer_engine::Session) {
        ui.label(&self.core.status);
        ui.label(format!("You are {}", self.core.name));
        for p in self.core.peers() {
            ui.colored_label(color(&p.color), &p.name);
        }
    }
}
