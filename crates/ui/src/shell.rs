use deesainer_tools::ToolKind;
use egui::{vec2, Align, Align2, Color32, Layout, Margin, Rect, Sense, Ui, Vec2};

use crate::icons::{self, Icon};
use crate::theme::{self, TOKENS};
use crate::widgets::Tip;
use crate::widgets::{self, PAD};
use crate::{canvas, menu, App, LeftTab, RightTab};

pub const RAIL: f32 = 48.0;
pub const SIDEBAR: f32 = 240.0;

pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(TOKENS.panel)
        .inner_margin(Margin::symmetric(PAD as i8, 0))
}

pub fn rail(app: &mut App, ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        let logo = icons::tile(ui, Icon::Logo, false, "Main menu");
        crate::widgets::menu(&logo)
            .align(egui::RectAlign::RIGHT_START)
            .show(|ui| menu::main_menu(app, ui));
        ui.add_space(8.0);
        let has_ai = app.extensions.iter().any(|e| e.sidebar_title().is_some());
        let mut tabs = vec![(LeftTab::Layers, Icon::Layers, "File")];
        if has_ai {
            tabs.push((LeftTab::Extension, Icon::Sparkle, "AI assistant"));
        }
        tabs.push((LeftTab::Assets, Icon::Assets, "Assets"));
        for (tab, icon, tip) in tabs {
            let active = app.show_layers && app.left_tab == tab;
            if icons::tile(ui, icon, active, tip).clicked() {
                if active {
                    app.show_layers = false;
                } else {
                    app.show_layers = true;
                    app.left_tab = tab;
                }
            }
            ui.add_space(4.0);
        }
        let (r, _) = ui.allocate_exact_size(vec2(20.0, 9.0), Sense::hover());
        ui.painter().rect_filled(
            Rect::from_center_size(r.center(), vec2(20.0, 1.0)),
            0.0,
            TOKENS.border,
        );
        ui.add_space(4.0);
        if icons::tile(ui, Icon::Variables, app.show_variables, "Variables").clicked() {
            app.show_variables = !app.show_variables;
        }
    });
}

pub fn file_header(app: &mut App, ui: &mut Ui) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let name = app.file_name.clone();
        let resp = widgets::dropdown_label(ui, &name, true);
        crate::widgets::menu(&resp).show(|ui| {
            for (label, action) in [
                ("Open…", "open"),
                ("Save", "save"),
                ("Save as…", "saveAs"),
                ("Export…", "export"),
            ] {
                if ui
                    .add_enabled(app.file_actions.is_some(), egui::Button::new(label))
                    .clicked()
                {
                    menu::run_file_action(app, action);
                    ui.close();
                }
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icons::button(ui, Icon::Sidebar, false, "Hide sidebar").clicked() {
                app.show_layers = false;
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let place = if app.peers.is_empty() {
            "Local file"
        } else {
            "Shared room"
        };
        ui.label(egui::RichText::new(place).color(TOKENS.text_secondary));
    });
    ui.add_space(10.0);
}

pub fn right_header(app: &mut App, ui: &mut Ui) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let me = avatar(ui, app.session.user(), TOKENS.accent);
        widgets::menu(&me).show(|ui| {
            widgets::menu_item(
                ui,
                None,
                None,
                &format!("{} (You)", app.session.user()),
                "",
                false,
            );
            if app.peers.is_empty() {
                widgets::menu_item(ui, None, None, "Nobody else is here", "", false);
            }
            for peer in &app.peers {
                widgets::menu_item(ui, None, None, &peer.name, "", false);
            }
            widgets::menu_separator(ui);
            if widgets::menu_item(ui, None, None, "Share…", "", true).clicked() {
                menu::run_file_action(app, "collaborate");
                ui.close();
            }
        });
        for peer in app.peers.iter().take(3) {
            avatar(ui, &peer.name, peer.color);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::primary(ui, "Share").clicked() {
                menu::run_file_action(app, "collaborate");
            }
            let chev = icons::sized_button(ui, Icon::ChevronDown, false, "", vec2(14.0, 24.0));
            crate::widgets::menu(&chev).show(|ui| {
                if widgets::menu_item(ui, Some(true), Some(Icon::Play), "Present", "⌥⌘↩", true)
                    .clicked()
                {
                    crate::player::present(app);
                    ui.close();
                }
            });
            if icons::button(ui, Icon::Play, false, "Present (⌥⌘↩)").clicked() {
                crate::player::present(app);
            }
        });
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if app.placing_comment {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Comments").font(theme::strong()));
        } else {
            for (tab, label) in [
                (RightTab::Design, "Design"),
                (RightTab::Prototype, "Prototype"),
                (RightTab::Inspect, "Inspect"),
            ] {
                if widgets::tab(ui, label, app.right_tab == tab).clicked() {
                    app.right_tab = tab;
                    app.show_prototype = tab == RightTab::Prototype;
                }
            }
        }
        if ui.available_width() > 44.0 {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = format!("{:.0}%", app.camera.zoom * 100.0);
                let resp = widgets::dropdown_label(ui, &label, false);
                crate::widgets::menu(&resp).show(|ui| zoom_menu(app, ui));
            });
        }
    });
    ui.add_space(8.0);
}

fn avatar(ui: &mut Ui, name: &str, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    ui.painter().circle_filled(rect.center(), 12.0, color);
    let initial: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        theme::strong(),
        Color32::WHITE,
    );
    resp.tip(name)
}

pub fn zoom_menu(app: &mut App, ui: &mut Ui) {
    let key = ui.id().with("zoom_text");
    let mut text = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| format!("{:.0}%", app.camera.zoom * 100.0));
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(180.0));
    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Ok(pct) = text.trim().trim_end_matches('%').trim().parse::<f64>() {
            let size = app.canvas.last_size;
            app.camera.zoom_at(
                size / 2.0,
                (pct / 100.0).clamp(0.02, 256.0) / app.camera.zoom,
            );
        }
        ui.data_mut(|d| d.remove::<String>(key));
        ui.close();
    } else {
        ui.data_mut(|d| d.insert_temp(key, text));
    }
    widgets::menu_separator(ui);
    let zoom = app.camera.zoom;
    for (label, what, key) in [
        ("Zoom in", "in", "⌘+"),
        ("Zoom out", "out", "⌘-"),
        ("Zoom to fit", "fit", "⇧1"),
        ("Zoom to 50%", "50", ""),
        ("Zoom to 100%", "100", "⌘0"),
        ("Zoom to 200%", "200", ""),
    ] {
        let on = match what {
            "50" => (zoom - 0.5).abs() < 1e-6,
            "100" => (zoom - 1.0).abs() < 1e-6,
            "200" => (zoom - 2.0).abs() < 1e-6,
            _ => false,
        };
        if widgets::menu_item(ui, Some(on), None, label, key, true).clicked() {
            match what {
                "50" | "200" => {
                    let target = if what == "50" { 0.5 } else { 2.0 };
                    let size = app.canvas.last_size;
                    app.camera.zoom_at(size / 2.0, target / app.camera.zoom);
                }
                w => canvas::zoom_to(app, w),
            }
            ui.close();
        }
    }
    widgets::menu_separator(ui);
    if widgets::menu_item(
        ui,
        Some(app.show_prototype),
        None,
        "Prototype connections",
        "",
        true,
    )
    .clicked()
    {
        app.show_prototype = !app.show_prototype;
        ui.close();
    }
    if widgets::menu_item(ui, Some(app.show_comments), None, "Comments", "⇧C", true).clicked() {
        app.show_comments = !app.show_comments;
        ui.close();
    }
}

const GROUPS: &[&[ToolKind]] = &[
    &[ToolKind::Move, ToolKind::Hand],
    &[ToolKind::Frame],
    &[
        ToolKind::Rectangle,
        ToolKind::Line,
        ToolKind::Ellipse,
        ToolKind::Polygon,
        ToolKind::Star,
    ],
    &[ToolKind::Pen],
    &[ToolKind::Text],
];

pub fn tool_icon(t: ToolKind) -> Icon {
    match t {
        ToolKind::Move => Icon::Move,
        ToolKind::Hand => Icon::Hand,
        ToolKind::Frame => Icon::Frame,
        ToolKind::Rectangle => Icon::Rectangle,
        ToolKind::Ellipse => Icon::Ellipse,
        ToolKind::Line => Icon::Line,
        ToolKind::Polygon => Icon::Polygon,
        ToolKind::Star => Icon::Star,
        ToolKind::Pen => Icon::Pen,
        ToolKind::Text => Icon::Text,
    }
}

pub fn toolbar(app: &mut App, ctx: &egui::Context, canvas: Rect) {
    egui::Area::new(egui::Id::new("toolbar"))
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(canvas.center_bottom() - vec2(0.0, 12.0))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(TOKENS.panel)
                .corner_radius(12)
                .inner_margin(Margin::same(6))
                .stroke(egui::Stroke::new(1.0, TOKENS.border))
                .shadow(egui::Shadow {
                    offset: [0, 2],
                    blur: 10,
                    spread: 0,
                    color: Color32::from_black_alpha(30),
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (gi, group) in GROUPS.iter().enumerate() {
                            tool_group(app, ui, gi, group);
                            ui.add_space(2.0);
                        }
                        let placing = app.placing_comment;
                        if tool_button(ui, Icon::Comment, placing, "Comment (C)").clicked() {
                            app.placing_comment = !placing;
                            app.show_comments = true;
                        }
                        if tool_button(ui, Icon::Actions, app.palette.is_some(), "Actions (⌘K)")
                            .clicked()
                        {
                            app.palette = Some(menu::Palette::default());
                        }
                        ui.add_space(6.0);
                        let (r, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
                        ui.painter().rect_filled(r, 0.0, TOKENS.border);
                        ui.add_space(6.0);
                        let modes = [(Icon::Design, "Design"), (Icon::Prototype, "Prototype")];
                        let sel = match app.right_tab {
                            RightTab::Design => Some(0),
                            RightTab::Prototype => Some(1),
                            RightTab::Inspect => None,
                        };
                        if let Some(i) = widgets::segmented(ui, &modes, sel, 32.0) {
                            app.right_tab = if i == 0 {
                                RightTab::Design
                            } else {
                                RightTab::Prototype
                            };
                            app.show_prototype = i == 1;
                            app.show_design = true;
                        }
                    });
                });
        });
}

fn tool_button(ui: &mut Ui, icon: Icon, active: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    let p = ui.painter();
    let color = if active {
        p.rect_filled(rect, 6.0, TOKENS.accent);
        TOKENS.accent_text
    } else {
        if resp.hovered() {
            p.rect_filled(rect, 6.0, TOKENS.row_hover);
        }
        TOKENS.text
    };
    icons::paint(
        p,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        icon,
        color,
    );
    resp.tip(tip)
}

fn tool_group(app: &mut App, ui: &mut Ui, gi: usize, group: &[ToolKind]) {
    let shown = if group.contains(&app.tools.active) {
        app.tools.active
    } else {
        app.group_tool
            .get(gi)
            .copied()
            .filter(|t| group.contains(t))
            .unwrap_or(group[0])
    };
    let tip = if shown.shortcut().is_empty() {
        shown.label().to_owned()
    } else {
        format!("{} ({})", shown.label(), shown.shortcut())
    };
    let active = app.tools.active == shown && !app.placing_comment;
    if tool_button(ui, tool_icon(shown), active, &tip).clicked() {
        app.placing_comment = false;
        app.set_tool(shown);
    }
    if group.len() < 2 {
        return;
    }
    let chev = icons::sized_button(ui, Icon::ChevronDown, false, "", vec2(12.0, 32.0));
    crate::widgets::menu(&chev)
        .align(egui::RectAlign::TOP_START)
        .show(|ui| {
            for t in group {
                let on = *t == shown;
                let label = if *t == ToolKind::Hand {
                    "Hand tool"
                } else {
                    t.label()
                };
                if widgets::menu_item(ui, Some(on), Some(tool_icon(*t)), label, t.shortcut(), true)
                    .clicked()
                {
                    if let Some(slot) = app.group_tool.get_mut(gi) {
                        *slot = *t;
                    }
                    app.placing_comment = false;
                    app.set_tool(*t);
                    ui.close();
                }
            }
            if gi == 2
                && widgets::menu_item(ui, Some(false), Some(Icon::Image), "Image…", "⇧⌘K", true)
                    .clicked()
            {
                menu::run_file_action(app, "placeImage");
                ui.close();
            }
        });
}

pub fn help(app: &mut App, ctx: &egui::Context, canvas: Rect) {
    egui::Area::new(egui::Id::new("help"))
        .pivot(Align2::RIGHT_BOTTOM)
        .fixed_pos(canvas.right_bottom() - vec2(12.0, 12.0))
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
            let fill = if resp.hovered() {
                TOKENS.row_hover
            } else {
                TOKENS.panel
            };
            ui.painter().circle(
                rect.center(),
                16.0,
                fill,
                egui::Stroke::new(1.0, TOKENS.border),
            );
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
                Icon::Help,
                TOKENS.text,
            );
            crate::widgets::menu(&resp)
                .align(egui::RectAlign::TOP_END)
                .show(|ui| {
                    menu::help_menu(app, ui);
                });
        });
}

pub fn toast(app: &mut App, ctx: &egui::Context, canvas: Rect) {
    let now = ctx.input(|i| i.time);
    if app.status != app.toast_seen {
        app.toast_seen = app.status.clone();
        app.toast_since = now;
    }
    let Some((msg, is_error)) = app.status.clone() else {
        return;
    };
    if !is_error && now - app.toast_since > 4.0 {
        return;
    }
    if !is_error {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    egui::Area::new(egui::Id::new("toast"))
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(canvas.center_bottom() - vec2(0.0, 72.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let fill = if is_error {
                TOKENS.error
            } else {
                Color32::from_rgb(44, 44, 44)
            };
            egui::Frame::new()
                .fill(fill)
                .corner_radius(8)
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&msg).color(Color32::WHITE));
                        if is_error {
                            let (r, resp) =
                                ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                            icons::paint(ui.painter(), r, Icon::Close, Color32::WHITE);
                            if resp.clicked() {
                                app.status = None;
                            }
                        }
                    });
                });
        });
}

pub fn palette(app: &mut App, ctx: &egui::Context, canvas: Rect) {
    let Some(mut state) = app.palette.take() else {
        return;
    };
    let entries = menu::palette_entries(app);
    let q = state.query.to_lowercase();
    let matches: Vec<&(String, String, bool, String)> = entries
        .iter()
        .filter(|(label, ..)| q.is_empty() || label.to_lowercase().contains(&q))
        .take(12)
        .collect();
    let (up, down, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if down && state.selected + 1 < matches.len() {
        state.selected += 1;
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    state.selected = state.selected.min(matches.len().saturating_sub(1));
    let mut run: Option<(bool, String)> = None;
    let mut close = esc;
    let resp = egui::Area::new(egui::Id::new("palette"))
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(canvas.center_bottom() - vec2(0.0, 72.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(TOKENS.panel)
                .corner_radius(12)
                .inner_margin(Margin::same(8))
                .stroke(egui::Stroke::new(1.0, TOKENS.border))
                .shadow(egui::Shadow {
                    offset: [0, 4],
                    blur: 20,
                    spread: 0,
                    color: Color32::from_black_alpha(40),
                })
                .show(ui, |ui| {
                    ui.set_width(500.0);
                    widgets::field(ui, 500.0, |ui| {
                        icons::glyph(ui, Icon::Search, TOKENS.text_secondary);
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut state.query)
                                .frame(egui::Frame::NONE)
                                .hint_text("Search actions")
                                .desired_width(f32::INFINITY),
                        );
                        r.request_focus();
                        if r.changed() {
                            state.selected = 0;
                        }
                    });
                    ui.add_space(6.0);
                    if matches.is_empty() {
                        ui.label(
                            egui::RichText::new("No matching actions").color(TOKENS.text_secondary),
                        );
                    }
                    for (i, (label, key, is_cmd, id)) in matches.iter().enumerate() {
                        let (rect, r) = ui.allocate_exact_size(vec2(500.0, 28.0), Sense::click());
                        let on = i == state.selected || r.hovered();
                        if on {
                            ui.painter().rect_filled(rect, 6.0, TOKENS.row_hover);
                        }
                        let enabled = !*is_cmd || app.session.is_enabled(id);
                        let color = if enabled {
                            TOKENS.text
                        } else {
                            TOKENS.text_tertiary
                        };
                        ui.painter().text(
                            rect.left_center() + vec2(10.0, 0.0),
                            Align2::LEFT_CENTER,
                            label,
                            theme::body(),
                            color,
                        );
                        ui.painter().text(
                            rect.right_center() - vec2(10.0, 0.0),
                            Align2::RIGHT_CENTER,
                            key,
                            theme::body(),
                            TOKENS.text_secondary,
                        );
                        if (r.clicked() || (enter && i == state.selected)) && enabled {
                            run = Some((*is_cmd, id.clone()));
                        }
                    }
                });
        });
    if ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() {
        close = true;
    }
    if let Some((is_cmd, id)) = run {
        if is_cmd {
            menu::run_command(app, &id);
        } else {
            menu::run_ui_action(app, &id);
        }
        close = true;
    }
    if !close {
        app.palette = Some(state);
    }
}

pub fn shortcuts_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_shortcuts {
        return;
    }
    let mut open = true;
    let mut rows: Vec<(String, String)> = menu::palette_entries(app)
        .into_iter()
        .filter(|(_, k, ..)| !k.is_empty())
        .map(|(l, k, ..)| (l, k))
        .collect();
    for t in ToolKind::ALL {
        if !t.shortcut().is_empty() {
            rows.push((t.label().to_owned(), t.shortcut().to_owned()));
        }
    }
    rows.sort();
    rows.dedup();
    egui::Window::new("Keyboard shortcuts")
        .open(&mut open)
        .default_size([420.0, 520.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("shortcuts")
                    .striped(true)
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (l, k) in rows {
                            ui.label(l);
                            ui.label(egui::RichText::new(k).color(TOKENS.text_secondary));
                            ui.end_row();
                        }
                    });
            });
        });
    app.show_shortcuts = open;
}
