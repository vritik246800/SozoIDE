mod components;
mod debug;
mod editor;
mod explorer;
mod guides;
mod overlays;
mod terminal;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{Action, App, Focus, Overlay, Panel, Regions, SplitDirection};

pub(crate) use crate::editor::char_width;
pub(crate) use components::{
    ACTIVE, BORDER, CURRENT_MATCH, MATCH, MUTED, TEXT, bar, busy, centered, key, panel_block,
    render_dialog, scale_length, sweep_bar, unfold,
};
pub(crate) use components::{mark_columns, middle_ellipsis, select_columns, skip_columns};
use components::{render_handle, render_outline, sweep, trace_border};
use editor::{
    render_editor, render_entry_menu, render_name_prompt, render_panel_drop, render_properties,
    render_tabs,
};
use explorer::render_explorer;
use overlays::{
    render_about, render_help, render_search, render_splash, render_tutorial, render_update,
    render_whats_new,
};
use terminal::render_terminal;

/// Whether colours are left out: `NO_COLOR` set to anything (no-color.org) or
/// `high_contrast = true` in the settings (TODO 5.2.7).
pub fn colourless() -> bool {
    crate::settings::get().high_contrast
        || std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

/// Every colour of `buffer` becomes an attribute a monochrome terminal shows: a background is
/// reverse video, the accent colour bold, red (errors) underlined, the dim colours dim.
pub fn without_colour(buffer: &mut ratatui::buffer::Buffer) {
    use ratatui::style::{Color, Modifier};
    for cell in buffer.content.iter_mut() {
        let mut modifier = cell.modifier;
        if cell.bg != Color::Reset {
            modifier |= Modifier::REVERSED;
        }
        match cell.fg {
            Color::Cyan | Color::LightCyan | Color::Yellow | Color::LightYellow => {
                modifier |= Modifier::BOLD
            }
            Color::Red | Color::LightRed => modifier |= Modifier::UNDERLINED,
            Color::DarkGray | Color::Gray => modifier |= Modifier::DIM,
            _ => {}
        }
        cell.fg = Color::Reset;
        cell.bg = Color::Reset;
        cell.modifier = modifier;
    }
}

/// The smallest window SozoIDE draws in; below it, a message instead of a broken screen.
pub const SMALLEST: (u16, u16) = (40, 10);

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < SMALLEST.0 || area.height < SMALLEST.1 {
        let text = format!(
            "SozoIDE needs a window of {}×{} or more (now {}×{})",
            SMALLEST.0, SMALLEST.1, area.width, area.height
        );
        let lines: Vec<Line> = vec![Line::default(); usize::from(area.height / 2)]
            .into_iter()
            .chain([Line::from(text).centered()])
            .collect();
        frame.render_widget(
            ratatui::widgets::Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }),
            area,
        );
        app.regions = Regions::default();
        return;
    }
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

    let tiling = app
        .placed_tiles(area)
        .place(vertical[1], &|panel, direction| {
            app.panel_length(panel, direction)
        });
    let mut regions = Regions {
        body: vertical[1],
        explorer: tiling.area(Panel::Explorer),
        editor: tiling.area(Panel::Editor),
        bottom_terminal: tiling.area(Panel::BottomTerminal),
        right_terminal: tiling.area(Panel::RightTerminal),
        ..Regions::default()
    };
    render_header(frame, app, &mut regions, vertical[0]);
    let editor_area = regions.editor;
    if !regions.explorer.is_empty() {
        render_explorer(frame, app, &mut regions);
    }
    for divider in &tiling.dividers {
        let vertical = divider.direction == Some(SplitDirection::Columns);
        render_handle(frame, divider.handle, vertical);
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
    if !regions.bottom_terminal.is_empty() {
        let area = regions.bottom_terminal;
        render_terminal(frame, app, &mut regions, area, Focus::BottomTerminal);
    }
    if !regions.right_terminal.is_empty() {
        let area = regions.right_terminal;
        render_terminal(frame, app, &mut regions, area, Focus::RightTerminal);
    }
    // A panel dropped elsewhere glides there from where it was (`GLIDE`).
    for panel in [
        Panel::Explorer,
        Panel::Editor,
        Panel::BottomTerminal,
        Panel::RightTerminal,
    ] {
        let place = tiling.area(panel);
        app.motion.place_panel(
            panel,
            crate::animation::Area {
                x: place.x,
                y: place.y,
                width: place.width,
                height: place.height,
            },
        );
    }
    regions.tiling = tiling;
    render_panel_drop(frame, app);
    render_footer(frame, app, &mut regions, vertical[2]);
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
    // The border of the panel that took the focus is traced from its corner (`FOCUS`).
    let focused = match app.focus {
        Focus::Explorer => app.regions.explorer,
        Focus::BottomTerminal => app.regions.bottom_terminal,
        Focus::RightTerminal => app.regions.right_terminal,
        Focus::Editor | Focus::Git | Focus::Database => {
            if app.show_git || app.show_database {
                app.regions.editor
            } else {
                app.regions
                    .pane_areas
                    .get(app.active_pane())
                    .copied()
                    .unwrap_or(app.regions.editor)
            }
        }
    };
    trace_border(frame.buffer_mut(), focused, app.motion.focus());
    if let Some(place) = app.motion.gliding() {
        let place = Rect::new(place.x, place.y, place.width, place.height);
        render_outline(frame.buffer_mut(), place);
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
        Overlay::Setup => overlays::render_setup(frame, app, area),
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
    // Code Intelligence boxes (TODO 3.3.1).
    overlays::render_signature(frame, app, content_area);
    overlays::render_rename(frame, app, area);
    overlays::render_actions(frame, app, area);
    overlays::render_edit_preview(frame, app, area);
    overlays::render_debug_input(frame, app, area);
    overlays::render_debug_confirm(frame, app, area);
    // The editing dialogs of TODO 2.2–2.3.
    overlays::render_go_to_line(frame, app, content_area);
    overlays::render_finder(frame, app, area);
    overlays::render_layouts(frame, app, area);
    overlays::render_comparison(frame, app, area);
    overlays::render_replace_plan(frame, app, area);
    overlays::render_disk_prompt(frame, app, area);
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
    // The path stops before the buttons, cut in the middle so the folder's name stays whole.
    let room = usize::from(help.x.saturating_sub(area.x + 1)).saturating_sub(11);
    let root = middle_ellipsis(&app.root.display().to_string(), room);
    let line = Line::from(vec![
        Span::styled(
            " SozoIDE ",
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {root}"), Style::default().fg(MUTED)),
    ]);
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
fn render_footer(frame: &mut Frame, app: &App, regions: &mut Regions, area: Rect) {
    // The keys of the panel with the focus first (`UX.md` §1.22), then the ones that work
    // everywhere; the user's own keys where a command has one (F29).
    let mut footer = vec![Span::raw(" ")];
    let key = |action| app.keys.label(action);
    let panel: Vec<(String, &str)> = match app.focus {
        Focus::Explorer => vec![
            ("←→".into(), "fold"),
            ("N".into(), "new"),
            ("F2".into(), "rename"),
            ("Del".into(), "delete"),
        ],
        Focus::Editor if app.active_tab.is_some() => vec![
            (key(Action::FindInFile), "find"),
            (key(Action::Run), "run"),
            (key(Action::Definition), "definition"),
        ],
        Focus::BottomTerminal | Focus::RightTerminal => vec![
            (key(Action::LeaveTerminal), "leave"),
            (key(Action::NextTerminal), "next tab"),
        ],
        Focus::Git => vec![
            ("s".into(), "stage"),
            ("u".into(), "unstage"),
            ("c".into(), "commit"),
        ],
        Focus::Database => vec![("N".into(), "add"), ("F5".into(), "run SQL")],
        Focus::Editor => Vec::new(),
    };
    let everywhere = [
        (key(Action::Explorer), "Explorer"),
        (key(Action::BottomTerminal), "Terminal"),
        (key(Action::SourceControl), "Git"),
        (key(Action::Database), "Database"),
        (key(Action::Help), "Help"),
        (key(Action::Quit), "Quit"),
    ];
    for (index, (keys, name)) in panel.into_iter().chain(everywhere).enumerate() {
        if index > 0 {
            footer.push(Span::raw("  "));
        }
        footer.push(Span::styled(
            format!("{keys} "),
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
        None => notice(app),
    };
    // The segments on the right (`UX.md` §1.22, TODO 7.3.3): the branch and its changes, the
    // place of the cursor and the language of the active file.
    let mut segments = Vec::new();
    // Problems of the project (TODO 3.3.2): a click lists them.
    let (errors, warnings) =
        app.lsp
            .diagnostics
            .values()
            .flatten()
            .fold((0, 0), |(e, w), problem| {
                if problem.error {
                    (e + 1, w)
                } else {
                    (e, w + 1)
                }
            });
    if errors + warnings > 0 {
        segments.push((Segment::Problems, format!(" ✖ {errors} ⚠ {warnings} ")));
    }
    if let Some((branch, changes)) = app.branch_and_changes() {
        let text = match changes {
            0 => format!(" {} {branch} ", app.icon("dev-git_branch")),
            changes => format!(" {} {branch} +{changes} ", app.icon("dev-git_branch")),
        };
        segments.push((Segment::Branch, text));
    }
    if let Some(file) = app.active_tab.and_then(|index| app.open_files.get(index)) {
        if file.image.is_none() && file.media.is_none() {
            segments.push((
                Segment::Place,
                format!(
                    " Ln {}, Col {} ",
                    file.cursor_row + 1,
                    file.cursor_column + 1
                ),
            ));
            segments.push((Segment::Indent, format!(" {} ", file.indent.label())));
            segments.push((
                Segment::LineEnd,
                format!(" {} ", if file.crlf { "CRLF" } else { "LF" }),
            ));
            segments.push((Segment::Encoding, format!(" {} ", file.encoding.label())));
        }
        if let Some(blame) = app.blame_of_cursor() {
            segments.insert(0, (Segment::Blame, blame));
        }
        // `…` while the colours of a long file are on their way (TODO 2.1.10).
        let colouring = if file.colouring() { "…" } else { "" };
        segments.push((Segment::Language, format!(" {}{colouring} ", file.language)));
    }
    let used: u16 = segments
        .iter()
        .map(|(_, text)| Line::from(text.as_str()).width() as u16)
        .sum();
    // Segments only when the row has room for them beside the message.
    let segments = if used + 20 < area.width / 2 {
        segments
    } else {
        Vec::new()
    };
    let used = if segments.is_empty() { 0 } else { used };
    // The message comes first, in up to 60 % of the row; the keys give way (they are in Help).
    let room = (message.width() as u16).min(area.width * 3 / 5);
    let [left, right, end] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(room),
        Constraint::Length(used),
    ])
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
    regions.footer_message = right;
    let mut x = end.x;
    for (segment, text) in segments {
        let width = Line::from(text.as_str()).width() as u16;
        let cell = Rect::new(x, end.y, width, 1).intersection(end);
        let style = if segment == Segment::Branch {
            Style::default().fg(ACTIVE)
        } else {
            Style::default().fg(MUTED)
        };
        frame.render_widget(Paragraph::new(text).style(style), cell);
        match segment {
            Segment::Branch => regions.footer_branch = cell,
            Segment::Place => regions.footer_place = cell,
            Segment::LineEnd => regions.footer_line_end = cell,
            Segment::Indent => regions.footer_indent = cell,
            Segment::Problems => regions.footer_problems = cell,
            _ => {}
        }
        x += width;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Segment {
    Problems,
    Branch,
    Place,
    Indent,
    LineEnd,
    Encoding,
    Language,
    Blame,
}

/// The status message as it comes in from the left and fades after a while (`NOTICE`).
fn notice(app: &App) -> Line<'static> {
    let (entered, faded) = app.motion.notice();
    if faded >= 1.0 {
        return Line::default();
    }
    let text = format!("{} ", app.status);
    let count = text.chars().count();
    let shown = ((count as f32 * entered).ceil() as usize).min(count);
    let line = Line::from(text.chars().take(shown).collect::<String>());
    if faded > 0.0 {
        line.style(Style::default().add_modifier(Modifier::DIM))
    } else {
        line
    }
}

#[cfg(test)]
mod tests;
