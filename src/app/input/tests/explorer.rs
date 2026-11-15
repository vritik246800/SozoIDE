//! The Explorer: its menu, its clipboard and what other programs change.

use super::*;

#[test]
fn delete_in_the_explorer_menu_asks_then_removes_and_closes_the_tabs() {
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("docs");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("a.md"), "a").unwrap();
    std::fs::write(directory.path().join("b.txt"), "b").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.show_explorer = true;
    app.regions.explorer = Rect::new(0, 0, 30, 20);
    app.open_file(folder.join("a.md")).unwrap();
    app.editor_paste("!");
    // The project folder has no Delete.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 10);
    assert!(
        !app.entry_menu
            .as_ref()
            .unwrap()
            .actions
            .contains(&EntryAction::Delete)
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    // docs is the first row: Delete comes before the paths and Properties, the last entries.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
    assert!(app.entry_menu.as_ref().unwrap().actions.ends_with(&[
        EntryAction::Delete,
        EntryAction::CopyPath,
        EntryAction::CopyRelativePath,
        EntryAction::RevealInSystem,
        EntryAction::Properties
    ]));
    for _ in 0..5 {
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        app.name_prompt.as_ref().unwrap().action,
        EntryAction::Delete
    );
    // Nothing to type, and Esc keeps everything.
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    app.name_paste("y");
    assert!(app.name_prompt.as_ref().unwrap().name.is_empty());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(folder.exists());
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
    for _ in 0..5 {
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!folder.exists() && app.name_prompt.is_none());
    // The tab of the file inside closed without saving it back.
    assert!(app.open_files.is_empty());
    app.flush_all_files();
    assert!(!folder.exists());
    assert!(
        app.entries
            .iter()
            .all(|entry| !entry.path.starts_with(&folder))
    );
    assert!(directory.path().join("b.txt").exists());
}

#[test]
fn right_clicks_in_the_explorer_create_and_rename_files_and_folders() {
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("src")).unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.show_explorer = true;
    app.regions.explorer = Rect::new(0, 0, 30, 20);
    // Below the tree: the root, which cannot be renamed.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 10);
    let menu = app.entry_menu.as_ref().unwrap();
    assert_eq!(menu.target, directory.path());
    assert_eq!(
        menu.actions,
        [
            EntryAction::NewFile,
            EntryAction::NewFolder,
            EntryAction::CopyPath,
            EntryAction::CopyRelativePath,
            EntryAction::RevealInSystem,
            EntryAction::Properties
        ]
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.entry_menu.is_none());

    // On the src folder: a new file goes inside it, opens and is selected.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
    assert_eq!(app.entry_menu.as_ref().unwrap().actions.len(), 11);
    app.regions.entry_menu_items = vec![(EntryAction::NewFile, Rect::new(3, 2, 14, 1))];
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 5, 2);
    assert!(app.entry_menu.is_none());
    for character in "main.rs".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let created = directory.path().join("src/main.rs");
    assert!(created.is_file());
    assert!(app.name_prompt.is_none());
    assert_eq!(app.entries[app.selected_entry].path, created);
    assert_eq!(app.open_files[app.active_tab.unwrap()].path, created);

    // A name that exists keeps the prompt open with the error.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        app.name_prompt.as_ref().unwrap().action,
        EntryAction::NewFolder
    );
    app.name_paste("main.rs");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.name_prompt.as_ref().unwrap().error.is_some());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.name_prompt.is_none());

    // Renaming the open file follows it in its tab.
    let row = app
        .entries
        .iter()
        .position(|entry| entry.path == created)
        .unwrap();
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        3,
        row as u16 + 1,
    );
    // Up from the top goes round to Properties, the paths, Delete, then Rename.
    for _ in 0..6 {
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.name_prompt.as_ref().unwrap().name, "main.rs");
    for _ in 0.."main.rs".len() {
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    app.name_paste("lib.rs");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let renamed = directory.path().join("src/lib.rs");
    assert!(renamed.is_file() && !created.exists());
    let tab = &app.open_files[app.active_tab.unwrap()];
    assert_eq!(
        (tab.path.as_path(), tab.name.as_str()),
        (renamed.as_path(), "lib.rs")
    );
}

#[test]
fn properties_open_from_the_menu_count_the_folder_and_close_with_esc_or_a_click() {
    use crossterm::event::MouseButton;
    use std::time::{Duration, Instant};
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("docs/sub")).unwrap();
    std::fs::write(root.join("docs/a.txt"), "abc").unwrap();
    std::fs::write(root.join("docs/sub/b.txt"), "hello").unwrap();
    std::fs::write(root.join("top.txt"), "top").unwrap();
    let mut app = App::new(root.to_path_buf()).unwrap();
    app.show_explorer = true;
    app.regions.explorer = Rect::new(0, 0, 30, 20);
    // docs is the first row; Up from the top goes round to Properties, the last entry.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
    press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let dialog = app.properties.as_ref().unwrap();
    assert_eq!(dialog.properties.path, root.join("docs"));
    assert_eq!(dialog.properties.kind, crate::app::Kind::Folder);
    // The count arrives in the tick; until then the dialog says it is calculating.
    assert!(dialog.count.is_none());
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.properties.as_ref().unwrap().count.is_none() {
        assert!(Instant::now() < deadline, "the count never came");
        app.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    let count = app.properties.as_ref().unwrap().count.unwrap();
    assert_eq!(
        (count.files, count.folders, count.bytes, count.unreadable),
        (2, 1, 8, 0)
    );
    // Keys other than Esc and Enter, and right-clicks, leave it as it is.
    press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 2);
    assert!(app.properties.is_some() && app.entry_menu.is_none());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.properties.is_none());

    // A file: its size at once, nothing to count; a click inside stays, outside closes.
    let row = app
        .entries
        .iter()
        .position(|entry| entry.path == root.join("top.txt"))
        .unwrap();
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        3,
        row as u16 + 1,
    );
    app.choose_entry_action(EntryAction::Properties);
    let dialog = app.properties.as_ref().unwrap();
    assert_eq!((dialog.properties.size, dialog.count), (Some(3), None));
    app.regions.properties = Rect::new(40, 5, 30, 10);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 45, 8);
    assert!(app.properties.is_some());
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 1);
    assert!(app.properties.is_none());

    // The empty space below the tree is the project folder; Enter closes too.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 15);
    assert_eq!(
        app.entry_menu.as_ref().unwrap().actions.last(),
        Some(&EntryAction::Properties)
    );
    app.choose_entry_action(EntryAction::Properties);
    assert_eq!(app.properties.as_ref().unwrap().properties.path, root);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.properties.is_none());
}

#[test]
fn the_explorer_cuts_copies_and_pastes_with_keys_and_the_menu() {
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(root.join("a.txt"), "a").unwrap();
    let mut app = App::new(root.to_path_buf()).unwrap();
    app.regions.explorer = Rect::new(0, 0, 30, 20);
    let select = |app: &mut App, name: &str| {
        app.focus = Focus::Explorer;
        app.selected_entry = app
            .entries
            .iter()
            .position(|entry| entry.path == root.join(name))
            .unwrap();
    };
    // Nothing kept: Paste is not in the menu and Ctrl+V says what to do.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
    assert!(
        !app.entry_menu
            .as_ref()
            .unwrap()
            .actions
            .contains(&EntryAction::Paste)
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    select(&mut app, "a.txt");
    press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert!(app.status.contains("first"));

    // Ctrl+C, Ctrl+V next to the file: a copy with a free name, selected.
    press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert!(root.join("a copy.txt").is_file());
    assert_eq!(
        app.entries[app.selected_entry].path,
        root.join("a copy.txt")
    );
    // The menu of a folder pastes into it, which opens.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
    let menu = app.entry_menu.as_ref().unwrap();
    assert_eq!(menu.target, root.join("docs"));
    assert_eq!(
        menu.actions,
        [
            EntryAction::NewFile,
            EntryAction::NewFolder,
            EntryAction::Cut,
            EntryAction::Copy,
            EntryAction::Paste,
            EntryAction::Duplicate,
            EntryAction::Rename,
            EntryAction::Delete,
            EntryAction::CopyPath,
            EntryAction::CopyRelativePath,
            EntryAction::RevealInSystem,
            EntryAction::Properties
        ]
    );
    app.choose_entry_action(EntryAction::Paste);
    assert!(root.join("docs/a.txt").is_file());
    assert!(
        app.entries
            .iter()
            .any(|entry| entry.path == root.join("docs/a.txt"))
    );

    // Ctrl+X moves once, and the open tab follows; never over a file there.
    app.open_file(root.join("a copy.txt")).unwrap();
    select(&mut app, "a.txt");
    press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    select(&mut app, "docs");
    press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert!(app.status.contains("already exists") && root.join("a.txt").is_file());
    select(&mut app, "a copy.txt");
    press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    select(&mut app, "docs");
    press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert!(root.join("docs/a copy.txt").is_file() && !root.join("a copy.txt").exists());
    assert_eq!(app.open_files[0].path, root.join("docs/a copy.txt"));
    assert!(app.file_clipboard.is_none(), "a move is done once");
}

#[test]
fn changes_by_other_programs_reach_the_open_tabs_and_the_explorer() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    std::fs::write(root.join("b.txt"), "two\n").unwrap();
    let mut app = App::new(root.to_path_buf()).unwrap();
    app.open_file(root.join("a.txt")).unwrap();
    app.open_file(root.join("b.txt")).unwrap();
    app.editor_paste("!");
    std::fs::write(root.join("a.txt"), "uno\n").unwrap();
    std::fs::write(root.join("b.txt"), "dos dos\n").unwrap();
    std::fs::write(root.join("c.txt"), "").unwrap();
    let now = Instant::now();
    app.tick_at(now);
    assert_eq!(app.open_files[0].lines, ["one"], "compared once a second");
    // Before the autosave of the edit in b.txt (2 s).
    app.tick_at(now + Duration::from_millis(1500));
    assert_eq!(app.open_files[0].lines, ["uno"]);
    assert_eq!(app.open_files[1].lines, ["!two"]);
    assert!(app.open_files[1].dirty);
    // The question waits (TODO 2.2.6): K keeps the edits, R reads the disk's, C compares.
    assert_eq!(app.status, "b.txt changed on disk");
    assert_eq!(
        app.disk_prompt.as_deref(),
        Some(root.join("b.txt").as_path())
    );
    press(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
    let compare = app.disk_compare.clone().unwrap();
    assert!(
        compare.contains(&"-dos dos".to_string()) && compare.contains(&"+!two".to_string()),
        "{compare:?}"
    );
    press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert!(app.disk_prompt.is_none());
    assert_eq!(app.open_files[1].lines, ["!two"]);
    assert_eq!(app.status, "b.txt: your edits are kept");
    assert!(
        app.entries
            .iter()
            .any(|entry| entry.path == root.join("c.txt"))
    );
}

#[test]
fn the_explorer_scrolls_sideways_up_to_its_widest_name() {
    let directory = tempfile::tempdir().unwrap();
    let long = "a_very_long_file_name_that_does_not_fit_in_the_explorer.txt";
    std::fs::write(directory.path().join(long), "").unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.regions.explorer = Rect::new(0, 1, 30, 20);
    let wheel = |app: &mut App, kind, modifiers| {
        app.handle_mouse(MouseEvent {
            kind,
            column: 5,
            row: 4,
            modifiers,
        });
    };
    wheel(&mut app, MouseEventKind::ScrollDown, KeyModifiers::SHIFT);
    assert_eq!(app.explorer_scroll_x, 6);
    // Never past the end of the widest name: 4 columns before it and the name, 28 in view.
    for _ in 0..20 {
        wheel(&mut app, MouseEventKind::ScrollRight, KeyModifiers::NONE);
    }
    assert_eq!(app.explorer_scroll_x, 4 + long.len() - 28);
    wheel(&mut app, MouseEventKind::ScrollLeft, KeyModifiers::NONE);
    assert_eq!(app.explorer_scroll_x, 4 + long.len() - 28 - 6);
    // The wheel alone still scrolls down the tree.
    wheel(&mut app, MouseEventKind::ScrollDown, KeyModifiers::NONE);
    assert_eq!(app.explorer_scroll_x, 4 + long.len() - 34);
}

#[test]
fn the_explorer_moves_by_the_keyboard_as_a_file_explorer() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("src/util")).unwrap();
    std::fs::write(root.join("src/util/math.rs"), "").unwrap();
    std::fs::write(root.join("src/main.rs"), "").unwrap();
    for name in ["file10.txt", "file2.txt", "file1.txt"] {
        std::fs::write(root.join(name), "").unwrap();
    }
    let mut app = App::new(root.to_path_buf()).unwrap();
    app.show_explorer = true;
    app.focus = Focus::Explorer;
    app.regions.explorer = Rect::new(0, 0, 30, 6);
    let names = |app: &App| {
        app.entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>()
    };
    let selected = |app: &App| app.entries[app.selected_entry].name.clone();
    // Folders first, then names as people read them.
    assert_eq!(names(&app), ["src", "file1.txt", "file2.txt", "file10.txt"]);
    let key = |app: &mut App, code| press(app, code, KeyModifiers::NONE);
    // → opens a folder, → again goes into it, ← goes back up, ← closes it.
    key(&mut app, KeyCode::Right);
    assert_eq!(names(&app)[..3], ["src", "util", "main.rs"]);
    assert_eq!(selected(&app), "src");
    key(&mut app, KeyCode::Right);
    assert_eq!(selected(&app), "util");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Left);
    assert_eq!(selected(&app), "src");
    key(&mut app, KeyCode::Left);
    assert_eq!(names(&app).len(), 4);
    // Home, End and a page of rows (6 rows high: a page is 3).
    key(&mut app, KeyCode::End);
    assert_eq!(selected(&app), "file10.txt");
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::PageDown);
    assert_eq!(selected(&app), "file10.txt");
    key(&mut app, KeyCode::PageUp);
    assert_eq!(selected(&app), "src");
    // F2 renames here (not About), Delete asks, N and Shift+N make a file and a folder.
    key(&mut app, KeyCode::F(2));
    assert_eq!(app.overlay, Overlay::None);
    let prompt = app.name_prompt.take().unwrap();
    assert_eq!(
        (prompt.action, prompt.name.as_str()),
        (EntryAction::Rename, "src")
    );
    key(&mut app, KeyCode::Delete);
    assert_eq!(app.name_prompt.take().unwrap().action, EntryAction::Delete);
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.name_prompt.take().unwrap().action, EntryAction::NewFile);
    key(&mut app, KeyCode::Char('N'));
    assert_eq!(
        app.name_prompt.take().unwrap().action,
        EntryAction::NewFolder
    );
    // Opening a file in a closed folder reveals it; Collapse all closes every folder.
    app.open_file(root.join("src/util/math.rs")).unwrap();
    app.tick();
    assert_eq!(selected(&app), "math.rs");
    app.collapse_all();
    assert_eq!(names(&app).len(), 4);
    assert_eq!(selected(&app), "src");
    // Outside the Explorer and the text of a file, F2 is still About.
    app.focus = Focus::Editor;
    app.active_tab = None;
    key(&mut app, KeyCode::F(2));
    assert_eq!(app.overlay, Overlay::About);
}

#[test]
fn delete_goes_to_the_trash_and_ctrl_z_brings_back_moves_renames_and_deletes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(root.join("a.txt"), "a").unwrap();
    let mut app = App::new(root.clone()).unwrap();
    app.focus = Focus::Explorer;
    let select = |app: &mut App, name: &str| {
        app.selected_entry = app
            .entries
            .iter()
            .position(|entry| entry.name == name)
            .unwrap();
    };
    // Delete asks, then the file goes to the trash (the test's own); Ctrl+Z brings it back.
    select(&mut app, "a.txt");
    press(&mut app, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(
        app.name_prompt.as_ref().unwrap().action,
        EntryAction::Delete
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!root.join("a.txt").exists());
    assert!(app.status.contains("Trash"), "{}", app.status);
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "a");
    // A rename undone, but never over a file that took the old name meanwhile.
    select(&mut app, "a.txt");
    press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    app.name_paste("b.txt");
    app.name_prompt.as_mut().unwrap().name = "b.txt".into();
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(root.join("b.txt").exists());
    std::fs::write(root.join("a.txt"), "new").unwrap();
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(app.status.starts_with("Could not undo"), "{}", app.status);
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "new");
    // Nothing left to undo.
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.status, "Nothing to undo in the Explorer");
    // Shift+Delete deletes for good after its own question.
    select(&mut app, "b.txt");
    press(&mut app, KeyCode::Delete, KeyModifiers::SHIFT);
    assert_eq!(
        app.name_prompt.as_ref().unwrap().action,
        EntryAction::DeleteForever
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!root.join("b.txt").exists());
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(!root.join("b.txt").exists());
    // The project folder itself is never deleted, nor anything outside it.
    assert!(app.trash_entry(&root).is_err());
    assert!(app.trash_entry(&root.join("../elsewhere")).is_err());
}

#[cfg(unix)]
#[test]
fn a_link_is_removed_without_touching_where_it_points_and_comes_back() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep.txt"), "keep").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("out")).unwrap();
    let mut app = App::new(root.clone()).unwrap();
    app.trash_entry(&root.join("out")).unwrap();
    assert!(std::fs::symlink_metadata(root.join("out")).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("keep.txt")).unwrap(),
        "keep"
    );
    app.focus = Focus::Explorer;
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(
        std::fs::read_link(root.join("out")).unwrap(),
        outside.path()
    );
}

#[test]
fn several_entries_are_marked_filtered_copied_duplicated_and_moved_by_dragging() {
    use crossterm::event::MouseButton;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("deep/inner")).unwrap();
    std::fs::create_dir(root.join("docs")).unwrap();
    for name in ["a.txt", "b.txt", "c.txt", "deep/inner/needle.rs"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    let mut app = App::new(root.clone()).unwrap();
    app.show_explorer = true;
    app.focus = Focus::Explorer;
    app.regions.explorer = Rect::new(0, 0, 30, 20);
    let row_of = |app: &App, name: &str| {
        let names: Vec<_> = app.entries.iter().map(|entry| entry.name.clone()).collect();
        names
            .iter()
            .position(|entry| entry == name)
            .unwrap_or_else(|| panic!("{name} {names:?}")) as u16
            + 1
    };
    // Shift+Down marks; Esc unmarks.
    app.selected_entry = row_of(&app, "a.txt") as usize - 1;
    press(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(app.marked.len(), 2);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.marked.is_empty());
    // Ctrl+click marks two files; Delete asks once and sends both to the trash; Ctrl+Z.
    let (a, c) = (row_of(&app, "a.txt"), row_of(&app, "c.txt"));
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: a,
        modifiers: KeyModifiers::CONTROL,
    });
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: c,
        modifiers: KeyModifiers::CONTROL,
    });
    assert_eq!(app.marked.len(), 2);
    press(&mut app, KeyCode::Delete, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!root.join("a.txt").exists() && !root.join("c.txt").exists());
    assert!(root.join("b.txt").exists());
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(root.join("a.txt").exists() && root.join("c.txt").exists());
    // `/` filters the whole project, inside closed folders too; letters do not create files.
    press(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
    for character in "needle".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    assert!(app.name_prompt.is_none());
    let names: Vec<_> = app
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["deep", "inner", "needle.rs"]);
    assert_eq!(app.entries[app.selected_entry].name, "needle.rs");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.explorer_filter.is_none());
    assert!(app.entries.iter().any(|entry| entry.name == "b.txt"));
    // Duplicate makes `b copy.txt`; Copy Relative Path says the path.
    app.selected_entry = row_of(&app, "b.txt") as usize - 1;
    app.duplicate_chosen();
    assert!(root.join("b copy.txt").exists());
    app.selected_entry = row_of(&app, "b.txt") as usize - 1;
    app.copy_paths(true);
    assert_eq!(app.status, "Copied b.txt");
    // Dragging b.txt onto docs asks; Enter moves it; Ctrl+Z moves it back.
    let (b, docs) = (row_of(&app, "b.txt"), row_of(&app, "docs"));
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: b,
        modifiers: KeyModifiers::NONE,
    });
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 5, docs);
    assert_eq!(
        app.name_prompt.as_ref().unwrap().action,
        EntryAction::MoveHere
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(root.join("docs/b.txt").exists() && !root.join("b.txt").exists());
    // The click opened b.txt in the editor; Ctrl+Z of the Explorer is the Explorer's.
    app.focus = Focus::Explorer;
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(root.join("b.txt").exists());
    // A folder holding only a folder is one row, `deep/inner`; it never moves into itself.
    app.move_into = Some(root.join("deep/inner"));
    app.selected_entry = row_of(&app, "deep/inner") as usize - 1;
    assert_eq!(
        app.entries[app.selected_entry].path,
        root.join("deep/inner")
    );
    assert!(app.confirm_move().is_err());
    // An ignored file is drawn dim; I turns it off.
    std::fs::write(root.join(".gitignore"), "c.txt\n").unwrap();
    app.refresh_entries().unwrap();
    let ignored = |app: &App, name: &str| {
        let path = root.join(name);
        app.gitignore
            .as_ref()
            .unwrap()
            .matched_path_or_any_parents(&path, false)
            .is_ignore()
    };
    assert!(ignored(&app, "c.txt") && !ignored(&app, "a.txt"));
    assert!(app.dim_ignored);
    press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
    assert!(!app.dim_ignored);
}
