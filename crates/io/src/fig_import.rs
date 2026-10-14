use std::collections::BTreeMap;
use std::sync::Arc;

use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{
    AutoLayout, BindTarget, BoolOp, CollectionId, ColorStop, Constraint, Constraints, CornerRadius,
    CounterAlign, Document, Effect, FillRule, ImageScaleMode, LayoutDirection, LineHeight, Modes,
    Node, NodeId, NodeKind, Override, Padding, Paint, PaintKind, PathCmd, PrimaryAlign, PropValue,
    Sizing, StrokeAlign, StrokeCap, StrokeJoin, Style, StyleId, StyleKind, TextAlignH, TextAlignV,
    TextAutoResize, TextCase, TextData, TextDecoration, VarValue, Variable, VariableId, VectorPath,
};
use deesainer_figfile::{FigFile, Value};
use deesainer_geom::{Point, Rect};

use crate::IoError;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FigReport {
    pub pages: usize,
    pub layers: usize,
    pub styles: usize,
    pub variables: usize,
    pub overrides: usize,
    pub skipped: BTreeMap<String, usize>,
}

type Guid = (u64, u64);

fn guid(v: &Value) -> Option<Guid> {
    Some((v.get("sessionID")?.as_u64()?, v.get("localID")?.as_u64()?))
}

pub fn import_fig(bytes: &[u8]) -> Result<(Document, FigReport), IoError> {
    let fig = deesainer_figfile::read(bytes).map_err(|e| IoError::Other(format!(".fig: {e}")))?;
    convert(&fig)
}

struct Ctx<'a> {
    fig: &'a FigFile,
    nodes: BTreeMap<Guid, &'a Value>,
    children: BTreeMap<Guid, Vec<Guid>>,
    ids: BTreeMap<Guid, NodeId>,
    keys: BTreeMap<String, Guid>,
    override_keys: BTreeMap<Guid, Guid>,
    report: FigReport,
}

impl Ctx<'_> {
    fn reference(&self, v: &Value) -> Option<Guid> {
        if let Some(g) = v.get("guid").and_then(guid) {
            return Some(g);
        }
        let key = v.path(&["assetRef", "key"])?.as_str()?;
        self.keys.get(key).copied()
    }

    fn paint_variable(&self, doc: &Document, p: &Value) -> Option<VariableId> {
        let alias = p.path(&["colorVar", "value", "alias"])?;
        let id = VariableId(self.ids.get(&self.reference(alias)?)?.0);
        doc.variable(id).map(|_| id)
    }
}

fn convert(fig: &FigFile) -> Result<(Document, FigReport), IoError> {
    let mut ctx = Ctx {
        fig,
        nodes: BTreeMap::new(),
        children: BTreeMap::new(),
        ids: BTreeMap::new(),
        keys: BTreeMap::new(),
        override_keys: BTreeMap::new(),
        report: FigReport::default(),
    };
    let changes = fig
        .message
        .get("nodeChanges")
        .map_or(&[][..], Value::as_array);
    let mut ordered: BTreeMap<Guid, Vec<(String, Guid)>> = BTreeMap::new();
    let mut root = None;
    for n in changes {
        if n.get("phase").and_then(Value::as_str) == Some("REMOVED") {
            continue;
        }
        let Some(g) = n.get("guid").and_then(guid) else {
            continue;
        };
        ctx.nodes.insert(g, n);
        if let Some(k) = n.get("key").and_then(Value::as_str) {
            ctx.keys.insert(k.to_owned(), g);
        }
        if let Some(k) = n.get("overrideKey").and_then(guid) {
            ctx.override_keys.insert(k, g);
        }
        match n.get("parentIndex") {
            Some(pi) => {
                if let Some(p) = pi.get("guid").and_then(guid) {
                    let pos = pi
                        .get("position")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    ordered.entry(p).or_default().push((pos, g));
                }
            }
            None => {
                if n.get("type").and_then(Value::as_str) == Some("DOCUMENT") {
                    root = Some(g);
                }
            }
        }
    }
    for (p, mut list) in ordered {
        list.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        ctx.children
            .insert(p, list.into_iter().map(|(_, g)| g).collect());
    }
    let root = root.ok_or_else(|| IoError::Other(".fig: no document node".into()))?;
    let mut doc = Document::new();
    let start = doc.next_free_id().0;
    let guids: Vec<Guid> = ctx.nodes.keys().copied().collect();
    for (next, g) in (start..).zip(guids) {
        ctx.ids.insert(g, NodeId(next));
    }
    import_variables(&mut ctx, &mut doc);
    import_styles(&mut ctx, &mut doc);
    let pages: Vec<Guid> = ctx
        .children
        .get(&root)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|g| {
            ctx.nodes.get(g).is_some_and(|n| {
                n.get("type").and_then(Value::as_str) == Some("CANVAS")
                    && n.get("internalOnly").and_then(Value::as_bool) != Some(true)
            })
        })
        .collect();
    let first = doc.first_page();
    for (i, pg) in pages.iter().enumerate() {
        let Some(n) = ctx.nodes.get(pg).copied() else {
            continue;
        };
        let page_id = if i == 0 {
            first
        } else {
            let id = ctx.ids[pg];
            let page = Node::new(id, "", NodeKind::Page, Rect::default());
            let at = doc.pages().len();
            doc.insert(page, doc.root(), at)
                .map_err(|e| IoError::Other(e.to_string()))?;
            id
        };
        ctx.ids.insert(*pg, page_id);
        if let Some(p) = doc.get_mut(page_id) {
            p.name = n
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Page")
                .to_owned();
            if let Some(c) = n.get("backgroundColor") {
                let a = n.num("backgroundOpacity", 1.0) as f32;
                p.fills = vec![Paint::solid(color(c).with_alpha(a))];
            }
        }
        ctx.report.pages += 1;
        for child in ctx.children.get(pg).cloned().unwrap_or_default() {
            add_node(&mut ctx, &mut doc, child, page_id, 0)?;
        }
    }
    for (name, bytes) in &fig.images {
        let _ = name;
        doc.insert_asset(deesainer_format::asset_id(bytes), Arc::new(bytes.clone()));
    }
    Ok((doc, ctx.report))
}

fn soft_deleted(n: &Value) -> bool {
    n.get("isSoftDeleted").and_then(Value::as_bool) == Some(true)
}

fn var_value(ctx: &Ctx<'_>, data: &Value) -> Option<VarValue> {
    match s(data, "dataType") {
        "COLOR" => data
            .path(&["value", "colorValue"])
            .map(|c| VarValue::Color(color(c))),
        "ALIAS" => {
            let g = ctx.reference(data.path(&["value", "alias"])?)?;
            if s(ctx.nodes.get(&g)?, "type") != "VARIABLE" {
                return None;
            }
            Some(VarValue::Alias(VariableId(ctx.ids.get(&g)?.0)))
        }
        _ => None,
    }
}

fn import_variables(ctx: &mut Ctx<'_>, doc: &mut Document) {
    let mut collections: BTreeMap<Guid, (CollectionId, Vec<Guid>)> = BTreeMap::new();
    for (g, n) in &ctx.nodes {
        if s(n, "type") != "VARIABLE_SET" || soft_deleted(n) {
            continue;
        }
        let mut modes: Vec<(&str, String, Guid)> = n
            .get("variableSetModes")
            .map_or(&[][..], Value::as_array)
            .iter()
            .filter_map(|m| {
                Some((
                    s(m, "sortPosition"),
                    s(m, "name").to_owned(),
                    m.get("id").and_then(guid)?,
                ))
            })
            .collect();
        modes.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        if modes.is_empty() {
            continue;
        }
        let set = Modes {
            name: match s(n, "name") {
                "" => "Variables".to_owned(),
                name => name.to_owned(),
            },
            names: modes.iter().map(|m| m.1.clone()).collect(),
            active: 0,
        };
        let id = if collections.is_empty() {
            if doc.set_modes(set).is_err() {
                continue;
            }
            CollectionId::DEFAULT
        } else {
            let Some(id) = ctx.ids.get(g).map(|id| CollectionId(id.0)) else {
                continue;
            };
            if doc.set_collection(id, Some(set)).is_err() {
                continue;
            }
            id
        };
        collections.insert(*g, (id, modes.into_iter().map(|m| m.2).collect()));
    }
    let mut found = Vec::new();
    for (g, n) in &ctx.nodes {
        if s(n, "type") != "VARIABLE" || soft_deleted(n) {
            continue;
        }
        let set = n
            .get("variableSetID")
            .and_then(|v| ctx.reference(v))
            .and_then(|set| collections.get(&set));
        let (Some((collection, modes)), Some(id)) = (set, ctx.ids.get(g)) else {
            found.push(None);
            continue;
        };
        let entries = n
            .path(&["variableDataValues", "entries"])
            .map_or(&[][..], Value::as_array);
        let value_in = |mode: &Guid| {
            entries
                .iter()
                .find(|e| e.get("modeID").and_then(guid).as_ref() == Some(mode))
                .and_then(|e| e.get("variableData"))
                .and_then(|d| var_value(ctx, d))
        };
        let first = entries
            .iter()
            .find_map(|e| e.get("variableData").and_then(|d| var_value(ctx, d)));
        let values: Option<Vec<VarValue>> = modes
            .iter()
            .map(|m| value_in(m).or_else(|| first.clone()))
            .collect();
        found.push(values.map(|values| {
            (
                VariableId(id.0),
                Variable {
                    name: s(n, "name").to_owned(),
                    values,
                    collection: *collection,
                    scopes: Vec::new(),
                    origin: None,
                },
            )
        }));
    }
    for v in found {
        let kept = v.is_some_and(|(id, var)| doc.set_variable(id, Some(var)).is_ok());
        if kept {
            ctx.report.variables += 1;
        } else {
            *ctx.report.skipped.entry("VARIABLE".to_owned()).or_default() += 1;
        }
    }
}

fn import_styles(ctx: &mut Ctx<'_>, doc: &mut Document) {
    let mut skipped = Vec::new();
    for (g, n) in &ctx.nodes {
        let ty = s(n, "styleType");
        if ty.is_empty() || ty == "NONE" || soft_deleted(n) {
            continue;
        }
        let kind = match ty {
            "FILL" => StyleKind::Paint(paints(ctx, n, "fillPaints")),
            "TEXT" => StyleKind::Text(text(n).style),
            "EFFECT" => StyleKind::Effect(effects(n)),
            other => {
                skipped.push(format!("{other} style"));
                continue;
            }
        };
        let Some(id) = ctx.ids.get(g) else {
            continue;
        };
        let style = Style {
            name: s(n, "name").to_owned(),
            description: String::new(),
            kind,
            origin: None,
        };
        doc.set_style(StyleId(id.0), Some(style));
        ctx.report.styles += 1;
    }
    for k in skipped {
        *ctx.report.skipped.entry(k).or_default() += 1;
    }
}

fn overrides(ctx: &Ctx<'_>, n: &Value) -> Vec<Override> {
    let mut out = Vec::new();
    for o in n
        .path(&["symbolData", "symbolOverrides"])
        .map_or(&[][..], Value::as_array)
    {
        let Some(last) = o
            .path(&["guidPath", "guids"])
            .map_or(&[][..], Value::as_array)
            .last()
            .and_then(guid)
        else {
            continue;
        };
        let target = if ctx.nodes.contains_key(&last) {
            last
        } else {
            ctx.override_keys.get(&last).copied().unwrap_or(last)
        };
        let (Some(tn), Some(tid)) = (ctx.nodes.get(&target).copied(), ctx.ids.get(&target)) else {
            continue;
        };
        let mut values = Vec::new();
        if s(tn, "type") == "TEXT" && (o.get("textData").is_some() || o.get("fontSize").is_some()) {
            let mut t = text(tn);
            if let Some(c) = o.path(&["textData", "characters"]).and_then(Value::as_str) {
                t.content = c.to_owned();
            }
            if let Some(size) = o.get("fontSize").and_then(Value::as_f64) {
                t.style.font_size = size;
            }
            values.push(PropValue::Kind(NodeKind::Text(t)));
        }
        if o.get("fillPaints").is_some() {
            values.push(PropValue::Fills(paints(ctx, o, "fillPaints")));
        }
        if o.get("strokePaints").is_some() {
            values.push(PropValue::Strokes(paints(ctx, o, "strokePaints")));
        }
        if o.get("effects").is_some() {
            values.push(PropValue::Effects(effects(o)));
        }
        if let Some(v) = o.get("opacity").and_then(Value::as_f64) {
            values.push(PropValue::Opacity(v as f32));
        }
        if let Some(v) = o.get("visible").and_then(Value::as_bool) {
            values.push(PropValue::Visible(v));
        }
        out.extend(values.into_iter().map(|value| Override {
            target: *tid,
            value,
        }));
    }
    out
}

fn s<'a>(n: &'a Value, k: &str) -> &'a str {
    n.get(k).and_then(Value::as_str).unwrap_or("")
}

fn color(c: &Value) -> Rgba {
    Rgba::new(
        c.num("r", 0.0) as f32,
        c.num("g", 0.0) as f32,
        c.num("b", 0.0) as f32,
        c.num("a", 1.0) as f32,
    )
}

fn blend(name: &str) -> BlendMode {
    match name {
        "PASS_THROUGH" => BlendMode::PassThrough,
        "DARKEN" => BlendMode::Darken,
        "MULTIPLY" => BlendMode::Multiply,
        "COLOR_BURN" => BlendMode::ColorBurn,
        "LIGHTEN" => BlendMode::Lighten,
        "SCREEN" => BlendMode::Screen,
        "COLOR_DODGE" => BlendMode::ColorDodge,
        "OVERLAY" => BlendMode::Overlay,
        "SOFT_LIGHT" => BlendMode::SoftLight,
        "HARD_LIGHT" => BlendMode::HardLight,
        "DIFFERENCE" => BlendMode::Difference,
        "EXCLUSION" => BlendMode::Exclusion,
        "HUE" => BlendMode::Hue,
        "SATURATION" => BlendMode::Saturation,
        "COLOR" => BlendMode::Color,
        "LUMINOSITY" => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

fn matrix(n: &Value) -> [f64; 6] {
    match n.get("transform") {
        Some(t) => [
            t.num("m00", 1.0),
            t.num("m01", 0.0),
            t.num("m02", 0.0),
            t.num("m10", 0.0),
            t.num("m11", 1.0),
            t.num("m12", 0.0),
        ],
        None => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    }
}

fn invert(m: [f64; 6]) -> Option<[f64; 6]> {
    let [a, b, c, d, e, f] = m;
    let det = a * e - b * d;
    if det.abs() < 1e-12 {
        return None;
    }
    let ia = e / det;
    let ib = -b / det;
    let id = -d / det;
    let ie = a / det;
    Some([ia, ib, -(ia * c + ib * f), id, ie, -(id * c + ie * f)])
}

fn apply(m: [f64; 6], x: f64, y: f64) -> Point {
    Point::new(m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
}

fn paint(ctx: &Ctx<'_>, p: &Value) -> Option<Paint> {
    let opacity = p.num("opacity", 1.0) as f32;
    let visible = p.get("visible").and_then(Value::as_bool).unwrap_or(true);
    let blend_mode = blend(s(p, "blendMode"));
    let stops = || -> Vec<ColorStop> {
        p.get("stops")
            .map_or(&[][..], Value::as_array)
            .iter()
            .map(|st| ColorStop {
                position: st.num("position", 0.0) as f32,
                color: st.get("color").map_or(Rgba::BLACK, color),
            })
            .collect()
    };
    let inv = invert(
        p.get("transform")
            .map_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0], |t| {
                [
                    t.num("m00", 1.0),
                    t.num("m01", 0.0),
                    t.num("m02", 0.0),
                    t.num("m10", 0.0),
                    t.num("m11", 1.0),
                    t.num("m12", 0.0),
                ]
            }),
    )
    .unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let kind = match s(p, "type") {
        "SOLID" => PaintKind::Solid {
            color: p.get("color").map_or(Rgba::BLACK, color),
        },
        "GRADIENT_LINEAR" => PaintKind::LinearGradient {
            start: apply(inv, 0.0, 0.5),
            end: apply(inv, 1.0, 0.5),
            stops: stops(),
        },
        kind @ ("GRADIENT_RADIAL" | "GRADIENT_ANGULAR" | "GRADIENT_DIAMOND") => {
            let c = apply(inv, 0.5, 0.5);
            let rx = apply(inv, 1.0, 0.5);
            let ry = apply(inv, 0.5, 1.0);
            let radius = Point::new(
                ((rx.x - c.x).powi(2) + (rx.y - c.y).powi(2)).sqrt(),
                ((ry.x - c.x).powi(2) + (ry.y - c.y).powi(2)).sqrt(),
            );
            match kind {
                "GRADIENT_ANGULAR" => PaintKind::AngularGradient {
                    center: c,
                    angle: (rx.y - c.y)
                        .atan2(rx.x - c.x)
                        .to_degrees()
                        .rem_euclid(360.0),
                    stops: stops(),
                },
                "GRADIENT_DIAMOND" => PaintKind::DiamondGradient {
                    center: c,
                    radius,
                    stops: stops(),
                },
                _ => PaintKind::RadialGradient {
                    center: c,
                    radius,
                    stops: stops(),
                },
            }
        }
        "IMAGE" => {
            let hash = p.path(&["image", "hash"]).and_then(Value::as_bytes)?;
            let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            let bytes = ctx.fig.images.get(&hex)?;
            PaintKind::Image {
                asset: deesainer_format::asset_id(bytes),
                scale_mode: match s(p, "imageScaleMode") {
                    "FIT" => ImageScaleMode::Fit,
                    "TILE" => ImageScaleMode::Tile,
                    "STRETCH" => ImageScaleMode::Stretch,
                    _ => ImageScaleMode::Fill,
                },
                adjust: Default::default(),
            }
        }
        _ => return None,
    };
    Some(Paint {
        kind,
        opacity,
        visible,
        blend: blend_mode,
    })
}

fn paints(ctx: &Ctx<'_>, n: &Value, key: &str) -> Vec<Paint> {
    n.get(key)
        .map_or(&[][..], Value::as_array)
        .iter()
        .filter_map(|p| paint(ctx, p))
        .collect()
}

fn effects(n: &Value) -> Vec<Effect> {
    n.get("effects")
        .map_or(&[][..], Value::as_array)
        .iter()
        .filter_map(|e| {
            let visible = e.get("visible").and_then(Value::as_bool).unwrap_or(true);
            let color = e.get("color").map_or(Rgba::new(0.0, 0.0, 0.0, 0.25), color);
            let offset = e.get("offset").map_or(Point::new(0.0, 0.0), |o| {
                Point::new(o.num("x", 0.0), o.num("y", 0.0))
            });
            let blur = e.num("radius", 0.0);
            let spread = e.num("spread", 0.0);
            Some(match s(e, "type") {
                "DROP_SHADOW" => Effect::DropShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    visible,
                },
                "INNER_SHADOW" => Effect::InnerShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    visible,
                },
                "FOREGROUND_BLUR" => Effect::LayerBlur {
                    radius: blur,
                    visible,
                },
                "BACKGROUND_BLUR" => Effect::BackgroundBlur {
                    radius: blur,
                    visible,
                },
                "GLASS" => Effect::BackgroundBlur {
                    radius: 20.0,
                    visible,
                },
                _ => return None,
            })
        })
        .collect()
}

fn constraint(name: &str) -> Constraint {
    match name {
        "MAX" => Constraint::Max,
        "CENTER" => Constraint::Center,
        "STRETCH" => Constraint::Stretch,
        "SCALE" => Constraint::Scale,
        _ => Constraint::Min,
    }
}

fn sizing(name: &str) -> Sizing {
    if name.starts_with("RESIZE_TO_FIT") {
        Sizing::Hug
    } else {
        Sizing::Fixed
    }
}

fn auto_layout(n: &Value) -> Option<AutoLayout> {
    let direction = match s(n, "stackMode") {
        "HORIZONTAL" => LayoutDirection::Horizontal,
        "VERTICAL" => LayoutDirection::Vertical,
        _ => return None,
    };
    Some(AutoLayout {
        direction,
        wrap: s(n, "stackWrap") == "WRAP",
        gap: n.num("stackSpacing", 0.0),
        counter_gap: n.num("stackCounterSpacing", 0.0),
        padding: Padding {
            top: n.num("stackVerticalPadding", 0.0),
            right: n.num("stackPaddingRight", n.num("stackHorizontalPadding", 0.0)),
            bottom: n.num("stackPaddingBottom", n.num("stackVerticalPadding", 0.0)),
            left: n.num("stackHorizontalPadding", 0.0),
        },
        primary_align: match s(n, "stackPrimaryAlignItems") {
            "CENTER" => PrimaryAlign::Center,
            "MAX" => PrimaryAlign::End,
            "SPACE_BETWEEN" | "SPACE_EVENLY" => PrimaryAlign::SpaceBetween,
            _ => PrimaryAlign::Start,
        },
        counter_align: match s(n, "stackCounterAlignItems") {
            "CENTER" => CounterAlign::Center,
            "MAX" => CounterAlign::End,
            _ => CounterAlign::Start,
        },
        primary_sizing: sizing(s(n, "stackPrimarySizing")),
        counter_sizing: sizing(s(n, "stackCounterSizing")),
        ..AutoLayout::default()
    })
}

fn weight_of(style: &str) -> u16 {
    let s = style.to_lowercase().replace([' ', '-'], "");
    for (k, w) in [
        ("extralight", 200),
        ("ultralight", 200),
        ("thin", 100),
        ("light", 300),
        ("medium", 500),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
    ] {
        if s.contains(k) {
            return w;
        }
    }
    400
}

fn text(n: &Value) -> TextData {
    let mut t = TextData {
        content: n
            .path(&["textData", "characters"])
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        ..TextData::default()
    };
    let size = n.num("fontSize", 12.0);
    t.style.font_size = size;
    if let Some(f) = n.get("fontName") {
        t.style.font_family = s(f, "family").to_owned();
        let style = s(f, "style");
        t.style.font_weight = weight_of(style);
        t.style.italic = style.to_lowercase().contains("italic");
    }
    if let Some(lh) = n.get("lineHeight") {
        let v = lh.num("value", 100.0);
        t.style.line_height = match s(lh, "units") {
            "PIXELS" => LineHeight::Pixels(v),
            "PERCENT" if (v - 100.0).abs() < 1e-6 => LineHeight::Auto,
            "PERCENT" => LineHeight::Percent(v),
            "RAW" => LineHeight::Percent(v * 100.0),
            _ => LineHeight::Auto,
        };
    }
    if let Some(ls) = n.get("letterSpacing") {
        let v = ls.num("value", 0.0);
        t.style.letter_spacing = match s(ls, "units") {
            "PIXELS" if size > 0.0 => v / size * 100.0,
            _ => v,
        };
    }
    t.style.decoration = match s(n, "textDecoration") {
        "UNDERLINE" => TextDecoration::Underline,
        "STRIKETHROUGH" => TextDecoration::Strikethrough,
        _ => TextDecoration::None,
    };
    t.style.case = match s(n, "textCase") {
        "UPPER" => TextCase::Upper,
        "LOWER" => TextCase::Lower,
        "TITLE" => TextCase::Title,
        _ => TextCase::Original,
    };
    t.align_h = match s(n, "textAlignHorizontal") {
        "CENTER" => TextAlignH::Center,
        "RIGHT" => TextAlignH::Right,
        "JUSTIFIED" => TextAlignH::Justified,
        _ => TextAlignH::Left,
    };
    t.align_v = match s(n, "textAlignVertical") {
        "CENTER" => TextAlignV::Center,
        "BOTTOM" => TextAlignV::Bottom,
        _ => TextAlignV::Top,
    };
    t.auto_resize = match s(n, "textAutoResize") {
        "WIDTH_AND_HEIGHT" => TextAutoResize::WidthAndHeight,
        "HEIGHT" => TextAutoResize::Height,
        _ => TextAutoResize::None,
    };
    t
}

fn vector_network(
    ctx: &Ctx<'_>,
    n: &Value,
    w: f64,
    h: f64,
) -> Option<deesainer_doc::VectorNetwork> {
    let net = n
        .path(&["vectorData", "vectorNetworkBlob"])
        .and_then(Value::as_u64)
        .and_then(|i| ctx.fig.blobs.get(i as usize))?;
    let nw = n
        .path(&["vectorData", "normalizedSize", "x"])
        .and_then(Value::as_f64)
        .unwrap_or(w);
    let nh = n
        .path(&["vectorData", "normalizedSize", "y"])
        .and_then(Value::as_f64)
        .unwrap_or(h);
    let (sx, sy) = (w / nw.max(1e-9), h / nh.max(1e-9));
    let pt = |q: (f32, f32)| Point::new(f64::from(q.0) * sx, f64::from(q.1) * sy);
    let curves: Vec<[Point; 4]> = deesainer_figfile::network_segments(net)
        .into_iter()
        .map(|[a, c1, c2, b]| [pt(a), pt(c1), pt(c2), pt(b)])
        .collect();
    (!curves.is_empty()).then(|| deesainer_doc::VectorNetwork::from_cubics(&curves))
}

fn vector_path(ctx: &Ctx<'_>, n: &Value, w: f64, h: f64) -> VectorPath {
    let blob = |idx: Option<u64>| idx.and_then(|i| ctx.fig.blobs.get(i as usize));
    let pt = |x: f32, y: f32, sx: f64, sy: f64| Point::new(f64::from(x) * sx, f64::from(y) * sy);
    let mut cmds = Vec::new();
    let net = blob(
        n.path(&["vectorData", "vectorNetworkBlob"])
            .and_then(Value::as_u64),
    );
    if let Some(net) = net {
        let nw = n
            .path(&["vectorData", "normalizedSize", "x"])
            .and_then(Value::as_f64)
            .unwrap_or(w);
        let nh = n
            .path(&["vectorData", "normalizedSize", "y"])
            .and_then(Value::as_f64)
            .unwrap_or(h);
        let (sx, sy) = (w / nw.max(1e-9), h / nh.max(1e-9));
        let mut last: Option<(f32, f32)> = None;
        for [a, c1, c2, b] in deesainer_figfile::network_segments(net) {
            if last != Some(a) {
                cmds.push(PathCmd::MoveTo {
                    to: pt(a.0, a.1, sx, sy),
                });
            }
            if c1 == a && c2 == b {
                cmds.push(PathCmd::LineTo {
                    to: pt(b.0, b.1, sx, sy),
                });
            } else {
                cmds.push(PathCmd::CubicTo {
                    ctrl1: pt(c1.0, c1.1, sx, sy),
                    ctrl2: pt(c2.0, c2.1, sx, sy),
                    to: pt(b.0, b.1, sx, sy),
                });
            }
            last = Some(b);
        }
    }
    if cmds.is_empty() {
        for g in n.get("fillGeometry").map_or(&[][..], Value::as_array) {
            if let Some(b) = blob(g.get("commandsBlob").and_then(Value::as_u64)) {
                cmds.extend(geometry(b));
            }
        }
    }
    VectorPath {
        cmds,
        fill_rule: FillRule::NonZero,
    }
}

fn geometry(blob: &[u8]) -> Vec<PathCmd> {
    use deesainer_figfile::PathCmd as F;
    let p = |x: f32, y: f32| Point::new(f64::from(x), f64::from(y));
    deesainer_figfile::path_commands(blob)
        .into_iter()
        .map(|c| match c {
            F::MoveTo(x, y) => PathCmd::MoveTo { to: p(x, y) },
            F::LineTo(x, y) => PathCmd::LineTo { to: p(x, y) },
            F::QuadTo(a, b, x, y) => PathCmd::QuadTo {
                ctrl: p(a, b),
                to: p(x, y),
            },
            F::CubicTo(a, b, c, d, x, y) => PathCmd::CubicTo {
                ctrl1: p(a, b),
                ctrl2: p(c, d),
                to: p(x, y),
            },
            F::Close => PathCmd::Close,
        })
        .collect()
}

fn add_node(
    ctx: &mut Ctx<'_>,
    doc: &mut Document,
    g: Guid,
    parent: NodeId,
    depth: usize,
) -> Result<(), IoError> {
    if depth > 200 {
        return Ok(());
    }
    let Some(n) = ctx.nodes.get(&g).copied() else {
        return Ok(());
    };
    let Some(id) = ctx.ids.get(&g).copied() else {
        return Ok(());
    };
    let ty = s(n, "type");
    let w = n
        .path(&["size", "x"])
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let h = n
        .path(&["size", "y"])
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let fills = paints(ctx, n, "fillPaints");
    let mut network = None;
    let kind = match ty {
        "FRAME" | "SECTION" => {
            if n.get("resizeToFit").and_then(Value::as_bool) == Some(true) && fills.is_empty() {
                NodeKind::Group
            } else {
                NodeKind::Frame
            }
        }
        "GROUP" => NodeKind::Group,
        "SYMBOL" => NodeKind::Component {
            description: s(n, "symbolDescription").to_owned(),
            properties: Vec::new(),
        },
        "INSTANCE" => {
            let main = n
                .path(&["symbolData", "symbolID"])
                .and_then(guid)
                .and_then(|m| ctx.ids.get(&m).copied());
            match main {
                Some(component) => {
                    let overrides = overrides(ctx, n);
                    ctx.report.overrides += overrides.len();
                    NodeKind::Instance {
                        component,
                        overrides,
                    }
                }
                None => NodeKind::Frame,
            }
        }
        "RECTANGLE" | "ROUNDED_RECTANGLE" => NodeKind::Rectangle,
        "ELLIPSE" => {
            let start = n
                .path(&["arcData", "startingAngle"])
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let end = n
                .path(&["arcData", "endingAngle"])
                .and_then(Value::as_f64)
                .unwrap_or(std::f64::consts::TAU);
            NodeKind::Ellipse {
                arc_start: start.to_degrees(),
                arc_end: end.to_degrees(),
                inner_ratio: n
                    .path(&["arcData", "innerRadius"])
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            }
        }
        "LINE" => NodeKind::Line,
        "REGULAR_POLYGON" => NodeKind::Polygon {
            sides: n
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(3)
                .clamp(3, 60) as u32,
        },
        "STAR" => NodeKind::Star {
            points: n
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .clamp(3, 60) as u32,
            inner_ratio: n.num("starInnerScale", 0.382),
        },
        "VECTOR" => {
            network = vector_network(ctx, n, w, h);
            NodeKind::Vector {
                path: match &network {
                    Some(net) => net.to_path(FillRule::NonZero),
                    None => vector_path(ctx, n, w, h),
                },
            }
        }
        "BOOLEAN_OPERATION" => NodeKind::BooleanOp {
            op: match s(n, "booleanOperation") {
                "SUBTRACT" => BoolOp::Subtract,
                "INTERSECT" => BoolOp::Intersect,
                "XOR" => BoolOp::Exclude,
                _ => BoolOp::Union,
            },
        },
        "TEXT" => NodeKind::Text(text(n)),
        other => {
            if n.get("fillGeometry")
                .is_some_and(|g| !g.as_array().is_empty())
            {
                NodeKind::Vector {
                    path: vector_path(ctx, n, w, h),
                }
            } else {
                *ctx.report.skipped.entry(other.to_owned()).or_default() += 1;
                return Ok(());
            }
        }
    };
    let m = matrix(n);
    let centre = apply(m, w / 2.0, h / 2.0);
    let flipped = m[0] * m[4] - m[1] * m[3] < 0.0;
    let rotation = if flipped {
        (-m[3]).atan2(-m[0]).to_degrees()
    } else {
        m[3].atan2(m[0]).to_degrees()
    };
    let mut node = Node::new(
        id,
        n.get("name").and_then(Value::as_str).unwrap_or(ty),
        kind,
        Rect::new(centre.x - w / 2.0, centre.y - h / 2.0, w, h),
    );
    node.network = network;
    node.rotation = if rotation.abs() < 1e-6 { 0.0 } else { rotation };
    node.visible = n.get("visible").and_then(Value::as_bool).unwrap_or(true);
    node.locked = n.get("locked").and_then(Value::as_bool).unwrap_or(false);
    node.opacity = n.num("opacity", 1.0) as f32;
    node.blend = match n.get("blendMode").and_then(Value::as_str) {
        Some(b) => blend(b),
        None if matches!(node.kind, NodeKind::Group | NodeKind::Frame) => BlendMode::PassThrough,
        None => BlendMode::Normal,
    };
    if !matches!(node.kind, NodeKind::Group) {
        node.fills = fills;
    }
    node.strokes = paints(ctx, n, "strokePaints");
    node.stroke.weight = n.num("strokeWeight", 1.0);
    node.stroke.align = match s(n, "strokeAlign") {
        "INSIDE" => StrokeAlign::Inside,
        "OUTSIDE" => StrokeAlign::Outside,
        _ => StrokeAlign::Center,
    };
    node.stroke.cap = match s(n, "strokeCap") {
        "ROUND" => StrokeCap::Round,
        "SQUARE" => StrokeCap::Square,
        _ => StrokeCap::None,
    };
    node.stroke.join = match s(n, "strokeJoin") {
        "ROUND" => StrokeJoin::Round,
        "BEVEL" => StrokeJoin::Bevel,
        _ => StrokeJoin::Miter,
    };
    node.stroke.dash = n
        .get("dashPattern")
        .map_or(&[][..], Value::as_array)
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    node.effects = effects(n);
    let style = |key: &str| {
        let g = ctx.reference(n.get(key)?)?;
        let id = StyleId(ctx.ids.get(&g)?.0);
        doc.style(id).map(|_| id)
    };
    node.styles.fill = style("styleIdForFill");
    node.styles.stroke = style("styleIdForStrokeFill");
    node.styles.text = style("styleIdForText");
    node.styles.effect = style("styleIdForEffect");
    let bound = |key: &str| {
        n.get(key)
            .map_or(&[][..], Value::as_array)
            .iter()
            .find_map(|p| ctx.paint_variable(doc, p))
    };
    if !matches!(node.kind, NodeKind::Group) {
        if let Some(v) = bound("fillPaints") {
            node.bindings.insert(BindTarget::FillColor, v);
        }
    }
    if let Some(v) = bound("strokePaints") {
        node.bindings.insert(BindTarget::StrokeColor, v);
    }
    let r = n.num("cornerRadius", 0.0);
    node.corner_radius = if n
        .get("rectangleCornerRadiiIndependent")
        .and_then(Value::as_bool)
        == Some(true)
    {
        CornerRadius {
            top_left: n.num("rectangleTopLeftCornerRadius", r),
            top_right: n.num("rectangleTopRightCornerRadius", r),
            bottom_right: n.num("rectangleBottomRightCornerRadius", r),
            bottom_left: n.num("rectangleBottomLeftCornerRadius", r),
        }
    } else {
        CornerRadius::uniform(r)
    };
    node.constraints = Constraints {
        horizontal: constraint(s(n, "horizontalConstraint")),
        vertical: constraint(s(n, "verticalConstraint")),
    };
    if node.kind.is_frame_like() {
        node.clips_content = n.get("frameMaskDisabled").and_then(Value::as_bool) == Some(false);
        node.layout = auto_layout(n);
    }
    node.layout_child.absolute = s(n, "stackPositioning") == "ABSOLUTE";
    let grow = n.num("stackChildPrimaryGrow", 0.0) > 0.0;
    let stretch = s(n, "stackChildAlignSelf") == "STRETCH";
    if let Some(dir) = doc.get(parent).and_then(|p| p.layout).map(|l| l.direction) {
        let (primary, counter) = (
            if grow { Sizing::Fill } else { Sizing::Fixed },
            if stretch { Sizing::Fill } else { Sizing::Fixed },
        );
        (node.layout_child.horizontal, node.layout_child.vertical) = match dir {
            LayoutDirection::Horizontal | LayoutDirection::Grid => (primary, counter),
            LayoutDirection::Vertical => (counter, primary),
        };
    }
    node.is_mask = n.get("mask").and_then(Value::as_bool) == Some(true);
    let container = node.kind.is_container();
    let at = doc.get(parent).map_or(0, |p| p.children.len());
    doc.insert(node, parent, at)
        .map_err(|e| IoError::Other(e.to_string()))?;
    ctx.report.layers += 1;
    if container {
        for child in ctx.children.get(&g).cloned().unwrap_or_default() {
            add_node(ctx, doc, child, id, depth + 1)?;
        }
    }
    if flipped {
        mirror_content(doc, id, 0);
    }
    Ok(())
}

fn mirror_content(doc: &mut Document, id: NodeId, depth: usize) {
    if depth > 200 {
        return;
    }
    let Some(node) = doc.get_mut(id) else {
        return;
    };
    let w = node.rect.width;
    let flip = |q: Point| Point::new(w - q.x, q.y);
    match &mut node.kind {
        NodeKind::Vector { path } => *path = path.map_points(flip),
        NodeKind::Ellipse {
            arc_start, arc_end, ..
        } if (*arc_end - *arc_start).abs() < 360.0 => {
            (*arc_start, *arc_end) = (180.0 - *arc_end, 180.0 - *arc_start);
        }
        _ => {}
    }
    if let Some(net) = &mut node.network {
        for v in &mut net.vertices {
            *v = flip(*v);
        }
        for seg in &mut net.segments {
            seg.ta.x = -seg.ta.x;
            seg.tb.x = -seg.tb.x;
        }
    }
    node.fills = deesainer_doc::mirror_paints(&node.fills, true);
    node.strokes = deesainer_doc::mirror_paints(&node.strokes, true);
    let children = node.children.clone();
    for c in children {
        if let Some(child) = doc.get_mut(c) {
            child.rect.x = w - child.rect.x - child.rect.width;
            child.rotation = -child.rotation;
        }
        mirror_content(doc, c, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage_cleanly() {
        assert!(import_fig(b"not a fig").is_err());
        assert!(import_fig(b"fig-kiwi\x01\x00\x00\x00").is_err());
    }

    #[test]
    fn gradient_handles_follow_the_inverse_transform() {
        let inv = invert([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap();
        assert_eq!(apply(inv, 0.0, 0.5), Point::new(0.0, 0.5));
        let inv = invert([2.0, 0.0, 0.0, 0.0, 2.0, 0.0]).unwrap();
        assert_eq!(apply(inv, 1.0, 0.5), Point::new(0.5, 0.25));
    }

    fn obj(fields: &[(&str, Value)]) -> Value {
        Value::Object(
            fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        )
    }

    fn id(local: u64) -> Value {
        obj(&[
            ("sessionID", Value::UInt(1)),
            ("localID", Value::UInt(local)),
        ])
    }

    fn st(v: &str) -> Value {
        Value::Str(v.to_owned())
    }

    fn en(v: &str) -> Value {
        Value::Enum(v.to_owned())
    }

    fn rgb(r: f64) -> Value {
        obj(&[
            ("r", Value::Float(r)),
            ("g", Value::Float(0.0)),
            ("b", Value::Float(0.0)),
            ("a", Value::Float(1.0)),
        ])
    }

    fn node(
        local: u64,
        ty: &str,
        name: &str,
        parent: Option<u64>,
        extra: &[(&str, Value)],
    ) -> Value {
        let mut f = vec![
            ("guid", id(local)),
            ("type", en(ty)),
            ("name", st(name)),
            (
                "size",
                obj(&[("x", Value::Float(100.0)), ("y", Value::Float(40.0))]),
            ),
        ];
        if let Some(p) = parent {
            f.push((
                "parentIndex",
                obj(&[("guid", id(p)), ("position", st(&format!("{local}")))]),
            ));
        }
        f.extend(extra.iter().cloned());
        obj(&f)
    }

    #[test]
    fn styles_variables_and_overrides_come_through() {
        let solid = |extra: &[(&str, Value)]| {
            let mut f = vec![("type", en("SOLID")), ("color", rgb(0.5))];
            f.extend(extra.iter().cloned());
            Value::Array(vec![obj(&f)])
        };
        let nodes = vec![
            node(0, "DOCUMENT", "Document", None, &[]),
            node(1, "CANVAS", "Page", Some(0), &[]),
            node(
                2,
                "CANVAS",
                "Internal",
                Some(0),
                &[("internalOnly", Value::Bool(true))],
            ),
            node(
                3,
                "RECTANGLE",
                "Brand",
                Some(2),
                &[
                    ("styleType", en("FILL")),
                    ("key", st("style-key")),
                    ("fillPaints", solid(&[])),
                ],
            ),
            node(
                4,
                "VARIABLE_SET",
                "Theme",
                Some(2),
                &[(
                    "variableSetModes",
                    Value::Array(vec![
                        obj(&[
                            ("id", id(41)),
                            ("name", st("Dark")),
                            ("sortPosition", st("b")),
                        ]),
                        obj(&[
                            ("id", id(40)),
                            ("name", st("Light")),
                            ("sortPosition", st("a")),
                        ]),
                    ]),
                )],
            ),
            node(
                5,
                "VARIABLE",
                "Accent",
                Some(2),
                &[
                    ("key", st("var-key")),
                    ("variableSetID", obj(&[("guid", id(4))])),
                    (
                        "variableDataValues",
                        obj(&[(
                            "entries",
                            Value::Array(vec![
                                obj(&[
                                    ("modeID", id(40)),
                                    (
                                        "variableData",
                                        obj(&[
                                            ("value", obj(&[("colorValue", rgb(1.0))])),
                                            ("dataType", en("COLOR")),
                                        ]),
                                    ),
                                ]),
                                obj(&[
                                    ("modeID", id(41)),
                                    (
                                        "variableData",
                                        obj(&[
                                            ("value", obj(&[("colorValue", rgb(0.25))])),
                                            ("dataType", en("COLOR")),
                                        ]),
                                    ),
                                ]),
                            ]),
                        )]),
                    ),
                ],
            ),
            node(
                6,
                "RECTANGLE",
                "Card",
                Some(1),
                &[
                    (
                        "styleIdForFill",
                        obj(&[("assetRef", obj(&[("key", st("style-key"))]))]),
                    ),
                    (
                        "fillPaints",
                        solid(&[(
                            "colorVar",
                            obj(&[(
                                "value",
                                obj(&[(
                                    "alias",
                                    obj(&[("assetRef", obj(&[("key", st("var-key"))]))]),
                                )]),
                            )]),
                        )]),
                    ),
                ],
            ),
            node(7, "SYMBOL", "Button", Some(1), &[]),
            node(
                8,
                "TEXT",
                "Label",
                Some(7),
                &[
                    ("textData", obj(&[("characters", st("Buy"))])),
                    ("fontSize", Value::Float(14.0)),
                ],
            ),
            node(
                9,
                "INSTANCE",
                "Button",
                Some(1),
                &[(
                    "symbolData",
                    obj(&[
                        ("symbolID", id(7)),
                        (
                            "symbolOverrides",
                            Value::Array(vec![obj(&[
                                ("guidPath", obj(&[("guids", Value::Array(vec![id(8)]))])),
                                ("textData", obj(&[("characters", st("Subscribe"))])),
                                ("visible", Value::Bool(false)),
                            ])]),
                        ),
                    ]),
                )],
            ),
        ];
        let fig = FigFile {
            magic: "fig-kiwi".into(),
            version: 1,
            message: obj(&[("nodeChanges", Value::Array(nodes))]),
            blobs: Vec::new(),
            images: BTreeMap::new(),
            meta: None,
        };
        let (doc, report) = convert(&fig).unwrap();
        assert_eq!(
            (report.styles, report.variables, report.overrides),
            (1, 1, 2)
        );
        let (style_id, style) = doc.styles().next().unwrap();
        assert_eq!(style.name, "Brand");
        let card = doc.get(doc.find_by_name("Card").unwrap()).unwrap();
        assert_eq!(card.styles.fill, Some(style_id));
        assert_eq!(doc.modes().name, "Theme");
        assert_eq!(doc.modes().names, ["Light", "Dark"]);
        let (var_id, var) = doc.variables().next().unwrap();
        assert_eq!(
            var.values[0],
            VarValue::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))
        );
        assert_eq!(
            var.values[1],
            VarValue::Color(Rgba::new(0.25, 0.0, 0.0, 1.0))
        );
        assert_eq!(card.bindings.get(&BindTarget::FillColor), Some(&var_id));
        let label = doc.find_by_name("Label").unwrap();
        let instance = doc
            .nodes()
            .find(|n| matches!(n.kind, NodeKind::Instance { .. }))
            .unwrap();
        let NodeKind::Instance { overrides, .. } = &instance.kind else {
            panic!("not an instance");
        };
        assert!(overrides.iter().all(|o| o.target == label));
        assert!(overrides.iter().any(|o| matches!(
            &o.value,
            PropValue::Kind(NodeKind::Text(t)) if t.content == "Subscribe" && t.style.font_size == 14.0
        )));
        assert!(overrides
            .iter()
            .any(|o| o.value == PropValue::Visible(false)));
    }

    #[test]
    fn mirrored_layers_are_flipped_not_turned() {
        let t = |a: f64, tx: f64| {
            obj(&[
                ("m00", Value::Float(a)),
                ("m01", Value::Float(0.0)),
                ("m02", Value::Float(tx)),
                ("m10", Value::Float(0.0)),
                ("m11", Value::Float(1.0)),
                ("m12", Value::Float(0.0)),
            ])
        };
        let nodes = vec![
            node(0, "DOCUMENT", "Document", None, &[]),
            node(1, "CANVAS", "Page", Some(0), &[]),
            node(
                2,
                "FRAME",
                "Logo",
                Some(1),
                &[("transform", t(-1.0, 100.0))],
            ),
            obj(&[
                ("guid", id(3)),
                ("type", en("RECTANGLE")),
                ("name", st("Mark")),
                (
                    "size",
                    obj(&[("x", Value::Float(10.0)), ("y", Value::Float(10.0))]),
                ),
                (
                    "parentIndex",
                    obj(&[("guid", id(2)), ("position", st("a"))]),
                ),
                ("transform", t(1.0, 0.0)),
            ]),
        ];
        let fig = FigFile {
            magic: "fig-kiwi".into(),
            version: 1,
            message: obj(&[("nodeChanges", Value::Array(nodes))]),
            blobs: Vec::new(),
            images: BTreeMap::new(),
            meta: None,
        };
        let (doc, _) = convert(&fig).unwrap();
        let logo = doc.get(doc.find_by_name("Logo").unwrap()).unwrap();
        assert_eq!(logo.rotation, 0.0);
        assert_eq!(logo.rect.x, 0.0);
        let mark = doc.get(doc.find_by_name("Mark").unwrap()).unwrap();
        assert_eq!(mark.rect.x, 90.0);
    }

    #[test]
    fn fig_sample() {
        let Ok(path) = std::env::var("DEESAINER_FIG_SAMPLE") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let (doc, report) = import_fig(&bytes).unwrap();
        println!("{report:?}");
        for n in doc.nodes() {
            println!(
                "{:?} {} {:?} rot={} parent={:?}",
                n.kind.type_name(),
                n.name,
                n.rect,
                n.rotation,
                n.parent
            );
        }
        assert!(report.layers > 0);
    }
}
