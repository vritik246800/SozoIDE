//! The drawing of the editing of the Onda 2 (TODO 2.3.1–2.3.5).

use super::*;

fn project(text: &str) -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.rs"), text).unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("a.rs")).unwrap();
    settle(&mut app);
    (directory, app)
}

#[test]
fn the_search_bar_shows_its_options_the_replace_row_and_why_an_expression_fails() {
    let (_directory, mut app) = project("fn a() {}\n");
    app.open_replace();
    settle(&mut app);
    let text = frame(&mut app, 120, 30);
    assert!(
        text.contains(" Aa ") && text.contains(" ab ") && text.contains(" .* "),
        "{text}"
    );
    assert!(text.contains("⇄ █") && text.contains("Ctrl+Enter: all"));
    let search = app.search.as_mut().unwrap();
    search.options.regex = true;
    search.query = "(".into();
    let root = app.root.clone();
    app.search.as_mut().unwrap().refresh(&root, None);
    let text = frame(&mut app, 120, 30);
    assert!(text.contains("unclosed group"), "{text}");
}

#[test]
fn the_finder_go_to_line_and_the_disk_question_draw_what_they_wait_for() {
    let (directory, mut app) = project("one\ntwo\n");
    app.open_finder(crate::app::FinderMode::Commands);
    app.finder.as_mut().unwrap().query = "redo".into();
    let text = frame(&mut app, 120, 30);
    assert!(
        text.contains("Commands") && text.contains("Redo") && text.contains("Ctrl+Y"),
        "{text}"
    );
    app.finder = None;
    app.go_to_line = Some("12".into());
    let text = frame(&mut app, 120, 30);
    assert!(
        text.contains("Go to line") && text.contains("> 12") && text.contains("1 to 2"),
        "{text}"
    );
    app.go_to_line = None;
    app.disk_prompt = Some(directory.path().join("a.rs"));
    app.disk_compare = Some(vec!["@@ -1 +1 @@".into(), "-one".into(), "+uno".into()]);
    let text = frame(&mut app, 120, 30);
    assert!(
        text.contains("changed on disk") && text.contains("K keep mine") && text.contains("+uno"),
        "{text}"
    );
}

#[test]
fn the_status_bar_says_the_line_end_encoding_and_indentation_and_clicks_change_them() {
    let (_directory, mut app) = project("fn a() {\n  b();\n}\n");
    let mut terminal = Terminal::new(TestBackend::new(160, 24)).unwrap();
    terminal.draw(|frame| super::draw(frame, &mut app)).unwrap();
    let footer = row(terminal.backend().buffer(), 23);
    assert!(
        footer.contains("Spaces: 2") && footer.contains(" LF ") && footer.contains("UTF-8"),
        "{footer}"
    );
    let line_end = app.regions.footer_line_end;
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        line_end.x + 1,
        line_end.y,
    );
    assert!(app.active_file().unwrap().crlf);
    let place = app.regions.footer_place;
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        place.x + 1,
        place.y,
    );
    assert!(app.go_to_line.is_some());
}

#[test]
fn the_pair_of_a_bracket_and_the_indentation_guides_are_drawn() {
    let (_directory, mut app) = project("fn a() {\n    if x {\n        y();\n    }\n}\n");
    app.focus = crate::app::Focus::Editor;
    let index = app.active_tab.unwrap();
    app.open_files[index].move_cursor_to(0, 7);
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| super::draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = buffer_text(&terminal);
    assert!(text.contains("│   y();"), "a guide down the level: {text}");
    // The `{` at the cursor and the `}` on the last line are marked.
    let marked = buffer
        .content()
        .iter()
        .filter(|cell| {
            cell.modifier.contains(ratatui::style::Modifier::UNDERLINED)
                && (cell.symbol() == "{" || cell.symbol() == "}")
        })
        .count();
    assert_eq!(marked, 2);
}
