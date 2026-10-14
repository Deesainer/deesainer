use serde::{Deserialize, Serialize};

pub mod display_p3;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ColorError {
    #[error("invalid hex color {0:?}: expected #RRGGBB or #RRGGBBAA")]
    InvalidHex(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::new(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Rgba = Rgba::new(1.0, 1.0, 1.0, 1.0);
    pub const DEFAULT_FILL: Rgba = Rgba::new(0.85, 0.85, 0.85, 1.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(input: &str) -> Result<Self, ColorError> {
        let err = || ColorError::InvalidHex(input.to_owned());
        let hex = input.strip_prefix('#').unwrap_or(input);
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err());
        }
        let channel = |i: usize| -> Result<f32, ColorError> {
            let pair = hex.get(i..i + 2).ok_or_else(err)?;
            let v = u8::from_str_radix(pair, 16).map_err(|_| err())?;
            Ok(f32::from(v) / 255.0)
        };
        let a = if hex.len() == 8 { channel(6)? } else { 1.0 };
        Ok(Self::new(channel(0)?, channel(2)?, channel(4)?, a))
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub fn is_finite(&self) -> bool {
        self.r.is_finite() && self.g.is_finite() && self.b.is_finite() && self.a.is_finite()
    }

    pub fn lerp(self, o: Rgba, t: f32) -> Rgba {
        Rgba::new(
            self.r + (o.r - self.r) * t,
            self.g + (o.g - self.g) * t,
            self.b + (o.b - self.b) * t,
            self.a + (o.a - self.a) * t,
        )
    }

    pub fn to_rgba8(&self) -> [u8; 4] {
        let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    pub fn from_rgba8([r, g, b, a]: [u8; 4]) -> Self {
        let f = |c: u8| f32::from(c) / 255.0;
        Rgba::new(f(r), f(g), f(b), f(a))
    }

    pub fn to_hex(&self) -> String {
        let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        if q(self.a) == 255 {
            format!("#{:02X}{:02X}{:02X}", q(self.r), q(self.g), q(self.b))
        } else {
            format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                q(self.r),
                q(self.g),
                q(self.b),
                q(self.a)
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlendMode {
    #[default]
    PassThrough,
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    Lighten,
    Screen,
    ColorDodge,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        for s in ["#000000", "#FFFFFF", "#1E90FF", "#11223344"] {
            assert_eq!(Rgba::from_hex(s).unwrap().to_hex(), s);
        }
    }

    #[test]
    fn hex_rejects_garbage() {
        for s in ["", "#", "#12345", "#GGGGGG", "#1234567", "#ééé"] {
            assert!(Rgba::from_hex(s).is_err(), "{s} should be rejected");
        }
    }
}
