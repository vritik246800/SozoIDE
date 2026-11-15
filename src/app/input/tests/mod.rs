//! The tests of the keys and the mouse, by panel; what they share is here.

mod code;
mod dialogs;
mod editing;
mod editor;
mod explorer;
mod layout;
mod terminal;

use super::super::{Dependencies, EntryAction, Tile};
use super::*;
use std::path::PathBuf;

fn test_app() -> App {
    App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The app drawn in a window, for the tests that click what is on screen.
fn drawn(columns: u16, rows: u16) -> (App, ratatui::Terminal<ratatui::backend::TestBackend>) {
    let app = test_app();
    let terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(columns, rows)).unwrap();
    (app, terminal)
}

fn draw(app: &mut App, terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>) {
    app.tick();
    let (explorer, bottom) = (app.show_explorer, app.show_bottom_terminal);
    app.motion
        .settle_panels(explorer, bottom, app.show_right_terminal);
    terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
}

fn show_all(app: &mut App, terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>) {
    app.show_explorer = true;
    app.show_bottom_terminal = true;
    app.show_right_terminal = true;
    draw(app, terminal);
    draw(app, terminal);
}

fn drag_panel(app: &mut App, from: (u16, u16), to: (u16, u16)) {
    mouse(app, MouseEventKind::Down(MouseButton::Left), from.0, from.1);
    mouse(app, MouseEventKind::Drag(MouseButton::Left), to.0, to.1);
    assert!(app.panel_drag.is_some_and(|drag| drag.zone.is_some()));
    mouse(app, MouseEventKind::Up(MouseButton::Left), to.0, to.1);
    assert!(app.panel_drag.is_none());
}

/// The program in the terminal of `panel` reads what is written to it, in a file.
fn program_in(app: &mut App, panel: Focus, output: &[u8]) {
    let terminal = app.terminals(panel).current_mut();
    terminal.parser.write().unwrap().process(output);
}

fn tools() -> Vec<crate::update::deps::Component> {
    crate::update::deps::own_tools(crate::update::deps::components(
        "linux",
        |tool| tool == "apt-get",
        &[],
    ))
}

fn answered(found: Vec<crate::update::deps::Found>, app: &mut App) {
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(found).unwrap();
    app.setup.as_mut().unwrap().set_reader(receiver);
    app.tick_dependencies();
}

fn terminal_lines(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Vec<String> {
    terminal
        .backend()
        .to_string()
        .lines()
        .map(|line| line.trim_matches('"').to_owned())
        .collect()
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, modifiers));
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

/// The mouse with Ctrl held, as the terminal sends it.
fn ctrl_mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::CONTROL,
    });
}

/// The requests a test `Lsp` wrote down: method, row and UTF-16 column.
fn asked(app: &App) -> Vec<(String, usize, usize)> {
    app.lsp
        .asked
        .iter()
        .map(|(method, _, row, column)| (method.clone(), *row, *column))
        .collect()
}
