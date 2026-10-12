use deesainer_color::{BlendMode, Rgba};
use deesainer_geom::Point;
use serde::{Deserialize, Serialize};

use crate::ids::AssetId;

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorStop {
    pub position: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageScaleMode {
    #[default]
    Fill,
    Fit,
    Stretch,
    Tile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PaintKind {
    Solid {
        color: Rgba,
    },
    LinearGradient {
        start: Point,
        end: Point,
        stops: Vec<ColorStop>,
    },
    RadialGradient {
        center: Point,
        radius: Point,
        stops: Vec<ColorStop>,
    },
    AngularGradient {
        center: Point,
        #[serde(default)]
        angle: f64,
        stops: Vec<ColorStop>,
    },
    DiamondGradient {
        center: Point,
        radius: Point,
        stops: Vec<ColorStop>,
    },
    Image {
        asset: AssetId,
        #[serde(default)]
        scale_mode: ImageScaleMode,
        #[serde(default, skip_serializing_if = "ImageAdjust::is_identity")]
        adjust: ImageAdjust,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paint {
    #[serde(flatten)]
    pub kind: PaintKind,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub blend: BlendMode,
}

impl Paint {
    pub fn solid(color: Rgba) -> Self {
        Self {
            kind: PaintKind::Solid { color },
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
        }
    }

    pub fn linear(start: Point, end: Point, stops: Vec<ColorStop>) -> Self {
        Self {
            kind: PaintKind::LinearGradient { start, end, stops },
            ..Self::solid(Rgba::BLACK)
        }
    }

    pub fn is_finite(&self) -> bool {
        let stops_ok = |stops: &[ColorStop]| {
            stops
                .iter()
                .all(|s| s.color.is_finite() && s.position.is_finite())
        };
        (0.0..=1.0).contains(&self.opacity)
            && match &self.kind {
                PaintKind::Solid { color } => color.is_finite(),
                PaintKind::LinearGradient { start, end, stops } => {
                    start.is_finite() && end.is_finite() && stops_ok(stops)
                }
                PaintKind::RadialGradient {
                    center,
                    radius,
                    stops,
                }
                | PaintKind::DiamondGradient {
                    center,
                    radius,
                    stops,
                } => center.is_finite() && radius.is_finite() && stops_ok(stops),
                PaintKind::AngularGradient {
                    center,
                    angle,
                    stops,
                } => center.is_finite() && angle.is_finite() && stops_ok(stops),
                PaintKind::Image { adjust, .. } => adjust.is_valid(),
            }
    }

    pub fn primary_color(&self) -> Option<Rgba> {
        match &self.kind {
            PaintKind::Solid { color } => Some(*color),
            PaintKind::LinearGradient { stops, .. }
            | PaintKind::RadialGradient { stops, .. }
            | PaintKind::AngularGradient { stops, .. }
            | PaintKind::DiamondGradient { stops, .. } => stops.first().map(|s| s.color),
            PaintKind::Image { .. } => None,
        }
    }

    pub fn stops(&self) -> Option<&[ColorStop]> {
        match &self.kind {
            PaintKind::LinearGradient { stops, .. }
            | PaintKind::RadialGradient { stops, .. }
            | PaintKind::AngularGradient { stops, .. }
            | PaintKind::DiamondGradient { stops, .. } => Some(stops),
            PaintKind::Solid { .. } | PaintKind::Image { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrokeAlign {
    #[default]
    Inside,
    Center,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrokeCap {
    #[default]
    None,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrokeJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StrokeStyle {
    pub weight: f64,
    pub align: StrokeAlign,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
    pub dash: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sides: Option<[f64; 4]>,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            weight: 1.0,
            align: StrokeAlign::Inside,
            cap: StrokeCap::None,
            join: StrokeJoin::Miter,
            dash: Vec::new(),
            sides: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Effect {
    DropShadow {
        color: Rgba,
        offset: Point,
        blur: f64,
        #[serde(default)]
        spread: f64,
        #[serde(default = "yes")]
        visible: bool,
    },
    InnerShadow {
        color: Rgba,
        offset: Point,
        blur: f64,
        #[serde(default)]
        spread: f64,
        #[serde(default = "yes")]
        visible: bool,
    },
    LayerBlur {
        radius: f64,
        #[serde(default = "yes")]
        visible: bool,
    },
    BackgroundBlur {
        radius: f64,
        #[serde(default = "yes")]
        visible: bool,
    },
}

impl Effect {
    pub fn is_visible(&self) -> bool {
        match self {
            Effect::DropShadow { visible, .. }
            | Effect::InnerShadow { visible, .. }
            | Effect::LayerBlur { visible, .. }
            | Effect::BackgroundBlur { visible, .. } => *visible,
        }
    }

    pub fn outset(&self) -> f64 {
        if !self.is_visible() {
            return 0.0;
        }
        match self {
            Effect::DropShadow {
                offset,
                blur,
                spread,
                ..
            } => blur * 2.0 + spread.max(0.0) + offset.x.abs().max(offset.y.abs()),
            Effect::LayerBlur { radius, .. } => radius * 2.0,
            Effect::InnerShadow { .. } | Effect::BackgroundBlur { .. } => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageAdjust {
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
    pub highlights: f32,
    pub shadows: f32,
}

impl ImageAdjust {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    pub fn is_valid(&self) -> bool {
        self.values()
            .iter()
            .all(|v| v.is_finite() && (-1.0..=1.0).contains(v))
    }

    pub fn values(&self) -> [f32; 7] {
        [
            self.exposure,
            self.contrast,
            self.saturation,
            self.temperature,
            self.tint,
            self.highlights,
            self.shadows,
        ]
    }

    pub fn key(&self) -> u64 {
        if self.is_identity() {
            return 0;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in self.values() {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h.max(1)
    }

    pub fn apply(&self, [r, g, b]: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return [r, g, b];
        }
        let smooth = |e0: f32, e1: f32, x: f32| {
            let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let gain = 2f32.powf(self.exposure * 2.0);
        let mut c = [r * gain, g * gain, b * gain];
        for v in &mut c {
            *v = (*v - 0.5) * (1.0 + self.contrast) + 0.5;
        }
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for v in &mut c {
            *v = luma + (*v - luma) * (1.0 + self.saturation);
        }
        c[0] += self.temperature * 0.1;
        c[2] -= self.temperature * 0.1;
        c[1] -= self.tint * 0.1;
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let lift = self.highlights * 0.25 * smooth(0.5, 1.0, luma)
            + self.shadows * 0.25 * (1.0 - smooth(0.0, 0.5, luma));
        c.map(|v| (v + lift).clamp(0.0, 1.0))
    }
}

pub fn mirror_paints(paints: &[Paint], horizontal: bool) -> Vec<Paint> {
    let m = |q: Point| {
        if horizontal {
            Point::new(1.0 - q.x, q.y)
        } else {
            Point::new(q.x, 1.0 - q.y)
        }
    };
    paints
        .iter()
        .map(|p| {
            let mut p = p.clone();
            match &mut p.kind {
                PaintKind::LinearGradient { start, end, .. } => {
                    *start = m(*start);
                    *end = m(*end);
                }
                PaintKind::RadialGradient { center, .. }
                | PaintKind::DiamondGradient { center, .. } => *center = m(*center),
                PaintKind::AngularGradient {
                    center,
                    angle,
                    stops,
                } => {
                    *center = m(*center);
                    *angle = if horizontal { 180.0 - *angle } else { -*angle };
                    stops.reverse();
                    for st in stops.iter_mut() {
                        st.position = 1.0 - st.position;
                    }
                }
                PaintKind::Solid { .. } | PaintKind::Image { .. } => {}
            }
            p
        })
        .collect()
}

#[cfg(test)]
mod adjust_tests {
    use super::ImageAdjust;

    #[test]
    fn adjustments_change_colour_as_named() {
        let none = ImageAdjust::default();
        assert_eq!(none.apply([0.2, 0.4, 0.6]), [0.2, 0.4, 0.6]);
        assert_eq!(none.key(), 0);
        let brighter = ImageAdjust {
            exposure: 0.5,
            ..none
        };
        assert!(brighter.apply([0.2, 0.2, 0.2])[0] > 0.3);
        let grey = ImageAdjust {
            saturation: -1.0,
            ..none
        }
        .apply([0.9, 0.1, 0.1]);
        assert!((grey[0] - grey[1]).abs() < 1e-6 && (grey[1] - grey[2]).abs() < 1e-6);
        let warm = ImageAdjust {
            temperature: 1.0,
            ..none
        }
        .apply([0.5, 0.5, 0.5]);
        assert!(warm[0] > 0.5 && warm[2] < 0.5);
        assert!(!ImageAdjust {
            contrast: 2.0,
            ..none
        }
        .is_valid());
        assert_ne!(
            brighter.key(),
            ImageAdjust {
                exposure: 0.25,
                ..none
            }
            .key()
        );
    }
}
