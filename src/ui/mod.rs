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
    ACTIVE, BORDER, MUTED, TEXT, bar, busy, centered, panel_block, render_dialog, scale_length,
    sweep_bar,
};
use components::{render_handle, sweep};
use editor::{render_editor, render_explorer, render_tabs, render_terminal};
use overlays::{render_about, render_help, render_search, render_splash, render_update};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let vertical = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);
    render_header(frame, app, vertical[0]);

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
        header_help: Rect::new(area.width.saturating_sub(20), 0, 9, 1),
        header_about: Rect::new(area.width.saturating_sub(10), 0, 10, 1),
        ..Regions::default()
    };
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

    let editor_parts =
        Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(editor_area);
    regions.tabs = editor_parts[0];
    regions.editor = editor_parts[1];
    let content_area = editor_parts[1];

    if explorer_width > 0 {
        render_explorer(frame, app, regions.explorer);
        render_handle(frame, regions.explorer_handle, true);
    }
    let tabs_area = regions.tabs;
    render_tabs(frame, app, &mut regions, tabs_area);
    if app.show_database {
        crate::database::view::render(frame, &mut app.database, regions.editor);
    } else if app.show_git {
        crate::git::view::render(frame, &mut app.git, regions.editor);
    } else {
        render_editor(frame, app, regions.editor);
    }
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
        Overlay::Help => render_help(frame, area, app.motion.overlay()),
        Overlay::About => render_about(frame, area, app.motion.overlay()),
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

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let root = app.root.display().to_string();
    let line = Line::from(vec![
        Span::styled(
            " SozoIDE ",
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {root}"), Style::default().fg(MUTED)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
    let right = area.width;
    let help = Rect::new(right.saturating_sub(20), area.y, 9, 1);
    let about = Rect::new(right.saturating_sub(10), area.y, 10, 1);
    frame.render_widget(
        Paragraph::new(" Help F1 ").alignment(Alignment::Center),
        help,
    );
    frame.render_widget(
        Paragraph::new(" About F2 ").alignment(Alignment::Center),
        about,
    );
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
        assert!(content.contains("Visual File History"));

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
        assert!(!text.contains("Starting...") && !text.contains("Help F1"));

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
        assert!(row(buffer, 0).contains("Help F1"));
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
        let bottom = (top + 1..40)
            .find(|&y| row(&open, y).contains('╰'))
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
