use deesainer_tools::ToolKind;
use egui::{Event, Key};
use serde_json::{json, Value};

use crate::canvas;
use crate::icons::Icon;
use crate::widgets;
use crate::{i18n, keys, App};

fn special(app: &mut App, id: &str) -> bool {
    if let Some(tool) = ToolKind::for_command(id) {
        app.set_tool(tool);
        return true;
    }
    match id {
        "node.rename" => {
            if let Some(first) = app.session.selection().first().copied() {
                let name = app
                    .session
                    .doc()
                    .get(first)
                    .map(|n| n.name.clone())
                    .unwrap_or_default();
                app.renaming = Some((first, name));
                app.show_layers = true;
            }
            true
        }
        "comment.add" => {
            app.placing_comment = true;
            app.show_comments = true;
            app.show_design = true;
            true
        }
        "edit.copy" | "edit.cut" | "edit.paste" => false,
        _ => false,
    }
}

pub fn run_command(app: &mut App, id: &str) {
    if special(app, id) {
        return;
    }
    match id {
        "image.place" => return run_file_action(app, "placeImage"),
        "import.svg" => return run_file_action(app, "importSvg"),
        "plugin.run" => return run_file_action(app, "runPlugin"),
        _ => {}
    }
    if let Some(action) = id.strip_prefix("file.") {
        if action != "new" {
            run_file_action(app, action);
            return;
        }
    }
    app.dispatch(id, Value::Null);
}

pub(crate) fn run_file_action(app: &mut App, action: &str) {
    match app.file_actions.as_mut() {
        Some(f) => match f(action, &mut app.session) {
            Ok(msg) if crate::library_review::open_from_message(app, &msg) => {}
            Ok(msg) if crate::plugin_panel::open_from_message(app, &msg) => {}
            Ok(msg) if !msg.is_empty() => app.info(msg),
            Ok(_) => {}
            Err(e) => app.status = Some((e, true)),
        },
        None => {
            let _ = app
                .session
                .dispatch(&format!("file.{action}"), &Value::Null);
        }
    }
    app.fitted = false;
}

enum Entry {
    Cmd(&'static str, &'static str),
    Ui(&'static str, &'static str, &'static str),
    Check(&'static str, &'static str, &'static str),
    Sub(&'static str, &'static [Entry]),
    Sep,
}

use Entry::{Check, Cmd, Sep, Sub, Ui};

const FILE: &[Entry] = &[
    Cmd("file.new", "New design"),
    Sep,
    Cmd("image.place", "Place image…"),
    Ui("Import SVG…", "", "file:importSvg"),
    Ui("Import from Figma…", "", "file:importFigma"),
    Ui("Import library…", "", "file:importLibrary"),
    Ui("Update library…", "", "file:updateLibrary"),
    Sep,
    Ui("Open…", "⌘O", "file:open"),
    Ui("Save", "⌘S", "file:save"),
    Ui("Save as…", "⇧⌘S", "file:saveAs"),
    Sep,
    Ui("Export…", "⇧⌘E", "file:export"),
    Sep,
    Ui("Version history…", "", "toggle:history"),
    Sep,
    Ui("Collaborate…", "", "file:collaborate"),
];

const EDIT: &[Entry] = &[
    Cmd("edit.undo", ""),
    Cmd("edit.redo", ""),
    Sep,
    Cmd("edit.copy", ""),
    Cmd("edit.cut", ""),
    Cmd("edit.paste", ""),
    Cmd("edit.duplicate", ""),
    Cmd("edit.delete", ""),
    Sep,
    Cmd("selection.selectAll", ""),
    Ui("Select none", "Esc", "select:none"),
    Cmd("selection.selectInverse", ""),
];

const RENDERER: &[Entry] = &[
    Check("Automatic", "", "render:auto"),
    Check("CPU", "", "render:cpu"),
    Check("Meshes (GPU)", "", "render:mesh"),
];

const VIEW: &[Entry] = &[
    Check("Layout guides", "⇧G", "toggle:guides"),
    Check("Snap to pixel grid", "", "toggle:pixelGrid"),
    Check("Design checks", "", "toggle:lint"),
    Check("Rulers", "⇧R", "toggle:rulers"),
    Check("Comments", "⇧C", "toggle:comments"),
    Check("Prototype connections", "", "toggle:prototype"),
    Check("Inspect", "⇧D", "toggle:inspect"),
    Sub("Renderer", RENDERER),
    Sub("Language", LANGUAGES),
    Sep,
    Check("Show/Hide UI", "⌘\\", "toggle:ui"),
    Check("Left sidebar", "", "toggle:left"),
    Check("Right sidebar", "", "toggle:right"),
    Sep,
    Ui("Zoom in", "⌘+", "zoom:in"),
    Ui("Zoom out", "⌘-", "zoom:out"),
    Ui("Zoom to 100%", "⌘0", "zoom:100"),
    Ui("Zoom to fit", "⇧1", "zoom:fit"),
    Ui("Zoom to selection", "⇧2", "zoom:selection"),
    Sep,
    Ui("Previous page", "", "page:prev"),
    Ui("Next page", "", "page:next"),
];

const BOOLEAN: &[Entry] = &[
    Cmd("object.booleanUnion", "Union"),
    Cmd("object.booleanSubtract", "Subtract"),
    Cmd("object.booleanIntersect", "Intersect"),
    Cmd("object.booleanExclude", "Exclude"),
];

const OBJECT: &[Entry] = &[
    Cmd("object.frameSelection", ""),
    Cmd("object.group", ""),
    Cmd("object.ungroup", "Ungroup selection"),
    Sep,
    Cmd("layout.addAutoLayout", ""),
    Sep,
    Cmd("component.create", ""),
    Cmd("component.resetOverrides", "Reset instance"),
    Cmd("component.detach", ""),
    Cmd("component.goToMain", ""),
    Sep,
    Cmd("object.bringToFront", ""),
    Cmd("object.bringForward", ""),
    Cmd("object.sendBackward", ""),
    Cmd("object.sendToBack", ""),
    Sep,
    Cmd("object.flipHorizontal", ""),
    Cmd("object.flipVertical", ""),
    Ui("Rotate 180°", "", "rotate:180"),
    Ui("Rotate 90° left", "", "rotate:-90"),
    Ui("Rotate 90° right", "", "rotate:90"),
    Sep,
    Cmd("object.flatten", "Flatten"),
    Cmd("object.outlineStroke", ""),
    Sub("Boolean groups", BOOLEAN),
    Sep,
    Cmd("object.toggleVisible", ""),
    Cmd("object.toggleLock", ""),
];

const ALIGNMENT: &[Entry] = &[
    Ui("Text align left", "", "text:align:left"),
    Ui("Text align center", "", "text:align:center"),
    Ui("Text align right", "", "text:align:right"),
    Ui("Text align justified", "", "text:align:justified"),
];

const CASE: &[Entry] = &[
    Ui("Original case", "", "text:case:original"),
    Ui("Uppercase", "", "text:case:upper"),
    Ui("Lowercase", "", "text:case:lower"),
    Ui("Title case", "", "text:case:title"),
];

const TEXT: &[Entry] = &[
    Ui("Bold", "⌘B", "text:bold"),
    Ui("Italic", "⌘I", "text:italic"),
    Ui("Underline", "⌘U", "text:underline"),
    Ui("Strikethrough", "⇧⌘X", "text:strike"),
    Sep,
    Sub("Alignment", ALIGNMENT),
    Sub("Case", CASE),
];

const ARRANGE: &[Entry] = &[
    Cmd("align.left", "Align left"),
    Cmd("align.horizontalCenter", "Align horizontal centers"),
    Cmd("align.right", "Align right"),
    Cmd("align.top", "Align top"),
    Cmd("align.verticalCenter", "Align vertical centers"),
    Cmd("align.bottom", "Align bottom"),
    Sep,
    Cmd("distribute.horizontal", ""),
    Cmd("distribute.vertical", ""),
];

const LANGUAGES: &[Entry] = &[
    Check("English", "", "language:en"),
    Check("Italiano", "", "language:it"),
    Check("Español", "", "language:es"),
    Check("Français", "", "language:fr"),
    Check("Deutsch", "", "language:de"),
    Check("中文", "", "language:zh"),
    Check("日本語", "", "language:ja"),
];

const HELP: &[Entry] = &[
    Ui("Keyboard shortcuts", "^⇧?", "help:shortcuts"),
    Ui("Extensions…", "", "help:extensions"),
    Sep,
    Ui("About Deesainer", "", "help:about"),
];

pub fn main_menu(app: &mut App, ui: &mut egui::Ui) {
    if widgets::menu_item(ui, None, Some(Icon::Search), app.tr("Actions…"), "⌘K", true).clicked()
    {
        app.palette = Some(Palette::default());
        ui.close();
    }
    widgets::menu_separator(ui);
    for (top, entries) in [
        ("File", FILE),
        ("Edit", EDIT),
        ("View", VIEW),
        ("Object", OBJECT),
        ("Text", TEXT),
        ("Arrange", ARRANGE),
    ] {
        ui.menu_button(app.tr(top), |ui| entries_ui(app, ui, entries));
    }
    widgets::menu_separator(ui);
    ui.menu_button(app.tr("Plugins"), |ui| {
        for (label, action) in app.plugins.clone() {
            if widgets::menu_item(ui, None, None, &label, "", true).clicked() {
                run_file_action(app, &action);
                ui.close();
            }
        }
        if !app.plugins.is_empty() {
            widgets::menu_separator(ui);
        }
        entries_ui(app, ui, &[Cmd("plugin.run", "Run plugin…")]);
    });
    widgets::menu_separator(ui);
    ui.menu_button(app.tr("Help and account"), |ui| entries_ui(app, ui, HELP));
}

const OBJECT_CONTEXT: &[Entry] = &[
    Cmd("edit.copy", "Copy"),
    Cmd("edit.paste", "Paste here"),
    Cmd("edit.duplicate", "Duplicate"),
    Sep,
    Cmd("object.bringToFront", "Bring to front"),
    Cmd("object.sendToBack", "Send to back"),
    Sep,
    Cmd("object.group", "Group selection"),
    Cmd("object.frameSelection", "Frame selection"),
    Cmd("object.ungroup", "Ungroup"),
    Cmd("object.flatten", "Flatten"),
    Cmd("object.outlineStroke", "Outline stroke"),
    Sep,
    Cmd("layout.addAutoLayout", "Add auto layout"),
    Cmd("component.create", "Create component"),
    Sep,
    Cmd("object.toggleVisible", "Show/Hide"),
    Cmd("object.toggleLock", "Lock/Unlock"),
    Sep,
    Cmd("object.flipHorizontal", "Flip horizontal"),
    Cmd("object.flipVertical", "Flip vertical"),
    Sep,
    Cmd("edit.delete", "Delete"),
];

const CANVAS_CONTEXT: &[Entry] = &[
    Cmd("edit.paste", "Paste here"),
    Sep,
    Ui("Show/Hide UI", "⌘\\", "toggle:ui"),
    Ui("Show/Hide comments", "⇧C", "toggle:comments"),
    Sep,
    Ui("Actions…", "⌘K", "palette"),
    Cmd("selection.selectAll", "Select all"),
];

pub fn object_context(app: &mut App, ui: &mut egui::Ui, rename: bool) {
    if rename && widgets::menu_item(ui, None, None, app.tr("Rename"), "⌘R", true).clicked() {
        run_command(app, "node.rename");
        ui.close();
    }
    entries_ui(app, ui, OBJECT_CONTEXT);
}

pub fn canvas_context(app: &mut App, ui: &mut egui::Ui) {
    entries_ui(app, ui, CANVAS_CONTEXT);
}

pub fn help_menu(app: &mut App, ui: &mut egui::Ui) {
    entries_ui(app, ui, HELP);
}

fn shortcut_label(app: &App, id: &str) -> String {
    app.session
        .registry()
        .iter()
        .find(|s| s.id == id)
        .and_then(|s| s.shortcut)
        .and_then(keys::parse)
        .map(|k| k.label())
        .unwrap_or_default()
}

fn command_label(app: &App, id: &str) -> String {
    app.session
        .registry()
        .iter()
        .find(|s| s.id == id)
        .map(|s| {
            s.menu_path
                .rsplit('/')
                .next()
                .filter(|l| !l.is_empty())
                .unwrap_or(s.label)
                .to_owned()
        })
        .unwrap_or_else(|| id.to_owned())
}

fn label_of<'a>(app: &App, label: &'a str) -> &'a str {
    app.tr(label)
}

fn entries_ui(app: &mut App, ui: &mut egui::Ui, entries: &[Entry]) {
    let has_checks = entries.iter().any(|e| matches!(e, Check(..)));
    for e in entries {
        match e {
            Cmd(id, label) => {
                let label = if label.is_empty() {
                    command_label(app, id)
                } else {
                    (*label).to_owned()
                };
                let label = label_of(app, &label).to_owned();
                let key = shortcut_label(app, id);
                let enabled = app.session.is_enabled(id);
                let check = has_checks.then_some(false);
                if widgets::menu_item(ui, check, None, &label, &key, enabled).clicked() {
                    run_command(app, id);
                    ui.close();
                }
            }
            Ui(label, key, action) => {
                let enabled = ui_action_enabled(app, action);
                let check = has_checks.then_some(false);
                if widgets::menu_item(ui, check, None, label_of(app, label), key, enabled).clicked()
                {
                    run_ui_action(app, action);
                    ui.close();
                }
            }
            Check(label, key, action) => {
                let on = ui_toggle_state(app, action);
                if widgets::menu_item(ui, Some(on), None, label_of(app, label), key, true).clicked()
                {
                    run_ui_action(app, action);
                    ui.close();
                }
            }
            Sub(label, sub) => {
                ui.menu_button(label_of(app, label), |ui| entries_ui(app, ui, sub));
            }
            Sep => widgets::menu_separator(ui),
        }
    }
}

fn ui_toggle_state(app: &App, action: &str) -> bool {
    match action {
        "toggle:comments" => app.show_comments,
        "render:auto" => app.render_mode == crate::RenderMode::Auto,
        "render:cpu" => app.render_mode == crate::RenderMode::Cpu,
        "render:mesh" => app.render_mode == crate::RenderMode::Mesh,
        "toggle:rulers" => app.show_rulers,
        "toggle:guides" => app.show_layout_guides,
        "toggle:pixelGrid" => app.snap_pixel_grid,
        "toggle:lint" => app.lint.open,
        "toggle:prototype" => app.show_prototype,
        "toggle:inspect" => app.right_tab == crate::RightTab::Inspect,
        "toggle:ui" => !app.ui_hidden,
        "toggle:left" => app.show_layers,
        "toggle:right" => app.show_design,
        _ if action.starts_with("language:") => action
            .strip_prefix("language:")
            .is_some_and(|l| app.language.as_str() == l),
        _ => false,
    }
}

fn selected_text(app: &App) -> Option<deesainer_doc::TextData> {
    let doc = app.session.doc();
    app.session
        .selection()
        .iter()
        .find_map(|id| doc.get(*id).and_then(|n| n.text().cloned()))
}

fn ui_action_enabled(app: &App, action: &str) -> bool {
    if action.starts_with("text:") {
        return selected_text(app).is_some();
    }
    if action.starts_with("rotate:") {
        return !app.session.selection().is_empty();
    }
    if action.starts_with("file:") {
        return app.file_actions.is_some();
    }
    if action.starts_with("page:") {
        return app.session.snapshot().pages().len() > 1;
    }
    true
}

pub fn run_ui_action(app: &mut App, action: &str) {
    if let Some(a) = action.strip_prefix("file:") {
        return run_file_action(app, a);
    }
    if let Some(z) = action.strip_prefix("zoom:") {
        return canvas::zoom_to(app, z);
    }
    if let Some(name) = action.strip_prefix("tool:") {
        if let Some(tool) = deesainer_tools::ToolKind::ALL
            .into_iter()
            .find(|t| t.label().eq_ignore_ascii_case(name))
        {
            app.set_tool(tool);
        }
        return;
    }
    if let Some(deg) = action
        .strip_prefix("rotate:")
        .and_then(|d| d.parse::<f64>().ok())
    {
        let doc = app.session.snapshot();
        let sel = app.session.selection().to_vec();
        app.session.begin_group("Rotate");
        for id in sel {
            let current = doc.get(id).map_or(0.0, |n| n.rotation);
            app.dispatch(
                "node.setRotation",
                json!({ "ids": [id], "degrees": current + deg }),
            );
        }
        app.session.end_group();
        return;
    }
    if let Some(l) = action.strip_prefix("language:") {
        if let Some(lang) = i18n::Language::from_code(l) {
            app.language = lang;
        }
        return;
    }
    let ids = app.session.selection().to_vec();
    let text = selected_text(app);
    match action {
        "palette" => app.palette = Some(Palette::default()),
        "select:none" => {
            app.dispatch("selection.clear", Value::Null);
        }
        "toggle:comments" => app.show_comments = !app.show_comments,
        "render:auto" => app.render_mode = crate::RenderMode::Auto,
        "render:cpu" => app.render_mode = crate::RenderMode::Cpu,
        "render:mesh" => app.render_mode = crate::RenderMode::Mesh,
        "toggle:rulers" => app.show_rulers = !app.show_rulers,
        "toggle:guides" => app.show_layout_guides = !app.show_layout_guides,
        "toggle:pixelGrid" => app.snap_pixel_grid = !app.snap_pixel_grid,
        "eyedropper" => app.eyedropper = !app.session.selection().is_empty(),
        "toggle:variables" => app.show_variables = !app.show_variables,
        "toggle:history" => app.history.open = !app.history.open,
        "toggle:lint" => app.lint.open = !app.lint.open,
        "present" => crate::player::present(app),
        "left:assets" => app.left_tab = crate::LeftTab::Assets,
        "left:layers" => app.left_tab = crate::LeftTab::Layers,
        "tab:prototype" => {
            app.right_tab = crate::RightTab::Prototype;
            app.show_prototype = true;
        }
        "tab:design" => {
            app.right_tab = crate::RightTab::Design;
            app.show_prototype = false;
        }
        "toggle:prototype" => app.show_prototype = !app.show_prototype,
        "toggle:inspect" => {
            app.right_tab = if app.right_tab == crate::RightTab::Inspect {
                crate::RightTab::Design
            } else {
                crate::RightTab::Inspect
            };
            app.show_prototype = false;
            app.show_design = true;
        }
        "toggle:ui" => app.ui_hidden = !app.ui_hidden,
        "toggle:left" => app.show_layers = !app.show_layers,
        "toggle:right" => app.show_design = !app.show_design,
        "page:prev" | "page:next" => {
            let doc = app.session.snapshot();
            let pages = doc.pages().to_vec();
            let cur = app.session.current_page();
            if let Some(i) = pages.iter().position(|p| *p == cur) {
                let n = pages.len();
                let j = if action == "page:next" {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                };
                app.dispatch("page.select", json!({ "id": pages[j] }));
                app.fitted = false;
            }
        }
        "text:bold" => {
            if let Some(t) = text {
                let w = if t.style.font_weight >= 600 { 400 } else { 700 };
                app.dispatch("text.setStyle", json!({ "ids": ids, "fontWeight": w }));
            }
        }
        "text:italic" => {
            if let Some(t) = text {
                app.dispatch(
                    "text.setStyle",
                    json!({ "ids": ids, "italic": !t.style.italic }),
                );
            }
        }
        "text:underline" | "text:strike" => {
            if let Some(t) = text {
                let want = if action == "text:underline" {
                    "underline"
                } else {
                    "strikethrough"
                };
                let now = serde_json::to_value(t.style.decoration)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let next = if now == want { "none" } else { want };
                app.dispatch("text.setStyle", json!({ "ids": ids, "decoration": next }));
            }
        }
        "help:shortcuts" => app.show_shortcuts = true,
        "help:extensions" => app.show_extensions = true,
        "help:about" => app.info(format!(
            "Deesainer {} - a clean-room, open-source design tool.",
            env!("CARGO_PKG_VERSION")
        )),
        a => {
            if let Some(v) = a.strip_prefix("text:align:") {
                app.dispatch("text.setStyle", json!({ "ids": ids, "align": v }));
            } else if let Some(v) = a.strip_prefix("text:case:") {
                app.dispatch("text.setStyle", json!({ "ids": ids, "case": v }));
            }
        }
    }
}

pub fn palette_entries(app: &App) -> Vec<(String, String, bool, String)> {
    let mut out = Vec::new();
    fn walk(app: &App, entries: &[Entry], out: &mut Vec<(String, String, bool, String)>) {
        for e in entries {
            match e {
                Cmd(id, label) => {
                    let l = if label.is_empty() {
                        command_label(app, id)
                    } else {
                        (*label).to_owned()
                    };
                    out.push((
                        label_of(app, &l).to_owned(),
                        shortcut_label(app, id),
                        true,
                        (*id).to_owned(),
                    ));
                }
                Ui(l, k, a) | Check(l, k, a) => {
                    out.push((
                        label_of(app, l).to_owned(),
                        (*k).to_owned(),
                        false,
                        (*a).to_owned(),
                    ));
                }
                Sub(_, sub) => walk(app, sub, out),
                Sep => {}
            }
        }
    }
    for list in [FILE, EDIT, VIEW, OBJECT, TEXT, ARRANGE, HELP] {
        walk(app, list, &mut out);
    }
    for spec in app.session.registry().iter() {
        if !out.iter().any(|(_, _, cmd, id)| *cmd && id == spec.id) {
            out.push((
                spec.label.to_owned(),
                spec.shortcut
                    .and_then(keys::parse)
                    .map(|k| k.label())
                    .unwrap_or_default(),
                true,
                spec.id.to_owned(),
            ));
        }
    }
    out
}

#[derive(Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
}

pub fn handle_shortcuts(app: &mut App, ui: &mut egui::Ui) {
    let wants_keyboard = ui.ctx().egui_wants_keyboard_input() || app.editing.is_some();
    let popup_open = egui::Popup::is_any_open(ui.ctx());
    let events = ui.input(|i| i.events.clone());
    for ev in events {
        match ev {
            Event::Copy if !wants_keyboard => {
                if let Some(v) = app.dispatch("edit.copy", Value::Null) {
                    ui.ctx().copy_text(v.to_string());
                }
            }
            Event::Cut if !wants_keyboard => {
                if let Some(v) = app.dispatch("edit.cut", Value::Null) {
                    ui.ctx().copy_text(v.to_string());
                }
            }
            Event::Paste(text) if !wants_keyboard => {
                let parsed: Option<Value> = serde_json::from_str(&text)
                    .ok()
                    .filter(|v: &Value| v.get("deesainer").is_some());
                match parsed {
                    Some(data) => {
                        app.dispatch("edit.paste", json!({ "data": data }));
                    }
                    None if app.session.clipboard().is_some() => {
                        app.dispatch("edit.paste", Value::Null);
                    }
                    None => {}
                }
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if !wants_keyboard && !(popup_open && key == Key::Escape) => {
                key_pressed(app, key, modifiers)
            }
            _ => {}
        }
    }
}

fn key_pressed(app: &mut App, key: Key, m: egui::Modifiers) {
    let plain = !m.command && !m.alt && !m.ctrl;
    match key {
        Key::Escape => {
            if app.instance_layer.take().is_some() {
                return;
            }
            if app.eyedropper {
                app.eyedropper = false;
                return;
            }
            if app.placing_comment {
                app.placing_comment = false;
                app.comment_draft = None;
                return;
            }
            if app.tools.is_busy() || app.tools.editing_path().is_some() {
                let outs = canvas::cancel_tool(&mut app.tools);
                app.run_outputs(outs);
            } else {
                app.dispatch("selection.clear", Value::Null);
            }
            return;
        }
        Key::Slash if plain && !m.shift => {
            crate::chat::open(app);
            return;
        }
        Key::Enter if app.tools.active == ToolKind::Pen => {
            let outs = app.tools.finish_pen();
            app.run_outputs(outs);
            return;
        }
        Key::Enter if plain && !m.shift => {
            if let [id] = app.session.selection() {
                let id = *id;
                if app
                    .session
                    .doc()
                    .get(id)
                    .is_some_and(|n| n.text().is_some())
                {
                    app.start_editing(id);
                    return;
                }
            }
        }
        Key::Backspace | Key::Delete
            if plain
                && app
                    .net_vertex
                    .is_some_and(|(n, _)| app.session.selection() == [n]) =>
        {
            if let Some((id, vertex)) = app.net_vertex.take() {
                app.dispatch("vector.removeVertex", json!({ "id": id, "vertex": vertex }));
            }
            return;
        }
        Key::Backspace | Key::Delete if plain && app.tools.editing_path().is_some() => {
            let outs = app.tools.delete_selected_point(app.session.doc());
            if !outs.is_empty() {
                app.run_outputs(outs);
            }
            return;
        }
        Key::Backspace if plain => {
            app.dispatch("edit.delete", Value::Null);
            return;
        }
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown if !m.command => {
            let step = if m.shift { 10.0 } else { 1.0 };
            let (dx, dy) = match key {
                Key::ArrowLeft => (-step, 0.0),
                Key::ArrowRight => (step, 0.0),
                Key::ArrowUp => (0.0, -step),
                _ => (0.0, step),
            };
            canvas::nudge(app, dx, dy);
            return;
        }
        Key::I if plain && !m.shift => {
            if app.session.selection().is_empty() {
                app.info("Select layers to pick a colour for");
            } else {
                app.eyedropper = true;
            }
            return;
        }
        Key::K if m.command && !m.shift && !m.alt => {
            app.palette = Some(Palette::default());
            return;
        }
        Key::Backslash if m.command => return run_ui_action(app, "toggle:ui"),
        Key::Num0 if m.command => return canvas::zoom_to(app, "100"),
        Key::B if m.command && !m.shift && !m.alt => return run_ui_action(app, "text:bold"),
        Key::I if m.command && !m.shift => return run_ui_action(app, "text:italic"),
        Key::U if m.command && !m.shift => return run_ui_action(app, "text:underline"),
        Key::X if m.command && m.shift => return run_ui_action(app, "text:strike"),
        Key::C if m.shift && !m.command => return run_ui_action(app, "toggle:comments"),
        Key::R if m.shift && !m.command => return run_ui_action(app, "toggle:rulers"),
        Key::D if m.shift && !m.command && !m.alt => return run_ui_action(app, "toggle:inspect"),
        Key::G if m.shift && !m.command && !m.ctrl => return run_ui_action(app, "toggle:guides"),
        Key::O if m.command && !m.shift => return run_file_action(app, "open"),
        Key::S if m.command && m.shift => return run_file_action(app, "saveAs"),
        Key::S if m.command => return run_file_action(app, "save"),
        Key::E if m.command && m.shift => return run_file_action(app, "export"),
        Key::K if m.command && m.shift => return run_file_action(app, "placeImage"),
        Key::Enter if m.command && m.alt => return crate::player::present(app),
        Key::Num0 if m.shift && !m.command => return canvas::zoom_to(app, "100"),
        Key::Num1 if m.shift && !m.command => return canvas::zoom_to(app, "fit"),
        Key::Num2 if m.shift && !m.command => return canvas::zoom_to(app, "selection"),
        Key::Equals | Key::Plus if m.command => return canvas::zoom_to(app, "in"),
        Key::Minus if m.command => return canvas::zoom_to(app, "out"),
        _ => {}
    }
    if plain && !m.shift {
        if let Some(tool) = canvas::tool_for_key(key) {
            app.set_tool(tool);
            return;
        }
    }
    let hit = app
        .shortcuts
        .iter()
        .find(|(s, _)| s.matches(key, m))
        .map(|(_, id)| *id);
    if let Some(id) = hit {
        if matches!(id, "edit.copy" | "edit.cut" | "edit.paste") {
            return;
        }
        if app.session.is_enabled(id) {
            run_command(app, id);
        }
    }
}
