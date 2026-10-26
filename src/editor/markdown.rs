use std::path::Path;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

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
    source_start: usize,
    source_end: usize,
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
        let border = Style::default().fg(Color::DarkGray);
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
            let available = column.width.saturating_sub(display_width(&cell.text));
            let (left, right) = column.alignment.padding(available);
            spans.push(Span::styled("│", border));
            spans.push(Span::styled(" ".repeat(left + 1), style));
            spans.push(Span::styled(cell.text.clone(), style));
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
                let cell_width = display_width(&cell.text);
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
                    + visual_column_to_char(&cell.text, visual_column - aligned_start);
            }
            border = next_border;
        }
        row.cells
            .last()
            .map(|cell| cell.source_end)
            .unwrap_or_default()
    }
}

pub struct MarkdownDocument {
    tables: Vec<MarkdownTable>,
    code: Vec<bool>,
}

impl MarkdownDocument {
    pub fn parse(lines: &[String]) -> Self {
        let fences = scan_fences(lines);
        let mut tables = Vec::new();
        let mut index = 0;
        while index + 1 < lines.len() {
            if fences.code[index] || fences.code[index + 1] {
                index += 1;
                continue;
            }
            if let Some(table) = parse_table(lines, &fences.code, index) {
                index = table.end;
                tables.push(table);
            } else {
                index += 1;
            }
        }
        Self {
            tables,
            code: fences.code,
        }
    }

    /// A table shows its source while one of the `raw` rows (cursor, mouse) is inside it.
    pub fn rendered_line(&self, source_row: usize, raw: &[usize]) -> Option<Vec<Span<'static>>> {
        let table = self
            .tables
            .iter()
            .find(|table| table.contains(source_row))?;
        if raw.iter().any(|row| table.contains(*row)) {
            return None;
        }
        Some(table.rendered_line(source_row))
    }

    pub fn source_column(
        &self,
        source_row: usize,
        raw: &[usize],
        visual_column: usize,
    ) -> Option<usize> {
        let table = self
            .tables
            .iter()
            .find(|table| table.contains(source_row))?;
        if raw.iter().any(|row| table.contains(*row)) {
            return None;
        }
        Some(table.source_column(source_row, visual_column))
    }

    /// Prose lines: neither code nor a table.
    pub fn formats_inline(&self, row: usize) -> bool {
        !self.code.get(row).copied().unwrap_or(true)
            && !self.tables.iter().any(|table| table.contains(row))
    }
}

/// One drawn character: the source character it stands for and how it looks.
struct Glyph {
    character: char,
    source: usize,
    style: Style,
}

/// Inline Markdown drawn over the highlighted line: straight quotes become typographic ones,
/// `*italic*`, `**bold**` and `==highlight==` lose their markers. `None` when nothing changes.
pub fn render_inline(line: &str, highlighted: &[Span<'static>]) -> Option<Vec<Span<'static>>> {
    let glyphs = inline_glyphs(line, highlighted)?;
    let mut spans: Vec<Span<'static>> = Vec::new();
    for glyph in glyphs {
        match spans.last_mut() {
            Some(span) if span.style == glyph.style => span.content.to_mut().push(glyph.character),
            _ => spans.push(Span::styled(glyph.character.to_string(), glyph.style)),
        }
    }
    Some(spans)
}

/// The source column under `visual_column` of a line drawn by `render_inline`.
pub fn inline_source_column(line: &str, visual_column: usize) -> Option<usize> {
    let glyphs = inline_glyphs(line, &[])?;
    let mut width = 0;
    for glyph in &glyphs {
        let next = width + display_width(&glyph.character.to_string());
        if visual_column < next {
            return Some(glyph.source);
        }
        width = next;
    }
    Some(line.chars().count())
}

fn inline_glyphs(line: &str, highlighted: &[Span<'static>]) -> Option<Vec<Glyph>> {
    let characters = line.chars().collect::<Vec<_>>();
    let mut styles = highlighted
        .iter()
        .flat_map(|span| span.content.chars().map(move |_| span.style))
        .collect::<Vec<_>>();
    if styles.len() != characters.len() {
        styles = vec![Style::default(); characters.len()];
    }
    // Code spans keep every character as written.
    let mut protected = vec![false; characters.len()];
    for (start, end) in pairs(&characters, &protected, &['`'], false) {
        protected[start..=end].fill(true);
    }
    let mut hidden = vec![false; characters.len()];
    let mut replaced = characters.clone();
    let mut changed = false;
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
    changed.then(|| {
        (0..characters.len())
            .filter(|index| !hidden[*index])
            .map(|index| Glyph {
                character: replaced[index],
                source: index,
                style: styles[index],
            })
            .collect()
    })
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
                .map(|row| display_width(&row.cells[index].text))
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

fn parse_separator_cell(cell: &Cell) -> Option<CellAlignment> {
    let text = cell.text.as_str();
    let left = text.starts_with(':');
    let right = text.ends_with(':');
    let dashes = text.trim_matches(':');
    if dashes.chars().count() < 3 || !dashes.chars().all(|character| character == '-') {
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
        source_start,
        source_end: source_start + trimmed.chars().count(),
    }
}

struct FenceRegions {
    code: Vec<bool>,
    mermaid: Vec<bool>,
}

struct OpenFence {
    character: char,
    length: usize,
    mermaid: bool,
}

fn scan_fences(lines: &[String]) -> FenceRegions {
    let mut regions = FenceRegions {
        code: vec![false; lines.len()],
        mermaid: vec![false; lines.len()],
    };
    let mut open: Option<OpenFence> = None;
    for (index, line) in lines.iter().enumerate() {
        if let Some(fence) = &open {
            regions.code[index] = true;
            if is_fence_close(line, fence) {
                open = None;
            } else if fence.mermaid {
                regions.mermaid[index] = true;
            }
            continue;
        }
        if let Some((character, length, info)) = fence_open(line) {
            regions.code[index] = true;
            open = Some(OpenFence {
                character,
                length,
                mermaid: info
                    .split_whitespace()
                    .next()
                    .is_some_and(|language| language.eq_ignore_ascii_case("mermaid")),
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

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
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
            text(&document.rendered_line(0, &[3]).unwrap()),
            "│ Name │ Age │"
        );
        assert_eq!(
            text(&document.rendered_line(1, &[3]).unwrap()),
            "├──────┼─────┤"
        );
        assert_eq!(
            text(&document.rendered_line(2, &[3]).unwrap()),
            "│ José │   4 │"
        );
        assert!(document.rendered_line(2, &[2]).is_none());
        assert!(
            document.rendered_line(0, &[9, 1]).is_none(),
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
        assert!(document.rendered_line(2, &[5]).is_none());
        assert!(document.rendered_line(4, &[0]).is_some());
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
        let escaped = text(&document.rendered_line(0, &[6]).unwrap());
        assert!(escaped.contains("A\\|B"));
        assert!(escaped.contains("`x|y`"));
        assert_eq!(
            text(&document.rendered_line(6, &[0]).unwrap()),
            "│   x │  1  │"
        );

        let invalid = vec!["| A | B |".to_owned(), "| -- | --- |".to_owned()];
        assert!(MarkdownDocument::parse(&invalid).tables.is_empty());
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

        assert_eq!(document.source_column(2, &[3], 2), Some(2));
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

        let document = MarkdownDocument::parse(&["```".into(), "**x**".into(), "```".into()]);
        assert!(!document.formats_inline(1));
    }
}
