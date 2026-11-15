//! The editor: Markdown, PDF, selection, search, problems, Code Intelligence and its margin.

use super::*;

#[test]
fn clicking_a_markdown_table_switches_between_render_and_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("README.md");
    std::fs::write(
        &path,
        "# Demo\n| Name | Age |\n| --- | ---: |\n| Ana | 30 |\nAfter\n",
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    settle(&mut app);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let content = buffer_text(&terminal);
    assert!(content.contains("│ Name │ Age │"));
    assert!(!content.contains("| Name | Age |"));

    let area = app.regions.editor;
    let gutter = app.open_files[0].lines.len().to_string().len() as u16 + 3;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: area.x + 1 + gutter + 2,
        row: area.y + 1 + 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.open_files[0].cursor_row, 3);
    assert_eq!(app.open_files[0].cursor_column, 2);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let content = buffer_text(&terminal);
    assert!(content.contains("| Ana | 30 |"));
    assert!(!content.contains('├'));

    for _ in 0..3 {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("│ Name │ Age │"));
}

#[test]
fn markdown_shows_its_source_under_the_mouse() {
    use crossterm::event::MouseEventKind;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.md");
    std::fs::write(&path, "# Notes\nSome **bold** and ==marked== words\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let area = app.regions.editor;
    let line = |terminal: &Terminal<TestBackend>| row(terminal.backend().buffer(), area.y + 2);
    assert!(line(&terminal).contains("Some bold and marked words"));

    click(&mut app, MouseEventKind::Moved, area.x + 10, area.y + 2);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(line(&terminal).contains("Some **bold** and ==marked== words"));
    click(&mut app, MouseEventKind::Moved, area.x + 10, area.y + 10);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(line(&terminal).contains("Some bold and"));
}

#[test]
fn the_editor_draws_its_selection_and_pdf_pages() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("notes.txt"), "select me\n").unwrap();
    std::fs::write(
        directory.path().join("guide.pdf"),
        crate::editor::sample_pdf(2),
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("notes.txt")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    for _ in 0..6 {
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    }
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let area = app.regions.editor;
    let buffer = terminal.backend().buffer();
    let y = area.y + 1;
    let line = row(buffer, y);
    let x = line[..line.find("select me").unwrap()].chars().count() as u16;
    let reversed = (x..x + 9)
        .map(|x| buffer[(x, y)].modifier.contains(Modifier::REVERSED))
        .collect::<Vec<_>>();
    assert_eq!(
        reversed,
        [true, true, true, true, true, true, false, false, false]
    );

    app.open_file(directory.path().join("guide.pdf")).unwrap();
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Page 1/2"));
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Page 2/2"));
}

#[test]
fn a_page_of_text_of_a_pdf_is_drawn_as_text_with_its_page() {
    let directory = tempfile::tempdir().unwrap();
    let text = "BT /F1 10 Tf 5 80 Td (Hello PDF) Tj ET";
    std::fs::write(
        directory.path().join("notes.pdf"),
        crate::editor::sample_pdf_pages(&[text, text]),
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("notes.pdf")).unwrap();
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("1 | Hello PDF") && text.contains("Page 1/2"));
}

#[test]
fn a_pdf_is_drawn_whole_in_the_first_frame_of_its_tab() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("notes.txt"), "old text\n".repeat(40)).unwrap();
    std::fs::write(
        directory.path().join("guide.pdf"),
        crate::editor::sample_pdf(1),
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("notes.txt")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    app.open_file(directory.path().join("guide.pdf")).unwrap();
    // The first frame of the tab, while a text tab would still be sweeping in.
    app.tick_at(std::time::Instant::now());
    assert!(app.motion.content() < 1.0);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let area = app.regions.editor;
    let buffer = terminal.backend().buffer();
    let rows = (area.y + 1..area.bottom() - 1)
        .map(|y| row(buffer, y))
        .collect::<Vec<_>>();
    assert!(rows.iter().all(|line| !line.contains("old text")));
}

#[test]
fn problems_are_marked_in_the_editor_and_said_in_the_footer_with_the_suggestions() {
    use crate::lsp::{Diagnostic, Event, Item};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.c");
    std::fs::write(&path, "int main() {\n  retur 0;\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    settle(&mut app);
    app.status = "Opened main.c".into();
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(
        buffer_text(&terminal).contains("Opened main.c"),
        "the status shows"
    );
    app.lsp.diagnostics.insert(
        path.clone(),
        vec![Diagnostic {
            row: 1,
            start: 2,
            end_row: 1,
            end: 7,
            error: true,
            message: "use of undeclared identifier 'retur'".into(),
        }],
    );
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains("error: use of undeclared identifier 'retur'"),
        "{text}"
    );
    let buffer = terminal.backend().buffer();
    let (x, y) = (0..20)
        .find_map(|y| row(buffer, y).find("retur").map(|x| (x, y)))
        .unwrap();
    let x = row(buffer, y)[..x].chars().count() as u16;
    let cell = &buffer[(x, y)];
    assert!(cell.modifier.contains(ratatui::style::Modifier::UNDERLINED));
    assert_eq!(cell.underline_color, ratatui::style::Color::Red);
    let number = row(buffer, y).find("2 |").unwrap();
    let number = row(buffer, y)[..number].chars().count() as u16;
    assert_eq!(buffer[(number, y)].fg, ratatui::style::Color::Red);

    // Suggestions open under the cursor with their count.
    app.lsp_event(Event::Completion {
        path,
        row: 1,
        column: 0,
        items: vec![Item {
            label: "return".into(),
            detail: "keyword".into(),
            insert: "return".into(),
        }],
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains(" return") && text.contains("keyword") && text.contains("1/1"));
}

#[test]
fn hover_peek_and_references_are_drawn_by_the_symbol() {
    use crate::lsp::{Event, Goal, Place};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.rs");
    let lib = directory.path().join("lib.rs");
    std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
    std::fs::write(&lib, "pub fn compute() -> u32 {\n    1\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(main.clone()).unwrap();
    settle(&mut app);
    app.open_files[0].move_cursor_to(1, 9);
    let press = |app: &mut App, code, modifiers| {
        app.handle_key(KeyEvent::new(code, modifiers));
    };
    let version = app.active_file().unwrap().edits;
    let hover = |app: &mut App, text: &str| {
        press(app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        app.lsp_event(Event::Hover {
            path: main.clone(),
            version,
            row: 1,
            column: 8,
            text: text.into(),
        });
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    hover(
        &mut app,
        "```rust\nlet total: u32\n```\n\nThe **sum** of the order.",
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let find = |text: &str| {
        (0..24).find_map(|y| {
            let line = row(&buffer, y);
            line.find(text)
                .map(|x| (line[..x].chars().count() as u16, y))
        })
    };
    let (_, symbol) = find("let total = compute();").unwrap();
    let (_, signature) = find("let total: u32").unwrap();
    let (x, sum) = find("The sum of the order.").unwrap();
    assert!(
        signature > symbol && sum > signature,
        "under the symbol's row"
    );
    assert!(
        buffer[(x + 4, sum)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    let text = buffer_text(&terminal);
    assert!(
        !text.contains("**") && !text.contains("```"),
        "drawn, not the marks"
    );
    assert_eq!(app.regions.hover.y, symbol + 1);

    // A long one scrolls with the arrows and says so on its border.
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    let long = (1..=30).map(|n| format!("line {n}")).collect::<Vec<_>>();
    hover(&mut app, &long.join("\n\n"));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("line 1 ") && text.contains("↑/↓"));
    assert!(app.hover.as_ref().unwrap().overflow > 0);
    assert_eq!(
        app.regions.hover.height, 12,
        "small as the menu: ten lines (Q72)"
    );
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(!buffer_text(&terminal).contains("line 1 "));
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    hover(&mut app, &"wide ".repeat(30));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_eq!(app.regions.hover.width, 60, "at most 60 columns");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    // Code wider than the prose keeps its frame whole (E49), as rust-analyzer answers.
    hover(
        &mut app,
        "```rust\nrproj::lib2\n```\n\n```rust\npub fn counter(x: i32) -> i32\n```",
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("│ pub fn counter(x: i32) -> i32 │"), "{text}");
    // No frame is broken onto a second row inside the box.
    let area = app.regions.hover;
    let buffer = terminal.backend().buffer();
    for y in area.y + 1..area.bottom() - 1 {
        let inside = (area.x + 1..area.right() - 1)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        assert!(!inside.starts_with('─'), "a broken frame: {inside}");
    }
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    // Peek: the file and line in the title, numbered and highlighted lines.
    press(&mut app, KeyCode::F(12), KeyModifiers::ALT);
    let place = |path: &std::path::PathBuf, row, start, end| Place {
        path: path.clone(),
        row,
        start,
        end_row: row,
        end,
    };
    app.lsp_event(Event::Places {
        path: main.clone(),
        version,
        goal: Goal::Peek,
        places: vec![place(&lib, 0, 7, 14)],
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "Peek ─ lib.rs:1",
        "1 │ pub fn compute() -> u32 {",
        "Enter: go",
    ] {
        assert!(text.contains(part), "{part}");
    }
    let buffer = terminal.backend().buffer();
    let y = (0..24)
        .find(|y| row(buffer, *y).contains("1 │ pub fn"))
        .unwrap();
    let x = row(buffer, y).find("1 │ pub fn").unwrap();
    let x = row(buffer, y)[..x].chars().count() as u16;
    assert_eq!(buffer[(x, y)].fg, ACTIVE, "the definition's row");
    assert!(y > app.regions.editor.y + 2, "under the cursor's row");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    // References: the workspace list with its own title and the symbol.
    app.lsp_event(Event::Places {
        path: main.clone(),
        version,
        goal: Goal::References,
        places: vec![place(&main, 1, 16, 23), place(&lib, 0, 7, 14)],
    });
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        " References ",
        "> compute",
        "2 results in 2 files",
        "pub fn compute() -> u32 {",
        "let total = compute();",
    ] {
        assert!(text.contains(part), "{part}");
    }
    assert!(!text.contains("Find in workspace") && !text.contains("compute█"));

    // Ctrl over a word underlines it as a link.
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    settle(&mut app);
    app.link = Some((0, 1, 8, 13));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let y = (0..24)
        .find(|y| row(buffer, *y).contains("let total = compute();"))
        .unwrap();
    let x = row(buffer, y).find("total").unwrap();
    let x = row(buffer, y)[..x].chars().count() as u16;
    let underlined = |x| {
        buffer[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    };
    assert!((x..x + 5).all(underlined) && !underlined(x + 5) && !underlined(x - 1));
}

#[test]
fn the_margin_marks_the_lines_git_sees_added_changed_or_deleted() {
    use crate::git::LineMark;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.rs");
    std::fs::write(&path, "fn main() {\n    let a = 1;\n    let b = 2;\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    settle(&mut app);
    let edits = app.open_files[0].edits;
    app.line_marks.insert(
        path.clone(),
        (
            edits,
            [
                (0, LineMark::DeletedAbove),
                (1, LineMark::Modified),
                (2, LineMark::Added),
                (3, LineMark::DeletedBelow),
            ]
            .into(),
        ),
    );
    app.breakpoints.insert(path.clone(), [1].into());
    let mut terminal = Terminal::new(TestBackend::new(110, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    // In the place of the bar, the breakpoint still before it; the width stays.
    let cases = [
        ("1 ▔ fn main() {", '▔', ratatui::style::Color::Red),
        ("2●▎     let a = 1;", '▎', ratatui::style::Color::Yellow),
        ("3 ▎     let b = 2;", '▎', ratatui::style::Color::Green),
        ("4 ▁ }", '▁', ratatui::style::Color::Red),
    ];
    for (text, glyph, colour) in cases {
        let y = (0..24)
            .find(|y| row(buffer, *y).contains(text))
            .unwrap_or_else(|| panic!("{text}"));
        let at = row(buffer, y).find(text).unwrap() + text.find(glyph).unwrap();
        let x = row(buffer, y)[..at].chars().count() as u16;
        assert_eq!(buffer[(x, y)].symbol(), glyph.to_string());
        assert_eq!(buffer[(x, y)].fg, colour, "{text}");
    }
    // Without marks, the bar.
    app.line_marks.clear();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("3 |     let b = 2;"));
}

#[test]
fn the_margin_marks_breakpoints_and_the_paused_line_and_the_panel_shows_the_session() {
    use crate::debug::{Frame, State, fake};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.c");
    std::fs::write(&path, "int main(void) {\n  int x = 1;\n  return x;\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(path.clone()).unwrap();
    settle(&mut app);
    app.breakpoints.insert(path.clone(), [1].into());
    let frame = Frame {
        id: 1,
        name: "main".into(),
        path: Some(path.clone()),
        row: 2,
    };
    app.debug = Some(fake(
        State::Paused("breakpoint".into()),
        vec![frame],
        vec![("x".into(), "1".into())],
    ));
    let mut terminal = Terminal::new(TestBackend::new(110, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "2●|   int x = 1;",
        "3▶|   return x;",
        "╭ Debug ",
        "Paused: breakpoint",
        "Call stack",
        "main  main.c:3",
        "x = 1",
        "Output",
        "hello",
    ] {
        assert!(text.contains(part), "{part}");
    }
    let buffer = terminal.backend().buffer();
    let y = (0..24).find(|y| row(buffer, *y).contains("2●|")).unwrap();
    let x = row(buffer, y).find('●').unwrap();
    let x = row(buffer, y)[..x].chars().count() as u16;
    assert_eq!(buffer[(x, y)].fg, ratatui::style::Color::Red);
    app.debug = None;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(!buffer_text(&terminal).contains("╭ Debug "));
}

#[test]
fn the_menu_preview_is_drawn_beside_its_entry_with_the_answers_or_why_not() {
    use crate::{
        app::PreviewRow,
        lsp::{Event, Failure, Goal, Place},
    };
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.rs");
    let lib = directory.path().join("lib.rs");
    std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
    std::fs::write(&lib, "pub fn compute() -> u32 {\n    1\n}\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(main.clone()).unwrap();
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let mouse = |app: &mut App, kind, (column, row): (u16, u16)| {
        app.handle_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    };
    let body = app.regions.pane_bodies[0];
    // The right-click on «compute» (row 1, column 18).
    let word = (body.x + 1 + 4 + 18, body.y + 2);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), word);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let entry = |app: &App, index: usize| app.regions.entry_menu_items[index].1;
    let menu_right = entry(&app, 1).right() + 1;
    let text = buffer_text(&terminal);
    assert!(text.contains(" Go to Definition      ▸"), "{text}");
    assert!(
        text.contains(" Go to Type Definition ▸ │"),
        "room before the border"
    );
    assert!(!text.contains(" Paste                 ▸"));
    // On Go to Definition: the spinner, then the answers beside the menu, at its height.
    let definition = entry(&app, 1);
    mouse(
        &mut app,
        MouseEventKind::Moved,
        (definition.x + 2, definition.y),
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Looking…"));
    let version = app.active_file().unwrap().edits;
    let place = |path: &std::path::Path, row, start, end| Place {
        path: path.to_path_buf(),
        row,
        start,
        end_row: row,
        end,
    };
    let answer = |app: &mut App, goal, places| {
        app.lsp_event(Event::Places {
            path: main.clone(),
            version,
            goal,
            places,
        })
    };
    answer(
        &mut app,
        Goal::Definition,
        vec![place(&lib, 0, 7, 14), place(&main, 1, 16, 23)],
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let find = |text: &str| {
        (0..30).find_map(|y| {
            let line = row(&buffer, y);
            line.find(text)
                .map(|x| (line[..x].chars().count() as u16, y))
        })
    };
    let (x, title) = find("Definitions (2)").unwrap();
    assert_eq!(
        title,
        definition.y - 1,
        "the box starts at the entry's height"
    );
    assert!(x > menu_right, "right of the menu");
    let (first_x, first) = find("lib.rs:1  pub fn compute() -> u32 {").unwrap();
    assert_eq!(first, definition.y);
    assert!(find("main.rs:2  let total = compute();").is_some());
    assert_eq!(buffer[(first_x, first)].fg, MUTED);
    let rows = app
        .regions
        .preview_rows
        .iter()
        .map(|(row, _)| *row)
        .collect::<Vec<_>>();
    assert_eq!(rows, [PreviewRow::Place(0), PreviewRow::Place(1)]);
    // The answer under the mouse in ACTIVE inverse.
    let second = app.regions.preview_rows[1].1;
    mouse(&mut app, MouseEventKind::Moved, (second.x + 3, second.y));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let cell = &terminal.backend().buffer()[(second.x + 3, second.y)];
    assert!(cell.modifier.contains(Modifier::REVERSED) && cell.fg == ACTIVE);

    // Peek: the definition's file and line in the title, its lines numbered.
    let peek = entry(&app, 2);
    mouse(&mut app, MouseEventKind::Moved, (peek.x + 2, peek.y));
    answer(&mut app, Goal::Peek, vec![place(&lib, 0, 7, 14)]);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains(" lib.rs:1 ") && text.contains("1 │ pub fn compute() -> u32 { "));
    // Nothing found, and a server that could not start: why, and Install.
    let declaration = entry(&app, 3);
    mouse(
        &mut app,
        MouseEventKind::Moved,
        (declaration.x + 2, declaration.y),
    );
    answer(&mut app, Goal::Declaration, Vec::new());
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("No declaration found"));
    let why = "error: Unknown binary 'rust-analyzer' in official toolchain 'stable-aarch64-apple-darwin'.";
    app.lsp.fail("rust-analyzer", Failure::Start(why.into()));
    let types = entry(&app, 4);
    mouse(&mut app, MouseEventKind::Moved, (types.x + 2, types.y));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("rust-analyzer could not start: error: Unknown binary"));
    assert!(
        text.contains("'stable-aarch64-apple-darwin'."),
        "wrapped, not cut"
    );
    assert!(text.contains(" Install "));
    assert_eq!(
        app.regions
            .preview_rows
            .iter()
            .map(|(row, _)| *row)
            .collect::<Vec<_>>(),
        [PreviewRow::Install]
    );

    // Without room on the right it goes on the left.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), (0, 0));
    let far = (body.right() - 2, body.y + 2);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), far);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let types = entry(&app, 4);
    mouse(&mut app, MouseEventKind::Moved, (types.x + 2, types.y));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let install = app.regions.preview_rows[0].1;
    assert!(install.right() < types.x, "left of the menu");
}

#[test]
fn the_editor_menu_opens_at_the_mouse_in_the_text() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.txt"), "alpha\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("a.txt")).unwrap();
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let body = app.regions.pane_bodies[0];
    let (column, row) = (body.x + 10, body.y + 3);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let items = app
        .regions
        .entry_menu_items
        .iter()
        .map(|(action, area)| (action.label(), area.x, area.y))
        .collect::<Vec<_>>();
    assert_eq!(items[0], ("Paste", column + 1, row + 1));
    let labels = items.iter().map(|item| item.0).collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "Paste",
            "Go to Definition",
            "Peek Definition",
            "Go to Declaration",
            "Go to Type Definition",
            "Go to Implementations",
            "Find References"
        ]
    );
    assert!(buffer_text(&terminal).contains(" Paste"));
    assert!(
        buffer_text(&terminal).contains(" Go to Type Definition "),
        "whole"
    );
}

#[test]
fn searches_count_and_mark_their_matches_in_the_bar_the_dialog_and_the_editor() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("a.rs"),
        "fn alpha() {}\nlet x = alpha();\n// ALPHA\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("b.txt"), "no match\nalpha\n").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("a.rs")).unwrap();
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    app.search_paste("alpha");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("◂ 2/3 ▸"), "{text}");
    // The editor stays in view: the second match is the current one, the others marked.
    let buffer = terminal.backend().buffer();
    let cell = |needle: &str, nth: usize| {
        let (y, line) = (0..buffer.area.height)
            .map(|y| (y, row(buffer, y)))
            .filter(|(_, line)| line.contains(needle))
            .nth(nth)
            .unwrap();
        let x = line[..line.find(needle).unwrap()].chars().count() as u16;
        buffer[(x, y)].clone()
    };
    assert_eq!(cell("alpha() {}", 0).bg, ratatui::style::Color::Yellow);
    assert_eq!(cell("alpha();", 0).bg, ACTIVE);
    assert_eq!(cell("ALPHA", 0).bg, ratatui::style::Color::Yellow);
    // The arrows of the counter step through the results.
    let next = app.regions.search_next;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: next.x,
        row: next.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.search.as_ref().unwrap().selected, 2);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    settle(&mut app);

    app.handle_key(KeyEvent::new(
        KeyCode::Char('P'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    app.search_paste("alpha");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.search.as_ref().unwrap().searching && std::time::Instant::now() < deadline {
        app.tick();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("4 results in 2 files"), "{text}");
    assert!(
        text.contains("a.rs (3)") && text.contains("b.txt (1)"),
        "{text}"
    );
    assert!(text.contains("◂ 1/4 ▸"));
    assert!(text.contains(" ›     1  fn alpha() {}"), "{text}");
    assert_eq!(app.regions.search_match_items.len(), 4);
    // The term is marked in each line of the list.
    let buffer = terminal.backend().buffer();
    let (y, line) = (0..buffer.area.height)
        .map(|y| (y, row(buffer, y)))
        .find(|(_, line)| line.contains("2  alpha"))
        .unwrap();
    let x = line[..line.find("2  alpha").unwrap()].chars().count() as u16 + 3;
    assert_eq!(buffer[(x, y)].bg, ratatui::style::Color::Yellow);
}
