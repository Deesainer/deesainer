use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{FillRule, PathCmd, StrokeCap, StrokeJoin, StrokeStyle, VectorPath};
use deesainer_geom::Affine;
use tiny_skia as sk;

pub fn transform(a: &Affine) -> sk::Transform {
    sk::Transform::from_row(
        a.a as f32, a.b as f32, a.c as f32, a.d as f32, a.e as f32, a.f as f32,
    )
}

pub fn color(c: Rgba, opacity: f32) -> sk::Color {
    let a = (c.a * opacity).clamp(0.0, 1.0);
    sk::Color::from_rgba(
        c.r.clamp(0.0, 1.0),
        c.g.clamp(0.0, 1.0),
        c.b.clamp(0.0, 1.0),
        if a.is_finite() { a } else { 0.0 },
    )
    .unwrap_or(sk::Color::TRANSPARENT)
}

pub fn path(p: &VectorPath) -> Option<sk::Path> {
    let mut b = sk::PathBuilder::new();
    let f = |v: f64| v as f32;
    for cmd in &p.cmds {
        match *cmd {
            PathCmd::MoveTo { to } => b.move_to(f(to.x), f(to.y)),
            PathCmd::LineTo { to } => b.line_to(f(to.x), f(to.y)),
            PathCmd::QuadTo { ctrl, to } => b.quad_to(f(ctrl.x), f(ctrl.y), f(to.x), f(to.y)),
            PathCmd::CubicTo { ctrl1, ctrl2, to } => b.cubic_to(
                f(ctrl1.x),
                f(ctrl1.y),
                f(ctrl2.x),
                f(ctrl2.y),
                f(to.x),
                f(to.y),
            ),
            PathCmd::Close => b.close(),
        }
    }
    b.finish()
}

pub fn fill_rule(r: FillRule) -> sk::FillRule {
    match r {
        FillRule::NonZero => sk::FillRule::Winding,
        FillRule::EvenOdd => sk::FillRule::EvenOdd,
    }
}

pub fn blend(b: BlendMode) -> sk::BlendMode {
    match b {
        BlendMode::PassThrough | BlendMode::Normal => sk::BlendMode::SourceOver,
        BlendMode::Darken => sk::BlendMode::Darken,
        BlendMode::Multiply => sk::BlendMode::Multiply,
        BlendMode::ColorBurn => sk::BlendMode::ColorBurn,
        BlendMode::Lighten => sk::BlendMode::Lighten,
        BlendMode::Screen => sk::BlendMode::Screen,
        BlendMode::ColorDodge => sk::BlendMode::ColorDodge,
        BlendMode::Overlay => sk::BlendMode::Overlay,
        BlendMode::SoftLight => sk::BlendMode::SoftLight,
        BlendMode::HardLight => sk::BlendMode::HardLight,
        BlendMode::Difference => sk::BlendMode::Difference,
        BlendMode::Exclusion => sk::BlendMode::Exclusion,
        BlendMode::Hue => sk::BlendMode::Hue,
        BlendMode::Saturation => sk::BlendMode::Saturation,
        BlendMode::Color => sk::BlendMode::Color,
        BlendMode::Luminosity => sk::BlendMode::Luminosity,
    }
}

pub fn stroke(style: &StrokeStyle, width: f64) -> sk::Stroke {
    let dash = if style.dash.len() >= 2 {
        let mut arr: Vec<f32> = style.dash.iter().map(|d| (*d as f32).max(0.0)).collect();
        if arr.len() % 2 == 1 {
            arr.extend(arr.clone());
        }
        sk::StrokeDash::new(arr, 0.0)
    } else {
        None
    };
    sk::Stroke {
        width: (width as f32).max(0.0),
        miter_limit: 4.0,
        line_cap: match style.cap {
            StrokeCap::None => sk::LineCap::Butt,
            StrokeCap::Round => sk::LineCap::Round,
            StrokeCap::Square => sk::LineCap::Square,
        },
        line_join: match style.join {
            StrokeJoin::Miter => sk::LineJoin::Miter,
            StrokeJoin::Round => sk::LineJoin::Round,
            StrokeJoin::Bevel => sk::LineJoin::Bevel,
        },
        dash,
    }
}
