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

use crate::app::{Action, App, Focus, Overlay, Regions};

pub(crate) use components::{
    ACTIVE, BORDER, CURRENT_MATCH, MATCH, MUTED, TEXT, bar, busy, centered, key, panel_block,
    render_dialog, scale_length, sweep_bar, unfold,
};
pub(crate) use components::{mark_columns, select_columns, skip_columns};
use components::{render_handle, sweep};
use editor::{
    render_editor, render_entry_menu, render_explorer, render_name_prompt, render_properties,
    render_tabs, render_terminal,
};
use overlays::{
    render_about, render_help, render_search, render_splash, render_tutorial, render_update,
    render_whats_new,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if let Some(index) = app.fullscreen_video() {
        // `F` on a video (F23): its tab over the whole terminal, and nothing else to click.
        let pane = app.active_pane();
        let mut regions = Regions {
            body: area,
            editor: area,
            editor_body: area,
            ..Regions::default()
        };
        regions.pane_areas[pane] = area;
        regions.pane_bodies[pane] = area;
        editor::render_media(frame, &mut app.open_files[index], area, true);
        app.regions = regions;
        return;
    }
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
            let key = app.keys.label(Action::Database);
            crate::database::view::render(frame, &mut app.database, body, &key);
        } else {
            let key = app.keys.label(Action::SourceControl);
            crate::git::view::render(frame, &mut app.git, body, &key);
        }
    } else {
        render_editor(frame, app, &mut regions, editor_area);
    }
    let content_area = regions.editor;
    if bottom_terminal_height > 0 {
        render_handle(frame, regions.bottom_handle, false);
        let area = regions.bottom_terminal;
        render_terminal(frame, app, &mut regions, area, Focus::BottomTerminal);
    }
    if right_terminal_width > 0 {
        render_handle(frame, regions.right_handle, true);
        let area = regions.right_terminal;
        render_terminal(frame, app, &mut regions, area, Focus::RightTerminal);
    }
    render_footer(frame, app, vertical[2]);
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
    // Kitty receives an image once, in the first frame that draws it: a sweep over that frame
    // would hide it until another tab is opened (E25), so images and PDFs come in whole.
    let image = app
        .shown_tab(app.active_pane())
        .is_some_and(|index| app.open_files[index].image.is_some());
    if !image {
        sweep(buffer, previous, content_area, progress, edge);
    }
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
    render_entry_menu(frame, app, area);
    render_name_prompt(frame, app, area);
    render_properties(frame, app, area);
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
    // A command waiting for Enter and the project choice go over Help too (F32, F33).
    overlays::render_choice(frame, app, area);
    overlays::render_confirm(frame, app, area);
    if app.splash_visible {
        render_splash(frame, area, &app.motion);
    }
}

fn render_header(frame: &mut Frame, app: &App, regions: &mut Regions, area: Rect) {
    let help_label = format!(" Help {} ", app.keys.label(Action::Help));
    let about_label = format!(" About {} ", app.keys.label(Action::About));
    let about_width = Line::from(about_label.as_str()).width() as u16;
    let help_width = Line::from(help_label.as_str()).width() as u16;
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

/// The keys on the left; on the right the problem on the cursor's line (F26) or the last thing
/// that happened (`app.status`).
fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    // The keys the user gave these commands (F29).
    let mut footer = vec![Span::raw(" ")];
    for (action, name) in [
        (Action::Explorer, "Explorer  "),
        (Action::BottomTerminal, "Terminal  "),
        (Action::SourceControl, "Git  "),
        (Action::Database, "Database  "),
        (Action::Help, "Help  "),
        (Action::Quit, "Quit"),
    ] {
        footer.push(Span::styled(
            format!("{} ", app.keys.label(action)),
            Style::default().fg(ACTIVE),
        ));
        footer.push(Span::raw(name));
    }
    let footer = Line::from(footer);
    let problem = app
        .active_tab
        .and_then(|index| app.open_files.get(index))
        .and_then(|file| {
            let row = file.cursor_row;
            app.lsp
                .diagnostics
                .get(&file.path)?
                .iter()
                .find(|problem| (problem.row..=problem.end_row).contains(&row))
        });
    let message = match problem {
        Some(problem) => {
            let (label, colour) = if problem.error {
                ("error", ratatui::style::Color::Red)
            } else {
                ("warning", ratatui::style::Color::Yellow)
            };
            Line::from(vec![
                Span::styled(format!("{label}: "), Style::default().fg(colour)),
                Span::styled(format!("{} ", problem.message), Style::default().fg(TEXT)),
            ])
        }
        None => Line::from(format!("{} ", app.status)),
    };
    // The message comes first, in up to 60 % of the row; the keys give way (they are in Help).
    let room = (message.width() as u16).min(area.width * 3 / 5);
    let [left, right] = Layout::horizontal([Constraint::Min(0), Constraint::Length(room)])
        .spacing(1)
        .areas(area);
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(MUTED)),
        left,
    );
    frame.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(MUTED))
            .alignment(Alignment::Right),
        right,
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
        // Pictures are drawn as half blocks in their own colours (Q43).
        let file = app.active_file().unwrap();
        assert!(file.media.is_some() && file.image.is_none());
        let blocks = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| {
                matches!(cell.symbol(), "▀" | "▄")
                    && matches!(cell.fg, ratatui::style::Color::Rgb(..))
            })
            .count();
        assert!(blocks > 100, "{blocks}");
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
        let mut tall = Terminal::new(TestBackend::new(140, 80)).unwrap();
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
            "New, rename, delete,",
            "properties (Explorer)",
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
    fn a_video_shows_its_bar_and_buttons_and_fills_the_screen_with_f() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(directory.path().join("clip.mp4")).unwrap();
        app.open_files[0].media = Some(crate::media::sample_video());
        settle(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = buffer_text(&terminal);
        for part in [
            " Pause  ",
            " Stop  ",
            " -5s  ",
            " Vol+  ",
            " Mute  ",
            " Full screen  ",
            "Vol 100%  1x",
            " 00:00 ─",
            "─ 01:15 ",
            "Space  S",
        ] {
            assert!(text.contains(part), "{part}");
        }
        let header = app.regions.header_help;
        assert!(header.width > 0, "the normal layout has its header");

        app.fullscreen = true;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(
            app.regions.pane_bodies[0], buffer.area,
            "the whole terminal"
        );
        assert_eq!(app.regions.header_help.width, 0, "no header");
        assert!(buffer[(0, 0)].symbol() != " " && row(buffer, 0).contains("clip.mp4"));
        // The buttons on the last row but one, over the bottom border.
        assert!(row(buffer, 28).contains(" Full screen "));
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
    fn a_video_is_half_blocks_also_in_a_terminal_with_pictures() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
        let mut picker = ratatui_image::picker::Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        let mut app = App::with_picker(directory.path().to_path_buf(), picker, false).unwrap();
        app.open_file(directory.path().join("clip.mp4")).unwrap();
        app.open_files[0].media = Some(crate::media::sample_video_with_a_frame());
        settle(&mut app);
        assert!(app.open_files[0].image.is_none(), "no picture protocol");
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let green = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.symbol() == "▀" && cell.fg == ratatui::style::Color::Rgb(0, 255, 0))
            .count();
        assert!(green > 100, "{green} green half blocks");
    }

    #[test]
    fn a_kitty_picture_leaves_the_rest_of_the_screen_drawn() {
        let mut picker = ratatui_image::picker::Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        let picture = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 255, 0, 255]));
        let mut image = picker.new_resize_protocol(image::DynamicImage::ImageRgba8(picture));
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        // The first frame sends the picture, the next ones only its rows (E35).
        for _ in 0..2 {
            terminal
                .draw(|frame| {
                    editor::render_picture(
                        frame,
                        &mut image,
                        Rect::new(2, 1, 8, 4),
                        ratatui_image::Resize::Fit(None),
                    );
                    frame.render_widget(Paragraph::new("right side"), Rect::new(12, 2, 20, 1));
                    frame.render_widget(Paragraph::new("under it"), Rect::new(0, 6, 20, 1));
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert!(row(buffer, 2).contains("right side"));
            assert!(row(buffer, 6).contains("under it"));
            for y in 1..5 {
                assert!(buffer[(2, y)].symbol().contains('\u{10EEEE}'), "row {y}");
            }
        }
    }

    #[test]
    fn a_model_is_drawn_in_half_blocks_with_its_keys_in_the_title() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ship.glb");
        std::fs::write(&path, crate::media::sample_glb()).unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path).unwrap();
        settle(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = buffer_text(&terminal);
        assert_eq!(app.active_file().unwrap().language, "3D model");
        assert!(text.contains("←/→ turn  ↑/↓ tilt  +/- zoom"));
        let red = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| {
                matches!(cell.symbol(), "▀" | "▄")
                    && matches!(cell.fg, ratatui::style::Color::Rgb(red, 0, 0) if red > 60)
            })
            .count();
        assert!(red > 20, "{red}");
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
                ("Rename", 6),
                ("Delete", 7),
                ("Properties", 8)
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
            "The file is deleted for good.",
            "This cannot be undone.",
            "Enter: delete  Esc: cancel",
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
            DatabaseController::with_config_path(directory.path().join("connections.toml"))
                .unwrap();
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
        ] {
            assert!(text.contains(part), "{part}\n{text}");
        }
    }

    /// The columns `area` covers on row `y`.
    fn cut(buffer: &ratatui::buffer::Buffer, y: u16, area: Rect) -> String {
        (area.x..area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect()
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
            border.contains("Bottom terminal [") && border.ends_with(" 1 ─ + ─╮"),
            "{border}"
        );
        let border = top(&terminal, right);
        assert!(
            border.contains("Right terminal") && border.ends_with(" 1 ─ 2 ─╮"),
            "{border}"
        );
        // No + button nor x: the + of a key in the title (Ctrl+Alt+B on Linux) is not one.
        assert!(
            !border.contains(" + ") && !border.contains(" x "),
            "{border}"
        );
        assert_eq!(app.regions.terminal_tabs.len(), 3);

        // + opens a second and a third shell; then it goes, and each tab has its x.
        let left = crossterm::event::MouseButton::Left;
        for count in [2, 3] {
            let add = app.regions.terminal_add;
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
        assert!(border.ends_with(" 1 x ─ 2 x ─ 3 x ─╮"), "{border}");
        assert_eq!(app.regions.terminal_add, Rect::default());
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
            (panel, index, close),
            (Focus::RightTerminal, 1, Rect::default())
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
    fn the_explorer_draws_long_names_scrolled_sideways_with_its_bar() {
        let directory = tempfile::tempdir().unwrap();
        let long = "a_very_long_file_name_that_does_not_fit_in_the_explorer.txt";
        std::fs::write(directory.path().join(long), "").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        settle(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let explorer = app.regions.explorer;
        let line =
            |terminal: &Terminal<TestBackend>, y| cut(terminal.backend().buffer(), y, explorer);
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

    fn settle(app: &mut App) {
        let now = std::time::Instant::now();
        app.tick_at(now);
        app.tick_at(now + std::time::Duration::from_secs(1));
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
        // The folder says something below it changed; once open, its file says what.
        assert_eq!(mark("src"), ("*".into(), Color::Yellow));
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

    #[test]
    fn release_notes_show_every_version_and_the_filter_chooses_new_or_fixed() {
        use crate::update::NOTES;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
        let mut app = App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
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
}
