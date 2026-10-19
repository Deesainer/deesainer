use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span<'a> {
    Text(&'a str),
    Bold(&'a str),
    Code(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<'a> {
    Heading(u8, &'a str),
    Bullet(usize, &'a str),
    Numbered(usize, &'a str, &'a str),
    Para(&'a str),
    Code(Vec<&'a str>),
    Table(Vec<Vec<&'a str>>),
    Rule,
    Blank,
}

pub fn spans(line: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        let next_bold = rest.find("**");
        let next_code = rest.find('`');
        let (at, bold) = match (next_bold, next_code) {
            (Some(b), Some(c)) if b < c => (b, true),
            (Some(b), None) => (b, true),
            (_, Some(c)) => (c, false),
            (None, None) => {
                out.push(Span::Text(rest));
                break;
            }
        };
        let marker = if bold { "**" } else { "`" };
        let inner_start = at + marker.len();
        let Some(len) = rest.get(inner_start..).and_then(|r| r.find(marker)) else {
            out.push(Span::Text(rest));
            break;
        };
        if at > 0 {
            if let Some(text) = rest.get(..at) {
                out.push(Span::Text(text));
            }
        }
        if let Some(inner) = rest.get(inner_start..inner_start + len) {
            out.push(if bold {
                Span::Bold(inner)
            } else {
                Span::Code(inner)
            });
        }
        rest = rest.get(inner_start + len + marker.len()..).unwrap_or("");
    }
    out
}

fn list_item(line: &str) -> Option<Block<'_>> {
    let trimmed = line.trim_start();
    let indent = (line.len() - trimmed.len()) / 2;
    if let Some(item) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("• "))
    {
        return Some(Block::Bullet(indent, item));
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && digits < 4 {
        let (number, after) = trimmed.split_at(digits);
        if let Some(item) = after
            .strip_prefix(". ")
            .or_else(|| after.strip_prefix(") "))
        {
            return Some(Block::Numbered(indent, number, item));
        }
    }
    None
}

fn table_row(line: &str) -> Vec<&str> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

pub fn blocks(text: &str) -> Vec<Block<'_>> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            let mut code = Vec::new();
            for l in lines.by_ref() {
                if l.trim().starts_with("```") {
                    break;
                }
                code.push(l);
            }
            out.push(Block::Code(code));
        } else if trimmed.starts_with('|') {
            let mut rows = vec![table_row(trimmed)];
            while let Some(next) = lines.peek().filter(|l| l.trim().starts_with('|')) {
                let row = table_row(next);
                let separator = row
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')));
                if !separator {
                    rows.push(row);
                }
                lines.next();
            }
            out.push(Block::Table(rows));
        } else if trimmed.is_empty() {
            if out.last() != Some(&Block::Blank) {
                out.push(Block::Blank);
            }
        } else if matches!(trimmed, "---" | "***" | "___") {
            out.push(Block::Rule);
        } else if let Some(h) = trimmed.strip_prefix("### ") {
            out.push(Block::Heading(3, h));
        } else if let Some(h) = trimmed.strip_prefix("## ") {
            out.push(Block::Heading(2, h));
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            out.push(Block::Heading(1, h));
        } else if let Some(item) = list_item(line) {
            out.push(item);
        } else {
            out.push(Block::Para(line));
        }
    }
    out
}

pub fn hex_color(s: &str) -> Option<Color32> {
    let hex = s.trim().strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|p| u8::from_str_radix(p, 16).ok())
    };
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color32::from_rgba_unmultiplied(
        byte(0)?,
        byte(2)?,
        byte(4)?,
        a,
    ))
}

fn on(color: Color32) -> Color32 {
    let l =
        0.299 * f32::from(color.r()) + 0.587 * f32::from(color.g()) + 0.114 * f32::from(color.b());
    if l > 150.0 || color.a() < 110 {
        Color32::from_gray(25)
    } else {
        Color32::WHITE
    }
}

#[derive(Clone, Copy)]
struct Style {
    size: f32,
    color: Color32,
    bold: bool,
    has_strong: bool,
}

fn font(style: &Style, bold: bool) -> FontId {
    if (bold || style.bold) && style.has_strong {
        FontId::new(style.size, FontFamily::Name("strong".into()))
    } else {
        FontId::proportional(style.size)
    }
}

fn job(line: &str, style: &Style) -> LayoutJob {
    let mut job = LayoutJob::default();
    let strong_color = Color32::from_gray(20);
    for span in spans(line) {
        match span {
            Span::Text(t) => job.append(
                t,
                0.0,
                TextFormat {
                    font_id: font(style, false),
                    color: style.color,
                    ..Default::default()
                },
            ),
            Span::Bold(t) => job.append(
                t,
                0.0,
                TextFormat {
                    font_id: font(style, true),
                    color: strong_color,
                    ..Default::default()
                },
            ),
            Span::Code(t) => {
                let (background, color) = match hex_color(t) {
                    Some(c) => (c, on(c)),
                    None => (Color32::from_rgb(238, 240, 243), Color32::from_gray(40)),
                };
                job.append(
                    &format!("\u{a0}{t}\u{a0}"),
                    0.0,
                    TextFormat {
                        font_id: FontId::monospace(style.size - 1.0),
                        color,
                        background,
                        ..Default::default()
                    },
                );
            }
        }
    }
    job
}

pub fn dot(ui: &mut egui::Ui, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, height), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 2.5, color);
}

pub fn show(ui: &mut egui::Ui, text: &str) {
    let has_strong = ui.fonts(|f| {
        f.families()
            .iter()
            .any(|fam| *fam == FontFamily::Name("strong".into()))
    });
    let body = Style {
        size: 12.5,
        color: Color32::from_gray(50),
        bold: false,
        has_strong,
    };
    ui.spacing_mut().item_spacing.y = 3.0;
    for block in blocks(text) {
        match block {
            Block::Heading(level, h) => {
                ui.add_space(if level == 1 { 6.0 } else { 4.0 });
                let style = Style {
                    size: match level {
                        1 => 16.0,
                        2 => 14.0,
                        _ => 13.0,
                    },
                    color: Color32::from_gray(20),
                    bold: true,
                    has_strong,
                };
                ui.add(egui::Label::new(job(h, &style)).wrap().selectable(true));
            }
            Block::Bullet(indent, item) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(indent as f32 * 14.0);
                    dot(ui, Color32::from_gray(120), 17.0);
                    ui.add(egui::Label::new(job(item, &body)).wrap().selectable(true));
                });
            }
            Block::Numbered(indent, number, item) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(indent as f32 * 14.0);
                    let n = Style {
                        color: Color32::from_rgb(13, 120, 220),
                        bold: true,
                        ..body
                    };
                    ui.add(egui::Label::new(job(&format!("{number}."), &n)));
                    ui.add(egui::Label::new(job(item, &body)).wrap().selectable(true));
                });
            }
            Block::Para(p) => {
                ui.add(egui::Label::new(job(p, &body)).wrap().selectable(true));
            }
            Block::Code(lines) => {
                egui::Frame::new()
                    .fill(Color32::from_rgb(246, 247, 249))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(lines.join("\n"))
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_gray(50)),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    });
            }
            Block::Table(rows) => {
                egui::Grid::new(ui.next_auto_id())
                    .striped(true)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        for (i, row) in rows.iter().enumerate() {
                            let style = Style {
                                bold: i == 0,
                                ..body
                            };
                            for cell in row {
                                ui.add(egui::Label::new(job(cell, &style)).selectable(true));
                            }
                            ui.end_row();
                        }
                    });
            }
            Block::Rule => {
                ui.separator();
            }
            Block::Blank => ui.add_space(3.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_bold_and_code() {
        assert_eq!(
            spans("Navy **primary** `#0B1F3A` card"),
            vec![
                Span::Text("Navy "),
                Span::Bold("primary"),
                Span::Text(" "),
                Span::Code("#0B1F3A"),
                Span::Text(" card"),
            ]
        );
        assert_eq!(spans("a **b"), vec![Span::Text("a **b")]);
        assert_eq!(spans("€ `x"), vec![Span::Text("€ `x")]);
        assert_eq!(spans("**€ 24.562,80**"), vec![Span::Bold("€ 24.562,80")]);
    }

    #[test]
    fn blocks_of_a_plan() {
        let plan = "## Colori\n- Sfondo `#E8ECF2`\n  - nested\n1. **Status bar**: 9:41\n\n\n```\ncode\n```\n| a | b |\n|---|---|\n| 1 | 2 |\n---\ntext";
        assert_eq!(
            blocks(plan),
            vec![
                Block::Heading(2, "Colori"),
                Block::Bullet(0, "Sfondo `#E8ECF2`"),
                Block::Bullet(1, "nested"),
                Block::Numbered(0, "1", "**Status bar**: 9:41"),
                Block::Blank,
                Block::Code(vec!["code"]),
                Block::Table(vec![vec!["a", "b"], vec!["1", "2"]]),
                Block::Rule,
                Block::Para("text"),
            ]
        );
        assert_eq!(blocks("```\nx"), vec![Block::Code(vec!["x"])]);
    }

    #[test]
    fn hex_codes_become_swatches() {
        assert_eq!(hex_color("#0B1F3A"), Some(Color32::from_rgb(11, 31, 58)));
        assert_eq!(
            hex_color("#05966940"),
            Some(Color32::from_rgba_unmultiplied(5, 150, 105, 64))
        );
        assert_eq!(hex_color("#GGGGGG"), None);
        assert_eq!(hex_color("0B1F3A"), None);
        assert_eq!(on(Color32::from_rgb(11, 31, 58)), Color32::WHITE);
        assert_eq!(on(Color32::from_rgb(232, 236, 242)), Color32::from_gray(25));
    }
}
