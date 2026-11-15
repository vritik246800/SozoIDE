//! Help, About, Setup, updates and shortcuts.

use super::*;

#[test]
fn the_setup_dialog_installs_what_is_chosen_after_saying_it_and_can_be_skipped() {
    use crate::update::deps::Found;
    let (mut app, mut terminal) = drawn(120, 40);
    let key = |code| KeyEvent::from(code);
    app.setup_checker = |_| std::sync::mpsc::channel().1;
    app.open_setup(tools());
    assert_eq!(app.overlay, Overlay::Setup);
    // Still checking: nothing to install yet.
    app.handle_key(key(KeyCode::Enter));
    assert!(app.confirm.is_none() && app.status.contains("checking"));
    answered(
        vec![
            Found::Installed(Some("git 2".into())),
            Found::Missing,
            Found::Missing,
            Found::Missing,
        ],
        &mut app,
    );
    draw(&mut app, &mut terminal);
    let screen = terminal.backend().to_string();
    for text in [
        "Best experience",
        "Installed",
        "[x]",
        "Videos in the editor",
        "Dependencies",
    ] {
        assert!(screen.contains(text), "{text}: {screen}");
    }
    // git is there and cannot be chosen; curl is left out by Space, ffmpeg by a click.
    app.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(
        app.setup.as_ref().unwrap().chosen,
        [false, true, true, true]
    );
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char(' ')));
    let row = app.regions.dependency_rows[2].1;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row.x + 1,
        row.y,
    );
    assert_eq!(
        app.setup.as_ref().unwrap().chosen,
        [false, false, false, true]
    );
    app.handle_key(key(KeyCode::Enter));
    let confirm = app.confirm.as_ref().unwrap();
    assert!(
        confirm.line.ends_with("install -y tmux"),
        "{}",
        confirm.line
    );
    // Enter again runs it in the bottom terminal, not before.
    assert!(!app.show_bottom_terminal);
    app.handle_key(key(KeyCode::Enter));
    assert!(app.show_bottom_terminal && app.focus == Focus::BottomTerminal);
    assert_eq!(app.overlay, Overlay::None);

    let mut skipping = test_app();
    skipping.setup_checker = |_| std::sync::mpsc::channel().1;
    skipping.open_setup(tools());
    skipping.handle_key(key(KeyCode::Esc));
    assert!(skipping.motion.overlay_closing());
    assert!(skipping.status.contains("Dependencies"));
    assert!(!skipping.show_bottom_terminal);
}

#[test]
fn a_tool_this_machine_cannot_install_is_not_chosen() {
    use crate::update::deps::Found;
    let mut app = test_app();
    app.setup_checker = |_| std::sync::mpsc::channel().1;
    let mut components = tools();
    components.truncate(1);
    components[0].install = None;
    app.open_setup(components);
    answered(vec![Found::Missing], &mut app);
    let setup = app.setup.as_ref().unwrap();
    assert_eq!(setup.chosen, [false]);
    assert!(!setup.can_install(0));
    app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
    assert_eq!(app.setup.as_ref().unwrap().chosen, [false]);
    // Nothing chosen: Enter closes it.
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    assert!(app.confirm.is_none() && app.motion.overlay_closing());
}

#[test]
fn t_in_dependencies_turns_tmux_on_only_when_it_can_run() {
    let directory = tempfile::tempdir().unwrap();
    let settings = directory.path().join("update.toml");
    let mut app = test_app();
    app.updater = crate::update::Updater::offering("9.9.9", settings);
    app.dependencies = Some(Dependencies::checked(Vec::new(), None));
    app.tmux_usable = || false;
    app.toggle_tmux();
    assert!(!app.updater.tmux() && app.status.contains("install it"));
    app.tmux_usable = || true;
    app.toggle_tmux();
    assert!(app.updater.tmux() && app.status.contains("tmux on"));
    app.toggle_tmux();
    assert!(!app.updater.tmux() && app.status.contains("tmux off"));
}

#[test]
fn help_about_and_search_shortcuts_work() {
    let mut app = test_app();
    app.open_file(app.root.join("Cargo.toml")).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::Help);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    // F2 in the Explorer renames (TODO 7.2.2): About from another panel.
    app.focus = crate::app::Focus::Editor;
    app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::About);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert_eq!(app.search.as_ref().unwrap().scope, SearchScope::CurrentFile);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(
        KeyCode::Char('P'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(app.search.as_ref().unwrap().scope, SearchScope::Workspace);
}

#[test]
fn closing_surfaces_keeps_content_until_the_transition_ends() {
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let now = std::time::Instant::now();
    app.tick_at(now);
    assert_eq!(app.overlay, Overlay::Help);
    app.tick_at(now + std::time::Duration::from_millis(200));
    assert_eq!(app.overlay, Overlay::None);

    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let now = std::time::Instant::now();
    app.tick_at(now);
    assert!(app.search.is_some());
    app.tick_at(now + std::time::Duration::from_millis(200));
    assert!(app.search.is_none());

    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::Help);
}

#[test]
fn dependencies_check_on_opening_and_install_only_after_enter() {
    use crate::{
        app::HelpTab,
        update::deps::{Component, Found},
    };
    let mut app = test_app();
    press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    press(&mut app, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(app.help_tab, HelpTab::Dependencies);
    assert!(
        app.dependencies.as_ref().unwrap().found.is_none(),
        "checking"
    );
    // Two components of our own: one there, one missing.
    let words = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    app.dependencies.as_mut().unwrap().components = vec![
        Component {
            name: "Cargo",
            kind: "Build tool",
            program: "cargo",
            probe: words(&["cargo", "--version"]),
            install: None,
            update: None,
        },
        Component {
            name: "Fake",
            kind: "Tool",
            program: "sozo-no-such-tool",
            probe: words(&["sozo-no-such-tool", "--version"]),
            install: Some(words(&["echo", "installed"])),
            update: None,
        },
    ];
    press(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
    let started = Instant::now();
    while app.dependencies.as_ref().unwrap().found.is_none() {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
        app.tick_at(Instant::now());
    }
    let dependencies = app.dependencies.as_ref().unwrap();
    let found = dependencies.found.clone().unwrap();
    assert!(matches!(&found[0], Found::Installed(Some(version)) if version.starts_with("cargo ")));
    assert_eq!(found[1], Found::Missing);
    assert_eq!(dependencies.counts(), Some((1, 1)));
    // No way to update here: said, nothing asked.
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.confirm.is_none());
    assert_eq!(
        app.status,
        "No package manager here can update Cargo: do it by hand"
    );
    // The missing one: its command waits for Enter; Esc runs nothing.
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let confirm = app.confirm.as_ref().unwrap();
    assert_eq!(confirm.title, "Install Fake");
    assert_eq!(confirm.rows[2], ("Command", "echo installed".to_owned()));
    assert!(!app.show_bottom_terminal);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.confirm.is_none() && !app.show_bottom_terminal);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.show_bottom_terminal && app.overlay == Overlay::None);
    assert_eq!(app.status, "Installing Fake: echo installed");
    // The row is watched; Help opens on the tab again and checks everything again.
    let dependencies = app.dependencies.as_ref().unwrap();
    assert_eq!(dependencies.watching.as_ref().map(|(row, _)| *row), Some(1));
    assert!(
        dependencies.found.is_some(),
        "what was found stays on screen"
    );
    press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert_eq!(app.help_tab, HelpTab::Dependencies);
    assert!(
        app.dependencies.as_ref().unwrap().reader.is_some(),
        "checking again"
    );
    press(&mut app, KeyCode::Left, KeyModifiers::NONE);
    assert_eq!(app.help_tab, HelpTab::Shortcuts);
}

#[test]
fn an_installation_changes_its_row_by_itself_and_a_server_that_arrives_starts() {
    use crate::{
        app::{Dependencies, tasks::Confirm},
        lsp::Failure,
        update::deps::{Component, Found},
    };
    let mut app = test_app();
    let words = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    let server = Component {
        name: "rust-analyzer",
        kind: "Language server",
        program: "rust-analyzer",
        probe: words(&["rust-analyzer", "--version"]),
        install: Some(words(&["rustup", "component", "add", "rust-analyzer"])),
        update: None,
    };
    app.dependencies = Some(Dependencies::checked(
        vec![server],
        Some(vec![Found::Missing]),
    ));
    app.lsp.fail(
        "rust-analyzer",
        Failure::Start("error: Unknown binary".into()),
    );
    // The installation ends: the watcher says what is there now.
    app.component_watcher = |_, _, before| {
        assert_eq!(before, Found::Missing);
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(Found::Installed(Some("rust-analyzer 1.99.1".into())))
            .unwrap();
        receiver
    };
    app.confirm = Some(Confirm {
        title: "Install rust-analyzer".into(),
        rows: Vec::new(),
        line: "rustup component add rust-analyzer".into(),
        running: "Installing rust-analyzer".into(),
        task: false,
        server: None,
        component: Some(0),
    });
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.lsp.missing("rust-analyzer"), "not before the tick");
    app.tick_at(Instant::now());
    let dependencies = app.dependencies.as_ref().unwrap();
    assert_eq!(
        dependencies.found.as_ref().unwrap()[0],
        Found::Installed(Some("rust-analyzer 1.99.1".into()))
    );
    assert!(dependencies.watching.is_none());
    // The server starts at once, without opening SozoIDE again.
    assert!(!app.lsp.missing("rust-analyzer"));
    assert_eq!(app.status, "rust-analyzer is installed");
    // One that fails again waits for the next thing found: no start at every tick.
    app.lsp.fail("rust-analyzer", Failure::Start(String::new()));
    app.tick_at(Instant::now());
    assert!(app.lsp.missing("rust-analyzer"));
}

#[test]
fn long_dialogs_scroll_with_the_keys_and_the_wheel() {
    let mut app = test_app();
    press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::PageDown, KeyModifiers::NONE);
    assert_eq!(app.overlay_scroll, 11);
    mouse(&mut app, MouseEventKind::ScrollUp, 10, 10);
    assert_eq!(app.overlay_scroll, 8);
    press(&mut app, KeyCode::Home, KeyModifiers::NONE);
    assert_eq!(app.overlay_scroll, 0);
}

#[test]
fn quit_shortcut_stops_the_application() {
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn about_checks_for_updates_and_opens_the_notice_in_its_place() {
    use crate::update::Updater;
    let directory = tempfile::tempdir().unwrap();
    let mut app = test_app();
    // F2 in the Explorer renames (TODO 7.2.2): About from another panel.
    app.focus = crate::app::Focus::Editor;
    press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    assert_eq!(app.overlay, Overlay::About);
    app.regions.about_update = Rect::new(40, 20, 23, 1);
    mouse(
        &mut app,
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        45,
        20,
    );
    assert_eq!(
        app.updater.checked.as_deref(),
        Some("Checking for updates...")
    );
    app.updater.checked = None;
    press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(
        app.updater.checked.as_deref(),
        Some("Checking for updates...")
    );
    assert_eq!(app.overlay, Overlay::About, "About stays while it checks");

    // The answer: a newer version, also one skipped before, takes About's place.
    app.updater = Updater::offering("99.0.0", directory.path().join("update.toml"));
    app.tick();
    assert_eq!(app.overlay, Overlay::Update);
}

#[test]
fn update_notice_skips_waits_and_quits_to_install() {
    use crate::update::Updater;
    let directory = tempfile::tempdir().unwrap();
    let settings = directory.path().join("update.toml");
    let mut app = test_app();
    app.updater = Updater::offering("99.0.0", settings.clone());
    app.tick();
    assert_eq!(app.overlay, Overlay::Update);
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::Update);

    app.regions.update_skip = Rect::new(30, 12, 23, 1);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 35,
        row: 12,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.updater.state, UpdateState::Idle);
    assert!(
        std::fs::read_to_string(&settings)
            .unwrap()
            .contains("99.0.0")
    );
    let now = std::time::Instant::now();
    app.tick_at(now + std::time::Duration::from_millis(200));
    assert_eq!(app.overlay, Overlay::None);

    app.updater = Updater::offering("99.0.1", settings);
    app.tick();
    assert_eq!(app.overlay, Overlay::Update);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.updater.state, UpdateState::Idle);
    app.tick_at(std::time::Instant::now() + std::time::Duration::from_millis(200));
    assert_eq!(app.overlay, Overlay::None);
    assert!(!app.should_quit);

    app.updater.state = UpdateState::Ready(directory.path().join("SozoIDE-macos-arm64.dmg"));
    app.tick();
    assert!(app.should_quit);
}

#[test]
fn changed_shortcuts_run_their_commands_and_the_old_keys_do_not() {
    use crate::app::keys::Binding;
    let mut app = test_app();
    let key = |text| Binding::parse(text).unwrap();
    app.keys.set(Action::Explorer, key("Ctrl+E"));
    press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert!(!app.show_explorer);
    press(&mut app, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert!(!app.show_explorer, "Ctrl+B is nobody's key now");
    press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert!(app.show_explorer);

    // The editor's commands follow too: Alt+W closes the tab and F7 marks a breakpoint.
    let path = app.root.join("Cargo.toml");
    app.open_file(path.clone()).unwrap();
    app.keys.set(Action::CloseTab, key("Alt+W"));
    app.keys.set(Action::Breakpoint, key("F8"));
    press(&mut app, KeyCode::F(9), KeyModifiers::NONE);
    assert!(
        app.breakpoints
            .get(&path)
            .is_none_or(|rows| rows.is_empty())
    );
    press(&mut app, KeyCode::F(8), KeyModifiers::NONE);
    assert!(app.breakpoints[&path].contains(&0));
    press(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(app.open_files.len(), 1);
    press(&mut app, KeyCode::Char('w'), KeyModifiers::ALT);
    assert!(app.open_files.is_empty());

    app.keys.set(Action::Quit, key("Alt+Q"));
    press(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert!(!app.should_quit);
    press(&mut app, KeyCode::Char('q'), KeyModifiers::ALT);
    assert!(app.should_quit);
}

#[test]
fn help_changes_a_shortcut_by_tab_or_a_click_keeps_it_and_resets_them_all() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.toml");
    let mut app = test_app();
    app.keys_path = Some(path.clone());
    press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert_eq!(app.overlay, Overlay::Help);
    press(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(app.key_editor.selected, Some(Action::Quit));
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.key_editor.selected, Some(Action::Explorer));
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.key_editor.capturing);
    // Keys that type or edit are refused and Help keeps waiting.
    press(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);
    assert!(app.key_editor.capturing);
    assert!(app.key_editor.message.as_deref().unwrap().contains("types"));
    press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(app.key_editor.message.as_deref(), Some("Ctrl+C stays copy"));
    press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert!(!app.key_editor.capturing);
    assert_eq!(app.keys.label(Action::Explorer), "Ctrl+E");
    assert_eq!(app.key_editor.message.as_deref(), Some("Explorer: Ctrl+E"));
    let kept = std::fs::read_to_string(&path).unwrap();
    assert!(kept.contains("explorer = \"Ctrl+E\""), "{kept}");

    // A click on Source Control's key, then the key the Explorer took: they swap.
    app.regions.help_keys = vec![(Action::SourceControl, Rect::new(10, 10, 52, 1))];
    mouse(
        &mut app,
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        12,
        10,
    );
    assert!(app.key_editor.capturing);
    assert_eq!(app.key_editor.selected, Some(Action::SourceControl));
    press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert_eq!(app.keys.label(Action::SourceControl), "Ctrl+E");
    assert_eq!(app.keys.label(Action::Explorer), "Ctrl+G");
    assert_eq!(
        app.key_editor.message.as_deref(),
        Some("Source Control: Ctrl+E; Explorer took Ctrl+G")
    );

    // While Help waits, Ctrl+Q is a key like the others; Esc gives up and Help stays.
    app.choose_shortcut(Action::Run);
    press(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert!(!app.should_quit);
    assert_eq!(app.keys.label(Action::Run), "Ctrl+Q");
    assert_eq!(app.keys.label(Action::Quit), "Ctrl+R");
    app.choose_shortcut(Action::Run);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.key_editor.capturing);
    assert!(app.overlay == Overlay::Help && !app.motion.overlay_closing());

    // R asks first: Esc keeps the keys, Enter puts every default back and the file goes.
    press(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
    assert!(app.key_editor.confirm_reset);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.key_editor.confirm_reset);
    assert_eq!(app.keys.label(Action::Explorer), "Ctrl+G");
    app.regions.help_reset = Rect::new(2, 30, 21, 1);
    mouse(
        &mut app,
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        3,
        30,
    );
    assert!(app.key_editor.confirm_reset);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.keys, crate::app::Keymap::defaults(std::env::consts::OS));
    assert!(!path.exists());
    assert_eq!(
        app.key_editor.message.as_deref(),
        Some("Every shortcut is back to the default")
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.motion.overlay_closing());
}

#[test]
fn about_opens_the_notes_of_every_version_and_tab_or_a_click_filters_them() {
    let mut app = test_app();
    // F2 in the Explorer renames (TODO 7.2.2): About from another panel.
    app.focus = crate::app::Focus::Editor;
    press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(app.overlay, Overlay::WhatsNew);
    assert!(app.all_notes);
    assert_eq!(app.notes.len(), crate::update::NOTES.len());
    assert_eq!(app.notes_filter, NotesFilter::All);
    // Tab and BackTab go round All, New and Fixed, from the top each time.
    app.overlay_scroll = 9;
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(
        (app.notes_filter, app.overlay_scroll),
        (NotesFilter::New, 0)
    );
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.notes_filter, NotesFilter::Fixed);
    press(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(app.notes_filter, NotesFilter::New);
    app.regions.notes_filters = vec![(NotesFilter::All, Rect::new(10, 5, 5, 1))];
    mouse(
        &mut app,
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        12,
        5,
    );
    assert_eq!(app.notes_filter, NotesFilter::All);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.motion.overlay_closing());

    // The button of About does the same, and What's new after an update shows all its notes.
    app.tick_at(std::time::Instant::now() + std::time::Duration::from_secs(1));
    // F2 in the Explorer renames (TODO 7.2.2): About from another panel.
    app.focus = crate::app::Focus::Editor;
    press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    app.notes_filter = NotesFilter::Fixed;
    app.regions.about_notes = Rect::new(40, 21, 19, 1);
    mouse(
        &mut app,
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        45,
        21,
    );
    assert_eq!(
        (app.overlay, app.notes_filter),
        (Overlay::WhatsNew, NotesFilter::All)
    );
}

#[test]
fn t_in_help_brings_the_tutorial_back_and_f1_in_whats_new_opens_help() {
    let mut app = test_app();
    press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert_eq!(app.overlay, Overlay::Help);
    press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
    assert_eq!((app.overlay, app.tutorial_step), (Overlay::Tutorial, 0));
    for _ in 0..crate::app::TUTORIAL_STEPS - 1 {
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    }
    assert_eq!(app.tutorial_step, crate::app::TUTORIAL_STEPS - 1);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.open_overlay(Overlay::WhatsNew);
    press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert_eq!(app.overlay, Overlay::Help);
}

#[test]
fn about_shows_the_licence_and_folders_and_l_lists_every_crate_of_the_lock_file() {
    // Every crate of Cargo.lock is in the list: packaging/licenses.sh ran after the lock changed.
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock")).unwrap();
    let listed: std::collections::HashSet<(&str, &str)> = crate::app::LICENSES
        .lines()
        .filter_map(|line| {
            let mut words = line.split(' ');
            Some((words.next()?, words.next()?))
        })
        .collect();
    for package in lock.split("[[package]]").skip(1) {
        let field = |key: &str| {
            package
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .map(|value| value.trim_matches('"'))
        };
        let (name, version) = (field("name = ").unwrap(), field("version = ").unwrap());
        assert!(
            listed.contains(&(name, version)),
            "{name} {version} is not in assets/licenses.txt: run packaging/licenses.sh"
        );
    }
    let mut app = test_app();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 50)).unwrap();
    app.open_overlay(Overlay::About);
    let now = std::time::Instant::now();
    app.tick_at(now);
    app.tick_at(now + std::time::Duration::from_secs(1));
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    let text = terminal_lines(&terminal).join("\n");
    assert!(
        text.contains("Apache-2.0 licence")
            && text.contains("Settings ")
            && text.contains("Cache "),
        "{text}"
    );
    press(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
    let comparison = app.comparison.as_ref().unwrap();
    assert!(comparison.plain && comparison.rows.len() > 100);
    assert!(comparison.rows.iter().any(|(name, licence)| {
        name.as_deref()
            .is_some_and(|name| name.starts_with("ratatui "))
            && licence.as_deref() == Some("MIT")
    }));
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.comparison.is_none());
}
