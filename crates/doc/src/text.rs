use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "camelCase")]
pub enum LineHeight {
    #[default]
    Auto,
    Pixels(f64),
    Percent(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignH {
    #[default]
    Left,
    Center,
    Right,
    Justified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignV {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAutoResize {
    #[default]
    WidthAndHeight,
    Height,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextCase {
    #[default]
    Original,
    Upper,
    Lower,
    Title,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextStyle {
    pub font_family: String,
    pub font_size: f64,
    pub font_weight: u16,
    pub italic: bool,
    pub line_height: LineHeight,
    pub letter_spacing: f64,
    pub decoration: TextDecoration,
    pub case: TextCase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<crate::Paint>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<FontFeature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variations: Vec<FontVariation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontVariation {
    pub tag: String,
    pub value: f32,
}

impl FontVariation {
    pub fn is_valid(&self) -> bool {
        self.tag.len() == 4
            && self.tag.bytes().all(|b| b.is_ascii_alphanumeric())
            && self.value.is_finite()
            && self.value.abs() <= 100_000.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontFeature {
    pub tag: String,
    pub value: u32,
}

impl FontFeature {
    pub fn is_valid(&self) -> bool {
        self.tag.len() == 4
            && self.tag.bytes().all(|b| b.is_ascii_alphanumeric())
            && self.value <= 99
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: "Inter".to_owned(),
            font_size: 16.0,
            font_weight: 400,
            italic: false,
            line_height: LineHeight::Auto,
            letter_spacing: 0.0,
            decoration: TextDecoration::None,
            case: TextCase::Original,
            fill: None,
            features: Vec::new(),
            variations: Vec::new(),
        }
    }
}

impl TextStyle {
    pub fn transform_case(&self, s: &str) -> String {
        match self.case {
            TextCase::Original => s.to_owned(),
            TextCase::Upper => s.to_uppercase(),
            TextCase::Lower => s.to_lowercase(),
            TextCase::Title => {
                let mut out = String::with_capacity(s.len());
                let mut start = true;
                for ch in s.chars() {
                    if start && ch.is_alphabetic() {
                        out.extend(ch.to_uppercase());
                        start = false;
                    } else {
                        out.push(ch);
                        start = ch.is_whitespace();
                    }
                }
                out
            }
        }
    }

    pub fn line_height_px(&self) -> f64 {
        match self.line_height {
            LineHeight::Auto => self.font_size * 1.21,
            LineHeight::Pixels(px) => px,
            LineHeight::Percent(p) => self.font_size * p / 100.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextData {
    pub content: String,
    pub style: TextStyle,
    #[serde(default)]
    pub runs: Vec<TextRun>,
    pub auto_resize: TextAutoResize,
    pub align_h: TextAlignH,
    pub align_v: TextAlignV,
    #[serde(skip_serializing_if = "is_zero")]
    pub paragraph_spacing: f64,
    #[serde(skip_serializing_if = "TextList::is_none")]
    pub list: TextList,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paragraphs: Vec<ParagraphFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ParagraphFormat {
    pub list: TextList,
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub level: u8,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextList {
    #[default]
    None,
    Bullet,
    Number,
}

impl TextList {
    pub fn is_none(&self) -> bool {
        *self == TextList::None
    }
}

impl Default for TextData {
    fn default() -> Self {
        Self {
            content: String::new(),
            style: TextStyle::default(),
            runs: Vec::new(),
            auto_resize: TextAutoResize::WidthAndHeight,
            align_h: TextAlignH::Left,
            align_v: TextAlignV::Top,
            paragraph_spacing: 0.0,
            list: TextList::None,
            paragraphs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRun {
    pub start: usize,
    pub end: usize,
    pub style: TextStyle,
}

impl TextData {
    pub const MAX_RUNS: usize = 4096;
    pub const MAX_LIST_LEVEL: u8 = 8;

    pub fn paragraph_count(&self) -> usize {
        self.content.bytes().filter(|b| *b == b'\n').count() + 1
    }

    pub fn paragraph_format(&self, i: usize) -> ParagraphFormat {
        if self.paragraphs.is_empty() {
            return ParagraphFormat {
                list: self.list,
                level: 0,
            };
        }
        self.paragraphs.get(i).copied().unwrap_or_default()
    }

    pub fn set_paragraph_format(
        &mut self,
        first: usize,
        last: usize,
        change: impl Fn(&mut ParagraphFormat),
    ) -> Result<(), String> {
        let count = self.paragraph_count();
        if first > last || last >= count {
            return Err(format!("paragraphs {first}..={last} out of 0..{count}"));
        }
        let mut formats: Vec<ParagraphFormat> =
            (0..count).map(|i| self.paragraph_format(i)).collect();
        for f in &mut formats[first..=last] {
            change(f);
            f.level = f.level.min(Self::MAX_LIST_LEVEL);
        }
        match formats.first().copied() {
            Some(f) if f.level == 0 && formats.iter().all(|g| *g == f) => {
                self.list = f.list;
                self.paragraphs.clear();
            }
            _ => {
                self.list = TextList::None;
                self.paragraphs = formats;
            }
        }
        Ok(())
    }

    fn remap_paragraphs(
        &mut self,
        old: &str,
        new: &str,
        prefix: usize,
        old_end: usize,
        new_end: usize,
    ) {
        if self.paragraphs.is_empty() {
            return;
        }
        let lines = |s: &str| s.bytes().filter(|b| *b == b'\n').count();
        let p0 = lines(&old[..prefix]);
        let removed = lines(&old[prefix..old_end]);
        let added = lines(&new[prefix..new_end]);
        let old_count = lines(old) + 1;
        let at = |i: usize| self.paragraphs.get(i).copied().unwrap_or_default();
        let at_start = prefix == 0 || old.as_bytes().get(prefix - 1) == Some(&b'\n');
        let next: Vec<ParagraphFormat> = if at_start {
            let above = at(p0.saturating_sub(1));
            (0..p0)
                .map(at)
                .chain(std::iter::repeat_n(above, added))
                .chain((p0 + removed..old_count).map(at))
                .collect()
        } else {
            (0..=p0)
                .map(at)
                .chain(std::iter::repeat_n(at(p0), added))
                .chain((p0 + 1 + removed..old_count).map(at))
                .collect()
        };
        self.paragraphs = next;
    }

    fn canonical_runs(mut runs: Vec<TextRun>, base: &TextStyle) -> Vec<TextRun> {
        runs.sort_by_key(|run| run.start);
        let mut merged: Vec<TextRun> = Vec::new();
        for run in runs {
            if run.style == *base {
                continue;
            }
            if let Some(last) = merged.last_mut() {
                if last.end == run.start && last.style == run.style {
                    last.end = run.end;
                    continue;
                }
            }
            merged.push(run);
        }
        merged
    }

    pub fn validate_runs(&self) -> Result<(), String> {
        if self.content.len() > 1_000_000 || self.runs.len() > Self::MAX_RUNS {
            return Err("text content or style run limit exceeded".into());
        }
        if self.paragraphs.len() > self.content.len() + 1
            || self
                .paragraphs
                .iter()
                .any(|p| p.level > Self::MAX_LIST_LEVEL)
        {
            return Err("invalid paragraph list formats".into());
        }
        let mut end = 0;
        for run in &self.runs {
            if run.start < end
                || run.start >= run.end
                || run.end > self.content.len()
                || !self.content.is_char_boundary(run.start)
                || !self.content.is_char_boundary(run.end)
            {
                return Err("invalid or overlapping text style range".into());
            }
            let style = &run.style;
            if !style.font_size.is_finite()
                || !(0.5..=10_000.0).contains(&style.font_size)
                || style.font_family.len() > 256
                || !(1..=1000).contains(&style.font_weight)
                || !style.letter_spacing.is_finite()
                || style.letter_spacing.abs() > 10_000.0
                || !style.line_height_px().is_finite()
                || !(0.1..=100_000.0).contains(&style.line_height_px())
                || style.case != self.style.case
                || style.fill.as_ref().is_some_and(|p| !p.is_finite())
                || style.variations.len() > 32
                || style.variations.iter().any(|v| !v.is_valid())
            {
                return Err("invalid text range typography".into());
            }
            end = run.end;
        }
        Ok(())
    }

    pub fn set_range_style(
        &mut self,
        start: usize,
        end: usize,
        style: TextStyle,
    ) -> Result<(), String> {
        if start >= end
            || end > self.content.len()
            || !self.content.is_char_boundary(start)
            || !self.content.is_char_boundary(end)
        {
            return Err("range must be non-empty and at UTF-8 boundaries".into());
        }
        self.validate_runs()?;
        let mut next = Vec::with_capacity(self.runs.len().saturating_add(3));
        for run in &self.runs {
            if run.end <= start || run.start >= end {
                next.push(run.clone());
            } else {
                if run.start < start {
                    next.push(TextRun {
                        start: run.start,
                        end: start,
                        style: run.style.clone(),
                    });
                }
                if run.end > end {
                    next.push(TextRun {
                        start: end,
                        end: run.end,
                        style: run.style.clone(),
                    });
                }
            }
        }
        next.push(TextRun { start, end, style });
        let merged = Self::canonical_runs(next, &self.style);
        if merged.len() > Self::MAX_RUNS {
            return Err("too many text style ranges".into());
        }
        let previous = std::mem::replace(&mut self.runs, merged);
        if let Err(e) = self.validate_runs() {
            self.runs = previous;
            return Err(e);
        }
        Ok(())
    }

    pub fn modify_range_styles(
        &mut self,
        start: usize,
        end: usize,
        mut change: impl FnMut(&mut TextStyle),
    ) -> Result<(), String> {
        if start >= end
            || end > self.content.len()
            || !self.content.is_char_boundary(start)
            || !self.content.is_char_boundary(end)
        {
            return Err("range must be non-empty and at UTF-8 boundaries".into());
        }
        self.validate_runs()?;
        let mut segments = Vec::with_capacity(self.runs.len().saturating_mul(2).saturating_add(1));
        let mut cursor = 0;
        for run in &self.runs {
            if cursor < run.start {
                segments.push(TextRun {
                    start: cursor,
                    end: run.start,
                    style: self.style.clone(),
                });
            }
            segments.push(run.clone());
            cursor = run.end;
        }
        if cursor < self.content.len() {
            segments.push(TextRun {
                start: cursor,
                end: self.content.len(),
                style: self.style.clone(),
            });
        }
        let mut next = Vec::with_capacity(segments.len().saturating_add(2));
        for segment in segments {
            if segment.end <= start || segment.start >= end {
                next.push(segment);
                continue;
            }
            if segment.start < start {
                next.push(TextRun {
                    start: segment.start,
                    end: start,
                    style: segment.style.clone(),
                });
            }
            let middle_start = segment.start.max(start);
            let middle_end = segment.end.min(end);
            let mut middle_style = segment.style.clone();
            change(&mut middle_style);
            next.push(TextRun {
                start: middle_start,
                end: middle_end,
                style: middle_style,
            });
            if segment.end > end {
                next.push(TextRun {
                    start: end,
                    end: segment.end,
                    style: segment.style,
                });
            }
        }
        let merged = Self::canonical_runs(next, &self.style);
        if merged.len() > Self::MAX_RUNS {
            return Err("too many text style ranges".into());
        }
        let previous = std::mem::replace(&mut self.runs, merged);
        if let Err(message) = self.validate_runs() {
            self.runs = previous;
            return Err(message);
        }
        Ok(())
    }

    pub fn replace_content(&mut self, new_content: String) -> Result<(), String> {
        if new_content.len() > 1_000_000 {
            return Err("text is longer than 1 MB".into());
        }
        self.validate_runs()?;
        let old = self.content.as_bytes();
        let new = new_content.as_bytes();
        let mut prefix = 0;
        while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
            prefix += 1;
        }
        while !self.content.is_char_boundary(prefix) || !new_content.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let mut suffix = 0;
        while suffix < old.len() - prefix
            && suffix < new.len() - prefix
            && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
        {
            suffix += 1;
        }
        while !self.content.is_char_boundary(old.len() - suffix)
            || !new_content.is_char_boundary(new.len() - suffix)
        {
            suffix -= 1;
        }
        let old_end = old.len() - suffix;
        let new_end = new.len() - suffix;
        let inherited = self
            .runs
            .iter()
            .find(|r| r.start < prefix && prefix <= r.end || r.start <= prefix && prefix < r.end)
            .map(|r| r.style.clone());
        let mut runs = Vec::new();
        for run in &self.runs {
            if run.start < prefix {
                let end = run.end.min(prefix);
                if run.start < end {
                    runs.push(TextRun {
                        start: run.start,
                        end,
                        style: run.style.clone(),
                    });
                }
            }
            if run.end > old_end {
                let start = new_end + run.start.max(old_end) - old_end;
                let end = new_end + run.end - old_end;
                if start < end {
                    runs.push(TextRun {
                        start,
                        end,
                        style: run.style.clone(),
                    });
                }
            }
        }
        if let Some(style) = inherited.filter(|_| prefix < new_end) {
            runs.push(TextRun {
                start: prefix,
                end: new_end,
                style,
            });
        }
        let runs = Self::canonical_runs(runs, &self.style);
        let old_paragraphs = self.paragraphs.clone();
        let old_content = self.content.clone();
        self.remap_paragraphs(&old_content, &new_content, prefix, old_end, new_end);
        self.content = new_content;
        let old_runs = std::mem::replace(&mut self.runs, runs);
        if let Err(e) = self.validate_runs() {
            self.content = old_content;
            self.runs = old_runs;
            self.paragraphs = old_paragraphs;
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(content: &str) -> TextData {
        let mut d = TextData {
            content: content.to_owned(),
            ..TextData::default()
        };
        d.set_paragraph_format(1, 1, |f| {
            f.list = TextList::Bullet;
            f.level = 1;
        })
        .unwrap();
        d
    }

    #[test]
    fn paragraph_formats_follow_edits() {
        let mut d = listed("Title\nItem\nEnd");
        assert_eq!(d.paragraphs.len(), 3);
        let bullet = ParagraphFormat {
            list: TextList::Bullet,
            level: 1,
        };
        d.replace_content("Title\nIt\nem\nEnd".into()).unwrap();
        assert_eq!(d.paragraph_format(1), bullet);
        assert_eq!(d.paragraph_format(2), bullet);
        assert_eq!(d.paragraph_format(3), ParagraphFormat::default());
        d.replace_content("Title\nItem\nEnd".into()).unwrap();
        assert_eq!(d.paragraphs.len(), 3);
        assert_eq!(d.paragraph_format(1), bullet);
        assert_eq!(d.paragraph_format(2), ParagraphFormat::default());
        d.replace_content("Title\nItem\n\nEnd".into()).unwrap();
        assert_eq!(d.paragraph_format(2), bullet);
        assert_eq!(d.paragraph_format(3), ParagraphFormat::default());
        d.replace_content("Title\nItem\nEnd".into()).unwrap();
        d.replace_content("Title\nEnd".into()).unwrap();
        assert_eq!(d.paragraph_format(1), ParagraphFormat::default());
        d.replace_content("Title\nItem\nEnd".into()).unwrap();
        d.set_paragraph_format(1, 1, |f| *f = bullet).unwrap();
        d.replace_content("TitleItem\nEnd".into()).unwrap();
        assert_eq!(d.paragraph_format(0), ParagraphFormat::default());
        assert_eq!(d.paragraphs.len(), 2);
    }

    #[test]
    fn alike_paragraphs_fold_back_into_the_layer_list() {
        let mut d = listed("a\nb");
        d.set_paragraph_format(0, 1, |f| {
            f.list = TextList::Number;
            f.level = 0;
        })
        .unwrap();
        assert!(d.paragraphs.is_empty());
        assert_eq!(d.list, TextList::Number);
        assert!(d.set_paragraph_format(0, 5, |_| {}).is_err());
        d.set_paragraph_format(0, 0, |f| f.level = 200).unwrap();
        assert_eq!(d.paragraph_format(0).level, TextData::MAX_LIST_LEVEL);
    }

    #[test]
    fn ranges_follow_unicode_edits_and_reject_invalid_offsets() {
        let mut text = TextData {
            content: "aé🙂z".into(),
            ..TextData::default()
        };
        let mut bold = text.style.clone();
        bold.font_weight = 700;
        assert!(text.set_range_style(1, 2, bold.clone()).is_err());
        text.set_range_style(1, 7, bold.clone()).unwrap();
        assert_eq!((text.runs[0].start, text.runs[0].end), (1, 7));
        text.replace_content("aé!🙂z".into()).unwrap();
        assert_eq!(
            text.runs,
            vec![TextRun {
                start: 1,
                end: 8,
                style: bold
            }]
        );
        text.replace_content("az".into()).unwrap();
        assert!(text.runs.is_empty());
        assert!(text.validate_runs().is_ok());
    }

    #[test]
    fn overlapping_and_excessive_ranges_are_rejected() {
        let mut text = TextData {
            content: "abc".into(),
            ..TextData::default()
        };
        text.runs = vec![
            TextRun {
                start: 0,
                end: 2,
                style: text.style.clone(),
            },
            TextRun {
                start: 1,
                end: 3,
                style: text.style.clone(),
            },
        ];
        assert!(text.validate_runs().is_err());
        text.runs.clear();
        text.runs = vec![
            TextRun {
                start: 0,
                end: 1,
                style: text.style.clone()
            };
            TextData::MAX_RUNS + 1
        ];
        assert!(text.validate_runs().is_err());
    }

    #[test]
    fn range_mutation_preserves_other_typography_and_utf8_boundaries() {
        let mut text = TextData {
            content: "aé🙂z".into(),
            ..TextData::default()
        };
        let mut serif = text.style.clone();
        serif.font_family = "serif".into();
        text.set_range_style(1, 7, serif).unwrap();
        text.modify_range_styles(0, 8, |style| style.font_size = 28.0)
            .unwrap();
        assert_eq!(text.runs.len(), 3);
        assert_eq!(text.runs[0].style.font_family, "Inter");
        assert_eq!(text.runs[1].style.font_family, "serif");
        assert_eq!(text.runs[2].style.font_family, "Inter");
        assert!(text.runs.iter().all(|r| r.style.font_size == 28.0));
        let before = text.runs.clone();
        assert!(text
            .modify_range_styles(2, 7, |style| style.font_size = 30.0)
            .is_err());
        assert_eq!(text.runs, before);
    }
}
