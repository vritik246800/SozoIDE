//! Help, About, the tutorial, the notes, updates and the questions that wait for Enter.

use super::*;

#[test]
fn a_huge_file_says_its_size_and_how_it_opens_before_enter() {
    let directory = tempfile::tempdir().unwrap();
    let huge = directory.path().join("huge.log");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(60 * 1024 * 1024)
        .unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.open_file(huge).unwrap();
    let text = frame(&mut app, 100, 30);
    for expected in [
        " Open ",
        "huge.log?",
        "60.0 MB: it opens read only, without colours.",
        "The whole file is read into memory.",
        "Enter: open  Esc: cancel",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    assert!(!text.contains("deleted"), "{text}");
}

#[test]
fn the_trust_question_says_what_runs_and_what_works_without_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("unknown-project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    let mut app = App::new(root.clone()).unwrap();
    app.trust = None;
    app.open_file(root.join("main.rs")).unwrap();
    let text = frame(&mut app, 100, 30);
    for expected in [
        " Trust ",
        "Trust the folder unknown-project?",
        "Language servers, the debugger and the commands of a",
        "project run code that is in this folder.",
        "Without trust you can still read, edit, search and use Git.",
        "Enter: trust  Esc: not now",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    // The file is open behind the question.
    assert!(text.contains("main.rs"), "{text}");
}

#[test]
fn renders_global_overlays_and_splash_after_transitions() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::F(1),
        crossterm::event::KeyModifiers::NONE,
    ));
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Help - all commands"));

    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    settle(&mut app);
    // F2 in the Explorer renames (TODO 7.2.2): About from another panel.
    app.focus = crate::app::Focus::Editor;
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::F(2),
        crossterm::event::KeyModifiers::NONE,
    ));
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("About SozoIDE"));

    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    settle(&mut app);
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Find in file"));
    assert!(app.regions.search_input.width > 0);
}

#[test]
fn renders_domain_dialogs_with_animated_clickable_regions() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let backend = TestBackend::new(150, 48);
    let mut terminal = Terminal::new(backend).unwrap();
    app.show_git = true;
    app.git
        .action(crate::git::controller::GitUiAction::OpenCommit);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Commit changes"));
    assert!(app.git.regions.actions.iter().any(|(action, area)| *action
        == crate::git::controller::GitUiAction::Submit
        && area.width > 0));

    app.show_git = false;
    app.show_database = true;
    app.database
        .action(crate::database::controller::DatabaseAction::AddConnection);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Add Connection"));
    assert_eq!(
        app.database.regions.form_fields.len(),
        crate::database::ConnectionField::ALL.len()
    );
}

#[test]
fn help_unfolds_without_reflowing_and_dims_what_is_behind() {
    use crate::animation::DIALOG;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    settle(&mut app);
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    let opened = Instant::now();
    app.tick_at(opened + DIALOG.open / 3);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let unfolding = terminal.backend().buffer().clone();
    app.tick_at(opened + DIALOG.open);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let open = terminal.backend().buffer().clone();

    let top = (0..40)
        .find(|&y| row(&open, y).contains("Help - all commands"))
        .unwrap();
    // The dialog's own bottom corner, under its top one: the boxes inside have theirs too.
    let left = (0..120).find(|&x| open[(x, top)].symbol() == "╭").unwrap();
    let bottom = (top + 1..40)
        .find(|&y| open[(left, y)].symbol() == "╰")
        .unwrap();
    let middle = (top + bottom) / 2;
    // The text is already where it will stay; only the rows near the edges are missing.
    assert_eq!(row(&unfolding, middle), row(&open, middle));
    assert_ne!(row(&unfolding, top + 1), row(&open, top + 1));
    assert!(unfolding[(60, top + 1)].modifier.contains(Modifier::DIM));
    assert!(open[(0, 0)].modifier.contains(Modifier::DIM));
    assert!(!open[(60, middle)].modifier.contains(Modifier::DIM));

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_eq!(app.overlay, Overlay::None);
    assert!(
        !terminal.backend().buffer()[(0, 0)]
            .modifier
            .contains(Modifier::DIM)
    );
}

#[test]
fn renders_the_update_notice_progress_and_errors() {
    use crate::update::{UpdateState, Updater};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    app.updater = Updater::offering("99.0.0", directory.path().join("update.toml"));
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    settle(&mut app);
    assert_eq!(app.overlay, Overlay::Update);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("SozoIDE 99.0.0 is available"));
    assert!(text.contains("Skip this version [S]"));
    assert!(app.regions.update_install.width > 0 && app.regions.update_skip.width > 0);

    app.updater.state = UpdateState::Downloading {
        received: 5_000_000,
        total: Some(10_000_000),
    };
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("5.0 / 10.0 MB") && text.contains("──"));
    assert_eq!(app.regions.update_install.width, 0);

    // Without a size the same bar sweeps.
    app.updater.state = UpdateState::Downloading {
        received: 5_000_000,
        total: None,
    };
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("5.0 MB"));

    app.updater.state = UpdateState::Failed("Download cancelled".to_string());
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("The update failed: Download cancelled"));
}

#[test]
fn about_grows_the_flower_and_links_the_repository() {
    use crate::branding::{ASCII_LOGO, ASCII_LOGO_LARGE};
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 52)).unwrap();
    // F2 in the Explorer renames (TODO 7.2.2): About from another panel.
    app.focus = crate::app::Focus::Editor;
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::F(2),
        crossterm::event::KeyModifiers::NONE,
    ));
    let opened = Instant::now();
    app.tick_at(opened + crate::animation::BLOOM.open / 4);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let growing = buffer_text(&terminal);
    app.tick_at(opened + crate::animation::BLOOM.open);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let grown = buffer_text(&terminal);
    // There is room for the large flower: whole once grown, with the text under it.
    let top = ASCII_LOGO_LARGE[0].trim();
    assert!(!growing.contains(top) && grown.contains(top));
    assert!(grown.contains("Contribute on GitHub [G]"));
    let buffer = terminal.backend().buffer();
    let flower = (0..52).find(|&y| row(buffer, y).contains(top)).unwrap();
    let name = (0..52)
        // The name alone between the dialog's borders (the panels behind show beside it).
        .find(|&y| {
            row(buffer, y)
                .split('│')
                .any(|part| part.trim() == "SozoIDE")
        })
        .unwrap();
    assert!(
        grown.contains("Press Esc to close"),
        "the whole dialog fits"
    );
    assert!(
        name >= flower + ASCII_LOGO_LARGE.len() as u16,
        "the name is under the flower"
    );
    // The flower sits in the middle of the dialog: as many columns on each side.
    let cells = row(buffer, flower).chars().collect::<String>();
    let columns = |text: &str| text.chars().count();
    let start = columns(&cells[..cells.find(top).unwrap()]);
    let left = columns(&cells[..cells[..cells.find(top).unwrap()].rfind('│').unwrap()]);
    let end = start + columns(top);
    let right = end
        + columns(
            cells[cells.find(top).unwrap() + top.len()..]
                .split('│')
                .next()
                .unwrap(),
        );
    assert!(
        (start - left - 1).abs_diff(right - end) <= 1,
        "{left} {start} {end} {right}"
    );
    let button = app.regions.about_repository;
    assert!(button.width > 20);
    let text = row(terminal.backend().buffer(), button.y);
    assert!(text.contains("Contribute on GitHub"), "{text}");
    // The update button under it, and what the last check found under that.
    let update = app.regions.about_update;
    assert_eq!(update.y, button.y + 1);
    let text = row(terminal.backend().buffer(), update.y);
    assert!(text.contains("Check for updates [U]"), "{text}");
    // Release notes beside it, on the same row: About keeps its height.
    let notes = app.regions.about_notes;
    assert_eq!((notes.y, notes.x), (update.y, update.right() + 2));
    let buffer = terminal.backend().buffer();
    let label = (notes.x..notes.right())
        .map(|x| buffer[(x, notes.y)].symbol())
        .collect::<String>();
    assert_eq!(label, " Release notes [N] ");
    app.updater.checked = Some("You have the latest version (0.7.0)".to_string());
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = row(terminal.backend().buffer(), update.y + 1);
    assert!(
        text.contains("You have the latest version (0.7.0)"),
        "{text}"
    );
    app.updater.checked = None;

    // A smaller terminal gets the flower at its own size.
    let mut small = Terminal::new(TestBackend::new(100, 42)).unwrap();
    small.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&small);
    assert!(text.contains(ASCII_LOGO[0].trim()) && !text.contains(top));
    assert!(text.contains("Press Esc to close"));
}

#[test]
fn the_tutorial_pages_and_skips_and_whats_new_lists_the_notes() {
    use crate::update::{NOTES, Welcome};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.welcome = Welcome::Tutorial;
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_eq!(app.overlay, Overlay::Tutorial);
    let text = buffer_text(&terminal);
    assert!(text.contains("Welcome to SozoIDE") && text.contains("SOZO_NO_TELEMETRY=1"));

    let next = app.regions.dialog_next;
    click(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        next.x + 1,
        next.y,
    );
    assert_eq!(app.tutorial_step, 1);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Explorer and editor"));
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(app.tutorial_step, 0);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    settle(&mut app);
    assert_eq!(app.overlay, Overlay::None);

    // From the version before: only this version's notes, every word on screen (E19).
    let previous = NOTES[1].version.to_owned();
    app.welcome = Welcome::WhatsNew(previous);
    settle(&mut app);
    // A version with many notes needs a tall window to show every word at once.
    let mut tall = Terminal::new(TestBackend::new(140, 200)).unwrap();
    tall.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&tall);
    assert!(text.contains("What's new"));
    assert!(text.contains(&format!("Version {}", NOTES[0].version)));
    let words = NOTES[0]
        .new
        .iter()
        .chain(NOTES[0].fixed)
        .flat_map(|note| note.split_whitespace());
    for word in words {
        assert!(text.contains(word), "{word} is missing");
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    settle(&mut app);
    assert_eq!(app.overlay, Overlay::None);

    // From an old version: every version since, newest first; the oldest at the end.
    let oldest = NOTES.last().unwrap();
    app.welcome = Welcome::WhatsNew("0.1.4".to_owned());
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains(&format!("{} versions since the one you had", NOTES.len())));
    assert!(!text.contains(&format!("Version {}", oldest.version)));
    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains(
            oldest
                .fixed
                .last()
                .unwrap()
                .split_whitespace()
                .last()
                .unwrap()
        )
    );
    assert!(!text.contains(&format!("Version {}", NOTES[0].version)));
}

#[test]
fn help_shows_its_sections_in_boxes_and_scrolls_when_short() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    settle(&mut app);
    let mut tall = Terminal::new(TestBackend::new(140, 200)).unwrap();
    tall.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&tall);
    for section in [
        "╭─ Panels ",
        "╭─ Editing ",
        "╭─ PDF ",
        "╭─ Videos ",
        "╭─ Source Control ",
        "╭─ Application ",
    ] {
        assert!(text.contains(section), "{section}");
    }
    assert!(text.contains("Ctrl+C / Ctrl+X"));
    // The keys of the Explorer menu, the search counter, the timeline and the builder.
    for keys in [
        "Right-click",
        "Previous / next result",
        "Click a commit",
        "Remove the builder row",
        "Undo the last edit",
        "New, duplicate, rename,",
        "copy path,",
        "Turn the model",
        "Run the file or its project",
        "Suggestions (language server)",
        "Go to the definition",
        "Breakpoint on the line",
        "Build and debug, or continue",
        "Back / forward 30 s",
        "Volume down / up",
        "Leave full screen, close",
        "Select in a page of text",
        "Cut, copy, paste (Explorer)",
        "Cut, copy, paste the text",
    ] {
        assert!(text.contains(keys), "{keys}");
    }
    // The section titles take the colour of their area, one of the terminal's.
    let buffer = tall.backend().buffer();
    let (x, y) = (0..80)
        .find_map(|y| row(buffer, y).find("╭─ Panels").map(|x| (x, y)))
        .unwrap();
    let x = row(buffer, y)[..x].chars().count() as u16 + 3;
    assert_eq!(buffer[(x, y)].fg, ratatui::style::Color::Cyan);

    let mut short = Terminal::new(TestBackend::new(100, 24)).unwrap();
    short.draw(|frame| draw(frame, &mut app)).unwrap();
    let first = buffer_text(&short);
    assert!(first.contains("Panels") && first.contains("scroll"));
    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    short.draw(|frame| draw(frame, &mut app)).unwrap();
    let last = buffer_text(&short);
    assert!(last.contains("Application") && !last.contains("╭─ Panels "));
}

#[test]
fn help_shows_the_dependencies_tab_with_its_table_and_summary() {
    use crate::{
        app::{Dependencies, HelpTab},
        update::deps::{Component, Found},
    };
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    let component = |name, kind, install: Option<&str>| Component {
        name,
        kind,
        program: name,
        probe: vec![name.to_owned()],
        install: install.map(|command| vec![command.to_owned()]),
        update: install.map(|command| vec![command.to_owned()]),
    };
    let components = vec![
        component("FFmpeg", "Tool", Some("brew")),
        component("Rust", "Compiler", None),
    ];
    app.dependencies = Some(Dependencies::checked(components.clone(), None));
    app.help_tab = HelpTab::Dependencies;
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::F(1),
        crossterm::event::KeyModifiers::NONE,
    ));
    // A busy runner can pause here (E53): opening the tab starts no check of its own.
    std::thread::sleep(std::time::Duration::from_millis(100));
    let now = std::time::Instant::now();
    app.tick_at(now);
    app.tick_at(now + std::time::Duration::from_millis(400));
    let mut terminal = Terminal::new(TestBackend::new(130, 40)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains("Checking...") && text.contains(" Shortcuts "),
        "{text}"
    );
    assert!(!text.contains("Install "), "no action before the check");
    app.dependencies = Some(Dependencies::checked(
        components,
        Some(vec![
            Found::Missing,
            Found::Installed(Some("rustc 1.97.1".into())),
        ]),
    ));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        " Dependencies ",
        "1 missing",
        "1 installed",
        "Component",
        "Installed",
        "Action",
        "FFmpeg",
        "Missing",
        "Install",
        "rustc 1.97.1",
        "Enter: install or update",
    ] {
        assert!(text.contains(part), "{part}");
    }
    assert!(!text.contains("Reset shortcuts"), "the other tab's row");
    assert_eq!(app.regions.dependency_rows.len(), 2);
    assert_eq!(app.regions.help_tabs.len(), 2);
    // What the package manager says (TODO 6.2.2): a newer version, or the latest.
    app.dependencies.as_mut().unwrap().outdated = vec![None, Some(true)];
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Update available"));
    app.dependencies.as_mut().unwrap().outdated = vec![None, Some(false)];
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Up to date"));
    app.dependencies.as_mut().unwrap().outdated = Vec::new();
    // The rows being installed or updated say so until they change (F35).
    let (_sender, quiet) = std::sync::mpsc::channel();
    app.dependencies.as_mut().unwrap().watching = Some((0, quiet));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let ffmpeg = (0..40)
        .map(|y| row(terminal.backend().buffer(), y))
        .find(|line| line.contains("FFmpeg"))
        .unwrap();
    assert!(
        ffmpeg.contains(" Installing…") && !ffmpeg.contains("Missing"),
        "{ffmpeg}"
    );
    let (_sender, quiet) = std::sync::mpsc::channel();
    app.dependencies.as_mut().unwrap().watching = Some((1, quiet));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains(" Updating…"));
    app.dependencies.as_mut().unwrap().watching = None;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let (_, area) = app.regions.dependency_rows[0];
    assert!(
        buffer[(area.x, area.y)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "the chosen row"
    );
    let y = app.regions.dependency_rows[1].1.y;
    let x = row(buffer, y).find("Installed").unwrap();
    let x = row(buffer, y)[..x].chars().count() as u16;
    assert_eq!(buffer[(x, y)].fg, ratatui::style::Color::Green);
    // A click on the tab goes back to the shortcuts.
    let (_, shortcuts) = app.regions.help_tabs[0];
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: shortcuts.x + 1,
        row: shortcuts.y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.help_tab, HelpTab::Shortcuts);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Reset shortcuts"));
}

#[test]
fn the_command_dialog_and_the_project_choice_are_drawn() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for name in ["api", "web"] {
        std::fs::create_dir(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("Cargo.toml"), "").unwrap();
    }
    let mut app = App::new(root.to_path_buf()).unwrap();
    settle(&mut app);
    app.handle_key(KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(150, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        " Choose the project: Build ",
        "Cargo",
        "api",
        "web",
        "Esc: cancel",
    ] {
        assert!(text.contains(part), "{part}");
    }
    assert_eq!(app.regions.choice_rows.len(), 2);
    // A click on the second chooses it; the command shows before it runs.
    let (_, web) = app.regions.choice_rows[1];
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: web.x + 2,
        row: web.y,
        modifiers: KeyModifiers::NONE,
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        " Build the Cargo project ",
        "Project",
        "Folder",
        "web",
        "From",
        "Cargo.toml",
        "cargo build",
        "Enter: run it in the bottom terminal",
    ] {
        assert!(text.contains(part), "{part}");
    }
    assert!(!text.contains("Choose the project"));
    let buffer = terminal.backend().buffer();
    let y = (0..30)
        .find(|y| row(buffer, *y).contains("cargo build"))
        .unwrap();
    let x = row(buffer, y).find("cargo build").unwrap();
    let x = row(buffer, y)[..x].chars().count() as u16;
    assert_eq!(buffer[(x, y)].fg, ACTIVE, "the command stands out");
}

#[test]
fn help_shows_the_users_keys_the_one_being_changed_and_reset() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(140, 50)).unwrap();
    let press = |app: &mut App, code, modifiers| {
        app.handle_key(crossterm::event::KeyEvent::new(code, modifiers));
    };
    let none = crossterm::event::KeyModifiers::NONE;
    press(&mut app, crossterm::event::KeyCode::F(1), none);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Reset shortcuts [R]") && text.contains("Click a key, or Tab"));
    assert!(text.contains("Next terminal tab") && text.contains("Shift+Enter"));
    let reset = app.regions.help_reset;
    assert_eq!(
        cut(terminal.backend().buffer(), reset.y, reset),
        " Reset shortcuts [R] "
    );
    // Each key on screen is where a click finds its command, also with Help scrolled.
    for scroll in [0, 12] {
        app.overlay_scroll = scroll;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(app.regions.help_keys.len() > 4, "{scroll}");
        for (action, area) in &app.regions.help_keys {
            let shown = cut(terminal.backend().buffer(), area.y, *area);
            assert!(
                shown.starts_with(&format!("│ {} ", app.keys.label(*action))),
                "{action:?}: {shown}"
            );
        }
    }
    app.overlay_scroll = 0;

    // Tab twice chooses the bottom terminal; Enter waits for its keys.
    press(&mut app, crossterm::event::KeyCode::Tab, none);
    press(&mut app, crossterm::event::KeyCode::Tab, none);
    press(&mut app, crossterm::event::KeyCode::Enter, none);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Press the new keys for Bottom terminal"));
    let (_, area) = app
        .regions
        .help_keys
        .iter()
        .find(|(action, _)| *action == Action::BottomTerminal)
        .copied()
        .unwrap();
    assert!(cut(terminal.backend().buffer(), area.y, area).starts_with("│ Press keys"));
    press(
        &mut app,
        crossterm::event::KeyCode::Char('t'),
        crossterm::event::KeyModifiers::ALT,
    );
    let label = app.keys.label(Action::BottomTerminal);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains(&format!("Bottom terminal: {label}")),
        "{label}"
    );

    // The new key is the one written everywhere: footer and the terminal's title.
    press(&mut app, crossterm::event::KeyCode::Esc, none);
    settle(&mut app);
    press(
        &mut app,
        crossterm::event::KeyCode::Char('t'),
        crossterm::event::KeyModifiers::ALT,
    );
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(row(buffer, buffer.area.height - 1).contains(&format!("{label} Terminal")));
    let bottom = app.regions.bottom_terminal;
    assert!(cut(buffer, bottom.y, bottom).contains(&format!("Bottom terminal [{label}]")));

    // R asks before it resets them all.
    press(&mut app, crossterm::event::KeyCode::F(1), none);
    press(&mut app, crossterm::event::KeyCode::Char('r'), none);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(&terminal).contains("Reset every shortcut to the default?"));
}

#[test]
fn release_notes_show_every_version_and_the_filter_chooses_new_or_fixed() {
    use crate::update::NOTES;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    // Tall enough for the many notes of 0.9.0 to show their first New and Fixed at once.
    let mut terminal = Terminal::new(TestBackend::new(120, 160)).unwrap();
    // F2 in the Explorer renames (TODO 7.2.2): About from another panel.
    app.focus = crate::app::Focus::Editor;
    app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    settle(&mut app);
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Release notes") && text.contains("the notes of every version"));
    assert!(text.contains(" All ") && text.contains(" New ") && text.contains(" Fixed "));
    assert!(!text.contains("versions since the one you had"));
    // The first words of a note, short enough not to wrap.
    let start = |note: &str| {
        note.split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let (new, fixed) = (start(NOTES[0].new[0]), start(NOTES[0].fixed[0]));
    assert!(text.contains(&new) && text.contains(&fixed));
    let filters = app.regions.notes_filters.clone();
    assert_eq!(filters.len(), 3);

    // A click on Fixed: only what was fixed, from the top.
    let (_, area) = filters[2];
    click(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        area.x + 1,
        area.y,
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains(&fixed) && !text.contains(&new), "{text}");
    // The dialog fits its notes: with fewer of them its filters move up.
    let (_, area) = app.regions.notes_filters[2];
    let buffer = terminal.backend().buffer();
    assert!(
        buffer[(area.x + 1, area.y)]
            .modifier
            .contains(Modifier::REVERSED)
    );
    assert_eq!(buffer[(area.x + 1, area.y)].fg, ACTIVE);
    // BackTab: New, without the fixed notes.
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains(&new) && !text.contains(&fixed));
}
