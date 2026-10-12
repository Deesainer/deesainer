use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use base64::Engine as _;
use deesainer_automation::control_server::ControlServer;
use deesainer_automation::{control, files};
use deesainer_ui::{Extension, ExtensionView};
use serde_json::{json, Value};

use crate::FileState;

pub struct ControlExtension {
    pub server: ControlServer,
    pub read_root: Option<PathBuf>,
    pub write_root: Option<PathBuf>,
    pub file: Rc<RefCell<FileState>>,
    pub pending_shots: Vec<(Value, std::sync::mpsc::Sender<String>)>,
}

fn window_png(image: &egui::ColorImage) -> Result<Value, String> {
    let [w, h] = image.size;
    let mut pix = deesainer_render::Pixmap::new(w as u32, h as u32)
        .ok_or_else(|| "empty window".to_owned())?;
    for (dst, px) in pix
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(&image.pixels)
    {
        dst.copy_from_slice(&px.to_array());
    }
    let png = deesainer_render::encode_png(&pix).map_err(|e| e.to_string())?;
    Ok(
        json!({"mimeType": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(png), "width": w, "height": h}),
    )
}

fn path_param(params: &Value) -> Result<&str, String> {
    params["path"]
        .as_str()
        .ok_or_else(|| "missing path".to_owned())
}

impl ControlExtension {
    fn root(&self, write: bool) -> Result<&PathBuf, String> {
        let r = if write {
            &self.write_root
        } else {
            &self.read_root
        };
        r.as_ref().ok_or_else(|| {
            let flag = if write {
                "--automation-write-root"
            } else {
                "--automation-read-root"
            };
            format!("file access is disabled; start the app with {flag} DIR")
        })
    }

    fn handle(
        &mut self,
        view: &mut ExtensionView<'_>,
        method: &str,
        params: &Value,
    ) -> Result<Value, String> {
        match method {
            "ui.state" => Ok(json!({
                "page": view.session.current_page(),
                "selection": view.session.selection(),
                "zoom": view.camera.zoom,
                "tool": view.tool,
                "file": self.file.borrow().path.as_ref().map(|p| p.display().to_string()),
                "revision": view.session.revision(),
                "instanceLayer": view.instance_layer.map(|(i, l)| json!({"instance": i, "layer": l})),
                "editing": view.editing,
                "renderer": {
                    "gpu": view.gpu,
                    "canvas": view.canvas_renderer,
                    "meshFallback": view.mesh_fallback,
                    "flattenedLayers": view.flattened_layers,
                },
            })),
            "ui.action" => {
                let action = params["action"]
                    .as_str()
                    .ok_or("ui.action needs an action, e.g. toggle:inspect")?;
                view.ui_actions.push(action.to_owned());
                Ok(json!({}))
            }
            "ui.pointer" => {
                let steps = params["events"]
                    .as_array()
                    .ok_or("ui.pointer needs events: [{type: down|move|up, x, y}]")?;
                if steps.len() > 1000 {
                    return Err("ui.pointer takes at most 1000 events".into());
                }
                let mut parsed = Vec::with_capacity(steps.len());
                for e in steps {
                    let kind = match e["type"].as_str() {
                        Some("down") => deesainer_ui::PointerKind::Down,
                        Some("move") => deesainer_ui::PointerKind::Move,
                        Some("up") => deesainer_ui::PointerKind::Up,
                        Some("dblclick") => deesainer_ui::PointerKind::DoubleClick,
                        Some("wheel") => {
                            let d = |k: &str| {
                                e[k].as_f64()
                                    .filter(|v| v.is_finite())
                                    .unwrap_or(0.0)
                                    .clamp(-1e5, 1e5) as f32
                            };
                            deesainer_ui::PointerKind::Wheel(d("dx"), d("dy"))
                        }
                        _ => {
                            return Err(
                                "each event needs type down, move, up, dblclick or wheel".into()
                            )
                        }
                    };
                    let (Some(x), Some(y)) = (e["x"].as_f64(), e["y"].as_f64()) else {
                        return Err("each event needs page coordinates x and y".into());
                    };
                    if !(x.is_finite() && y.is_finite() && x.abs() < 1e7 && y.abs() < 1e7) {
                        return Err("x and y must be finite page coordinates".into());
                    }
                    let flag = |k: &str| e[k].as_bool().unwrap_or(false);
                    let modifiers = egui::Modifiers {
                        alt: flag("alt"),
                        shift: flag("shift"),
                        ctrl: flag("ctrl"),
                        command: flag("command"),
                        mac_cmd: cfg!(target_os = "macos") && flag("command"),
                    };
                    parsed.push(deesainer_ui::PointerStep {
                        kind,
                        at: deesainer_geom::Point::new(x, y),
                        screen: e["space"].as_str() == Some("screen"),
                        modifiers,
                    });
                }
                view.pointer.extend(parsed);
                Ok(json!({"queued": steps.len()}))
            }
            "ui.keys" => {
                let text = params["text"].as_str().unwrap_or("");
                let keys = params["keys"].as_array().cloned().unwrap_or_default();
                if text.chars().count() + keys.len() > 1000 {
                    return Err("ui.keys takes at most 1000 characters and keys".into());
                }
                let flag = |k: &str| params[k].as_bool().unwrap_or(false);
                let modifiers = egui::Modifiers {
                    alt: flag("alt"),
                    shift: flag("shift"),
                    ctrl: flag("ctrl"),
                    command: flag("command"),
                    mac_cmd: cfg!(target_os = "macos") && flag("command"),
                };
                let mut steps = Vec::new();
                for k in &keys {
                    let name = k.as_str().unwrap_or("");
                    let key = egui::Key::from_name(name)
                        .ok_or_else(|| format!("unknown key {name:?}"))?;
                    steps.push(deesainer_ui::PointerKind::Key(key));
                }
                let typed: Vec<_> = text.chars().map(deesainer_ui::PointerKind::Char).collect();
                let n = typed.len() + steps.len();
                view.pointer
                    .extend(
                        typed
                            .into_iter()
                            .chain(steps)
                            .map(|kind| deesainer_ui::PointerStep {
                                kind,
                                at: deesainer_geom::Point::new(0.0, 0.0),
                                screen: true,
                                modifiers,
                            }),
                    );
                Ok(json!({"queued": n}))
            }
            "ui.screenshot" => {
                let scale = params["scale"].as_f64().unwrap_or(1.0).clamp(0.1, 4.0);
                let (w, h) = (
                    (f64::from(view.canvas_size.x) * scale).round().max(1.0) as u32,
                    (f64::from(view.canvas_size.y) * scale).round().max(1.0) as u32,
                );
                let vp = deesainer_render::Viewport {
                    transform: deesainer_geom::Affine::scale(scale, scale)
                        .then(deesainer_geom::Affine::translate(
                            f64::from(view.camera.pan.x),
                            f64::from(view.camera.pan.y),
                        ))
                        .then(deesainer_geom::Affine::scale(
                            view.camera.zoom,
                            view.camera.zoom,
                        )),
                    width: w,
                    height: h,
                };
                let opts = deesainer_render::Options {
                    background: true,
                    text: Some(deesainer_engine::text_painter()),
                    hidden: &[],
                };
                let (pix, _) = deesainer_render::render_page(
                    view.session.doc(),
                    view.session.current_page(),
                    &vp,
                    opts,
                )
                .map_err(|e| e.to_string())?;
                let png = deesainer_render::encode_png(&pix).map_err(|e| e.to_string())?;
                Ok(
                    json!({"mimeType": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(png), "width": w, "height": h}),
                )
            }
            "app.open" => {
                let path = files::resolve_in_root(self.root(false)?, path_param(params)?, false)?;
                let doc = files::load_document(&path)?;
                view.session.replace_document(doc, false);
                let mut f = self.file.borrow_mut();
                if files::is_foreign(&path) {
                    f.path = None;
                    f.saved_revision = u64::MAX;
                } else {
                    f.path = Some(path);
                    f.saved_revision = view.session.revision();
                }
                Ok(json!({ "opened": true }))
            }
            "app.save" => {
                let path = files::resolve_in_root(self.root(true)?, path_param(params)?, true)?;
                files::save_document(view.session.doc(), &path)?;
                Ok(json!({ "saved": true }))
            }
            "app.export" => {
                let rel = path_param(params)?;
                let path = files::resolve_in_root(self.root(true)?, rel, true)?;
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png")
                    .to_lowercase();
                let image_cmd = match ext.as_str() {
                    "jpg" | "jpeg" => "export.jpeg",
                    "webp" => "export.webp",
                    _ => "export.png",
                };
                let mut p = json!({});
                if let Some(id) = params.get("id").filter(|v| !v.is_null()) {
                    p["id"] = id.clone();
                }
                let bytes = match ext.as_str() {
                    "svg" => view
                        .session
                        .dispatch("export.svg", &p)
                        .map_err(|e| e.to_string())?["svg"]
                        .as_str()
                        .unwrap_or("")
                        .as_bytes()
                        .to_vec(),
                    "pdf" => {
                        let ids = p
                            .get("id")
                            .map(|id| json!({ "ids": [id] }))
                            .unwrap_or(json!({}));
                        let v = view
                            .session
                            .dispatch("export.pdf", &ids)
                            .map_err(|e| e.to_string())?;
                        base64::engine::general_purpose::STANDARD
                            .decode(v["data"].as_str().unwrap_or(""))
                            .map_err(|e| e.to_string())?
                    }
                    _ => {
                        p["scale"] = json!(params["scale"].as_f64().unwrap_or(1.0));
                        let v = view
                            .session
                            .dispatch(image_cmd, &p)
                            .map_err(|e| e.to_string())?;
                        base64::engine::general_purpose::STANDARD
                            .decode(v["data"].as_str().unwrap_or(""))
                            .map_err(|e| e.to_string())?
                    }
                };
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(json!({ "exported": rel }))
            }
            other => Err(format!("unknown method {other:?}")),
        }
    }
}

impl Extension for ControlExtension {
    fn name(&self) -> &str {
        "Control server"
    }

    fn poll(&mut self, view: &mut ExtensionView<'_>) -> bool {
        let mut changed = false;
        if let Some(image) = view.window.image.take() {
            let reply = |id: &Value| match window_png(&image) {
                Ok(v) => json!({"id": id, "result": v}),
                Err(e) => json!({"id": id, "error": {"message": e}}),
            };
            for (id, tx) in self.pending_shots.drain(..) {
                let _ = tx.send(reply(&id).to_string());
            }
        }
        while let Ok(req) = self.server.requests.try_recv() {
            let msg: Value = serde_json::from_str(&req.line).unwrap_or(Value::Null);
            let method = msg["method"].as_str().unwrap_or("");
            if method == "ui.windowScreenshot" {
                self.pending_shots.push((msg["id"].clone(), req.reply));
                view.window.requested = true;
                continue;
            }
            let reply = if method.starts_with("ui.") || method.starts_with("app.") {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                match self.handle(view, method, &params) {
                    Ok(result) => json!({"id": msg["id"], "result": result}),
                    Err(e) => json!({"id": msg["id"], "error": {"message": e}}),
                }
            } else {
                control::handle_line(view.session, &req.line)
            };
            let _ = req.reply.send(reply.to_string());
            changed = true;
        }
        changed
    }

    fn ui(&mut self, ui: &mut egui::Ui, _session: &mut deesainer_engine::Session) {
        ui.label(format!("Listening on {}", self.server.addr));
        ui.label(format!(
            "Read root: {}",
            self.read_root
                .as_ref()
                .map_or("disabled".into(), |p| p.display().to_string())
        ));
        ui.label(format!(
            "Write root: {}",
            self.write_root
                .as_ref()
                .map_or("disabled".into(), |p| p.display().to_string())
        ));
    }
}
