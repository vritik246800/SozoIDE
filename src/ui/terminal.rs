//! A terminal panel: the shell of the tab on screen (two side by side when split), its tabs on
//! the right of the top border (F04, Q59, TODO 5.2.4), the text chosen with the mouse in reverse
//! (Q57), and the search of its history (`F3`, TODO 5.1.2).

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::{
    app::{Action, App, BOTTOM_TERMINALS, Focus, Regions},
    terminal::Terminal,
};

use super::{ACTIVE, MATCH, MUTED, TEXT, panel_block};

pub(super) fn render_terminal(
    frame: &mut Frame,
    app: &App,
    regions: &mut Regions,
    area: Rect,
    panel: Focus,
) {
    let bottom = panel == Focus::BottomTerminal;
    let (terminals, name, action) = if bottom {
        (
            &app.bottom_terminal,
            "Bottom terminal",
            Action::BottomTerminal,
        )
    } else {
        (&app.right_terminal, "Right terminal", Action::RightTerminal)
    };
    let focused = app.focus == panel;
    let terminal = terminals.current();
    // ` 1 x `, ` 2 vim x ` and ` + ` (three at most), and ` | ` to split.
    let count = terminals.tabs.len();
    let closable = count > 1;
    let labels = (0..count)
        .map(|index| {
            let label = match &terminals.renaming {
                Some(typed) if index == terminals.active => format!("{} {typed}_", index + 1),
                _ => terminals.label(index),
            };
            if closable {
                format!(" {label} x ")
            } else {
                format!(" {label} ")
            }
        })
        .collect::<Vec<_>>();
    let add = count < BOTTOM_TERMINALS;
    let tabs_width = labels
        .iter()
        .map(|label| Line::from(label.as_str()).width() as u16 + 1)
        .sum::<u16>()
        + if add { 4 } else { 0 }
        + 4;
    let scrollback = terminal.scrollback();
    let icon = app.icon("dev-terminal");
    let history = if scrollback > 0 {
        format!(" [history: {scrollback}]")
    } else {
        String::new()
    };
    // The title gives way to the tabs: without its key, then only the icon.
    let room = usize::from(area.width.saturating_sub(tabs_width + 3));
    let title = [
        format!(" {icon} {name} [{}]{history} ", app.keys.label(action)),
        format!(" {icon} {name}{history} "),
        format!(" {icon} "),
    ]
    .into_iter()
    .find(|title| Line::from(title.as_str()).width() <= room)
    .unwrap_or_default();
    let block = panel_block(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Split: the shell on screen on the left, the next one on the right.
    let shown: Vec<(&Terminal, Rect, bool)> = if terminals.split && count > 1 {
        let half = inner.width / 2;
        let other = (terminals.active + 1) % count;
        vec![
            (
                terminal,
                Rect {
                    width: half.saturating_sub(1),
                    ..inner
                },
                true,
            ),
            (
                &terminals.tabs[other],
                Rect {
                    x: inner.x + half,
                    width: inner.width - half,
                    ..inner
                },
                false,
            ),
        ]
    } else {
        vec![(terminal, inner, true)]
    };
    for (shell, place, on_screen) in &shown {
        if let Ok(parser) = shell.parser.read() {
            let cursor =
                Cursor::default().visibility(focused && *on_screen && shell.scrollback() == 0);
            frame.render_widget(PseudoTerminal::new(parser.screen()).cursor(cursor), *place);
        }
    }
    if shown.len() == 2 {
        let x = shown[1].1.x.saturating_sub(1);
        for y in inner.y..inner.bottom() {
            frame
                .buffer_mut()
                .set_string(x, y, "│", Style::default().fg(MUTED));
        }
    }
    if let Some(selection) = app
        .terminal_selection
        .filter(|chosen| chosen.panel == panel)
    {
        let (start, end) = selection.bounds();
        let buffer = frame.buffer_mut();
        for row in start.0..=end.0.min(inner.height.saturating_sub(1)) {
            let from = if row == start.0 { start.1 } else { 0 };
            let to = if row == end.0 { end.1 } else { inner.width };
            for column in from..=to.min(inner.width.saturating_sub(1)) {
                buffer[(inner.x + column, inner.y + row)]
                    .modifier
                    .toggle(Modifier::REVERSED);
            }
        }
    }
    if let Some(search) = &terminals.search {
        render_search(frame, search, shown[0].1);
    }
    if area.height == 0 || area.width < tabs_width + 4 {
        return;
    }
    let mut x = area.right() - 1 - tabs_width;
    let button = Style::default().fg(TEXT).add_modifier(Modifier::REVERSED);
    let current = Style::default()
        .fg(ACTIVE)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    for (index, label) in labels.iter().enumerate() {
        let width = Line::from(label.as_str()).width() as u16;
        let style = if index == terminals.active {
            current
        } else {
            button
        };
        frame.buffer_mut().set_string(x, area.y, label, style);
        let (tab, close) = if closable {
            (
                Rect::new(x, area.y, width - 2, 1),
                Rect::new(x + width - 2, area.y, 2, 1),
            )
        } else {
            (Rect::new(x, area.y, width, 1), Rect::default())
        };
        regions.terminal_tabs.push((panel, index, tab, close));
        x += width + 1;
    }
    if add {
        frame.buffer_mut().set_string(x, area.y, " + ", button);
        regions
            .terminal_add
            .push((panel, Rect::new(x, area.y, 3, 1)));
        x += 4;
    }
    let split = if terminals.split { current } else { button };
    frame.buffer_mut().set_string(x, area.y, " | ", split);
    regions
        .terminal_split
        .push((panel, Rect::new(x, area.y, 3, 1)));
}

/// The occurrences of the search marked on screen and its bar on the last row.
fn render_search(frame: &mut Frame, search: &crate::terminal::TerminalSearch, area: Rect) {
    if area.height == 0 || area.width < 10 {
        return;
    }
    let query: Vec<char> = search.query.to_lowercase().chars().collect();
    let buffer = frame.buffer_mut();
    if !query.is_empty() {
        for y in area.y..area.bottom() {
            let row: Vec<char> = (area.x..area.right())
                .map(|x| {
                    buffer[(x, y)]
                        .symbol()
                        .chars()
                        .next()
                        .unwrap_or(' ')
                        .to_lowercase()
                        .next()
                        .unwrap_or(' ')
                })
                .collect();
            let mut at = 0;
            while at + query.len() <= row.len() {
                if row[at..at + query.len()] == query[..] {
                    for column in at..at + query.len() {
                        buffer[(area.x + column as u16, y)].set_style(MATCH);
                    }
                    at += query.len();
                } else {
                    at += 1;
                }
            }
        }
    }
    let count = match (search.query.is_empty(), search.matches.len()) {
        (true, _) => String::new(),
        (false, 0) => "  no results".to_owned(),
        (false, total) => format!("  {}/{total}", search.index + 1),
    };
    let bar = format!(
        " Find: {}_{count}   Enter older  Shift+Enter newer  Esc ",
        search.query
    );
    let y = area.bottom() - 1;
    let line: String = bar.chars().take(usize::from(area.width)).collect();
    buffer.set_string(
        area.x,
        y,
        format!("{line:<width$}", width = usize::from(area.width)),
        Style::default().fg(TEXT).add_modifier(Modifier::REVERSED),
    );
}
