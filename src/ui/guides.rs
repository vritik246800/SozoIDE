//! What the editor adds over its text (TODO 2.3.4): the pair of the bracket at the cursor, a
//! guide down each level of indentation, and the spaces at the end of a selected line.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{ACTIVE, BORDER};
use crate::editor::OpenFile;

/// Draws over the text of `file` already in `buffer`: `text` is where its first column is, from
/// display row `scroll` and column `scroll_x`.
pub(super) fn decorate(
    buffer: &mut Buffer,
    file: &OpenFile,
    text: Rect,
    scroll: usize,
    scroll_x: usize,
) {
    let width = file.indent.width.max(1);
    let selection = file.selection();
    for y in 0..text.height {
        let display = scroll + usize::from(y);
        let Some(row) = file.display_source(display) else {
            continue;
        };
        if file.display_offset(display) > 0 {
            continue;
        }
        let line = &file.lines[row];
        let leading = line.chars().take_while(|c| *c == ' ').count();
        // A guide at the start of each level, where the line has only spaces.
        for column in (width..leading).step_by(width) {
            put(
                buffer,
                text,
                y,
                column,
                scroll_x,
                "│",
                Style::default().fg(BORDER),
            );
        }
        // The spaces at the end of a selected line show as `·`.
        if let Some(((start, _), (end, _))) = selection
            && (start..=end).contains(&row)
        {
            let chars: Vec<char> = line.chars().collect();
            let trailing = chars.iter().rev().take_while(|c| c.is_whitespace()).count();
            let first = chars.len() - trailing;
            for column in first..chars.len() {
                put(
                    buffer,
                    text,
                    y,
                    column,
                    scroll_x,
                    "·",
                    Style::default().fg(BORDER),
                );
            }
        }
    }
    if let Some((from, to)) = file.matching_bracket() {
        for (row, column) in [from, to] {
            let display = file.display_row(row);
            if display < scroll || display >= scroll + usize::from(text.height) || file.wrap {
                continue;
            }
            let x = file.lines[row]
                .chars()
                .take(column)
                .map(crate::ui::char_width)
                .sum::<usize>();
            let y = (display - scroll) as u16;
            if let Some(cell) = cell(buffer, text, y, x, scroll_x) {
                cell.set_style(
                    Style::default()
                        .fg(ACTIVE)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                );
            }
        }
    }
}

fn cell(
    buffer: &mut Buffer,
    text: Rect,
    y: u16,
    column: usize,
    scroll_x: usize,
) -> Option<&mut ratatui::buffer::Cell> {
    let x = column.checked_sub(scroll_x)?;
    let x = text.x.checked_add(u16::try_from(x).ok()?)?;
    (x < text.right()).then_some(())?;
    buffer.cell_mut((x, text.y + y))
}

fn put(
    buffer: &mut Buffer,
    text: Rect,
    y: u16,
    column: usize,
    scroll_x: usize,
    symbol: &str,
    style: Style,
) {
    if let Some(cell) = cell(buffer, text, y, column, scroll_x)
        && cell.symbol() == " "
    {
        cell.set_symbol(symbol).set_style(style);
    }
}
