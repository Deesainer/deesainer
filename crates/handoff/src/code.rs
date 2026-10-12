use std::fmt::Write as _;

use deesainer_color::Rgba;
use deesainer_doc::{Document, NodeId};

use crate::{spec, PaintSpec, Spec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Css,
    SwiftUi,
    Compose,
}

impl Language {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "css" => Some(Self::Css),
            "swiftui" => Some(Self::SwiftUi),
            "compose" => Some(Self::Compose),
            _ => None,
        }
    }
}

pub fn code(doc: &Document, id: NodeId, language: Language) -> Option<String> {
    let spec = spec(doc, id)?;
    Some(match language {
        Language::Css => css(&spec),
        Language::SwiftUi => swiftui(&spec),
        Language::Compose => compose(&spec),
    })
}

fn n(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let r = if r == 0.0 { 0.0 } else { r };
    let s = format!("{r:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_owned()
}

fn color(hex: &str) -> Rgba {
    Rgba::from_hex(hex).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0))
}

fn css_var(name: &str) -> String {
    let mut out = String::from("--");
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && out.len() > 2 {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn css_color(hex: &str, opacity: f32) -> String {
    let c = color(hex);
    let a = (c.a * opacity).clamp(0.0, 1.0);
    if a >= 0.999 {
        Rgba::new(c.r, c.g, c.b, 1.0).to_hex()
    } else {
        let [r, g, b, _] = c.to_rgba8();
        format!("rgba({r}, {g}, {b}, {})", n(f64::from(a)))
    }
}

fn css_linear(p: &PaintSpec, w: f64, h: f64) -> Option<String> {
    let (s, e) = (p.start?, p.end?);
    let (sx, sy, ex, ey) = (s[0] * w, s[1] * h, e[0] * w, e[1] * h);
    let (dx, dy) = (ex - sx, ey - sy);
    let len = dx.hypot(dy);
    if len <= f64::EPSILON {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let angle = ux.atan2(-uy).to_degrees().rem_euclid(360.0);
    let line = (w * ux).abs() + (h * uy).abs();
    let (cx, cy) = (w / 2.0 - ux * line / 2.0, h / 2.0 - uy * line / 2.0);
    let stops: Vec<String> = p
        .stops
        .iter()
        .map(|(pos, hex)| {
            let t = f64::from(*pos);
            let (px, py) = (sx + dx * t, sy + dy * t);
            let at = if line > f64::EPSILON {
                ((px - cx) * ux + (py - cy) * uy) / line
            } else {
                t
            };
            format!("{} {}%", css_color(hex, p.opacity), n(at * 100.0))
        })
        .collect();
    Some(format!(
        "linear-gradient({}deg, {})",
        n(angle),
        stops.join(", ")
    ))
}

fn css_paint(p: &PaintSpec, w: f64, h: f64) -> Option<String> {
    Some(match p.kind {
        "solid" => {
            let c = css_color(p.color.as_deref()?, p.opacity);
            format!("linear-gradient({c}, {c})")
        }
        "linearGradient" => css_linear(p, w, h)?,
        "radialGradient" => {
            let (c, r) = (p.center?, p.radius?);
            let stops: Vec<String> = p
                .stops
                .iter()
                .map(|(pos, hex)| {
                    format!(
                        "{} {}%",
                        css_color(hex, p.opacity),
                        n(f64::from(*pos) * 100.0)
                    )
                })
                .collect();
            format!(
                "radial-gradient({}px {}px at {}% {}%, {})",
                n(r[0] * w),
                n(r[1] * h),
                n(c[0] * 100.0),
                n(c[1] * 100.0),
                stops.join(", ")
            )
        }
        "angularGradient" => {
            let (c, a) = (p.center?, p.angle?);
            let stops: Vec<String> = p
                .stops
                .iter()
                .map(|(pos, hex)| {
                    format!(
                        "{} {}%",
                        css_color(hex, p.opacity),
                        n(f64::from(*pos) * 100.0)
                    )
                })
                .collect();
            format!(
                "conic-gradient(from {}deg at {}% {}%, {})",
                n((a + 90.0).rem_euclid(360.0)),
                n(c[0] * 100.0),
                n(c[1] * 100.0),
                stops.join(", ")
            )
        }
        "diamondGradient" => {
            let (c, r) = (p.center?, p.radius?);
            let stops: Vec<String> = p
                .stops
                .iter()
                .map(|(pos, hex)| {
                    format!(
                        "{} {}%",
                        css_color(hex, p.opacity),
                        n(f64::from(*pos) * 100.0)
                    )
                })
                .collect();
            format!(
                "/* diamond gradient, approximated */ radial-gradient({}px {}px at {}% {}%, {})",
                n(r[0] * w),
                n(r[1] * h),
                n(c[0] * 100.0),
                n(c[1] * 100.0),
                stops.join(", ")
            )
        }
        "image" => format!("url(\"{}\")", p.asset.as_deref().unwrap_or_default()),
        _ => return None,
    })
}

fn css(spec: &Spec) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        let _ = writeln!(out, "{s}");
    };
    line(format!("/* {} */", spec.name.replace("*/", "* /")));
    for s in &spec.styles {
        line(format!(
            "/* {} style: {} */",
            s.property,
            s.name.replace("*/", "* /")
        ));
    }
    let var_for = |property: &str| spec.variables.iter().find(|v| v.property == property);
    let flowed = spec.in_layout.as_ref().is_some_and(|l| !l.absolute);
    if !flowed {
        line("position: absolute;".into());
        line(format!("left: {}px;", n(spec.x)));
        line(format!("top: {}px;", n(spec.y)));
    }
    let (w, h) = (spec.width, spec.height);
    let hug_w = spec.layout.as_ref().is_some_and(|l| {
        (l.direction == "row" && l.primary_sizing == "hug")
            || (l.direction == "column" && l.counter_sizing == "hug")
    }) || spec
        .in_layout
        .as_ref()
        .is_some_and(|l| l.horizontal == "hug");
    let hug_h = spec.layout.as_ref().is_some_and(|l| {
        (l.direction == "column" && l.primary_sizing == "hug")
            || (l.direction == "row" && l.counter_sizing == "hug")
    }) || spec.in_layout.as_ref().is_some_and(|l| l.vertical == "hug");
    match var_for("width") {
        Some(v) => line(format!("width: var({}, {}px);", css_var(&v.name), n(w))),
        None if hug_w => line("width: fit-content;".into()),
        None => line(format!("width: {}px;", n(w))),
    }
    match var_for("height") {
        Some(v) => line(format!("height: var({}, {}px);", css_var(&v.name), n(h))),
        None if hug_h => line("height: fit-content;".into()),
        None => line(format!("height: {}px;", n(h))),
    }
    let l = spec.size_limits;
    for (prop, v) in [
        ("min-width", l.min_width),
        ("max-width", l.max_width),
        ("min-height", l.min_height),
        ("max-height", l.max_height),
    ] {
        if let Some(v) = v {
            line(format!("{prop}: {}px;", n(v)));
        }
    }
    if let Some(l) = &spec.in_layout {
        let (along, across) = if l.direction == "row" {
            (l.horizontal, l.vertical)
        } else {
            (l.vertical, l.horizontal)
        };
        if along == "fill" {
            line("flex: 1 0 0;".into());
        }
        if across == "fill" {
            line("align-self: stretch;".into());
        }
    }
    if let Some((cols, rows)) = spec.layout.as_ref().and_then(|l| l.grid) {
        let l = spec.layout.as_ref();
        line("display: grid;".into());
        line(format!("grid-template-columns: repeat({cols}, 1fr);"));
        if rows > 0 {
            line(format!("grid-template-rows: repeat({rows}, 1fr);"));
        }
        if let Some(l) = l {
            line(format!("gap: {}px {}px;", n(l.counter_gap), n(l.gap)));
            let [t, r, b, lft] = l.padding;
            if [t, r, b, lft].iter().any(|v| *v != 0.0) {
                line(format!(
                    "padding: {}px {}px {}px {}px;",
                    n(t),
                    n(r),
                    n(b),
                    n(lft)
                ));
            }
        }
    } else if let Some(l) = &spec.layout {
        line("display: flex;".into());
        line(format!("flex-direction: {};", l.direction));
        if l.wrap {
            line("flex-wrap: wrap;".into());
        }
        let gap = match var_for("gap") {
            Some(v) => format!("var({}, {}px)", css_var(&v.name), n(l.gap)),
            None => format!("{}px", n(l.gap)),
        };
        if l.primary_align != "spaceBetween" {
            if l.wrap {
                line(format!("gap: {}px {gap};", n(l.counter_gap)));
            } else {
                line(format!("gap: {gap};"));
            }
        }
        let [t, r, b, lft] = l.padding;
        if [t, r, b, lft].iter().any(|v| *v != 0.0) {
            line(format!(
                "padding: {}px {}px {}px {}px;",
                n(t),
                n(r),
                n(b),
                n(lft)
            ));
        }
        line(format!(
            "justify-content: {};",
            match l.primary_align {
                "center" => "center",
                "end" => "flex-end",
                "spaceBetween" => "space-between",
                _ => "flex-start",
            }
        ));
        line(format!(
            "align-items: {};",
            match l.counter_align {
                "center" => "center",
                "end" => "flex-end",
                _ => "flex-start",
            }
        ));
    }
    if spec.rotation != 0.0 {
        line(format!("transform: rotate({}deg);", n(spec.rotation)));
    }
    if let Some([tl, tr, br, bl]) = spec.corner_radius {
        let radius = if tl == tr && tr == br && br == bl {
            match var_for("cornerRadius") {
                Some(v) => format!("var({}, {}px)", css_var(&v.name), n(tl)),
                None => format!("{}px", n(tl)),
            }
        } else {
            format!("{}px {}px {}px {}px", n(tl), n(tr), n(br), n(bl))
        };
        line(format!("border-radius: {radius};"));
    } else if spec.kind == "ellipse" {
        line("border-radius: 50%;".into());
    }
    if spec.clips_content {
        line("overflow: hidden;".into());
    }
    if let Some(t) = &spec.text {
        line(format!(
            "font-family: \"{}\";",
            t.font_family.replace('"', "'")
        ));
        line(format!("font-size: {}px;", n(t.font_size)));
        line(format!("font-weight: {};", t.font_weight));
        if !t.features.is_empty() {
            let list: Vec<String> = t
                .features
                .iter()
                .map(|(tag, v)| format!("\"{tag}\" {v}"))
                .collect();
            line(format!("font-feature-settings: {};", list.join(", ")));
        }
        if !t.variations.is_empty() {
            let list: Vec<String> = t
                .variations
                .iter()
                .map(|(tag, v)| format!("\"{tag}\" {}", n(f64::from(*v))))
                .collect();
            line(format!("font-variation-settings: {};", list.join(", ")));
        }
        if t.italic {
            line("font-style: italic;".into());
        }
        match t.line_height {
            Some(lh) => line(format!("line-height: {}px;", n(lh))),
            None => line("line-height: normal;".into()),
        }
        if t.letter_spacing != 0.0 {
            line(format!("letter-spacing: {}px;", n(t.letter_spacing)));
        }
        line(format!("text-align: {};", t.align));
        if let Some(d) = t.decoration {
            line(format!("text-decoration: {d};"));
        }
        if let Some(c) = t.case {
            line(format!("text-transform: {c};"));
        }
        if let [p] = spec.fills.as_slice() {
            if p.kind == "solid" {
                if let Some(hex) = &p.color {
                    let c = css_color(hex, p.opacity);
                    match var_for("fillColor") {
                        Some(v) => line(format!("color: var({}, {c});", css_var(&v.name))),
                        None => line(format!("color: {c};")),
                    }
                }
            }
        }
    }
    let text_solid =
        spec.text.is_some() && matches!(spec.fills.as_slice(), [p] if p.kind == "solid");
    if !spec.fills.is_empty() && !text_solid {
        let layers: Vec<String> = spec
            .fills
            .iter()
            .rev()
            .filter_map(|p| css_paint(p, w, h))
            .collect();
        match (spec.fills.as_slice(), var_for("fillColor")) {
            ([p], Some(v)) if p.kind == "solid" => {
                let c = css_color(p.color.as_deref().unwrap_or("#000000"), p.opacity);
                line(format!("background: var({}, {c});", css_var(&v.name)));
            }
            ([p], None) if p.kind == "solid" => {
                line(format!(
                    "background: {};",
                    css_color(p.color.as_deref().unwrap_or("#000000"), p.opacity)
                ));
            }
            _ if !layers.is_empty() => {
                line(format!("background: {};", layers.join(", ")));
                let sizes: Vec<&str> = spec
                    .fills
                    .iter()
                    .rev()
                    .map(|p| match p.scale_mode {
                        Some("fill") => "cover",
                        Some("fit") => "contain",
                        Some("stretch") => "100% 100%",
                        _ => "auto",
                    })
                    .collect();
                if spec.fills.iter().any(|p| p.kind == "image") {
                    line(format!("background-size: {};", sizes.join(", ")));
                    line("background-position: center;".into());
                }
                if spec.text.is_some() {
                    line("background-clip: text;".into());
                    line("color: transparent;".into());
                }
            }
            _ => {}
        }
        let blends: Vec<&str> = spec
            .fills
            .iter()
            .rev()
            .map(|p| p.blend.unwrap_or("normal"))
            .collect();
        if blends.iter().any(|b| *b != "normal") {
            line(format!("background-blend-mode: {};", blends.join(", ")));
        }
    }
    let mut shadows: Vec<String> = Vec::new();
    if let (Some(st), Some(p)) = (&spec.stroke, spec.strokes.first()) {
        let c = css_color(
            p.color
                .as_deref()
                .or(p.stops.first().map(|s| s.1.as_str()))
                .unwrap_or("#000000"),
            p.opacity,
        );
        let style = if st.dash.is_empty() {
            "solid"
        } else {
            "dashed"
        };
        match st.align {
            _ if st.sides.is_some() => {
                let [t, r, b, l] = st.sides.unwrap_or_default();
                line(format!("border-style: {style};"));
                line(format!("border-color: {c};"));
                line(format!(
                    "border-width: {}px {}px {}px {}px;",
                    n(t),
                    n(r),
                    n(b),
                    n(l)
                ));
                if st.align == "inside" {
                    line("box-sizing: border-box;".into());
                }
            }
            "inside" if st.dash.is_empty() => {
                shadows.push(format!("inset 0 0 0 {}px {c}", n(st.weight)))
            }
            "inside" => {
                line(format!("border: {}px {style} {c};", n(st.weight)));
                line("box-sizing: border-box;".into());
            }
            "outside" => line(format!("outline: {}px {style} {c};", n(st.weight))),
            _ => {
                line(format!("outline: {}px {style} {c};", n(st.weight)));
                line(format!("outline-offset: -{}px;", n(st.weight / 2.0)));
            }
        }
    }
    let mut filters = Vec::new();
    for e in &spec.effects {
        match e.kind {
            "dropShadow" | "innerShadow" => {
                let c = css_color(e.color.as_deref().unwrap_or("#00000040"), 1.0);
                let inset = if e.kind == "innerShadow" {
                    "inset "
                } else {
                    ""
                };
                shadows.push(format!(
                    "{inset}{}px {}px {}px {}px {c}",
                    n(e.x),
                    n(e.y),
                    n(e.blur),
                    n(e.spread)
                ));
            }
            "layerBlur" => filters.push(format!("blur({}px)", n(e.blur / 2.0))),
            "backgroundBlur" => line(format!("backdrop-filter: blur({}px);", n(e.blur / 2.0))),
            _ => {}
        }
    }
    if !shadows.is_empty() {
        line(format!("box-shadow: {};", shadows.join(", ")));
    }
    if !filters.is_empty() {
        line(format!("filter: {};", filters.join(" ")));
    }
    if spec.opacity < 1.0 {
        match var_for("opacity") {
            Some(v) => line(format!(
                "opacity: var({}, {});",
                css_var(&v.name),
                n(f64::from(spec.opacity))
            )),
            None => line(format!("opacity: {};", n(f64::from(spec.opacity)))),
        }
    }
    if let Some(b) = spec.blend {
        line(format!("mix-blend-mode: {b};"));
    }
    out
}

fn swift_color(hex: &str, opacity: f32) -> String {
    let c = color(hex);
    let a = (c.a * opacity).clamp(0.0, 1.0);
    let base = format!(
        "Color(red: {}, green: {}, blue: {})",
        n(f64::from(c.r)),
        n(f64::from(c.g)),
        n(f64::from(c.b))
    );
    if a >= 0.999 {
        base
    } else {
        format!("{base}.opacity({})", n(f64::from(a)))
    }
}

fn swift_paint(p: &PaintSpec) -> Option<String> {
    let stops = |p: &PaintSpec| {
        p.stops
            .iter()
            .map(|(pos, hex)| {
                format!(
                    ".init(color: {}, location: {})",
                    swift_color(hex, p.opacity),
                    n(f64::from(*pos))
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(match p.kind {
        "solid" => swift_color(p.color.as_deref()?, p.opacity),
        "linearGradient" => {
            let (s, e) = (p.start?, p.end?);
            format!(
                "LinearGradient(stops: [{}], startPoint: UnitPoint(x: {}, y: {}), endPoint: UnitPoint(x: {}, y: {}))",
                stops(p),
                n(s[0]),
                n(s[1]),
                n(e[0]),
                n(e[1])
            )
        }
        "angularGradient" => {
            let (c, a) = (p.center?, p.angle?);
            format!(
                "AngularGradient(stops: [{}], center: UnitPoint(x: {}, y: {}), startAngle: .degrees({}), endAngle: .degrees({}))",
                stops(p),
                n(c[0]),
                n(c[1]),
                n(a),
                n(a + 360.0)
            )
        }
        "radialGradient" | "diamondGradient" => {
            let (c, r) = (p.center?, p.radius?);
            format!(
                "EllipticalGradient(stops: [{}], center: UnitPoint(x: {}, y: {}), startRadiusFraction: 0, endRadiusFraction: {})",
                stops(p),
                n(c[0]),
                n(c[1]),
                n(r[0].max(r[1]))
            )
        }
        _ => return None,
    })
}

fn swiftui(spec: &Spec) -> String {
    let mut out = format!("// {}\n", spec.name.replace('\n', " "));
    let shape = match spec.corner_radius {
        Some([r, ..]) => format!("RoundedRectangle(cornerRadius: {})", n(r)),
        None if spec.kind == "ellipse" => "Ellipse()".to_owned(),
        None => "Rectangle()".to_owned(),
    };
    let mut mods: Vec<String> = Vec::new();
    if let Some(t) = &spec.text {
        let _ = writeln!(out, "Text({:?})", t.content);
        let weight = match t.font_weight {
            0..=149 => ".ultraLight",
            150..=249 => ".thin",
            250..=349 => ".light",
            350..=449 => ".regular",
            450..=549 => ".medium",
            550..=649 => ".semibold",
            650..=749 => ".bold",
            750..=849 => ".heavy",
            _ => ".black",
        };
        mods.push(format!(
            ".font(.custom({:?}, size: {}).weight({weight}))",
            t.font_family,
            n(t.font_size)
        ));
        if t.italic {
            mods.push(".italic()".into());
        }
        if let Some(c) = spec.fills.first().and_then(swift_paint) {
            mods.push(format!(".foregroundStyle({c})"));
        }
        if t.letter_spacing != 0.0 {
            mods.push(format!(".kerning({})", n(t.letter_spacing)));
        }
        if let Some(lh) = t.line_height {
            mods.push(format!(
                ".lineSpacing({})",
                n((lh - t.font_size * 1.2).max(0.0))
            ));
        }
        match t.align {
            "center" => mods.push(".multilineTextAlignment(.center)".into()),
            "right" => mods.push(".multilineTextAlignment(.trailing)".into()),
            _ => {}
        }
        match t.decoration {
            Some("underline") => mods.push(".underline()".into()),
            Some("line-through") => mods.push(".strikethrough()".into()),
            _ => {}
        }
        match t.case {
            Some("uppercase") => mods.push(".textCase(.uppercase)".into()),
            Some("lowercase") => mods.push(".textCase(.lowercase)".into()),
            _ => {}
        }
    } else if let Some(l) = &spec.layout {
        let stack = if l.direction == "row" {
            "HStack"
        } else {
            "VStack"
        };
        let align = match (l.direction, l.counter_align) {
            ("row", "center") => ".center",
            ("row", "end") => ".bottom",
            ("row", _) => ".top",
            (_, "center") => ".center",
            (_, "end") => ".trailing",
            _ => ".leading",
        };
        let _ = writeln!(
            out,
            "{stack}(alignment: {align}, spacing: {}) {{\n    // children\n}}",
            n(l.gap)
        );
        let [t, r, b, lft] = l.padding;
        if [t, r, b, lft].iter().any(|v| *v != 0.0) {
            mods.push(format!(
                ".padding(EdgeInsets(top: {}, leading: {}, bottom: {}, trailing: {}))",
                n(t),
                n(lft),
                n(b),
                n(r)
            ));
        }
        if let Some(c) = spec.fills.first().and_then(swift_paint) {
            mods.push(format!(".background({c}, in: {shape})"));
        }
    } else {
        let _ = writeln!(out, "{shape}");
        if let Some(c) = spec.fills.first().and_then(swift_paint) {
            mods.push(format!(".fill({c})"));
        }
        if spec.fills.iter().any(|p| p.kind == "image") {
            out.push_str(
                "// image fill: Image(\"…\").resizable().aspectRatio(contentMode: .fill)\n",
            );
        }
    }
    mods.push(format!(
        ".frame(width: {}, height: {})",
        n(spec.width),
        n(spec.height)
    ));
    if let (Some(st), Some(p)) = (&spec.stroke, spec.strokes.first()) {
        if let Some(c) = swift_paint(p) {
            let style = if st.dash.is_empty() {
                format!("lineWidth: {}", n(st.weight))
            } else {
                let dash: Vec<String> = st.dash.iter().map(|d| n(*d)).collect();
                format!(
                    "style: StrokeStyle(lineWidth: {}, dash: [{}])",
                    n(st.weight),
                    dash.join(", ")
                )
            };
            let call = if st.align == "inside" {
                "strokeBorder"
            } else {
                "stroke"
            };
            mods.push(format!(".overlay({shape}.{call}({c}, {style}))"));
        }
    }
    for e in &spec.effects {
        match e.kind {
            "dropShadow" => mods.push(format!(
                ".shadow(color: {}, radius: {}, x: {}, y: {})",
                swift_color(e.color.as_deref().unwrap_or("#00000040"), 1.0),
                n(e.blur / 2.0),
                n(e.x),
                n(e.y)
            )),
            "layerBlur" => mods.push(format!(".blur(radius: {})", n(e.blur / 2.0))),
            "innerShadow" => mods.push("// inner shadow: no direct SwiftUI modifier".into()),
            "backgroundBlur" => mods.push(".background(.ultraThinMaterial)".into()),
            _ => {}
        }
    }
    if spec.rotation != 0.0 {
        mods.push(format!(".rotationEffect(.degrees({}))", n(spec.rotation)));
    }
    if spec.opacity < 1.0 {
        mods.push(format!(".opacity({})", n(f64::from(spec.opacity))));
    }
    if let Some(b) = spec.blend {
        mods.push(format!(".blendMode(.{})", swift_blend(b)));
    }
    let flowed = spec.in_layout.as_ref().is_some_and(|l| !l.absolute);
    if !flowed {
        mods.push(format!(
            ".position(x: {}, y: {})",
            n(spec.x + spec.width / 2.0),
            n(spec.y + spec.height / 2.0)
        ));
    }
    for m in mods {
        let _ = writeln!(out, "    {m}");
    }
    out
}

fn swift_blend(css: &str) -> &'static str {
    match css {
        "multiply" => "multiply",
        "screen" => "screen",
        "overlay" => "overlay",
        "darken" => "darken",
        "lighten" => "lighten",
        "color-dodge" => "colorDodge",
        "color-burn" => "colorBurn",
        "soft-light" => "softLight",
        "hard-light" => "hardLight",
        "difference" => "difference",
        "exclusion" => "exclusion",
        "hue" => "hue",
        "saturation" => "saturation",
        "color" => "color",
        "luminosity" => "luminosity",
        _ => "normal",
    }
}

fn kotlin_color(hex: &str, opacity: f32) -> String {
    let c = color(hex);
    let [r, g, b, a] = Rgba::new(c.r, c.g, c.b, (c.a * opacity).clamp(0.0, 1.0)).to_rgba8();
    format!("Color(0x{a:02X}{r:02X}{g:02X}{b:02X})")
}

fn compose_brush(p: &PaintSpec, w: f64, h: f64) -> Option<String> {
    let stops = |p: &PaintSpec| {
        p.stops
            .iter()
            .map(|(pos, hex)| {
                format!(
                    "{}f to {}",
                    n(f64::from(*pos)),
                    kotlin_color(hex, p.opacity)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(match p.kind {
        "solid" => kotlin_color(p.color.as_deref()?, p.opacity),
        "linearGradient" => {
            let (s, e) = (p.start?, p.end?);
            format!(
                "Brush.linearGradient({}, start = Offset({}f, {}f), end = Offset({}f, {}f))",
                stops(p),
                n(s[0] * w),
                n(s[1] * h),
                n(e[0] * w),
                n(e[1] * h)
            )
        }
        "angularGradient" => {
            let c = p.center?;
            format!(
                "Brush.sweepGradient({}, center = Offset({}f, {}f))",
                stops(p),
                n(c[0] * w),
                n(c[1] * h)
            )
        }
        "radialGradient" | "diamondGradient" => {
            let (c, r) = (p.center?, p.radius?);
            format!(
                "Brush.radialGradient({}, center = Offset({}f, {}f), radius = {}f)",
                stops(p),
                n(c[0] * w),
                n(c[1] * h),
                n((r[0] * w).max(r[1] * h))
            )
        }
        _ => return None,
    })
}

fn compose(spec: &Spec) -> String {
    let mut out = format!("// {}\n", spec.name.replace('\n', " "));
    let (w, h) = (spec.width, spec.height);
    let shape = match spec.corner_radius {
        Some([tl, tr, br, bl]) => format!(
            "RoundedCornerShape(topStart = {}.dp, topEnd = {}.dp, bottomEnd = {}.dp, bottomStart = {}.dp)",
            n(tl),
            n(tr),
            n(br),
            n(bl)
        ),
        None if spec.kind == "ellipse" => "CircleShape".to_owned(),
        None => "RectangleShape".to_owned(),
    };
    let mut m: Vec<String> = Vec::new();
    let flowed = spec.in_layout.as_ref().is_some_and(|l| !l.absolute);
    if !flowed {
        m.push(format!(
            ".offset(x = {}.dp, y = {}.dp)",
            n(spec.x),
            n(spec.y)
        ));
    }
    m.push(format!(".size(width = {}.dp, height = {}.dp)", n(w), n(h)));
    if spec.rotation != 0.0 {
        m.push(format!(".rotate({}f)", n(spec.rotation)));
    }
    for e in &spec.effects {
        if e.kind == "dropShadow" {
            m.push(format!(
                "// drop shadow: x {}, y {}, blur {}, spread {}, {}",
                n(e.x),
                n(e.y),
                n(e.blur),
                n(e.spread),
                e.color.as_deref().unwrap_or("")
            ));
        }
    }
    if spec.corner_radius.is_some() || spec.kind == "ellipse" || spec.clips_content {
        m.push(format!(".clip({shape})"));
    }
    if spec.text.is_none() {
        for p in &spec.fills {
            if let Some(b) = compose_brush(p, w, h) {
                if p.kind == "solid" {
                    m.push(format!(".background({b})"));
                } else {
                    m.push(format!(".background(brush = {b})"));
                }
            }
        }
    }
    if let (Some(st), Some(p)) = (&spec.stroke, spec.strokes.first()) {
        if let Some(c) = compose_brush(p, w, h) {
            m.push(format!(
                ".border(width = {}.dp, color = {c}, shape = {shape})",
                n(st.weight)
            ));
        }
    }
    for e in &spec.effects {
        if e.kind == "layerBlur" {
            m.push(format!(".blur(radius = {}.dp)", n(e.blur / 2.0)));
        }
    }
    if spec.opacity < 1.0 {
        m.push(format!(".alpha({}f)", n(f64::from(spec.opacity))));
    }
    if let Some(l) = &spec.layout {
        let [t, r, b, lft] = l.padding;
        if [t, r, b, lft].iter().any(|v| *v != 0.0) {
            m.push(format!(
                ".padding(start = {}.dp, top = {}.dp, end = {}.dp, bottom = {}.dp)",
                n(lft),
                n(t),
                n(r),
                n(b)
            ));
        }
    }
    let modifier = format!("Modifier\n        {}", m.join("\n        "));
    if let Some(t) = &spec.text {
        let mut args = vec![
            format!("text = {:?}", t.content),
            format!("modifier = {modifier}"),
        ];
        args.push(format!("fontSize = {}.sp", n(t.font_size)));
        args.push(format!("fontWeight = FontWeight({})", t.font_weight));
        args.push(format!(
            "fontFamily = FontFamily.Default, // {}",
            t.font_family
        ));
        if t.italic {
            args.push("fontStyle = FontStyle.Italic".into());
        }
        if let Some(c) = spec
            .fills
            .first()
            .filter(|p| p.kind == "solid")
            .and_then(|p| compose_brush(p, w, h))
        {
            args.push(format!("color = {c}"));
        }
        if t.letter_spacing != 0.0 {
            args.push(format!("letterSpacing = {}.sp", n(t.letter_spacing)));
        }
        if let Some(lh) = t.line_height {
            args.push(format!("lineHeight = {}.sp", n(lh)));
        }
        match t.align {
            "center" => args.push("textAlign = TextAlign.Center".into()),
            "right" => args.push("textAlign = TextAlign.End".into()),
            "justify" => args.push("textAlign = TextAlign.Justify".into()),
            _ => {}
        }
        match t.decoration {
            Some("underline") => args.push("textDecoration = TextDecoration.Underline".into()),
            Some("line-through") => args.push("textDecoration = TextDecoration.LineThrough".into()),
            _ => {}
        }
        let _ = writeln!(
            out,
            "Text(\n    {}\n)",
            args.join(",\n    ").replace(", //", " //")
        );
        return out;
    }
    if let Some(l) = &spec.layout {
        let row = l.direction == "row";
        let arrangement = match l.primary_align {
            "center" => format!(
                "Arrangement.spacedBy({}.dp, Alignment.CenterHorizontally)",
                n(l.gap)
            ),
            "end" => format!("Arrangement.spacedBy({}.dp, Alignment.End)", n(l.gap)),
            "spaceBetween" => "Arrangement.SpaceBetween".to_owned(),
            _ => format!("Arrangement.spacedBy({}.dp)", n(l.gap)),
        };
        let (container, arr_name, align_name, align) = if row {
            (
                "Row",
                "horizontalArrangement",
                "verticalAlignment",
                match l.counter_align {
                    "center" => "Alignment.CenterVertically",
                    "end" => "Alignment.Bottom",
                    _ => "Alignment.Top",
                },
            )
        } else {
            (
                "Column",
                "verticalArrangement",
                "horizontalAlignment",
                match l.counter_align {
                    "center" => "Alignment.CenterHorizontally",
                    "end" => "Alignment.End",
                    _ => "Alignment.Start",
                },
            )
        };
        let arrangement = if row {
            arrangement
        } else {
            arrangement
                .replace("Alignment.CenterHorizontally", "Alignment.CenterVertically")
                .replace("Alignment.End", "Alignment.Bottom")
        };
        let _ = writeln!(
            out,
            "{container}(\n    modifier = {modifier},\n    {arr_name} = {arrangement},\n    {align_name} = {align},\n) {{\n    // children\n}}"
        );
        return out;
    }
    let _ = writeln!(out, "Box(\n    modifier = {modifier}\n)");
    out
}

#[cfg(test)]
pub(crate) fn css_var_for_tests(name: &str) -> String {
    css_var(name)
}
