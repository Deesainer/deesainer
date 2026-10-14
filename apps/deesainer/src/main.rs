#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assistant;
mod control;
mod recovery;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use eframe::egui;
#[cfg(windows)]
use std::process::Stdio;
use std::process::{Child, Command};

use deesainer_automation::files;
use deesainer_automation::sync_client::SyncClient;
use deesainer_automation::team_client::{TeamClient, TeamFile};
use deesainer_engine::Session;
use deesainer_ui::{App, Extension, ExtensionView, RemotePeer};
use serde_json::{json, Value};

macro_rules! note {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

#[derive(Default)]
struct FileState {
    path: Option<PathBuf>,
    saved_revision: u64,
    collaborate: bool,
    figma_import: bool,
}

type FigmaResult = Result<(deesainer_doc::Document, String, usize), String>;

struct FigmaImport {
    link: String,
    token: String,
    remember: bool,
    error: String,
    running: Option<std::sync::mpsc::Receiver<FigmaResult>>,
}

struct SyncExtension {
    client: SyncClient,
    version_label: String,
    branch_name: String,
    review: Option<(egui::TextureHandle, u64)>,
}

fn review_texture(ctx: &egui::Context, pix: &deesainer_render::Pixmap) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_premultiplied(
        [pix.width() as usize, pix.height() as usize],
        pix.data(),
    );
    ctx.load_texture("merge-review", image, egui::TextureOptions::LINEAR)
}

fn ago(at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    match now.saturating_sub(at) {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

fn hex_color(s: &str) -> egui::Color32 {
    let [r, g, b, a] = deesainer_color::Rgba::from_hex(s)
        .unwrap_or(deesainer_color::Rgba::BLACK)
        .to_rgba8();
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl Extension for SyncExtension {
    fn name(&self) -> &str {
        "Multiplayer"
    }

    fn poll(&mut self, view: &mut ExtensionView<'_>) -> bool {
        self.client.chat = view.chat.clone();
        let changed = self.client.poll(view.session, view.cursor);
        *view.peers = self
            .client
            .peers()
            .into_iter()
            .map(|p| RemotePeer {
                name: p.name,
                color: hex_color(&p.color),
                page: p.presence.page,
                cursor: p.presence.cursor,
                selection: p.presence.selection,
                chat: p.presence.chat,
            })
            .collect();
        changed
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        ui.label(&self.client.status);
        ui.label(format!("You are {}", self.client.name));
        let pending = self.client.pending();
        if pending > 0 {
            ui.label(format!("{pending} change(s) waiting to sync"));
        }
        for p in self.client.peers() {
            ui.colored_label(hex_color(&p.color), &p.name);
        }
        ui.separator();
        ui.label(egui::RichText::new("Version history").strong());
        if self.client.is_read_only() {
            ui.label("View only: you can look at the list but not change it.");
        } else {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.version_label)
                        .hint_text("Name this version")
                        .desired_width(160.0),
                );
                if ui.button("Save version").clicked() {
                    self.client.save_version(&self.version_label);
                    self.version_label.clear();
                }
            });
        }
        if self.client.versions.is_empty() {
            ui.label(egui::RichText::new("No saved versions yet.").weak());
        }
        let read_only = self.client.is_read_only();
        let mut restore = None;
        for v in &self.client.versions {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(&v.label);
                    ui.label(
                        egui::RichText::new(format!("{} · {}", v.author, ago(v.at)))
                            .small()
                            .weak(),
                    );
                });
                if !read_only {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Restore")
                            .on_hover_text("Replaces the shared document for everyone; the current state is saved first")
                            .clicked()
                        {
                            restore = Some(v.id);
                        }
                    });
                }
            });
        }
        if let Some(id) = restore {
            self.client.restore_version(id);
        }
        ui.separator();
        ui.label(egui::RichText::new("Branches").strong());
        let current = self.client.branch.clone();
        ui.label(if current.is_empty() {
            "You are on the main document.".to_owned()
        } else {
            format!("You are on branch \"{current}\".")
        });
        if !read_only {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.branch_name)
                        .hint_text("Branch name")
                        .desired_width(160.0),
                );
                if ui
                    .button("Start branch")
                    .on_hover_text("A copy of the main document you can change on its own")
                    .clicked()
                {
                    self.client.create_branch(&self.branch_name);
                    self.branch_name.clear();
                }
            });
        }
        let mut switch: Option<String> = None;
        if !current.is_empty() && ui.button("Back to main").clicked() {
            switch = Some(String::new());
        }
        let mut merge: Option<(String, bool)> = None;
        let mut preview = None;
        let mut delete = None;
        for b in &self.client.branches {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(&b.name);
                    ui.label(
                        egui::RichText::new(format!("{} · {}", b.author, ago(b.at)))
                            .small()
                            .weak(),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !read_only {
                        if ui.button("Delete").clicked() {
                            delete = Some(b.name.clone());
                        }
                        if ui
                            .button("Merge")
                            .on_hover_text("Brings this branch's changes into the main document; clashes keep main's value")
                            .clicked()
                        {
                            merge = Some((b.name.clone(), false));
                        }
                        if ui
                            .button("Review")
                            .on_hover_text("Lists what a merge would change and what clashes")
                            .clicked()
                        {
                            preview = Some(b.name.clone());
                        }
                    }
                    if b.name != current && ui.button("Open").clicked() {
                        switch = Some(b.name.clone());
                    }
                });
            });
        }
        if self.client.branches.is_empty() {
            ui.label(egui::RichText::new("No branches yet.").weak());
        }
        if let Some((name, applied, conflicts)) = &self.client.last_merge {
            ui.label(format!(
                "Merged \"{name}\": {applied} change(s), {} conflict(s).",
                conflicts.len()
            ));
            for c in conflicts.iter().take(5) {
                ui.label(egui::RichText::new(c).small().weak());
            }
        }
        if let Some(name) = preview {
            self.client.preview_merge(&name);
        }
        if let Some((branch, changes, conflicts)) = self.client.merge_preview.clone() {
            let mut dismiss = false;
            ui.group(|ui| {
                ui.label(egui::RichText::new(format!("Review of \"{branch}\"")).strong());
                if changes.is_empty() && conflicts.is_empty() {
                    ui.label("Nothing to merge.");
                }
                for line in &changes {
                    ui.label(line);
                }
                for c in conflicts.iter().take(8) {
                    ui.label(egui::RichText::new(format!("Clash: {c}")).small());
                }
                if self.review.is_none() {
                    if let Some(merged) = &self.client.merge_preview_doc {
                        if let Ok(c) =
                            deesainer_engine::compare::documents(session.doc(), merged, 0, 0.5)
                        {
                            self.review = Some((review_texture(ui.ctx(), &c.diff), c.changed));
                        }
                    }
                }
                if let Some((tex, changed)) = &self.review {
                    let size = tex.size_vec2();
                    let w = ui.available_width().min(size.x).max(1.0);
                    ui.image((tex.id(), egui::vec2(w, size.y * w / size.x.max(1.0))));
                    ui.label(
                        egui::RichText::new(format!(
                            "{changed} pixel(s) change on the first page (in magenta)."
                        ))
                        .small(),
                    );
                }
                if conflicts.len() > 8 {
                    ui.label(format!("and {} more clash(es)", conflicts.len() - 8));
                }
                ui.horizontal(|ui| {
                    if !read_only && ui.button("Merge, keep main on clashes").clicked() {
                        merge = Some((branch.clone(), false));
                        dismiss = true;
                    }
                    if !read_only
                        && !conflicts.is_empty()
                        && ui.button("Merge, prefer the branch").clicked()
                    {
                        merge = Some((branch.clone(), true));
                        dismiss = true;
                    }
                    if ui.button("Close").clicked() {
                        dismiss = true;
                    }
                });
            });
            if dismiss {
                self.client.merge_preview = None;
                self.client.merge_preview_doc = None;
                self.review = None;
            }
        }
        if let Some((name, prefer_branch)) = merge {
            self.client.merge_branch(&name, prefer_branch);
        }
        if let Some(name) = delete {
            self.client.delete_branch(&name);
            if name == current {
                switch = Some(String::new());
            }
        }
        if let Some(branch) = switch {
            self.client = self.client.on_branch(&branch);
        }
    }
}

struct Collab {
    addr: String,
    name: String,
    token: String,
    relay: Option<Child>,
    error: String,
    server: String,
    email: String,
    password: String,
    team: Option<TeamClient>,
    team_files: Vec<TeamFile>,
    team_error: String,
    team_resumed: bool,
}

fn settings_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".deesainer").join("settings.json"))
}

fn load_language() -> deesainer_ui::Language {
    let Some(path) = settings_path() else {
        return deesainer_ui::Language::default();
    };
    let Ok(body) = std::fs::read_to_string(&path) else {
        return deesainer_ui::Language::default();
    };
    serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("language")?.as_str().map(str::to_owned))
        .and_then(|s| deesainer_ui::Language::from_code(&s))
        .unwrap_or_default()
}

fn save_language(language: deesainer_ui::Language) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = serde_json::json!({ "language": language.as_str() }).to_string();
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    if let Ok(mut f) = opts.open(&path) {
        use std::io::Write as _;
        let _ = f.write_all(body.as_bytes());
    }
}

fn team_session_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".deesainer").join("team.json"))
}

fn remember_team_session(base: &str, token: &str) {
    let Some(path) = team_session_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = serde_json::json!({ "server": base, "token": token }).to_string();
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    if let Ok(mut f) = opts.open(&path) {
        use std::io::Write as _;
        let _ = f.write_all(body.as_bytes());
    }
}

fn forget_team_session() {
    if let Some(path) = team_session_path() {
        let _ = std::fs::remove_file(path);
    }
}

struct Shell {
    app: App,
    file: Rc<RefCell<FileState>>,
    figma: FigmaImport,
    title: String,
    last_recovery: Instant,
    recovery_revision: u64,
    recovery: Option<recovery::Recovery>,
    pending_recovery: Option<recovery::Pending>,
    snapshot_written: bool,
    last_language: deesainer_ui::Language,
    cjk_fonts: bool,
    collab: Collab,
}

impl Drop for Shell {
    fn drop(&mut self) {
        deesainer_automation::control_server::clear_pointer();
        if let Some(mut relay) = self.collab.relay.take() {
            let _ = relay.kill();
        }
    }
}

impl Shell {
    fn figma_window(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.figma.running {
            match rx.try_recv() {
                Ok(Ok((doc, name, layers))) => {
                    self.app.session.replace_document(doc, false);
                    let mut s = self.file.borrow_mut();
                    s.path = None;
                    s.saved_revision = u64::MAX;
                    s.figma_import = false;
                    drop(s);
                    self.app
                        .info(format!("Imported \"{name}\" from Figma ({layers} layers)"));
                    self.figma.running = None;
                }
                Ok(Err(e)) => {
                    self.figma.error = e;
                    self.figma.running = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.figma.error = "The download stopped unexpectedly.".into();
                    self.figma.running = None;
                }
            }
        }
        let mut open = self.file.borrow().figma_import;
        if !open {
            return;
        }
        egui::Window::new("Import from Figma")
            .open(&mut open)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.label("Paste the link of a Figma design file you can access.");
                ui.add_space(4.0);
                egui::Grid::new("figma-import").num_columns(2).show(ui, |ui| {
                    ui.label("File link");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.figma.link)
                            .hint_text("https://www.figma.com/design/…")
                            .desired_width(260.0),
                    );
                    ui.end_row();
                    ui.label("Access token");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.figma.token)
                            .password(true)
                            .hint_text("figd_…")
                            .desired_width(260.0),
                    );
                    ui.end_row();
                });
                ui.checkbox(&mut self.figma.remember, "Remember the token on this computer");
                ui.weak("Create a personal access token in Figma: Settings › Security › Personal access tokens (file content: read).");
                if !self.figma.error.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(220, 60, 60), &self.figma.error);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let busy = self.figma.running.is_some();
                    let ready = !busy && !self.figma.link.trim().is_empty() && !self.figma.token.trim().is_empty();
                    if ui.add_enabled(ready, egui::Button::new("Import")).clicked() {
                        self.figma.error.clear();
                        if self.figma.remember {
                            if let Err(e) = deesainer_automation::figma_api::save_token(&self.figma.token) {
                                self.figma.error = format!("Could not save the token: {e}");
                            }
                        }
                        let (tx, rx) = std::sync::mpsc::channel();
                        let link = self.figma.link.clone();
                        let token = self.figma.token.clone();
                        std::thread::spawn(move || {
                            let r = deesainer_automation::figma_api::import(&link, &token)
                                .map(|(doc, report, name)| (doc, name, report.layers));
                            let _ = tx.send(r);
                        });
                        self.figma.running = Some(rx);
                    }
                    if busy {
                        ui.spinner();
                        ui.label("Downloading…");
                    }
                    if deesainer_automation::figma_api::token().is_some()
                        && ui.small_button("Forget saved token").clicked()
                    {
                        deesainer_automation::figma_api::clear_token();
                        self.figma.token.clear();
                    }
                });
            });
        self.file.borrow_mut().figma_import = open;
    }

    fn collaborate_window(&mut self, ctx: &egui::Context) {
        let open = self.file.borrow().collaborate;
        if !open {
            return;
        }
        let connected = self
            .app
            .extensions
            .iter()
            .any(|e| e.name() == "Multiplayer");
        let mut close = false;
        let frame = egui::Frame::new()
            .fill(egui::Color32::WHITE)
            .corner_radius(12)
            .inner_margin(egui::Margin::same(16))
            .shadow(egui::Shadow {
                offset: [0, 8],
                blur: 32,
                spread: 0,
                color: egui::Color32::from_black_alpha(50),
            });
        let modal = egui::Modal::new(egui::Id::new("share"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(60))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Share this file").strong().size(13.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new("×").size(16.0)).frame(false)).clicked() {
                            close = true;
                        }
                        let link = format!("deesainer://{}", self.collab.addr);
                        if ui
                            .add(egui::Button::new(egui::RichText::new("Copy link").color(egui::Color32::from_rgb(13, 153, 255))).frame(false))
                            .on_hover_text(&link)
                            .clicked()
                        {
                            ui.ctx().copy_text(self.collab.addr.clone());
                        }
                    });
                });
                ui.separator();
                if connected {
                    ui.label("Live session running.");
                } else {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(229, 244, 255))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.label("Host this file on your computer, or join a session someone else is hosting. Hosting listens on this computer only.");
                        });
                    ui.add_space(8.0);
                    egui::Grid::new("share-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Your name");
                        ui.add(egui::TextEdit::singleline(&mut self.collab.name).desired_width(280.0));
                        ui.end_row();
                        ui.label("Address");
                        ui.add(egui::TextEdit::singleline(&mut self.collab.addr).desired_width(280.0));
                        ui.end_row();
                        ui.label("Token");
                        ui.add(egui::TextEdit::singleline(&mut self.collab.token).password(true).desired_width(280.0));
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let host = egui::Button::new(egui::RichText::new("Host this file").color(egui::Color32::WHITE))
                            .fill(egui::Color32::from_rgb(13, 153, 255));
                        if ui.add(host).on_hover_text("Starts a relay for the saved file and joins it").clicked() {
                            self.host();
                        }
                        if ui.button("Join").clicked() {
                            self.join();
                        }
                    });
                    if !self.collab.error.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(220, 60, 60), &self.collab.error);
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    self.team_section(ui);
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Who has access").color(egui::Color32::from_gray(110)));
                ui.horizontal(|ui| {
                    ui.label(format!("{} (you)", self.collab.name));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("owner").color(egui::Color32::from_gray(110)));
                    });
                });
                for peer in &self.app.peers {
                    ui.horizontal(|ui| {
                        ui.colored_label(peer.color, "●");
                        ui.label(&peer.name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("can edit").color(egui::Color32::from_gray(110)));
                        });
                    });
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Agents can join with: deesainer-cli mcp --join ADDRESS").color(egui::Color32::from_gray(110)).small());
            });
        if close || modal.should_close() {
            self.file.borrow_mut().collaborate = false;
        }
    }

    fn host(&mut self) {
        let path = self.file.borrow().path.clone();
        let Some(path) = path else {
            self.collab.error = "Save the file first (File > Save as…).".into();
            return;
        };
        if let Err(e) = files::save_document(self.app.session.doc(), &path) {
            self.collab.error = e;
            return;
        }
        let relay_bin = std::env::current_exe().ok().and_then(|p| {
            p.parent().map(|d| {
                d.join(if cfg!(windows) {
                    "deesainer-relay.exe"
                } else {
                    "deesainer-relay"
                })
            })
        });
        let Some(bin) = relay_bin.filter(|b| b.exists()) else {
            self.collab.error = "deesainer-relay was not found next to the app.".into();
            return;
        };
        let mut cmd = Command::new(bin);
        cmd.args([
            "--file",
            &path.to_string_lossy(),
            "--bind",
            &self.collab.addr,
        ]);
        #[cfg(windows)]
        if !cfg!(debug_assertions) {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        match cmd.spawn() {
            Ok(child) => {
                self.collab.relay = Some(child);
                std::thread::sleep(Duration::from_millis(300));
                self.join();
            }
            Err(e) => self.collab.error = format!("cannot start the relay: {e}"),
        }
    }

    fn team_section(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Team server").strong());
        if !self.collab.team_resumed {
            self.collab.team_resumed = true;
            let saved = team_session_path()
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            if let Some(v) = saved {
                let (base, token) = (
                    v["server"].as_str().unwrap_or(""),
                    v["token"].as_str().unwrap_or(""),
                );
                if let Ok(client) = TeamClient::resume(base, token) {
                    self.collab.server = base.to_owned();
                    self.collab.team_files = client.files().unwrap_or_default();
                    self.collab.team = Some(client);
                }
            }
        }
        let mut open: Option<u64> = None;
        match &self.collab.team {
            None => {
                egui::Grid::new("team-grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Server");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.collab.server)
                                .desired_width(280.0),
                        );
                        ui.end_row();
                        ui.label("Email");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.collab.email).desired_width(280.0),
                        );
                        ui.end_row();
                        ui.label("Password");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.collab.password)
                                .password(true)
                                .desired_width(280.0),
                        );
                        ui.end_row();
                    });
                if ui.button("Sign in").clicked() {
                    match TeamClient::login(
                        &self.collab.server,
                        &self.collab.email,
                        &self.collab.password,
                    ) {
                        Ok(client) => {
                            remember_team_session(&client.base, &client.token);
                            self.collab.password.clear();
                            self.collab.team_files = client.files().unwrap_or_default();
                            self.collab.team = Some(client);
                            self.collab.team_error.clear();
                        }
                        Err(e) => self.collab.team_error = e,
                    }
                }
            }
            Some(client) => {
                ui.horizontal(|ui| {
                    ui.label(format!("{} on {}", client.name, client.base));
                    if ui.small_button("Refresh").clicked() {
                        match client.files() {
                            Ok(f) => self.collab.team_files = f,
                            Err(e) => self.collab.team_error = e,
                        }
                    }
                });
                if self.collab.team_files.is_empty() {
                    ui.label(
                        egui::RichText::new("No files yet.").color(egui::Color32::from_gray(110)),
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for f in &self.collab.team_files {
                            ui.horizontal(|ui| {
                                ui.label(&f.name);
                                ui.label(
                                    egui::RichText::new(&f.role)
                                        .color(egui::Color32::from_gray(110)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Open").clicked() {
                                            open = Some(f.id);
                                        }
                                    },
                                );
                            });
                        }
                    });
                let mut sign_out = false;
                ui.horizontal(|ui| {
                    if ui
                        .button("Upload this file")
                        .on_hover_text(
                            "Puts the open document on the server as a new file and opens it there",
                        )
                        .clicked()
                    {
                        let name = self
                            .file
                            .borrow()
                            .path
                            .as_ref()
                            .and_then(|p| p.file_stem())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let bytes = deesainer_format::write(self.app.session.doc(), None);
                        match bytes.map_err(|e| e.to_string()).and_then(|b| {
                            client
                                .create(if name.is_empty() { "Untitled" } else { &name }, Some(&b))
                        }) {
                            Ok(id) => {
                                self.collab.team_files = client.files().unwrap_or_default();
                                open = Some(id);
                            }
                            Err(e) => self.collab.team_error = e,
                        }
                    }
                    if ui.button("Sign out").clicked() {
                        sign_out = true;
                    }
                });
                if sign_out {
                    let _ = client.logout();
                    forget_team_session();
                    self.collab.team = None;
                    self.collab.team_files.clear();
                }
            }
        }
        if !self.collab.team_error.is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(220, 60, 60),
                &self.collab.team_error,
            );
        }
        if let Some(id) = open {
            self.open_team_file(id);
        }
    }

    fn open_team_file(&mut self, id: u64) {
        let Some(client) = &self.collab.team else {
            return;
        };
        self.app.session.set_user(&client.name);
        let sync = SyncClient::connect_file(
            &client.room_address(),
            &client.name,
            &client.token,
            "",
            &id.to_string(),
        );
        self.app.extensions.retain(|e| e.name() != "Multiplayer");
        self.app.extensions.push(Box::new(SyncExtension {
            client: sync,
            version_label: String::new(),
            branch_name: String::new(),
            review: None,
        }));
        self.collab.team_error.clear();
        self.file.borrow_mut().collaborate = false;
    }

    fn join(&mut self) {
        self.app.session.set_user(&self.collab.name);
        let client = SyncClient::connect(&self.collab.addr, &self.collab.name, &self.collab.token);
        self.app.extensions.push(Box::new(SyncExtension {
            client,
            version_label: String::new(),
            branch_name: String::new(),
            review: None,
        }));
        self.collab.error.clear();
    }
}

fn recovery_path() -> Option<PathBuf> {
    recovery::Recovery::in_home().map(|r| r.document_path())
}

impl Shell {
    fn dirty(&self) -> bool {
        self.app.session.revision() != self.file.borrow().saved_revision
    }

    fn keep_recovery(&mut self) {
        if self.pending_recovery.is_some() {
            return;
        }
        let Some(r) = &self.recovery else { return };
        let rev = self.app.session.revision();
        if !self.dirty() {
            if self.snapshot_written {
                r.discard();
                self.snapshot_written = false;
            }
            return;
        }
        if self.last_recovery.elapsed() > Duration::from_secs(30) && rev != self.recovery_revision {
            let original = self.file.borrow().path.clone();
            self.snapshot_written |= r
                .snapshot(self.app.session.doc(), original.as_deref())
                .is_ok();
            self.recovery_revision = rev;
            self.last_recovery = Instant::now();
        }
    }

    fn recovery_window(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_recovery.clone() else {
            return;
        };
        let what = pending
            .original
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(
                || "an untitled file".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
        let when = recovery::ago(pending.saved_at, std::time::SystemTime::now());
        let mut choice = None;
        let frame = egui::Frame::new()
            .fill(egui::Color32::WHITE)
            .corner_radius(12)
            .inner_margin(egui::Margin::same(16))
            .shadow(egui::Shadow {
                offset: [0, 8],
                blur: 32,
                spread: 0,
                color: egui::Color32::from_black_alpha(50),
            });
        egui::Modal::new(egui::Id::new("recovery"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(60))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(egui::RichText::new("Recover unsaved changes?").strong().size(13.0));
                ui.add_space(8.0);
                ui.label(format!(
                    "Deesainer closed with unsaved changes to {what}. The last snapshot was taken {when}."
                ));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Discard").clicked() {
                        choice = Some(false);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let recover = egui::Button::new(
                            egui::RichText::new("Recover").color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(13, 153, 255));
                        if ui.add(recover).clicked() {
                            choice = Some(true);
                        }
                    });
                });
            });
        let Some(recover) = choice else { return };
        self.pending_recovery = None;
        let Some(r) = &self.recovery else { return };
        if recover {
            match r.load() {
                Ok(doc) => {
                    self.app.session.replace_document(doc, false);
                    let mut s = self.file.borrow_mut();
                    s.path = None;
                    s.saved_revision = u64::MAX;
                    drop(s);
                    self.app
                        .info(format!("Recovered unsaved changes to {what}"));
                }
                Err(e) => self.app.info(format!("Could not recover: {e}")),
            }
        }
        r.discard();
    }
}

impl eframe::App for Shell {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.app.feed_input(raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.app.language != self.last_language {
            self.last_language = self.app.language;
            save_language(self.app.language);
        }
        if self.app.language.is_cjk() && !self.cjk_fonts {
            self.cjk_fonts = true;
            if let Some(data) = deesainer_text::cjk_font_data() {
                let mut fonts = egui::FontDefinitions::default();
                fonts
                    .font_data
                    .insert("cjk".to_owned(), egui::FontData::from_owned(data).into());
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family)
                        .or_default()
                        .push("cjk".to_owned());
                }
                ui.ctx().set_fonts(fonts);
            }
        }
        self.app.gpu = if let Some(state) = frame.wgpu_render_state() {
            if !self.app.has_gpu_compositor() {
                deesainer_ui::attach_wgpu(&mut self.app, state, MULTISAMPLING.into());
            }
            "wgpu"
        } else if frame.gl().is_some() {
            "gl"
        } else {
            "none"
        };
        self.app.file_name = self
            .file
            .borrow()
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || "Untitled".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
        self.app.ui(ui);
        self.recovery_window(ui.ctx());
        self.collaborate_window(ui.ctx());
        self.figma_window(ui.ctx());
        let rev = self.app.session.revision();
        let title = {
            let state = self.file.borrow();
            let name = state.path.as_ref().and_then(|p| p.file_name()).map_or_else(
                || "Untitled".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            let dirty = if rev != state.saved_revision {
                " •"
            } else {
                ""
            };
            format!("{name}{dirty} - Deesainer")
        };
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        self.keep_recovery();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.pending_recovery.is_some() {
            return;
        }
        let Some(r) = &self.recovery else { return };
        if self.dirty() {
            let original = self.file.borrow().path.clone();
            let _ = r.snapshot(self.app.session.doc(), original.as_deref());
        } else {
            r.discard();
        }
    }
}

fn pick_save(ext: &str, default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(ext.to_uppercase(), &[ext])
        .set_file_name(default_name)
        .save_file()
}

fn file_action(
    state: &Rc<RefCell<FileState>>,
    action: &str,
    session: &mut Session,
) -> Result<String, String> {
    match action {
        "open" => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Deesainer or Figma", &["deesainer", "fig"])
                .add_filter("Deesainer", &["deesainer"])
                .add_filter("Figma (.fig)", &["fig"])
                .pick_file()
            else {
                return Ok(String::new());
            };
            let doc = files::load_document(&path)?;
            session.replace_document(doc, false);
            session.set_versions(files::load_versions(&path).unwrap_or_default());
            let mut s = state.borrow_mut();
            if files::is_foreign(&path) {
                s.path = None;
                s.saved_revision = u64::MAX;
                return Ok(format!("Imported {}", path.display()));
            }
            s.path = Some(path.clone());
            s.saved_revision = session.revision();
            Ok(format!("Opened {}", path.display()))
        }
        "save" | "saveAs" => {
            let existing = state.borrow().path.clone();
            let path = match (action, existing) {
                ("save", Some(p)) => p,
                _ => match pick_save("deesainer", "Untitled.deesainer") {
                    Some(p) => p,
                    None => return Ok(String::new()),
                },
            };
            files::save_session(session, &path)?;
            let mut s = state.borrow_mut();
            s.path = Some(path.clone());
            s.saved_revision = session.revision();
            Ok(format!("Saved {}", path.display()))
        }
        a if a.starts_with("saveAsset:") => {
            let asset = a.trim_start_matches("saveAsset:");
            let reply = session
                .dispatch("inspect.asset", &json!({ "asset": asset }))
                .map_err(|e| e.to_string())?;
            let ext = match reply["mimeType"].as_str() {
                Some("image/jpeg") => "jpg",
                Some("image/webp") => "webp",
                _ => "png",
            };
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(reply["data"].as_str().unwrap_or(""))
                .map_err(|e| e.to_string())?;
            let Some(path) = rfd::FileDialog::new()
                .add_filter(ext.to_uppercase(), &[ext])
                .set_file_name(format!("image.{ext}"))
                .save_file()
            else {
                return Ok(String::new());
            };
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(format!("Saved {}", path.display()))
        }
        a if a.starts_with("exportSettings:") => {
            let id: u64 = a
                .trim_start_matches("exportSettings:")
                .parse()
                .map_err(|_| "export needs a layer id".to_owned())?;
            let reply = session
                .dispatch("export.settings", &json!({ "id": id }))
                .map_err(|e| e.to_string())?;
            let files = reply["files"].as_array().cloned().unwrap_or_default();
            let bytes = |f: &Value| -> Result<Vec<u8>, String> {
                match f["svg"].as_str() {
                    Some(svg) => Ok(svg.as_bytes().to_vec()),
                    None => decode(f),
                }
            };
            let name = |f: &Value| f["fileName"].as_str().unwrap_or("Export").to_owned();
            if let [only] = files.as_slice() {
                let Some(path) = rfd::FileDialog::new().set_file_name(name(only)).save_file()
                else {
                    return Ok(String::new());
                };
                std::fs::write(&path, bytes(only)?).map_err(|e| e.to_string())?;
                return Ok(format!("Exported {}", path.display()));
            }
            let Some(dir) = rfd::FileDialog::new().pick_folder() else {
                return Ok(String::new());
            };
            for f in &files {
                std::fs::write(dir.join(name(f)), bytes(f)?).map_err(|e| e.to_string())?;
            }
            Ok(format!(
                "Exported {} files to {}",
                files.len(),
                dir.display()
            ))
        }
        a if a == "export" || a.starts_with("export:") => {
            let mut parts = a.splitn(4, ':').skip(1);
            let format = parts.next().unwrap_or("png").to_lowercase();
            let scale: f64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(2.0);
            let name = parts.next().filter(|n| !n.is_empty()).unwrap_or("Export");
            let mut dialog = rfd::FileDialog::new();
            for f in std::iter::once(format.as_str()).chain(["png", "jpg", "webp", "svg", "pdf"]) {
                dialog = dialog.add_filter(f.to_uppercase(), &[f]);
            }
            let Some(path) = dialog.set_file_name(format!("{name}.{format}")).save_file() else {
                return Ok(String::new());
            };
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
            let id = session.selection().first().copied().map(|i| json!(i));
            let bytes = match ext.as_str() {
                "svg" => {
                    let reply = session
                        .dispatch("export.svg", &json!({ "id": id }))
                        .map_err(|e| e.to_string())?;
                    reply["svg"].as_str().unwrap_or("").as_bytes().to_vec()
                }
                "pdf" => {
                    let reply = session
                        .dispatch("export.pdf", &Value::Null)
                        .map_err(|e| e.to_string())?;
                    decode(&reply)?
                }
                _ => {
                    let reply = session
                        .dispatch(image_cmd, &json!({ "id": id, "scale": scale }))
                        .map_err(|e| e.to_string())?;
                    decode(&reply)?
                }
            };
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(format!("Exported {}", path.display()))
        }
        "importLibrary" | "updateLibrary" => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Deesainer", &["deesainer"])
                .pick_file()
            else {
                return Ok(String::new());
            };
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Library");
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            if action == "updateLibrary" {
                let m = deesainer_ui::library_review::MARKER;
                return Ok(format!("{m}{name}{m}{data}"));
            }
            let command = "library.import";
            let params = json!({
                "data": data,
                "library": name,
            });
            let reply = session
                .dispatch(command, &params)
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "Imported {} component(s) from {name}",
                reply["imported"].as_u64().unwrap_or(0)
            ))
        }
        "importSvg" => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("SVG", &["svg"])
                .pick_file()
            else {
                return Ok(String::new());
            };
            let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("SVG");
            session
                .dispatch("import.svg", &json!({ "data": data, "name": name }))
                .map_err(|e| e.to_string())?;
            Ok(format!("Imported {}", path.display()))
        }
        "placeImage" => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg"])
                .pick_file()
            else {
                return Ok(String::new());
            };
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Image");
            session
                .dispatch(
                    "image.place",
                    &json!({ "data": base64::engine::general_purpose::STANDARD.encode(bytes), "name": name }),
                )
                .map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        "runPlugin" => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Plugin", &["wasm", "wat", "json"])
                .pick_file()
            else {
                return Ok(String::new());
            };
            run_plugin_file(session, &path, None)
        }
        action if action.starts_with("plugin:") => {
            let rest = &action["plugin:".len()..];
            let (path, entry) = rest.split_once('|').unwrap_or((rest, "run"));
            run_plugin_file(session, std::path::Path::new(path), Some(entry))
        }
        "collaborate" => {
            state.borrow_mut().collaborate = true;
            Ok(String::new())
        }
        "importFigma" => {
            state.borrow_mut().figma_import = true;
            Ok(String::new())
        }
        other => Err(format!("unknown file action {other}")),
    }
}

fn run_plugin_file(
    session: &mut Session,
    path: &std::path::Path,
    entry: Option<&str>,
) -> Result<String, String> {
    let (module_path, entry) = if path.extension().and_then(|e| e.to_str()) == Some("json") {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let manifest: deesainer_plugins::Manifest =
            serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let dir = path.parent().unwrap_or(std::path::Path::new("."));
        let default_entry = manifest
            .commands
            .first()
            .map_or("run".to_owned(), |c| c.entry.clone());
        (
            dir.join(manifest.main),
            entry.map_or(default_entry, str::to_owned),
        )
    } else {
        (path.to_path_buf(), entry.unwrap_or("run").to_owned())
    };
    let bytes =
        std::fs::read(&module_path).map_err(|e| format!("{}: {e}", module_path.display()))?;
    let params = if module_path.extension().and_then(|e| e.to_str()) == Some("wat") {
        json!({ "wat": String::from_utf8_lossy(&bytes), "entry": entry })
    } else {
        json!({ "data": base64::engine::general_purpose::STANDARD.encode(&bytes), "entry": entry })
    };
    let report = session
        .dispatch("plugin.run", &params)
        .map_err(|e| e.to_string())?;
    if report["panel"].is_object() {
        let mut module = params.clone();
        if let Some(o) = module.as_object_mut() {
            o.remove("entry");
        }
        let m = deesainer_ui::plugin_panel::MARKER;
        return Ok(format!("{m}{module}{m}{}", report["panel"]));
    }
    let logs: Vec<String> = report["logs"]
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(format!(
        "Plugin ran ({} commands). {}",
        report["calls"],
        logs.join(" · ")
    ))
}

fn installed_plugins() -> Vec<(String, String)> {
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) else {
        return Vec::new();
    };
    let dir = PathBuf::from(home).join(".deesainer").join("plugins");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let manifest_path = e.path().join("manifest.json");
        let Ok(text) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<deesainer_plugins::Manifest>(&text) else {
            continue;
        };
        for c in &m.commands {
            out.push((
                format!("{}: {}", m.name, c.label),
                format!("plugin:{}|{}", manifest_path.display(), c.entry),
            ));
        }
    }
    out.sort();
    out
}

fn decode(v: &Value) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(v["data"].as_str().unwrap_or(""))
        .map_err(|e| e.to_string())
}

fn demo(session: &mut Session) {
    let steps = [
        (
            "node.create.frame",
            json!({"x": 0, "y": 0, "width": 960, "height": 640, "name": "Landing"}),
        ),
        (
            "node.create.rectangle",
            json!({"parent": 3, "x": 0, "y": 0, "width": 960, "height": 72, "fill": "#14141F", "name": "Header"}),
        ),
        (
            "node.create.text",
            json!({"parent": 3, "x": 64, "y": 160, "content": "Design at the speed of thought", "fontSize": 40, "fontWeight": 700}),
        ),
        (
            "node.create.rectangle",
            json!({"parent": 3, "x": 64, "y": 260, "width": 180, "height": 48, "fill": "#1E90FF", "name": "Button"}),
        ),
        ("node.setProps", json!({"ids": [6], "cornerRadius": 12})),
        (
            "node.create.ellipse",
            json!({"parent": 3, "x": 620, "y": 140, "width": 220, "height": 220, "fill": "#FF7A59", "name": "Sun"}),
        ),
        (
            "node.create.star",
            json!({"parent": 3, "x": 560, "y": 400, "width": 120, "height": 120, "fill": "#FFC93C"}),
        ),
        (
            "node.create.rectangle",
            json!({"parent": 3, "x": 720, "y": 420, "width": 180, "height": 120, "fill": "#FFFFFF", "name": "Card"}),
        ),
        (
            "node.setEffects",
            json!({"ids": [9], "effects": [{"type": "dropShadow", "color": {"r": 0, "g": 0, "b": 0, "a": 0.2}, "offset": {"x": 0, "y": 8}, "blur": 16}]}),
        ),
        ("node.setProps", json!({"ids": [9], "cornerRadius": 16})),
    ];
    for (cmd, params) in steps {
        if let Err(e) = session.dispatch(cmd, &params) {
            note!("demo: {cmd} failed: {e}");
        }
    }
    let _ = session.dispatch("selection.clear", &Value::Null);
}

const MULTISAMPLING: u16 = 4;

type Launch = Rc<RefCell<Option<(Session, Option<control::ControlExtension>)>>>;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let file = Rc::new(RefCell::new(FileState::default()));
    let opened = args
        .iter()
        .enumerate()
        .find(|(i, a)| {
            const VALUED: [&str; 8] = [
                "--join",
                "--branch",
                "--render",
                "--gpu",
                "--control",
                "--control-token-file",
                "--automation-read-root",
                "--automation-write-root",
            ];
            !a.starts_with("--") && (*i == 0 || !VALUED.contains(&args[i - 1].as_str()))
        })
        .map(|(_, a)| a)
        .map(PathBuf::from)
        .or_else(|| {
            args.iter()
                .any(|a| a == "--recover")
                .then(recovery_path)
                .flatten()
        });
    let pending_recovery = if opened.is_none() {
        recovery::Recovery::in_home().and_then(|r| r.pending())
    } else {
        None
    };
    let mut session = match opened.as_deref().map(files::open) {
        Some(Ok(s)) => {
            if args.iter().all(|a| a != "--recover") {
                file.borrow_mut().path = opened.clone();
            }
            s
        }
        Some(Err(e)) => {
            note!("deesainer: {e}");
            std::process::exit(1);
        }
        None => match Session::new() {
            Ok(s) => s,
            Err(e) => {
                note!("deesainer: cannot start the engine: {e}");
                std::process::exit(1);
            }
        },
    };
    session.set_clock(deesainer_automation::now_rfc3339);
    session.set_user(&deesainer_automation::os_user());
    if args.iter().any(|a| a == "--demo") {
        demo(&mut session);
    }
    file.borrow_mut().saved_revision = session.revision();
    let display_p3 = deesainer_gamut::display_p3();
    let options = eframe::NativeOptions {
        viewport: {
            let vp = egui::ViewportBuilder::default()
                .with_title("Deesainer")
                .with_inner_size([1400.0, 900.0]);
            match eframe::icon_data::from_png_bytes(include_bytes!(
                "../../../assets/app-icon/png/256.png"
            )) {
                Ok(icon) => vp.with_icon(icon),
                Err(_) => vp,
            }
        },
        multisampling: MULTISAMPLING,
        ..Default::default()
    };
    let state = Rc::clone(&file);
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let mut control_ext = None;
    if let Some(port) = flag("--control") {
        let port: u16 = port.parse().unwrap_or_else(|_| {
            note!("deesainer: --control needs a port number");
            std::process::exit(2);
        });
        let token_file = flag("--control-token-file").map(PathBuf::from).unwrap_or_else(|| {
            note!("deesainer: --control needs --control-token-file FILE (created with a fresh token if missing)");
            std::process::exit(2);
        });
        let token = deesainer_automation::control_server::load_or_create_token(&token_file)
            .unwrap_or_else(|e| {
                note!("deesainer: {e}");
                std::process::exit(2);
            });
        match deesainer_automation::control_server::ControlServer::start(port, token) {
            Ok(server) => {
                note!("deesainer: control server on {}", server.addr);
                deesainer_automation::control_server::write_pointer(port, &token_file);
                control_ext = Some(control::ControlExtension {
                    server,
                    read_root: flag("--automation-read-root").map(PathBuf::from),
                    write_root: flag("--automation-write-root").map(PathBuf::from),
                    file: Rc::clone(&file),
                    pending_shots: Vec::new(),
                });
            }
            Err(e) => {
                note!("deesainer: {e}");
                std::process::exit(2);
            }
        }
    }
    let present = args.iter().any(|a| a == "--present");
    let render_mode = match args
        .iter()
        .position(|a| a == "--render")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
    {
        Some("cpu") => deesainer_ui::RenderMode::Cpu,
        Some("mesh") => deesainer_ui::RenderMode::Mesh,
        _ => deesainer_ui::RenderMode::Auto,
    };
    let join = args
        .iter()
        .position(|a| a == "--join")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let branch = args
        .iter()
        .position(|a| a == "--branch")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let launch = Rc::new(RefCell::new(Some((session, control_ext))));
    let creator = |launch: Launch,
                   state: Rc<RefCell<FileState>>,
                   join: Option<String>,
                   pending_recovery: Option<recovery::Pending>|
     -> eframe::AppCreator<'static> {
        let branch = branch.clone();
        Box::new(move |_cc| {
            let Some((session, mut control_ext)) = launch.borrow_mut().take() else {
                return Err("the app was already created".into());
            };
            let mut app = App::new(session);
            app.language = load_language();
            app.display_p3 = display_p3;
            app.render_mode = render_mode;
            app.plugins = installed_plugins();
            app.extensions.push(Box::new(assistant::Assistant::new()));
            if let Some(ext) = control_ext.take() {
                app.extensions.push(Box::new(ext));
            }
            if present {
                app.set_show_prototype(true);
                app.present();
            }
            if let Some(addr) = join.as_deref() {
                let name = deesainer_automation::os_user();
                app.session.set_user(&name);
                app.extensions.push(Box::new(SyncExtension {
                    client: SyncClient::connect_branch(addr, &name, "", &branch),
                    version_label: String::new(),
                    branch_name: String::new(),
                    review: None,
                }));
            }
            let actions_state = Rc::clone(&state);
            app.file_actions = Some(Box::new(move |action, session| {
                file_action(&actions_state, action, session)
            }));
            let last_language = app.language;
            Ok(Box::new(Shell {
                app,
                file: state,
                title: String::new(),
                last_recovery: Instant::now(),
                recovery_revision: 0,
                recovery: recovery::Recovery::in_home(),
                pending_recovery,
                last_language,
                cjk_fonts: false,
                snapshot_written: false,
                figma: FigmaImport {
                    link: String::new(),
                    token: deesainer_automation::figma_api::token().unwrap_or_default(),
                    remember: true,
                    error: String::new(),
                    running: None,
                },
                collab: Collab {
                    addr: "127.0.0.1:7700".into(),
                    name: deesainer_automation::os_user(),
                    token: String::new(),
                    relay: None,
                    error: String::new(),
                    server: "http://127.0.0.1:7800".into(),
                    email: String::new(),
                    password: String::new(),
                    team: None,
                    team_files: Vec::new(),
                    team_error: String::new(),
                    team_resumed: false,
                },
            }))
        })
    };
    let gl_only = flag("--gpu").as_deref() == Some("gl");
    let run = |renderer, options: eframe::NativeOptions| {
        eframe::run_native(
            "Deesainer",
            eframe::NativeOptions {
                renderer,
                ..options
            },
            creator(
                Rc::clone(&launch),
                Rc::clone(&state),
                join.clone(),
                pending_recovery.clone(),
            ),
        )
    };
    if gl_only {
        return run(eframe::Renderer::Glow, options);
    }
    match run(eframe::Renderer::Wgpu, options.clone()) {
        Err(e) if launch.borrow().is_some() => {
            note!("deesainer: wgpu is unavailable ({e}); using OpenGL");
            run(eframe::Renderer::Glow, options)
        }
        other => other,
    }
}
