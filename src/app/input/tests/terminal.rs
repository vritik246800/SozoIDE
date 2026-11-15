//! The terminals: keys, the mouse and what runs in them.

use super::*;

#[test]
fn a_click_goes_to_the_program_that_asked_for_the_mouse_and_shift_chooses_text() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    let area = app.regions.bottom_terminal;
    let (x, y) = (area.x + 3, area.y + 2);
    program_in(
        &mut app,
        Focus::BottomTerminal,
        b"\x1b[?1000h\x1b[?1002h\x1b[?1006h",
    );
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(app.focus, Focus::BottomTerminal);
    assert!(
        app.terminal_selection.is_none(),
        "the program has the click"
    );
    assert_eq!(app.terminal_mouse, Some((Focus::BottomTerminal, 0)));
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x + 1, y);
    assert!(app.terminal_selection.is_none());
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x + 1, y);
    assert_eq!(app.terminal_mouse, None);

    // With Shift, text as in any terminal.
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::SHIFT,
    });
    assert!(app.terminal_selection.is_some());
    assert_eq!(app.terminal_mouse, None);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);

    // The right button also reaches it; a shell keeps its menu-less press.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), x, y);
    assert_eq!(app.terminal_mouse, Some((Focus::BottomTerminal, 2)));
    mouse(&mut app, MouseEventKind::Up(MouseButton::Right), x, y);
    assert_eq!(app.terminal_mouse, None);
}

#[test]
fn the_wheel_does_not_move_the_history_under_a_full_screen_program() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    let lines = (0..200)
        .map(|index| format!("line {index}\r\n"))
        .collect::<String>();
    program_in(&mut app, Focus::RightTerminal, lines.as_bytes());
    let area = app.regions.right_terminal;
    mouse(&mut app, MouseEventKind::ScrollUp, area.x + 3, area.y + 3);
    assert_eq!(app.right_terminal.current().scrollback(), 3);
    mouse(&mut app, MouseEventKind::ScrollDown, area.x + 3, area.y + 3);
    assert_eq!(app.right_terminal.current().scrollback(), 0);
    program_in(
        &mut app,
        Focus::RightTerminal,
        b"\x1b[?1049h\x1b[?1006h\x1b[?1000h",
    );
    mouse(&mut app, MouseEventKind::ScrollUp, area.x + 3, area.y + 3);
    assert_eq!(app.right_terminal.current().scrollback(), 0);
}

#[test]
fn ctrl_keys_belong_to_the_terminal_with_the_focus_and_f6_leaves_it() {
    let mut app = test_app();
    app.show_bottom_terminal = true;
    app.show_git = false;
    app.focus = Focus::BottomTerminal;
    let ctrl = |letter| KeyEvent::new(KeyCode::Char(letter), KeyModifiers::CONTROL);
    for letter in ['b', 'j', 'g', 'd', 'p'] {
        app.handle_key(ctrl(letter));
        assert_eq!(app.focus, Focus::BottomTerminal, "Ctrl+{letter}");
        assert!(app.show_explorer && app.show_bottom_terminal);
        assert!(!app.show_git && !app.show_database && app.search.is_none());
    }
    // What is not Ctrl and a letter stays the IDE's.
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::Help);
    app.overlay = Overlay::None;
    app.handle_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Editor);
    // Out of the terminal the same keys are the commands again.
    app.handle_key(ctrl('b'));
    assert!(!app.show_explorer);
    // Ctrl+Q always quits.
    app.focus = Focus::BottomTerminal;
    app.handle_key(ctrl('q'));
    assert!(app.should_quit);
}

#[test]
fn ctrl_z_undoes_in_the_editor_and_reaches_the_terminals_elsewhere() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "one\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::End, KeyModifiers::NONE);
    for character in " two".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    let active = app.active_tab.unwrap();
    assert_eq!(app.open_files[active].lines, ["one two"]);
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.open_files[active].lines, ["one"]);
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.status, "Nothing to undo");
    // In a terminal Ctrl+Z belongs to the shell; the editor keeps its text.
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    app.focus = Focus::BottomTerminal;
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.open_files[active].lines, ["onex"]);
}

#[test]
fn ctrl_r_runs_the_file_in_the_bottom_terminal_and_stays_with_the_shell_there() {
    let directory = tempfile::tempdir().unwrap();
    let script = if cfg!(windows) {
        "hello.bat"
    } else {
        "hello.sh"
    };
    std::fs::write(directory.path().join(script), "echo hello\n").unwrap();
    std::fs::write(directory.path().join("notes.md"), "# notes\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("notes.md")).unwrap();
    app.editor_paste("unsaved");
    press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert_eq!(app.status, "SozoIDE does not know how to run .md files");
    assert!(!app.show_bottom_terminal);
    assert!(!app.open_files[0].dirty, "Ctrl+R saves first");

    app.open_file(directory.path().join(script)).unwrap();
    press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(app.status.starts_with("Running"), "{}", app.status);
    assert!(app.show_bottom_terminal && app.focus == Focus::BottomTerminal);
    // In the terminal Ctrl+R belongs to the shell (history search).
    app.status.clear();
    press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(app.status.is_empty());

    // The Explorer runs the selected file.
    app.focus = Focus::Explorer;
    app.show_bottom_terminal = false;
    app.selected_entry = app
        .entries
        .iter()
        .position(|entry| entry.path.ends_with(script))
        .unwrap();
    press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(app.show_bottom_terminal && app.status.starts_with("Running"));
}

#[test]
fn dragging_over_a_terminal_chooses_its_text_and_letting_go_copies_it() {
    use crate::terminal::Terminal;
    let mut app = test_app();
    app.show_bottom_terminal = true;
    app.bottom_terminal.tabs = vec![Terminal::unavailable("hello world\r\nsecond line".into())];
    // Inside the border the text starts at column 31, row 23.
    app.regions.bottom_terminal = Rect::new(30, 22, 60, 10);
    let left = crossterm::event::MouseButton::Left;
    mouse(&mut app, MouseEventKind::Down(left), 31, 23);
    mouse(&mut app, MouseEventKind::Drag(left), 35, 23);
    mouse(&mut app, MouseEventKind::Up(left), 35, 23);
    assert_eq!(app.focus, Focus::BottomTerminal);
    assert_eq!(app.clipboard.take().as_deref(), Some("hello"));
    assert_eq!(app.status, "Copied 5 characters");
    assert!(
        app.terminal_selection.is_some(),
        "the choice stays on screen"
    );
    // A key goes to the shell and the choice goes.
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(app.terminal_selection, None);

    // Backwards over two rows.
    mouse(&mut app, MouseEventKind::Down(left), 36, 24);
    mouse(&mut app, MouseEventKind::Drag(left), 37, 23);
    mouse(&mut app, MouseEventKind::Up(left), 37, 23);
    assert_eq!(app.clipboard.take().as_deref(), Some("world\nsecond"));
    // Past the border the drag stays inside it; the empty rows are left out.
    mouse(&mut app, MouseEventKind::Down(left), 31, 23);
    mouse(&mut app, MouseEventKind::Drag(left), 200, 100);
    mouse(&mut app, MouseEventKind::Up(left), 200, 100);
    assert_eq!(
        app.clipboard.take().as_deref(),
        Some("hello world\nsecond line")
    );
    assert_eq!(
        app.terminal_selection.map(|selection| selection.end),
        Some((7, 57))
    );
    // The wheel scrolls the history and the choice goes; a click chooses nothing.
    mouse(&mut app, MouseEventKind::ScrollUp, 40, 25);
    assert_eq!(app.terminal_selection, None);
    mouse(&mut app, MouseEventKind::Down(left), 40, 25);
    mouse(&mut app, MouseEventKind::Up(left), 40, 25);
    assert_eq!((app.clipboard.take(), app.terminal_selection), (None, None));
}

#[test]
fn terminal_tabs_add_close_and_switch_with_their_keys() {
    let mut app = test_app();
    app.show_bottom_terminal = true;
    app.focus = Focus::BottomTerminal;
    for _ in 0..4 {
        app.add_terminal(Focus::BottomTerminal);
    }
    assert_eq!(app.bottom_terminal.tabs.len(), 3, "three at most");
    assert_eq!(app.bottom_terminal.active, 2);
    press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(app.bottom_terminal.active, 0);
    press(&mut app, KeyCode::PageUp, KeyModifiers::CONTROL);
    assert_eq!(app.bottom_terminal.active, 2);
    for _ in 0..4 {
        app.close_terminal(Focus::BottomTerminal, 0);
    }
    assert_eq!(app.bottom_terminal.tabs.len(), 1, "the last one stays");
    // The right terminal starts with two tabs and takes a third like the bottom one.
    assert_eq!(app.right_terminal.tabs.len(), 2);
    app.add_terminal(Focus::RightTerminal);
    assert_eq!(app.right_terminal.tabs.len(), 3);
    app.close_terminal(Focus::RightTerminal, 2);
    app.right_terminal.select(0);
    app.show_right_terminal = true;
    app.focus = Focus::RightTerminal;
    press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(
        (app.right_terminal.active, app.bottom_terminal.active),
        (1, 0)
    );
    // Outside a terminal the keys reach the editor.
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(app.right_terminal.active, 1);
}

#[test]
fn f3_finds_in_the_history_shift_f3_clears_it_and_a_tab_takes_a_name() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    app.focus = Focus::BottomTerminal;
    // The shell's own output (from `/bin/sh` or `cmd.exe`) may come at any time: only what the
    // test wrote is looked for.
    let mut output = String::new();
    for number in 0..60 {
        output.push_str(&format!("line {number}\r\n"));
    }
    output.push_str("the needle is here\r\n");
    program_in(&mut app, Focus::BottomTerminal, output.as_bytes());
    press(&mut app, KeyCode::F(3), KeyModifiers::NONE);
    for character in "LINE 1".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    let search = app.bottom_terminal.search.clone().unwrap();
    assert_eq!(search.matches.len(), 11, "line 1 and line 10 to 19");
    assert_eq!(search.index, 10, "the newest first");
    // Enter goes to older ones and scrolls the history to them.
    for _ in 0..10 {
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    }
    assert_eq!(app.bottom_terminal.search.as_ref().unwrap().index, 0);
    assert!(app.bottom_terminal.current().scrollback() > 0);
    draw(&mut app, &mut terminal);
    let text = terminal_lines(&terminal).join("\n");
    assert!(text.contains("Find: LINE 1_  1/11"), "{text}");
    // Esc closes it and the shell comes back live; the keys reach it again.
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.bottom_terminal.search.is_none());
    assert_eq!(app.bottom_terminal.current().scrollback(), 0);
    // Shift+F3 drops the history.
    press(&mut app, KeyCode::F(3), KeyModifiers::SHIFT);
    let lines = app.bottom_terminal.current().lines();
    assert!(!lines.iter().any(|line| line == "line 0"), "{lines:?}");
    // A program's title shows on its tab; a right click names the tab, which wins.
    program_in(&mut app, Focus::BottomTerminal, b"\x1b]0;vim\x07");
    assert_eq!(app.bottom_terminal.label(0), "1 vim");
    draw(&mut app, &mut terminal);
    let (_, _, tab, _) = app.regions.terminal_tabs[0];
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        tab.x + 1,
        tab.y,
    );
    assert_eq!(app.bottom_terminal.renaming.as_deref(), Some(""));
    for character in "build\u{7}".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        app.bottom_terminal.label(0),
        "1 build",
        "no control characters"
    );
    // Shift+F6 splits: two shells, the divider between them; again, one.
    press(&mut app, KeyCode::F(6), KeyModifiers::SHIFT);
    assert!(app.bottom_terminal.split && app.bottom_terminal.tabs.len() >= 2);
    draw(&mut app, &mut terminal);
    press(&mut app, KeyCode::F(6), KeyModifiers::SHIFT);
    assert!(!app.bottom_terminal.split);
}

#[test]
fn ctrl_click_opens_a_file_of_the_project_or_a_web_address_and_nothing_else() {
    let (mut app, mut terminal) = drawn(120, 40);
    show_all(&mut app, &mut terminal);
    let area = app.regions.bottom_terminal;
    // A clean screen with only what the test writes on its first rows.
    program_in(
        &mut app,
        Focus::BottomTerminal,
        b"\x1b[2J\x1b[H  --> src/main.rs:3:2\r\nsee https://example.com/x\r\nbad javascript:alert(1)\r\n\x1b]8;;javascript:alert(1)\x1b\\click\x1b]8;;\x1b\\\r\n",
    );
    let ctrl_click = |app: &mut App, column: u16, row: u16| {
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 1 + column,
            row: area.y + 1 + row,
            modifiers: KeyModifiers::CONTROL,
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: area.x + 1 + column,
            row: area.y + 1 + row,
            modifiers: KeyModifiers::CONTROL,
        });
    };
    ctrl_click(&mut app, 8, 0);
    let file = app.active_file().unwrap();
    assert!(file.path.ends_with("src/main.rs"), "{:?}", file.path);
    assert_eq!((file.cursor_row, file.cursor_column), (2, 1));
    assert_eq!(app.focus, Focus::Editor);
    ctrl_click(&mut app, 8, 1);
    assert_eq!(app.status, "Opened https://example.com/x");
    for row in [2, 3] {
        app.status.clear();
        let tabs = app.open_files.len();
        ctrl_click(&mut app, 6, row);
        assert!(
            !app.status.starts_with("Opened"),
            "row {row}: {}",
            app.status
        );
        assert_eq!(app.open_files.len(), tabs);
    }
}
