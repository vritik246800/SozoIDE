use std::{
    cmp::Ordering,
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
mod mermaid;
mod pdf;
mod search;

pub use highlight::{file_icon, highlight_lines, is_image_path, language_name};
#[cfg(test)]
pub use pdf::sample as sample_pdf;
use pdf::{PdfDocument, is_pdf_path};
pub use search::{SearchScope, SearchState};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);

pub struct OpenFile {
    pub path: PathBuf,
    pub name: String,
    pub icon: (char, Color),
    /// The language or file kind shown in the title.
    pub language: &'static str,
    pub lines: Vec<String>,
    pub highlighted: Vec<Vec<Span<'static>>>,
    markdown: Option<MarkdownDocument>,
    /// Rows on screen; a Mermaid diagram can take more rows than its source.
    rows: Vec<Row>,
    pub image: Option<StatefulProtocol>,
    /// A PDF shows one page at a time as `image` (F21).
    pub pdf: Option<PdfDocument>,
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
    /// Where the selection started; the cursor is its other end. Equal to the cursor, or none,
    /// selects nothing.
    pub anchor: Option<(usize, usize)>,
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
        if is_image_path(&path) || is_pdf_path(&path) {
            let (language, image, pdf) = if is_pdf_path(&path) {
                let bytes = fs::read(&path)
                    .with_context(|| format!("Could not open {}", path.display()))?;
                let document = PdfDocument::open(bytes)
                    .with_context(|| format!("Could not open {}", path.display()))?;
                let page = document
                    .render(0)
                    .with_context(|| format!("Could not draw {}", path.display()))?;
                ("PDF", page, Some(document))
            } else {
                let image = image::ImageReader::open(&path)
                    .with_context(|| format!("Could not open {}", path.display()))?
                    .decode()
                    .with_context(|| format!("Could not decode {}", path.display()))?;
                ("Image", image, None)
            };
            return Ok(Self {
                path,
                name,
                icon,
                language,
                lines: Vec::new(),
                highlighted: Vec::new(),
                markdown: None,
                rows: Vec::new(),
                image: Some(picker.new_resize_protocol(image)),
                pdf,
                scroll: 0,
                scroll_x: 0,
                max_width: 0,
                pane: 0,
                follow_cursor: false,
                hover_row: None,
                cursor_row: 0,
                cursor_column: 0,
                anchor: None,
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
        let language = language_name(&path, &lines[0]);
        let mut file = Self {
            path,
            name,
            icon,
            language,
            lines,
            highlighted,
            markdown: None,
            rows: Vec::new(),
            image: None,
            pdf: None,
            scroll: 0,
            scroll_x: 0,
            max_width: 0,
            pane: 0,
            follow_cursor: false,
            hover_row: None,
            cursor_row: 0,
            cursor_column: 0,
            anchor: None,
            dirty: false,
            trailing_newline,
            last_edit: None,
        };
        file.layout();
        Ok(file)
    }

    /// The page on screen and the number of pages of a PDF.
    pub fn page(&self) -> Option<(usize, usize)> {
        self.pdf
            .as_ref()
            .map(|document| (document.page, document.pages()))
    }

    /// Shows page `page` of a PDF (from 0, kept inside the document).
    pub fn show_page(&mut self, page: usize, picker: &mut Picker) {
        let Some(document) = &mut self.pdf else {
            return;
        };
        let page = page.min(document.pages() - 1);
        if page == document.page {
            return;
        }
        if let Some(image) = document.render(page) {
            document.page = page;
            self.image = Some(picker.new_resize_protocol(image));
        }
    }

    /// The selection, its start before its end, as (row, column) in characters of the source.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let (row, column) = self.anchor?;
        let row = row.min(self.lines.len().saturating_sub(1));
        let anchor = (row, column.min(self.lines[row].chars().count()));
        let cursor = (self.cursor_row, self.cursor_column);
        match anchor.cmp(&cursor) {
            Ordering::Less => Some((anchor, cursor)),
            Ordering::Greater => Some((cursor, anchor)),
            Ordering::Equal => None,
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let ((start_row, start_column), (end_row, end_column)) = self.selection()?;
        let mut text = String::new();
        for row in start_row..=end_row {
            let from = if row == start_row { start_column } else { 0 };
            let to = if row == end_row {
                end_column
            } else {
                usize::MAX
            };
            text.extend(self.lines[row].chars().take(to).skip(from));
            if row < end_row {
                text.push('\n');
            }
        }
        Some(text)
    }

    /// The columns on screen of the selection on source row `row`; a selected line end takes
    /// one more column.
    pub fn selected_columns(&self, row: usize) -> Option<(usize, usize)> {
        let ((start_row, start_column), (end_row, end_column)) = self.selection()?;
        if !(start_row..=end_row).contains(&row) {
            return None;
        }
        let line = &self.lines[row];
        let width =
            |column: usize| Line::from(line.chars().take(column).collect::<String>()).width();
        let from = if row == start_row {
            width(start_column)
        } else {
            0
        };
        let to = if row == end_row {
            width(end_column)
        } else {
            width(usize::MAX) + 1
        };
        Some((from, to))
    }

    /// Before the cursor moves: `extend` keeps the selection, or starts one at the cursor;
    /// otherwise the selection ends.
    pub fn mark(&mut self, extend: bool) {
        if extend {
            self.anchor
                .get_or_insert((self.cursor_row, self.cursor_column));
        } else {
            self.anchor = None;
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some((0, 0));
        self.cursor_row = self.lines.len().saturating_sub(1);
        self.cursor_column = self.current_line().chars().count();
        self.follow_cursor = true;
    }

    /// Removes the selected text and leaves the cursor where it started.
    pub fn delete_selection(&mut self) -> bool {
        let removed = self.remove_selection();
        if removed {
            self.changed();
        }
        removed
    }

    fn remove_selection(&mut self) -> bool {
        let selection = self.selection();
        self.anchor = None;
        let Some(((start_row, start_column), (end_row, end_column))) = selection else {
            return false;
        };
        let end = byte_at(&self.lines[end_row], end_column);
        let tail = self.lines[end_row][end..].to_owned();
        let start = byte_at(&self.lines[start_row], start_column);
        self.lines[start_row].truncate(start);
        self.lines[start_row].push_str(&tail);
        self.lines.drain(start_row + 1..=end_row);
        self.cursor_row = start_row;
        self.cursor_column = start_column;
        true
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
        if let Some(((start, _), (end, _))) = self.selection() {
            rows.extend(start..=end);
        }
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
        self.remove_selection();
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
        if self.delete_selection() {
            return;
        }
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
        if self.delete_selection() {
            return;
        }
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
        byte_at(self.current_line(), column)
    }

    fn clamp_cursor(&mut self) {
        self.follow_cursor = true;
        self.cursor_row = self.cursor_row.min(self.lines.len().saturating_sub(1));
        self.cursor_column = self.cursor_column.min(self.current_line().chars().count());
    }
}

/// The byte where character `column` of `line` starts, or its end.
fn byte_at(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map_or(line.len(), |(index, _)| index)
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

    #[test]
    fn a_selection_copies_deletes_and_is_replaced_by_typing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "first line\nsécond line\nthird\n").unwrap();
        let mut file = OpenFile::load(path, &mut Picker::halfblocks()).unwrap();
        file.move_cursor_to(0, 6);
        file.mark(true);
        file.move_cursor(1, 0);
        file.move_cursor_to(1, 6);
        assert_eq!(file.selected_text().as_deref(), Some("line\nsécond"));
        assert_eq!(
            file.selected_columns(0),
            Some((6, 11)),
            "the line end is selected too"
        );
        assert_eq!(file.selected_columns(1), Some((0, 6)));
        assert_eq!(file.selected_columns(2), None);

        file.insert('X');
        assert_eq!(file.lines, ["first X line", "third"]);
        assert_eq!((file.cursor_row, file.cursor_column), (0, 7));
        assert!(file.selection().is_none());

        // Backwards from the cursor, and the selection ends when the cursor moves alone.
        file.mark(true);
        file.move_cursor_to(0, 0);
        assert_eq!(file.selected_text().as_deref(), Some("first X"));
        file.mark(false);
        assert!(file.selection().is_none());

        file.select_all();
        assert_eq!(file.selected_text().as_deref(), Some("first X line\nthird"));
        assert!(file.delete_selection());
        assert_eq!(file.lines, [""]);
        file.backspace();
        assert_eq!(file.lines, [""]);
    }

    #[test]
    fn pdf_files_open_one_page_at_a_time() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manual.pdf");
        std::fs::write(&path, pdf::sample(2)).unwrap();
        let mut picker = Picker::halfblocks();
        let mut file = OpenFile::load(path, &mut picker).unwrap();
        assert_eq!(file.language, "PDF");
        assert!(file.image.is_some() && file.lines.is_empty());
        assert_eq!(file.page(), Some((0, 2)));
        file.show_page(5, &mut picker);
        assert_eq!(file.page(), Some((1, 2)), "the last page at most");
        file.show_page(0, &mut picker);
        assert_eq!(file.page(), Some((0, 2)));

        std::fs::write(directory.path().join("broken.pdf"), "%PDF-1.4 nothing").unwrap();
        assert!(OpenFile::load(directory.path().join("broken.pdf"), &mut picker).is_err());
    }
}
