mod components;
mod editor;
mod overlays;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, Overlay, Regions};

pub(crate) use components::{
    ACTIVE, BORDER, MUTED, TEXT, bar, busy, centered, key, panel_block, render_dialog,
    scale_length, sweep_bar,
};
use components::{render_handle, sweep};
pub(crate) use components::{select_columns, skip_columns};
use editor::{render_editor, render_explorer, render_tabs, render_terminal};
use overlays::{
    render_about, render_help, render_search, render_splash, render_tutorial, render_update,
    render_whats_new,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let vertical = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    let explorer_width = scale_length(app.explorer_width, app.motion.explorer());
    let right_terminal_width = scale_length(app.right_terminal_width, app.motion.right_terminal());
    let bottom_terminal_height =
        scale_length(app.bottom_terminal_height, app.motion.bottom_terminal());
    let mut horizontal_constraints = Vec::new();
    if explorer_width > 0 {
        horizontal_constraints.push(Constraint::Length(explorer_width));
        horizontal_constraints.push(Constraint::Length(1));
    }
    horizontal_constraints.push(Constraint::Min(20));
    if right_terminal_width > 0 {
        horizontal_constraints.push(Constraint::Length(1));
        horizontal_constraints.push(Constraint::Length(right_terminal_width));
    }
    let horizontal = Layout::horizontal(horizontal_constraints).split(vertical[1]);
    let mut center_index = 0;
    let mut regions = Regions {
        body: vertical[1],
        ..Regions::default()
    };
    render_header(frame, app, &mut regions, vertical[0]);
    if explorer_width > 0 {
        regions.explorer = horizontal[0];
        regions.explorer_handle = horizontal[1];
        center_index = 2;
    }
    regions.editor = horizontal[center_index];
    if right_terminal_width > 0 {
        regions.right_handle = horizontal[center_index + 1];
        regions.right_terminal = horizontal[center_index + 2];
    }

    let center = if bottom_terminal_height > 0 {
        Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(bottom_terminal_height),
        ])
        .split(regions.editor)
    } else {
        Layout::vertical([Constraint::Min(3)]).split(regions.editor)
    };
    let editor_area = center[0];
    if bottom_terminal_height > 0 {
        regions.bottom_handle = center[1];
        regions.bottom_terminal = center[2];
    }

    if explorer_width > 0 {
        render_explorer(frame, app, regions.explorer);
        render_handle(frame, regions.explorer_handle, true);
    }
    if app.show_database || app.show_git {
        // Source Control and Database take the text area under the focused pane's tabs.
        let [tabs, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(editor_area);
        let pane = app.active_pane();
        render_tabs(frame, app, &mut regions, tabs, pane);
        regions.tabs = tabs;
        regions.editor = body;
        regions.editor_body = body;
        if app.show_database {
            crate::database::view::render(frame, &mut app.database, body);
        } else {
            crate::git::view::render(frame, &mut app.git, body);
        }
    } else {
        render_editor(frame, app, &mut regions, editor_area);
    }
    let content_area = regions.editor;
    if bottom_terminal_height > 0 {
        render_handle(frame, regions.bottom_handle, false);
        render_terminal(frame, app, regions.bottom_terminal, true);
    }
    if right_terminal_width > 0 {
        render_handle(frame, regions.right_handle, true);
        render_terminal(frame, app, regions.right_terminal, false);
    }
    render_footer(frame, vertical[2]);
    app.regions = regions;
    app.resize_terminals();

    // New content is drawn over the previous frame from its edge, never over a blank area.
    let previous = app.motion.previous();
    let buffer = frame.buffer_mut();
    if app.show_database {
        let motion = &app.database.motion;
        let area = app.database.regions.content;
        sweep(buffer, previous, area, motion.content(), motion.edge());
    } else if app.show_git {
        let motion = &app.git.motion;
        sweep(
            buffer,
            previous,
            app.git.regions.content,
            motion.content(),
            motion.edge(),
        );
    }
    let (progress, edge) = (app.motion.content(), app.motion.content_edge());
    sweep(buffer, previous, content_area, progress, edge);
    app.motion.remember(frame.buffer_mut());

    // Dialogs come last so that they dim everything else and stay out of the next transition.
    if app.show_database {
        crate::database::view::render_form(frame, &mut app.database, content_area);
    } else if app.show_git {
        crate::git::view::render_mode(frame, &mut app.git, content_area);
    }
    if app.search.is_some() {
        render_search(frame, app, area);
    }
    match app.overlay {
        Overlay::Help => render_help(frame, app, area),
        Overlay::About => render_about(frame, app, area),
        Overlay::Tutorial => render_tutorial(frame, app, area),
        Overlay::WhatsNew => render_whats_new(frame, app, area),
        Overlay::Update => {
            let progress = app.motion.overlay();
            render_update(frame, app, area, progress);
        }
        Overlay::None => {}
    }
    if app.splash_visible {
        render_splash(frame, area, &app.motion);
    }
}

fn render_header(frame: &mut Frame, app: &App, regions: &mut Regions, area: Rect) {
    let help_label = format!(" Help {} ", key("F1"));
    let about_label = format!(" About {} ", key("F2"));
    let about_width = about_label.len() as u16;
    let help_width = help_label.len() as u16;
    let about = Rect::new(
        area.right().saturating_sub(about_width),
        area.y,
        about_width,
        1,
    );
    let help = Rect::new(
        about.x.saturating_sub(help_width + 1),
        area.y,
        help_width,
        1,
    );
    let root = app.root.display().to_string();
    let line = Line::from(vec![
        Span::styled(
            " SozoIDE ",
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {root}"), Style::default().fg(MUTED)),
    ]);
    // The path stops before the buttons instead of running under them.
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(area.x, area.y, help.x.saturating_sub(area.x + 1), 1),
    );
    frame.render_widget(
        Paragraph::new(help_label).alignment(Alignment::Center),
        help,
    );
    frame.render_widget(
        Paragraph::new(about_label).alignment(Alignment::Center),
        about,
    );
    regions.header_help = help;
    regions.header_about = about;
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let footer = Line::from(vec![
        Span::styled(" Ctrl+B ", Style::default().fg(ACTIVE)),
        Span::raw("Explorer  "),
        Span::styled("Ctrl+J ", Style::default().fg(ACTIVE)),
        Span::raw("Terminal  "),
        Span::styled("Ctrl+G ", Style::default().fg(ACTIVE)),
        Span::raw("Git  "),
        Span::styled("Ctrl+D ", Style::default().fg(ACTIVE)),
        Span::raw("Database  "),
        Span::styled(format!("{} ", key("F1")), Style::default().fg(ACTIVE)),
        Span::raw("Help  "),
        Span::styled("Ctrl+Q ", Style::default().fg(ACTIVE)),
        Span::raw("Quit"),
    ]);
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(MUTED)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Instant};

    use ratatui::{Terminal, backend::TestBackend};

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
        assert!(app.regions.explorer_handle.width > 0);
        assert!(app.regions.bottom_handle.height > 0);
        assert!(app.regions.right_handle.width > 0);
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
        assert!(app.active_file().unwrap().image.is_some());
        app.overlay = Overlay::Help;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        app.overlay = Overlay::About;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    }

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
    fn panel_transition_defers_pty_resize_until_the_final_frame() {
        let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
        app.show_bottom_terminal = true;
        let initial_size = app.bottom_terminal.size();
        let start = std::time::Instant::now();
        app.tick_at(start);
        let backend = TestBackend::new(140, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert_eq!(app.regions.bottom_terminal.height, 0);
        assert_eq!(app.bottom_terminal.size(), initial_size);

        app.tick_at(start + std::time::Duration::from_millis(60));
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(app.regions.bottom_terminal.height > 0);
        assert!(app.regions.bottom_terminal.height < app.bottom_terminal_height);
        assert_eq!(app.bottom_terminal.size(), initial_size);

        app.tick_at(start + crate::animation::PANEL.open);
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let area = app.regions.bottom_terminal;
        assert_eq!(area.height, app.bottom_terminal_height);
        assert_eq!(
            app.bottom_terminal.size(),
            (
                area.width.saturating_sub(2).max(1),
                area.height.saturating_sub(2).max(1),
            )
        );
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
        assert!(buffer_text(&terminal).contains("Find in current file"));
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

    /// Lets every transition finish; the splash stays for 1.4 s.
    fn click(app: &mut App, kind: crossterm::event::MouseEventKind, column: u16, row: u16) {
        app.handle_mouse(crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
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
        let text =
            |terminal: &Terminal<TestBackend>| row(terminal.backend().buffer(), editor.y + 1);
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
    fn about_grows_the_flower_and_links_the_repository() {
        use crate::branding::{ASCII_LOGO, ASCII_LOGO_LARGE};
        let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 48)).unwrap();
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
        let flower = (0..48).find(|&y| row(buffer, y).contains(top)).unwrap();
        let name = (0..48)
            .find(|&y| row(buffer, y).trim_matches([' ', '│']) == "SozoIDE")
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

        // A smaller terminal gets the flower at its own size.
        let mut small = Terminal::new(TestBackend::new(100, 36)).unwrap();
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
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = buffer_text(&terminal);
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
        let diff = if diff.contains("a.txt  +1 −1") {
            diff
        } else {
            // The diff comes from a thread; draw again once it is there.
            std::thread::sleep(std::time::Duration::from_millis(300));
            show(&mut app, &mut terminal, GitTab::Diff)
        };
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
    fn help_shows_its_sections_in_boxes_and_scrolls_when_short() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        settle(&mut app);
        let mut tall = Terminal::new(TestBackend::new(140, 60)).unwrap();
        tall.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = buffer_text(&tall);
        for section in [
            "╭─ Panels ",
            "╭─ Editing ",
            "╭─ PDF ",
            "╭─ Source Control ",
            "╭─ Application ",
        ] {
            assert!(text.contains(section), "{section}");
        }
        assert!(text.contains("Ctrl+C / Ctrl+X"));
        // The section titles take the colour of their area, one of the terminal's.
        let buffer = tall.backend().buffer();
        let (x, y) = (0..60)
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

    fn settle(app: &mut App) {
        let now = std::time::Instant::now();
        app.tick_at(now);
        app.tick_at(now + std::time::Duration::from_secs(1));
    }

    fn row(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }
}
