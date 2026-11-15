//! Source Control and Database.

use super::*;

#[test]
fn renders_source_control_and_database_with_clickable_actions() {
    let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let backend = TestBackend::new(150, 48);
    let mut terminal = Terminal::new(backend).unwrap();
    app.show_git = true;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(app.git.regions.actions.len() >= 10);
    let content = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(content.contains("Source Control"));
    assert!(content.contains("Commit Graph"));
    assert!(content.contains("Launchpad") && content.contains("Inspect"));
    assert!(!content.contains("Line History") && !content.contains("Health"));

    app.show_git = false;
    app.show_database = true;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let content = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(content.contains("Database"));
    assert!(content.contains("Add Connection"));
    assert!(!app.database.regions.actions.is_empty());
}

#[test]
fn source_control_draws_branches_as_a_tree_launchpad_cards_and_a_numbered_diff() {
    use crate::git::{GitTab, controller::GitUiAction};
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        )
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Test User"]);
    git(&["config", "user.email", "test@example.com"]);
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "First commit"]);
    git(&["branch", "feature/login"]);
    git(&["branch", "feature/signup"]);
    std::fs::write(root.join("a.txt"), "one\n2\n").unwrap();
    std::fs::write(root.join("new.txt"), "new\n").unwrap();
    let mut app = App::new(root.to_path_buf()).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(150, 44)).unwrap();
    app.show_git = true;
    let show = |app: &mut App, terminal: &mut Terminal<TestBackend>, tab| {
        app.git.panel.lock().unwrap().change_tab(tab);
        settle(app);
        terminal.draw(|frame| draw(frame, app)).unwrap();
        buffer_text(terminal)
    };

    let branches = show(&mut app, &mut terminal, GitTab::Branches);
    for part in [
        "Local (3)",
        "├─ feature/",
        "│  ├─   login",
        "│  └─   signup",
        "└─ ● main",
    ] {
        assert!(branches.contains(part), "{part} in the branches");
    }
    assert!(branches.contains("First commit"));
    // A click on a branch asks to switch to it.
    let (_, area) = app
        .git
        .regions
        .actions
        .iter()
        .find(|(action, _)| matches!(action, GitUiAction::Branch(_)))
        .cloned()
        .unwrap();
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: area.x + 5,
        row: area.y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert!(matches!(
        app.git.mode,
        crate::git::controller::GitMode::ConfirmCheckout(ref branch) if branch == "feature/login"
    ));
    app.git.action(GitUiAction::Cancel);

    let launchpad = show(&mut app, &mut terminal, GitTab::Launchpad);
    for part in [
        "Current branch",
        "● main",
        "Upstream",
        "No upstream",
        "Ahead / Behind",
        "↑ 0 ahead",
        "Staged 0",
        "Unstaged 1",
        "Untracked 1",
        "M a.txt",
        "U new.txt",
    ] {
        assert!(launchpad.contains(part), "{part} in the launchpad");
    }

    app.git.panel.lock().unwrap().select(0);
    app.git.action(GitUiAction::ShowDiff);
    let diff = show(&mut app, &mut terminal, GitTab::Diff);
    // The diff comes from a thread: draw again until it is there, never a fixed wait (E80).
    let mut diff = diff;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !diff.contains("a.txt  +1 −1") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
        diff = show(&mut app, &mut terminal, GitTab::Diff);
    }
    for part in [
        "▾ M a.txt  +1 −1",
        "▾ @@ -1,2 +1,2 @@",
        "  1   1 │  one",
        "  2     │- two",
        "      2 │+ 2",
    ] {
        assert!(diff.contains(part), "{part} in the diff");
    }
    assert!(diff.contains("Side by side [v]") && diff.contains("Next file"));
}

#[test]
fn the_timeline_draws_coloured_lanes_the_commit_files_and_a_coloured_diff() {
    use crate::git::{
        GitTab,
        controller::{GitUiAction, TimelineStage},
    };
    use ratatui::style::Color;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        )
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Test User"]);
    git(&["config", "user.email", "test@example.com"]);
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "Base"]);
    git(&["checkout", "-q", "-b", "feature"]);
    std::fs::write(root.join("f.txt"), "feature\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "Feature work"]);
    git(&["checkout", "-q", "main"]);
    std::fs::write(root.join("a.txt"), "one\n2\nthree\nfour\n").unwrap();
    git(&["commit", "-q", "-am", "Change a"]);
    git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let mut app = App::new(root.to_path_buf()).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(150, 40)).unwrap();
    app.show_git = true;
    app.git
        .panel
        .lock()
        .unwrap()
        .change_tab(GitTab::CommitGraph);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains("●─╮") && text.contains("(HEAD -> main)"),
        "{text}"
    );
    assert!(text.contains("(feature) Feature work"), "{text}");
    let at = |buffer: &ratatui::buffer::Buffer, needle: &str| {
        (0..buffer.area.height)
            .find_map(|y| {
                let line = row(buffer, y);
                line.find(needle)
                    .map(|byte| (line[..byte].chars().count() as u16, y))
            })
            .unwrap()
    };
    let buffer = terminal.backend().buffer();
    // The merge commit's lane and the feature lane have their own colours.
    let (x, y) = at(buffer, "●─╮");
    assert_ne!(buffer[(x, y)].fg, buffer[(x + 2, y)].fg);

    // The second row (Change a): its files slide in on the right.
    app.git.action(GitUiAction::OpenCommitFiles(1));
    while app.git.operation || app.git.pending_commit.is_some() {
        settle(&mut app);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains(" M a.txt +2 −1"), "{text}");
    assert!(text.contains("Enter: diff  Esc: close"));
    let buffer = terminal.backend().buffer();
    let (x, y) = at(buffer, " M a.txt");
    assert_eq!(buffer[(x + 1, y)].fg, Color::Yellow);

    app.git.action(GitUiAction::TimelineFile(0));
    assert_eq!(app.git.timeline, TimelineStage::Diff);
    // The diff comes in from the right on the clock of the last tick, a second ahead.
    app.tick_at(std::time::Instant::now() + std::time::Duration::from_secs(5));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains("‹ Files") && text.contains("+ four"),
        "{text}"
    );
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[at(buffer, "- two")].fg, Color::Yellow);
    assert_eq!(buffer[at(buffer, "+ 2")].fg, Color::Yellow);
    assert_eq!(buffer[at(buffer, "+ four")].fg, Color::Green);
}

#[test]
fn the_database_workspace_draws_tables_a_diagram_the_builder_sql_and_scrolling_results() {
    use crate::database::{
        DatabaseTab,
        controller::{DatabaseAction, DatabaseController, GridColumn},
    };
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new(directory.path().to_path_buf()).unwrap();
    app.database =
        DatabaseController::with_config_path(directory.path().join("connections.toml")).unwrap();
    let wait = |app: &mut App| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.database.operation && std::time::Instant::now() < deadline {
            app.tick();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!app.database.operation);
    };
    app.database.action(DatabaseAction::AddConnection);
    let form = app.database.form.as_mut().unwrap();
    form.name = "Shop".into();
    form.database = directory.path().join("shop.db").display().to_string();
    app.database.action(DatabaseAction::SaveConnection);
    wait(&mut app);
    app.database.panel.blocking_lock()
        .insert_query(
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);\
         CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id), total REAL);\
         INSERT INTO customers VALUES (1, 'Ana Maria da Silva Costa'), (2, 'Rui');\
         INSERT INTO orders VALUES (1, 1, 5), (2, 1, 20), (3, 2, 7);",
    );
    app.database.action(DatabaseAction::RunQuery);
    wait(&mut app);
    app.database.action(DatabaseAction::UseConnection);
    wait(&mut app);
    for action in [
        DatabaseAction::Object(0),
        DatabaseAction::Object(1),
        DatabaseAction::Column(0, 1),
        DatabaseAction::Column(1, 2),
        DatabaseAction::Cell(1, GridColumn::Total),
        DatabaseAction::Cell(1, GridColumn::Total),
        DatabaseAction::Cell(1, GridColumn::Total),
        DatabaseAction::RunQuery,
    ] {
        app.database.action(action);
    }
    wait(&mut app);
    app.show_database = true;
    app.database
        .action(DatabaseAction::Tab(DatabaseTab::Diagram));
    settle(&mut app);
    settle(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(150, 46)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "Tables (2)",
        "[x] customers",
        "Views (0)",
        "╭ customers ",
        "╭ orders ",
        "[ ] → customer_id  integer",
        "[x]   total  real",
        "orders.customer_id → customers.id",
        "Results: 2 rows",
        "# │ name                     │ sum_total",
        "──┼──",
        "1 │ Ana Maria da Silva Costa │        25",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    // The two boxes are joined by a line in the gap between them.
    let buffer = terminal.backend().buffer();
    let (y, line) = (0..buffer.area.height)
        .map(|y| (y, row(buffer, y)))
        .find(|(_, line)| line.contains("* id  integer"))
        .unwrap();
    assert!(line.contains("│───╮  │"), "{line}");
    assert!(row(buffer, y + 1).contains("│   ╰──│"));

    // In a narrow terminal the row numbers stay and the cells move sideways.
    app.show_explorer = false;
    settle(&mut app);
    let mut narrow = Terminal::new(TestBackend::new(44, 46)).unwrap();
    app.database.result_horizontal_scroll = 1;
    narrow.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&narrow);
    assert!(text.contains("1  Ana Maria da Silva Costa │"), "{text}");
    // No further than the end of the last column: 39 columns in 38.
    app.database.result_horizontal_scroll = 500;
    narrow.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_eq!(app.database.result_horizontal_scroll, 1);
    app.show_explorer = true;
    settle(&mut app);

    app.database
        .action(DatabaseAction::Tab(DatabaseTab::Builder));
    settle(&mut app);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("Table      Column  Show  Total"), "{text}");
    assert!(text.contains("orders     total   [x]   Sum"), "{text}");
    assert!(
        app.database
            .regions
            .actions
            .iter()
            .any(|(action, _)| *action == DatabaseAction::Cell(1, GridColumn::Value))
    );
    // In a short terminal both rows still fit between the header and the hint.
    let mut short = Terminal::new(TestBackend::new(150, 21)).unwrap();
    short.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&short);
    assert!(
        text.contains("customers  name") && text.contains("orders     total"),
        "{text}"
    );

    app.database.action(DatabaseAction::Tab(DatabaseTab::Sql));
    settle(&mut app);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "Made by the builder",
        "SELECT customers.name,",
        "Copy SQL",
        "Run [",
        "2 row(s)",
        "Explain [",
        " 1     +    Made by the builder",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    let actions = &app.database.regions.actions;
    assert!(
        actions
            .iter()
            .any(|(action, _)| *action == DatabaseAction::SqlTab(0))
    );
    assert!(
        actions
            .iter()
            .any(|(action, _)| *action == DatabaseAction::NewSqlTab)
    );
    // More rows than shown: the title and the Load more button say so.
    app.database
        .panel
        .blocking_lock()
        .results
        .as_mut()
        .unwrap()
        .more = true;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains("Load more [") && text.contains("rows and more"),
        "{text}"
    );
    // A statement waits in its box with its parameters.
    app.database.confirm = Some(crate::database::more::Statement {
        sql: "UPDATE orders SET total = ? WHERE id = ?".into(),
        parameters: vec![
            crate::database::DatabaseValue::Text("1'2".into()),
            crate::database::DatabaseValue::Integer(3),
        ],
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    for part in [
        "Run this statement?",
        "UPDATE orders SET total = ? WHERE id = ?",
        "1 = Text(\"1'2\")",
        "2 = Integer(3)",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    app.database.confirm = None;
    // A cell shown whole: JSON formatted.
    app.database
        .panel
        .blocking_lock()
        .results
        .as_mut()
        .unwrap()
        .rows[0][0] = crate::database::DatabaseValue::Text("{\"a\":[1,2]}".into());
    app.database.cell = (0, 0);
    app.database.cell_popup = true;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains("\"a\": [") && text.contains("╭ Cell"),
        "{text}"
    );
    app.database.cell_popup = false;
    // The list says whether a connection is encrypted.
    {
        let mut panel = app.database.panel.blocking_lock();
        let mut remote = panel.connections[0].clone();
        remote.driver = crate::database::Driver::PostgreSQL;
        remote.name = "Remote".into();
        remote.tls = crate::database::Tls::Disable;
        panel.connections.push(remote.clone());
        remote.name = "Safe".into();
        remote.tls = crate::database::Tls::Verify;
        panel.connections.push(remote);
        panel.tab = DatabaseTab::Connections;
    }
    settle(&mut app);
    settle(&mut app);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = buffer_text(&terminal);
    assert!(
        text.contains("Remote  PostgreSQL") && text.contains("no TLS"),
        "{text}"
    );
    assert!(
        text.contains("Safe  PostgreSQL") && text.contains("shop.db  TLS "),
        "{text}"
    );
}
