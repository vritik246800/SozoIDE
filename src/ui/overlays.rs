use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::{
    app::{App, AppMotion, TUTORIAL_STEPS},
    branding::{self, ASCII_LOGO},
    editor::SearchScope,
    update::{NOTES, UpdateState},
};

use super::{
    ACTIVE, MUTED, TEXT, bar, centered, components::key_for, render_dialog, scale_length, sweep_bar,
};

/// The splash prints line by line, its bar sweeps while SozoIDE starts, and then the IDE is
/// drawn over it line by line from the top (`DESIGN.md` §1.3).
pub(super) fn render_splash(frame: &mut Frame, area: Rect, motion: &AppMotion) {
    let cut = scale_length(area.height, motion.splash_exit());
    let visible = Rect::new(area.x, area.y + cut, area.width, area.height - cut);
    if visible.is_empty() {
        return;
    }
    let mut lines = ASCII_LOGO
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::default().fg(ACTIVE))))
        .collect::<Vec<_>>();
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            branding::NAME,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Version {}", branding::VERSION)),
        Line::from(""),
        sweep_bar(30, motion.elapsed()),
        Line::from(Span::styled("Starting...", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            "Press any key to continue",
            Style::default().fg(MUTED),
        )),
    ]);
    let top = area.height.saturating_sub(lines.len() as u16) / 2;
    let printed = (motion.splash_draw() * lines.len() as f32).ceil() as usize;
    lines.truncate(printed);
    frame.render_widget(Clear, visible);
    let first = top.max(cut);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .scroll((first - top, 0)),
        Rect::new(area.x, area.y + first, area.width, area.height - first),
    );
}

pub(super) fn render_search(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(search) = &app.search else {
        return;
    };
    let scope = match search.scope {
        SearchScope::CurrentFile => "Find in current file [Ctrl+P]",
        SearchScope::Workspace => "Find in workspace [Ctrl+Shift+P]",
    };
    let progress = app.motion.search();
    render_dialog(
        frame,
        centered(area, 84, 70),
        progress,
        format!(" {scope} "),
        |frame, inner| render_search_results(frame, app, inner),
    );
}

fn render_search_results(frame: &mut Frame, app: &mut App, inner: Rect) {
    if inner.height < 2 {
        return;
    }
    let root = app.root.clone();
    let Some(search) = &mut app.search else {
        return;
    };
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(inner);
    app.regions.search_input = sections[0];
    app.regions.search_results = sections[1];
    frame.render_widget(
        Paragraph::new(format!("> {}", search.query)).style(Style::default().fg(TEXT)),
        sections[0],
    );
    let height = sections[1].height as usize;
    let selected = search.selected;
    if selected >= search.scroll + height {
        search.scroll = selected + 1 - height;
    } else if selected < search.scroll {
        search.scroll = selected;
    }
    app.regions.search_match_items.clear();
    let lines = search
        .matches
        .iter()
        .enumerate()
        .skip(search.scroll)
        .take(height)
        .map(|(index, found)| {
            let row = sections[1].y + (index - search.scroll) as u16;
            app.regions
                .search_match_items
                .push((index, Rect::new(sections[1].x, row, sections[1].width, 1)));
            let path = found
                .path
                .strip_prefix(&root)
                .unwrap_or(&found.path)
                .display();
            let style = if index == selected {
                Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(TEXT)
            };
            Line::from(Span::styled(
                format!(
                    "{}:{}:{}  {}",
                    path,
                    found.line + 1,
                    found.column + 1,
                    found.text
                ),
                style,
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), sections[1]);
    frame.render_widget(
        Paragraph::new(format!(
            "{} results  |  Up/Down: navigate  Enter: open  Esc: close",
            search.matches.len()
        ))
        .style(Style::default().fg(MUTED)),
        sections[2],
    );
}

pub(super) fn render_help(frame: &mut Frame, area: Rect, progress: f32) {
    let os = std::env::consts::OS;
    render_dialog(
        frame,
        centered(area, 76, 84),
        progress,
        format!(" Help - all commands ({}) ", os_name(os)),
        |frame, inner| {
            frame.render_widget(
                Paragraph::new(help_lines(os).join("\n")).wrap(Wrap { trim: false }),
                inner,
            );
        },
    );
}

fn os_name(os: &str) -> &'static str {
    match os {
        "macos" => "macOS",
        "windows" => "Windows",
        _ => "Linux",
    }
}

/// The Help text for `os` (`std::env::consts::OS`): the keys that reach SozoIDE differ per system.
fn help_lines(os: &str) -> Vec<&'static str> {
    let mac = os == "macos";
    let mut lines = vec![
        "Navigation and panels",
        "  Ctrl+B             Toggle Explorer",
        "  Ctrl+J             Toggle bottom terminal",
    ];
    lines.extend(if mac {
        [
            "  Ctrl+Option+B      Toggle right terminal",
            "                     (Option as Meta: Terminal > Settings > Profiles > Keyboard,",
            "                     or iTerm2 > Profiles > Keys > Left Option key: Esc+)",
        ]
        .as_slice()
    } else {
        ["  Ctrl+Shift+B       Toggle right terminal"].as_slice()
    });
    lines.extend([
        "  Ctrl+G             Open Source Control",
        "  Ctrl+D             Open Database",
        "  Mouse drag         Resize panel dividers",
        "  Mouse wheel        Scroll panel or terminal history",
        if mac {
            "  fn+Up/fn+Down      Scroll focused editor or terminal (PageUp/PageDown)"
        } else {
            "  PageUp/PageDown    Scroll focused editor or terminal"
        },
        "",
        "Files and search",
        "  Enter              Open file or expand directory",
        "  Ctrl+W             Close active tab",
        "  Drag a tab         To an editor edge: split it; to the centre: join",
        "  Ctrl+P             Find in current file",
        "  Ctrl+Shift+P       Find in workspace",
        "  Esc                Close dialog or panel",
        "",
        "Editing",
        "  Type/paste         Edit the active text file",
        if mac {
            "  Arrows/fn+Arrows   Move the cursor (fn+Left/Right: Home/End)"
        } else {
            "  Arrows/Home/End    Move the cursor"
        },
        "  Enter/Tab          Insert a new line or tab",
        if mac {
            "  Delete/fn+Delete   Remove text before or after the cursor"
        } else {
            "  Backspace/Delete   Remove text"
        },
        "  Autosave           Save after 2 idle seconds or focus change",
        "",
        "Source Control",
        "  Ctrl+G             Changes, graph, branches and history",
        "  Buttons/tabs       Stage, commit, diff, pull, push and rebase",
        "",
        "Database",
        "  Ctrl+D             Connections, query editor and results",
        if mac {
            "  Ctrl+Enter/fn+F5   Run query"
        } else {
            "  Ctrl+Enter/F5      Run query"
        },
        "",
        "Updates",
        match os {
            "macos" => "  Enter / U          Install the version offered (replaces SozoIDE.app)",
            "windows" => "  Enter / U          Install the version offered (runs the MSI installer)",
            _ => "  Enter / U          Install the version offered (DEB/RPM ask for your sudo password)",
        },
        "  S                  Skip that version",
        "  Esc                Later, or cancel the download",
        "",
        "Application",
    ]);
    lines.extend(if mac {
        ["  fn+F1              Help", "  fn+F2              About"]
    } else {
        ["  F1                 Help", "  F2                 About"]
    });
    lines.extend([
        "  G (in About)       Open the repository to contribute",
        "  Ctrl+Q             Quit",
    ]);
    lines
}

/// The logo grown up to `progress` (0 to 1): characters appear by their distance from the
/// centre of the flower, and the newest ones sprout as `.` first (`DESIGN.md` §1.3).
fn bloom_logo(progress: f32) -> Vec<String> {
    let center_x = ASCII_LOGO[0].chars().count() as f32 / 2.0;
    let center_y = ASCII_LOGO.len() as f32 / 2.0;
    // Terminal cells are about twice as tall as they are wide.
    let distance = |x: usize, y: usize| ((x as f32 - center_x) / 2.0).hypot(y as f32 - center_y);
    let farthest = ASCII_LOGO
        .iter()
        .enumerate()
        .flat_map(|(y, line)| line.chars().enumerate().map(move |(x, _)| distance(x, y)))
        .fold(0.0, f32::max);
    let radius = progress.clamp(0.0, 1.0) * (farthest + 2.0);
    ASCII_LOGO
        .iter()
        .enumerate()
        .map(|(y, line)| {
            line.chars()
                .enumerate()
                .map(|(x, character)| match distance(x, y) {
                    _ if character == ' ' => ' ',
                    reach if reach <= radius - 1.5 => character,
                    reach if reach <= radius => '.',
                    _ => ' ',
                })
                .collect()
        })
        .collect()
}

pub(super) fn render_about(frame: &mut Frame, app: &mut App, area: Rect) {
    let button = " Contribute on GitHub [G] ";
    let mut lines = bloom_logo(app.motion.bloom())
        .into_iter()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(ACTIVE))))
        .collect::<Vec<_>>();
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            branding::NAME,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Version {}", branding::VERSION)),
        Line::from(format!("Created by {}", branding::AUTHOR)),
        Line::from("A terminal-native integrated development environment"),
        Line::from(""),
        Line::from(Span::styled(
            button,
            Style::default()
                .fg(ACTIVE)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(MUTED),
        )),
    ]);
    let button_row = lines.len() as u16 - 3;
    let progress = app.motion.overlay();
    render_dialog(
        frame,
        centered(area, 66, 84),
        progress,
        " About SozoIDE ".to_string(),
        |frame, inner| {
            frame.render_widget(
                Paragraph::new(lines)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: false }),
                inner,
            );
            let width = (button.len() as u16).min(inner.width);
            if button_row < inner.height {
                app.regions.about_repository = Rect::new(
                    inner.x + (inner.width - width) / 2,
                    inner.y + button_row,
                    width,
                    1,
                );
            }
        },
    );
}

/// The tutorial pages for `os` (`std::env::consts::OS`): what each panel does and its key.
fn tutorial_steps(os: &str) -> [(&'static str, String); TUTORIAL_STEPS] {
    let key = |label| key_for(os, label);
    [
        (
            "Welcome to SozoIDE",
            "An IDE that lives in your terminal: files, editor, two shells, Git and SQL side by \
             side. This tour shows where everything is and takes a minute."
                .to_string(),
        ),
        (
            "Explorer and editor",
            format!(
                "{} shows or hides the Explorer. Enter opens a file in a tab and the editor saves \
                 by itself 2 seconds after you stop typing. Drag a tab to an edge of the editor \
                 to split it.",
                key("Ctrl+B")
            ),
        ),
        (
            "Terminals",
            format!(
                "{} opens the bottom terminal and {} the right one. Drag the dividers to resize \
                 the panels; the mouse wheel scrolls the history.",
                key("Ctrl+J"),
                key("Ctrl+Shift+B")
            ),
        ),
        (
            "Source Control",
            format!(
                "{} opens changes, diffs, the commit graph, branches and history. Stage, commit, \
                 pull and push are one click away, and risky steps ask first.",
                key("Ctrl+G")
            ),
        ),
        (
            "Database",
            format!(
                "{} connects to SQLite, PostgreSQL or MySQL. Write a query and run it with \
                 {} or {}.",
                key("Ctrl+D"),
                key("Ctrl+Enter"),
                key("F5")
            ),
        ),
        (
            "Search and help",
            format!(
                "{} finds in the open file and {} in the whole folder. {} lists every command, \
                 {} shows About and {} quits.",
                key("Ctrl+P"),
                key("Ctrl+Shift+P"),
                key("F1"),
                key("F2"),
                key("Ctrl+Q")
            ),
        ),
    ]
}

/// The first start (F17): one page at a time, with Next and Skip always in reach.
pub(super) fn render_tutorial(frame: &mut Frame, app: &mut App, area: Rect) {
    let steps = tutorial_steps(std::env::consts::OS);
    let step = app.tutorial_step.min(TUTORIAL_STEPS - 1);
    let (title, text) = &steps[step];
    let last = step + 1 == TUTORIAL_STEPS;
    let next = if last {
        " Start [Enter] "
    } else {
        " Next [Enter] "
    };
    let skip = " Skip tutorial [Esc] ";
    let area = dialog_area(area, 66, 13);
    let progress = app.motion.overlay();
    render_dialog(
        frame,
        area,
        progress,
        format!(" Tutorial {}/{} ", step + 1, TUTORIAL_STEPS),
        |frame, inner| {
            let rows = Layout::vertical([
                Constraint::Length(2),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    *title,
                    Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
                )),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(text.as_str()).wrap(Wrap { trim: true }),
                rows[1],
            );
            let (next_area, skip_area) = buttons(frame, rows[2], next, skip);
            app.regions.dialog_next = next_area;
            app.regions.dialog_skip = skip_area;
            let dots = (0..TUTORIAL_STEPS)
                .map(|index| if index == step { "●" } else { "○" })
                .collect::<Vec<_>>()
                .join(" ");
            frame.render_widget(
                Paragraph::new(format!("{dots}   Left: back")).style(Style::default().fg(MUTED)),
                rows[3],
            );
        },
    );
}

/// After an update (F18): what this version brings and what it fixes.
pub(super) fn render_whats_new(frame: &mut Frame, app: &mut App, area: Rect) {
    let mut lines = vec![
        Line::from(Span::styled(
            format!("SozoIDE is now on version {}", NOTES.version),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "New",
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(
        NOTES
            .new
            .iter()
            .map(|item| Line::from(format!("  - {item}"))),
    );
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            "Fixed",
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        )),
    ]);
    lines.extend(
        NOTES
            .fixed
            .iter()
            .map(|item| Line::from(format!("  - {item}"))),
    );
    let close = " Close [Enter] ";
    let area = dialog_area(area, 80, lines.len() as u16 + 4);
    let progress = app.motion.overlay();
    render_dialog(
        frame,
        area,
        progress,
        " What's new ".to_string(),
        |frame, inner| {
            let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rows[0]);
            let (close_area, _) = buttons(frame, rows[1], close, "");
            app.regions.dialog_next = close_area;
        },
    );
}

/// A dialog of `width` × `height` cells in the middle of `area`, smaller if it does not fit.
fn dialog_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// The main button and, if there is one, the second; returns where they were drawn.
fn buttons(frame: &mut Frame, row: Rect, main: &str, second: &str) -> (Rect, Rect) {
    let parts = Layout::horizontal([
        Constraint::Length(main.len() as u16),
        Constraint::Length(2),
        Constraint::Length(second.len() as u16),
    ])
    .split(row);
    frame.render_widget(
        Paragraph::new(main.to_string()).style(
            Style::default()
                .fg(ACTIVE)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        parts[0],
    );
    if !second.is_empty() {
        frame.render_widget(
            Paragraph::new(second.to_string())
                .style(Style::default().fg(TEXT).add_modifier(Modifier::REVERSED)),
            parts[2],
        );
    }
    (
        parts[0],
        if second.is_empty() {
            Rect::default()
        } else {
            parts[2]
        },
    )
}

pub(super) fn render_update(frame: &mut Frame, app: &mut App, area: Rect, progress: f32) {
    let width = area.width.saturating_sub(4).min(64);
    let height = area.height.min(9);
    let area = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    app.regions.update_install = Rect::default();
    app.regions.update_skip = Rect::default();
    render_dialog(
        frame,
        area,
        progress,
        " Update SozoIDE ".to_string(),
        |frame, inner| render_update_state(frame, app, inner),
    );
}

fn render_update_state(frame: &mut Frame, app: &mut App, inner: Rect) {
    let rows = Layout::vertical([Constraint::Length(1); 7]).split(inner);
    let version = app.updater.version.clone();
    let muted = |text: &str| Paragraph::new(text.to_string()).style(Style::default().fg(MUTED));
    match &app.updater.state {
        UpdateState::Downloading { received, total } => {
            frame.render_widget(
                Paragraph::new(format!("Downloading SozoIDE {version}")),
                rows[0],
            );
            let megabytes = |bytes: u64| bytes as f64 / 1_000_000.0;
            let label = match total {
                Some(total) => {
                    format!("{:.1} / {:.1} MB", megabytes(*received), megabytes(*total))
                }
                None => format!("{:.1} MB", megabytes(*received)),
            };
            let parts = Layout::horizontal([
                Constraint::Min(1),
                Constraint::Length(label.len() as u16 + 2),
            ])
            .split(rows[2]);
            let width = parts[0].width;
            let line = match total {
                Some(_) => bar(width, 0.0, app.motion.download() * f32::from(width)),
                None => sweep_bar(width, app.motion.elapsed()),
            };
            frame.render_widget(Paragraph::new(line), parts[0]);
            frame.render_widget(Paragraph::new(label).alignment(Alignment::Right), parts[1]);
            frame.render_widget(
                muted("SozoIDE installs it and restarts by itself."),
                rows[4],
            );
            frame.render_widget(muted("Esc: cancel"), rows[6]);
        }
        UpdateState::Ready(_) => {
            frame.render_widget(
                Paragraph::new(format!("Installing SozoIDE {version}...")),
                rows[0],
            );
            frame.render_widget(muted("SozoIDE restarts when it is done."), rows[2]);
        }
        UpdateState::Failed(error) => {
            frame.render_widget(
                Paragraph::new(format!("The update failed: {error}")).wrap(Wrap { trim: true }),
                Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(1),
                ),
            );
            frame.render_widget(muted("Esc: close"), rows[6]);
        }
        UpdateState::Available | UpdateState::Idle => {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!("SozoIDE {version}"),
                        Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!(" is available. You have {}.", branding::VERSION)),
                ]))
                .wrap(Wrap { trim: true }),
                rows[0],
            );
            let install = " Update [Enter] ";
            let skip = " Skip this version [S] ";
            let buttons = Layout::horizontal([
                Constraint::Length(install.len() as u16),
                Constraint::Length(2),
                Constraint::Length(skip.len() as u16),
            ])
            .split(rows[2]);
            frame.render_widget(
                Paragraph::new(install).style(
                    Style::default()
                        .fg(ACTIVE)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                ),
                buttons[0],
            );
            frame.render_widget(
                Paragraph::new(skip)
                    .style(Style::default().fg(TEXT).add_modifier(Modifier::REVERSED)),
                buttons[2],
            );
            app.regions.update_install = buttons[0];
            app.regions.update_skip = buttons[2];
            frame.render_widget(
                muted("SozoIDE downloads it, installs it and restarts."),
                rows[4],
            );
            frame.render_widget(muted("Esc: later"), rows[6]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_adapts_to_each_os() {
        let mac = help_lines("macos").join("\n");
        assert!(mac.contains("Ctrl+Option+B      Toggle right terminal"));
        assert!(mac.contains("Option as Meta"));
        assert!(mac.contains("fn+F1"));
        assert!(mac.contains("fn+F5"));
        assert!(mac.contains("replaces SozoIDE.app"));
        assert!(!mac.contains("Ctrl+Shift+B"));

        for (os, installer) in [("windows", "MSI installer"), ("linux", "sudo password")] {
            let text = help_lines(os).join("\n");
            assert!(text.contains("Ctrl+Shift+B       Toggle right terminal"));
            assert!(text.contains("  F1                 Help"));
            assert!(text.contains("Ctrl+Enter/F5"));
            assert!(text.contains(installer));
            assert!(!text.contains("Option"));
            assert!(!text.contains("fn+"));
        }
        assert_eq!(os_name("windows"), "Windows");
    }

    #[test]
    fn the_flower_grows_from_its_centre() {
        assert!(bloom_logo(0.0).iter().all(|line| line.trim().is_empty()));
        assert_eq!(bloom_logo(1.0), ASCII_LOGO);
        let half = bloom_logo(0.45);
        let middle = &half[ASCII_LOGO.len() / 2];
        assert!(middle.contains('@'), "{middle}");
        assert!(half[0].trim().is_empty(), "the top petal is not out yet");
        assert!(
            half.iter().any(|line| line.contains('.')),
            "sprouts at the edge"
        );
    }

    #[test]
    fn the_tutorial_names_the_keys_of_each_system() {
        let mac = tutorial_steps("macos").map(|(_, text)| text).join("\n");
        assert!(mac.contains("Ctrl+Option+B") && mac.contains("fn+F1"));
        let linux = tutorial_steps("linux").map(|(_, text)| text).join("\n");
        assert!(linux.contains("Ctrl+Shift+B") && !linux.contains("fn+"));
    }
}
