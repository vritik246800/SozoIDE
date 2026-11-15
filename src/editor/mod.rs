use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result};
pub use markdown::is_markdown_path as is_markdown;
pub use markdown::{MarkdownDocument, Pointer, Row, heading_row, headings};
use markdown::{is_markdown_path, visual_column_to_char};
use ratatui::{
    style::Color,
    text::{Line, Span},
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};

use crate::media::{self, Media};

mod edit;
mod highlight;
mod markdown;
mod mermaid;
mod pdf;
mod search;
mod text;

pub use edit::{EditorConfig, Indent, detect_indent, editor_config};
use highlight::FileHighlight;
pub use highlight::{file_icon, highlight_lines, language_name};
use pdf::{PageView, PdfDocument, is_pdf_path};
#[cfg(test)]
pub use pdf::{
    sample as sample_pdf, sample_pages as sample_pdf_pages,
    sample_with_outline as sample_pdf_with_outline,
};
pub use search::{
    MAX_MATCHES, Planned, SearchMatch, SearchOptions, SearchScope, SearchState, apply_replace,
    matcher, plan_replace, replace_first, replace_line,
};
pub use text::{Encoding, char_width};

/// From this many lines a file opens in plain text and its colours come from a thread
/// (TODO 2.1.10): opening never waits for them.
const COLOUR_ON_THREAD: usize = 2_000;

/// Where the colours worked out on a thread arrive.
type Colours = std::sync::mpsc::Receiver<(FileHighlight, Vec<Vec<Span<'static>>>)>;
/// A text file above `LARGE_FILE` opens read only and without colours; above `HUGE_FILE` the
/// app asks first; above `MAX_FILE` it does not open (R11).
const LARGE_FILE: u64 = 5 * 1024 * 1024;
const HUGE_FILE: u64 = 50 * 1024 * 1024;
// ponytail: a file is read whole, as lines and as spans; a paged reader if logs over 512 MB
// ever matter.
const MAX_FILE: u64 = 512 * 1024 * 1024;
/// A PDF is read whole before its first page is drawn.
const MOST_PDF: u64 = 256 * 1024 * 1024;
/// Edits of one kind closer than this undo together: a word typed, a paste.
const UNDO_GROUP: Duration = Duration::from_secs(1);
/// Steps of undo kept for each file.
const UNDO_STEPS: usize = 200;

/// The text and the cursor before an edit.
struct Snapshot {
    lines: Vec<String>,
    cursor: (usize, usize),
    trailing_newline: bool,
}

/// What an edit did, to group the ones that undo together.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    Type,
    Erase,
    Cut,
    Paste,
    Reload,
}

/// A text file so large that the app asks before reading it whole (R11). Pictures, videos and
/// PDFs have their own readers and limits.
pub fn is_huge_text(path: &Path) -> bool {
    media::kind(path).is_none()
        && !is_pdf_path(path)
        && fs::metadata(path).is_ok_and(|file| file.len() > HUGE_FILE)
}

pub struct OpenFile {
    pub path: PathBuf,
    pub name: String,
    pub icon: (char, Color),
    /// The language or file kind shown in the title.
    pub language: &'static str,
    pub lines: Vec<String>,
    pub highlighted: Vec<Vec<Span<'static>>>,
    highlight: FileHighlight,
    markdown: Option<MarkdownDocument>,
    /// Rows on screen; a Mermaid diagram can take more rows than its source.
    rows: Vec<Row>,
    pub image: Option<StatefulProtocol>,
    /// A picture, GIF, video or 3D model, drawn as text (F03, F22, F23).
    pub media: Option<Media>,
    /// A PDF shows one page at a time as `image` (F21).
    pub pdf: Option<PdfDocument>,
    /// The first row on screen (`rows`, not source lines) and the first column.
    pub scroll: u16,
    pub scroll_x: u16,
    /// The first row as drawn: it runs to `scroll` in steps after a page (`SCROLL`, F08).
    pub shown_scroll: crate::animation::Scroll,
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
    /// Too large to edit (`LARGE_FILE`): read, selected, copied and searched, never changed.
    pub read_only: bool,
    /// Counts the edits, from 1: the language server is sent the text when it changes (F26).
    pub edits: u64,
    trailing_newline: bool,
    /// The text before each edit, for `undo`, and the kind and time of the last one.
    history: Vec<Snapshot>,
    /// What `undo` took back, for `redo` (TODO 2.1.1); a new edit forgets it.
    future: Vec<Snapshot>,
    /// How this file indents: its `.editorconfig`, else its lines, else the settings.
    pub indent: Indent,
    /// Its lines end with `\r\n`; it is saved so, never mixed (TODO 2.1.7).
    pub crlf: bool,
    pub encoding: Encoding,
    /// Not valid text (bad UTF-8): shown, never saved over (TODO 2.1.7).
    pub not_text: bool,
    /// What `.editorconfig` asks of a save.
    config: EditorConfig,
    /// Long lines wrap into rows of `wrap_width` columns (TODO 2.1.8).
    pub wrap: bool,
    pub wrap_width: usize,
    /// How long after the last key the file saves itself; none: never (F42).
    pub autosave: Option<Duration>,
    /// Colours being worked out on a thread, for the text as it was at that edit.
    colouring: Option<(u64, Colours)>,
    last_change: Option<(Change, Instant)>,
    last_edit: Option<Instant>,
    /// The file on disk when it was read or saved: its modification time and size, to see
    /// another program change it.
    disk: Option<(SystemTime, u64)>,
}

impl OpenFile {
    pub fn load(path: PathBuf, picker: &mut Picker) -> Result<Self> {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let icon = file_icon(&path);
        if media::kind(&path).is_some() || is_pdf_path(&path) {
            // Pictures, GIFs, videos and models are drawn as text (Q43); a PDF page is its text,
            // or a picture in the terminal's picture protocol when it draws more than text (Q50).
            let (language, pdf, media) = if is_pdf_path(&path) {
                if fs::metadata(&path).is_ok_and(|file| file.len() > MOST_PDF) {
                    anyhow::bail!("{name} is larger than 256 MB: too large to open");
                }
                let bytes = fs::read(&path)
                    .with_context(|| format!("Could not open {}", path.display()))?;
                let document = media::within(media::OPENING, || PdfDocument::open(bytes))
                    .with_context(|| format!("Could not open {}", path.display()))?;
                ("PDF", Some(document), None)
            } else {
                let file = path.clone();
                let media = media::within(media::OPENING, move || Media::open(&file))?;
                (media.language(), None, Some(media))
            };
            let mut file = Self {
                path,
                name,
                icon,
                language,
                lines: Vec::new(),
                highlighted: Vec::new(),
                highlight: FileHighlight::default(),
                markdown: None,
                rows: Vec::new(),
                image: None,
                pdf,
                media,
                scroll: 0,
                scroll_x: 0,
                shown_scroll: crate::animation::Scroll::new(std::time::Instant::now(), 0),
                max_width: 0,
                pane: 0,
                follow_cursor: false,
                hover_row: None,
                cursor_row: 0,
                cursor_column: 0,
                anchor: None,
                dirty: false,
                read_only: false,
                edits: 1,
                trailing_newline: false,
                history: Vec::new(),
                future: Vec::new(),
                indent: Indent::default(),
                crlf: false,
                encoding: Encoding::Utf8,
                not_text: false,
                config: EditorConfig::default(),
                wrap: false,
                wrap_width: 0,
                autosave: None,
                colouring: None,
                last_change: None,
                last_edit: None,
                disk: None,
            };
            if file.pdf.is_some() && !file.set_page(0, picker) {
                anyhow::bail!("Could not draw {}", file.path.display());
            }
            return Ok(file);
        }

        let disk = on_disk(&path);
        let size = disk.map_or(0, |(_, size)| size);
        if size > MAX_FILE {
            anyhow::bail!("{name} is larger than 512 MB: too large to open");
        }
        let decoded = read_lines(&path)?;
        let not_text = !decoded.valid;
        let read_only = size > LARGE_FILE || not_text;
        let (lines, trailing_newline, crlf, encoding) = (
            decoded.lines,
            decoded.trailing_newline,
            decoded.crlf,
            decoded.encoding,
        );
        let settings = crate::settings::get();
        let config = editor_config(&path, path.ancestors().last().unwrap_or(&path));
        let indent = config
            .indent
            .or_else(|| detect_indent(&lines))
            .unwrap_or(Indent {
                tabs: !settings.insert_spaces,
                width: settings.tab_width,
            });
        let wrap = if markdown::is_markdown_path(&path) {
            settings.wrap_markdown
        } else {
            settings.wrap
        };
        let mut highlight = FileHighlight::default();
        let plain = |lines: &[String]| {
            lines
                .iter()
                .map(|line| vec![Span::raw(line.clone())])
                .collect()
        };
        let (highlighted, language) = if read_only {
            (plain(&lines), "Read only")
        } else if lines.len() >= COLOUR_ON_THREAD {
            (plain(&lines), language_name(&path, &lines[0]))
        } else {
            (
                highlight.update(&path, &lines),
                language_name(&path, &lines[0]),
            )
        };
        let mut file = Self {
            path,
            name,
            icon,
            language,
            lines,
            highlighted,
            highlight,
            markdown: None,
            rows: Vec::new(),
            image: None,
            pdf: None,
            media: None,
            scroll: 0,
            scroll_x: 0,
            shown_scroll: crate::animation::Scroll::new(std::time::Instant::now(), 0),
            max_width: 0,
            pane: 0,
            follow_cursor: false,
            hover_row: None,
            cursor_row: 0,
            cursor_column: 0,
            anchor: None,
            dirty: false,
            read_only,
            edits: 1,
            trailing_newline,
            history: Vec::new(),
            future: Vec::new(),
            indent,
            crlf,
            encoding,
            not_text,
            config,
            wrap,
            wrap_width: 0,
            autosave: settings.autosave(),
            colouring: None,
            last_change: None,
            last_edit: None,
            disk,
        };
        if !read_only && file.lines.len() >= COLOUR_ON_THREAD {
            file.colour_on_thread(FileHighlight::default());
        }
        file.layout();
        Ok(file)
    }

    /// Works out the colours of the whole text on a thread; `tick_colours` takes them.
    fn colour_on_thread(&mut self, mut highlight: FileHighlight) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (path, lines) = (self.path.clone(), self.lines.clone());
        std::thread::spawn(move || {
            let spans = highlight.update(&path, &lines);
            let _ = sender.send((highlight, spans));
        });
        self.colouring = Some((self.edits, receiver));
    }

    /// In the tick: the colours from the thread, when they are for this text; for an older one
    /// the thread goes again from where it got (only what changed is worked out again).
    pub fn tick_colours(&mut self) -> bool {
        let Some((edits, receiver)) = &self.colouring else {
            return false;
        };
        let Ok((highlight, spans)) = receiver.try_recv() else {
            return false;
        };
        if *edits == self.edits {
            self.highlight = highlight;
            self.highlighted = spans;
            self.colouring = None;
            self.layout();
            true
        } else {
            self.colour_on_thread(highlight);
            false
        }
    }

    /// The colours are still on their way (the file is drawn in plain text).
    pub fn colouring(&self) -> bool {
        self.colouring.is_some()
    }

    /// Text to edit, not a picture, a PDF, a video or a model.
    /// The target of the `[[ ]]` at `column` of row `row` of a Markdown file (F40).
    pub fn wiki_link_at(&self, row: usize, column: usize) -> Option<String> {
        let document = self.markdown.as_ref()?;
        if document.is_code(row) {
            return None;
        }
        markdown::wiki_link_at(self.lines.get(row)?, column)
    }

    /// Outside code in a Markdown file: a task box ticked or not at `column` of `row`, as one
    /// edit (TODO 5.1.9); false when there is none there.
    pub fn toggle_task_at(&mut self, row: usize, column: usize) -> bool {
        if self
            .markdown
            .as_ref()
            .is_none_or(|document| document.is_code(row))
        {
            return false;
        }
        let Some(changed) = self
            .lines
            .get(row)
            .and_then(|line| markdown::toggle_task(line, column))
        else {
            return false;
        };
        let length = self.lines[row].chars().count();
        self.replace_range(row, 0, length, &changed);
        true
    }

    /// A `[text](target)` or a `[^note]` at `column` of `row` of a Markdown file.
    pub fn pointer_at(&self, row: usize, column: usize) -> Option<Pointer> {
        let document = self.markdown.as_ref()?;
        if document.is_code(row) {
            return None;
        }
        markdown::pointer_at(self.lines.get(row)?, column)
    }

    /// The `#heading` of a `[[ ]]` at `column` of `row`.
    pub fn wiki_heading_at(&self, row: usize, column: usize) -> Option<String> {
        markdown::wiki_heading_at(self.lines.get(row)?, column)
    }

    pub fn is_text(&self) -> bool {
        self.shows_text() && self.pdf.is_none() && !self.read_only
    }

    /// Lines of text on screen, to edit or, on a page of a PDF, to select and search.
    pub fn shows_text(&self) -> bool {
        self.image.is_none() && self.media.is_none()
    }

    /// The page on screen and the number of pages of a PDF.
    pub fn page(&self) -> Option<(usize, usize)> {
        self.pdf
            .as_ref()
            .map(|document| (document.page, document.pages()))
    }

    /// Zooms or moves the page of a PDF drawn as a picture (TODO 5.1.6); false for other keys.
    pub fn zoom_page(
        &mut self,
        character: Option<char>,
        arrow: Option<(f32, f32)>,
        picker: &mut Picker,
    ) -> bool {
        let Some(document) = &mut self.pdf else {
            return false;
        };
        if self.image.is_none() || !document.zoom_key(character, arrow) {
            return false;
        }
        let page = document.page;
        self.set_page(page, picker);
        true
    }

    /// Shows page `page` of a PDF (from 0, kept inside the document).
    pub fn show_page(&mut self, page: usize, picker: &mut Picker) {
        let Some(document) = &self.pdf else {
            return;
        };
        let page = page.min(document.pages() - 1);
        if page != document.page {
            self.set_page(page, picker);
        }
    }

    /// Puts page `page` on screen: its text from the top, or its picture; false when it
    /// cannot be drawn.
    fn set_page(&mut self, page: usize, picker: &mut Picker) -> bool {
        let Some(view) = self.pdf.as_mut().and_then(|document| document.view(page)) else {
            return false;
        };
        let lines = match view {
            PageView::Text(lines) => {
                self.image = None;
                lines
            }
            PageView::Picture(picture) => {
                let picture = match &self.pdf {
                    Some(document) => document.zoomed(picture),
                    None => picture,
                };
                self.image = Some(picker.new_resize_protocol(picture));
                Vec::new()
            }
        };
        self.highlighted = lines
            .iter()
            .map(|line| vec![Span::raw(line.clone())])
            .collect();
        self.lines = lines;
        (self.cursor_row, self.cursor_column, self.anchor) = (0, 0, None);
        (self.scroll, self.scroll_x) = (0, 0);
        self.layout();
        if let Some(document) = &mut self.pdf {
            document.page = page;
            document.prefetch();
        }
        true
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
        if self.read_only {
            return false;
        }
        if self.selection().is_some() {
            self.remember(Change::Cut);
        }
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
        // The text of a PDF page is only read: it is never written over the PDF.
        if !self.dirty || self.pdf.is_some() {
            return Ok(());
        }
        if self.not_text {
            anyhow::bail!("{} is not valid text: it is never saved over", self.name);
        }
        if self.config.trim_trailing == Some(true) {
            for line in &mut self.lines {
                let kept = line.trim_end_matches([' ', '\t']).len();
                line.truncate(kept);
            }
        }
        if self.config.final_newline == Some(true) {
            self.trailing_newline = true;
        }
        fs::write(&self.path, text::encode(&self.text(), self.encoding))
            .with_context(|| format!("Could not save {}", self.path.display()))?;
        self.dirty = false;
        self.last_edit = None;
        self.disk = on_disk(&self.path);
        Ok(())
    }

    /// Another program changed the file since it was read or saved (a text file still there).
    pub fn changed_on_disk(&self) -> bool {
        self.is_text() && on_disk(&self.path).is_some_and(|disk| Some(disk) != self.disk)
    }

    /// Another program changed the file while it has edits not saved: they stay, and are
    /// saved over it (Q40).
    pub fn keep_edits(&mut self) {
        self.disk = on_disk(&self.path);
    }

    /// Reads the file again, keeping the cursor and the scroll; Ctrl+Z brings back the text
    /// before.
    pub fn reload(&mut self) -> Result<()> {
        let disk = on_disk(&self.path);
        let decoded = read_lines(&self.path)?;
        self.last_change = None;
        self.remember(Change::Reload);
        self.lines = decoded.lines;
        self.trailing_newline = decoded.trailing_newline;
        self.crlf = decoded.crlf;
        self.encoding = decoded.encoding;
        self.anchor = None;
        self.dirty = false;
        self.last_edit = None;
        self.disk = disk;
        self.clamp_cursor();
        self.follow_cursor = false;
        self.edits += 1;
        self.highlighted = self.highlight.update(&self.path, &self.lines);
        self.layout();
        Ok(())
    }

    /// The text as it is saved.
    pub fn text(&self) -> String {
        let end = if self.crlf { "\r\n" } else { "\n" };
        let mut text = self.lines.join(end);
        if self.trailing_newline {
            text.push_str(end);
        }
        text
    }

    pub fn autosave_due(&self, now: Instant) -> bool {
        self.dirty
            && !self.not_text
            && self.autosave.is_some_and(|delay| {
                self.last_edit
                    .is_some_and(|last_edit| now.duration_since(last_edit) >= delay)
            })
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
        // The mouse over a `[[ ]]` keeps showing its text, to click it (F40).
        rows.extend(self.hover_row.filter(|row| {
            !self
                .lines
                .get(*row)
                .is_some_and(|line| markdown::has_wiki_link(line))
        }));
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
        if self.wrap && self.wrap_width > 0 && !row.is_diagram() {
            let from = row.part_index() * self.wrap_width;
            let spans = text::slice_spans(&spans, from, from + self.wrap_width);
            let number = (!row.filler && row.part_index() == 0).then_some(row.source + 1);
            return (number, spans);
        }
        ((!row.filler).then_some(row.source + 1), spans)
    }

    /// The source line of display row `display` when it shows text (not the rows a diagram
    /// adds), also on the rows a wrapped line goes on to.
    pub fn display_source(&self, display: usize) -> Option<usize> {
        self.rows
            .get(display)
            .filter(|row| !row.filler)
            .map(|row| row.source)
    }

    /// The first column of the source line that display row `display` shows: more than 0 on
    /// the rows a wrapped line goes on to.
    pub fn display_offset(&self, display: usize) -> usize {
        match self.rows.get(display) {
            Some(row) if self.wrap && self.wrap_width > 0 => row.part_index() * self.wrap_width,
            _ => 0,
        }
    }

    /// The row on screen and the column of the cursor, also on a wrapped line.
    pub fn cursor_place(&self) -> (usize, usize) {
        let x = self.cursor_x();
        let first = self.display_row(self.cursor_row);
        if self.wrap && self.wrap_width > 0 {
            let parts = text::wrapped_rows(&self.lines[self.cursor_row], self.wrap_width);
            let part = (x / self.wrap_width).min(parts - 1);
            (first + part, x - part * self.wrap_width)
        } else {
            (first, x)
        }
    }

    /// Alt+Z: long lines wrap on screen, or not.
    pub fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.scroll_x = 0;
        self.follow_cursor = true;
        self.layout();
    }

    /// The editor's width for wrapping, from the frame; the rows are worked out again when
    /// it changes.
    pub fn set_wrap_width(&mut self, width: usize) {
        if self.wrap && self.wrap_width != width {
            self.wrap_width = width;
            self.layout();
        }
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
            let (row, column) = self.cursor_place();
            if row < self.scroll as usize {
                self.scroll = row as u16;
            } else if height > 0 && row >= self.scroll as usize + height {
                self.scroll = (row + 1 - height) as u16;
            }
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
        if self.read_only {
            return;
        }
        self.remember(Change::Type);
        self.remove_selection();
        self.put(character);
        self.changed();
    }

    /// Pastes `text` over the selection as one edit: one step of undo, highlighted once.
    pub fn insert_str(&mut self, text: &str) {
        let characters = text
            .chars()
            .filter(|character| matches!(character, '\n' | '\t') || !character.is_control())
            .collect::<Vec<_>>();
        if self.read_only || characters.is_empty() && self.selection().is_none() {
            return;
        }
        // A paste is a step of its own, never part of the edit before it.
        self.last_change = None;
        self.remember(Change::Paste);
        self.remove_selection();
        for character in characters {
            self.put(character);
        }
        self.changed();
    }

    /// Puts one character at the cursor, without the bookkeeping of an edit.
    fn put(&mut self, character: char) {
        if character == '\n' {
            let index = self.byte_index(self.cursor_column);
            let tail = self.current_line_mut().split_off(index);
            self.lines.insert(self.cursor_row + 1, tail);
            self.cursor_row += 1;
            self.cursor_column = 0;
        } else {
            let index = self.byte_index(self.cursor_column);
            self.current_line_mut().insert(index, character);
            self.cursor_column += 1;
        }
    }

    pub fn backspace(&mut self) {
        if self.read_only || self.delete_selection() {
            return;
        }
        if self.cursor_column > 0 || self.cursor_row > 0 {
            self.remember(Change::Erase);
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
        } else {
            return;
        }
        self.changed();
    }

    pub fn delete(&mut self) {
        if self.read_only || self.delete_selection() {
            return;
        }
        let line_length = self.current_line().chars().count();
        if self.cursor_column < line_length || self.cursor_row + 1 < self.lines.len() {
            self.remember(Change::Erase);
        }
        if self.cursor_column < line_length {
            let start = self.byte_index(self.cursor_column);
            let end = self.byte_index(self.cursor_column + 1);
            self.current_line_mut().replace_range(start..end, "");
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.current_line_mut().push_str(&next);
        } else {
            return;
        }
        self.changed();
    }

    /// Keeps the text before an edit, unless it continues the last one (same kind, within
    /// `UNDO_GROUP`).
    fn remember(&mut self, change: Change) {
        let now = Instant::now();
        let continues = self
            .last_change
            .is_some_and(|(last, at)| last == change && now.duration_since(at) < UNDO_GROUP);
        self.last_change = Some((change, now));
        if continues {
            return;
        }
        if self.history.len() == UNDO_STEPS {
            self.history.remove(0);
        }
        self.history.push(self.snapshot());
        self.future.clear();
    }

    /// Puts back the text before the last edit; false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.history.pop() else {
            return false;
        };
        self.future.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    #[cfg(test)]
    pub fn make_autosave_due(&mut self) {
        self.last_edit = Some(Instant::now() - self.autosave.unwrap_or_default());
    }

    /// The text of the same file in the other pane, as it is edited there (TODO 5.2.6): it
    /// is the other tab that saves it.
    pub fn take_text_of(&mut self, lines: &[String]) {
        if self.lines == lines {
            return;
        }
        self.lines = lines.to_vec();
        self.changed();
        (self.dirty, self.last_edit, self.follow_cursor) = (false, None, false);
        self.cursor_row = self.cursor_row.min(self.lines.len().saturating_sub(1));
        let length = self
            .lines
            .get(self.cursor_row)
            .map_or(0, |line| line.chars().count());
        self.cursor_column = self.cursor_column.min(length);
        self.anchor = None;
    }

    fn changed(&mut self) {
        self.follow_cursor = true;
        self.dirty = true;
        self.edits += 1;
        self.last_edit = Some(Instant::now());
        if self.colouring.is_some() {
            // The thread brings the colours; until then the text is plain.
            self.highlighted = self
                .lines
                .iter()
                .map(|line| vec![Span::raw(line.clone())])
                .collect();
        } else {
            self.highlighted = self.highlight.update(&self.path, &self.lines);
        }
        self.layout();
    }

    /// Parses the Markdown again and works out the rows on screen and the widest line.
    fn layout(&mut self) {
        // A large file is shown as it is: drawing its Markdown would take as long as its colours.
        // Pictures are read from beside the document, never from above the folder it is in.
        let folder = self
            .path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        self.markdown = (is_markdown_path(&self.path) && !self.read_only)
            .then(|| MarkdownDocument::parse_in(&self.lines, &folder, &folder));
        self.rows = match &self.markdown {
            Some(document) => document.rows(self.lines.len()),
            None => (0..self.lines.len()).map(Row::plain).collect(),
        };
        // Long lines wrap into more rows, without a number (TODO 2.1.8); diagrams keep theirs.
        if self.wrap && self.wrap_width > 0 {
            let width = self.wrap_width;
            self.rows = std::mem::take(&mut self.rows)
                .into_iter()
                .flat_map(|row| {
                    let parts = if row.is_diagram() {
                        1
                    } else {
                        text::wrapped_rows(&self.lines[row.source], width)
                    };
                    (0..parts).map(move |part| row.part(part))
                })
                .collect();
        }
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
        // A picture, video, model or PDF has no lines (E34).
        let length = self
            .lines
            .get(self.cursor_row)
            .map_or(0, |line| line.chars().count());
        self.cursor_column = self.cursor_column.min(length);
    }
}

/// The modification time and the size of a file, none when it cannot be read.
fn on_disk(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// The lines of a text file and whether it ends with a newline.
fn read_lines(path: &Path) -> Result<text::Decoded> {
    let bytes = fs::read(path).with_context(|| format!("Could not open {}", path.display()))?;
    text::decode(&bytes).ok_or_else(|| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        anyhow::anyhow!("{name} is a binary file: there is no text in it to show")
    })
}

/// The byte where character `column` of `line` starts, or its end.
fn byte_at(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map_or(line.len(), |(index, _)| index)
}

/// The file a `[[target]]` means (F40), as a vault finds it: beside `from`, from `root`, then
/// the file anywhere under `root` whose path ends in it, then the one with its name. `.md`
/// when it has no extension.
pub fn find_document(target: &str, from: Option<&Path>, root: &Path) -> Option<PathBuf> {
    // A link is text someone else may have written: it opens nothing outside the project,
    // by `..`, by a whole path or through a link on disk.
    let inside = root.canonicalize().ok()?;
    document(target, from, root).filter(|found| found.starts_with(&inside))
}

fn document(target: &str, from: Option<&Path>, root: &Path) -> Option<PathBuf> {
    let target = target.replace('\\', "/");
    let name = if Path::new(&target).extension().is_some() {
        target
    } else {
        format!("{target}.md")
    };
    let beside = from.and_then(Path::parent).map(|folder| folder.join(&name));
    if let Some(found) = beside
        .into_iter()
        .chain([root.join(&name)])
        .find(|path| path.is_file())
    {
        return found.canonicalize().ok();
    }
    // ponytail: walks the folder at each click; an index if vaults get slow.
    let files = ignore::WalkBuilder::new(root)
        .hidden(false)
        .sort_by_file_name(Ord::cmp)
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(ignore::DirEntry::into_path)
        .collect::<Vec<_>>();
    let wanted = name.to_lowercase();
    let last = wanted.rsplit('/').next().unwrap_or(&wanted).to_owned();
    let lower = |path: &Path| path.to_string_lossy().replace('\\', "/").to_lowercase();
    files
        .iter()
        .find(|path| lower(&full(path)).ends_with(&format!("/{wanted}")))
        .or_else(|| {
            files
                .iter()
                .find(|path| lower(path).ends_with(&format!("/{last}")))
        })
        .and_then(|path| path.canonicalize().ok())
}

/// `path` with the folders above it, so a vault's `Project/Docs/a` finds it from inside
/// `Project`.
fn full(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_wiki_link_never_opens_what_is_outside_the_project() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("vault");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/a.md"), "[[../../secret]]").unwrap();
        std::fs::write(root.join("inside.md"), "in").unwrap();
        let secret = directory.path().join("secret.md");
        std::fs::write(&secret, "private").unwrap();
        let from = root.join("notes/a.md");
        let find = |target: &str| find_document(target, Some(&from), &root);
        assert_eq!(
            find("../inside"),
            root.join("inside.md").canonicalize().ok()
        );
        for target in [
            "../../secret",
            "..\\..\\secret",
            "notes/../../secret",
            secret.to_str().unwrap(),
            "../../secret.md",
        ] {
            assert_eq!(find(target), None, "{target}");
        }
        // Nor through a link on disk that leaves the project.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, root.join("way-out.md")).unwrap();
            assert_eq!(find("way-out"), None);
        }
    }

    #[test]
    fn a_wiki_link_finds_its_document_as_a_vault_does() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Project");
        let folder = root.join("Docs/planning");
        fs::create_dir_all(&folder).unwrap();
        for file in [
            "Docs/planning/CHANGELOG.md",
            "Docs/planning/near.md",
            "README.md",
        ] {
            fs::write(root.join(file), "text").unwrap();
        }
        fs::write(root.join("image.png"), "png").unwrap();
        let from = folder.join("near.md");
        let find = |target| {
            find_document(target, Some(&from), &root)
                .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        };
        assert_eq!(find("near").as_deref(), Some("near.md"), "beside the file");
        assert_eq!(
            find("README").as_deref(),
            Some("README.md"),
            "from the root"
        );
        assert_eq!(
            find("image.png").as_deref(),
            Some("image.png"),
            "its own extension"
        );
        assert_eq!(find("planning/CHANGELOG").as_deref(), Some("CHANGELOG.md"));
        // A path from the vault above the project, an Obsidian one.
        assert_eq!(
            find("Project/Docs/planning/CHANGELOG").as_deref(),
            Some("CHANGELOG.md")
        );
        assert_eq!(
            find("changelog").as_deref(),
            Some("CHANGELOG.md"),
            "by name"
        );
        assert_eq!(find("Other/changelog").as_deref(), Some("CHANGELOG.md"));
        assert_eq!(find("gone"), None);
    }

    use super::*;

    /// Source line `row` as drawn on screen.
    fn drawn(file: &OpenFile, row: usize) -> String {
        let (_, spans) = file.display_line(file.display_row(row), 40);
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn undo_puts_back_words_erasures_and_cuts_one_step_at_a_time() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.txt");
        fs::write(&path, "one\n").unwrap();
        let mut picker = Picker::halfblocks();
        let mut file = OpenFile::load(path, &mut picker).unwrap();
        assert!(!file.undo());
        file.move_cursor_to(0, 3);
        for character in " two\nthree".chars() {
            file.insert(character);
        }
        file.backspace();
        file.backspace();
        file.mark(false);
        file.select_all();
        file.delete_selection();
        assert_eq!(file.lines, [""]);
        // The cut, then both erasures, then everything typed in one go.
        assert!(file.undo());
        assert_eq!(file.lines, ["one two", "thr"]);
        assert!(file.undo());
        assert_eq!(file.lines, ["one two", "three"]);
        assert_eq!((file.cursor_row, file.cursor_column), (1, 5));
        assert!(file.undo());
        assert_eq!(file.lines, ["one"]);
        assert!(file.dirty && !file.undo());
        // A pause starts a new step.
        file.insert('!');
        file.last_change = file
            .last_change
            .map(|(change, at)| (change, at - UNDO_GROUP));
        file.insert('?');
        assert!(file.undo());
        assert_eq!(file.lines, ["one!"]);
    }

    #[test]
    fn new_lines_and_joined_lines_keep_the_newline_at_the_end_of_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.rs");
        fs::write(&path, "one\ntwo\n").unwrap();
        let mut file = OpenFile::load(path.clone(), &mut Picker::halfblocks()).unwrap();
        file.move_cursor_to(0, 3);
        file.insert('\n');
        file.insert('x');
        assert_eq!(file.text(), "one\nx\ntwo\n", "a new line (E47)");
        file.move_cursor_to(1, 1);
        file.delete();
        assert_eq!(file.text(), "one\nxtwo\n", "Delete joins lines");
        file.move_cursor_to(1, 0);
        file.backspace();
        assert_eq!(file.text(), "onextwo\n", "Backspace joins lines");
        file.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "onextwo\n");
        // A file without one stays without.
        fs::write(&path, "one").unwrap();
        let mut file = OpenFile::load(path, &mut Picker::halfblocks()).unwrap();
        file.move_cursor_to(0, 3);
        file.insert('\n');
        assert_eq!(file.text(), "one\n");
    }

    #[test]
    fn a_paste_is_one_edit_and_a_file_changed_on_disk_reads_again() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.md");
        fs::write(&path, "one\ntwo\n").unwrap();
        let mut file = OpenFile::load(path.clone(), &mut Picker::halfblocks()).unwrap();
        file.move_cursor_to(0, 3);
        file.insert('!');
        file.insert_str(" pasted\r\nline\u{7}");
        assert_eq!(file.lines, ["one! pasted", "line", "two"]);
        assert_eq!(file.highlighted.len(), 3);
        assert!(file.undo());
        assert_eq!(file.lines, ["one!", "two"], "the paste alone goes back");
        file.save().unwrap();
        assert!(!file.changed_on_disk(), "its own save is not a change");

        // Another program writes the file: it reads again with the cursor where it was.
        file.move_cursor_to(1, 2);
        fs::write(&path, "uno\ndos\ntres\n").unwrap();
        assert!(file.changed_on_disk());
        file.reload().unwrap();
        assert_eq!(file.lines, ["uno", "dos", "tres"]);
        assert_eq!((file.cursor_row, file.cursor_column), (1, 2));
        assert!(!file.dirty && !file.changed_on_disk());
        assert_eq!(file.display_rows(), 3);
        assert!(file.undo());
        assert_eq!(
            file.lines,
            ["one!", "two"],
            "Ctrl+Z brings back the text before"
        );

        // Edits not saved stay when the file changes under them.
        fs::write(&path, "changed again\n").unwrap();
        file.keep_edits();
        assert!(!file.changed_on_disk());
        assert_eq!(file.lines, ["one!", "two"]);
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
    fn a_large_file_opens_read_only_without_colours_and_a_binary_one_says_what_it_is() {
        let directory = tempfile::tempdir().unwrap();
        let mut picker = Picker::halfblocks();
        let small = directory.path().join("small.rs");
        std::fs::write(&small, "fn main() {}\n").unwrap();
        let file = OpenFile::load(small.clone(), &mut picker).unwrap();
        assert!(!file.read_only && file.is_text() && file.language == "Rust");
        assert!(
            file.highlighted[0].len() > 1,
            "a small file has its colours"
        );
        assert!(!is_huge_text(&small));

        // One byte over the limit: read, selected and copied, never changed nor saved.
        let large = directory.path().join("large.rs");
        let line = "fn main() { let answer = 42; } // filler\n";
        let text = line.repeat(LARGE_FILE as usize / line.len() + 1);
        std::fs::write(&large, &text).unwrap();
        let mut file = OpenFile::load(large.clone(), &mut picker).unwrap();
        assert!(file.read_only && !file.is_text() && file.shows_text());
        assert_eq!(file.language, "Read only");
        assert_eq!(file.highlighted[0].len(), 1, "no colours");
        file.insert('x');
        file.insert_str("pasted");
        file.backspace();
        file.delete();
        file.select_all();
        assert!(!file.delete_selection());
        assert!(!file.dirty && !file.undo());
        assert_eq!(file.selected_text().unwrap().len(), text.len() - 1);
        file.save().unwrap();
        assert_eq!(std::fs::read_to_string(&large).unwrap().len(), text.len());

        // The size is asked from the disk, without reading the file: a sparse one is enough.
        let huge = directory.path().join("huge.log");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(HUGE_FILE + 1)
            .unwrap();
        assert!(is_huge_text(&huge));
        let video = directory.path().join("huge.mp4");
        std::fs::File::create(&video)
            .unwrap()
            .set_len(HUGE_FILE + 1)
            .unwrap();
        assert!(!is_huge_text(&video), "a video has its own reader");
        let over = directory.path().join("over.log");
        std::fs::File::create(&over)
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        let error = OpenFile::load(over, &mut picker).err().unwrap().to_string();
        assert_eq!(error, "over.log is larger than 512 MB: too large to open");

        let pdf = directory.path().join("huge.pdf");
        std::fs::File::create(&pdf)
            .unwrap()
            .set_len(MOST_PDF + 1)
            .unwrap();
        let error = OpenFile::load(pdf, &mut picker).err().unwrap().to_string();
        assert_eq!(error, "huge.pdf is larger than 256 MB: too large to open");

        let binary = directory.path().join("data.bin");
        std::fs::write(&binary, [b'a', 0, 0xff, b'\n']).unwrap();
        let error = OpenFile::load(binary, &mut picker)
            .err()
            .unwrap()
            .to_string();
        assert_eq!(
            error,
            "data.bin is a binary file: there is no text in it to show"
        );
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

    #[test]
    fn a_text_page_of_a_pdf_is_read_selected_and_never_saved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        let text = "BT /F1 10 Tf 5 80 Td (Hello PDF) Tj ET";
        let bytes = pdf::sample_pages(&[text, pdf::SQUARE]);
        std::fs::write(&path, &bytes).unwrap();
        let mut picker = Picker::halfblocks();
        let mut file = OpenFile::load(path.clone(), &mut picker).unwrap();
        assert!(file.image.is_none() && file.shows_text() && !file.is_text());
        assert_eq!(file.lines, ["Hello PDF"]);
        file.anchor = Some((0, 0));
        file.move_cursor_to(0, 5);
        assert_eq!(file.selected_text().as_deref(), Some("Hello"));
        file.dirty = true;
        file.save().unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "the PDF stays as it was"
        );

        // The next page draws a square: a picture, and no text left behind.
        file.show_page(1, &mut picker);
        assert!(file.image.is_some() && file.lines.is_empty() && !file.shows_text());
        file.show_page(0, &mut picker);
        assert_eq!(file.lines, ["Hello PDF"]);
        assert_eq!(
            (file.cursor_row, file.cursor_column, file.anchor),
            (0, 0, None)
        );
    }
}
