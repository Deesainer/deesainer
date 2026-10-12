use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, Vec2};

pub struct Tokens {
    pub selection: Color32,
    pub hover: Color32,
    pub guide: Color32,
    pub marquee_fill: Color32,
    pub handle_fill: Color32,
    pub canvas_label: Color32,
    pub component: Color32,
    pub annotation: Color32,
    pub error: Color32,
    pub panel: Color32,
    pub canvas: Color32,
    pub border: Color32,
    pub field: Color32,
    pub field_hover: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub row_selected: Color32,
    pub row_hover: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub tile_active: Color32,
    pub menu_bg: Color32,
    pub menu_text: Color32,
    pub menu_text_weak: Color32,
    pub menu_separator: Color32,
}

pub const TOKENS: Tokens = Tokens {
    selection: Color32::from_rgb(13, 153, 255),
    hover: Color32::from_rgb(13, 153, 255),
    guide: Color32::from_rgb(242, 72, 120),
    marquee_fill: Color32::from_rgba_premultiplied(3, 40, 72, 28),
    handle_fill: Color32::WHITE,
    canvas_label: Color32::from_rgb(150, 150, 150),
    component: Color32::from_rgb(151, 71, 255),
    annotation: Color32::from_rgb(232, 89, 12),
    error: Color32::from_rgb(220, 60, 60),
    panel: Color32::WHITE,
    canvas: Color32::from_rgb(245, 245, 245),
    border: Color32::from_rgb(230, 230, 230),
    field: Color32::from_rgb(245, 245, 245),
    field_hover: Color32::from_rgb(236, 236, 236),
    text: Color32::from_rgb(30, 30, 30),
    text_secondary: Color32::from_rgb(110, 110, 110),
    text_tertiary: Color32::from_rgb(170, 170, 170),
    row_selected: Color32::from_rgb(229, 244, 255),
    row_hover: Color32::from_rgb(242, 242, 242),
    accent: Color32::from_rgb(13, 153, 255),
    accent_text: Color32::WHITE,
    tile_active: Color32::from_rgb(229, 244, 255),
    menu_bg: Color32::from_rgb(30, 30, 30),
    menu_text: Color32::WHITE,
    menu_text_weak: Color32::from_rgb(140, 140, 140),
    menu_separator: Color32::from_rgb(56, 56, 56),
};

pub const BODY: f32 = 11.0;
pub const STRONG: f32 = 11.0;
pub const TITLE: f32 = 13.0;

pub fn body() -> FontId {
    FontId::new(BODY, FontFamily::Proportional)
}

pub fn strong() -> FontId {
    FontId::new(STRONG, FontFamily::Name("strong".into()))
}

pub fn title() -> FontId {
    FontId::new(TITLE, FontFamily::Name("strong".into()))
}

pub fn color32(c: deesainer_color::Rgba) -> Color32 {
    let [r, g, b, a] = c.to_rgba8();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn rgba(c: Color32) -> deesainer_color::Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    deesainer_color::Rgba::from_rgba8([r, g, b, a])
}

#[cfg(not(target_arch = "wasm32"))]
fn find_font(names: &[&str]) -> Option<Vec<u8>> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("DEESAINER_FONTS_DIR") {
        dirs.push(d.into());
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        dirs.push(home.join("Library/Fonts"));
        dirs.push(home.join(".local/share/fonts"));
    }
    for d in [
        "/Library/Fonts",
        "/System/Library/Fonts",
        "/usr/share/fonts/truetype",
        "C:\\Windows\\Fonts",
    ] {
        dirs.push(d.into());
    }
    for name in names {
        for d in &dirs {
            if let Ok(bytes) = std::fs::read(d.join(name)) {
                return Some(bytes);
            }
        }
    }
    None
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let weighted = |bytes: Vec<u8>, wght: f32| {
            egui::FontData::from_owned(bytes).tweak(egui::epaint::text::FontTweak {
                coords: egui::epaint::text::VariationCoords::new([(b"wght", wght)]),
                ..Default::default()
            })
        };
        let regular = find_font(&[
            "InterVariable.ttf",
            "Inter-Regular.ttf",
            "Inter.ttf",
            "SFNS.ttf",
            "segoeui.ttf",
            "dejavu/DejaVuSans.ttf",
        ]);
        let bold = find_font(&[
            "Inter-SemiBold.ttf",
            "seguisb.ttf",
            "dejavu/DejaVuSans-Bold.ttf",
        ]);
        if let Some(bytes) = regular {
            let bold = bold.unwrap_or_else(|| bytes.clone());
            defs.font_data
                .insert("ui".into(), weighted(bytes, 450.0).into());
            defs.font_data
                .insert("ui-strong".into(), weighted(bold, 600.0).into());
            if let Some(f) = defs.families.get_mut(&FontFamily::Proportional) {
                f.insert(0, "ui".into());
            }
        }
    }
    let mut strong = Vec::new();
    if defs.font_data.contains_key("ui-strong") {
        strong.push("ui-strong".to_owned());
    }
    strong.extend(
        defs.families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    defs.families
        .insert(FontFamily::Name("strong".into()), strong);
    ctx.set_fonts(defs);
}

pub fn dark_popup(style: &mut egui::Style) {
    let v = &mut style.visuals;
    v.window_fill = TOKENS.menu_bg;
    v.panel_fill = TOKENS.menu_bg;
    v.window_stroke = Stroke::NONE;
    v.window_corner_radius = CornerRadius::same(13);
    v.menu_corner_radius = CornerRadius::same(13);
    v.override_text_color = Some(TOKENS.menu_text);
    v.weak_text_color = Some(TOKENS.menu_text_weak);
    v.selection.bg_fill = TOKENS.accent;
    v.extreme_bg_color = Color32::from_rgb(44, 44, 44);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, TOKENS.menu_separator);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TOKENS.menu_text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TOKENS.menu_text);
    v.widgets.inactive.bg_fill = Color32::from_rgb(70, 70, 70);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    for w in [
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_fill = TOKENS.accent;
        w.weak_bg_fill = TOKENS.accent;
        w.fg_stroke = Stroke::new(1.0, TOKENS.menu_text);
        w.bg_stroke = Stroke::NONE;
        w.corner_radius = CornerRadius::same(5);
    }
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.item_spacing = Vec2::new(6.0, 0.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.menu_width = 200.0;
}

pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (
                egui::TextStyle::Small,
                FontId::new(10.0, FontFamily::Proportional),
            ),
            (egui::TextStyle::Body, body()),
            (egui::TextStyle::Button, body()),
            (egui::TextStyle::Heading, title()),
            (
                egui::TextStyle::Monospace,
                FontId::new(11.0, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        style.spacing.button_padding = Vec2::new(6.0, 3.0);
        style.spacing.interact_size = Vec2::new(24.0, 24.0);
        style.spacing.window_margin = Margin::same(10);
        style.spacing.menu_margin = Margin::same(6);
        style.spacing.icon_width = 12.0;
        let v = &mut style.visuals;
        *v = egui::Visuals::light();
        v.panel_fill = TOKENS.panel;
        v.window_fill = TOKENS.panel;
        v.extreme_bg_color = TOKENS.field;
        v.faint_bg_color = TOKENS.field;
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::new(1.0, TOKENS.border);
        v.override_text_color = Some(TOKENS.text);
        v.selection.bg_fill = TOKENS.row_selected;
        v.selection.stroke = Stroke::new(1.0, TOKENS.accent);
        v.hyperlink_color = TOKENS.accent;
        let r = CornerRadius::same(5);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = r;
            w.fg_stroke = Stroke::new(1.0, TOKENS.text);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, TOKENS.border);
        v.widgets.noninteractive.bg_fill = TOKENS.panel;
        v.widgets.inactive.bg_fill = Color32::from_rgb(222, 222, 222);
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.bg_fill = TOKENS.row_hover;
        v.widgets.hovered.weak_bg_fill = TOKENS.row_hover;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.active.bg_fill = TOKENS.field_hover;
        v.widgets.active.weak_bg_fill = TOKENS.field_hover;
        v.widgets.active.bg_stroke = Stroke::NONE;
        v.widgets.open.weak_bg_fill = TOKENS.row_hover;
        v.popup_shadow = egui::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(40),
        };
        v.window_shadow = v.popup_shadow;
    });
}
