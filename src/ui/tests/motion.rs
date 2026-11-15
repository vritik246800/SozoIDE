//! The transitions of TODO 7.1.1–7.3.4, drawn half way and at their end, and what a frame
//! half way through one costs.

use std::time::{Duration, Instant};

use ratatui::style::{Color, Modifier};

use super::*;
use crate::animation::{FOCUS, GLIDE, MENU, NOTICE, ROWS, SCROLL};
use crate::app::{EntryAction, Focus, NOTICE_LIFE};

fn project() -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("src/util")).unwrap();
    for name in [
        "src/a.rs",
        "src/b.rs",
        "src/c.rs",
        "src/util/m.rs",
        "README.md",
    ] {
        std::fs::write(root.join(name), "line\n".repeat(200)).unwrap();
    }
    let mut app = App::new(root.to_path_buf()).unwrap();
    settle(&mut app);
    (directory, app)
}

fn draw_at(app: &mut App, terminal: &mut Terminal<TestBackend>, now: Instant) {
    app.tick_at(now);
    terminal.draw(|frame| super::draw(frame, app)).unwrap();
}

#[test]
fn a_folder_opens_and_closes_row_by_row_and_keys_do_not_wait() {
    let (_directory, mut app) = project();
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    let start = app.motion.now();
    app.focus = Focus::Explorer;
    // src is the first row: → opens it.
    app.handle_key(crossterm::event::KeyEvent::from(
        crossterm::event::KeyCode::Right,
    ));
    let rows = |app: &App| app.shown_entries().len();
    assert_eq!(app.entries.len(), 6);
    draw_at(&mut app, &mut terminal, start);
    assert_eq!(rows(&app), 2, "no new row yet");
    // Rows come in fast and slow down (ease-out): a fifth of the way, about half are in.
    draw_at(&mut app, &mut terminal, start + ROWS.open / 5);
    let half = rows(&app);
    assert!(half > 2 && half < 6, "{half}");
    // A key half way acts on the whole tree at once: Down goes into the folder.
    app.handle_key(crossterm::event::KeyEvent::from(
        crossterm::event::KeyCode::Down,
    ));
    assert_eq!(app.entries[app.selected_entry].name, "util");
    draw_at(&mut app, &mut terminal, start + ROWS.open);
    assert_eq!(rows(&app), 6);
    let text = buffer_text(&terminal);
    assert!(
        text.contains("▾") && text.contains("│ "),
        "guides and an open folder"
    );
    // ← twice: up to src, then it closes, its rows going one by one.
    let closing = start + ROWS.open;
    app.handle_key(crossterm::event::KeyEvent::from(
        crossterm::event::KeyCode::Left,
    ));
    app.handle_key(crossterm::event::KeyEvent::from(
        crossterm::event::KeyCode::Left,
    ));
    assert_eq!(app.entries.len(), 2);
    draw_at(&mut app, &mut terminal, closing + ROWS.close / 2);
    let going = rows(&app);
    assert!(going > 2 && going < 6, "{going}");
    draw_at(&mut app, &mut terminal, closing + ROWS.close);
    assert_eq!(rows(&app), 2);
}

#[test]
fn the_explorer_title_has_the_folder_and_buttons_that_work() {
    let (directory, mut app) = project();
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    app.show_explorer = true;
    app.explorer_width = 40;
    let name = directory
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    app.open_file(directory.path().join("src/util/m.rs"))
        .unwrap();
    let later = app.motion.now() + Duration::from_secs(1);
    draw_at(&mut app, &mut terminal, later);
    draw_at(&mut app, &mut terminal, later + Duration::from_secs(1));
    let buffer = terminal.backend().buffer().clone();
    let explorer = app.regions.explorer;
    let title = cut(&buffer, explorer.y, explorer);
    assert!(
        title.contains(&name) && title.contains("+f") && title.contains("⊟"),
        "{title}"
    );
    // The file of the active tab was revealed, marked bold and underlined.
    let y = (explorer.y..explorer.bottom())
        .find(|y| cut(&buffer, *y, explorer).contains("m.rs"))
        .unwrap();
    let x = (explorer.x..explorer.right())
        .find(|x| buffer[(*x, y)].symbol() == "m")
        .unwrap();
    // It is also the selected row (revealed): reversed; elsewhere it would be bold.
    assert!(buffer[(x, y)].modifier.contains(Modifier::REVERSED));
    // The buttons: ⊟ closes every folder, +f asks for a new file.
    let collapse = app
        .regions
        .explorer_actions
        .iter()
        .find(|(action, _)| *action == crate::app::ExplorerAction::CollapseAll)
        .unwrap()
        .1;
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        collapse.x,
        collapse.y,
    );
    assert!(app.entries.iter().all(|entry| !entry.expanded));
    let new_file = app.regions.explorer_actions[0].1;
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        new_file.x,
        new_file.y,
    );
    assert_eq!(
        app.name_prompt.as_ref().unwrap().action,
        EntryAction::NewFile
    );
}

#[test]
fn an_empty_folder_says_what_to_do() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    settle(&mut app);
    let text = frame(&mut app, 100, 20);
    assert!(text.contains("This folder is empty.") && text.contains("Shift+N"));
}

#[test]
fn the_focus_is_traced_from_the_corner_and_menus_open_line_by_line() {
    let (_directory, mut app) = project();
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    let start = app.motion.now() + Duration::from_secs(2);
    draw_at(&mut app, &mut terminal, start);
    app.focus = Focus::Editor;
    draw_at(&mut app, &mut terminal, start);
    let editor = app.regions.pane_areas[0];
    let lit = |terminal: &Terminal<TestBackend>| {
        let buffer = terminal.backend().buffer();
        (editor.x..editor.right())
            .filter(|x| buffer[(*x, editor.bottom() - 1)].fg == Color::Cyan)
            .count()
    };
    // At the start only the top left corner is traced: the bottom border is not lit.
    assert_eq!(lit(&terminal), 0);
    draw_at(&mut app, &mut terminal, start + FOCUS.open);
    assert!(lit(&terminal) > usize::from(editor.width) / 2);

    // A right-click menu opens from one line to all of them.
    let menu_at = start + FOCUS.open;
    app.open_entry_menu(Some(0), (5, 5));
    draw_at(&mut app, &mut terminal, menu_at);
    let items = |app: &App| app.regions.entry_menu_items.len();
    assert_eq!(items(&app), 1);
    draw_at(&mut app, &mut terminal, menu_at + MENU.open);
    assert_eq!(items(&app), app.entry_menu.as_ref().unwrap().actions.len());
}

#[test]
fn a_message_comes_in_from_the_left_and_fades_after_a_while() {
    let (_directory, mut app) = project();
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    let start = app.motion.now() + Duration::from_secs(2);
    draw_at(&mut app, &mut terminal, start);
    app.status = "Saved everything".into();
    let footer = |terminal: &Terminal<TestBackend>| row(terminal.backend().buffer(), 23);
    draw_at(&mut app, &mut terminal, start);
    assert!(!footer(&terminal).contains("Saved everything"));
    draw_at(&mut app, &mut terminal, start + NOTICE.open / 2);
    assert!(footer(&terminal).contains("Sav") && !footer(&terminal).contains("everything"));
    draw_at(&mut app, &mut terminal, start + NOTICE.open);
    assert!(footer(&terminal).contains("Saved everything"));
    draw_at(
        &mut app,
        &mut terminal,
        start + NOTICE_LIFE + NOTICE.close / 2,
    );
    let buffer = terminal.backend().buffer();
    let x = row(buffer, 23).find("Saved").unwrap() as u16;
    assert!(buffer[(x, 23)].modifier.contains(Modifier::DIM));
    draw_at(&mut app, &mut terminal, start + NOTICE_LIFE + NOTICE.close);
    assert!(!footer(&terminal).contains("Saved"));
}

#[test]
fn a_page_scrolls_in_steps_and_a_dropped_panel_glides() {
    let (directory, mut app) = project();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    app.open_file(directory.path().join("src/a.rs")).unwrap();
    app.focus = Focus::Editor;
    let start = app.motion.now() + Duration::from_secs(2);
    draw_at(&mut app, &mut terminal, start);
    app.handle_key(crossterm::event::KeyEvent::from(
        crossterm::event::KeyCode::PageDown,
    ));
    let index = app.active_tab.unwrap();
    let target = usize::from(app.open_files[index].scroll);
    assert!(target > 0);
    draw_at(&mut app, &mut terminal, start + SCROLL.open / 3);
    let step = app.open_files[index]
        .shown_scroll
        .value(start + SCROLL.open / 3);
    assert!(step > 0 && step < target, "{step} of {target}");
    draw_at(&mut app, &mut terminal, start + SCROLL.open);
    assert_eq!(
        app.open_files[index]
            .shown_scroll
            .value(start + SCROLL.open),
        target
    );

    // The Explorer dropped on the right edge: its outline goes there from the left.
    let dropped = start + SCROLL.open;
    let from = app.regions.explorer;
    app.motion.glide_panel(
        crate::app::Panel::Explorer,
        crate::animation::Area {
            x: from.x,
            y: from.y,
            width: from.width,
            height: from.height,
        },
    );
    app.tiles = app.tiles.clone().moved(
        crate::app::Panel::Explorer,
        None,
        crate::app::DropZone::Right,
    );
    draw_at(&mut app, &mut terminal, dropped);
    let to = app.regions.explorer;
    assert!(to.x > from.x);
    draw_at(&mut app, &mut terminal, dropped + GLIDE.open / 2);
    let outline = app.motion.gliding().unwrap();
    assert!(outline.x > from.x && outline.x < to.x, "{outline:?}");
    draw_at(&mut app, &mut terminal, dropped + GLIDE.open);
    assert!(app.motion.gliding().is_none());
}

/// TODO 7.F.3: no transition holds a key back, and a frame half way through one costs less
/// than 4 ms on a terminal of 200 by 60 (measured in release; debug builds get 10 times more).
#[test]
fn a_frame_half_way_through_a_transition_stays_within_its_budget() {
    let (_directory, mut app) = project();
    let mut terminal = Terminal::new(TestBackend::new(200, 60)).unwrap();
    let start = app.motion.now() + Duration::from_secs(2);
    draw_at(&mut app, &mut terminal, start);
    app.focus = Focus::Explorer;
    app.handle_key(crossterm::event::KeyEvent::from(
        crossterm::event::KeyCode::Right,
    ));
    app.status = "Opened src".into();
    app.focus = Focus::Editor;
    let budget = if cfg!(debug_assertions) {
        Duration::from_millis(40)
    } else {
        Duration::from_millis(4)
    };
    let mut slowest = Duration::ZERO;
    for step in 0..10 {
        let now = start + ROWS.open * step / 20;
        app.tick_at(now);
        let began = Instant::now();
        terminal.draw(|frame| super::draw(frame, &mut app)).unwrap();
        slowest = slowest.max(began.elapsed());
    }
    assert!(slowest < budget, "{slowest:?}");
}

#[test]
fn the_status_bar_has_segments_and_the_header_keeps_the_folder_name() {
    let (directory, mut app) = project();
    std::fs::create_dir(directory.path().join(".git")).unwrap();
    std::fs::write(directory.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    app.open_file(directory.path().join("src/a.rs")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    let start = app.motion.now() + Duration::from_secs(2);
    draw_at(&mut app, &mut terminal, start);
    let footer = row(terminal.backend().buffer(), 23);
    assert!(
        footer.contains("main") && footer.contains("Ln 1, Col 1"),
        "{footer}"
    );
    assert!(footer.contains("Rust"), "{footer}");
    // A click on the branch opens Source Control.
    let branch = app.regions.footer_branch;
    click(
        &mut app,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        branch.x + 1,
        branch.y,
    );
    assert!(app.show_git && app.focus == Focus::Git);
    // On a narrow window the path is cut in the middle and the folder's name stays: a long
    // path on every system (Linux's temporary folder alone fits, E84).
    let deep = directory
        .path()
        .join("a-folder-with-a-long-name/and-another-long-one/orbit");
    std::fs::create_dir_all(&deep).unwrap();
    let mut app = App::new(deep).unwrap();
    let mut narrow = Terminal::new(TestBackend::new(60, 20)).unwrap();
    draw_at(&mut app, &mut narrow, start);
    let header = row(narrow.backend().buffer(), 0);
    assert!(header.contains("orbit") && header.contains('…'), "{header}");
}
