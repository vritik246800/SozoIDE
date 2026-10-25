use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use markdown::{MarkdownDocument, is_markdown_path, visual_column_to_char};
use ratatui::text::Span;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};

mod highlight;
mod markdown;
mod search;

pub use highlight::{highlight_lines, is_image_path, language_name};
pub use search::{SearchScope, SearchState};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);

pub struct OpenFile {
    pub path: PathBuf,
    pub name: String,
    pub lines: Vec<String>,
    pub highlighted: Vec<Vec<Span<'static>>>,
    markdown: Option<MarkdownDocument>,
    pub image: Option<StatefulProtocol>,
    pub scroll: u16,
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
        if is_image_path(&path) {
            let image = image::ImageReader::open(&path)
                .with_context(|| format!("Could not open {}", path.display()))?
                .decode()
                .with_context(|| format!("Could not decode {}", path.display()))?;
            return Ok(Self {
                path,
                name,
                lines: Vec::new(),
                highlighted: Vec::new(),
                markdown: None,
                image: Some(picker.new_resize_protocol(image)),
                scroll: 0,
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
        let markdown = is_markdown_path(&path).then(|| MarkdownDocument::parse(&lines));
        Ok(Self {
            path,
            name,
            lines,
            highlighted,
            markdown,
            image: None,
            scroll: 0,
            cursor_row: 0,
            cursor_column: 0,
            dirty: false,
            trailing_newline,
            last_edit: None,
        })
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

    pub fn rendered_line(&self, row: usize) -> Option<Vec<Span<'static>>> {
        self.markdown.as_ref()?.rendered_line(row, self.cursor_row)
    }

    pub fn cursor_column_for_click(&self, row: usize, visual_column: usize) -> usize {
        let Some(line) = self.lines.get(row) else {
            return visual_column;
        };
        self.markdown
            .as_ref()
            .and_then(|document| document.source_column(row, self.cursor_row, visual_column))
            .unwrap_or_else(|| visual_column_to_char(line, visual_column))
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
        self.dirty = true;
        self.last_edit = Some(Instant::now());
        self.highlighted = highlight_lines(&self.path, &self.lines);
        self.markdown = is_markdown_path(&self.path).then(|| MarkdownDocument::parse(&self.lines));
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
        self.cursor_row = self.cursor_row.min(self.lines.len().saturating_sub(1));
        self.cursor_column = self.cursor_column.min(self.current_line().chars().count());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(file.rendered_line(1).is_some());
        assert_eq!(file.lines[3], "| Ana | 30 |");
        file.move_cursor_to(3, 4);
        assert!(file.rendered_line(3).is_none());
        file.insert('X');
        assert_eq!(file.lines[3], "| AnXa | 30 |");
        assert_eq!(file.highlighted.len(), file.lines.len());

        file.move_cursor_to(0, 0);
        assert!(file.rendered_line(1).is_some());
        file.save().unwrap();
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("| AnXa | 30 |"));
        assert!(!saved.contains('│'));
        assert!(!saved.contains('├'));
    }
}
