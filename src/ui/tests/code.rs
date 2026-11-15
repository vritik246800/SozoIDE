//! The drawing of Code Intelligence and of the debugger of the Onda 3 (TODO 3.3.1–3.3.3).

use super::*;
use crate::debug::{Frame as DebugFrame, State};

fn app_with(text: &str) -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.rs"), text).unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("a.rs")).unwrap();
    settle(&mut app);
    (directory, app)
}

#[test]
fn rename_actions_signature_and_the_edit_preview_draw_what_they_wait_for() {
    let (directory, mut app) = app_with("fn a() {}\n");
    let path = directory.path().join("a.rs");
    app.rename = Some(crate::app::RenameBox {
        path: path.clone(),
        version: 1,
        row: 0,
        column: 3,
        name: "beta".into(),
    });
    let text = frame(&mut app, 120, 30);
    assert!(
        text.contains("Rename the symbol") && text.contains("> beta"),
        "{text}"
    );
    app.rename = None;
    app.action_menu = Some(crate::app::ActionMenu {
        path: path.clone(),
        actions: vec![crate::lsp::CodeAction {
            title: "Import HashMap".into(),
            edit: None,
            command: None,
        }],
        selected: 0,
    });
    assert!(frame(&mut app, 120, 30).contains("Import HashMap"));
    app.action_menu = None;
    app.signature = Some((
        path.clone(),
        "fn add(a: i32, b: i32)".into(),
        Some((15, 21)),
    ));
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| super::draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = buffer_text(&terminal);
    assert!(text.contains("fn add(a: i32, b: i32)"));
    let bold = buffer
        .content()
        .iter()
        .filter(|cell| {
            cell.modifier.contains(ratatui::style::Modifier::BOLD) && cell.symbol() == "b"
        })
        .count();
    assert!(bold >= 1, "the active parameter in bold");
    app.signature = None;
    app.edit_preview = Some(crate::app::EditPreview {
        title: "Rename".into(),
        files: vec![
            crate::lsp::FileEdit {
                path: path.clone(),
                edits: Vec::new(),
            },
            crate::lsp::FileEdit {
                path: directory.path().join("b.rs"),
                edits: Vec::new(),
            },
        ],
        answer: None,
    });
    let text = frame(&mut app, 120, 30);
    assert!(
        text.contains("changes in 2 files") && text.contains("Enter: apply"),
        "{text}"
    );
}

#[test]
fn the_debug_panel_shows_frames_a_variable_tree_watches_and_clicks_choose() {
    let (directory, mut app) = app_with("fn main() {}\n");
    let path = directory.path().join("a.rs");
    let mut session = crate::debug::fake(
        State::Paused("breakpoint".into()),
        vec![
            DebugFrame {
                id: 1,
                name: "inner".into(),
                path: Some(path.clone()),
                row: 0,
            },
            DebugFrame {
                id: 2,
                name: "main".into(),
                path: Some(path.clone()),
                row: 0,
            },
        ],
        vec![("point".into(), "{...}".into())],
    );
    session.tree[0].reference = 5;
    session.expanded.insert(5);
    session.children.insert(
        5,
        vec![crate::debug::Variable {
            name: "x".into(),
            value: "2".into(),
            reference: 0,
        }],
    );
    session.watches.push(("x * 2".into(), "4".into()));
    session.console.push(("x".into(), "2".into()));
    app.debug = Some(session);
    app.breakpoint_options.insert(
        (path.clone(), 0),
        crate::debug::Breakpoint {
            condition: Some("x > 1".into()),
            hits: None,
            log: None,
        },
    );
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal.draw(|frame| super::draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "Call stack",
        "▸inner",
        "▾point",
        "x = 2",
        "x * 2 = 4",
        "> x",
        "if x > 1",
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    // A click on the second frame chooses it.
    let (_, row) = app
        .regions
        .debug_rows
        .iter()
        .find(|(target, _)| *target == crate::app::DebugTarget::Frame(1))
        .copied()
        .unwrap();
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        row.x + 1,
        row.y,
    );
    assert_eq!(app.debug.as_ref().unwrap().frame, 1);
    // The box of a breakpoint with its three fields.
    app.debug_input = Some(crate::app::DebugInput {
        kind: crate::app::DebugInputKind::Breakpoint(path, 0),
        fields: vec!["x > 1".into(), String::new(), String::new()],
        field: 0,
    });
    let text = frame(&mut app, 120, 30);
    assert!(
        text.contains("Stop if") && text.contains("After hits") && text.contains("Log instead"),
        "{text}"
    );
}
