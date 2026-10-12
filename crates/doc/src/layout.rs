use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Constraint {
    #[default]
    Min,
    Max,
    Center,
    Stretch,
    Scale,
}

impl Constraint {
    pub fn solve(self, pos: f64, size: f64, old: f64, new: f64) -> (f64, f64) {
        match self {
            Constraint::Min => (pos, size),
            Constraint::Max => {
                let right = old - (pos + size);
                (new - right - size, size)
            }
            Constraint::Center => {
                let offset = pos + size / 2.0 - old / 2.0;
                (new / 2.0 + offset - size / 2.0, size)
            }
            Constraint::Stretch => {
                let right = old - (pos + size);
                (pos, (new - pos - right).max(0.0))
            }
            Constraint::Scale => {
                if old.abs() < 1e-9 {
                    (pos, size)
                } else {
                    let k = new / old;
                    (pos * k, size * k)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Constraints {
    pub horizontal: Constraint,
    pub vertical: Constraint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayoutDirection {
    #[default]
    Horizontal,
    Vertical,
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrimaryAlign {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CounterAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sizing {
    #[default]
    Fixed,
    Hug,
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Padding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Padding {
    pub const fn uniform(p: f64) -> Self {
        Self {
            top: p,
            right: p,
            bottom: p,
            left: p,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoLayout {
    pub direction: LayoutDirection,
    pub wrap: bool,
    pub gap: f64,
    pub counter_gap: f64,
    pub padding: Padding,
    pub primary_align: PrimaryAlign,
    pub counter_align: CounterAlign,
    pub primary_sizing: Sizing,
    pub counter_sizing: Sizing,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub grid_columns: u32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub grid_rows: u32,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

impl AutoLayout {
    pub fn columns(&self) -> usize {
        if self.grid_columns == 0 {
            2
        } else {
            self.grid_columns.min(1000) as usize
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayoutChild {
    pub horizontal: Sizing,
    pub vertical: Sizing,
    pub absolute: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridPattern {
    #[default]
    Grid,
    Columns,
    Rows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridAlign {
    #[default]
    Stretch,
    Min,
    Center,
    Max,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayoutGrid {
    pub pattern: GridPattern,
    pub size: f64,
    pub count: u32,
    pub gutter: f64,
    pub offset: f64,
    pub align: GridAlign,
    pub color: deesainer_color::Rgba,
    pub visible: bool,
}

impl Default for LayoutGrid {
    fn default() -> Self {
        Self {
            pattern: GridPattern::Grid,
            size: 10.0,
            count: 5,
            gutter: 20.0,
            offset: 0.0,
            align: GridAlign::Stretch,
            color: deesainer_color::Rgba::new(1.0, 0.0, 0.0, 0.1),
            visible: true,
        }
    }
}

impl LayoutGrid {
    pub fn bands(&self, len: f64) -> Vec<(f64, f64)> {
        let n = self.count.clamp(1, 1000) as f64;
        let gutter = self.gutter.max(0.0);
        let (start, width) = match self.align {
            GridAlign::Stretch => {
                let w = ((len - 2.0 * self.offset - gutter * (n - 1.0)) / n).max(0.0);
                (self.offset, w)
            }
            GridAlign::Min => (self.offset, self.size.max(0.0)),
            GridAlign::Max => {
                let w = self.size.max(0.0);
                (len - self.offset - (w * n + gutter * (n - 1.0)), w)
            }
            GridAlign::Center => {
                let w = self.size.max(0.0);
                ((len - (w * n + gutter * (n - 1.0))) / 2.0 + self.offset, w)
            }
        };
        (0..self.count.clamp(1, 1000))
            .map(|i| (start + f64::from(i) * (width + gutter), width))
            .collect()
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    #[test]
    fn stretch_columns_fill_the_width() {
        let g = LayoutGrid {
            pattern: GridPattern::Columns,
            count: 4,
            gutter: 20.0,
            offset: 10.0,
            ..LayoutGrid::default()
        };
        let b = g.bands(400.0);
        assert_eq!(b.len(), 4);
        assert!((b[0].0 - 10.0).abs() < 1e-9);
        let end = b[3].0 + b[3].1;
        assert!((end - 390.0).abs() < 1e-9);
    }

    #[test]
    fn centered_columns_are_centered() {
        let g = LayoutGrid {
            pattern: GridPattern::Columns,
            count: 2,
            size: 100.0,
            gutter: 0.0,
            align: GridAlign::Center,
            ..LayoutGrid::default()
        };
        assert_eq!(g.bands(400.0), vec![(100.0, 100.0), (200.0, 100.0)]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulerGuide {
    pub vertical: bool,
    pub at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Png,
    Jpg,
    Webp,
    Svg,
    Pdf,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpg => "jpg",
            ExportFormat::Webp => "webp",
            ExportFormat::Svg => "svg",
            ExportFormat::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSetting {
    pub format: ExportFormat,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SizeLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_height: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_height: Option<f64>,
}

fn clamp_to(v: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let v = max.map_or(v, |m| v.min(m));
    min.map_or(v, |m| v.max(m))
}

impl SizeLimits {
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }

    pub fn width(&self, v: f64) -> f64 {
        clamp_to(v, self.min_width, self.max_width)
    }

    pub fn height(&self, v: f64) -> f64 {
        clamp_to(v, self.min_height, self.max_height)
    }
}
