//! Code Intelligence, debugging and the tasks of a project.

use super::*;

#[test]
fn suggestions_narrow_as_you_type_and_the_chosen_one_replaces_the_word() {
    use crate::lsp::{Event, Item};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.rs");
    std::fs::write(&path, "fn main() {\n    let list = Vec::new();\n}\n").unwrap();
    std::fs::write(directory.path().join("lib.rs"), "pub fn helper() {}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::End, KeyModifiers::NONE);
    app.editor_paste("\n    list.pu");
    let item = |label: &str, insert: &str| Item {
        label: label.into(),
        detail: String::new(),
        insert: insert.into(),
    };
    let answer = |app: &mut App, row| {
        app.lsp_event(Event::Completion {
            path: path.clone(),
            row,
            column: 11,
            items: vec![
                item("push", "push"),
                item("pop", "pop"),
                item("as_ptr", "as_ptr"),
            ],
        })
    };
    // A late answer for another row is dropped.
    answer(&mut app, 0);
    assert!(app.completion.is_none());
    answer(&mut app, 2);
    let labels = |app: &App| {
        let (items, ..) = app.completion_view().unwrap();
        items
            .iter()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&app), ["push"], "pu: push only");
    press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(
        labels(&app),
        ["push", "pop", "as_ptr"],
        "p: starting with it, then the rest"
    );
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.active_file().unwrap().lines[2], "    list.pop");
    assert!(app.completion.is_none());
    // Esc closes it; a key the list does not use reaches the editor.
    answer(&mut app, 2);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.completion.is_none());
    answer(&mut app, 2);
    press(&mut app, KeyCode::Char('('), KeyModifiers::NONE);
    assert!(app.completion.is_none());
    // The bracket closes itself (TODO 2.1.4).
    assert_eq!(app.active_file().unwrap().lines[2], "    list.pop()");

    // A definition elsewhere opens there; messages reach the status bar.
    let version = app.active_file().unwrap().edits;
    app.lsp_event(Event::Places {
        path: path.clone(),
        version,
        goal: crate::lsp::Goal::Definition,
        places: vec![crate::lsp::Place {
            path: directory.path().join("lib.rs"),
            row: 0,
            start: 7,
            end_row: 0,
            end: 13,
        }],
    });
    let file = app.active_file().unwrap();
    assert!(file.path.ends_with("lib.rs"));
    assert_eq!((file.cursor_row, file.cursor_column), (0, 7));
    app.lsp_event(Event::Message("rust-analyzer stopped".into()));
    assert_eq!(app.status, "rust-analyzer stopped");
    // Without a server Ctrl+Space and F12 do nothing harmful.
    press(&mut app, KeyCode::Char(' '), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    assert!(app.completion.is_none());
}

#[test]
fn ctrl_and_the_mouse_resting_on_a_word_ask_for_its_hover_once_and_leaving_closes_it() {
    use crate::{app::intellisense::HOVER_DELAY, lsp::Event};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.rs");
    std::fs::write(&path, "fn main() {\n    let total = compute();\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    let text_x = 30 + 1 + 4;
    let hover = ("textDocument/hover".to_owned(), 1, 8);
    assert_eq!(HOVER_DELAY, Duration::from_secs(2));
    // «total» is at columns 8 to 12 of row 1: the mouse alone asks nothing (Q72).
    mouse(&mut app, MouseEventKind::Moved, text_x + 9, 4);
    app.tick_at(Instant::now() + HOVER_DELAY * 2);
    assert!(asked(&app).is_empty() && app.hover.is_none());
    // With Ctrl, nothing before it rests 2 s.
    ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 9, 4);
    app.tick_at(Instant::now() + Duration::from_millis(400));
    assert!(asked(&app).is_empty());
    // Inside the same word it keeps waiting; then it is asked once, at the word's start.
    ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 11, 4);
    let rested = Instant::now() + HOVER_DELAY + Duration::from_millis(50);
    app.tick_at(rested);
    app.tick_at(rested + Duration::from_millis(500));
    assert_eq!(asked(&app), std::slice::from_ref(&hover));
    ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 10, 4);
    app.tick_at(rested + Duration::from_secs(1));
    assert_eq!(
        asked(&app),
        std::slice::from_ref(&hover),
        "not again inside the same word"
    );
    // An answer for another edit is dropped; the right one fills the box.
    let version = app.active_file().unwrap().edits;
    let answer = |app: &mut App, version, text: &str| {
        app.lsp_event(Event::Hover {
            path: path.clone(),
            version,
            row: 1,
            column: 8,
            text: text.into(),
        })
    };
    answer(&mut app, version + 1, "stale");
    assert!(app.hover.as_ref().unwrap().text.is_none());
    answer(&mut app, version, "```rust\nlet total: u32\n```");
    assert_eq!(
        app.hover.as_ref().unwrap().text.as_deref(),
        Some("```rust\nlet total: u32\n```")
    );
    // Ctrl let go, it stays on its word and over its box; off the word and the box it
    // closes. Spaces ask nothing.
    mouse(&mut app, MouseEventKind::Moved, text_x + 12, 4);
    assert!(app.hover.is_some());
    app.regions.hover = Rect::new(40, 5, 20, 3);
    mouse(&mut app, MouseEventKind::Moved, 45, 6);
    assert!(app.hover.is_some());
    mouse(&mut app, MouseEventKind::Moved, text_x + 2, 4);
    assert!(app.hover.is_none());
    app.regions.hover = Rect::default();
    app.tick_at(rested + Duration::from_secs(1));
    assert_eq!(asked(&app).len(), 1);
    // Ctrl let go before the 2 s: nothing is asked. «compute» starts at column 16.
    ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 17, 4);
    assert!(app.hover.is_some());
    mouse(&mut app, MouseEventKind::Moved, text_x + 18, 4);
    assert!(app.hover.is_none());
    app.tick_at(Instant::now() + HOVER_DELAY * 2);
    assert_eq!(asked(&app).len(), 1);

    // Ctrl+K asks at the cursor, also just after the word.
    app.open_files[0].move_cursor_to(1, 13);
    app.lsp.asked.clear();
    press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    assert_eq!(asked(&app), [hover]);
    answer(&mut app, version, "");
    assert!(app.hover.is_none());
    assert_eq!(app.status, "Nothing to show for this symbol");
    press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    answer(&mut app, version, "let total: u32");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.hover.is_none());
    // A key that edits closes it and reaches the text.
    press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    answer(&mut app, version, "let total: u32");
    press(&mut app, KeyCode::Char('s'), KeyModifiers::NONE);
    assert!(app.hover.is_none());
    assert_eq!(
        app.active_file().unwrap().lines[1],
        "    let totals = compute();"
    );
    // Text pasted while it was asked: the answer is for the text before.
    app.open_files[0].move_cursor_to(1, 9);
    press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    let before = app.active_file().unwrap().edits;
    app.editor_paste("x");
    answer(&mut app, before, "let totals: u32");
    assert!(app.hover.as_ref().is_none_or(|hover| hover.text.is_none()));
    app.hover = None;
    // On a space the cursor has no symbol.
    app.open_files[0].move_cursor_to(1, 2);
    press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    assert_eq!(app.status, "No symbol at the cursor");
}

#[test]
fn a_failed_server_says_why_in_the_hover_and_how_to_install_it() {
    use crate::{app::intellisense::HOVER_DELAY, lsp::Failure};
    let directory = tempfile::tempdir().unwrap();
    let rust = directory.path().join("main.rs");
    let notes = directory.path().join("notes.md");
    std::fs::write(&rust, "fn main() {\n    let total = compute();\n}\n").unwrap();
    std::fs::write(&notes, "a few words\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    let text_x = 30 + 1 + 4;
    let rest = |app: &mut App, x: u16, y: u16| {
        app.hover = None;
        ctrl_mouse(app, MouseEventKind::Moved, x, y);
        assert!(app.hover.is_some(), "the mouse rests on a word");
        app.tick_at(Instant::now() + HOVER_DELAY * 2);
        app.hover.as_ref().and_then(|hover| hover.text.clone())
    };
    let why = "error: Unknown binary 'rust-analyzer' in official toolchain 'stable'.";
    app.lsp.fail("rust-analyzer", Failure::Start(why.into()));
    app.open_file(rust.clone()).unwrap();
    let install = format!(
        "{} or Ctrl+click offers to install it",
        app.keys.label(Action::Definition)
    );
    assert_eq!(
        rest(&mut app, text_x + 9, 4),
        Some(format!("rust-analyzer could not start: {why}\n\n{install}"))
    );
    // A server that stopped cannot be installed again: only why.
    app.lsp.restart("rust-analyzer");
    app.lsp
        .fail("rust-analyzer", Failure::Stopped("killed".into()));
    assert_eq!(
        rest(&mut app, text_x + 9, 4),
        Some("rust-analyzer stopped: killed".into())
    );
    // Without a server for the language the mouse says nothing.
    app.open_file(notes).unwrap();
    assert_eq!(rest(&mut app, text_x + 3, 3), None);
    assert!(app.hover.is_none());
}

#[test]
fn definitions_peeks_and_references_open_and_alt_left_goes_back() {
    use crate::lsp::{Event, Goal, Place};
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.rs");
    let lib = directory.path().join("lib.rs");
    std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
    std::fs::write(
        &lib,
        "pub fn compute() -> u32 {\n    1\n}\n// compute again\n",
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(main.clone()).unwrap();
    app.open_files[0].move_cursor_to(1, 16);
    let place = |path: &PathBuf, row, start, end| Place {
        path: path.clone(),
        row,
        start,
        end_row: row,
        end,
    };
    let answer = |app: &mut App, goal, places: Vec<Place>| {
        let file = app.active_file().unwrap();
        let (path, version) = (file.path.clone(), file.edits);
        app.lsp_event(Event::Places {
            path,
            version,
            goal,
            places,
        });
    };
    let at = |app: &App| {
        let file = app.active_file().unwrap();
        (
            file.path.file_name().unwrap().to_str().unwrap().to_owned(),
            file.cursor_row,
            file.cursor_column,
        )
    };
    press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 16)]);
    assert_eq!(app.status, "Looking for the definition...");
    // An answer for an older edit is dropped.
    let version = app.active_file().unwrap().edits;
    app.lsp_event(Event::Places {
        path: main.clone(),
        version: version + 1,
        goal: Goal::Definition,
        places: vec![place(&lib, 0, 7, 14)],
    });
    assert_eq!(at(&app), ("main.rs".into(), 1, 16));
    // One definition opens there; Alt+Left comes back.
    answer(&mut app, Goal::Definition, vec![place(&lib, 0, 7, 14)]);
    assert_eq!(at(&app), ("lib.rs".into(), 0, 7));
    press(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(at(&app), ("main.rs".into(), 1, 16));

    // Several open a list to choose from, which takes no typing; Enter opens one.
    answer(
        &mut app,
        Goal::Definition,
        vec![place(&lib, 3, 3, 10), place(&lib, 0, 7, 14)],
    );
    let search = app.search.as_ref().unwrap();
    assert_eq!(search.title.as_deref(), Some("Definitions"));
    assert_eq!(search.query, "compute");
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(app.search.as_ref().unwrap().query, "compute");
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let file = app.active_file().unwrap();
    assert_eq!(
        at(&app),
        ("lib.rs".into(), 3, 10),
        "the second, sorted by row"
    );
    assert_eq!(file.anchor, Some((3, 3)), "the symbol selected");
    // The ESC b of the Terminal.app's Option+Left goes back too.
    press(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
    assert_eq!(at(&app), ("main.rs".into(), 1, 16));
    app.tick_at(Instant::now() + Duration::from_secs(1));

    // References: the same list, with every use and its line.
    app.lsp.asked.clear();
    press(&mut app, KeyCode::F(12), KeyModifiers::SHIFT);
    assert_eq!(asked(&app), [("textDocument/references".into(), 1, 16)]);
    answer(&mut app, Goal::References, Vec::new());
    assert_eq!(app.status, "No references found");
    answer(
        &mut app,
        Goal::References,
        vec![place(&main, 1, 16, 23), place(&lib, 0, 7, 14)],
    );
    let search = app.search.as_ref().unwrap();
    assert_eq!(search.title.as_deref(), Some("References"));
    let lines = search
        .matches
        .iter()
        .map(|found| (found.line, found.column, found.length, found.text.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        [
            (0, 7, 7, "pub fn compute() -> u32 {"),
            (1, 16, 7, "    let total = compute();"),
        ]
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.tick_at(Instant::now() + Duration::from_secs(1));
    assert!(app.search.is_none());

    // Peek shows the definition without leaving the tab; the arrows scroll, Enter goes.
    press(&mut app, KeyCode::F(12), KeyModifiers::ALT);
    answer(&mut app, Goal::Peek, vec![place(&lib, 0, 7, 14)]);
    let peek = app.peek.as_ref().unwrap();
    assert_eq!((peek.row, peek.column, peek.top), (0, 7, 0));
    assert_eq!(peek.lines[3], "// compute again");
    assert_eq!(at(&app).0, "main.rs");
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.peek.as_ref().unwrap().top, 1);
    assert_eq!(at(&app), ("main.rs".into(), 1, 16), "the cursor stays");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.peek.is_none());
    assert_eq!(at(&app), ("lib.rs".into(), 0, 7));
    press(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(at(&app), ("main.rs".into(), 1, 16));
    // Esc closes it where it was.
    press(&mut app, KeyCode::F(12), KeyModifiers::ALT);
    answer(&mut app, Goal::Peek, vec![place(&lib, 0, 7, 14)]);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.peek.is_none());
    assert_eq!(at(&app).0, "main.rs");

    // Ctrl+click asks for the definition of what was clicked.
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    app.lsp.asked.clear();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 30 + 1 + 4 + 9,
        row: 4,
        modifiers: KeyModifiers::CONTROL,
    });
    assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 9)]);
    assert!(!app.selecting && app.active_file().unwrap().anchor.is_none());
}

#[test]
fn code_intelligence_says_why_without_a_server() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.md");
    std::fs::write(&path, "# Notes\nsome words\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    app.open_files[0].move_cursor_to(1, 2);
    for (code, modifiers) in [
        (KeyCode::F(12), KeyModifiers::NONE),
        (KeyCode::F(12), KeyModifiers::ALT),
        (KeyCode::F(12), KeyModifiers::SHIFT),
        (KeyCode::Char('k'), KeyModifiers::CONTROL),
    ] {
        app.status.clear();
        press(&mut app, code, modifiers);
        assert_eq!(app.status, "No language server for .md files", "{code:?}");
    }
    assert!(app.hover.is_none() && app.peek.is_none() && app.search.is_none());
    assert_eq!(app.active_file().unwrap().lines, ["# Notes", "some words"]);
    // The mouse resting on a word says nothing.
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    app.status.clear();
    mouse(&mut app, MouseEventKind::Moved, 30 + 1 + 4 + 1, 4);
    app.tick_at(Instant::now() + Duration::from_secs(1));
    assert!(app.hover.is_none() && app.status.is_empty());
    press(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(app.status, "No earlier place to go back to");
}

#[test]
fn f7_builds_the_project_after_showing_the_command_once_and_shift_f5_stops_it() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("Cargo.toml"), "[package]\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    let rows = |app: &App| app.confirm.as_ref().unwrap().rows.clone();
    press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    let shown = rows(&app);
    assert_eq!(shown[0], ("Project", "Cargo".to_owned()));
    assert_eq!(shown[2], ("From", "Cargo.toml".to_owned()));
    assert!(shown[3].1.ends_with("&& cargo build"), "{:?}", shown[3]);
    assert_eq!(
        app.confirm.as_ref().unwrap().title,
        "Build the Cargo project"
    );
    // Esc runs nothing; Enter types it into the bottom terminal, which takes the focus.
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.confirm.is_none() && !app.show_bottom_terminal);
    assert_eq!(app.status, "Cancelled");
    press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.confirm.is_none() && app.show_bottom_terminal);
    assert_eq!(app.focus, Focus::BottomTerminal);
    assert!(app.status.starts_with("Building: cd "), "{}", app.status);
    // The same command does not ask again; Shift+F5 stops it from the terminal.
    press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    assert!(app.confirm.is_none());
    press(&mut app, KeyCode::F(5), KeyModifiers::SHIFT);
    assert_eq!(app.status, "Stopped");
    press(&mut app, KeyCode::F(5), KeyModifiers::SHIFT);
    assert_eq!(
        app.status, "Stopped",
        "with nothing running the key is the shell's"
    );
    app.open_file(directory.path().join("Cargo.toml")).unwrap();
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::F(17), KeyModifiers::NONE);
    assert_eq!(app.status, "Nothing is running");
    // Clean, Rebuild and Run ask for their own commands.
    for (code, modifiers, command) in [
        (KeyCode::F(7), KeyModifiers::SHIFT, "&& cargo clean"),
        (
            KeyCode::F(7),
            KeyModifiers::CONTROL,
            "&& cargo clean && cargo build",
        ),
        (KeyCode::F(5), KeyModifiers::CONTROL, "&& cargo run"),
    ] {
        press(&mut app, code, modifiers);
        assert!(rows(&app)[3].1.ends_with(command), "{:?}", rows(&app));
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    }
}

#[test]
fn several_projects_are_chosen_and_a_folder_without_one_says_how() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut app = App::new(root.to_path_buf()).unwrap();
    press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    assert_eq!(
        app.status,
        "Failed: no project here. Add [tasks.build] to .sozo.toml"
    );
    press(&mut app, KeyCode::F(5), KeyModifiers::CONTROL);
    assert_eq!(
        app.status,
        "Failed: no project here. Add [tasks.run] to .sozo.toml"
    );
    // Two folders with a project: chosen once for the session.
    for name in ["api", "web"] {
        std::fs::create_dir(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("Cargo.toml"), "").unwrap();
    }
    press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    let choice = app.project_choice.as_ref().unwrap();
    let folders = choice
        .projects
        .iter()
        .map(|project| project.folder.clone())
        .collect::<Vec<_>>();
    assert_eq!(folders, [root.join("api"), root.join("web")]);
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.project_choice.is_none());
    let confirm = app.confirm.as_ref().unwrap();
    // Joined as the app joins it: `\\` between the parts on Windows.
    let web = std::path::Path::new(root.file_name().unwrap()).join("web");
    assert_eq!(confirm.rows[1], ("Folder", web.display().to_string()));
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    assert!(app.project_choice.is_none(), "the choice is kept");
    assert_eq!(
        app.confirm.as_ref().unwrap().rows[1].1,
        web.display().to_string()
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    // A command nobody can know, and a tool that is not there, say so.
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("CMakeLists.txt"), "").unwrap();
    let mut app = App::new(other.path().to_path_buf()).unwrap();
    press(&mut app, KeyCode::F(5), KeyModifiers::CONTROL);
    assert_eq!(
        app.status,
        "Failed: This CMake project has no run command: add [tasks.run] to .sozo.toml"
    );
    std::fs::write(
        other.path().join(".sozo.toml"),
        "[tasks.build]\nprogram = \"sozo-no-such-tool\"\n",
    )
    .unwrap();
    press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    assert_eq!(
        app.status,
        "Failed: sozo-no-such-tool is missing: Help, then Dependencies"
    );
    assert!(app.confirm.is_none());
}

#[test]
fn the_editor_menu_previews_each_code_entry_beside_it_and_a_click_opens_a_place() {
    use crate::{
        app::PreviewRow,
        lsp::{Event, Goal, Place},
    };
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.rs");
    let lib = directory.path().join("lib.rs");
    std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
    std::fs::write(&lib, "pub fn compute() -> u32 {\n    1\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(main.clone()).unwrap();
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    let text_x = 30 + 1 + 4;
    // The menu's entries as the view draws them: Paste, then the six questions.
    let open_menu = |app: &mut App| {
        mouse(
            app,
            MouseEventKind::Down(MouseButton::Right),
            text_x + 18,
            4,
        );
        let actions = app.entry_menu.as_ref().unwrap().actions.clone();
        app.regions.entry_menu_items = actions
            .iter()
            .enumerate()
            .map(|(index, action)| (*action, Rect::new(50, 6 + index as u16, 24, 1)))
            .collect();
    };
    let version = app.active_file().unwrap().edits;
    let place = |path: &std::path::Path, row, start, end| Place {
        path: path.to_path_buf(),
        row,
        start,
        end_row: row,
        end,
    };
    let answer = |app: &mut App, goal, version, places| {
        app.lsp_event(Event::Places {
            path: main.clone(),
            version,
            goal,
            places,
        })
    };
    open_menu(&mut app);
    assert!(app.menu_preview.is_none(), "Paste has nothing beside it");
    // The mouse on Go to Definition asks about «compute» once, and nothing jumps.
    mouse(&mut app, MouseEventKind::Moved, 55, 7);
    mouse(&mut app, MouseEventKind::Moved, 60, 7);
    assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 18)]);
    answer(
        &mut app,
        Goal::Definition,
        version + 1,
        vec![place(&lib, 0, 7, 14)],
    );
    assert!(
        app.menu_preview.as_ref().unwrap().places.is_none(),
        "another edit"
    );
    answer(
        &mut app,
        Goal::Definition,
        version,
        vec![place(&lib, 0, 7, 14), place(&main, 1, 16, 23)],
    );
    let preview = app.menu_preview.as_ref().unwrap();
    let places = preview.places.as_ref().unwrap();
    assert_eq!(places.len(), 2);
    assert_eq!(places[0].text, "pub fn compute() -> u32 {");
    assert!(app.entry_menu.is_some() && app.search.is_none());
    assert_eq!(app.active_file().unwrap().path, main);
    // Paste has none; the arrow down to Peek asks for it and shows the file's lines.
    mouse(&mut app, MouseEventKind::Moved, 55, 6);
    assert!(app.menu_preview.is_none());
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.menu_preview.as_ref().unwrap().goal, Goal::Peek);
    answer(&mut app, Goal::Peek, version, vec![place(&lib, 0, 7, 14)]);
    let preview = app.menu_preview.as_ref().unwrap();
    assert_eq!(preview.lines, ["pub fn compute() -> u32 {", "    1", "}"]);
    // A click on it opens the place in its own tab; Back returns.
    app.regions.preview_rows = vec![(PreviewRow::Place(0), Rect::new(75, 8, 30, 3))];
    mouse(&mut app, MouseEventKind::Moved, 80, 9);
    assert_eq!(
        app.menu_preview.as_ref().unwrap().hovered,
        Some(PreviewRow::Place(0))
    );
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 80, 9);
    assert!(app.entry_menu.is_none() && app.menu_preview.is_none());
    let file = app.active_file().unwrap();
    assert_eq!(
        (file.path.clone(), file.cursor_row, file.cursor_column),
        (lib.clone(), 0, 7)
    );
    assert_eq!(app.open_files.len(), 2);
    app.go_back();
    assert_eq!(app.active_file().unwrap().path, main);

    // The entry clicked before the answer acts when it comes: references are a list.
    app.regions.preview_rows.clear();
    open_menu(&mut app);
    mouse(&mut app, MouseEventKind::Moved, 55, 12);
    assert_eq!(app.menu_preview.as_ref().unwrap().goal, Goal::References);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 55, 12);
    assert!(app.entry_menu.is_none());
    assert_eq!(app.status, "Looking for the references...");
    answer(
        &mut app,
        Goal::References,
        version,
        vec![place(&main, 1, 16, 23), place(&lib, 0, 7, 14)],
    );
    assert!(app.menu_preview.is_none());
    assert_eq!(
        app.search.as_ref().unwrap().title.as_deref(),
        Some("References")
    );
    app.search = None;
    // With the answer already there, the entry acts at once; a closed menu takes its
    // preview with it.
    open_menu(&mut app);
    mouse(&mut app, MouseEventKind::Moved, 55, 9);
    answer(
        &mut app,
        Goal::Declaration,
        version,
        vec![place(&lib, 0, 7, 14)],
    );
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 55, 9);
    assert_eq!(app.active_file().unwrap().path, lib);
    app.go_back();
    open_menu(&mut app);
    mouse(&mut app, MouseEventKind::Moved, 55, 10);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.tick_at(Instant::now() + Duration::from_secs(1));
    assert!(app.menu_preview.is_none());
}

#[test]
fn a_missing_server_offers_its_installation_and_the_request_runs_once_it_is_there() {
    use crate::{
        app::PreviewRow,
        lsp::{Failure, Goal},
    };
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.rs");
    let script = directory.path().join("run.py");
    std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
    std::fs::write(&script, "total = 1\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.installs = |name| {
        Some(vec![
            "rustup".into(),
            "component".into(),
            "add".into(),
            name.into(),
        ])
    };
    // Here the server is there as soon as it is watched.
    app.watcher = |_| {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(()).unwrap();
        receiver
    };
    app.open_file(main.clone()).unwrap();
    app.open_files[0].move_cursor_to(1, 18);
    let why = "error: Unknown binary 'rust-analyzer' in official toolchain 'stable'.";
    app.lsp.fail("rust-analyzer", Failure::Start(why.into()));
    // F12 with the server unable to start: the dialog with the command, nothing yet.
    press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    let confirm = app.confirm.as_ref().unwrap();
    assert_eq!(confirm.title, "Install rust-analyzer");
    assert_eq!(
        confirm.rows,
        [
            ("For", "Go to Definition in .rs files".to_owned()),
            ("Why", format!("rust-analyzer could not start: {why}")),
            ("Component", "rust-analyzer (Language server)".to_owned()),
            ("Command", "rustup component add rust-analyzer".to_owned()),
        ]
    );
    assert!(asked(&app).is_empty() && app.server_watch.is_none());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.confirm.is_none() && app.server_watch.is_none());
    assert_eq!(app.status, "Cancelled");
    // Enter installs in the bottom terminal; once it is there the request runs again.
    press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        app.status,
        "Installing rust-analyzer: rustup component add rust-analyzer"
    );
    assert!(app.show_bottom_terminal && app.server_watch.is_some());
    app.tick_at(Instant::now() + Duration::from_secs(1));
    assert!(app.server_watch.is_none());
    assert_eq!(app.lsp.installable(&main), None);
    assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 18)]);
    assert_eq!(app.status, "Looking for the definition...");

    // A request already sent when the server fails (it stopped before answering)
    // offers the installation too.
    app.lsp.asked.clear();
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    assert!(app.hover.is_some() && app.confirm.is_none());
    app.lsp.fail("rust-analyzer", Failure::Start(String::new()));
    app.tick_at(Instant::now() + Duration::from_secs(2));
    assert!(app.hover.is_none());
    assert_eq!(
        app.confirm.as_ref().unwrap().rows[0].1,
        "Information on the symbol in .rs files"
    );
    // A server that never comes is said.
    app.watcher = |_| std::sync::mpsc::channel().1;
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    app.tick_at(Instant::now() + Duration::from_secs(3));
    assert_eq!(app.status, "rust-analyzer is still not installed");
    assert!(app.server_watch.is_none());

    // The preview of the menu says why and its Install row offers the same.
    app.focus = Focus::Editor;
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        30 + 1 + 4 + 18,
        4,
    );
    let actions = app.entry_menu.as_ref().unwrap().actions.clone();
    app.regions.entry_menu_items = actions
        .iter()
        .enumerate()
        .map(|(index, action)| (*action, Rect::new(50, 6 + index as u16, 24, 1)))
        .collect();
    mouse(&mut app, MouseEventKind::Moved, 55, 7);
    let preview = app.menu_preview.as_ref().unwrap();
    assert_eq!(
        preview.failure.as_deref(),
        Some("rust-analyzer could not start")
    );
    assert!(preview.install);
    app.choose_preview_row(PreviewRow::Install);
    assert_eq!(app.confirm.as_ref().unwrap().title, "Install rust-analyzer");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    // Without a package manager for it, or after a crash, the footer only says why.
    app.installs = |_| None;
    app.lsp.fail("pyright-langserver", Failure::Missing);
    app.open_file(script).unwrap();
    press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    assert!(app.confirm.is_none());
    assert_eq!(
        app.status,
        "pyright-langserver is not installed. No package manager here installs it: do it by hand"
    );
    app.lsp.restart("pyright-langserver");
    app.lsp
        .fail("pyright-langserver", Failure::Stopped("killed".into()));
    app.ask_places(Goal::References);
    assert!(app.confirm.is_none());
    assert_eq!(app.status, "pyright-langserver stopped: killed");
}

#[test]
fn the_editor_menu_goes_to_declarations_types_and_implementations_and_ctrl_marks_the_link() {
    use crate::lsp::{Event, Goal, Place};
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.rs");
    std::fs::write(&path, "fn main() {\n    let total = compute();\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    app.regions.editor_body = Rect::new(30, 2, 60, 20);
    app.regions.pane_bodies[0] = app.regions.editor_body;
    let text_x = 30 + 1 + 4;
    // The right-click puts the cursor on «compute»; the menu asks about it.
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        text_x + 18,
        4,
    );
    app.choose_entry_action(EntryAction::Code(Goal::Declaration));
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        text_x + 18,
        4,
    );
    app.choose_entry_action(EntryAction::Code(Goal::Implementation));
    assert_eq!(
        asked(&app),
        [
            ("textDocument/declaration".into(), 1, 18),
            ("textDocument/implementation".into(), 1, 18)
        ]
    );
    let version = app.active_file().unwrap().edits;
    let place = |row, start, end| Place {
        path: path.clone(),
        row,
        start,
        end_row: row,
        end,
    };
    let answer = |app: &mut App, goal, places| {
        app.lsp_event(Event::Places {
            path: path.clone(),
            version,
            goal,
            places,
        })
    };
    answer(
        &mut app,
        Goal::Implementation,
        vec![place(0, 3, 7), place(1, 8, 13)],
    );
    assert_eq!(
        app.search.as_ref().unwrap().title.as_deref(),
        Some("Implementations")
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.tick_at(Instant::now() + Duration::from_secs(1));
    answer(&mut app, Goal::TypeDefinition, vec![place(0, 3, 7)]);
    let file = app.active_file().unwrap();
    assert_eq!(
        (file.cursor_row, file.cursor_column),
        (0, 3),
        "one place goes there"
    );
    answer(&mut app, Goal::Declaration, Vec::new());
    assert_eq!(app.status, "No declaration found");

    // Ctrl over a word marks it as a link; without Ctrl, or off it, the mark goes.
    let moved = |app: &mut App, column, modifiers| {
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row: 4,
            modifiers,
        })
    };
    moved(&mut app, text_x + 10, KeyModifiers::CONTROL);
    assert_eq!(app.link, Some((0, 1, 8, 13)));
    assert_eq!(app.status, "Ctrl+click: go to the definition");
    moved(&mut app, text_x + 2, KeyModifiers::CONTROL);
    assert_eq!(app.link, None, "a space is no link");
    moved(&mut app, text_x + 10, KeyModifiers::NONE);
    assert_eq!(app.link, None);
}

#[test]
fn f9_marks_breakpoints_f5_builds_in_the_background_and_shift_f5_stops() {
    use crate::debug::{Frame, State, fake};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.md");
    std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::F(9), KeyModifiers::NONE);
    assert_eq!(
        app.breakpoints[&path].iter().copied().collect::<Vec<_>>(),
        [1]
    );
    press(&mut app, KeyCode::F(9), KeyModifiers::NONE);
    assert!(app.breakpoints[&path].is_empty(), "F9 again takes it off");

    // F5 plans and builds in a thread; the answer comes in a tick.
    press(&mut app, KeyCode::F(5), KeyModifiers::NONE);
    assert!(app.debug_building());
    let start = Instant::now();
    while app.debug_building() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
        app.tick_at(Instant::now());
    }
    assert_eq!(app.status, "SozoIDE does not know how to debug .md files");
    assert!(app.debug.is_none());

    // Paused: F10 steps, the margin knows the line; Shift+F5 (or F17) closes the panel.
    let frame = Frame {
        id: 1,
        name: "main".into(),
        path: Some(path.clone()),
        row: 2,
    };
    app.debug = Some(fake(
        State::Paused("breakpoint".into()),
        vec![frame],
        Vec::new(),
    ));
    assert_eq!(app.paused_row(&path), Some(2));
    press(&mut app, KeyCode::F(10), KeyModifiers::NONE);
    assert_eq!(app.debug.as_ref().unwrap().state, State::Running);
    assert_eq!(app.paused_row(&path), None);
    press(&mut app, KeyCode::F(5), KeyModifiers::SHIFT);
    assert!(app.debug.is_none());
    assert_eq!(app.status, "Debugging stopped");
    app.debug = Some(fake(
        State::Ended("Exited with code 0".into()),
        Vec::new(),
        Vec::new(),
    ));
    press(&mut app, KeyCode::F(17), KeyModifiers::NONE);
    assert!(app.debug.is_none());
}

/// A project with two Rust files, trusted, and `a.rs` open in the editor.
fn rust_project() -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn old() {}\nfn main() { old(); }\n").unwrap();
    std::fs::write(root.join("b.rs"), "use crate::old;\n").unwrap();
    let mut app = App::new(root.clone()).unwrap();
    app.trust = Some(true);
    app.open_file(root.join("a.rs")).unwrap();
    app.focus = Focus::Editor;
    (directory, app)
}

fn edit_of(
    path: &std::path::Path,
    row: usize,
    start: usize,
    end: usize,
    text: &str,
) -> crate::lsp::FileEdit {
    crate::lsp::FileEdit {
        path: path.to_path_buf(),
        edits: vec![crate::lsp::actions::TextEdit {
            row,
            start,
            end_row: row,
            end,
            text: text.into(),
        }],
    }
}

#[test]
fn rename_asks_the_server_and_edits_in_two_files_wait_for_enter() {
    use crate::lsp::{EditGoal, Event};
    let (_directory, mut app) = rust_project();
    let root = app.root.clone();
    let index = app.active_tab.unwrap();
    app.open_files[index].move_cursor_to(0, 4);
    // F2 in the editor: the box with the word, Enter asks the server.
    press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    assert_eq!(app.rename.as_ref().unwrap().name, "old");
    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    for key in "new".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.lsp.asked.last().unwrap().0, "textDocument/rename");
    // The answer changes two files: nothing changes before Enter, Esc changes nothing.
    let version = app.open_files[index].edits;
    let files = vec![
        edit_of(&root.join("a.rs"), 0, 3, 6, "new"),
        edit_of(&root.join("b.rs"), 0, 11, 14, "new"),
    ];
    app.lsp_event(Event::Edits {
        path: root.join("a.rs"),
        version,
        goal: EditGoal::Rename,
        result: Ok(files.clone()),
    });
    assert!(app.edit_preview.is_some());
    assert_eq!(app.open_files[index].lines[0], "fn old() {}");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(
        std::fs::read_to_string(root.join("b.rs")).unwrap(),
        "use crate::old;\n"
    );
    app.lsp_event(Event::Edits {
        path: root.join("a.rs"),
        version,
        goal: EditGoal::Rename,
        result: Ok(files),
    });
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.open_files[index].lines[0], "fn new() {}");
    assert_eq!(
        std::fs::read_to_string(root.join("b.rs")).unwrap(),
        "use crate::new;\n"
    );
    // The open file takes it as one Ctrl+Z.
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.open_files[index].lines[0], "fn old() {}");
}

#[test]
fn late_or_hostile_answers_change_nothing_and_formatting_is_one_undo() {
    use crate::lsp::{EditGoal, Event};
    let (_directory, mut app) = rust_project();
    let root = app.root.clone();
    let index = app.active_tab.unwrap();
    let version = app.open_files[index].edits;
    // An answer for an older text is dropped.
    app.lsp_event(Event::Edits {
        path: root.join("a.rs"),
        version: version - 1,
        goal: EditGoal::Format,
        result: Ok(vec![edit_of(&root.join("a.rs"), 0, 0, 2, "XX")]),
    });
    assert_eq!(app.open_files[index].lines[0], "fn old() {}");
    assert_eq!(app.status, "The file changed: ask again");
    // An edit past the end of the file refuses the whole answer.
    app.lsp_event(Event::Edits {
        path: root.join("a.rs"),
        version,
        goal: EditGoal::Format,
        result: Ok(vec![edit_of(&root.join("a.rs"), 9, 0, 1, "x")]),
    });
    assert!(app.status.starts_with("Not applied"), "{}", app.status);
    // A server asking to apply an edit out of the project is answered no.
    app.lsp_event(Event::ApplyEdit {
        server: "rust-analyzer",
        id: serde_json::json!(4),
        label: "Move".into(),
        result: Err("x.rs is outside the project".into()),
    });
    assert!(app.status.contains("outside the project"));
    assert!(app.edit_preview.is_none());
    // Format of the open file applies at once, as one step of undo.
    press(
        &mut app,
        KeyCode::Char('F'),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    );
    assert_eq!(app.lsp.asked.last().unwrap().0, "textDocument/formatting");
    app.lsp_event(Event::Edits {
        path: root.join("a.rs"),
        version,
        goal: EditGoal::Format,
        result: Ok(vec![edit_of(&root.join("a.rs"), 0, 2, 3, "  ")]),
    });
    assert_eq!(app.open_files[index].lines[0], "fn  old() {}");
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.open_files[index].lines[0], "fn old() {}");
}

#[test]
fn code_actions_signatures_symbols_and_problems_answer_their_keys() {
    use crate::lsp::{CodeAction, Diagnostic, Event, Symbol};
    let (_directory, mut app) = rust_project();
    let root = app.root.clone();
    let index = app.active_tab.unwrap();
    let version = app.open_files[index].edits;
    press(&mut app, KeyCode::Char('.'), KeyModifiers::CONTROL);
    assert_eq!(app.lsp.asked.last().unwrap().0, "textDocument/codeAction");
    let edit = serde_json::json!({"changes": {crate::lsp::uri::from_path(&root.join("a.rs")): [
        {"newText": "pub ", "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}}
    ]}});
    app.lsp_event(Event::Actions {
        path: root.join("a.rs"),
        version,
        actions: vec![
            CodeAction {
                title: "Make it public".into(),
                edit: Some(edit),
                command: None,
            },
            CodeAction {
                title: "Other".into(),
                edit: None,
                command: None,
            },
        ],
    });
    assert_eq!(app.action_menu.as_ref().unwrap().actions.len(), 2);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.open_files[index].lines[0], "pub fn old() {}");
    // Typing `(` asks the signature; `)` closes it.
    app.open_files[index].move_cursor_to(1, 15);
    press(&mut app, KeyCode::Char('('), KeyModifiers::NONE);
    assert_eq!(
        app.lsp.asked.last().unwrap().0,
        "textDocument/signatureHelp"
    );
    let version = app.open_files[index].edits;
    app.lsp_event(Event::Signature {
        path: root.join("a.rs"),
        version,
        signature: Some(("fn old(a: u8)".into(), Some((7, 12)))),
    });
    assert!(app.signature.is_some());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.signature.is_none());
    // `@` in the finder asks the symbols of the file and Enter goes to one.
    press(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('@'), KeyModifiers::NONE);
    assert_eq!(
        app.lsp.asked.last().unwrap().0,
        "textDocument/documentSymbol"
    );
    app.lsp_event(Event::Symbols {
        path: Some(root.join("a.rs")),
        symbols: vec![Symbol {
            name: "main".into(),
            kind: "function",
            path: root.join("a.rs"),
            row: 1,
            start: 3,
        }],
    });
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        (
            app.open_files[index].cursor_row,
            app.open_files[index].cursor_column
        ),
        (1, 3)
    );
    // Ctrl+Shift+M: every problem of the project, errors first.
    for (name, error) in [("b.rs", false), ("a.rs", true)] {
        app.lsp.diagnostics.insert(
            root.join(name),
            vec![Diagnostic {
                row: 0,
                start: 0,
                end_row: 0,
                end: 1,
                error,
                message: format!("in {name}"),
            }],
        );
    }
    press(
        &mut app,
        KeyCode::Char('M'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let rows = app.finder_rows();
    assert!(
        matches!(&rows[0], crate::app::FinderRow::Problem(path, problem) if path.ends_with("a.rs") && problem.error)
    );
    assert_eq!(rows.len(), 2);
}

#[test]
fn breakpoint_options_watches_and_the_console_go_through_their_boxes_and_stay_with_the_folder() {
    let (_directory, mut app) = rust_project();
    let root = app.root.clone();
    // Shift+F9 makes the breakpoint and opens its box; Tab goes to the count.
    press(&mut app, KeyCode::F(9), KeyModifiers::SHIFT);
    assert!(app.breakpoints[&root.join("a.rs")].contains(&0));
    for key in "i > 3".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('2'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let options = &app.breakpoint_options[&(root.join("a.rs"), 0)];
    assert_eq!(
        (options.condition.as_deref(), options.hits.as_deref()),
        (Some("i > 3"), Some("2"))
    );
    // Without a session the expression boxes say to start first.
    press(
        &mut app,
        KeyCode::Char('E'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(app.debug_input.is_none() && app.status.contains("F5"));
    app.debug = Some(crate::debug::fake(
        crate::debug::State::Paused("step".into()),
        Vec::new(),
        Vec::new(),
    ));
    press(
        &mut app,
        KeyCode::Char('W'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    for key in "len".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.debug.as_ref().unwrap().watches[0].0, "len");
    press(
        &mut app,
        KeyCode::Char('E'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    for key in "1 + 1".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.debug.as_ref().unwrap().console[0].0, "1 + 1");
    // The breakpoints and their options come back with the folder.
    let sessions = tempfile::tempdir().unwrap();
    let file = sessions.path().join("sessions.toml");
    app.sessions = Some(file.clone());
    app.save_session();
    let mut again = App::new(root.clone()).unwrap();
    again.sessions = Some(file);
    again.restore_session().unwrap();
    assert!(again.breakpoints[&root.join("a.rs")].contains(&0));
    assert_eq!(
        again.breakpoint_options[&(root.join("a.rs"), 0)]
            .condition
            .as_deref(),
        Some("i > 3")
    );
}

#[test]
fn a_save_asks_the_server_to_format_only_when_the_settings_say_so_in_a_trusted_folder() {
    let (_directory, mut app) = rust_project();
    let index = app.active_tab.unwrap();
    let asked = |app: &App| {
        app.lsp
            .asked
            .iter()
            .filter(|(method, ..)| method == "textDocument/formatting")
            .count()
    };
    app.format_saved(index, false);
    assert_eq!(asked(&app), 0);
    app.format_saved(index, true);
    assert_eq!(asked(&app), 1);
    app.trust = Some(false);
    app.format_saved(index, true);
    assert_eq!(asked(&app), 1, "not in a folder that is not trusted");
}
