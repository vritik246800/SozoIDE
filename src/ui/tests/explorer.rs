//! The Explorer: its menu, its prompt, Properties, Git marks and the sideways scroll.

use super::*;

#[test]
fn the_explorer_menu_opens_at_the_mouse_and_the_prompt_shows_the_icon_of_the_name() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.txt"), "").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.show_explorer = true;
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let explorer = app.regions.explorer;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: explorer.x + 2,
        row: explorer.y + 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let labels = app
        .regions
        .entry_menu_items
        .iter()
        .map(|(action, area)| (action.label(), area.y - explorer.y))
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            ("New File", 2),
            ("New Folder", 3),
            ("Cut", 4),
            ("Copy", 5),
            ("Duplicate", 6),
            ("Rename", 7),
            ("Delete", 8),
            ("Copy Path", 9),
            ("Copy Relative Path", 10),
            ("Reveal in File Manager", 11),
            ("Properties", 12)
        ]
    );
    assert!(buffer_text(&terminal).contains(" New Folder"));

    app.choose_entry_action(crate::app::EntryAction::NewFile);
    app.name_paste("main.rs");
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    let (icon, _) = crate::editor::file_icon(std::path::Path::new("main.rs"));
    assert!(text.contains(&format!("> {icon} main.rs█")), "{text}");
    assert!(text.contains("Enter: create  Esc: cancel"));
    assert!(app.regions.entry_menu_items.is_empty());

    // Delete asks first, with what goes and that it does not come back.
    app.name_prompt = None;
    app.open_entry_menu(Some(0), (explorer.x + 2, explorer.y + 1));
    app.choose_entry_action(crate::app::EntryAction::Delete);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "╭ Delete ",
        "a.txt?",
        "The file goes to the Trash.",
        "Ctrl+Z in the Explorer brings it back.",
        "Enter: delete  Esc: cancel",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    // Shift+Delete asks with its own words: that one does not come back.
    app.name_prompt = None;
    app.prompt_selected(crate::app::EntryAction::DeleteForever);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "Delete for Good",
        "The file is deleted for good.",
        "This cannot be undone.",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
}

#[test]
fn the_properties_dialog_says_calculating_then_shows_the_size() {
    use crate::app::EntryAction;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("docs/sub")).unwrap();
    std::fs::write(root.join("docs/a.txt"), "abc").unwrap();
    std::fs::write(root.join("docs/sub/b.txt"), "hello").unwrap();
    std::fs::write(root.join("notes.txt"), [b'x'; 1536]).unwrap();
    let mut app = App::new(root.to_path_buf()).unwrap();
    app.show_explorer = true;
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let docs = app
        .entries
        .iter()
        .position(|entry| entry.path == root.join("docs"))
        .unwrap();
    app.open_entry_menu(Some(docs), (2, 2));
    app.choose_entry_action(EntryAction::Properties);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    let project = root.file_name().unwrap().to_string_lossy().into_owned();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    for part in [
        "╭ Properties ",
        "docs",
        "Kind         Folder",
        &format!("Location     {project}"),
        "Size         ",
        "Contains     ",
        &format!("Modified     {today}"),
        "Permissions  ",
        "Esc: close",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    // Size and Contains wait for the count with the spinner.
    assert_eq!(text.matches("Calculating\u{2026}").count(), 2, "{text}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.properties.as_ref().unwrap().count.is_none() {
        assert!(std::time::Instant::now() < deadline, "the count never came");
        app.tick();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(!text.contains("Calculating"), "{text}");
    assert!(text.contains("Size         8 B"), "{text}");
    assert!(text.contains("Contains     2 files, 1 folder"), "{text}");
    // The dialog sits in the middle, where a click inside keeps it.
    let area = app.regions.properties;
    assert!(area.width > 0 && area.x > 0 && area.y > 0);

    // A file: its size at once, in KB with the bytes, and nothing to contain.
    app.properties = None;
    let notes = app
        .entries
        .iter()
        .position(|entry| entry.path == root.join("notes.txt"))
        .unwrap();
    app.open_entry_menu(Some(notes), (2, 2));
    app.choose_entry_action(EntryAction::Properties);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Kind         File"), "{text}");
    assert!(text.contains("Size         1.5 KB (1,536 bytes)"), "{text}");
    assert!(!text.contains("Contains") && !text.contains("Calculating"));
    app.properties = None;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(!buffer_text(&terminal).contains("Properties"));
    assert_eq!(app.regions.properties, Rect::default());
}

#[test]
fn the_explorer_draws_long_names_scrolled_sideways_with_its_bar() {
    let directory = tempfile::tempdir().unwrap();
    let long = "a_very_long_file_name_that_does_not_fit_in_the_explorer.txt";
    std::fs::write(directory.path().join(long), "").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let explorer = app.regions.explorer;
    let line = |terminal: &Terminal<TestBackend>, y| cut(terminal.backend().buffer(), y, explorer);
    assert!(line(&terminal, explorer.y + 1).contains("a_very_long"));
    assert!(
        line(&terminal, explorer.bottom() - 1).contains('━'),
        "the bar shows"
    );
    app.explorer_scroll_x = 1000;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row = line(&terminal, explorer.y + 1);
    assert!(
        row.contains("not_fit_in_the_explorer.txt│") && !row.contains("a_very"),
        "{row}"
    );
    let bar = line(&terminal, explorer.bottom() - 1);
    assert!(
        bar.trim_end_matches('╯').ends_with('━'),
        "the thumb at the end: {bar}"
    );
}

#[test]
fn the_explorer_marks_changed_files_and_the_folders_above_them() {
    use crate::git::{ChangeArea, FileStatus};
    use ratatui::style::Color;
    let directory = tempfile::tempdir().unwrap();
    let long = "a_name_much_longer_than_the_explorer_is_wide.txt";
    std::fs::create_dir(directory.path().join("src")).unwrap();
    for name in ["src/lib.rs", "clean.txt", long] {
        std::fs::write(directory.path().join(name), "").unwrap();
    }
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.activate_selected();
    // The rows of the folder that opened are all in (`ROWS`).
    app.tick_at(std::time::Instant::now() + crate::animation::ROWS.open);
    let modified = (FileStatus::Modified, ChangeArea::Unstaged);
    app.git_changes = [
        ("src", modified),
        ("src/lib.rs", modified),
        (long, (FileStatus::Added, ChangeArea::Untracked)),
    ]
    .into_iter()
    .map(|(path, mark)| (PathBuf::from(path), mark))
    .collect();
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let explorer = app.regions.explorer;
    let at = |name: &str| {
        (explorer.y..explorer.bottom())
            .find(|y| row(buffer, *y)[..].contains(name))
            .unwrap()
    };
    let mark = |name: &str| {
        let cell = &buffer[(explorer.right() - 2, at(name))];
        (cell.symbol().to_string(), cell.fg)
    };
    // The folder says how many files below it changed; once open, its file says what.
    assert_eq!(mark("src"), ("1".into(), Color::Yellow));
    assert_eq!(mark("lib.rs"), ("M".into(), Color::Yellow));
    assert_eq!(mark("clean.txt").0, " ");
    // A changed file's name is in the colour of its change.
    let lib = at("lib.rs");
    let name = (explorer.x..explorer.right())
        .find(|x| buffer[(*x, lib)].symbol() == "l")
        .unwrap();
    assert_eq!(buffer[(name, lib)].fg, Color::Yellow);
    // A name too long for the panel gives way to its mark at the right edge.
    assert_eq!(mark("a_name_much"), ("U".into(), Color::Green));
    assert!(!buffer_text(&terminal).contains(long));
    // Scrolled sideways to the end, the end of the name stops clear of its mark.
    app.explorer_scroll_x = usize::MAX;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let y = (explorer.y..explorer.bottom())
        .find(|y| row(buffer, *y).contains("wide.txt"))
        .unwrap();
    let text = row(buffer, y);
    let end = text[..text.find("wide.txt").unwrap()].chars().count() + "wide.txt".len();
    assert!(end <= usize::from(explorer.right() - 3), "{text}");
    assert_eq!(buffer[(explorer.right() - 2, y)].symbol(), "U");
}
