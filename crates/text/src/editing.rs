use cosmic_text::{Action, Buffer, Cursor, Edit, Editor, Motion, Selection};
use deesainer_doc::{
    Paint, TextAlignV, TextAutoResize, TextCase, TextData, TextDecoration, TextStyle,
};
use deesainer_geom::{Point, Rect};

use super::{fonts, paragraphs, shaped_buffer};

const MAX_CONTENT: usize = 1_000_000;
const MAX_HISTORY: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMotion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Start,
    Finish,
    PreviousWord,
    NextWord,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditAction {
    Insert(String),
    Backspace,
    Delete,
    SelectAll,
    Move(TextMotion, bool),
    ToggleBold,
    ToggleItalic,
    Undo,
    Redo,
}

#[derive(Default, Debug)]
pub struct EditGeometry {
    pub caret: Option<Rect>,
    pub selection: Vec<Rect>,
}

pub struct TextEditor {
    editor: Editor<'static>,
    display: Buffer,
    display_size: (f64, f64),
    data: TextData,
    width: Option<f64>,
    undo: Vec<EditCheckpoint>,
    redo: Vec<EditCheckpoint>,
}

struct EditCheckpoint {
    editor: Editor<'static>,
    data: TextData,
}

fn display_size(buffer: &Buffer, data: &TextData, width: Option<f64>) -> (f64, f64) {
    let mut max_width: f64 = 0.0;
    let mut bottom: f64 = 0.0;
    let p = paragraphs(data);
    for run in buffer.layout_runs() {
        max_width = max_width.max(f64::from(run.line_w) + p.indent_of(run.line_i));
        bottom =
            bottom.max(f64::from(run.line_top + run.line_height) + run.line_i as f64 * p.spacing);
    }
    if bottom == 0.0 {
        bottom = data.style.line_height_px();
    }
    (width.unwrap_or(max_width.ceil()), bottom.ceil())
}

impl TextEditor {
    pub fn new(data: TextData, width: Option<f64>) -> Result<Self, String> {
        data.validate_runs()?;
        let mut f = fonts().lock().map_err(|_| "font system unavailable")?;
        if !f.usable {
            return Err("no fonts are installed here, so text cannot be edited".into());
        }
        let buffer = shaped_buffer(&mut f.system, &data, width, &data.content);
        let display = shaped_buffer(
            &mut f.system,
            &data,
            width,
            &data.style.transform_case(&data.content),
        );
        let display_size = display_size(&display, &data, width);
        let mut editor = Editor::new(buffer);
        editor.action(&mut f.system, Action::Motion(Motion::BufferEnd));
        Ok(Self {
            editor,
            display,
            display_size,
            data,
            width,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn data(&self) -> &TextData {
        &self.data
    }

    pub fn visual_size(&self) -> (f64, f64) {
        self.display_size
    }

    fn sync_content(&mut self) -> Result<(), String> {
        let content = self.editor.with_buffer(|buffer| {
            buffer
                .lines
                .iter()
                .map(|line| line.text())
                .collect::<Vec<_>>()
                .join("\n")
        });
        if content != self.data.content {
            self.data.replace_content(content)?;
        }
        Ok(())
    }

    fn refresh_display(&mut self) -> Result<(), String> {
        let mut f = fonts().lock().map_err(|_| "font system unavailable")?;
        self.display = shaped_buffer(
            &mut f.system,
            &self.data,
            self.width,
            &self.data.style.transform_case(&self.data.content),
        );
        self.display_size = display_size(&self.display, &self.data, self.width);
        Ok(())
    }

    pub fn action(&mut self, action: EditAction) -> Result<(), String> {
        if let EditAction::Undo | EditAction::Redo = action {
            let (from, to) = if matches!(action, EditAction::Undo) {
                (&mut self.undo, &mut self.redo)
            } else {
                (&mut self.redo, &mut self.undo)
            };
            if let Some(previous) = from.pop() {
                to.push(EditCheckpoint {
                    editor: std::mem::replace(&mut self.editor, previous.editor),
                    data: std::mem::replace(&mut self.data, previous.data),
                });
                self.refresh_display()?;
            }
            return Ok(());
        }
        if let EditAction::Insert(text) = &action {
            if self.data.content.len().saturating_add(text.len()) > MAX_CONTENT {
                return Err("text exceeds the 1 MB editing limit".into());
            }
        }
        let mut f = fonts().lock().map_err(|_| "font system unavailable")?;
        let changes = matches!(
            action,
            EditAction::Insert(_)
                | EditAction::Backspace
                | EditAction::Delete
                | EditAction::ToggleBold
                | EditAction::ToggleItalic
        );
        let style_change = matches!(action, EditAction::ToggleBold | EditAction::ToggleItalic);
        let bold = matches!(action, EditAction::ToggleBold);
        let previous = changes.then(|| EditCheckpoint {
            editor: self.editor.clone(),
            data: self.data.clone(),
        });
        match action {
            EditAction::Insert(text) => self.editor.insert_string(&text, None),
            EditAction::Backspace => self.editor.action(&mut f.system, Action::Backspace),
            EditAction::Delete => self.editor.action(&mut f.system, Action::Delete),
            EditAction::SelectAll => {
                self.editor
                    .set_selection(Selection::Normal(Cursor::new(0, 0)));
                self.editor
                    .action(&mut f.system, Action::Motion(Motion::BufferEnd));
            }
            EditAction::Move(motion, extend) => {
                if extend {
                    if self.editor.selection() == Selection::None {
                        self.editor
                            .set_selection(Selection::Normal(self.editor.cursor()));
                    }
                } else {
                    self.editor.set_selection(Selection::None);
                }
                let cursor = self.to_display(self.editor.cursor());
                let motion = match motion {
                    TextMotion::Left => Motion::Left,
                    TextMotion::Right => Motion::Right,
                    TextMotion::Up => Motion::Up,
                    TextMotion::Down => Motion::Down,
                    TextMotion::Home => Motion::Home,
                    TextMotion::End => Motion::End,
                    TextMotion::Start => Motion::BufferStart,
                    TextMotion::Finish => Motion::BufferEnd,
                    TextMotion::PreviousWord => Motion::PreviousWord,
                    TextMotion::NextWord => Motion::NextWord,
                };
                if let Some((cursor, _)) =
                    self.display
                        .cursor_motion(&mut f.system, cursor, None, motion)
                {
                    self.editor.set_cursor(self.to_source(cursor));
                }
            }
            EditAction::ToggleBold | EditAction::ToggleItalic => {
                if let Some((start, end)) = self.selected_range() {
                    let style = self
                        .selected_style()
                        .unwrap_or_else(|| self.data.style.clone());
                    if bold {
                        let weight = if style.font_weight >= 600 { 400 } else { 700 };
                        self.data
                            .modify_range_styles(start, end, |style| style.font_weight = weight)?;
                    } else {
                        let italic = !style.italic;
                        self.data
                            .modify_range_styles(start, end, |style| style.italic = italic)?;
                    }
                }
            }
            EditAction::Undo | EditAction::Redo => {}
        }
        self.editor
            .with_buffer_mut(|buffer| buffer.shape_until_scroll(&mut f.system, false));
        drop(f);
        let old_content = self.data.content.clone();
        self.sync_content()?;
        if self.data.content != old_content || style_change {
            self.refresh_display()?;
        }
        if let Some(previous) = previous {
            if previous.data != self.data {
                if self.undo.len() >= MAX_HISTORY {
                    self.undo.remove(0);
                }
                self.undo.push(previous);
                self.redo.clear();
            }
        }
        Ok(())
    }

    pub fn cursor_offset(&self) -> usize {
        let cursor = self.editor.cursor();
        self.data
            .content
            .split('\n')
            .take(cursor.line)
            .fold(0usize, |n, line| {
                n.saturating_add(line.len()).saturating_add(1)
            })
            .saturating_add(cursor.index)
            .min(self.data.content.len())
    }

    pub fn composed(
        &self,
        preedit: &str,
        active: Option<std::ops::Range<usize>>,
        highlight: deesainer_color::Rgba,
    ) -> Result<TextData, String> {
        let mut at = self.cursor_offset();
        while !self.data.content.is_char_boundary(at) {
            at -= 1;
        }
        let mut data = self.data.clone();
        if preedit.is_empty() || data.content.len() + preedit.len() > MAX_CONTENT {
            return Ok(data);
        }
        let mut content = data.content.clone();
        content.insert_str(at, preedit);
        data.replace_content(content)?;
        let base = data
            .runs
            .iter()
            .find(|r| r.start <= at && at < r.end)
            .map_or_else(|| data.style.clone(), |r| r.style.clone());
        let underlined = TextStyle {
            decoration: TextDecoration::Underline,
            ..base.clone()
        };
        data.set_range_style(at, at + preedit.len(), underlined.clone())?;
        if let Some(range) = active {
            let byte = |chars: usize| {
                preedit
                    .char_indices()
                    .nth(chars)
                    .map_or(preedit.len(), |(i, _)| i)
            };
            let (a, b) = (at + byte(range.start), at + byte(range.end));
            if a < b {
                let marked = TextStyle {
                    fill: Some(Paint::solid(highlight)),
                    ..underlined
                };
                data.set_range_style(a, b, marked)?;
            }
        }
        Ok(data)
    }

    pub fn selected_text(&self) -> Option<String> {
        self.editor.copy_selection()
    }

    pub fn selected_paragraphs(&self) -> (usize, usize) {
        match self.editor.selection_bounds() {
            Some((a, b)) => (a.line, b.line),
            None => {
                let line = self.editor.cursor().line;
                (line, line)
            }
        }
    }

    pub fn selected_range(&self) -> Option<(usize, usize)> {
        let (start, end) = self.editor.selection_bounds()?;
        let offset = |cursor: Cursor| {
            self.data
                .content
                .split('\n')
                .take(cursor.line)
                .fold(0usize, |n, line| {
                    n.saturating_add(line.len()).saturating_add(1)
                })
                .saturating_add(cursor.index)
        };
        let (start, end) = (offset(start), offset(end));
        (start < end && end <= self.data.content.len()).then_some((start, end))
    }

    pub fn selected_style(&self) -> Option<TextStyle> {
        let (start, _) = self.selected_range()?;
        Some(
            self.data
                .runs
                .iter()
                .find(|run| run.start <= start && start < run.end)
                .map_or_else(|| self.data.style.clone(), |run| run.style.clone()),
        )
    }

    pub fn modify_selected_style(
        &mut self,
        change: impl FnMut(&mut TextStyle),
    ) -> Result<bool, String> {
        let Some((start, end)) = self.selected_range() else {
            return Ok(false);
        };
        let previous = EditCheckpoint {
            editor: self.editor.clone(),
            data: self.data.clone(),
        };
        let mut next = self.data.clone();
        next.modify_range_styles(start, end, change)?;
        if next == previous.data {
            return Ok(true);
        }
        self.data = next;
        if let Err(message) = self.refresh_display() {
            self.data = previous.data;
            return Err(message);
        }
        if self.undo.len() >= MAX_HISTORY {
            self.undo.remove(0);
        }
        self.undo.push(previous);
        self.redo.clear();
        Ok(true)
    }

    fn boundaries(&self, line: &str) -> Vec<(usize, usize)> {
        let mut displayed = 0;
        let mut word_start = true;
        let mut out = Vec::new();
        for (source, c) in line.char_indices() {
            out.push((source, displayed));
            displayed += match self.data.style.case {
                TextCase::Original => c.len_utf8(),
                TextCase::Upper => c.to_uppercase().map(char::len_utf8).sum::<usize>(),
                TextCase::Lower => c.to_lowercase().map(char::len_utf8).sum::<usize>(),
                TextCase::Title if word_start && c.is_alphabetic() => {
                    c.to_uppercase().map(char::len_utf8).sum::<usize>()
                }
                TextCase::Title => c.len_utf8(),
            };
            if self.data.style.case == TextCase::Title {
                word_start = c.is_whitespace();
            }
        }
        out.push((line.len(), displayed));
        out
    }

    fn to_display(&self, cursor: Cursor) -> Cursor {
        let index = self
            .data
            .content
            .split('\n')
            .nth(cursor.line)
            .map(|line| {
                self.boundaries(line)
                    .into_iter()
                    .take_while(|(source, _)| *source <= cursor.index)
                    .last()
                    .map_or(0, |(_, shown)| shown)
            })
            .unwrap_or(0);
        Cursor { index, ..cursor }
    }

    fn to_source(&self, cursor: Cursor) -> Cursor {
        let Some(line) = self.data.content.split('\n').nth(cursor.line) else {
            return Cursor::new(0, 0);
        };
        let index = self
            .boundaries(line)
            .into_iter()
            .min_by_key(|(_, shown)| shown.abs_diff(cursor.index))
            .map(|(source, _)| source)
            .unwrap_or(0);
        Cursor { index, ..cursor }
    }

    pub fn vertical_offset(&self, height: f64) -> f64 {
        if self.data.auto_resize != TextAutoResize::None {
            return 0.0;
        }
        let free = height - self.display_size.1;
        match self.data.align_v {
            TextAlignV::Top => 0.0,
            TextAlignV::Center => free / 2.0,
            TextAlignV::Bottom => free,
        }
    }

    pub fn pointer(
        &mut self,
        point: Point,
        height: f64,
        extend: bool,
        clicks: u8,
    ) -> Result<(), String> {
        if !point.is_finite() {
            return Err("non-finite text pointer".into());
        }
        let dy = self.vertical_offset(height);
        let p = paragraphs(&self.data);
        let local = point.y - dy;
        let shift = self
            .display
            .layout_runs()
            .map(|run| {
                let off = run.line_i as f64 * p.spacing;
                let top = f64::from(run.line_top) + off;
                let gap = if local < top {
                    top - local
                } else {
                    (local - top - f64::from(run.line_height)).max(0.0)
                };
                (gap, off, run.line_i)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or((0.0, 0), |(_, off, line)| (off, line));
        let (shift, line) = shift;
        let x = (point.x - p.indent_of(line)).clamp(-1_000_000.0, 1_000_000.0) as f32;
        let y = (local - shift).clamp(-1_000_000.0, 1_000_000.0) as f32;
        if let Some(hit) = self.display.hit(x, y) {
            let cursor = self.to_source(hit);
            if extend && self.editor.selection() == Selection::None {
                self.editor
                    .set_selection(Selection::Normal(self.editor.cursor()));
            } else if !extend {
                self.editor.set_selection(Selection::None);
            }
            self.editor.set_cursor(cursor);
            let text = || {
                self.editor
                    .with_buffer(|b| b.lines.get(cursor.line).map(|l| l.text().to_owned()))
                    .unwrap_or_default()
            };
            if clicks >= 3 {
                let len = text().len();
                self.editor
                    .set_selection(Selection::Normal(Cursor::new(cursor.line, 0)));
                self.editor.set_cursor(Cursor::new(cursor.line, len));
            } else if clicks == 2 {
                if let Some((start, end)) = word_at(&text(), cursor.index) {
                    self.editor
                        .set_selection(Selection::Normal(Cursor::new(cursor.line, start)));
                    self.editor.set_cursor(Cursor::new(cursor.line, end));
                }
            }
        }
        Ok(())
    }

    pub fn geometry(&self, height: f64) -> EditGeometry {
        let dy = self.vertical_offset(height);
        let cursor = self.to_display(self.editor.cursor());
        let selected = self
            .editor
            .selection_bounds()
            .map(|(a, b)| (self.to_display(a), self.to_display(b)));
        let mut geometry = EditGeometry::default();
        let p = paragraphs(&self.data);
        for run in self.display.layout_runs() {
            let dy = dy + run.line_i as f64 * p.spacing;
            if let Some(x) = run.cursor_position(&cursor) {
                geometry.caret = Some(Rect::new(
                    f64::from(x) + p.indent_of(run.line_i),
                    f64::from(run.line_top) + dy,
                    0.0,
                    f64::from(run.line_height),
                ));
            }
            if let Some((a, b)) = selected.filter(|(a, b)| (a.line..=b.line).contains(&run.line_i))
            {
                for (x, w) in run.highlight(a, b) {
                    geometry.selection.push(Rect::new(
                        f64::from(x) + p.indent_of(run.line_i),
                        f64::from(run.line_top) + dy,
                        f64::from(w),
                        f64::from(run.line_height),
                    ));
                }
            }
        }
        geometry
    }
}

fn word_at(line: &str, index: usize) -> Option<(usize, usize)> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut last = None;
    for (start, word) in line.split_word_bound_indices() {
        let end = start + word.len();
        if (start..end).contains(&index) {
            return Some((start, end));
        }
        last = Some((start, end));
    }
    last.filter(|(_, end)| *end == index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::{TextCase, TextStyle};

    fn data(content: &str) -> TextData {
        TextData {
            content: content.into(),
            style: TextStyle {
                font_family: "sans-serif".into(),
                font_size: 24.0,
                ..TextStyle::default()
            },
            ..TextData::default()
        }
    }

    #[test]
    fn edits_unicode_at_caret_and_reverses_draft() {
        let mut edit = TextEditor::new(data("aé🙂"), None).unwrap();
        edit.action(EditAction::Move(TextMotion::Left, false))
            .unwrap();
        edit.action(EditAction::Backspace).unwrap();
        assert_eq!(edit.data().content, "a🙂");
        edit.action(EditAction::Undo).unwrap();
        assert_eq!(edit.data().content, "aé🙂");
        edit.action(EditAction::Redo).unwrap();
        assert_eq!(edit.data().content, "a🙂");
    }

    #[test]
    fn case_expansion_maps_to_source_boundaries() {
        let mut d = data("aßz");
        d.style.case = TextCase::Upper;
        let mut edit = TextEditor::new(d, None).unwrap();
        let g = edit.geometry(30.0);
        assert!(g.caret.is_some());
        edit.action(EditAction::Move(TextMotion::Left, true))
            .unwrap();
        assert_eq!(edit.selected_text().as_deref(), Some("z"));
        edit.action(EditAction::Move(TextMotion::Left, true))
            .unwrap();
        assert_eq!(edit.selected_text().as_deref(), Some("ßz"));
    }

    #[test]
    fn word_and_line_deletion_and_triple_click() {
        let mut edit = TextEditor::new(data("one two three"), None).unwrap();
        edit.action(EditAction::Move(TextMotion::PreviousWord, true))
            .unwrap();
        edit.action(EditAction::Backspace).unwrap();
        assert_eq!(edit.data().content, "one two ");
        edit.action(EditAction::Move(TextMotion::Home, true))
            .unwrap();
        edit.action(EditAction::Backspace).unwrap();
        assert_eq!(edit.data().content, "");
        let mut edit = TextEditor::new(data("first line\nsecond one"), None).unwrap();
        edit.pointer(Point::new(4.0, 10.0), 60.0, false, 3).unwrap();
        assert_eq!(edit.selected_text().as_deref(), Some("first line"));
        edit.pointer(Point::new(4.0, 10.0), 60.0, false, 2).unwrap();
        assert_eq!(edit.selected_text().as_deref(), Some("first"));
    }

    #[test]
    fn selections_highlight_only_their_lines_and_words_at_their_start() {
        let mut d = data("First paragraph here\nSecond paragraph");
        d.style.font_size = 32.0;
        let mut edit = TextEditor::new(d, None).unwrap();
        let x = {
            edit.pointer(Point::new(1.0, 18.0), 78.0, false, 1).unwrap();
            for _ in 0..6 {
                edit.action(EditAction::Move(TextMotion::Right, false))
                    .unwrap();
            }
            edit.geometry(78.0).caret.unwrap().x
        };
        edit.pointer(Point::new(x + 0.5, 18.0), 78.0, false, 2)
            .unwrap();
        assert_eq!(edit.selected_text().as_deref(), Some("paragraph"));
        let g = edit.geometry(78.0);
        assert_eq!(g.selection.len(), 1, "{:?}", g.selection);
        edit.pointer(Point::new(x, 18.0), 78.0, false, 3).unwrap();
        assert_eq!(
            edit.selected_text().as_deref(),
            Some("First paragraph here")
        );
        assert_eq!(edit.geometry(78.0).selection.len(), 1);
        assert_eq!(word_at("ab cd", 5), Some((3, 5)));
        assert_eq!(word_at("ab cd", 2), Some((2, 3)));
    }

    #[test]
    fn select_all_replace_and_limit() {
        let mut edit = TextEditor::new(data("hello\nworld"), Some(120.0)).unwrap();
        edit.action(EditAction::SelectAll).unwrap();
        edit.action(EditAction::Insert("new".into())).unwrap();
        assert_eq!(edit.data().content, "new");
        assert!(edit
            .action(EditAction::Insert("x".repeat(MAX_CONTENT)))
            .is_err());
        assert_eq!(edit.data().content, "new");
    }

    #[test]
    fn empty_text_still_has_a_caret() {
        let edit = TextEditor::new(data(""), None).unwrap();
        let caret = edit.geometry(30.0).caret.unwrap();
        assert!(caret.height > 0.0);
        assert_eq!(caret.x, 0.0);
    }

    #[test]
    fn selected_bold_survives_typing_and_local_undo() {
        let mut edit = TextEditor::new(data("café"), None).unwrap();
        edit.action(EditAction::SelectAll).unwrap();
        edit.action(EditAction::ToggleBold).unwrap();
        assert_eq!(edit.data().runs[0].style.font_weight, 700);
        edit.action(EditAction::Undo).unwrap();
        assert!(edit.data().runs.is_empty());
        edit.action(EditAction::Redo).unwrap();
        assert_eq!(edit.data().runs[0].style.font_weight, 700);
        edit.action(EditAction::Move(TextMotion::Finish, false))
            .unwrap();
        edit.action(EditAction::Insert("!".into())).unwrap();
        assert_eq!(edit.data().runs[0].end, "café!".len());
    }

    #[test]
    fn selected_typography_changes_draft_and_undo_preserves_unicode_range() {
        let mut edit = TextEditor::new(data("café🙂"), None).unwrap();
        edit.action(EditAction::Move(TextMotion::Left, true))
            .unwrap();
        assert_eq!(edit.selected_range(), Some(("café".len(), "café🙂".len())));
        edit.modify_selected_style(|style| {
            style.font_size = 40.0;
            style.italic = true;
        })
        .unwrap();
        assert_eq!(edit.data().runs.len(), 1);
        assert_eq!(edit.data().runs[0].start, "café".len());
        assert_eq!(edit.selected_style().unwrap().font_size, 40.0);
        edit.action(EditAction::Undo).unwrap();
        assert!(edit.data().runs.is_empty());
        edit.action(EditAction::Redo).unwrap();
        assert_eq!(edit.data().runs[0].style.font_size, 40.0);
    }

    #[test]
    fn invalid_selected_typography_does_not_mutate_draft() {
        let mut edit = TextEditor::new(data("hello"), None).unwrap();
        edit.action(EditAction::SelectAll).unwrap();
        assert!(edit
            .modify_selected_style(|style| style.font_size = f64::NAN)
            .is_err());
        assert!(edit.data().runs.is_empty());
        edit.action(EditAction::Undo).unwrap();
        assert!(edit.data().runs.is_empty());
    }

    #[test]
    fn changing_selected_size_preserves_mixed_families() {
        let mut d = data("ab");
        let mut serif = d.style.clone();
        serif.font_family = "serif".into();
        d.set_range_style(1, 2, serif).unwrap();
        let mut edit = TextEditor::new(d, None).unwrap();
        edit.action(EditAction::SelectAll).unwrap();
        edit.modify_selected_style(|style| style.font_size = 40.0)
            .unwrap();
        assert_eq!(edit.data().runs.len(), 2);
        assert_eq!(edit.data().runs[0].style.font_family, "sans-serif");
        assert_eq!(edit.data().runs[1].style.font_family, "serif");
        assert!(edit.data().runs.iter().all(|r| r.style.font_size == 40.0));
    }

    #[test]
    fn bold_shortcut_preserves_other_mixed_attributes() {
        let mut d = data("ab");
        let mut serif = d.style.clone();
        serif.font_family = "serif".into();
        serif.font_size = 32.0;
        d.set_range_style(1, 2, serif).unwrap();
        let mut edit = TextEditor::new(d, None).unwrap();
        edit.action(EditAction::SelectAll).unwrap();
        edit.action(EditAction::ToggleBold).unwrap();
        assert_eq!(edit.data().runs.len(), 2);
        assert_eq!(edit.data().runs[1].style.font_family, "serif");
        assert_eq!(edit.data().runs[1].style.font_size, 32.0);
        assert!(edit.data().runs.iter().all(|r| r.style.font_weight == 700));
    }

    #[test]
    fn composition_shows_in_the_layer_without_touching_the_editor() {
        let editor = TextEditor::new(data("Hello 世"), None).unwrap();
        let red = deesainer_color::Rgba::new(1.0, 0.0, 0.0, 1.0);
        let shown = editor.composed("ni hao", Some(0..2), red).unwrap();
        assert_eq!(shown.content, "Hello 世ni hao");
        assert_eq!(
            editor.data().content,
            "Hello 世",
            "the editor keeps the real text"
        );
        let at = "Hello 世".len();
        let underlined: Vec<_> = shown
            .runs
            .iter()
            .filter(|r| r.style.decoration == TextDecoration::Underline)
            .collect();
        assert_eq!(underlined.first().unwrap().start, at);
        assert_eq!(underlined.last().unwrap().end, at + "ni hao".len());
        let marked = shown.runs.iter().find(|r| r.style.fill.is_some()).unwrap();
        assert_eq!((marked.start, marked.end), (at, at + 2));
        let wide = editor.composed("你好吗", Some(1..2), red).unwrap();
        let marked = wide.runs.iter().find(|r| r.style.fill.is_some()).unwrap();
        assert_eq!(&wide.content[marked.start..marked.end], "好");
        assert_eq!(
            editor.composed("", None, red).unwrap(),
            editor.data().clone()
        );
        assert_eq!(editor.cursor_offset(), "Hello 世".len());
    }
}
