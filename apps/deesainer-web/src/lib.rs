#[cfg(target_arch = "wasm32")]
mod room;

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    struct Shell {
        app: deesainer_ui::App,
    }

    impl eframe::App for Shell {
        fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
            if let Some(state) = frame.wgpu_render_state() {
                self.app.gpu = "wgpu";
                if !self.app.has_gpu_compositor() {
                    deesainer_ui::attach_wgpu(&mut self.app, state, 1);
                }
            }
            self.app.ui(ui);
        }
    }

    async fn fetch_bytes(window: &web_sys::Window, url: &str) -> Option<Vec<u8>> {
        let resp = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
            .await
            .ok()?
            .dyn_into::<web_sys::Response>()
            .ok()?;
        if !resp.ok() {
            return None;
        }
        let buf = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().ok()?)
            .await
            .ok()?;
        Some(js_sys::Uint8Array::new(&buf).to_vec())
    }

    async fn load_fonts(window: &web_sys::Window) {
        let Some(list) = fetch_bytes(window, "fonts/index.json").await else {
            return;
        };
        let Ok(names) = serde_json::from_slice::<Vec<String>>(&list) else {
            return;
        };
        for name in names.iter().take(64) {
            if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
                continue;
            }
            if let Some(data) = fetch_bytes(window, &format!("fonts/{name}")).await {
                if data.len() <= 32 * 1024 * 1024 {
                    deesainer_text::add_font(data);
                }
            }
        }
    }

    #[wasm_bindgen(start)]
    pub async fn start() -> Result<(), JsValue> {
        if let Some(window) = web_sys::window() {
            load_fonts(&window).await;
        }
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| JsValue::from_str("no document"))?;
        let canvas = document
            .get_element_by_id("deesainer")
            .ok_or_else(|| JsValue::from_str("missing <canvas id=\"deesainer\">"))?
            .dyn_into::<web_sys::HtmlCanvasElement>()?;
        let session =
            deesainer_engine::Session::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
        let query = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok());
        let param = |k: &str| query.as_ref().and_then(|q| q.get(k));
        let room = match param("join") {
            Some(url) => {
                let name = param("name").unwrap_or_else(|| "Browser".to_owned());
                let token = param("token").unwrap_or_default();
                let branch = param("branch").unwrap_or_default();
                let file = param("file").unwrap_or_default();
                Some(crate::room::WebRoom::connect(
                    &url, &name, &token, &branch, &file,
                )?)
            }
            None => None,
        };
        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |_cc| {
                    let mut app = deesainer_ui::App::new(session);
                    if let Some(room) = room {
                        app.extensions.push(Box::new(room));
                    }
                    Ok(Box::new(Shell { app }))
                }),
            )
            .await
    }
}
