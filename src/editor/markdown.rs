use std::{cell::RefCell, collections::HashMap, path::Path};

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

const BORDER: Color = Color::DarkGray;

#[derive(Clone, Copy)]
enum CellAlignment {
    Left,
    Right,
    Center,
}

impl CellAlignment {
    fn padding(self, available: usize) -> (usize, usize) {
        match self {
            Self::Left => (0, available),
            Self::Right => (available, 0),
            Self::Center => (available / 2, available - available / 2),
        }
    }
}

struct Cell {
    text: String,
    /// The cell as drawn: inline Markdown (bold, italic, quotes…) applied.
    glyphs: Vec<Glyph>,
    source_start: usize,
    source_end: usize,
}

impl Cell {
    /// The width of the cell's text as a whole: an emoji joined by a ZWJ, or with a skin
    /// tone, is one picture, not the sum of its parts (TODO 5.3.3).
    fn width(&self) -> usize {
        display_width(
            &self
                .glyphs
                .iter()
                .map(|glyph| glyph.character)
                .collect::<String>(),
        )
    }
}

struct Column {
    width: usize,
    alignment: CellAlignment,
}

#[derive(Clone, Copy, PartialEq)]
enum RowKind {
    Header,
    Separator,
    Body,
}

struct TableRow {
    kind: RowKind,
    cells: Vec<Cell>,
}

struct MarkdownTable {
    start: usize,
    end: usize,
    rows: Vec<TableRow>,
    columns: Vec<Column>,
}

impl MarkdownTable {
    fn contains(&self, row: usize) -> bool {
        row >= self.start && row < self.end
    }

    fn rendered_line(&self, source_row: usize) -> Vec<Span<'static>> {
        let row = &self.rows[source_row - self.start];
        let border = Style::default().fg(BORDER);
        if row.kind == RowKind::Separator {
            let mut spans = vec![Span::styled("├", border)];
            for (index, column) in self.columns.iter().enumerate() {
                spans.push(Span::styled("─".repeat(column.width + 2), border));
                spans.push(Span::styled(
                    if index + 1 == self.columns.len() {
                        "┤"
                    } else {
                        "┼"
                    },
                    border,
                ));
            }
            return spans;
        }

        let style = if row.kind == RowKind::Header {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Reset)
        };
        let mut spans = Vec::new();
        for (index, cell) in row.cells.iter().enumerate() {
            let column = &self.columns[index];
            let available = column.width.saturating_sub(cell.width());
            let (left, right) = column.alignment.padding(available);
            spans.push(Span::styled("│", border));
            spans.push(Span::styled(" ".repeat(left + 1), style));
            spans.extend(glyph_spans(&cell.glyphs, style));
            spans.push(Span::styled(" ".repeat(right + 1), style));
        }
        spans.push(Span::styled("│", border));
        spans
    }

    fn source_column(&self, source_row: usize, visual_column: usize) -> usize {
        let row = &self.rows[source_row - self.start];
        let mut border = 0;
        for (index, column) in self.columns.iter().enumerate() {
            let cell = &row.cells[index];
            let next_border = border + column.width + 3;
            if visual_column <= next_border {
                if row.kind == RowKind::Separator {
                    let segment = column.width + 2;
                    let offset = visual_column.saturating_sub(border + 1).min(segment);
                    let length = cell.source_end.saturating_sub(cell.source_start);
                    return cell.source_start + offset * length / segment.max(1);
                }
                let content_start = border + 2;
                if visual_column < content_start {
                    return cell.source_start;
                }
                let cell_width = cell.width();
                let available = column.width.saturating_sub(cell_width);
                let (left, _) = column.alignment.padding(available);
                let aligned_start = content_start + left;
                if visual_column < aligned_start {
                    return cell.source_start;
                }
                if visual_column >= aligned_start + cell_width {
                    return cell.source_end;
                }
                return cell.source_start
                    + glyph_source(&cell.glyphs, visual_column - aligned_start)
                        .unwrap_or(cell.text.chars().count());
            }
            border = next_border;
        }
        row.cells
            .last()
            .map(|cell| cell.source_end)
            .unwrap_or_default()
    }
}

/// A closed fenced block, drawn in a frame or, for Mermaid, as a diagram.
struct Fence {
    start: usize,
    /// One past the closing fence.
    end: usize,
    language: String,
    /// The widest line of the block.
    width: usize,
    diagram: Option<Vec<String>>,
}

impl Fence {
    fn contains(&self, row: usize) -> bool {
        row >= self.start && row < self.end
    }

    /// Rows on screen: the longer of the source and the diagram, so that showing the source
    /// under the mouse never moves what is below the block.
    fn height(&self) -> usize {
        let diagram = self.diagram.as_ref().map_or(0, Vec::len);
        (self.end - self.start).max(diagram)
    }

    /// Columns between the frame's sides: the code with a space on each side, or the label.
    fn inner(&self) -> usize {
        (self.width + 2).max(display_width(&self.language) + 4)
    }

    fn framed_line(
        &self,
        row: usize,
        line: &str,
        highlighted: &[Span<'static>],
    ) -> Vec<Span<'static>> {
        let border = Style::default().fg(BORDER);
        let inner = self.inner();
        if row == self.start {
            let mut spans = vec![Span::styled("╭─", border)];
            let mut used = 1;
            if !self.language.is_empty() {
                spans.push(Span::styled(
                    format!(" {} ", self.language),
                    Style::default().fg(Color::Cyan),
                ));
                used += display_width(&self.language) + 2;
            }
            spans.push(Span::styled(
                format!("{}╮", "─".repeat(inner - used)),
                border,
            ));
            return spans;
        }
        if row + 1 == self.end {
            return vec![Span::styled(format!("╰{}╯", "─".repeat(inner)), border)];
        }
        let mut spans = vec![Span::styled("│ ", border)];
        if highlighted.is_empty() {
            spans.push(Span::raw(line.to_owned()));
        } else {
            spans.extend(highlighted.iter().cloned());
        }
        let padding = inner.saturating_sub(display_width(line) + 2);
        spans.push(Span::raw(" ".repeat(padding)));
        spans.push(Span::styled(" │", border));
        spans
    }
}

/// One row on screen: the source line it belongs to and, in a Mermaid block, the line of
/// the block it draws. A diagram taller than its source adds `filler` rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub source: usize,
    pub filler: bool,
    diagram: Option<(usize, usize)>,
    /// A row of the picture of a `![…](…)` line: which picture and which of its rows.
    image: Option<(usize, usize)>,
    /// Which row of a wrapped line this is, from 0 (TODO 2.1.8).
    part: usize,
}

impl Row {
    pub fn plain(source: usize) -> Self {
        Self {
            source,
            filler: false,
            diagram: None,
            image: None,
            part: 0,
        }
    }

    /// A diagram or a picture: never wrapped.
    pub fn is_diagram(&self) -> bool {
        self.diagram.is_some() || self.image.is_some()
    }

    /// The same source line, as its row `part` when it wraps.
    pub fn part(self, part: usize) -> Self {
        Self { part, ..self }
    }

    pub fn part_index(&self) -> usize {
        self.part
    }
}

pub struct MarkdownDocument {
    tables: Vec<MarkdownTable>,
    fences: Vec<Fence>,
    code: Vec<bool>,
    /// The pictures of the lines that are only `![alt](file)`: the line and its half blocks.
    images: Vec<(usize, Vec<Line<'static>>)>,
}

/// How wide a picture in a document is drawn, in cells, and how tall at most.
const IMAGE_WIDTH: u16 = 40;
const IMAGE_ROWS: u16 = 20;

/// The file of a line that is only `![alt](file)`; none for an address or anything else.
pub fn image_target(line: &str) -> Option<&str> {
    let line = line.trim();
    let rest = line.strip_prefix("![")?;
    let (_, rest) = rest.split_once("](")?;
    let target = rest.strip_suffix(')')?.split_whitespace().next()?;
    (!target.contains("://") && !target.starts_with("data:")).then_some(target)
}

/// The picture of `target` beside the document in `folder`, inside `root`, as half blocks;
/// kept by path and time, so that typing does not read it again.
fn image_lines(folder: &Path, root: &Path, target: &str) -> Option<Vec<Line<'static>>> {
    use std::sync::Mutex;
    type Kept =
        std::collections::HashMap<std::path::PathBuf, (std::time::SystemTime, Vec<Line<'static>>)>;
    static KEPT: Mutex<Option<Kept>> = Mutex::new(None);
    let root = root.canonicalize().ok()?;
    let path = folder.join(target).canonicalize().ok()?;
    if !path.starts_with(&root) || crate::media::kind(&path) != Some(crate::media::Kind::Picture) {
        return None;
    }
    let modified = std::fs::metadata(&path)
        .and_then(|file| file.modified())
        .ok()?;
    let mut kept = KEPT.lock().ok()?;
    let kept = kept.get_or_insert_with(Default::default);
    if let Some((time, lines)) = kept.get(&path)
        && *time == modified
    {
        return Some(lines.clone());
    }
    let picture = crate::media::small_picture(&path, 320)?;
    let rows = ((u32::from(IMAGE_WIDTH) * picture.height()) / picture.width().max(1) / 2)
        .clamp(1, u32::from(IMAGE_ROWS)) as u16;
    let lines = crate::media::half_blocks(&picture, IMAGE_WIDTH, rows);
    if kept.len() >= 32 {
        kept.clear();
    }
    kept.insert(path, (modified, lines.clone()));
    Some(lines)
}

impl MarkdownDocument {
    /// Whether row `row` is inside a code block, where nothing is Markdown.
    pub fn is_code(&self, row: usize) -> bool {
        self.code.get(row).copied().unwrap_or(true)
    }

    pub fn parse(lines: &[String]) -> Self {
        let regions = scan_fences(lines);
        let mut tables = Vec::new();
        let mut index = 0;
        while index + 1 < lines.len() {
            if regions.code[index] || regions.code[index + 1] {
                index += 1;
                continue;
            }
            if let Some(table) = parse_table(lines, &regions.code, index) {
                index = table.end;
                tables.push(table);
            } else {
                index += 1;
            }
        }
        let fences = regions
            .blocks
            .into_iter()
            .map(|(start, end, language)| {
                let body = &lines[start + 1..end - 1];
                let diagram = language
                    .eq_ignore_ascii_case("mermaid")
                    .then(|| render_diagram(body))
                    .flatten();
                Fence {
                    start,
                    end,
                    width: body
                        .iter()
                        .map(|line| display_width(line))
                        .max()
                        .unwrap_or(0),
                    language,
                    diagram,
                }
            })
            .collect();
        Self {
            tables,
            fences,
            code: regions.code,
            images: Vec::new(),
        }
    }

    /// The same, with the pictures of its `![…](…)` lines read from beside the document in
    /// `folder` (files inside `root` only).
    pub fn parse_in(lines: &[String], folder: &Path, root: &Path) -> Self {
        let mut document = Self::parse(lines);
        document.images = lines
            .iter()
            .enumerate()
            .filter(|(row, _)| !document.code.get(*row).copied().unwrap_or(true))
            .filter_map(|(row, line)| Some((row, image_lines(folder, root, image_target(line)?)?)))
            .collect();
        document
    }

    /// The rows on screen for a file of `count` lines.
    pub fn rows(&self, count: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(count);
        let mut diagrams = self
            .fences
            .iter()
            .enumerate()
            .filter(|(_, fence)| fence.diagram.is_some())
            .peekable();
        let mut source = 0;
        while source < count {
            if let Some(index) = self.images.iter().position(|(row, _)| *row == source) {
                rows.extend((0..self.images[index].1.len()).map(|line| Row {
                    source,
                    filler: line > 0,
                    diagram: None,
                    image: Some((index, line)),
                    part: 0,
                }));
                source += 1;
                continue;
            }
            let Some((index, fence)) = diagrams.next_if(|(_, fence)| fence.start == source) else {
                rows.push(Row::plain(source));
                source += 1;
                continue;
            };
            let lines = fence.end - fence.start;
            rows.extend((0..fence.height()).map(|line| Row {
                source: fence.start + line.min(lines - 1),
                filler: line >= lines,
                diagram: Some((index, line)),
                image: None,
                part: 0,
            }));
            source = fence.end;
        }
        rows
    }

    /// How `row` looks, `None` for its highlighted source. Tables and blocks show their source
    /// while one of the `raw` rows (cursor, mouse) is inside them; other lines while they are
    /// one of them. `width` is how far a rule reaches.
    pub fn render(
        &self,
        row: Row,
        raw: &[usize],
        width: usize,
        line: &str,
        highlighted: &[Span<'static>],
    ) -> Option<Vec<Span<'static>>> {
        let source = row.source;
        // A picture's rows; on its line the cursor shows the source, the rest stay empty.
        if let Some((index, line)) = row.image {
            if raw.contains(&source) {
                return row.filler.then(Vec::new);
            }
            return Some(
                self.images
                    .get(index)
                    .and_then(|(_, lines)| lines.get(line))
                    .map_or_else(Vec::new, |line| line.spans.clone()),
            );
        }
        if let Some(fence) = self.fence(source) {
            let editing = raw.iter().any(|row| fence.contains(*row));
            return match (row.diagram, editing) {
                (Some(_), true) => row.filler.then(Vec::new),
                (Some((_, line)), false) => Some(diagram_line(
                    fence
                        .diagram
                        .as_ref()
                        .and_then(|diagram| diagram.get(line))
                        .map_or("", String::as_str),
                )),
                (None, true) => None,
                (None, false) => Some(fence.framed_line(source, line, highlighted)),
            };
        }
        if self.code.get(source).copied().unwrap_or(true) {
            return None;
        }
        if let Some(table) = self.table(source) {
            if raw.iter().any(|row| table.contains(*row)) {
                return None;
            }
            return Some(table.rendered_line(source));
        }
        if raw.contains(&source) {
            return None;
        }
        if is_rule(line) {
            return Some(vec![Span::styled(
                "─".repeat(width),
                Style::default().fg(BORDER),
            )]);
        }
        render_inline(line, highlighted)
    }

    /// The source column under `visual_column` of a drawn row, `None` where the row shows
    /// its source as written.
    pub fn source_column(
        &self,
        source_row: usize,
        raw: &[usize],
        visual_column: usize,
        line: &str,
    ) -> Option<usize> {
        if let Some(fence) = self.fence(source_row) {
            if raw.iter().any(|row| fence.contains(*row)) {
                return None;
            }
            if fence.diagram.is_some() || source_row == fence.start || source_row + 1 == fence.end {
                return Some(0);
            }
            return Some(visual_column_to_char(line, visual_column.saturating_sub(2)));
        }
        if self.code.get(source_row).copied().unwrap_or(true) {
            return None;
        }
        if let Some(table) = self.table(source_row) {
            if raw.iter().any(|row| table.contains(*row)) {
                return None;
            }
            return Some(table.source_column(source_row, visual_column));
        }
        if raw.contains(&source_row) {
            return None;
        }
        if is_rule(line) {
            return Some(0);
        }
        inline_source_column(line, visual_column)
    }

    /// The widest drawn table, frame or diagram.
    pub fn width(&self) -> usize {
        let fences = self.fences.iter().map(|fence| {
            fence.diagram.as_ref().map_or(fence.inner() + 2, |diagram| {
                diagram
                    .iter()
                    .map(|line| display_width(line))
                    .max()
                    .unwrap_or(0)
            })
        });
        let tables = self.tables.iter().map(|table| {
            table
                .columns
                .iter()
                .map(|column| column.width + 3)
                .sum::<usize>()
                + 1
        });
        let images = self.images.iter().map(|_| usize::from(IMAGE_WIDTH));
        fences.chain(tables).chain(images).max().unwrap_or(0)
    }

    fn fence(&self, row: usize) -> Option<&Fence> {
        self.fences.iter().find(|fence| fence.contains(row))
    }

    fn table(&self, row: usize) -> Option<&MarkdownTable> {
        self.tables.iter().find(|table| table.contains(row))
    }
}

thread_local! {
    /// The diagrams drawn, by their Mermaid text.
    static DRAWN: RefCell<HashMap<String, Option<Vec<String>>>> = RefCell::default();
}

/// A Mermaid block drawn with box characters (`editor::mermaid`); `None` keeps the code frame.
/// Each key parses the Markdown again: a diagram whose text did not change is not drawn again.
fn render_diagram(body: &[String]) -> Option<Vec<String>> {
    let source = body.join("\n");
    DRAWN.with_borrow_mut(|drawn| {
        if let Some(diagram) = drawn.get(&source) {
            return diagram.clone();
        }
        // ponytail: forgets everything when full; an LRU if many diagrams are edited at once.
        if drawn.len() >= 64 {
            drawn.clear();
        }
        let diagram = super::mermaid::render(&source);
        drawn.insert(source, diagram.clone());
        diagram
    })
}

/// A diagram line: box drawing and arrows in the accent colour, labels in the text colour.
fn diagram_line(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for character in text.chars() {
        let style = if matches!(u32::from(character), 0x2500..=0x25FF) {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        push_character(&mut spans, character, style);
    }
    spans
}

/// `---`, `***` or `___` (three or more, spaces allowed) on a line of its own.
fn is_rule(line: &str) -> bool {
    let indent = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    let marks = line
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    indent <= 3
        && marks.len() >= 3
        && matches!(marks[0], '-' | '*' | '_')
        && marks.iter().all(|mark| *mark == marks[0])
}

fn push_character(spans: &mut Vec<Span<'static>>, character: char, style: Style) {
    match spans.last_mut() {
        Some(span) if span.style == style => span.content.to_mut().push(character),
        _ => spans.push(Span::styled(character.to_string(), style)),
    }
}

/// Glyphs drawn over `base`: their own colours and attributes win.
fn glyph_spans(glyphs: &[Glyph], base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for glyph in glyphs {
        push_character(&mut spans, glyph.character, base.patch(glyph.style));
    }
    spans
}

/// The source character under `visual_column`, `None` past the last glyph.
fn glyph_source(glyphs: &[Glyph], visual_column: usize) -> Option<usize> {
    let mut width = 0;
    for glyph in glyphs {
        width += display_width(&glyph.character.to_string());
        if visual_column < width {
            return Some(glyph.source);
        }
    }
    None
}

/// One drawn character: the source character it stands for and how it looks.
struct Glyph {
    character: char,
    source: usize,
    style: Style,
}

/// Inline Markdown drawn over the highlighted line: headings lose their `#` and take the
/// style of their level, quotes get a bar, straight quotes become typographic ones,
/// `*italic*`, `**bold**` and `==highlight==` lose their markers. `None` when nothing changes.
pub fn render_inline(line: &str, highlighted: &[Span<'static>]) -> Option<Vec<Span<'static>>> {
    let glyphs = inline_glyphs(line, highlighted, true)?;
    Some(glyph_spans(&glyphs, Style::default()))
}

/// The source column under `visual_column` of a line drawn by `render_inline`.
pub fn inline_source_column(line: &str, visual_column: usize) -> Option<usize> {
    let glyphs = inline_glyphs(line, &[], true)?;
    Some(glyph_source(&glyphs, visual_column).unwrap_or(line.chars().count()))
}

/// A table cell as drawn: inline Markdown, but no headings or quotes.
fn cell_glyphs(text: &str) -> Vec<Glyph> {
    inline_glyphs(text, &[], false).unwrap_or_else(|| {
        text.chars()
            .enumerate()
            .map(|(source, character)| Glyph {
                character,
                source,
                style: Style::default(),
            })
            .collect()
    })
}

/// What starts a line: a heading of some level or a quote of some depth, and where the
/// text after the markers begins.
enum Prefix {
    Heading(usize),
    Quote,
}

fn block_prefix(characters: &[char]) -> Option<(Prefix, usize)> {
    let indent = characters
        .iter()
        .take_while(|character| **character == ' ')
        .count();
    if indent > 3 {
        return None;
    }
    let hashes = characters[indent..]
        .iter()
        .take_while(|character| **character == '#')
        .count();
    if (1..=6).contains(&hashes)
        && characters
            .get(indent + hashes)
            .is_none_or(|next| *next == ' ')
    {
        return Some((
            Prefix::Heading(hashes),
            (indent + hashes + 1).min(characters.len()),
        ));
    }
    let mut index = indent;
    while characters.get(index) == Some(&'>') {
        index += 1;
        if characters.get(index) == Some(&' ') {
            index += 1;
        }
    }
    (index > indent).then_some((Prefix::Quote, index))
}

/// Terminals have one font size: a level shows its size by case, spacing, weight and colour.
fn heading_style(level: usize) -> Style {
    let (color, modifier) = match level {
        1 => (Color::Magenta, Modifier::BOLD | Modifier::UNDERLINED),
        2 => (Color::Cyan, Modifier::BOLD),
        3 => (Color::Green, Modifier::BOLD | Modifier::UNDERLINED),
        4 => (Color::Yellow, Modifier::BOLD),
        5 => (Color::Blue, Modifier::empty()),
        _ => (Color::Red, Modifier::ITALIC | Modifier::DIM),
    };
    Style::default().fg(color).add_modifier(modifier)
}

fn inline_glyphs(line: &str, highlighted: &[Span<'static>], blocks: bool) -> Option<Vec<Glyph>> {
    let characters = line.chars().collect::<Vec<_>>();
    let mut styles = highlighted
        .iter()
        .flat_map(|span| span.content.chars().map(move |_| span.style))
        .collect::<Vec<_>>();
    if styles.len() != characters.len() {
        styles = vec![Style::default(); characters.len()];
    }
    let mut protected = vec![false; characters.len()];
    let mut hidden = vec![false; characters.len()];
    let mut replaced = characters.clone();
    let mut changed = false;
    let mut level = 0;
    if let Some((prefix, body)) = blocks.then(|| block_prefix(&characters)).flatten() {
        changed = true;
        protected[..body].fill(true);
        match prefix {
            Prefix::Heading(heading) => {
                level = heading;
                hidden[..body].fill(true);
                styles[body..].fill(heading_style(heading));
            }
            Prefix::Quote => {
                for index in (0..body).filter(|index| characters[*index] == '>') {
                    replaced[index] = '▌';
                    styles[index] = Style::default().fg(Color::Blue);
                }
                for style in &mut styles[body..] {
                    *style = style.add_modifier(Modifier::ITALIC);
                }
            }
        }
    }
    // A task of a list: `- [ ]` and `- [x]` become a box, empty or ticked (TODO 5.1.9).
    if blocks && let Some((open, done)) = task_box(&characters) {
        changed = true;
        replaced[open] = if done { '☑' } else { '☐' };
        styles[open] = Style::default().fg(if done { Color::Green } else { Color::Yellow });
        hidden[open + 1..open + 3].fill(true);
        protected[..open + 3].fill(true);
        if done {
            for style in &mut styles[open + 3..] {
                *style = style.add_modifier(Modifier::CROSSED_OUT | Modifier::DIM);
            }
        }
    }
    // Code spans keep every character as written.
    for (start, end) in pairs(&characters, &protected, &['`'], false) {
        protected[start..=end].fill(true);
    }
    // `[text](target)` shows its text, underlined; `[^note]` its label, raised.
    for link in links(&characters, &protected) {
        changed = true;
        protected[link.start..link.end].fill(true);
        hidden[link.start] = true;
        hidden[link.close..link.end].fill(true);
        for style in &mut styles[link.start + 1..link.close] {
            *style = style.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
        }
    }
    for (start, end) in footnotes(&characters, &protected) {
        changed = true;
        protected[start..end].fill(true);
        hidden[start + 1] = true;
        for style in &mut styles[start..end] {
            *style = style.fg(Color::Cyan);
        }
    }
    // `[[target|text]]` shows its text, or its target, and nothing else (F40).
    for link in wiki_links(&characters, &protected) {
        changed = true;
        protected[link.start..link.end].fill(true);
        hidden[link.start..link.text].fill(true);
        hidden[link.close..link.end].fill(true);
        for style in &mut styles[link.text..link.close] {
            *style = style.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
        }
    }
    for (token, modifier) in [
        (&['*', '*'][..], Some(Modifier::BOLD)),
        (&['=', '='][..], None),
        (&['*'][..], Some(Modifier::ITALIC)),
    ] {
        for (start, end) in pairs(&characters, &protected, token, false) {
            changed = true;
            let length = token.len();
            for index in [start..start + length, end..end + length]
                .into_iter()
                .flatten()
            {
                hidden[index] = true;
                protected[index] = true;
            }
            for style in &mut styles[start + length..end] {
                *style = match modifier {
                    Some(modifier) => style.add_modifier(modifier),
                    None => style.bg(Color::Yellow).fg(Color::Black),
                };
            }
        }
    }
    for (quote, open, close) in [
        ('"', '\u{201C}', '\u{201D}'),
        ('\'', '\u{2018}', '\u{2019}'),
    ] {
        for (start, end) in pairs(&characters, &protected, &[quote], true) {
            changed = true;
            replaced[start] = open;
            replaced[end] = close;
        }
    }
    if !changed {
        return None;
    }
    let mut glyphs = Vec::new();
    for index in (0..characters.len()).filter(|index| !hidden[*index]) {
        let mut character = replaced[index];
        if level == 1 || level == 2 {
            // ponytail: first char of the upper case only (ß stays one glyph); fine for headings.
            character = character.to_uppercase().next().unwrap_or(character);
        }
        if level == 1 && !glyphs.is_empty() {
            glyphs.push(Glyph {
                character: ' ',
                source: index,
                style: styles[index],
            });
        }
        glyphs.push(Glyph {
            character,
            source: index,
            style: styles[index],
        });
    }
    Some(glyphs)
}

/// Where the box of a task starts (`[`) and whether it is ticked: `- [ ] `, `* [x] `, `1. [X] `.
fn task_box(characters: &[char]) -> Option<(usize, bool)> {
    let indent = characters.iter().take_while(|c| c.is_whitespace()).count();
    let rest = &characters[indent..];
    let marker = match rest {
        ['-' | '*' | '+', ' ', ..] => 2,
        _ => {
            let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
            match rest.get(digits..digits + 2) {
                Some(['.' | ')', ' ']) if digits > 0 => digits + 2,
                _ => return None,
            }
        }
    };
    let open = indent + marker;
    match characters.get(open..open + 4)? {
        ['[', mark @ (' ' | 'x' | 'X'), ']', ' '] => Some((open, *mark != ' ')),
        _ => None,
    }
}

/// The line with the task box at character `column` ticked or not, when there is one there.
pub fn toggle_task(line: &str, column: usize) -> Option<String> {
    let characters: Vec<char> = line.chars().collect();
    let (open, done) = task_box(&characters)?;
    if !(open..open + 3).contains(&column) {
        return None;
    }
    let mut changed = characters;
    changed[open + 1] = if done { ' ' } else { 'x' };
    Some(changed.into_iter().collect())
}

/// A `[text](target)` by character (not an image `![…]`, not `[[…]]`): `[`, `]` and one past `)`.
struct Link {
    start: usize,
    close: usize,
    end: usize,
}

fn links(characters: &[char], protected: &[bool]) -> Vec<Link> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        let start = index;
        index += 1;
        if characters[start] != '['
            || protected[start]
            || characters.get(start + 1) == Some(&'[')
            || characters.get(start + 1) == Some(&'^')
            || (start > 0 && matches!(characters[start - 1], '!' | '['))
        {
            continue;
        }
        let Some(close) = characters[start..]
            .iter()
            .position(|c| *c == ']')
            .map(|at| start + at)
        else {
            break;
        };
        if characters.get(close + 1) != Some(&'(') || close == start + 1 {
            continue;
        }
        let Some(end) = characters[close..]
            .iter()
            .position(|c| *c == ')')
            .map(|at| close + at + 1)
        else {
            break;
        };
        if characters[close + 2..end - 1]
            .iter()
            .any(|c| c.is_whitespace())
            || end - close <= 3
        {
            continue;
        }
        found.push(Link { start, close, end });
        index = end;
    }
    found
}

/// Each `[^label]` that is not its definition (`[^label]:` at the start): start, one past end.
fn footnotes(characters: &[char], protected: &[bool]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut index = 0;
    while index + 2 < characters.len() {
        if characters[index..index + 2] == ['[', '^']
            && !protected[index]
            && let Some(length) = characters[index + 2..].iter().position(|c| *c == ']')
            && length > 0
        {
            let end = index + 2 + length + 1;
            let definition = index == 0 && characters.get(end) == Some(&':');
            if !definition {
                found.push((index, end));
            }
            index = end;
            continue;
        }
        index += 1;
    }
    found
}

/// Where a click at character `column` of `line` points: the target of a `[text](target)`
/// (with its `#heading`), or the label of a `[^note]`.
pub enum Pointer {
    Target(String),
    Note(String),
}

pub fn pointer_at(line: &str, column: usize) -> Option<Pointer> {
    let characters: Vec<char> = line.chars().collect();
    let none = vec![false; characters.len()];
    if let Some(link) = links(&characters, &none)
        .into_iter()
        .find(|link| (link.start..link.end).contains(&column))
    {
        return Some(Pointer::Target(
            characters[link.close + 2..link.end - 1].iter().collect(),
        ));
    }
    footnotes(&characters, &none)
        .into_iter()
        .find(|(start, end)| (*start..*end).contains(&column))
        .map(|(start, end)| Pointer::Note(characters[start + 2..end - 1].iter().collect()))
}

/// The row of `heading` (its text, or its GitHub anchor such as `getting-started`) in `lines`.
pub fn heading_row(lines: &[String], heading: &str) -> Option<usize> {
    let anchor = |text: &str| -> String {
        text.trim()
            .to_lowercase()
            .chars()
            .filter_map(|c| match c {
                ' ' | '-' => Some('-'),
                c if c.is_alphanumeric() || c == '_' => Some(c),
                _ => None,
            })
            .collect()
    };
    let wanted = anchor(heading);
    headings(lines)
        .into_iter()
        .find(|(_, text, _)| anchor(text) == wanted)
        .map(|(_, _, row)| row)
}

/// The headings of a document for its table of contents: level, text and row.
pub fn headings(lines: &[String]) -> Vec<(usize, String, usize)> {
    let regions = scan_fences(lines);
    lines
        .iter()
        .enumerate()
        .filter(|(row, _)| !regions.code[*row])
        .filter_map(|(row, line)| {
            let level = line.chars().take_while(|c| *c == '#').count();
            let text = line.get(level..)?;
            ((1..=6).contains(&level) && text.starts_with(' '))
                .then(|| (level, text.trim().to_owned(), row))
        })
        .collect()
}

/// A `[[target|text]]` by character: where it starts, where its target and its text start,
/// where `]]` starts and where it ends.
struct WikiLink {
    start: usize,
    target: usize,
    text: usize,
    close: usize,
    end: usize,
}

fn wiki_links(characters: &[char], protected: &[bool]) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let mut index = 0;
    while index + 1 < characters.len() {
        let start = index;
        index += 1;
        if characters[start..start + 2] != ['[', '['] || protected[start] {
            continue;
        }
        let target = start + 2;
        let Some(length) = characters[target..]
            .windows(2)
            .position(|pair| pair == [']', ']'])
        else {
            continue;
        };
        let close = target + length;
        let inner = &characters[target..close];
        if inner.is_empty() || inner.contains(&'[') {
            continue;
        }
        let text = inner
            .iter()
            .position(|character| *character == '|')
            .map_or(target, |bar| target + bar + 1);
        index = close + 2;
        links.push(WikiLink {
            start,
            target,
            text,
            close,
            end: close + 2,
        });
    }
    links
}

/// The `#heading` of the `[[ ]]` at character `column` of `line`, if it has one.
pub fn wiki_heading_at(line: &str, column: usize) -> Option<String> {
    let characters = line.chars().collect::<Vec<_>>();
    let link = wiki_links(&characters, &vec![false; characters.len()])
        .into_iter()
        .find(|link| (link.start..link.end).contains(&column))?;
    let end = if link.text > link.target {
        link.text - 1
    } else {
        link.close
    };
    let target: String = characters[link.target..end].iter().collect();
    target
        .split_once('#')
        .map(|(_, heading)| heading.trim().to_owned())
        .filter(|heading| !heading.is_empty())
}

/// The target of the `[[ ]]` at character `column` of `line`, without `|text` or `#heading`.
pub fn wiki_link_at(line: &str, column: usize) -> Option<String> {
    let characters = line.chars().collect::<Vec<_>>();
    let link = wiki_links(&characters, &vec![false; characters.len()])
        .into_iter()
        .find(|link| (link.start..link.end).contains(&column))?;
    let end = if link.text > link.target {
        link.text - 1
    } else {
        link.close
    };
    let target = characters[link.target..end].iter().collect::<String>();
    let target = target.split('#').next().unwrap_or_default().trim();
    (!target.is_empty()).then(|| target.to_owned())
}

/// Whether `line` has a `[[ ]]`: the mouse over it keeps showing its text (F40).
pub fn has_wiki_link(line: &str) -> bool {
    let characters = line.chars().collect::<Vec<_>>();
    !wiki_links(&characters, &vec![false; characters.len()]).is_empty()
}

/// Opening and closing positions of `token`: an opener is followed by text, a closer comes
/// right after text and something sits between them. With `outside_words`, both also need a
/// non-alphanumeric neighbour on their outer side, so apostrophes (`don't`) stay as they are.
fn pairs(
    characters: &[char],
    protected: &[bool],
    token: &[char],
    outside_words: bool,
) -> Vec<(usize, usize)> {
    let length = token.len();
    let word = |index: Option<usize>| {
        index
            .and_then(|index| characters.get(index))
            .is_some_and(|character| character.is_alphanumeric())
    };
    let text = |index: Option<usize>| {
        index
            .and_then(|index| characters.get(index))
            .is_some_and(|character| !character.is_whitespace())
    };
    let mut found = Vec::new();
    let mut open: Option<usize> = None;
    let mut index = 0;
    while index + length <= characters.len() {
        let here = &characters[index..index + length];
        // A lone `*` next to another one belongs to a `**` that found no partner.
        let doubled = length == 1
            && token[0] == '*'
            && (index.checked_sub(1).map(|before| characters[before]) == Some('*')
                || characters.get(index + 1) == Some(&'*'));
        if here != token || protected[index..index + length].iter().any(|cell| *cell) || doubled {
            index += 1;
            continue;
        }
        let before = index.checked_sub(1);
        let after = index + length;
        if let Some(start) = open
            && index > start + length
            && text(before)
            && !(outside_words && word(Some(after)))
        {
            found.push((start, index));
            open = None;
        } else if text(Some(after)) && !(outside_words && word(before)) {
            open = Some(index);
        }
        index += length;
    }
    found
}

pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
        })
}

pub fn mermaid_lines(lines: &[String]) -> Vec<bool> {
    scan_fences(lines).mermaid
}

pub fn visual_column_to_char(text: &str, visual_column: usize) -> usize {
    let mut width = 0;
    for (index, character) in text.chars().enumerate() {
        let character_width = display_width(&character.to_string());
        if visual_column < width + character_width {
            return if character_width > 1 && visual_column > width {
                index + 1
            } else {
                index
            };
        }
        width += character_width;
    }
    text.chars().count()
}

fn display_width(text: &str) -> usize {
    Line::from(text).width()
}

fn parse_table(lines: &[String], code: &[bool], start: usize) -> Option<MarkdownTable> {
    let header = parse_cells(&lines[start])?;
    let separator = parse_cells(&lines[start + 1])?;
    if header.len() != separator.len() {
        return None;
    }
    let alignments = separator
        .iter()
        .map(parse_separator_cell)
        .collect::<Option<Vec<_>>>()?;
    let mut rows = vec![
        TableRow {
            kind: RowKind::Header,
            cells: header,
        },
        TableRow {
            kind: RowKind::Separator,
            cells: separator,
        },
    ];
    let mut end = start + 2;
    while end < lines.len() && !code[end] {
        let Some(cells) = parse_cells(&lines[end]) else {
            break;
        };
        if cells.len() != rows[0].cells.len() {
            break;
        }
        rows.push(TableRow {
            kind: RowKind::Body,
            cells,
        });
        end += 1;
    }

    let columns = alignments
        .into_iter()
        .enumerate()
        .map(|(index, alignment)| {
            let width = rows
                .iter()
                .filter(|row| row.kind != RowKind::Separator)
                .map(|row| row.cells[index].width())
                .max()
                .unwrap_or(0)
                .max(3);
            Column { width, alignment }
        })
        .collect();
    Some(MarkdownTable {
        start,
        end,
        rows,
        columns,
    })
}

/// One dash or more, as GitHub reads them, with an optional `:` on each side. The en and em
/// dashes that smart punctuation makes of `--` count as hyphens.
fn parse_separator_cell(cell: &Cell) -> Option<CellAlignment> {
    let text = cell.text.as_str();
    let (left, text) = text
        .strip_prefix(':')
        .map_or((false, text), |rest| (true, rest));
    let (right, dashes) = text
        .strip_suffix(':')
        .map_or((false, text), |rest| (true, rest));
    if dashes.is_empty()
        || !dashes
            .chars()
            .all(|character| matches!(character, '-' | '–' | '—'))
    {
        return None;
    }
    Some(match (left, right) {
        (true, true) => CellAlignment::Center,
        (false, true) => CellAlignment::Right,
        _ => CellAlignment::Left,
    })
}

fn parse_cells(line: &str) -> Option<Vec<Cell>> {
    let mut cells = Vec::new();
    let mut start_byte = 0;
    let mut start_character = 0;
    let mut escaped = false;
    let mut in_code = false;
    let mut found_pipe = false;
    for (character_index, (byte_index, character)) in line.char_indices().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '`' => in_code = !in_code,
            '|' if !in_code => {
                found_pipe = true;
                cells.push(make_cell(&line[start_byte..byte_index], start_character));
                start_byte = byte_index + character.len_utf8();
                start_character = character_index + 1;
            }
            _ => {}
        }
    }
    if !found_pipe {
        return None;
    }
    cells.push(make_cell(&line[start_byte..], start_character));
    if cells.first().is_some_and(|cell| cell.text.is_empty()) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|cell| cell.text.is_empty()) {
        cells.pop();
    }
    if cells.is_empty() { None } else { Some(cells) }
}

fn make_cell(text: &str, source_start: usize) -> Cell {
    let leading = text
        .chars()
        .take_while(|character| character.is_whitespace())
        .count();
    let trimmed = text.trim();
    let source_start = source_start + leading;
    Cell {
        text: trimmed.to_owned(),
        glyphs: cell_glyphs(trimmed),
        source_start,
        source_end: source_start + trimmed.chars().count(),
    }
}

struct FenceRegions {
    code: Vec<bool>,
    mermaid: Vec<bool>,
    /// Closed blocks: first row, one past the closing row and the language.
    blocks: Vec<(usize, usize, String)>,
}

struct OpenFence {
    start: usize,
    character: char,
    length: usize,
    language: String,
}

fn scan_fences(lines: &[String]) -> FenceRegions {
    let mut regions = FenceRegions {
        code: vec![false; lines.len()],
        mermaid: vec![false; lines.len()],
        blocks: Vec::new(),
    };
    let mut open: Option<OpenFence> = None;
    for (index, line) in lines.iter().enumerate() {
        if let Some(fence) = &open {
            regions.code[index] = true;
            if is_fence_close(line, fence) {
                regions
                    .blocks
                    .push((fence.start, index + 1, fence.language.clone()));
                open = None;
            } else if fence.language.eq_ignore_ascii_case("mermaid") {
                regions.mermaid[index] = true;
            }
            continue;
        }
        if let Some((character, length, info)) = fence_open(line) {
            regions.code[index] = true;
            open = Some(OpenFence {
                start: index,
                character,
                length,
                language: info.split_whitespace().next().unwrap_or("").to_owned(),
            });
        }
    }
    regions
}

fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let (rest, character, length) = fence_marker(line)?;
    Some((character, length, rest))
}

fn is_fence_close(line: &str, fence: &OpenFence) -> bool {
    let Some((rest, character, length)) = fence_marker(line) else {
        return false;
    };
    character == fence.character && length >= fence.length && rest.trim().is_empty()
}

fn fence_marker(line: &str) -> Option<(&str, char, usize)> {
    let spaces = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    if spaces > 3 {
        return None;
    }
    let rest = &line[spaces..];
    let character = rest.chars().next()?;
    if character != '`' && character != '~' {
        return None;
    }
    let length = rest
        .chars()
        .take_while(|marker| *marker == character)
        .count();
    if length < 3 {
        return None;
    }
    Some((&rest[length..], character, length))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diagram_is_drawn_again_only_when_its_text_changes() {
        let lines = ["```mermaid", "graph TD", "  Kept --> Drawn", "```"]
            .map(String::from)
            .to_vec();
        let source = "graph TD\n  Kept --> Drawn";
        DRAWN.with_borrow_mut(|drawn| drawn.remove(source));
        let first = MarkdownDocument::parse(&lines);
        assert!(DRAWN.with_borrow(|drawn| drawn.contains_key(source)));
        // What the memory holds is what is drawn.
        DRAWN.with_borrow_mut(|drawn| drawn.insert(source.into(), Some(vec!["memo".into()])));
        let again = MarkdownDocument::parse(&lines);
        assert!(first.fences[0].diagram.is_some());
        assert_eq!(again.fences[0].diagram, Some(vec!["memo".to_owned()]));
    }

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    fn rendered(
        document: &MarkdownDocument,
        lines: &[String],
        row: usize,
        raw: &[usize],
    ) -> Option<Vec<Span<'static>>> {
        document.render(Row::plain(row), raw, 80, &lines[row], &[])
    }

    #[test]
    fn formats_tables_without_changing_line_count_or_source() {
        let lines = vec![
            "| Name | Age |".to_owned(),
            "| :--- | ---: |".to_owned(),
            "| José | 4 |".to_owned(),
        ];
        let document = MarkdownDocument::parse(&lines);

        assert_eq!(
            text(&rendered(&document, &lines, 0, &[3]).unwrap()),
            "│ Name │ Age │"
        );
        assert_eq!(
            text(&rendered(&document, &lines, 1, &[3]).unwrap()),
            "├──────┼─────┤"
        );
        assert_eq!(
            text(&rendered(&document, &lines, 2, &[3]).unwrap()),
            "│ José │   4 │"
        );
        assert!(rendered(&document, &lines, 2, &[2]).is_none());
        assert!(
            rendered(&document, &lines, 0, &[9, 1]).is_none(),
            "the mouse over the table"
        );
        assert_eq!(lines[2], "| José | 4 |");
    }

    #[test]
    fn skips_code_and_tracks_mermaid_content() {
        let lines = vec![
            "```mermaid".to_owned(),
            "graph TD".to_owned(),
            "| not | a table |".to_owned(),
            "```".to_owned(),
            "| Real | Table |".to_owned(),
            "| --- | --- |".to_owned(),
        ];
        let document = MarkdownDocument::parse(&lines);
        let mermaid = mermaid_lines(&lines);

        assert!(document.tables.is_empty() || document.tables[0].start == 4);
        assert_eq!(mermaid, vec![false, true, true, false, false, false]);
        assert!(rendered(&document, &lines, 4, &[0]).is_some());
    }

    #[test]
    fn handles_multiple_tables_alignments_and_escaped_pipes() {
        let lines = vec![
            "| A\\|B | `x|y` |".to_owned(),
            "| :---: | --- |".to_owned(),
            "| left | right |".to_owned(),
            String::new(),
            "| A | B |".to_owned(),
            "| ---: | :---: |".to_owned(),
            "| x | 1 |".to_owned(),
        ];
        let document = MarkdownDocument::parse(&lines);

        assert_eq!(document.tables.len(), 2);
        let escaped = text(&rendered(&document, &lines, 0, &[6]).unwrap());
        assert!(escaped.contains("A\\|B"));
        assert!(escaped.contains("`x|y`"));
        assert_eq!(
            text(&rendered(&document, &lines, 6, &[0]).unwrap()),
            "│   x │  1  │"
        );
    }

    #[test]
    fn alignment_rows_take_one_dash_and_the_dashes_of_smart_punctuation() {
        let body = |separator: &str| {
            let lines = vec![
                "| Name | Age |".to_owned(),
                separator.to_owned(),
                "| Al | 4 |".to_owned(),
            ];
            let document = MarkdownDocument::parse(&lines);
            (!document.tables.is_empty())
                .then(|| text(&rendered(&document, &lines, 2, &[9]).unwrap()))
        };
        // One hyphen is enough, as on GitHub.
        assert_eq!(body("|:-:|-:|").as_deref(), Some("│  Al  │   4 │"));
        assert_eq!(body("| :--: | :--: |").as_deref(), Some("│  Al  │  4  │"));
        assert_eq!(body("| - | :- |").as_deref(), Some("│ Al   │ 4   │"));
        assert_eq!(body("| -- | --- |").as_deref(), Some("│ Al   │ 4   │"));
        // The dashes smart punctuation makes of "--" count as hyphens.
        assert_eq!(body("|:—:|:—:|").as_deref(), Some("│  Al  │  4  │"));
        assert_eq!(body("| :–: | —: |").as_deref(), Some("│  Al  │   4 │"));
        assert_eq!(body("| :———: | –––: |").as_deref(), Some("│  Al  │   4 │"));
        for invalid in [
            "| :: | --- |",
            "| : | --- |",
            "| ::-:: | --- |",
            "| - - | --- |",
            "| :-x | --- |",
            "| = | --- |",
        ] {
            assert_eq!(body(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn tilde_fences_require_a_long_enough_closing_marker() {
        let lines = vec![
            "~~~mermaid".to_owned(),
            "graph TD".to_owned(),
            "~~".to_owned(),
            "A-->B".to_owned(),
            "~~~~~".to_owned(),
        ];
        assert_eq!(mermaid_lines(&lines), vec![false, true, true, true, false]);
    }

    #[test]
    fn maps_formatted_and_raw_unicode_clicks_to_source_columns() {
        let lines = vec![
            "| Name | Age |".to_owned(),
            "| --- | ---: |".to_owned(),
            "| José | 4 |".to_owned(),
        ];
        let document = MarkdownDocument::parse(&lines);

        assert_eq!(document.source_column(2, &[3], 2, &lines[2]), Some(2));
        assert_eq!(visual_column_to_char("a界b", 1), 1);
        assert_eq!(visual_column_to_char("a界b", 2), 2);
        assert_eq!(visual_column_to_char("a界b", 3), 2);
        assert_eq!(visual_column_to_char("a界b", 4), 3);
    }

    #[test]
    fn draws_inline_markup_and_maps_clicks_back_to_the_source() {
        let line = "Say \"hi\", **bold** *it* ==mark== don't `**raw**`";
        let spans = render_inline(line, &[]).unwrap();
        assert_eq!(
            text(&spans),
            "Say \u{201C}hi\u{201D}, bold it mark don't `**raw**`"
        );
        let style_of = |word: &str| {
            spans
                .iter()
                .find(|span| span.content.contains(word))
                .unwrap()
                .style
        };
        assert!(style_of("bold").add_modifier.contains(Modifier::BOLD));
        assert!(style_of("it").add_modifier.contains(Modifier::ITALIC));
        assert_eq!(style_of("mark").bg, Some(Color::Yellow));
        assert!(!style_of("don't").add_modifier.contains(Modifier::BOLD));

        // "bold" starts at visual column 10 and at source column 12, after its `**`.
        assert_eq!(inline_source_column(line, 10), Some(12));
        assert_eq!(inline_source_column(line, 0), Some(0));
        assert!(render_inline("* item and 2 * 3", &[]).is_none());
        assert!(render_inline("plain text", &[]).is_none());

        let lines = ["```".to_owned(), "**x**".to_owned(), "```".to_owned()];
        let document = MarkdownDocument::parse(&lines);
        assert_eq!(
            text(&rendered(&document, &lines, 1, &[9]).unwrap()),
            "│ **x** │"
        );
    }

    #[test]
    fn headings_quotes_and_rules_are_drawn_and_map_clicks_back() {
        let heading = render_inline("# Big title", &[]).unwrap();
        assert_eq!(text(&heading), "B I G   T I T L E");
        assert_eq!(heading[0].style.fg, Some(Color::Magenta));
        assert!(heading[0].style.add_modifier.contains(Modifier::UNDERLINED));
        // "I" is the second letter: visual column 2, source column 3 after "# B".
        assert_eq!(inline_source_column("# Big title", 2), Some(3));
        let colours = (1..=6)
            .map(|level| {
                let line = format!("{} Title", "#".repeat(level));
                render_inline(&line, &[]).unwrap()[0].style.fg
            })
            .collect::<Vec<_>>();
        assert_eq!(
            colours,
            [
                Color::Magenta,
                Color::Cyan,
                Color::Green,
                Color::Yellow,
                Color::Blue,
                Color::Red
            ]
            .map(Some)
        );
        assert_eq!(text(&render_inline("## Sub", &[]).unwrap()), "SUB");
        assert_eq!(text(&render_inline("###### small", &[]).unwrap()), "small");
        assert!(render_inline("#hashtag", &[]).is_none());

        let quote = render_inline("> > quoted **bold**", &[]).unwrap();
        assert_eq!(text(&quote), "▌ ▌ quoted bold");
        assert!(
            quote
                .last()
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::ITALIC)
        );

        let lines = vec!["Above".to_owned(), "---".to_owned(), "* * *".to_owned()];
        let document = MarkdownDocument::parse(&lines);
        assert_eq!(
            text(&rendered(&document, &lines, 1, &[0]).unwrap()),
            "─".repeat(80)
        );
        assert!(rendered(&document, &lines, 2, &[0]).is_some());
        assert!(
            rendered(&document, &lines, 1, &[1]).is_none(),
            "the source under the mouse"
        );
        assert!(!is_rule("--"));
        assert!(!is_rule("| --- |"));
    }

    #[test]
    fn code_blocks_are_framed_until_the_cursor_or_mouse_is_inside() {
        let lines = vec![
            "```rust".to_owned(),
            "let x = 1;".to_owned(),
            "```".to_owned(),
        ];
        let document = MarkdownDocument::parse(&lines);
        let drawn = (0..3)
            .map(|row| text(&rendered(&document, &lines, row, &[9]).unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            drawn,
            ["╭─ rust ─────╮", "│ let x = 1; │", "╰────────────╯"]
        );
        assert!(rendered(&document, &lines, 0, &[1]).is_none());
        // A click after "│ " lands on the same character of the source.
        assert_eq!(document.source_column(1, &[9], 6, &lines[1]), Some(4));
    }

    #[test]
    fn mermaid_is_drawn_in_rows_of_its_own_and_shows_its_source_when_editing() {
        let lines = vec![
            "Intro".to_owned(),
            "```mermaid".to_owned(),
            "graph TD".to_owned(),
            "A[Key] --> B[App]".to_owned(),
            "```".to_owned(),
            "After".to_owned(),
        ];
        let document = MarkdownDocument::parse(&lines);
        let rows = document.rows(lines.len());
        // The diagram is taller than its four lines: it adds rows that belong to the block.
        assert!(rows.len() > lines.len());
        assert_eq!(rows.last().unwrap().source, 5);
        let diagram = rows
            .iter()
            .map(|row| {
                text(
                    &document
                        .render(*row, &[0], 80, &lines[row.source], &[])
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            diagram.contains("│ Key │") && diagram.contains("│ App │"),
            "{diagram}"
        );
        // Inside the block: the source, and empty rows where the diagram was taller.
        let editing = rows
            .iter()
            .filter(|row| row.source >= 1 && row.source <= 4)
            .map(|row| document.render(*row, &[3], 80, &lines[row.source], &[]))
            .collect::<Vec<_>>();
        assert!(editing[..4].iter().all(Option::is_none));
        assert!(
            editing[4..]
                .iter()
                .all(|line| line.as_ref().is_some_and(Vec::is_empty))
        );
        assert!(
            rows.iter()
                .filter(|row| row.filler)
                .all(|row| row.source == 4)
        );

        // A diagram that does not parse stays a code block.
        let broken = vec![
            "```mermaid".to_owned(),
            "nonsense".to_owned(),
            "```".to_owned(),
        ];
        let document = MarkdownDocument::parse(&broken);
        assert_eq!(document.rows(3).len(), 3);
        assert!(text(&rendered(&document, &broken, 0, &[9]).unwrap()).contains("mermaid"));
    }

    #[test]
    fn wiki_links_show_only_their_text_and_clicks_find_their_target() {
        let line = "See [[Docs/guide#setup|the guide]] and [[notes]].";
        let spans = render_inline(line, &[]).unwrap();
        assert_eq!(text(&spans), "See the guide and notes.");
        let link = spans
            .iter()
            .find(|span| span.content == "the guide")
            .unwrap();
        assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(link.style.fg, Some(Color::Blue));

        // Over the text drawn, and over the source of a row the cursor is on.
        let column = inline_source_column(line, 6).unwrap();
        assert_eq!(wiki_link_at(line, column).as_deref(), Some("Docs/guide"));
        let column = inline_source_column(line, 19).unwrap();
        assert_eq!(wiki_link_at(line, column).as_deref(), Some("notes"));
        assert_eq!(wiki_link_at(line, 5).as_deref(), Some("Docs/guide"));
        assert_eq!(wiki_link_at(line, 0), None);
        assert_eq!(wiki_link_at(line, 36), None);

        // A code span stays as written; broken and empty ones are text; a heading alone is
        // no document.
        assert_eq!(
            text(&render_inline("`[[a]]` **b**", &[]).unwrap()),
            "`[[a]]` b"
        );
        assert!(render_inline("[[open and [[a", &[]).is_none());
        assert!(render_inline("[[]]", &[]).is_none());
        assert_eq!(wiki_link_at("[[#top]]", 3), None);
        assert!(has_wiki_link("a [[b]]") && !has_wiki_link("a [b](c)"));
    }

    #[test]
    fn table_cells_draw_their_inline_markdown() {
        let lines = vec![
            "| Name | Note |".to_owned(),
            "| --- | --- |".to_owned(),
            "| **Ana** | *new* |".to_owned(),
        ];
        let document = MarkdownDocument::parse(&lines);
        let row = rendered(&document, &lines, 2, &[9]).unwrap();
        assert_eq!(text(&row), "│ Ana  │ new  │");
        let style_of = |word: &str| row.iter().find(|span| span.content == word).unwrap().style;
        assert!(style_of("Ana").add_modifier.contains(Modifier::BOLD));
        assert!(style_of("new").add_modifier.contains(Modifier::ITALIC));
        // "Ana" starts at visual column 2 and at source column 4, after "| **".
        assert_eq!(document.source_column(2, &[9], 2, &lines[2]), Some(4));
    }

    #[test]
    fn tasks_links_notes_headings_and_pictures_of_a_document() {
        // Tasks: a box, ticked or not, that a click on it changes.
        assert_eq!(
            task_box(&"- [ ] buy".chars().collect::<Vec<_>>()),
            Some((2, false))
        );
        assert_eq!(
            task_box(&"  12. [X] done".chars().collect::<Vec<_>>()),
            Some((6, true))
        );
        assert_eq!(task_box(&"-[ ] no".chars().collect::<Vec<_>>()), None);
        assert_eq!(task_box(&"- [y] no".chars().collect::<Vec<_>>()), None);
        assert_eq!(toggle_task("- [ ] buy", 3).as_deref(), Some("- [x] buy"));
        assert_eq!(toggle_task("- [x] buy", 2).as_deref(), Some("- [ ] buy"));
        assert_eq!(toggle_task("- [ ] buy", 7), None, "only the box ticks");
        let drawn = |line: &str| {
            render_inline(line, &[])
                .unwrap()
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        };
        assert_eq!(drawn("- [ ] buy milk"), "- ☐ buy milk");
        assert_eq!(drawn("- [x] done"), "- ☑ done");
        // Links show their text; notes their label; images and wiki links are not links.
        assert_eq!(
            drawn("see [the guide](docs/guide.md#setup) now"),
            "see the guide now"
        );
        assert_eq!(drawn("a fact[^1]."), "a fact[1].");
        assert!(
            matches!(pointer_at("see [the guide](docs/guide.md#setup)", 6), Some(Pointer::Target(target)) if target == "docs/guide.md#setup")
        );
        assert!(
            matches!(pointer_at("a fact[^src] b", 8), Some(Pointer::Note(label)) if label == "src")
        );
        assert!(pointer_at("![logo](a.png)", 3).is_none());
        assert!(
            pointer_at("[^1]: the note", 1).is_none(),
            "a definition points nowhere"
        );
        assert!(
            pointer_at("[a](b c)", 1).is_none(),
            "a space is not an address"
        );
        // Headings: by text or by anchor, never inside code.
        let lines: Vec<String> = [
            "# Title",
            "```",
            "# not a heading",
            "```",
            "## Getting Started!",
            "text",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(heading_row(&lines, "getting-started"), Some(4));
        assert_eq!(heading_row(&lines, "Getting Started!"), Some(4));
        assert_eq!(heading_row(&lines, "not a heading"), None);
        assert_eq!(
            headings(&lines),
            [
                (1, "Title".to_owned(), 0),
                (2, "Getting Started!".to_owned(), 4)
            ]
        );
        assert_eq!(
            wiki_heading_at("[[notes#todo|later]]", 3).as_deref(),
            Some("todo")
        );
        // Pictures: a line that is only an image of a file beside it, never above its folder.
        assert_eq!(
            image_target("![logo](assets/logo.png)"),
            Some("assets/logo.png")
        );
        assert_eq!(image_target("![x](https://e.com/a.png)"), None);
        assert_eq!(image_target("text ![x](a.png)"), None);
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("docs");
        std::fs::create_dir(&folder).unwrap();
        image::RgbaImage::from_pixel(40, 20, image::Rgba([0, 200, 0, 255]))
            .save(folder.join("dot.png"))
            .unwrap();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255]))
            .save(directory.path().join("outside.png"))
            .unwrap();
        let lines: Vec<String> = ["# Doc", "![dot](dot.png)", "![out](../outside.png)", "end"]
            .map(String::from)
            .to_vec();
        let document = MarkdownDocument::parse_in(&lines, &folder, &folder);
        let rows = document.rows(lines.len());
        let pictures = rows.iter().filter(|row| row.image.is_some()).count();
        assert_eq!(
            pictures, 10,
            "40 cells wide, 20 pixels tall in half blocks: 10 rows; the one outside is text"
        );
        assert!(
            rows.iter()
                .all(|row| row.image.is_none() || row.source == 1)
        );
        let first = rows.iter().find(|row| row.image.is_some()).unwrap();
        assert!(
            !document
                .render(*first, &[], 80, &lines[1], &[])
                .unwrap()
                .is_empty()
        );
        assert!(
            document.render(*first, &[1], 80, &lines[1], &[]).is_none(),
            "the cursor shows its source"
        );
    }
}
