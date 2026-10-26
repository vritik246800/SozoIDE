use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use markdown::{MarkdownDocument, Row, is_markdown_path, visual_column_to_char};
use ratatui::{
    style::Color,
    text::{Line, Span},
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};

mod highlight;
mod markdown;
mod search;

pub use highlight::{file_icon, highlight_lines, is_image_path, language_name};
pub use search::{SearchScope, SearchState};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);

pub struct OpenFile {
    pub path: PathBuf,
    pub name: String,
    pub icon: (char, Color),
    pub lines: Vec<String>,
    pub highlighted: Vec<Vec<Span<'static>>>,
    markdown: Option<MarkdownDocument>,
    /// Rows on screen; a Mermaid diagram can take more rows than its source.
    rows: Vec<Row>,
    pub image: Option<StatefulProtocol>,
    /// The first row on screen (`rows`, not source lines) and the first column.
    pub scroll: u16,
    pub scroll_x: u16,
    /// The widest line as drawn, for the horizontal scroll.
    pub max_width: usize,
    /// The editor pane that shows this tab when the editor is split (F19), set by the app.
    pub pane: usize,
    /// Set when the cursor moves: the next frame scrolls to it. The mouse wheel scrolls freely.
    pub follow_cursor: bool,
    /// The row under the mouse, which shows its Markdown source like the cursor row.
    pub hover_row: Option<usize>,
    pub cursor_row: usize,
    pub cursor_column: usize,
    pub dirty: bool,
    trailing_newline: bool,
    last_edit: Option<Instant>,
}

impl OpenFile {
    pub fn load(path: PathBuf, picker: &mut Picker) -> Result<Self> {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let icon = file_icon(&path);
        if is_image_path(&path) {
            let image = image::ImageReader::open(&path)
                .with_context(|| format!("Could not open {}", path.display()))?
                .decode()
                .with_context(|| format!("Could not decode {}", path.display()))?;
            return Ok(Self {
                path,
                name,
                icon,
                lines: Vec::new(),
                highlighted: Vec::new(),
                markdown: None,
                rows: Vec::new(),
                image: Some(picker.new_resize_protocol(image)),
                scroll: 0,
                scroll_x: 0,
                max_width: 0,
                pane: 0,
                follow_cursor: false,
                hover_row: None,
                cursor_row: 0,
                cursor_column: 0,
                dirty: false,
                trailing_newline: false,
                last_edit: None,
            });
        }

        let bytes =
            fs::read(&path).with_context(|| format!("Could not open {}", path.display()))?;
        if bytes.contains(&0) {
            anyhow::bail!("{} is a binary file", path.display());
        }
        let text = String::from_utf8_lossy(&bytes);
        let trailing_newline = text.ends_with('\n');
        let mut lines = text.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let highlighted = highlight_lines(&path, &lines);
        let mut file = Self {
            path,
            name,
            icon,
            lines,
            highlighted,
            markdown: None,
            rows: Vec::new(),
            image: None,
            scroll: 0,
            scroll_x: 0,
            max_width: 0,
            pane: 0,
            follow_cursor: false,
            hover_row: None,
            cursor_row: 0,
            cursor_column: 0,
            dirty: false,
            trailing_newline,
            last_edit: None,
        };
        file.layout();
        Ok(file)
    }

    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let mut text = self.lines.join("\n");
        if self.trailing_newline {
            text.push('\n');
        }
        fs::write(&self.path, text)
            .with_context(|| format!("Could not save {}", self.path.display()))?;
        self.dirty = false;
        self.last_edit = None;
        Ok(())
    }

    pub fn autosave_due(&self, now: Instant) -> bool {
        self.dirty
            && self
                .last_edit
                .is_some_and(|last_edit| now.duration_since(last_edit) >= AUTOSAVE_DELAY)
    }

    pub fn move_cursor(&mut self, row_delta: isize, column_delta: isize) {
        if row_delta < 0 {
            self.cursor_row = self.cursor_row.saturating_sub(row_delta.unsigned_abs());
        } else {
            self.cursor_row = self
                .cursor_row
                .saturating_add(row_delta as usize)
                .min(self.lines.len().saturating_sub(1));
        }
        if column_delta < 0 {
            if self.cursor_column == 0 && self.cursor_row > 0 {
                self.cursor_row -= 1;
                self.cursor_column = self.current_line().chars().count();
            } else {
                self.cursor_column = self
                    .cursor_column
                    .saturating_sub(column_delta.unsigned_abs());
            }
        } else if column_delta > 0 {
            let line_length = self.current_line().chars().count();
            if self.cursor_column >= line_length && self.cursor_row + 1 < self.lines.len() {
                self.cursor_row += 1;
                self.cursor_column = 0;
            } else {
                self.cursor_column = self
                    .cursor_column
                    .saturating_add(column_delta as usize)
                    .min(line_length);
            }
        }
        self.clamp_cursor();
    }

    pub fn move_cursor_to(&mut self, row: usize, column: usize) {
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        self.cursor_column = column;
        self.clamp_cursor();
    }

    /// Rows that show their Markdown source: the cursor row and the row under the mouse.
    fn source_rows(&self) -> Vec<usize> {
        let mut rows = vec![self.cursor_row];
        rows.extend(self.hover_row);
        rows
    }

    pub fn display_rows(&self) -> usize {
        self.rows.len()
    }

    /// The first row on screen of source line `row`.
    pub fn display_row(&self, row: usize) -> usize {
        self.rows.partition_point(|display| display.source < row)
    }

    /// The source line drawn on screen row `display`.
    pub fn source_row(&self, display: usize) -> Option<usize> {
        self.rows.get(display).map(|row| row.source)
    }

    /// Screen row `display`: its line number (none on the rows a diagram adds) and its text,
    /// drawn Markdown or the highlighted source. A rule reaches `width` columns.
    pub fn display_line(
        &self,
        display: usize,
        width: usize,
    ) -> (Option<usize>, Vec<Span<'static>>) {
        let Some(row) = self.rows.get(display).copied() else {
            return (None, Vec::new());
        };
        let line = &self.lines[row.source];
        let highlighted = &self.highlighted[row.source];
        let spans = self
            .markdown
            .as_ref()
            .and_then(|document| {
                document.render(row, &self.source_rows(), width, line, highlighted)
            })
            .unwrap_or_else(|| {
                if highlighted.is_empty() {
                    vec![Span::raw(line.clone())]
                } else {
                    highlighted.clone()
                }
            });
        ((!row.filler).then_some(row.source + 1), spans)
    }

    pub fn cursor_column_for_click(&self, row: usize, visual_column: usize) -> usize {
        let Some(line) = self.lines.get(row) else {
            return visual_column;
        };
        self.markdown
            .as_ref()
            .and_then(|document| {
                document.source_column(row, &self.source_rows(), visual_column, line)
            })
            .unwrap_or_else(|| visual_column_to_char(line, visual_column))
    }

    /// The columns before the cursor on its line, which always shows its source.
    pub fn cursor_x(&self) -> usize {
        let prefix = self.lines[self.cursor_row]
            .chars()
            .take(self.cursor_column)
            .collect::<String>();
        Line::from(prefix).width()
    }

    /// Scrolls to the cursor before drawing, if it just moved, in a view of `height` rows and
    /// `width` columns; keeps both scrolls inside the text.
    pub fn scroll_to_cursor(&mut self, height: usize, width: usize) {
        if std::mem::take(&mut self.follow_cursor) {
            let row = self.display_row(self.cursor_row);
            if row < self.scroll as usize {
                self.scroll = row as u16;
            } else if height > 0 && row >= self.scroll as usize + height {
                self.scroll = (row + 1 - height) as u16;
            }
            let column = self.cursor_x();
            if column < self.scroll_x as usize {
                self.scroll_x = column as u16;
            } else if width > 0 && column >= self.scroll_x as usize + width {
                self.scroll_x = (column + 1 - width) as u16;
            }
        }
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(1) as u16);
        self.scroll_x = self
            .scroll_x
            .min((self.max_width + 1).saturating_sub(width) as u16);
    }

    pub fn insert(&mut self, character: char) {
        if character == '\n' {
            let index = self.byte_index(self.cursor_column);
            let tail = self.current_line_mut().split_off(index);
            self.lines.insert(self.cursor_row + 1, tail);
            self.cursor_row += 1;
            self.cursor_column = 0;
            self.trailing_newline = false;
        } else {
            let index = self.byte_index(self.cursor_column);
            self.current_line_mut().insert(index, character);
            self.cursor_column += 1;
        }
        self.changed();
    }

    pub fn backspace(&mut self) {
        if self.cursor_column > 0 {
            let start = self.byte_index(self.cursor_column - 1);
            let end = self.byte_index(self.cursor_column);
            self.current_line_mut().replace_range(start..end, "");
            self.cursor_column -= 1;
        } else if self.cursor_row > 0 {
            let line = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_column = self.current_line().chars().count();
            self.current_line_mut().push_str(&line);
            self.trailing_newline = false;
        } else {
            return;
        }
        self.changed();
    }

    pub fn delete(&mut self) {
        let line_length = self.current_line().chars().count();
        if self.cursor_column < line_length {
            let start = self.byte_index(self.cursor_column);
            let end = self.byte_index(self.cursor_column + 1);
            self.current_line_mut().replace_range(start..end, "");
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.current_line_mut().push_str(&next);
            self.trailing_newline = false;
        } else {
            return;
        }
        self.changed();
    }

    #[cfg(test)]
    pub fn make_autosave_due(&mut self) {
        self.last_edit = Some(Instant::now() - AUTOSAVE_DELAY);
    }

    fn changed(&mut self) {
        self.follow_cursor = true;
        self.dirty = true;
        self.last_edit = Some(Instant::now());
        self.highlighted = highlight_lines(&self.path, &self.lines);
        self.layout();
    }

    /// Parses the Markdown again and works out the rows on screen and the widest line.
    fn layout(&mut self) {
        self.markdown = is_markdown_path(&self.path).then(|| MarkdownDocument::parse(&self.lines));
        self.rows = match &self.markdown {
            Some(document) => document.rows(self.lines.len()),
            None => (0..self.lines.len()).map(Row::plain).collect(),
        };
        let widest = self
            .lines
            .iter()
            .map(|line| Line::from(line.as_str()).width());
        self.max_width = widest
            .max()
            .unwrap_or(0)
            .max(self.markdown.as_ref().map_or(0, MarkdownDocument::width));
    }

    fn current_line(&self) -> &String {
        &self.lines[self.cursor_row]
    }

    fn current_line_mut(&mut self) -> &mut String {
        &mut self.lines[self.cursor_row]
    }

    fn byte_index(&self, column: usize) -> usize {
        self.current_line()
            .char_indices()
            .nth(column)
            .map(|(index, _)| index)
            .unwrap_or_else(|| self.current_line().len())
    }

    fn clamp_cursor(&mut self) {
        self.follow_cursor = true;
        self.cursor_row = self.cursor_row.min(self.lines.len().saturating_sub(1));
        self.cursor_column = self.cursor_column.min(self.current_line().chars().count());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source line `row` as drawn on screen.
    fn drawn(file: &OpenFile, row: usize) -> String {
        let (_, spans) = file.display_line(file.display_row(row), 40);
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn markdown_tables_toggle_for_editing_without_changing_saved_source() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("README.md");
        std::fs::write(
            &path,
            "# Demo\n| Name | Age |\n| --- | ---: |\n| Ana | 30 |\nAfter\n",
        )
        .unwrap();
        let mut picker = Picker::halfblocks();
        let mut file = OpenFile::load(path.clone(), &mut picker).unwrap();

        assert_ne!(drawn(&file, 1), file.lines[1]);
        assert_eq!(file.lines[3], "| Ana | 30 |");
        file.move_cursor_to(3, 4);
        assert_eq!(drawn(&file, 3), file.lines[3]);
        file.insert('X');
        assert_eq!(file.lines[3], "| AnXa | 30 |");
        assert_eq!(file.highlighted.len(), file.lines.len());

        file.move_cursor_to(0, 0);
        assert_ne!(drawn(&file, 1), file.lines[1]);
        file.save().unwrap();
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("| AnXa | 30 |"));
        assert!(!saved.contains('│'));
        assert!(!saved.contains('├'));
    }

    #[test]
    fn the_wheel_scrolls_away_from_the_cursor_until_it_moves() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("long.txt");
        std::fs::write(&path, "line\n".repeat(100)).unwrap();
        let mut file = OpenFile::load(path, &mut Picker::halfblocks()).unwrap();
        file.scroll = 50;
        file.scroll_to_cursor(20, 80);
        assert_eq!(
            file.scroll, 50,
            "the cursor on row 0 does not pull the view back"
        );
        file.scroll = 500;
        file.scroll_to_cursor(20, 80);
        assert_eq!(file.scroll, 99, "never past the last line");
        file.move_cursor(1, 0);
        file.scroll_to_cursor(20, 80);
        assert_eq!(file.scroll, 1, "moving the cursor brings it back into view");
    }
}
