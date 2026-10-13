use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use cosmic_text::{
    Align, Attrs, Buffer, CacheKeyFlags, Family, FontSystem, Metrics, Shaping, Style, SwashCache,
    Weight, Wrap,
};
use deesainer_doc::{
    FillRule, Node, PathCmd, TextAlignH, TextAlignV, TextAutoResize, TextCase, TextData,
    TextDecoration, TextList, TextStyle, VectorPath,
};
use deesainer_geom::{Point, Rect};

mod editing;
pub use editing::{EditAction, EditGeometry, TextEditor, TextMotion};

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
    scale: swash::scale::ScaleContext,
    usable: bool,
    layouts: HashMap<u64, TextLayout>,
}

fn fonts() -> &'static Mutex<Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut system = FontSystem::new();
        if let Ok(dir) = std::env::var("DEESAINER_FONTS_DIR") {
            system.db_mut().load_fonts_dir(dir);
        }
        drop_shadowing_faces(system.db_mut());
        let usable = has_fonts(&system);
        Mutex::new(Fonts {
            usable,
            system,
            cache: SwashCache::new(),
            scale: swash::scale::ScaleContext::new(),
            layouts: HashMap::new(),
        })
    })
}

fn has_fonts(system: &FontSystem) -> bool {
    system.db().faces().next().is_some()
}

fn drop_shadowing_faces(db: &mut cosmic_text::fontdb::Database) {
    let decorative = |ps: &str| {
        let ps = ps.to_ascii_lowercase();
        ["ornament", "dingbat"].iter().any(|w| ps.contains(w))
    };
    let key = |f: &cosmic_text::fontdb::FaceInfo| {
        (
            f.families
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_default(),
            f.weight.0,
            format!("{:?}{:?}", f.style, f.stretch),
        )
    };
    let text_faces: std::collections::HashSet<_> = db
        .faces()
        .filter(|f| !decorative(&f.post_script_name))
        .map(key)
        .collect();
    let doomed: Vec<_> = db
        .faces()
        .filter(|f| decorative(&f.post_script_name) && text_faces.contains(&key(f)))
        .map(|f| f.id)
        .collect();
    for id in doomed {
        db.remove_face(id);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub top: f64,
    pub height: f64,
    pub baseline: f64,
    pub left: f64,
    pub width: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    pub width: f64,
    pub height: f64,
    pub lines: Vec<Line>,
    pub outline: VectorPath,
    pub colored: Vec<(deesainer_doc::Paint, VectorPath)>,
}

fn hash_key(data: &TextData, width: Option<f64>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{data:?}").hash(&mut h);
    width.map(f64::to_bits).hash(&mut h);
    h.finish()
}

fn align(a: TextAlignH) -> Align {
    match a {
        TextAlignH::Left => Align::Left,
        TextAlignH::Center => Align::Center,
        TextAlignH::Right => Align::Right,
        TextAlignH::Justified => Align::Justified,
    }
}

fn attrs_for(style: &TextStyle) -> Attrs<'_> {
    let mut features = cosmic_text::FontFeatures::new();
    for f in style.features.iter().filter(|f| f.is_valid()) {
        let b = f.tag.as_bytes();
        features.set(
            cosmic_text::FeatureTag::new(&[b[0], b[1], b[2], b[3]]),
            f.value,
        );
    }
    Attrs::new()
        .font_features(features)
        .family(Family::Name(&style.font_family))
        .weight(Weight(style.font_weight.clamp(1, 1000)))
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        })
        .letter_spacing((style.letter_spacing / 100.0) as f32)
        .metrics(Metrics::new(
            style.font_size.clamp(0.5, 10_000.0) as f32,
            style.line_height_px().clamp(0.1, 100_000.0) as f32,
        ))
        .cache_key_flags(CacheKeyFlags::DISABLE_HINTING)
}

fn display_offsets(data: &TextData) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut shown = 0;
    let mut word_start = true;
    for (source, c) in data.content.char_indices() {
        out.push((source, shown));
        shown += match data.style.case {
            TextCase::Original => c.len_utf8(),
            TextCase::Upper => c.to_uppercase().map(char::len_utf8).sum::<usize>(),
            TextCase::Lower => c.to_lowercase().map(char::len_utf8).sum::<usize>(),
            TextCase::Title if word_start && c.is_alphabetic() => {
                c.to_uppercase().map(char::len_utf8).sum::<usize>()
            }
            TextCase::Title => c.len_utf8(),
        };
        if data.style.case == TextCase::Title {
            word_start = c.is_whitespace();
        }
    }
    out.push((data.content.len(), shown));
    out
}

pub fn layout(data: &TextData, width: Option<f64>) -> TextLayout {
    let width = width.filter(|w| w.is_finite() && *w >= 0.0);
    let key = hash_key(data, width);
    let Ok(mut guard) = fonts().lock() else {
        return fallback(data, width);
    };
    layout_with(&mut guard, key, data, width)
}

fn layout_with(f: &mut Fonts, key: u64, data: &TextData, width: Option<f64>) -> TextLayout {
    if !f.usable {
        return fallback(data, width);
    }
    if let Some(hit) = f.layouts.get(&key) {
        return hit.clone();
    }
    let result = layout_uncached(f, data, width);
    if f.layouts.len() > 4096 {
        f.layouts.clear();
    }
    f.layouts.insert(key, result.clone());
    result
}

fn fallback(data: &TextData, width: Option<f64>) -> TextLayout {
    let lh = data.style.line_height_px();
    let paragraphs: Vec<&str> = data.content.split('\n').collect();
    let w = width.unwrap_or_else(|| {
        paragraphs
            .iter()
            .map(|p| p.chars().count())
            .max()
            .unwrap_or(0) as f64
            * data.style.font_size
            * 0.55
    });
    TextLayout {
        width: w,
        height: paragraphs.len() as f64 * lh,
        lines: Vec::new(),
        outline: VectorPath::default(),
        colored: Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paragraphs {
    pub indent: f64,
    pub spacing: f64,
    pub items: Vec<(f64, Option<(f64, String)>)>,
}

impl Paragraphs {
    pub fn indent_of(&self, i: usize) -> f64 {
        self.items.get(i).map_or(0.0, |item| item.0)
    }
}

pub fn paragraphs(data: &TextData) -> Paragraphs {
    const LEVELS: usize = TextData::MAX_LIST_LEVEL as usize + 1;
    let size = data.style.font_size.clamp(0.5, 10_000.0);
    let step = (size * 1.4).ceil();
    let digits = data.paragraph_count().to_string().len() as f64;
    let number_width = (size * (0.6 * digits + 1.0)).ceil();
    let mut counters = [0usize; LEVELS];
    let mut items = Vec::new();
    for (i, text) in data.content.split('\n').enumerate() {
        let f = data.paragraph_format(i);
        let level = usize::from(f.level).min(LEVELS - 1);
        let left = level as f64 * step;
        let item = match f.list {
            TextList::None => {
                counters[level..].fill(0);
                (left, None)
            }
            list => {
                let width = if list == TextList::Number {
                    number_width
                } else {
                    step
                };
                let shown = if text.trim().is_empty() {
                    None
                } else {
                    counters[level + 1..].fill(0);
                    let n = if list == TextList::Number {
                        counters[level] += 1;
                        counters[level]
                    } else {
                        counters[level] = 0;
                        0
                    };
                    Some((left, marker(list, level, n)))
                };
                (left + width, shown)
            }
        };
        items.push(item);
    }
    let indent = items.iter().map(|i| i.0).fold(0.0, f64::max);
    let spacing = if data.paragraph_spacing.is_finite() {
        data.paragraph_spacing.clamp(0.0, 100_000.0)
    } else {
        0.0
    };
    Paragraphs {
        indent,
        spacing,
        items,
    }
}

fn marker(list: TextList, level: usize, n: usize) -> String {
    match list {
        TextList::None => String::new(),
        TextList::Bullet => ["•", "◦", "▪"][level % 3].to_owned(),
        TextList::Number => match level % 3 {
            0 => format!("{n}."),
            1 => format!("{}.", letters(n)),
            _ => format!("{}.", roman(n)),
        },
    }
}

fn letters(mut n: usize) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn roman(mut n: usize) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const DIGITS: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, d) in DIGITS {
        while n >= v {
            out.push_str(d);
            n -= v;
        }
    }
    out
}

fn varied_glyph(
    system: &mut FontSystem,
    scale: &mut swash::scale::ScaleContext,
    glyph: &cosmic_text::LayoutGlyph,
    variations: &[deesainer_doc::FontVariation],
) -> Option<(Vec<cosmic_text::Command>, f32)> {
    use swash::zeno::PathData;
    let font = system.get_font(glyph.font_id, glyph.font_weight)?;
    let swash_font = font.as_swash();
    let axes = swash_font.variations();
    let tag = |t: &str| swash::tag_from_str_lossy(t);
    if !variations
        .iter()
        .any(|v| axes.find_by_tag(tag(&v.tag)).is_some())
    {
        return None;
    }
    let mut settings: Vec<(swash::Tag, f32)> = Vec::new();
    if !variations.iter().any(|v| v.tag == "wght") {
        settings.push((tag("wght"), f32::from(glyph.font_weight.0)));
    }
    let base: Vec<i16> = axes.normalized_coords(settings.iter().copied()).collect();
    settings.extend(variations.iter().map(|v| (tag(&v.tag), v.value)));
    let coords: Vec<i16> = axes.normalized_coords(settings.iter().copied()).collect();
    let size = glyph.font_size;
    let mut scaler = scale
        .builder(swash_font)
        .size(size)
        .normalized_coords(coords.iter())
        .build();
    let outline = scaler.scale_outline(glyph.glyph_id)?;
    let cmds: Vec<cosmic_text::Command> = outline.path().commands().collect();
    let advance = |c: &[i16]| {
        swash_font
            .glyph_metrics(c)
            .scale(size)
            .advance_width(glyph.glyph_id)
    };
    Some((cmds, advance(&coords) - advance(&base)))
}

fn push_commands(cmds: &[cosmic_text::Command], gx: f64, gy: f64, out: &mut Vec<PathCmd>) {
    let p = |x: f32, y: f32| Point::new(gx + f64::from(x), gy - f64::from(y));
    for c in cmds {
        out.push(match *c {
            cosmic_text::Command::MoveTo(a) => PathCmd::MoveTo { to: p(a.x, a.y) },
            cosmic_text::Command::LineTo(a) => PathCmd::LineTo { to: p(a.x, a.y) },
            cosmic_text::Command::QuadTo(c1, a) => PathCmd::QuadTo {
                ctrl: p(c1.x, c1.y),
                to: p(a.x, a.y),
            },
            cosmic_text::Command::CurveTo(c1, c2, a) => PathCmd::CubicTo {
                ctrl1: p(c1.x, c1.y),
                ctrl2: p(c2.x, c2.y),
                to: p(a.x, a.y),
            },
            cosmic_text::Command::Close => PathCmd::Close,
        });
    }
}

fn push_outline(
    system: &mut FontSystem,
    cache: &mut SwashCache,
    glyph: &cosmic_text::LayoutGlyph,
    gx: f64,
    gy: f64,
    cmds: &mut Vec<PathCmd>,
) {
    let physical = glyph.physical((0.0, 0.0), 1.0);
    if let Some(outline) = cache.get_outline_commands(system, physical.cache_key) {
        push_commands(outline, gx, gy, cmds);
    }
}

fn shaped_buffer(
    system: &mut FontSystem,
    data: &TextData,
    width: Option<f64>,
    content: &str,
) -> Buffer {
    let style = &data.style;
    let attrs = attrs_for(style);
    let mut buffer = Buffer::new(
        system,
        Metrics::new(
            style.font_size.clamp(0.5, 10_000.0) as f32,
            style.line_height_px().clamp(0.1, 100_000.0) as f32,
        ),
    );
    let indent = paragraphs(data).indent;
    let width = width
        .filter(|w| w.is_finite() && *w >= 0.0)
        .map(|w| (w - indent).max(1.0).min(f64::from(f32::MAX)) as f32);
    buffer.set_wrap(if width.is_some() {
        Wrap::WordOrGlyph
    } else {
        Wrap::None
    });
    buffer.set_size(width, None);
    if data.runs.is_empty() || data.validate_runs().is_err() {
        buffer.set_text(
            content,
            &attrs,
            Shaping::Advanced,
            Some(align(data.align_h)),
        );
    } else {
        let offsets = (content != data.content).then(|| display_offsets(data));
        let valid_offsets = offsets
            .as_ref()
            .is_none_or(|map| map.last().is_some_and(|(_, n)| *n == content.len()));
        let shown = |source: usize| -> Option<usize> {
            offsets.as_ref().map_or(Some(source), |map| {
                map.binary_search_by_key(&source, |(s, _)| *s)
                    .ok()
                    .map(|i| map[i].1)
            })
        };
        let mut pieces = Vec::with_capacity(data.runs.len().saturating_mul(2).saturating_add(1));
        let mut previous = 0;
        for (index, run) in data.runs.iter().enumerate() {
            let (Some(start), Some(end)) = (shown(run.start), shown(run.end)) else {
                continue;
            };
            if let Some(piece) = content.get(previous..start) {
                if !piece.is_empty() {
                    pieces.push((piece, attrs.clone()));
                }
            }
            if let Some(piece) = content.get(start..end) {
                if !piece.is_empty() {
                    pieces.push((piece, attrs_for(&run.style).metadata(index + 1)));
                }
            }
            previous = end;
        }
        if let Some(piece) = content.get(previous..) {
            if !piece.is_empty() {
                pieces.push((piece, attrs.clone()));
            }
        }
        if valid_offsets && pieces.iter().map(|(s, _)| s.len()).sum::<usize>() == content.len() {
            buffer.set_rich_text(pieces, &attrs, Shaping::Advanced, Some(align(data.align_h)));
        } else {
            buffer.set_text(
                content,
                &attrs,
                Shaping::Advanced,
                Some(align(data.align_h)),
            );
        }
    }
    buffer.shape_until_scroll(system, false);
    buffer
}

fn layout_uncached(f: &mut Fonts, data: &TextData, width: Option<f64>) -> TextLayout {
    let style = &data.style;
    let line_height = style.line_height_px().clamp(0.1, 100_000.0) as f32;
    let content = style.transform_case(&data.content);
    let Fonts {
        system,
        cache,
        scale,
        ..
    } = f;
    let buffer = shaped_buffer(system, data, width, &content);

    let mut lines = Vec::new();
    let mut parts: Vec<(Option<deesainer_doc::Paint>, Vec<PathCmd>)> = vec![(None, Vec::new())];
    let style_of = |meta: usize| -> &TextStyle {
        meta.checked_sub(1)
            .and_then(|i| data.runs.get(i))
            .map_or(style, |r| &r.style)
    };
    let part_of = |parts: &mut Vec<(Option<deesainer_doc::Paint>, Vec<PathCmd>)>,
                   color: Option<deesainer_doc::Paint>|
     -> usize {
        parts
            .iter()
            .position(|(c, _)| *c == color)
            .unwrap_or_else(|| {
                parts.push((color, Vec::new()));
                parts.len() - 1
            })
    };
    let mut max_w: f64 = 0.0;
    let mut bottom: f64 = 0.0;
    let paras = paragraphs(data);
    let spacing = paras.spacing;
    let mut marked = None;
    for run in buffer.layout_runs() {
        let indent = paras.indent_of(run.line_i);
        let dy = run.line_i as f64 * spacing;
        let left = run.glyphs.first().map(|g| f64::from(g.x)).unwrap_or(0.0) + indent;
        lines.push(Line {
            top: f64::from(run.line_top) + dy,
            height: f64::from(run.line_height),
            baseline: f64::from(run.line_y) + dy,
            left,
            width: f64::from(run.line_w),
            text: run.text.to_owned(),
        });
        bottom = bottom.max(f64::from(run.line_top + run.line_height) + dy);
        let shown = paras.items.get(run.line_i).and_then(|i| i.1.as_ref());
        if let Some((mx, text)) = shown.filter(|_| marked != Some(run.line_i)) {
            marked = Some(run.line_i);
            let mut m = Buffer::new(
                system,
                Metrics::new(style.font_size.clamp(0.5, 10_000.0) as f32, line_height),
            );
            m.set_text(text, &attrs_for(style), Shaping::Advanced, None);
            m.shape_until_scroll(system, false);
            let base = f64::from(run.line_y) + dy;
            for mrun in m.layout_runs() {
                for glyph in mrun.glyphs {
                    let gx = mx + f64::from(glyph.x + glyph.font_size * glyph.x_offset);
                    let gy = base + f64::from(glyph.y - glyph.font_size * glyph.y_offset);
                    push_outline(system, cache, glyph, gx, gy, &mut parts[0].1);
                }
            }
        }
        let mut group: Option<(usize, f64, f64)> = None;
        let close =
            |group: &mut Option<(usize, f64, f64)>,
             parts: &mut Vec<(Option<deesainer_doc::Paint>, Vec<PathCmd>)>| {
                let Some((meta, x0, x1)) = group.take() else {
                    return;
                };
                let st = style_of(meta);
                let size = f64::from(st.font_size.clamp(0.5, 10_000.0) as f32);
                let y = dy
                    + match st.decoration {
                        TextDecoration::None => return,
                        TextDecoration::Underline => f64::from(run.line_y) + size * 0.12,
                        TextDecoration::Strikethrough => f64::from(run.line_y) - size * 0.3,
                    };
                let thickness = (size / 14.0).max(0.5);
                let k = part_of(parts, st.fill.clone());
                let c = &mut parts[k].1;
                c.push(PathCmd::MoveTo {
                    to: Point::new(x0, y),
                });
                c.push(PathCmd::LineTo {
                    to: Point::new(x1, y),
                });
                c.push(PathCmd::LineTo {
                    to: Point::new(x1, y + thickness),
                });
                c.push(PathCmd::LineTo {
                    to: Point::new(x0, y + thickness),
                });
                c.push(PathCmd::Close);
            };
        let mut shift = 0.0;
        for glyph in run.glyphs {
            let st = style_of(glyph.metadata);
            let varied = if st.variations.is_empty() {
                None
            } else {
                varied_glyph(system, scale, glyph, &st.variations)
            };
            let grow = varied.as_ref().map_or(0.0, |(_, d)| f64::from(*d));
            let (gx0, gx1) = (
                f64::from(glyph.x) + indent + shift,
                f64::from(glyph.x + glyph.w) + indent + shift + grow,
            );
            match &mut group {
                Some((meta, _, x1)) if *meta == glyph.metadata => *x1 = gx1,
                _ => {
                    close(&mut group, &mut parts);
                    group = Some((glyph.metadata, gx0, gx1));
                }
            }
            let gx = f64::from(glyph.x + glyph.font_size * glyph.x_offset) + indent + shift;
            let gy = f64::from(run.line_y + glyph.y - glyph.font_size * glyph.y_offset) + dy;
            let k = part_of(&mut parts, style_of(glyph.metadata).fill.clone());
            match &varied {
                Some((cmds, _)) => push_commands(cmds, gx, gy, &mut parts[k].1),
                None => push_outline(system, cache, glyph, gx, gy, &mut parts[k].1),
            }
            shift += grow;
        }
        close(&mut group, &mut parts);
        if shift != 0.0 {
            if let Some(line) = lines.last_mut() {
                line.width += shift;
            }
        }
        max_w = max_w.max(f64::from(run.line_w) + indent + shift);
    }
    if lines.is_empty() {
        bottom = f64::from(line_height);
    }
    let path = |cmds: Vec<PathCmd>| VectorPath {
        cmds,
        fill_rule: FillRule::NonZero,
    };
    let mut colored = Vec::new();
    let mut base = Vec::new();
    for (color, cmds) in parts {
        match color {
            Some(c) => colored.push((c, path(cmds))),
            None => base = cmds,
        }
    }
    TextLayout {
        width: width.unwrap_or(max_w.ceil()),
        height: bottom.ceil(),
        lines,
        outline: path(base),
        colored,
    }
}

pub fn measure(data: &TextData, fixed_width: Option<f64>) -> (f64, f64) {
    let l = layout(data, fixed_width);
    (l.width, l.height)
}

pub fn node_outlines(node: &Node) -> Vec<VectorPath> {
    let (base, colored) = node_parts(node);
    std::iter::once(base)
        .chain(colored.into_iter().map(|(_, p)| p))
        .collect()
}

pub fn node_parts(node: &Node) -> (VectorPath, Vec<(deesainer_doc::Paint, VectorPath)>) {
    let Some(data) = node.text() else {
        return (VectorPath::default(), Vec::new());
    };
    let width = match data.auto_resize {
        TextAutoResize::WidthAndHeight => None,
        TextAutoResize::Height | TextAutoResize::None => Some(node.rect.width),
    };
    let l = layout(data, width);
    let dy = if data.auto_resize == TextAutoResize::None {
        let free = node.rect.height - l.height;
        match data.align_v {
            TextAlignV::Top => 0.0,
            TextAlignV::Center => free / 2.0,
            TextAlignV::Bottom => free,
        }
    } else {
        0.0
    };
    let shift = |p: VectorPath| {
        if dy.abs() > 1e-9 {
            p.map_points(|q| Point::new(q.x, q.y + dy))
        } else {
            p
        }
    };
    let (w, h) = (node.rect.width.max(1e-9), node.rect.height.max(1e-9));
    (
        shift(l.outline),
        l.colored
            .into_iter()
            .map(|(paint, path)| {
                let path = shift(path);
                (fit_to_range(paint, path.bounds(), w, h), path)
            })
            .collect(),
    )
}

fn fit_to_range(
    mut paint: deesainer_doc::Paint,
    bounds: Option<Rect>,
    w: f64,
    h: f64,
) -> deesainer_doc::Paint {
    use deesainer_doc::PaintKind;
    let Some(b) = bounds else { return paint };
    let map = |p: Point| Point::new((b.x + p.x * b.width) / w, (b.y + p.y * b.height) / h);
    match &mut paint.kind {
        PaintKind::LinearGradient { start, end, .. } => {
            *start = map(*start);
            *end = map(*end);
        }
        PaintKind::RadialGradient { center, radius, .. }
        | PaintKind::DiamondGradient { center, radius, .. } => {
            *center = map(*center);
            *radius = Point::new(radius.x * b.width / w, radius.y * b.height / h);
        }
        PaintKind::AngularGradient { center, .. } => *center = map(*center),
        PaintKind::Solid { .. } | PaintKind::Image { .. } => {}
    }
    paint
}

pub fn cjk_font_data() -> Option<Vec<u8>> {
    const CJK_FAMILIES: &[&str] = &[
        "Noto Sans CJK",
        "Noto Sans SC",
        "Noto Sans TC",
        "Noto Sans JP",
        "Source Han Sans",
        "PingFang SC",
        "PingFang TC",
        "Hiragino Sans",
        "Hiragino Kaku Gothic",
        "Microsoft YaHei",
        "SimHei",
        "Yu Gothic",
        "Meiryo",
        "Malgun Gothic",
        "Apple SD Gothic Neo",
    ];
    let Ok(guard) = fonts().lock() else {
        return None;
    };
    let db = guard.system.db();
    let id = db
        .faces()
        .find(|f| {
            f.families
                .iter()
                .any(|(n, _)| CJK_FAMILIES.iter().any(|c| n.starts_with(c)))
        })
        .map(|f| f.id)?;
    let mut out = None;
    db.with_face_data(id, |data, _| {
        out = Some(data.to_vec());
    });
    out
}

pub fn families() -> Vec<String> {
    let Ok(guard) = fonts().lock() else {
        return Vec::new();
    };
    let mut names: Vec<String> = guard
        .system
        .db()
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn add_font(data: Vec<u8>) -> usize {
    let Ok(mut guard) = fonts().lock() else {
        return 0;
    };
    let before = guard.system.db().faces().count();
    guard.system.db_mut().load_font_data(data);
    drop_shadowing_faces(guard.system.db_mut());
    let added = guard.system.db().faces().count().saturating_sub(before);
    guard.usable = has_fonts(&guard.system);
    guard.layouts.clear();
    added
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontAxis {
    pub tag: String,
    pub name: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

pub fn font_axes(style: &TextStyle) -> Vec<FontAxis> {
    let Ok(mut guard) = fonts().lock() else {
        return Vec::new();
    };
    if !guard.usable {
        return Vec::new();
    }
    let data = TextData {
        content: "a".into(),
        style: style.clone(),
        ..TextData::default()
    };
    let buffer = shaped_buffer(&mut guard.system, &data, None, "a");
    let Some((id, weight)) = buffer
        .layout_runs()
        .flat_map(|r| r.glyphs.iter())
        .map(|g| (g.font_id, g.font_weight))
        .next()
    else {
        return Vec::new();
    };
    let Some(font) = guard.system.get_font(id, weight) else {
        return Vec::new();
    };
    font.as_swash()
        .variations()
        .filter(|v| !v.is_hidden())
        .map(|v| {
            let tag: [u8; 4] = v.tag().to_be_bytes();
            let tag = String::from_utf8_lossy(&tag).into_owned();
            FontAxis {
                name: v.name(None).map_or_else(|| tag.clone(), |n| n.to_string()),
                tag,
                min: v.min_value(),
                default: v.default_value(),
                max: v.max_value(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_color::Rgba;
    use deesainer_doc::TextStyle;

    fn data(content: &str) -> TextData {
        TextData {
            content: content.into(),
            style: TextStyle {
                font_family: "sans-serif".into(),
                font_size: 20.0,
                ..TextStyle::default()
            },
            ..TextData::default()
        }
    }

    #[test]
    fn longer_text_is_wider_and_paragraphs_add_lines() {
        let short = layout(&data("Hi"), None);
        let long = layout(&data("Hello there, world"), None);
        assert!(long.width >= short.width);
        let two = layout(&data("a\nb"), None);
        assert!(two.height > short.height);
    }

    #[test]
    fn fixed_width_wraps() {
        let text = data("one two three four five six seven eight nine ten");
        let free = layout(&text, None);
        let wrapped = layout(&text, Some(60.0));
        assert_eq!(wrapped.width, 60.0);
        if free.width > 60.0 && !free.lines.is_empty() {
            assert!(wrapped.lines.len() > 1, "{:?}", wrapped.lines.len());
            assert!(wrapped.height > free.height);
        }
    }

    fn bounds(p: &VectorPath) -> Option<(f64, f64, f64, f64)> {
        let mut pts = Vec::new();
        for c in &p.cmds {
            match c {
                PathCmd::MoveTo { to } | PathCmd::LineTo { to } => pts.push(*to),
                PathCmd::QuadTo { to, .. } | PathCmd::CubicTo { to, .. } => pts.push(*to),
                PathCmd::Close => {}
            }
        }
        let x0 = pts.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        let x1 = pts.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        let y0 = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        let y1 = pts.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        (!pts.is_empty()).then_some((x0, y0, x1, y1))
    }

    #[test]
    fn a_family_name_picks_its_text_face_not_its_ornaments() {
        let guard = fonts().lock().unwrap();
        let db = guard.system.db();
        let query = cosmic_text::fontdb::Query {
            families: &[cosmic_text::fontdb::Family::Name("Hoefler Text")],
            ..Default::default()
        };
        if let Some(id) = db.query(&query) {
            let face = db.face(id).unwrap();
            assert!(
                !face.post_script_name.contains("Ornament"),
                "{}",
                face.post_script_name
            );
        }
    }

    #[test]
    fn paragraph_spacing_adds_space_between_paragraphs_only() {
        let plain = layout(&data("one\ntwo\nthree"), None);
        let mut spaced = data("one\ntwo\nthree");
        spaced.paragraph_spacing = 12.0;
        let spaced = layout(&spaced, None);
        assert_eq!(spaced.height, plain.height + 24.0);
        assert_eq!(spaced.lines[1].top, plain.lines[1].top + 12.0);
        assert_eq!(spaced.lines[2].baseline, plain.lines[2].baseline + 24.0);
    }

    #[test]
    fn lists_indent_the_text_and_draw_markers() {
        let plain = layout(&data("apples\npears"), None);
        let mut listed = data("apples\n\npears");
        listed.list = TextList::Number;
        let indent = paragraphs(&listed).indent;
        assert!(indent > 0.0);
        let l = layout(&listed, None);
        assert_eq!(l.lines[0].left, plain.lines[0].left + indent);
        assert!(
            (l.width - (plain.width + indent)).abs() <= 1.0,
            "{} {}",
            l.width,
            plain.width
        );
        let (x0, ..) = bounds(&l.outline).unwrap();
        assert!(x0 < indent, "{x0} {indent}");
        let mut two = listed.clone();
        two.content = "apples\npears".into();
        let fewer = layout(&two, None);
        assert_eq!(
            l.outline.cmds.len(),
            fewer.outline.cmds.len(),
            "an empty item must not add a marker"
        );
        let wrapped = layout(&listed, Some(80.0));
        assert_eq!(wrapped.width, 80.0);
        assert!(wrapped.lines.iter().all(|line| line.left >= indent - 1e-9));
    }

    #[test]
    fn without_any_font_text_lays_out_roughly_instead_of_failing() {
        let system = FontSystem::new_with_locale_and_db(
            "en-US".into(),
            cosmic_text::fontdb::Database::new(),
        );
        assert!(!has_fonts(&system));
        let mut f = Fonts {
            usable: has_fonts(&system),
            system,
            cache: SwashCache::new(),
            scale: swash::scale::ScaleContext::new(),
            layouts: HashMap::new(),
        };
        let d = data("Hello\nworld");
        let l = layout_with(&mut f, hash_key(&d, None), &d, None);
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(l.outline.cmds.is_empty());
    }

    #[test]
    fn fonts_can_be_added_from_bytes() {
        assert_eq!(add_font(b"not a font".to_vec()), 0);
        let candidates = [
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
        ];
        if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
            let _ = add_font(bytes);
            assert!(!families().is_empty());
            assert!(layout(&data("After adding"), None).outline.cmds.len() > 10);
        }
    }

    #[test]
    fn a_width_axis_narrows_and_widens_the_letters() {
        let mut d = data("Variable");
        d.style.font_size = 40.0;
        let has_width = font_axes(&d.style).iter().any(|a| a.tag == "wdth");
        if !has_width {
            eprintln!("no variable font with a width axis installed; skipped");
            return;
        }
        let at = |w: f32| {
            let mut d = d.clone();
            d.style.variations = vec![deesainer_doc::FontVariation {
                tag: "wdth".into(),
                value: w,
            }];
            layout(&d, None)
        };
        let (narrow, normal, wide) = (at(60.0), at(100.0), at(150.0));
        let plain = layout(&d, None);
        assert!(
            (normal.width - plain.width).abs() < 1.0,
            "{} {}",
            normal.width,
            plain.width
        );
        assert!(
            narrow.width < normal.width - 10.0,
            "{} {}",
            narrow.width,
            normal.width
        );
        assert!(
            wide.width > normal.width + 10.0,
            "{} {}",
            wide.width,
            normal.width
        );
        let (x0, _, x1, _) = bounds(&narrow.outline).unwrap();
        assert!(
            x1 - x0 <= narrow.width + 1.0,
            "{} {}",
            x1 - x0,
            narrow.width
        );
    }

    #[test]
    fn paragraphs_list_and_nest_one_by_one() {
        let mut d = data("Intro\nFirst\nSub one\nSub two\nSecond\nOutro");
        let set = |d: &mut TextData, i: usize, list: TextList, level: u8| {
            d.set_paragraph_format(i, i, |f| {
                f.list = list;
                f.level = level;
            })
            .unwrap();
        };
        set(&mut d, 1, TextList::Number, 0);
        set(&mut d, 2, TextList::Number, 1);
        set(&mut d, 3, TextList::Number, 1);
        set(&mut d, 4, TextList::Number, 0);
        let p = paragraphs(&d);
        let markers: Vec<Option<&str>> = p
            .items
            .iter()
            .map(|i| i.1.as_ref().map(|m| m.1.as_str()))
            .collect();
        assert_eq!(
            markers,
            [None, Some("1."), Some("a."), Some("b."), Some("2."), None]
        );
        assert_eq!(p.indent_of(0), 0.0);
        assert!(p.indent_of(2) > p.indent_of(1));
        assert_eq!(p.indent_of(4), p.indent_of(1));
        let l = layout(&d, None);
        assert_eq!(l.lines[0].left, 0.0);
        assert!((l.lines[2].left - p.indent_of(2)).abs() < 1e-9);
        set(&mut d, 5, TextList::Number, 0);
        set(&mut d, 4, TextList::None, 0);
        assert_eq!(paragraphs(&d).items[5].1.as_ref().unwrap().1, "1.");
        assert_eq!(marker(TextList::Number, 2, 4), "iv.");
        assert_eq!(marker(TextList::Number, 1, 28), "ab.");
        assert_eq!(marker(TextList::Bullet, 1, 0), "◦");
    }

    #[test]
    fn the_editor_caret_follows_the_indent_and_spacing() {
        let mut d = data("one\ntwo");
        d.list = TextList::Bullet;
        d.paragraph_spacing = 10.0;
        let indent = paragraphs(&d).indent;
        let edit = TextEditor::new(d.clone(), None).unwrap();
        let caret = edit.geometry(100.0).caret.unwrap();
        let l = layout(&d, None);
        assert!(
            (caret.y - l.lines[1].top).abs() < 1e-6,
            "{} {}",
            caret.y,
            l.lines[1].top
        );
        assert!(caret.x > indent);
        let mut edit = edit;
        edit.pointer(
            Point::new(indent + 1.0, l.lines[0].top + 2.0),
            100.0,
            false,
            1,
        )
        .unwrap();
        assert_eq!(edit.cursor_offset(), 0);
        edit.pointer(
            Point::new(indent + 1.0, l.lines[1].top + 2.0),
            100.0,
            false,
            1,
        )
        .unwrap();
        assert_eq!(edit.cursor_offset(), 4);
    }

    #[test]
    fn a_gradient_on_a_range_spans_the_range() {
        use deesainer_doc::{ColorStop, Node, NodeKind, Paint, PaintKind};
        let mut text = data("Hello world");
        let gradient = Paint::linear(
            Point::new(0.0, 0.5),
            Point::new(1.0, 0.5),
            vec![
                ColorStop {
                    position: 0.0,
                    color: Rgba::new(1.0, 0.0, 0.0, 1.0),
                },
                ColorStop {
                    position: 1.0,
                    color: Rgba::new(0.0, 0.0, 1.0, 1.0),
                },
            ],
        );
        text.set_range_style(
            6,
            11,
            TextStyle {
                fill: Some(gradient),
                ..text.style.clone()
            },
        )
        .unwrap();
        let (w, h) = measure(&text, None);
        let node = Node::new(
            deesainer_doc::NodeId(10),
            "T",
            NodeKind::Text(text),
            Rect::new(0.0, 0.0, w, h),
        );
        let (_, parts) = node_parts(&node);
        let (paint, path) = &parts[0];
        let b = path.bounds().unwrap();
        let PaintKind::LinearGradient { start, end, .. } = &paint.kind else {
            panic!("not a gradient");
        };
        assert!((start.x * w - b.x).abs() < 1e-6, "{start:?} {b:?}");
        assert!((end.x * w - (b.x + b.width)).abs() < 1e-6);
        assert!(start.x > 0.3, "the range starts well into the layer");
    }

    #[test]
    fn a_coloured_range_gets_its_own_path_and_underline() {
        let mut text = data("Hello world");
        let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
        text.set_range_style(
            6,
            11,
            TextStyle {
                fill: Some(deesainer_doc::Paint::solid(red)),
                decoration: TextDecoration::Underline,
                ..text.style.clone()
            },
        )
        .unwrap();
        let l = layout(&text, None);
        assert_eq!(l.colored.len(), 1);
        assert_eq!(l.colored[0].0, deesainer_doc::Paint::solid(red));
        let (x0, _, x1, _) = bounds(&l.colored[0].1).unwrap();
        let (bx0, _, bx1, _) = bounds(&l.outline).unwrap();
        assert!(
            x0 >= bx1 - 1.0 && x1 > x0,
            "coloured {x0}..{x1}, base {bx0}..{bx1}"
        );
        let plain = layout(&data("Hello world"), None);
        assert!(l.outline.cmds.len() < plain.outline.cmds.len() + 1);
    }

    #[test]
    fn empty_text_has_one_line_height() {
        let l = layout(&data(""), None);
        assert!(l.height > 0.0);
        assert_eq!(l.width, 0.0);
    }

    #[test]
    fn outlines_exist_when_fonts_are_available() {
        let l = layout(&data("Ab"), None);
        if !families().is_empty() && !l.lines.is_empty() {
            assert!(!l.outline.cmds.is_empty());
        }
    }

    #[test]
    fn mixed_font_sizes_change_the_shaped_line() {
        let mut mixed = data("small BIG");
        let plain = layout(&mixed, None);
        let mut large = mixed.style.clone();
        large.font_size = 60.0;
        mixed.set_range_style(6, 9, large).unwrap();
        let styled = layout(&mixed, None);
        assert!(styled.height > plain.height);
        assert!(styled.width > plain.width);
    }
}
