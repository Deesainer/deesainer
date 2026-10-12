use egui::{vec2, Align, Layout, Rect, Response, Sense, Stroke, Ui, UiBuilder, Vec2};

use crate::icons::{self, Icon};
use crate::theme::{self, TOKENS};

pub trait Tip {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self;
}

impl Tip for Response {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self {
        let mut t = egui::containers::Tooltip::for_enabled(&self);
        t.popup = t.popup.style(theme::dark_popup);
        let text = text.into();
        t.show(|ui| {
            ui.style_mut().spacing.window_margin = egui::Margin::symmetric(8, 4);
            ui.label(text);
        });
        self
    }
}

pub fn double_or_more(resp: &Response) -> bool {
    resp.double_clicked() || resp.triple_clicked()
}

pub fn menu(resp: &Response) -> egui::Popup<'_> {
    egui::Popup::menu(resp).style(theme::dark_popup)
}

pub fn context_menu<R>(resp: &Response, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    egui::Popup::context_menu(resp)
        .style(theme::dark_popup)
        .show(add)
        .map(|r| r.inner)
}

pub fn menu_item(
    ui: &mut Ui,
    check: Option<bool>,
    icon: Option<Icon>,
    label: &str,
    shortcut: &str,
    enabled: bool,
) -> Response {
    let font = theme::strong();
    let label_g = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), TOKENS.menu_text);
    let key_g =
        ui.painter()
            .layout_no_wrap(shortcut.to_owned(), theme::body(), TOKENS.menu_text_weak);
    let lead = if check.is_some() { 20.0 } else { 0.0 } + if icon.is_some() { 22.0 } else { 0.0 };
    let min_w = 8.0 + lead + label_g.size().x + 24.0 + key_g.size().x + 8.0;
    let key = ui.id().with("menu_row_width");
    let prev = ui.data(|d| d.get_temp::<f32>(key)).unwrap_or(0.0);
    let w = min_w.max(180.0).max(prev);
    ui.data_mut(|d| d.insert_temp(key, w));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW), sense);
    let hovered = enabled && resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 5.0, TOKENS.accent);
    }
    let fg = if enabled {
        TOKENS.menu_text
    } else {
        TOKENS.menu_text_weak
    };
    let mut x = rect.min.x + 8.0;
    if let Some(on) = check {
        if on {
            ui.painter().text(
                egui::pos2(x + 5.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                "✓",
                theme::body(),
                fg,
            );
        }
        x += 20.0;
    }
    if let Some(i) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), Vec2::splat(14.0)),
            i,
            fg,
        );
        x += 22.0;
    }
    ui.painter().galley(
        egui::pos2(x, rect.center().y - label_g.size().y / 2.0),
        label_g,
        fg,
    );
    let key_color = if hovered {
        TOKENS.menu_text
    } else {
        TOKENS.menu_text_weak
    };
    ui.painter().galley(
        egui::pos2(
            rect.max.x - 8.0 - key_g.size().x,
            rect.center().y - key_g.size().y / 2.0,
        ),
        key_g,
        key_color,
    );
    resp
}

pub fn menu_separator(ui: &mut Ui) {
    ui.add_space(4.0);
    let w = ui
        .data(|d| d.get_temp::<f32>(ui.id().with("menu_row_width")))
        .unwrap_or(180.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, TOKENS.menu_separator);
    ui.add_space(4.0);
}

pub const SIDEBAR_LEFT: &str = "right-sidebar-left";

pub const PAD: f32 = 12.0;
pub const ROW: f32 = 24.0;

pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    let r = Rect::from_min_max(rect.min - vec2(PAD, 0.0), rect.max + vec2(PAD, 0.0));
    ui.painter().rect_filled(r, 0.0, TOKENS.border);
}

pub fn section(
    ui: &mut Ui,
    title: &str,
    actions: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) {
    divider(ui);
    header(ui, title, actions);
    body(ui);
    ui.add_space(8.0);
}

pub fn header(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    header_styled(ui, title, theme::strong(), TOKENS.text, 32.0, actions);
}

pub fn header_empty(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    header_styled(
        ui,
        title,
        theme::strong(),
        TOKENS.text_secondary,
        40.0,
        actions,
    );
}

pub fn title_row(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    header_styled(ui, title, theme::title(), TOKENS.text, 44.0, actions);
}

fn header_styled(
    ui: &mut Ui,
    title: &str,
    font: egui::FontId,
    color: egui::Color32,
    height: f32,
    actions: impl FnOnce(&mut Ui),
) {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(egui::RichText::new(title).font(font).color(color));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                actions(ui);
            });
        },
    );
}

pub fn field<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    ui.painter().rect_filled(rect, 5.0, TOKENS.field);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 4.0;
    let r = add(&mut child);
    if resp.contains_pointer() {
        ui.painter().rect_stroke(
            rect,
            5.0,
            Stroke::new(1.0, TOKENS.border),
            egui::StrokeKind::Inside,
        );
    }
    r
}

pub enum Lead<'a> {
    Text(&'a str),
    Icon(Icon),
    None,
}

pub fn number(
    ui: &mut Ui,
    lead: Lead<'_>,
    value: &mut f64,
    speed: f64,
    suffix: &str,
    width: f32,
) -> Response {
    field(ui, width, |ui| {
        match lead {
            Lead::Text(t) => {
                ui.label(egui::RichText::new(t).color(TOKENS.text_secondary));
            }
            Lead::Icon(i) => {
                icons::glyph(ui, i, TOKENS.text_secondary);
            }
            Lead::None => {}
        }
        ui.spacing_mut().interact_size.x = 0.0;
        let suffix = suffix.to_owned();
        ui.add(
            egui::DragValue::new(value)
                .speed(speed)
                .custom_formatter(move |v, _| format!("{}{suffix}", trim(v)))
                .custom_parser(|s| s.trim().trim_end_matches(['%', '°']).trim().parse().ok()),
        )
    })
}

pub fn number_mixed(
    ui: &mut Ui,
    lead: &str,
    value: Option<f64>,
    fallback: f64,
    width: f32,
) -> (Response, Option<f64>) {
    let key = ui.id().with(("mixed", lead));
    let icon = match lead {
        "rotation" => Some(Icon::Angle),
        "opacity" => Some(Icon::Opacity),
        "radius" => Some(Icon::Radius),
        "weight" => Some(Icon::Minus),
        _ => None,
    };
    let mut v: f64 = ui
        .data(|d| d.get_temp(key))
        .unwrap_or(value.unwrap_or(fallback));
    if !ui.data(|d| d.get_temp::<bool>(key.with("active")).unwrap_or(false)) {
        v = value.unwrap_or(fallback);
    }
    let mixed = value.is_none();
    let resp = field(ui, width, |ui| {
        match icon {
            Some(i) => {
                icons::glyph(ui, i, TOKENS.text_secondary);
            }
            None => {
                ui.label(egui::RichText::new(lead).color(TOKENS.text_secondary));
            }
        }
        ui.spacing_mut().interact_size.x = 0.0;
        ui.add(
            egui::DragValue::new(&mut v)
                .speed(1.0)
                .custom_formatter(move |x, _| if mixed { "Mixed".to_owned() } else { trim(x) })
                .custom_parser(|s| s.trim().trim_end_matches(['%', '°']).trim().parse().ok()),
        )
    });
    let active = resp.dragged() || resp.has_focus();
    ui.data_mut(|d| {
        d.insert_temp(key, v);
        d.insert_temp(key.with("active"), active);
    });
    let changed = resp.changed();
    (resp, changed.then_some(v))
}

pub fn trim(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let s = format!("{r:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

pub fn select(ui: &mut Ui, width: f32, text: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
    let fill = if resp.hovered() {
        TOKENS.field_hover
    } else {
        TOKENS.field
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    let clip = Rect::from_min_max(rect.min, rect.max - vec2(24.0, 0.0));
    ui.painter().with_clip_rect(clip).text(
        rect.left_center() + vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        theme::body(),
        TOKENS.text,
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.right_center() - vec2(12.0, 0.0), Vec2::splat(12.0)),
        Icon::ChevronDown,
        TOKENS.text_secondary,
    );
    resp
}

pub fn choice(
    ui: &mut Ui,
    width: f32,
    current: &str,
    options: &[(&'static str, &'static str)],
) -> Option<&'static str> {
    let label = options
        .iter()
        .find(|(v, _)| *v == current)
        .map_or(current, |(_, l)| *l);
    let resp = select(ui, width, label);
    let mut picked = None;
    menu(&resp).show(|ui| {
        ui.set_min_width(width);
        for (v, l) in options {
            if menu_item(ui, Some(*v == current), None, l, "", true).clicked() {
                picked = Some(*v);
                ui.close();
            }
        }
    });
    picked.filter(|p| *p != current)
}

pub fn align_grid(
    ui: &mut Ui,
    width: f32,
    active: (usize, usize),
    horizontal: bool,
) -> Option<(usize, usize)> {
    let h = 52.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    ui.painter().rect_filled(rect, 5.0, TOKENS.field);
    let cw = rect.width() / 3.0;
    let ch = rect.height() / 3.0;
    let mut clicked = None;
    for row in 0..3 {
        for col in 0..3 {
            let cell = Rect::from_min_size(
                rect.min + vec2(cw * col as f32, ch * row as f32),
                vec2(cw, ch),
            );
            let resp = ui.interact(cell, ui.id().with(("align-cell", col, row)), Sense::click());
            let c = cell.center();
            if (col, row) == active {
                let color = TOKENS.accent;
                for k in [-1.0f32, 0.0, 1.0] {
                    let (a, b) = if horizontal {
                        let x = c.x + k * 4.0;
                        let len = if k == 0.0 { 9.0 } else { 6.0 };
                        (
                            egui::pos2(x, c.y - len / 2.0),
                            egui::pos2(x, c.y + len / 2.0),
                        )
                    } else {
                        let y = c.y + k * 4.0;
                        let len = if k == 0.0 { 9.0 } else { 6.0 };
                        (
                            egui::pos2(c.x - len / 2.0, y),
                            egui::pos2(c.x + len / 2.0, y),
                        )
                    };
                    ui.painter().line_segment([a, b], Stroke::new(2.0, color));
                }
            } else {
                let color = if resp.hovered() {
                    TOKENS.text_secondary
                } else {
                    TOKENS.text_tertiary
                };
                ui.painter().circle_filled(c, 1.5, color);
            }
            if resp.clicked() {
                clicked = Some((col, row));
            }
        }
    }
    clicked
}

pub fn half(ui: &Ui) -> f32 {
    ((ui.available_width() - 8.0 - 32.0) / 2.0).floor()
}

pub fn segmented(
    ui: &mut Ui,
    items: &[(Icon, &str)],
    selected: Option<usize>,
    cell: f32,
) -> Option<usize> {
    let w = cell * items.len() as f32 + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, ROW), Sense::hover());
    ui.painter().rect_filled(rect, 5.0, TOKENS.field);
    let mut clicked = None;
    for (i, (icon, tip)) in items.iter().enumerate() {
        let r = Rect::from_min_size(
            rect.min + vec2(2.0 + cell * i as f32, 2.0),
            vec2(cell, ROW - 4.0),
        );
        let resp = ui
            .interact(r, ui.id().with(("seg", tip, i)), Sense::click())
            .tip(*tip);
        let on = selected == Some(i);
        if on {
            ui.painter().rect(
                r,
                4.0,
                TOKENS.panel,
                Stroke::new(1.0, TOKENS.border),
                egui::StrokeKind::Inside,
            );
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, TOKENS.field_hover);
        }
        let color = if on {
            TOKENS.text
        } else {
            TOKENS.text_secondary
        };
        icons::paint(
            ui.painter(),
            Rect::from_center_size(r.center(), Vec2::splat(14.0)),
            *icon,
            color,
        );
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

pub fn tab(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        theme::strong(),
        if selected {
            TOKENS.text
        } else {
            TOKENS.text_secondary
        },
    );
    let size = galley.size() + vec2(12.0, 0.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(size.x, ROW), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 5.0, TOKENS.field);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, TOKENS.row_hover);
    }
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, TOKENS.text);
    resp
}

pub fn dropdown_label(ui: &mut Ui, label: &str, strong: bool) -> Response {
    let font = if strong {
        theme::strong()
    } else {
        theme::body()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, TOKENS.text);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0, ROW), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, TOKENS.row_hover);
    }
    let pos = rect.left_center() + vec2(4.0, -galley.size().y / 2.0);
    ui.painter().galley(pos, galley, TOKENS.text);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.right_center() - vec2(10.0, 0.0), Vec2::splat(12.0)),
        Icon::ChevronDown,
        TOKENS.text_secondary,
    );
    resp
}

pub fn primary(ui: &mut Ui, label: &str) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::strong(), TOKENS.accent_text);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, ROW), Sense::click());
    let fill = if resp.hovered() {
        TOKENS.accent.gamma_multiply(0.9)
    } else {
        TOKENS.accent
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        TOKENS.accent_text,
    );
    resp
}

pub fn swatch(ui: &mut Ui, color: egui::Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
    let p = ui.painter();
    if color.a() < 255 {
        p.rect_filled(rect, 2.0, egui::Color32::WHITE);
        let h = rect.width() / 2.0;
        for (dx, dy) in [(0.0, 0.0), (h, h)] {
            p.rect_filled(
                Rect::from_min_size(rect.min + vec2(dx, dy), Vec2::splat(h)),
                0.0,
                egui::Color32::from_gray(204),
            );
        }
    }
    p.rect_filled(rect, 2.0, color);
    p.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, egui::Color32::from_black_alpha(25)),
        egui::StrokeKind::Inside,
    );
    resp
}

pub fn paint_value(
    ui: &mut Ui,
    color: egui::Color32,
    label: &str,
    opacity_pct: &mut f64,
    width: f32,
) -> (Response, bool) {
    ui.spacing_mut().item_spacing.x = 1.0;
    let pct_w = 52.0;
    let resp = field(ui, (width - pct_w - 1.0).max(40.0), |ui| {
        let r = swatch(ui, color);
        ui.label(label);
        r
    });
    let oc = number(ui, Lead::None, opacity_pct, 0.5, " %", pct_w).changed();
    ui.spacing_mut().item_spacing.x = 4.0;
    (resp, oc)
}

pub fn color_picker(ui: &mut Ui, color: &mut egui::Color32) -> bool {
    use egui::ecolor::Hsva;
    let mut changed = false;
    let w = 216.0;
    let hue_key = ui.id().with("picker-hue");
    let mut hsva = Hsva::from(*color);
    if hsva.s < 1e-4 || hsva.v < 1e-4 {
        if let Some(h) = ui.data(|d| d.get_temp::<f32>(hue_key)) {
            hsva.h = h;
        }
    }
    let (sq, resp) = ui.allocate_exact_size(vec2(w, 160.0), Sense::click_and_drag());
    let hue = egui::Color32::from(Hsva::new(hsva.h, 1.0, 1.0, 1.0));
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(sq.left_top(), egui::Color32::WHITE);
    mesh.colored_vertex(sq.right_top(), hue);
    mesh.colored_vertex(sq.right_bottom(), hue);
    mesh.colored_vertex(sq.left_bottom(), egui::Color32::WHITE);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
    let mut shade = egui::Mesh::default();
    shade.colored_vertex(sq.left_top(), egui::Color32::TRANSPARENT);
    shade.colored_vertex(sq.right_top(), egui::Color32::TRANSPARENT);
    shade.colored_vertex(sq.right_bottom(), egui::Color32::BLACK);
    shade.colored_vertex(sq.left_bottom(), egui::Color32::BLACK);
    shade.add_triangle(0, 1, 2);
    shade.add_triangle(0, 2, 3);
    ui.painter().add(shade);
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on() {
            hsva.s = ((p.x - sq.left()) / sq.width()).clamp(0.0, 1.0);
            hsva.v = 1.0 - ((p.y - sq.top()) / sq.height()).clamp(0.0, 1.0);
            changed = true;
        }
    }
    let knob = egui::pos2(
        sq.left() + hsva.s * sq.width(),
        sq.top() + (1.0 - hsva.v) * sq.height(),
    );
    ui.painter().circle(
        knob,
        6.0,
        egui::Color32::from(Hsva::new(hsva.h, hsva.s, hsva.v, 1.0)),
        Stroke::new(2.0, egui::Color32::WHITE),
    );
    ui.add_space(8.0);
    let (hr, hresp) = ui.allocate_exact_size(vec2(w, 12.0), Sense::click_and_drag());
    let mut hm = egui::Mesh::default();
    let steps = 12;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let c = egui::Color32::from(Hsva::new(t, 1.0, 1.0, 1.0));
        let x = hr.left() + t * hr.width();
        hm.colored_vertex(egui::pos2(x, hr.top()), c);
        hm.colored_vertex(egui::pos2(x, hr.bottom()), c);
        if i > 0 {
            let b = (i as u32) * 2;
            hm.add_triangle(b - 2, b - 1, b);
            hm.add_triangle(b - 1, b + 1, b);
        }
    }
    ui.painter().add(hm);
    if let Some(p) = hresp.interact_pointer_pos() {
        if hresp.is_pointer_button_down_on() || hresp.clicked() {
            hsva.h = ((p.x - hr.left()) / hr.width()).clamp(0.0, 0.9999);
            changed = true;
        }
    }
    let hx = hr.left() + hsva.h * hr.width();
    ui.painter().circle(
        egui::pos2(hx, hr.center().y),
        7.0,
        hue,
        Stroke::new(2.0, egui::Color32::WHITE),
    );
    ui.add_space(8.0);
    let (ar, aresp) = ui.allocate_exact_size(vec2(w, 12.0), Sense::click_and_drag());
    let cell = 6.0;
    let mut x = ar.left();
    let mut k = 0;
    while x < ar.right() {
        for (row, y) in [ar.top(), ar.top() + cell].into_iter().enumerate() {
            let c = if (k + row) % 2 == 0 {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(204)
            };
            ui.painter().rect_filled(
                Rect::from_min_size(egui::pos2(x, y), vec2(cell.min(ar.right() - x), cell)),
                0.0,
                c,
            );
        }
        x += cell;
        k += 1;
    }
    let opaque = egui::Color32::from(Hsva::new(hsva.h, hsva.s, hsva.v, 1.0));
    let mut am = egui::Mesh::default();
    am.colored_vertex(ar.left_top(), egui::Color32::TRANSPARENT);
    am.colored_vertex(ar.right_top(), opaque);
    am.colored_vertex(ar.right_bottom(), opaque);
    am.colored_vertex(ar.left_bottom(), egui::Color32::TRANSPARENT);
    am.add_triangle(0, 1, 2);
    am.add_triangle(0, 2, 3);
    ui.painter().add(am);
    if let Some(p) = aresp.interact_pointer_pos() {
        if aresp.is_pointer_button_down_on() || aresp.clicked() {
            hsva.a = ((p.x - ar.left()) / ar.width()).clamp(0.0, 1.0);
            changed = true;
        }
    }
    let ax = ar.left() + hsva.a * ar.width();
    ui.painter().circle(
        egui::pos2(ax, ar.center().y),
        7.0,
        opaque,
        Stroke::new(2.0, egui::Color32::WHITE),
    );
    ui.add_space(10.0);
    if changed {
        ui.data_mut(|d| d.insert_temp(hue_key, hsva.h));
        *color = egui::Color32::from(hsva);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new("Hex").color(TOKENS.text_secondary));
        let key = ui.id().with("picker-hex");
        let mut text = ui
            .data(|d| d.get_temp::<String>(key))
            .unwrap_or_else(|| hex(*color));
        let r = field(ui, 110.0, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .frame(egui::Frame::NONE)
                    .desired_width(96.0),
            )
        });
        if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, text.clone()));
        } else {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        if r.lost_focus() {
            let t = text.trim().trim_start_matches('#');
            if t.len() == 6 {
                if let Ok(v) = u32::from_str_radix(t, 16) {
                    let a = color.a();
                    *color = egui::Color32::from_rgba_unmultiplied(
                        (v >> 16) as u8,
                        (v >> 8) as u8,
                        v as u8,
                        a,
                    );
                    changed = true;
                }
            }
        }
        let mut pct = (f64::from(color.a()) / 2.55).round();
        if number(ui, Lead::None, &mut pct, 0.5, " %", 64.0).changed() {
            let [r, g, b, _] = color.to_srgba_unmultiplied();
            *color = egui::Color32::from_rgba_unmultiplied(
                r,
                g,
                b,
                (pct.clamp(0.0, 100.0) * 2.55).round() as u8,
            );
            changed = true;
        }
    });
    changed
}

pub fn hex(color: egui::Color32) -> String {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    format!("{r:02X}{g:02X}{b:02X}")
}

pub fn color_pct_field(
    ui: &mut Ui,
    color: &mut egui::Color32,
    opacity_pct: &mut f64,
    width: f32,
) -> (bool, bool) {
    let label = hex(*color);
    let (resp, oc) = paint_value(ui, *color, &label, opacity_pct, width);
    let cc = popover(&resp, "")
        .show(|ui| {
            egui::color_picker::color_picker_color32(
                ui,
                color,
                egui::color_picker::Alpha::OnlyBlend,
            )
        })
        .is_some_and(|r| r.inner);
    (cc, oc)
}

pub fn popover<'a>(resp: &Response, title: &'a str) -> egui::Popup<'a> {
    let _ = title;
    let left = resp
        .ctx
        .data(|d| d.get_temp::<f32>(egui::Id::new(SIDEBAR_LEFT)))
        .filter(|l| *l <= resp.rect.left() + 1.0)
        .unwrap_or(resp.rect.left());
    let anchor = Rect::from_min_max(
        egui::pos2(left, resp.rect.top()),
        egui::pos2(left, resp.rect.bottom()),
    );
    egui::Popup::from_toggle_button_response(resp)
        .anchor(anchor)
        .align(egui::RectAlign::LEFT_START)
        .gap(8.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(
            egui::Frame::new()
                .fill(TOKENS.panel)
                .corner_radius(12)
                .stroke(Stroke::new(1.0, TOKENS.border))
                .inner_margin(egui::Margin::same(12))
                .shadow(egui::Shadow {
                    offset: [0, 4],
                    blur: 20,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(40),
                }),
        )
}

pub fn popover_title(ui: &mut Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).font(theme::strong()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = icons::button(ui, Icon::Close, false, "").clicked();
        });
    });
    ui.add_space(4.0);
    close
}

pub fn labelled<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, ROW), Sense::hover());
        ui.painter().text(
            r.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            theme::body(),
            TOKENS.text_secondary,
        );
        add(ui)
    })
    .inner
}
