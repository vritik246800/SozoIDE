//! Drawing tests, one file per panel; what they share is here.

mod code;
mod dialogs;
mod domains;
mod editing;
mod editor;
mod explorer;
mod media;
mod motion;
mod workspace;

use std::{path::PathBuf, time::Instant};

use ratatui::{Terminal, backend::TestBackend};

use super::*;

/// The frame of `app` on a terminal of `width` by `height`, one line of text per row.
fn frame(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| super::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|row| {
            (0..width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lets every transition finish; the splash stays for 1.4 s.
fn click(app: &mut App, kind: crossterm::event::MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
}

/// The columns `area` covers on row `y`.
fn cut(buffer: &ratatui::buffer::Buffer, y: u16, area: Rect) -> String {
    (area.x..area.right())
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn settle(app: &mut App) {
    let now = std::time::Instant::now();
    app.tick_at(now);
    app.tick_at(now + std::time::Duration::from_secs(1));
}

fn row(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
