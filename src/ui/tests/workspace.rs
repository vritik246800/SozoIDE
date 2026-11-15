//! The workspace: panels, tabs, the splash and the transitions between them.

use super::*;

#[test]
fn renders_all_panels_and_clickable_tabs() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    app.open_file(app.root.join("Cargo.toml")).unwrap();
    app.open_file(app.root.join("src/main.rs")).unwrap();
    app.show_bottom_terminal = true;
    app.show_right_terminal = true;
    let now = std::time::Instant::now();
    app.tick_at(now);
    app.tick_at(now + std::time::Duration::from_millis(200));
    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(app.regions.explorer.width > 0);
    assert!(app.regions.editor.height > 0);
    assert!(app.regions.bottom_terminal.height > 0);
    assert!(app.regions.right_terminal.width > 0);
    let dividers = &app.regions.tiling.dividers;
    assert_eq!(dividers.len(), 3);
    assert!(dividers[0].handle.width > 0 && dividers[2].handle.height > 0);
    assert_eq!(app.regions.tab_items.len(), 2);
    let content = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(content.contains("SozoIDE"));
    assert!(content.contains("Cargo.toml"));
    assert!(content.contains("main.rs"));
}

#[test]
fn renders_ascii_empty_state_help_about_and_image() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Select a file in Explorer")
    );
    app.open_file(app.root.join("assets/Sozo.png")).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    // Pictures are drawn as half blocks in their own colours (Q43).
    let file = app.active_file().unwrap();
    assert!(file.media.is_some() && file.image.is_none());
    let blocks = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .filter(|cell| {
            matches!(cell.symbol(), "▀" | "▄") && matches!(cell.fg, ratatui::style::Color::Rgb(..))
        })
        .count();
    assert!(blocks > 100, "{blocks}");
    app.overlay = Overlay::Help;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    app.overlay = Overlay::About;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
}

#[test]
fn panel_transition_defers_pty_resize_until_the_final_frame() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    app.show_bottom_terminal = true;
    let initial_size = app.bottom_terminal.current().size();
    let start = std::time::Instant::now();
    app.tick_at(start);
    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_eq!(app.regions.bottom_terminal.height, 0);
    assert_eq!(app.bottom_terminal.current().size(), initial_size);

    app.tick_at(start + std::time::Duration::from_millis(60));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(app.regions.bottom_terminal.height > 0);
    assert!(app.regions.bottom_terminal.height < app.bottom_terminal_height);
    assert_eq!(app.bottom_terminal.current().size(), initial_size);

    app.tick_at(start + crate::animation::PANEL.open);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let area = app.regions.bottom_terminal;
    assert_eq!(area.height, app.bottom_terminal_height);
    assert_eq!(
        app.bottom_terminal.current().size(),
        (
            area.width.saturating_sub(2).max(1),
            area.height.saturating_sub(2).max(1),
        )
    );
}

#[test]
fn splash_prints_line_by_line_and_the_ide_is_drawn_over_it() {
    use crate::animation::{SPLASH_DRAW, SPLASH_EXIT};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
    let start = Instant::now();
    let mut app = App::with_picker(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ratatui_image::picker::Picker::halfblocks(),
        true,
    )
    .unwrap();
    let created = Instant::now();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    // The logo prints first; the bar and the hints come last.
    app.tick_at(start + SPLASH_DRAW.open / 4);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(!text.contains("Starting...") && !text.contains(&format!("Help {}", key("F1"))));

    app.tick_at(created + SPLASH_DRAW.open);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Starting...") && text.contains("Press any key to continue"));
    assert!(text.contains('─'), "the loading bar");

    // Moving the mouse does not close it; the first key does.
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 3,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!app.motion.splash_closing());
    // Keys arrive in real time, after the splash has printed.
    std::thread::sleep(SPLASH_DRAW.open);
    app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    let closing = Instant::now();

    // Halfway out, the top of the IDE is drawn over the splash and keys reach the IDE.
    app.tick_at(closing + SPLASH_EXIT.open / 2);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(row(buffer, 0).contains(&format!("Help {}", key("F1"))));
    assert!(row(buffer, 33).contains("Press any key to continue"));
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(!app.show_explorer);
    assert!(app.splash_visible);
    app.tick_at(closing + SPLASH_EXIT.open);
    assert!(!app.splash_visible);
}

#[test]
fn another_tab_comes_in_from_its_side_over_the_previous_one() {
    use crate::animation::TAB;
    let directory = tempfile::tempdir().unwrap();
    for (name, letter) in [("first.txt", "a"), ("second.txt", "b")] {
        std::fs::write(directory.path().join(name), letter.repeat(300)).unwrap();
    }
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("first.txt")).unwrap();
    app.open_file(directory.path().join("second.txt")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    // The first tab is on the left, so it comes in from the left over the second one.
    app.active_tab = Some(0);
    let start = Instant::now();
    app.tick_at(start);
    app.tick_at(start + TAB.open / 2);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let area = app.regions.editor;
    let text_row = |terminal: &Terminal<TestBackend>| {
        let buffer = terminal.backend().buffer();
        (area.x..area.right())
            .map(|x| buffer[(x, area.y + 1)].symbol())
            .collect::<String>()
    };
    let line = text_row(&terminal);
    assert!(line.find('a').unwrap() < line.find('b').unwrap(), "{line}");

    app.tick_at(start + TAB.open);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(!text_row(&terminal).contains('b'));
}

#[test]
fn the_empty_editor_stays_centred_when_panels_open_and_close() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(140, 50)).unwrap();
    let hint_at = |terminal: &Terminal<TestBackend>| {
        let buffer = terminal.backend().buffer();
        (0..50)
            .find_map(|y| {
                let text = row(buffer, y);
                let start = text.find("Select a file")?;
                Some((text[..start].chars().count() as u16 + 12, y))
            })
            .unwrap()
    };
    for explorer in [true, false] {
        app.show_explorer = explorer;
        settle(&mut app);
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let editor = app.regions.editor;
        let (x, y) = hint_at(&terminal);
        let middle = editor.x + editor.width / 2;
        assert!(x.abs_diff(middle) <= 1, "{x} vs {middle}");
        // The logo and three hints, centred between the top and the bottom border.
        let block_top = y - 23;
        let below = editor.bottom() - 1 - (y + 3);
        assert!((block_top - editor.y - 1).abs_diff(below) <= 1);
    }
}

#[test]
fn dragging_a_tab_to_the_right_edge_splits_the_editor() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let directory = tempfile::tempdir().unwrap();
    for (name, text) in [("left.txt", "alpha"), ("right.txt", "omega")] {
        std::fs::write(directory.path().join(name), text).unwrap();
    }
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(directory.path().join("right.txt")).unwrap();
    app.open_file(directory.path().join("left.txt")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let (_, tab, _) = app.regions.tab_items[0];
    let body = app.regions.editor_body;
    click(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        tab.x + 2,
        tab.y,
    );
    click(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        body.right() - 3,
        body.y + 5,
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Split right"));
    click(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        body.right() - 3,
        body.y + 5,
    );
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let (other, active) = (app.regions.pane_bodies[0], app.regions.pane_bodies[1]);
    assert_eq!(app.regions.editor, active);
    assert!(
        active.x > other.x && active.y == other.y,
        "right.txt went to the right"
    );
    // Each pane has its own tab bar with its own tab.
    let bar = |pane: usize| {
        let area = app.regions.tab_bars[pane];
        row(terminal.backend().buffer(), area.y)
            .chars()
            .skip(area.x as usize)
            .take(area.width as usize)
            .collect::<String>()
    };
    assert!(bar(0).contains("left.txt") && !bar(0).contains("right.txt"));
    assert!(bar(1).contains("right.txt") && !bar(1).contains("left.txt"));
    let line = row(terminal.backend().buffer(), active.y + 1);
    assert!(line.contains("alpha") && line.contains("omega"), "{line}");
    // A click in the other pane focuses it and puts the cursor there.
    click(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        other.x + 9,
        other.y + 1,
    );
    assert_eq!(app.active_file().unwrap().name, "left.txt");
    assert_eq!(app.active_file().unwrap().cursor_column, 4);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_eq!(app.regions.editor, other);

    // right.txt dragged onto the left pane joins it there; its pane closes.
    let (_, tab, _) = *app
        .regions
        .tab_items
        .iter()
        .find(|(index, _, _)| app.open_files[*index].name == "right.txt")
        .unwrap();
    click(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        tab.x + 2,
        tab.y,
    );
    click(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        other.x + 5,
        other.y + 5,
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Move here"));
    click(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        other.x + 5,
        other.y + 5,
    );
    assert!(app.split.is_none());
    assert_eq!(app.pane_tabs(0).len(), 2);
}

#[test]
fn long_lines_scroll_sideways_and_many_tabs_scroll_in_their_bar() {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    let directory = tempfile::tempdir().unwrap();
    let long = format!("start {} END", "x".repeat(200));
    std::fs::write(
        directory.path().join("long.txt"),
        format!("{long}\nshort\n"),
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    for index in 0..12 {
        let name = format!("file-number-{index}.txt");
        std::fs::write(directory.path().join(&name), "text").unwrap();
        app.open_file(directory.path().join(name)).unwrap();
    }
    app.open_file(directory.path().join("long.txt")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    // The last tab is in view: the bar scrolled and shows `‹`, the first tabs are hidden.
    let bar = row(terminal.backend().buffer(), app.regions.tabs.y);
    assert!(bar.contains("long.txt") && bar.contains('‹'), "{bar}");
    assert!(!bar.contains("file-number-0.txt"), "{bar}");
    // The wheel over the bar scrolls it back, and the bar stays where the wheel left it.
    let tabs = app.regions.tabs;
    for _ in 0..12 {
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: tabs.x + 4,
            row: tabs.y,
            modifiers: KeyModifiers::NONE,
        });
    }
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let bar = row(terminal.backend().buffer(), app.regions.tabs.y);
    assert!(
        bar.contains("file-number-0.txt") && bar.contains('›'),
        "{bar}"
    );

    // Shift and the wheel scroll the text sideways; the end of the long line comes in.
    let editor = app.regions.editor;
    let text = |terminal: &Terminal<TestBackend>| row(terminal.backend().buffer(), editor.y + 1);
    assert!(text(&terminal).contains("start") && !text(&terminal).contains("END"));
    assert!(row(terminal.backend().buffer(), editor.bottom() - 1).contains('━'));
    for _ in 0..40 {
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: editor.x + 10,
            row: editor.y + 2,
            modifiers: KeyModifiers::SHIFT,
        });
    }
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(text(&terminal).contains("END") && !text(&terminal).contains("start"));
    assert!(app.active_file().unwrap().scroll_x > 0);
    // A key at the start of the line brings the cursor, and the view, back.
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Home,
        KeyModifiers::NONE,
    ));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(text(&terminal).contains("start"));
}

#[test]
fn terminal_tabs_sit_in_the_border_and_answer_to_the_mouse() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    app.show_bottom_terminal = true;
    app.show_right_terminal = true;
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(150, 40)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (bottom, right) = (app.regions.bottom_terminal, app.regions.right_terminal);
    let top = |terminal: &Terminal<TestBackend>, area: Rect| {
        cut(terminal.backend().buffer(), area.y, area)
    };
    let border = top(&terminal, bottom);
    assert!(
        border.contains("Bottom terminal [") && border.ends_with(" 1 ─ + ─ | ─╮"),
        "{border}"
    );
    let border = top(&terminal, right);
    assert!(
        border.contains("Right terminal") && border.ends_with(" 1 x ─ 2 x ─ + ─ | ─╮"),
        "{border}"
    );
    assert_eq!(app.regions.terminal_tabs.len(), 3);

    // + opens a second and a third shell; then it goes, and each tab has its x.
    let left = crossterm::event::MouseButton::Left;
    for count in [2, 3] {
        let (_, add) = *app
            .regions
            .terminal_add
            .iter()
            .find(|(panel, _)| *panel == Focus::BottomTerminal)
            .unwrap();
        click(
            &mut app,
            crossterm::event::MouseEventKind::Down(left),
            add.x + 1,
            add.y,
        );
        assert_eq!(app.bottom_terminal.tabs.len(), count);
        assert_eq!(app.bottom_terminal.active, count - 1);
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    }
    let border = top(&terminal, bottom);
    assert!(border.ends_with(" 1 x ─ 2 x ─ 3 x ─ | ─╮"), "{border}");
    assert!(
        !app.regions
            .terminal_add
            .iter()
            .any(|(panel, _)| *panel == Focus::BottomTerminal)
    );
    assert_eq!(app.focus, Focus::BottomTerminal);
    // The x of the first tab closes it; a tab shows its shell.
    let (_, _, _, close) = app.regions.terminal_tabs[0];
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(left),
        close.x,
        close.y,
    );
    assert_eq!(
        (app.bottom_terminal.tabs.len(), app.bottom_terminal.active),
        (2, 1)
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (panel, index, tab, close) = app.regions.terminal_tabs[3];
    assert_eq!(
        (panel, index, close.width),
        (Focus::RightTerminal, 1, 2),
        "the right one closes too"
    );
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(left),
        tab.x,
        tab.y,
    );
    assert_eq!(
        (app.right_terminal.active, app.focus),
        (1, Focus::RightTerminal)
    );

    // A choice of text is drawn in reverse over the shell's own colours.
    let cell = |terminal: &Terminal<TestBackend>| {
        terminal.backend().buffer()[(right.x + 2, right.y + 1)].modifier
    };
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let plain = cell(&terminal);
    app.terminal_selection = Some(crate::app::TerminalSelection {
        panel: Focus::RightTerminal,
        anchor: (0, 3),
        end: (0, 0),
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_ne!(
        cell(&terminal).contains(Modifier::REVERSED),
        plain.contains(Modifier::REVERSED)
    );
}

#[test]
fn small_windows_give_up_panels_in_order_and_a_tiny_one_says_so() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    app.show_explorer = true;
    app.show_bottom_terminal = true;
    app.show_right_terminal = true;
    settle(&mut app);
    let shown = |app: &App| {
        (
            !app.regions.explorer.is_empty(),
            !app.regions.bottom_terminal.is_empty(),
            !app.regions.right_terminal.is_empty(),
            !app.regions.editor.is_empty(),
        )
    };
    let draw_at = |app: &mut App, width, height| {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        buffer_text(&terminal)
    };
    draw_at(&mut app, 120, 40);
    assert_eq!(shown(&app), (true, true, true, true));
    // 80×24: everything still fits.
    draw_at(&mut app, 80, 24);
    assert_eq!(shown(&app), (true, true, true, true));
    // 60×20: the right terminal first, then the bottom one; the editor and Explorer stay.
    draw_at(&mut app, 60, 20);
    assert_eq!(shown(&app), (true, false, false, true));
    // 40×10: only the editor.
    let text = draw_at(&mut app, 40, 10);
    assert_eq!(shown(&app), (false, false, false, true), "{text}");
    // Smaller: a message instead of a broken screen; the panels' settings are kept.
    let text = draw_at(&mut app, 39, 9);
    assert!(text.contains("needs a window of 40×10"), "{text}");
    assert!(
        app.show_right_terminal && app.show_explorer,
        "nothing the user chose changes"
    );
    draw_at(&mut app, 120, 40);
    assert_eq!(shown(&app), (true, true, true, true));
}

#[test]
fn wide_characters_keep_the_editor_the_explorer_and_tables_aligned() {
    use unicode_width::UnicodeWidthStr;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("漢字のファイル.md"),
        "| 名前 | 値 |\n|---|---|\n| 日本語 | 1 |\n| a | 👩‍💻 |\n\n漢字 and 👍🏽 then é\n",
    )
    .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.show_explorer = true;
    app.open_file(directory.path().join("漢字のファイル.md"))
        .unwrap();
    app.open_files[0].move_cursor_to(5, 0);
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    // Rows as the terminal shows them: each cell's symbol, wide ones taking two cells.
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            let mut row = String::new();
            let mut x = 0;
            while x < buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                row.push_str(symbol);
                x += (symbol.width() as u16).max(1);
            }
            row
        })
        .collect();
    // Every row is exactly the window wide: nothing wide pushes a border out.
    for row in &rows {
        assert_eq!(row.width(), 100, "{row}");
    }
    // The table's borders line up although its cells are wide.
    let table: Vec<&String> = rows
        .iter()
        .filter(|row| row.contains("日本語") || row.contains("名前") || row.contains("👩"))
        .collect();
    assert_eq!(table.len(), 3, "{rows:#?}");
    // Where each `│` is, by the width of the text before it as a whole (a ZWJ sequence is
    // one picture two cells wide, not the sum of its parts).
    let bars = |row: &str| {
        row.char_indices()
            .filter(|(_, character)| *character == '│')
            .map(|(at, _)| row[..at].width())
            .collect::<Vec<_>>()
    };
    assert_eq!(bars(table[0]), bars(table[1]), "{}\n{}", table[0], table[1]);
    assert_eq!(bars(table[1]), bars(table[2]), "{}\n{}", table[1], table[2]);
    // The Explorer shows the wide name within its border.
    assert!(
        rows.iter().any(|row| row.contains("漢字のファイル.md")),
        "{rows:#?}"
    );
}
