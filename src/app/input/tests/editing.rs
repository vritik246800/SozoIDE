//! The editing of the Onda 2 (TODO 2.1–2.3) through the keys and the mouse.

use super::*;

fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = directory.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let app = App::new(directory.path().to_path_buf()).unwrap();
    (directory, app)
}

#[test]
fn go_to_line_redo_and_comment_answer_their_keys() {
    let (directory, mut app) = project(&[("a.rs", "one\ntwo\nthree\n")]);
    app.open_file(directory.path().join("a.rs")).unwrap();
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
    assert!(app.go_to_line.is_some());
    for key in ['3', ':', ':', '2', 'x'] {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    assert_eq!(
        app.go_to_line.as_deref(),
        Some("3:2"),
        "one `:` and digits only"
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let file = app.active_file().unwrap();
    assert_eq!((file.cursor_row, file.cursor_column), (2, 1));
    // Past the end: the last line.
    app.go_to("999");
    assert_eq!(app.active_file().unwrap().cursor_row, 2);
    // Ctrl+/ comments; Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z.
    press(&mut app, KeyCode::Char('/'), KeyModifiers::CONTROL);
    assert_eq!(app.active_file().unwrap().lines[2], "// three");
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.active_file().unwrap().lines[2], "three");
    press(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(app.active_file().unwrap().lines[2], "// three");
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    press(
        &mut app,
        KeyCode::Char('Z'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(app.active_file().unwrap().lines[2], "// three");
    press(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(app.status, "Nothing to redo");
}

#[test]
fn the_finder_opens_files_by_letters_and_the_palette_runs_commands() {
    let (_directory, mut app) = project(&[
        ("src/main.rs", ""),
        ("src/my_animation.rs", ""),
        ("README.md", ""),
    ]);
    press(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.finder.as_ref().unwrap().indexing() {
        assert!(std::time::Instant::now() < deadline);
        app.tick();
    }
    for key in "main".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    let first = app.finder.as_ref().unwrap().file_results()[0].clone();
    assert_eq!(first, std::path::PathBuf::from("src/main.rs"));
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.finder.is_none());
    assert!(app.active_file().unwrap().path.ends_with("src/main.rs"));
    // The palette: "about" runs About as if F2 was pressed.
    press(
        &mut app,
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(
        app.finder.as_ref().unwrap().mode,
        crate::app::FinderMode::Commands
    );
    for key in "about".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    let (_, name, key) = app.command_results()[0].clone();
    assert_eq!(name, "About");
    assert!(key.contains("F2"));
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.overlay, Overlay::About);
    // Esc closes without opening anything.
    app.overlay = Overlay::None;
    press(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.finder.is_none());
}

#[test]
fn search_options_and_replace_in_the_file_are_one_undo_and_the_workspace_asks() {
    let (directory, mut app) = project(&[("a.txt", "Foo foo food\n"), ("b.txt", "foo\n")]);
    app.open_file(directory.path().join("a.txt")).unwrap();
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
    let search = app.search.as_ref().unwrap();
    assert!(search.replace.is_some() && search.in_replace);
    // Tab to the query, type, Alt+W for whole words, Tab back to the replace row.
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    for key in "foo".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    assert_eq!(app.search.as_ref().unwrap().matches.len(), 3);
    press(&mut app, KeyCode::Char('w'), KeyModifiers::ALT);
    assert_eq!(app.search.as_ref().unwrap().matches.len(), 2);
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    for key in "bar".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    // Enter replaces the chosen one, Ctrl+Enter the rest; one Ctrl+Z each.
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.active_file().unwrap().lines[0], "bar foo food");
    press(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(app.active_file().unwrap().lines[0], "bar bar food");
    app.close_search();
    app.tick_at(std::time::Instant::now() + std::time::Duration::from_secs(1));
    app.focus = Focus::Editor;
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.active_file().unwrap().lines[0], "bar foo food");

    // The workspace: Ctrl+Enter only asks; Esc writes nothing; Enter writes both files.
    app.flush_all_files();
    app.open_search(SearchScope::Workspace);
    app.search
        .as_mut()
        .unwrap()
        .push_str("foo", directory.path(), None);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.search.as_ref().unwrap().searching {
        assert!(std::time::Instant::now() < deadline);
        app.search.as_mut().unwrap().poll();
    }
    press(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
    for key in "qux".chars() {
        press(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(app.replace_plan.is_some());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("b.txt")).unwrap(),
        "foo\n"
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("b.txt")).unwrap(),
        "qux\n"
    );
    // A new search has no options on: `food` matches too.
    assert_eq!(
        app.active_file().unwrap().lines[0],
        "bar qux quxd",
        "the open tab read again"
    );
}

#[test]
fn two_clicks_select_a_word_and_three_the_line() {
    use crossterm::event::MouseButton;
    let (directory, mut app) = project(&[("a.txt", "hello world\nnext\n")]);
    app.open_file(directory.path().join("a.txt")).unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    draw(&mut app, &mut terminal);
    let body = app.regions.pane_bodies[0];
    let gutter = 1 + 3;
    let (x, y) = (body.x + 1 + gutter + 8, body.y + 1);
    for _ in 0..2 {
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);
    }
    assert_eq!(
        app.active_file().unwrap().selected_text().as_deref(),
        Some("world")
    );
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(
        app.active_file().unwrap().selected_text().as_deref(),
        Some("hello world\n")
    );
}

#[test]
fn a_pdf_is_searched_on_every_page_goes_to_a_page_and_zooms_a_drawn_one() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.pdf");
    let page = |text: &str| format!("BT /F1 10 Tf 5 80 Td ({text}) Tj ET");
    let (one, two) = (page("First chapter"), page("The needle is here"));
    std::fs::write(
        &path,
        crate::editor::sample_pdf_with_outline(
            &[&one, &two, "0 0 1 rg 20 20 60 60 re f"],
            &[("Start", 0), ("Drawing", 2)],
        ),
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    app.focus = Focus::Editor;
    // Ctrl+P finds on the second page while the first is on screen, and shows that page.
    press(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    for character in "needle".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    let search = app.search.as_ref().unwrap();
    assert_eq!(search.matches.len(), 1);
    assert!(
        search.matches[0].text.starts_with("p. 2"),
        "{}",
        search.matches[0].text
    );
    app.preview_match();
    assert_eq!(app.active_file().unwrap().page().unwrap().0, 1);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    // Ctrl+L takes a page number.
    press(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('3'), KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.active_file().unwrap().page().unwrap().0, 2);
    // The third page is a drawing: + zooms it, 0 fits it again; a text page does not zoom.
    press(&mut app, KeyCode::Char('+'), KeyModifiers::NONE);
    assert!(app.active_file().unwrap().pdf.as_ref().unwrap().zoom > 1.0);
    press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    assert!(app.active_file().unwrap().pdf.as_ref().unwrap().centre.0 > 0.5);
    press(&mut app, KeyCode::Char('0'), KeyModifiers::NONE);
    assert_eq!(app.active_file().unwrap().pdf.as_ref().unwrap().zoom, 1.0);
    press(&mut app, KeyCode::Home, KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('+'), KeyModifiers::NONE);
    assert_eq!(app.active_file().unwrap().pdf.as_ref().unwrap().zoom, 1.0);
    // Ctrl+O with @ lists its table of contents; Enter goes to the page.
    press(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('@'), KeyModifiers::NONE);
    for character in "draw".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.active_file().unwrap().page().unwrap().0, 2);
}

#[test]
fn named_tasks_the_test_under_the_cursor_and_a_build_s_problems_wait_for_their_dialog() {
    let (directory, mut app) = project(&[
        (
            ".sozo.toml",
            "[tasks.lint]\nprogram = \"cargo\"\nargs = [\"clippy\"]\n",
        ),
        (
            "src/lib.rs",
            "#[test]\nfn adds_two() {\n    assert_eq!(2, 1 + 1);\n}\n",
        ),
    ]);
    app.focus = Focus::Editor;
    // Alt+F7 lists the named tasks; Enter shows the command and waits.
    press(&mut app, KeyCode::F(7), KeyModifiers::ALT);
    assert_eq!(app.finder.as_ref().unwrap().tasks.len(), 1);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let confirm = app.confirm.as_ref().unwrap();
    assert_eq!(confirm.line, "cargo clippy");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(
        app.confirm.is_none() && !app.task_running,
        "Esc runs nothing"
    );
    // Alt+F8: the last one again, asked again.
    press(&mut app, KeyCode::F(8), KeyModifiers::ALT);
    assert_eq!(app.confirm.as_ref().unwrap().line, "cargo clippy");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    // Alt+F9 in a test: that test alone.
    app.open_file(directory.path().join("src/lib.rs")).unwrap();
    app.open_files[0].move_cursor_to(2, 0);
    press(&mut app, KeyCode::F(9), KeyModifiers::ALT);
    assert_eq!(
        app.confirm.as_ref().unwrap().line,
        "cargo test adds_two -- --nocapture"
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    // What a build printed in the bottom terminal is in the Problems list, and opens there.
    program_in(
        &mut app,
        Focus::BottomTerminal,
        b"\r\nerror[E0308]: mismatched types\r\n  --> src/lib.rs:3:5\r\n",
    );
    press(
        &mut app,
        KeyCode::Char('m'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let rows = app.finder_rows();
    assert!(
        rows.iter().any(|row| matches!(row, crate::app::FinderRow::Problem(_, problem) if problem.message == "mismatched types (build)" && problem.row == 2)),
        "{}",
        rows.len()
    );
}

#[test]
fn the_same_file_in_both_panes_stays_the_same_and_two_files_compare_side_by_side() {
    let (directory, mut app) = project(&[
        ("a.txt", "one\ntwo\nthree\n"),
        ("b.txt", "one\n2\nthree\nfour\n"),
    ]);
    app.open_file(directory.path().join("a.txt")).unwrap();
    app.focus = Focus::Editor;
    // Alt+S: the file also in the other pane.
    press(&mut app, KeyCode::Char('s'), KeyModifiers::ALT);
    assert_eq!(app.open_files.len(), 2);
    assert!(app.split.is_some());
    assert_ne!(app.open_files[0].pane, app.open_files[1].pane);
    // Typed in one, the other follows; only the one typed in saves.
    app.select_tab(0);
    press(&mut app, KeyCode::Char('X'), KeyModifiers::SHIFT);
    app.tick();
    assert_eq!(app.open_files[1].lines[0], app.open_files[0].lines[0]);
    assert!(app.open_files[0].lines[0].contains('X') && !app.open_files[1].dirty);
    // Alt+D: compare with b.txt, side by side.
    press(&mut app, KeyCode::Char('d'), KeyModifiers::ALT);
    assert!(app.finder.as_ref().unwrap().compare);
    for character in "b.txt".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    app.finder.as_mut().unwrap().query = "b.txt".into();
    // The file list comes from a thread: wait for it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.finder_rows().is_empty() && std::time::Instant::now() < deadline {
        app.tick();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let comparison = app.comparison.as_ref().expect("a comparison");
    assert!(comparison.title.contains("a.txt") && comparison.title.contains("b.txt"));
    assert!(
        comparison
            .rows
            .iter()
            .any(|(left, right)| left.as_deref() == Some("two") && right.as_deref() == Some("2")),
        "{:?}",
        comparison.rows
    );
    assert!(
        comparison
            .rows
            .iter()
            .any(|(left, right)| left.is_none() && right.as_deref() == Some("four"))
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.comparison.is_none());
}
