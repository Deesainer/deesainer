mod assets;
mod canvas;
mod chat;
mod inspect;
pub use canvas::attach_wgpu;
pub use i18n::Language;
mod comments;
mod connect;
mod design;
mod design_ext;
mod eyedropper;
mod history;
mod i18n;
mod icons;
pub mod keys;
mod layers;
pub mod library_review;
mod lint;
mod menu;
mod player;
pub mod plugin_panel;
mod shell;
pub mod theme;
mod widgets;

use std::collections::BTreeSet;

use deesainer_doc::NodeId;
use deesainer_engine::Session;
use deesainer_tools::{Out, Then, ToolKind, Tools};
use serde_json::Value;

pub use canvas::Camera;

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePeer {
    pub name: String,
    pub color: egui::Color32,
    pub page: Option<NodeId>,
    pub cursor: Option<deesainer_geom::Point>,
    pub selection: Vec<NodeId>,
    pub chat: Option<String>,
}

pub struct ExtensionView<'a> {
    pub session: &'a mut Session,
    pub cursor: Option<deesainer_geom::Point>,
    pub chat: Option<String>,
    pub peers: &'a mut Vec<RemotePeer>,
    pub camera: Camera,
    pub canvas_size: egui::Vec2,
    pub tool: &'static str,
    pub window: &'a mut WindowShot,
    pub gpu: &'static str,
    pub canvas_renderer: &'static str,
    pub mesh_fallback: Option<String>,
    pub flattened_layers: usize,
    pub instance_layer: Option<(NodeId, NodeId)>,
    pub editing: Option<NodeId>,
    pub ui_actions: &'a mut Vec<String>,
    pub pointer: &'a mut Vec<PointerStep>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerStep {
    pub kind: PointerKind,
    pub at: deesainer_geom::Point,
    pub screen: bool,
    pub modifiers: egui::Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerKind {
    Down,
    Move,
    Up,
    Wheel(f32, f32),
    DoubleClick,
    Key(egui::Key),
    Char(char),
}

#[derive(Default)]
pub struct WindowShot {
    pub requested: bool,
    in_flight: bool,
    pub image: Option<std::sync::Arc<egui::ColorImage>>,
}

pub trait Extension {
    fn name(&self) -> &str;
    fn poll(&mut self, view: &mut ExtensionView<'_>) -> bool;
    fn ui(&mut self, _ui: &mut egui::Ui, _session: &mut Session) {}
    fn sidebar_title(&self) -> Option<&str> {
        None
    }
    fn sidebar(&mut self, _ui: &mut egui::Ui, _session: &mut Session) {}
    fn overlay(&mut self, _ctx: &egui::Context) {}
}

pub type FileActions = Box<dyn FnMut(&str, &mut Session) -> Result<String, String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeftTab {
    Layers,
    Assets,
    Extension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RightTab {
    Design,
    Prototype,
    Inspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    #[default]
    Auto,
    Cpu,
    Mesh,
}

pub(crate) struct TextEditing {
    pub id: NodeId,
    pub instance: Option<NodeId>,
    pub editor: deesainer_text::TextEditor,
    pub focus_requested: bool,
    pub preedit: String,
    pub preedit_active: Option<std::ops::Range<usize>>,
}

pub struct App {
    pub session: Session,
    pub(crate) tools: Tools,
    pub camera: Camera,
    pub(crate) canvas: canvas::CanvasCache,
    pub(crate) editing: Option<TextEditing>,
    pub(crate) status: Option<(String, bool)>,
    pub(crate) expanded: BTreeSet<NodeId>,
    pub(crate) renaming: Option<(NodeId, String)>,
    pub(crate) shortcuts: Vec<(keys::Shortcut, &'static str)>,
    pub(crate) show_layers: bool,
    pub(crate) show_design: bool,
    pub(crate) fitted: bool,
    pub(crate) drag_group_open: bool,
    pub extensions: Vec<Box<dyn Extension>>,
    pub(crate) show_extensions: bool,
    pub(crate) show_variables: bool,
    pub(crate) show_comments: bool,
    pub(crate) placing_comment: bool,
    pub(crate) comment_draft: Option<(deesainer_geom::Point, String)>,
    pub(crate) chat: Option<chat::CursorChat>,
    pub pointer_page: Option<deesainer_geom::Point>,
    pub peers: Vec<RemotePeer>,
    pub(crate) player: Option<player::PlayerWindow>,
    pub(crate) show_prototype: bool,
    pub plugins: Vec<(String, String)>,
    font_families: Option<Vec<String>>,
    pub(crate) font_previews: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    pub(crate) asset_thumbs: std::collections::HashMap<NodeId, (u64, Option<egui::TextureHandle>)>,
    pub file_actions: Option<FileActions>,
    pub file_name: String,
    pub language: i18n::Language,
    pub(crate) left_tab: LeftTab,
    pub(crate) right_tab: RightTab,
    pub(crate) group_tool: Vec<ToolKind>,
    pub(crate) toast_seen: Option<(String, bool)>,
    pub(crate) toast_since: f64,
    styled: bool,
    pub window_shot: WindowShot,
    pub(crate) palette: Option<menu::Palette>,
    pub(crate) ui_hidden: bool,
    pub(crate) show_shortcuts: bool,
    pub(crate) show_rulers: bool,
    pub(crate) guide_drag: Option<(bool, f64)>,
    pub(crate) show_layout_guides: bool,
    pub(crate) snap_pixel_grid: bool,
    pub(crate) eyedropper: bool,
    pub(crate) connect_drag: Option<NodeId>,
    pub(crate) instance_layer: Option<(NodeId, NodeId)>,
    pub(crate) history: history::HistoryView,
    pub(crate) lint: lint::LintView,
    pub(crate) library_review: Option<library_review::LibraryReview>,
    pub(crate) plugin_panel: Option<plugin_panel::PluginPanel>,
    pointer_queue: std::collections::VecDeque<PointerStep>,
    pointer_modifiers: bool,
    pub(crate) gradient_edit: Option<(NodeId, usize)>,
    pub(crate) gradient_sticky: Option<(NodeId, usize)>,
    pub(crate) net_vertex: Option<(NodeId, usize)>,
    pub(crate) net_region: Option<(NodeId, usize)>,
    pub(crate) net_pen: Option<(NodeId, usize, usize)>,
    pub render_mode: RenderMode,
    pub gpu: &'static str,
    pub display_p3: bool,
    pub(crate) gpu_compositor: bool,
    pub(crate) gpu_health: Option<deesainer_gpu::Health>,
    pub(crate) inspect: inspect::InspectCache,
    pub(crate) inspect_language: &'static str,
}

impl App {
    pub fn new(session: Session) -> Self {
        let shortcuts = session
            .registry()
            .iter()
            .filter_map(|s| s.shortcut.and_then(keys::parse).map(|k| (k, s.id)))
            .collect();
        Self {
            session,
            tools: Tools::new(),
            camera: Camera::default(),
            canvas: canvas::CanvasCache::default(),
            editing: None,
            status: None,
            expanded: BTreeSet::new(),
            renaming: None,
            shortcuts,
            show_layers: true,
            show_design: true,
            fitted: false,
            drag_group_open: false,
            extensions: Vec::new(),
            show_extensions: false,
            show_variables: false,
            show_comments: true,
            placing_comment: false,
            comment_draft: None,
            pointer_page: None,
            chat: None,
            peers: Vec::new(),
            player: None,
            show_prototype: false,
            plugins: Vec::new(),
            font_families: None,
            font_previews: std::collections::HashMap::new(),
            asset_thumbs: std::collections::HashMap::new(),
            file_actions: None,
            file_name: "Untitled".to_owned(),
            language: i18n::Language::default(),
            left_tab: LeftTab::Layers,
            right_tab: RightTab::Design,
            group_tool: vec![
                ToolKind::Move,
                ToolKind::Frame,
                ToolKind::Rectangle,
                ToolKind::Pen,
                ToolKind::Text,
            ],
            toast_seen: None,
            toast_since: 0.0,
            styled: false,
            window_shot: WindowShot::default(),
            palette: None,
            ui_hidden: false,
            show_shortcuts: false,
            show_rulers: false,
            guide_drag: None,
            show_layout_guides: true,
            snap_pixel_grid: true,
            eyedropper: false,
            connect_drag: None,
            instance_layer: None,
            history: history::HistoryView::default(),
            lint: lint::LintView::default(),
            library_review: None,
            plugin_panel: None,
            pointer_queue: std::collections::VecDeque::new(),
            pointer_modifiers: false,
            gradient_edit: None,
            gradient_sticky: None,
            net_vertex: None,
            net_pen: None,
            net_region: None,
            render_mode: RenderMode::Auto,
            gpu: "none",
            display_p3: false,
            gpu_compositor: false,
            gpu_health: None,
            inspect: inspect::InspectCache::default(),
            inspect_language: "css",
        }
    }

    pub fn dispatch(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.dispatch(id, &params) {
            Ok(v) => {
                self.status = None;
                Some(v)
            }
            Err(e) => {
                self.status = Some((e.to_string(), true));
                None
            }
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), false));
    }

    pub(crate) fn run_outputs(&mut self, outs: Vec<Out>) {
        for out in outs {
            match out {
                Out::BeginGroup(label) => self.session.begin_group(&label),
                Out::EndGroup => self.session.end_group(),
                Out::Run {
                    command,
                    params,
                    then,
                } => {
                    if let Some(reply) = self.dispatch(&command, params) {
                        if then == Then::EditText {
                            if let Some(id) = reply.get("id").and_then(Value::as_u64) {
                                self.start_editing(NodeId(id));
                            }
                        }
                    }
                }
                Out::SetTool(t) => self.tools.active = t,
                Out::Pan(dx, dy) => {
                    self.camera.pan.x += (dx * self.camera.zoom) as f32;
                    self.camera.pan.y += (dy * self.camera.zoom) as f32;
                }
                Out::EditText(id) => self.start_editing(id),
            }
        }
    }

    pub(crate) fn start_editing(&mut self, id: NodeId) {
        let Some(node) = self.session.doc().get(id).cloned() else {
            return;
        };
        let Some(text) = node.text() else {
            return;
        };
        let width = match text.auto_resize {
            deesainer_doc::TextAutoResize::WidthAndHeight => None,
            _ => Some(node.rect.width),
        };
        let editor = match deesainer_text::TextEditor::new(text.clone(), width) {
            Ok(editor) => editor,
            Err(message) => {
                self.status = Some((message, true));
                return;
            }
        };
        self.commit_editing();
        let _ = self
            .session
            .dispatch("selection.set", &serde_json::json!({ "ids": [id] }));
        self.editing = Some(TextEditing {
            id,
            instance: None,
            editor,
            focus_requested: false,
            preedit: String::new(),
            preedit_active: None,
        });
    }

    pub(crate) fn start_instance_editing(&mut self, instance: NodeId, target: NodeId) {
        let doc = self.session.snapshot();
        let Some(inst) = doc.get(instance) else {
            return;
        };
        let Some((rect, _, node)) = deesainer_engine::slots::slot_box(&doc, inst, target) else {
            return;
        };
        let Some(text) = node.text() else {
            return;
        };
        let width = match text.auto_resize {
            deesainer_doc::TextAutoResize::WidthAndHeight => None,
            _ => Some(rect.width),
        };
        let editor = match deesainer_text::TextEditor::new(text.clone(), width) {
            Ok(editor) => editor,
            Err(message) => {
                self.status = Some((message, true));
                return;
            }
        };
        self.commit_editing();
        let _ = self
            .session
            .dispatch("selection.set", &serde_json::json!({ "ids": [instance] }));
        self.instance_layer = Some((instance, target));
        self.editing = Some(TextEditing {
            id: target,
            instance: Some(instance),
            editor,
            focus_requested: false,
            preedit: String::new(),
            preedit_active: None,
        });
    }

    pub(crate) fn editing_frame(&self) -> Option<(deesainer_doc::Node, deesainer_geom::Affine)> {
        let edit = self.editing.as_ref()?;
        let doc = self.session.doc();
        match edit.instance {
            None => Some((doc.get(edit.id)?.clone(), doc.world_transform(edit.id))),
            Some(inst) => {
                let instance = doc.get(inst)?;
                let (rect, rotation, mut node) =
                    deesainer_engine::slots::slot_box(doc, instance, edit.id)?;
                node.rect = rect;
                node.rotation = rotation;
                let t = doc
                    .world_transform(inst)
                    .then(deesainer_doc::Document::local_transform(&node));
                Some((node, t))
            }
        }
    }

    pub(crate) fn commit_editing(&mut self) {
        if let Some(inst) = self.editing.as_ref().and_then(|e| e.instance) {
            let current = self.editing_frame().and_then(|(n, _)| n.text().cloned());
            let Some(edit) = self.editing.take() else {
                return;
            };
            let buffer = &edit.editor.data().content;
            if current.is_some_and(|c| c.content != *buffer) {
                self.dispatch(
                    "instance.setOverride",
                    serde_json::json!({ "id": inst, "target": edit.id, "prop": "text", "value": buffer }),
                );
            }
            return;
        }
        let Some(edit) = self.editing.take() else {
            return;
        };
        let buffer = &edit.editor.data().content;
        let current = self
            .session
            .doc()
            .get(edit.id)
            .and_then(|n| n.text().cloned());
        match current {
            Some(_) if buffer.is_empty() => {
                self.dispatch("edit.delete", serde_json::json!({ "ids": [edit.id] }));
            }
            Some(c) if c.content != *buffer || c.runs != edit.editor.data().runs => {
                self.dispatch(
                    "text.setContent",
                    serde_json::json!({ "id": edit.id, "content": buffer, "runs": edit.editor.data().runs }),
                );
            }
            _ => {}
        }
    }

    pub fn tr<'a>(&self, english: &'a str) -> &'a str {
        i18n::tr(self.language, english)
    }

    pub(crate) fn font_families(&mut self) -> Vec<String> {
        self.font_families
            .get_or_insert_with(|| {
                let mut f = deesainer_text::families();
                f.retain(|n| !n.starts_with('.'));
                if !f.iter().any(|n| n == "Inter") {
                    f.insert(0, "Inter".to_owned());
                }
                f
            })
            .clone()
    }

    pub fn present(&mut self) {
        player::present(self);
    }

    pub fn set_show_prototype(&mut self, on: bool) {
        self.show_prototype = on;
        if on {
            self.right_tab = RightTab::Prototype;
        }
    }

    pub fn set_tool(&mut self, tool: ToolKind) {
        self.placing_comment = false;
        let outs = self.tools.set_tool(tool);
        self.run_outputs(outs);
    }

    fn window_capture(&mut self, ctx: &egui::Context) {
        let shot = &mut self.window_shot;
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(std::sync::Arc::clone(image)),
                _ => None,
            })
        });
        if let Some(image) = image {
            shot.image = Some(image);
            shot.in_flight = false;
            shot.requested = false;
        }
        if shot.requested && !shot.in_flight {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.in_flight = true;
        }
        if shot.requested || shot.in_flight {
            ctx.request_repaint();
        }
    }

    pub fn feed_input(&mut self, raw: &mut egui::RawInput) {
        let Some(step) = self.pointer_queue.pop_front() else {
            if std::mem::take(&mut self.pointer_modifiers) {
                raw.events
                    .push(egui::Event::ModifiersChanged(egui::Modifiers::default()));
            }
            return;
        };
        self.pointer_modifiers = true;
        match step.kind {
            PointerKind::Key(key) => {
                raw.events
                    .push(egui::Event::ModifiersChanged(step.modifiers));
                for pressed in [true, false] {
                    raw.events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: step.modifiers,
                    });
                }
                return;
            }
            PointerKind::Char(c) => {
                raw.events.push(egui::Event::Text(c.to_string()));
                return;
            }
            _ => {}
        }
        let pos = if step.screen {
            egui::pos2(step.at.x as f32, step.at.y as f32)
        } else {
            self.camera.to_screen(self.canvas.last_origin, step.at)
        };
        raw.events
            .push(egui::Event::ModifiersChanged(step.modifiers));
        raw.events.push(egui::Event::PointerMoved(pos));
        if let PointerKind::Wheel(dx, dy) = step.kind {
            raw.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(dx, dy),
                phase: egui::TouchPhase::Move,
                modifiers: step.modifiers,
            });
            return;
        }
        if step.kind == PointerKind::DoubleClick {
            for pressed in [true, false, true, false] {
                raw.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: step.modifiers,
                });
            }
            return;
        }
        if step.kind != PointerKind::Move {
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: step.kind == PointerKind::Down,
                modifiers: step.modifiers,
            });
        }
    }

    pub fn has_gpu_compositor(&self) -> bool {
        self.gpu_compositor
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let mut ui_actions = Vec::new();
        let mut pointer = Vec::new();
        let mut exts = std::mem::take(&mut self.extensions);
        for ext in &mut exts {
            let mut view = ExtensionView {
                session: &mut self.session,
                cursor: self.pointer_page,
                chat: self
                    .chat
                    .as_ref()
                    .map(|c| c.text.clone())
                    .filter(|t| !t.is_empty()),
                peers: &mut self.peers,
                camera: self.camera,
                canvas_size: self.canvas.last_size,
                tool: self.tools.active.label(),
                window: &mut self.window_shot,
                gpu: self.gpu,
                canvas_renderer: if self.canvas.drew_mesh { "mesh" } else { "cpu" },
                mesh_fallback: self.canvas.mesh_fallback(),
                flattened_layers: self
                    .gpu_health
                    .as_ref()
                    .map_or(0, deesainer_gpu::Health::flattened_layers),
                instance_layer: self.instance_layer,
                editing: self.editing.as_ref().map(|e| e.id),
                ui_actions: &mut ui_actions,
                pointer: &mut pointer,
            };
            changed |= ext.poll(&mut view);
            ext.overlay(ui.ctx());
        }
        self.extensions = exts;
        for action in ui_actions {
            menu::run_ui_action(self, &action);
        }
        self.pointer_queue.extend(pointer);
        if !self.pointer_queue.is_empty() || self.pointer_modifiers {
            ui.ctx().request_repaint();
        }
        if changed || !self.extensions.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.window_capture(ui.ctx());
        if !self.styled {
            theme::install_fonts(ui.ctx());
            theme::apply(ui.ctx());
            self.styled = true;
            ui.ctx().request_repaint();
            return;
        }
        if let Some((id, _)) = self.gradient_sticky {
            if self.session.selection() != [id] {
                self.gradient_sticky = None;
            }
        }
        self.gradient_edit = self.gradient_sticky;
        if self.palette.is_none() && self.player.is_none() {
            menu::handle_shortcuts(self, ui);
        }
        if self.ui_hidden {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| canvas::canvas(self, ui));
            return;
        }
        egui::Panel::left("rail")
            .exact_size(shell::RAIL)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::TOKENS.panel))
            .show(ui, |ui| shell::rail(self, ui));
        if self.show_layers {
            egui::Panel::left("layers")
                .resizable(true)
                .default_size(shell::SIDEBAR)
                .size_range(200.0..=480.0)
                .frame(shell::panel_frame())
                .show(ui, |ui| match self.left_tab {
                    LeftTab::Layers => {
                        shell::file_header(self, ui);
                        layers::panel(self, ui);
                    }
                    LeftTab::Assets => {
                        shell::file_header(self, ui);
                        assets::panel(self, ui);
                    }
                    LeftTab::Extension => {
                        let mut exts = std::mem::take(&mut self.extensions);
                        if let Some(ext) = exts.iter_mut().find(|e| e.sidebar_title().is_some()) {
                            ext.sidebar(ui, &mut self.session);
                        }
                        self.extensions = exts;
                    }
                });
        }
        history::window(self, ui.ctx());
        lint::window(self, ui.ctx());
        library_review::window(self, ui.ctx());
        plugin_panel::window(self, ui.ctx());
        if self.show_variables {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::TOKENS.panel))
                .show(ui, |ui| design_ext::variables_view(self, ui));
            shell::shortcuts_window(self, ui.ctx());
            return;
        }
        if self.show_design {
            let panel = egui::Panel::right("design")
                .resizable(true)
                .default_size(shell::SIDEBAR)
                .size_range(200.0..=480.0)
                .frame(shell::panel_frame())
                .show(ui, |ui| {
                    shell::right_header(self, ui);
                    if self.placing_comment {
                        comments::comments_panel(self, ui);
                        return;
                    }
                    match self.right_tab {
                        RightTab::Design => design::panel(self, ui),
                        RightTab::Prototype => design::prototype_panel(self, ui),
                        RightTab::Inspect => inspect::panel(self, ui),
                    }
                });
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new(widgets::SIDEBAR_LEFT),
                    panel.response.rect.left(),
                )
            });
        }
        let central = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                canvas::canvas(self, ui);
                ui.max_rect()
            });
        let canvas_rect = central.inner;
        shell::toolbar(self, ui.ctx(), canvas_rect);
        shell::help(self, ui.ctx(), canvas_rect);
        shell::toast(self, ui.ctx(), canvas_rect);
        shell::palette(self, ui.ctx(), canvas_rect);
        shell::shortcuts_window(self, ui.ctx());

        comments::comments_window(self, ui.ctx());
        player::window(self, ui.ctx());
        if self.show_extensions {
            let mut open = true;
            let mut exts = std::mem::take(&mut self.extensions);
            egui::Window::new("Extensions")
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    if exts.is_empty() {
                        ui.label("No extensions running.");
                    }
                    for ext in &mut exts {
                        ui.collapsing(ext.name().to_owned(), |ui| ext.ui(ui, &mut self.session));
                    }
                });
            self.extensions = exts;
            self.show_extensions = open;
        }
    }
}
