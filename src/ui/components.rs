use std::time::Duration;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::animation::{self, Edge, SWEEP_CYCLE};

pub(crate) const ACTIVE: Color = Color::Cyan;
pub(crate) const TEXT: Color = Color::Reset;
pub(crate) const MUTED: Color = Color::DarkGray;
pub(crate) const BORDER: Color = Color::DarkGray;
/// A search match: the terminal's yellow behind its black, readable in light and dark themes.
pub(crate) const MATCH: Style = Style::new().fg(Color::Black).bg(Color::Yellow);
/// The match the search is on.
pub(crate) const CURRENT_MATCH: Style = Style::new().fg(Color::Black).bg(ACTIVE);

/// A shortcut as this system types it (`UX.md` §1): on macOS the function keys need `fn` and
/// the right terminal is Ctrl+Option+B, because Ctrl+Shift+B arrives as Ctrl+B (E11); on Linux
/// it is Ctrl+Alt+B for the same reason (E18).
pub(crate) fn key(label: &'static str) -> &'static str {
    key_for(std::env::consts::OS, label)
}

pub(crate) fn key_for(os: &str, label: &'static str) -> &'static str {
    if os == "linux" && label == "Ctrl+Shift+B" {
        return "Ctrl+Alt+B";
    }
    if os != "macos" {
        return label;
    }
    match label {
        "F1" => "fn+F1",
        "F2" => "fn+F2",
        "F5" => "fn+F5",
        "Ctrl+Shift+B" => "Ctrl+Option+B",
        "PageUp" => "fn+Up",
        "PageDown" => "fn+Down",
        other => other,
    }
}

/// `spans` without their first `columns` columns, for the horizontal scroll; a wide character
/// cut in two leaves a space.
pub(crate) fn skip_columns(spans: Vec<Span<'static>>, columns: usize) -> Vec<Span<'static>> {
    let mut skipped = 0;
    let mut kept = Vec::new();
    for span in spans {
        if skipped >= columns {
            kept.push(span);
            continue;
        }
        let mut text = String::new();
        for character in span.content.chars() {
            if skipped >= columns {
                text.push(character);
                continue;
            }
            skipped += Span::raw(character.to_string()).width();
            if skipped > columns {
                text.push(' ');
            }
        }
        if !text.is_empty() {
            kept.push(Span::styled(text, span.style));
        }
    }
    kept
}

/// `spans` with the columns `from..to` in reverse video, for a selection; a selection that
/// goes on past the text (the line end) gets spaces.
pub(crate) fn select_columns(
    spans: Vec<Span<'static>>,
    from: usize,
    to: usize,
) -> Vec<Span<'static>> {
    let (mut selected, column) = restyle_columns(&spans, from, to, |style| {
        style.add_modifier(Modifier::REVERSED)
    });
    if column < to {
        let start = column.max(from);
        push_span(
            &mut selected,
            &" ".repeat(to - start),
            Style::default().add_modifier(Modifier::REVERSED),
        );
    }
    selected
}

/// `spans` with the columns `from..to` in `style`, for a search match.
pub(crate) fn mark_columns(
    spans: Vec<Span<'static>>,
    from: usize,
    to: usize,
    style: Style,
) -> Vec<Span<'static>> {
    restyle_columns(&spans, from, to, |own| own.patch(style)).0
}

/// `spans` with `change` on the style of the columns `from..to`, and the width of the text.
fn restyle_columns(
    spans: &[Span<'static>],
    from: usize,
    to: usize,
    change: impl Fn(Style) -> Style,
) -> (Vec<Span<'static>>, usize) {
    let mut restyled = Vec::<Span<'static>>::new();
    let mut column = 0;
    for span in spans {
        for character in span.content.chars() {
            let style = if (from..to).contains(&column) {
                change(span.style)
            } else {
                span.style
            };
            push_span(&mut restyled, character.encode_utf8(&mut [0; 4]), style);
            column += Span::raw(character.to_string()).width();
        }
    }
    (restyled, column)
}

fn push_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    match spans.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push_str(text),
        _ => spans.push(Span::styled(text.to_owned(), style)),
    }
}

pub(crate) fn panel_block(title: String, focused: bool) -> Block<'static> {
    Block::default()
        .title(Line::from(title))
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(if focused { ACTIVE } else { BORDER }))
        .style(Style::default().fg(TEXT))
}

pub(crate) fn render_handle(frame: &mut Frame, area: Rect, vertical: bool) {
    let symbol = if vertical { "│" } else { "─" };
    let lines = (0..area.width.max(area.height))
        .map(|_| Line::from(symbol))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(BORDER)),
        area,
    );
}

pub(crate) fn centered(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height_percent) / 2),
        Constraint::Percentage(height_percent),
        Constraint::Percentage((100 - height_percent) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width_percent) / 2),
        Constraint::Percentage(width_percent),
        Constraint::Percentage((100 - width_percent) / 2),
    ])
    .split(vertical[1])[1]
}

pub(crate) fn scale_length(length: u16, progress: f32) -> u16 {
    ((length as f32 * progress.clamp(0.0, 1.0)).round() as u16).min(length)
}

/// The rows of a dialog that have unfolded from its middle row: the top and bottom border
/// first, then the rest.
pub(crate) fn unfold(area: Rect, progress: f32) -> Rect {
    if progress <= 0.0 {
        return Rect::new(area.x, area.y + area.height / 2, area.width, 0);
    }
    let height = scale_length(area.height, progress).max(area.height.min(2));
    Rect::new(
        area.x,
        area.y + (area.height - height) / 2,
        area.width,
        height,
    )
}

/// Draws a dialog at its final size and shows only the rows that have unfolded, with the
/// border on their edges, so its text never reflows while it opens or closes. Everything
/// behind it is dimmed while it is on screen.
pub(crate) fn render_dialog(
    frame: &mut Frame,
    area: Rect,
    progress: f32,
    title: String,
    draw: impl FnOnce(&mut Frame, Rect),
) {
    let visible = unfold(area, progress);
    if visible.is_empty() || area.width < 2 {
        return;
    }
    let screen = frame.area();
    frame
        .buffer_mut()
        .set_style(screen, Style::default().add_modifier(Modifier::DIM));
    let behind = (visible != area).then(|| frame.buffer_mut().clone());
    frame.render_widget(Clear, area);
    let block = panel_block(title, true);
    let inner = block.inner(area);
    frame.render_widget(block.clone(), area);
    draw(frame, inner);
    if let Some(behind) = behind {
        let buffer = frame.buffer_mut();
        for position in area
            .positions()
            .filter(|&position| !visible.contains(position))
        {
            buffer[position] = behind[position].clone();
        }
        // No style: the content keeps its colours under the new edges.
        frame.render_widget(block.style(Style::default()), visible);
    }
}

/// Shows the previous frame wherever the new content of `area` has not arrived yet. It comes
/// in from `edge` (line by line from the top, column by column from a side) and never moves,
/// so there is no blank area between the old and the new content.
pub(crate) fn sweep(buffer: &mut Buffer, previous: &Buffer, area: Rect, progress: f32, edge: Edge) {
    if progress >= 1.0 || previous.area != buffer.area {
        return;
    }
    let area = area.intersection(buffer.area);
    let rows = scale_length(area.height, progress);
    let columns = scale_length(area.width, progress);
    for position in area.positions() {
        let arrived = match edge {
            Edge::Top => position.y < area.y + rows,
            Edge::Left => position.x < area.x + columns,
            Edge::Right => position.x >= area.right() - columns,
        };
        if !arrived {
            buffer[position] = previous[position].clone();
        }
    }
}

/// One row of `width` cells: `━` in ACTIVE from `start` to `end` (in cells, to half a cell)
/// over a `─` track in BORDER.
pub(crate) fn bar(width: u16, start: f32, end: f32) -> Line<'static> {
    let (start, end) = ((start * 2.0).round() as i32, (end * 2.0).round() as i32);
    let filled = |half: i32| (start..end).contains(&half);
    let cells = (0..i32::from(width))
        .map(|cell| {
            let symbol = match (filled(cell * 2), filled(cell * 2 + 1)) {
                (true, true) => "━",
                (true, false) => "╾",
                (false, true) => "╼",
                (false, false) => return Span::styled("─", Style::default().fg(BORDER)),
            };
            Span::styled(symbol, Style::default().fg(ACTIVE))
        })
        .collect::<Vec<_>>();
    Line::from(cells)
}

/// A bar for work of unknown length: a segment sweeps across it once every `SWEEP_CYCLE`.
pub(crate) fn sweep_bar(width: u16, elapsed: Duration) -> Line<'static> {
    let length = f32::from(width);
    let segment = length * 0.3;
    let start = -segment + (length + segment) * animation::cycle(elapsed, SWEEP_CYCLE);
    bar(width, start, start + segment)
}

/// The spinner of a running operation in ACTIVE, then what is running.
pub(crate) fn busy(spinner: &'static str, text: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(spinner, Style::default().fg(ACTIVE)),
        Span::raw(format!(" {text}")),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, layout::Position};

    use super::*;

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn dialogs_unfold_from_their_middle_row() {
        let area = Rect::new(10, 8, 40, 20);
        assert!(unfold(area, 0.0).is_empty());
        assert_eq!(unfold(area, 0.01), Rect::new(10, 17, 40, 2));
        assert_eq!(unfold(area, 0.5), Rect::new(10, 13, 40, 10));
        assert_eq!(unfold(area, 1.0), area);
        assert_eq!(scale_length(30, 2.0), 30);
        assert_eq!(scale_length(30, -1.0), 0);
    }

    #[test]
    fn a_half_open_dialog_keeps_its_text_in_place_over_the_dimmed_screen() {
        let lines = (0..10)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>();
        let draw = |progress: f32| {
            let mut terminal = Terminal::new(TestBackend::new(30, 14)).unwrap();
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        Paragraph::new(vec![Line::from("behind ".repeat(5)); 14]),
                        frame.area(),
                    );
                    render_dialog(
                        frame,
                        Rect::new(2, 1, 26, 12),
                        progress,
                        " Dialog ".to_string(),
                        |frame, inner| frame.render_widget(Paragraph::new(lines.join("\n")), inner),
                    );
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let open = draw(1.0);
        let half = draw(0.5);

        // The middle rows are the same as when the dialog is open: nothing reflowed.
        for y in 6..8 {
            assert_eq!(row(&half, y), row(&open, y));
        }
        assert!(row(&half, 4).contains("╭ Dialog"));
        assert!(row(&half, 9).contains("╰"));
        assert!(row(&half, 2).starts_with("behind"));
        assert!(half[(0, 2)].modifier.contains(Modifier::DIM));
        assert!(!half[(5, 6)].modifier.contains(Modifier::DIM));
        assert!(row(&open, 2).contains("line 0"));
    }

    #[test]
    fn new_content_arrives_from_its_edge_over_the_previous_frame() {
        let area = Rect::new(0, 0, 10, 4);
        let previous = Buffer::with_lines(["oooooooooo"; 4]);
        let fresh = Buffer::with_lines(["nnnnnnnnnn"; 4]);
        let swept = |progress: f32, edge: Edge| {
            let mut buffer = fresh.clone();
            sweep(&mut buffer, &previous, area, progress, edge);
            buffer
        };

        let top = swept(0.5, Edge::Top);
        assert_eq!(
            (row(&top, 1), row(&top, 2)),
            ("nnnnnnnnnn".into(), "oooooooooo".into())
        );
        assert_eq!(row(&swept(0.3, Edge::Left), 0), "nnnooooooo");
        assert_eq!(row(&swept(0.3, Edge::Right), 3), "ooooooonnn");
        assert_eq!(swept(1.0, Edge::Left), fresh);
        assert_eq!(swept(0.0, Edge::Right), previous);

        // A frame of another size (the terminal was resized) is never mixed in.
        let mut buffer = fresh.clone();
        sweep(
            &mut buffer,
            &Buffer::empty(Rect::new(0, 0, 3, 3)),
            area,
            0.5,
            Edge::Top,
        );
        assert_eq!(buffer, fresh);
        assert_eq!(buffer[Position::new(9, 3)].symbol(), "n");
    }

    #[test]
    fn bars_fill_to_half_a_cell() {
        assert_eq!(text(&bar(6, 0.0, 6.0)), "━━━━━━");
        assert_eq!(text(&bar(6, 0.0, 0.0)), "──────");
        assert_eq!(text(&bar(6, 0.0, 2.5)), "━━╾───");
        assert_eq!(text(&bar(6, 1.5, 4.0)), "─╼━━──");
        assert_eq!(bar(6, 0.0, 3.0).spans[0].style.fg, Some(ACTIVE));
        assert_eq!(bar(6, 0.0, 3.0).spans[5].style.fg, Some(BORDER));

        // The indeterminate bar starts and ends outside the track and crosses it in between.
        assert_eq!(text(&sweep_bar(10, Duration::ZERO)), "──────────");
        assert!(text(&sweep_bar(10, SWEEP_CYCLE / 2)).contains("━━"));
        assert_eq!(text(&busy("⠋", "Working...")), "⠋ Working...");
    }

    #[test]
    fn selections_reverse_their_columns_and_the_line_end() {
        let spans = vec![
            Span::styled("fn ", Style::default().fg(Color::Magenta)),
            Span::raw("main"),
        ];
        let selected = select_columns(spans.clone(), 1, 5);
        let reversed = selected
            .iter()
            .filter(|span| span.style.add_modifier.contains(Modifier::REVERSED))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(reversed, "n ma");
        assert_eq!(
            selected
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "fn main"
        );
        // Past the end of the text the selected line end is a space.
        let past = select_columns(spans, 5, 8);
        let text = past
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "fn main ");
        assert!(
            past.last()
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }
}
