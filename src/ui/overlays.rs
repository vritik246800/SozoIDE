use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::{
    app::{App, AppMotion},
    branding::{self, ASCII_LOGO},
    editor::SearchScope,
    update::UpdateState,
};

use super::{ACTIVE, MUTED, TEXT, bar, centered, render_dialog, scale_length, sweep_bar};

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
    lines.push("  Ctrl+Q             Quit");
    lines
}

pub(super) fn render_about(frame: &mut Frame, area: Rect, progress: f32) {
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
        Line::from(format!("Created by {}", branding::AUTHOR)),
        Line::from("A terminal-native integrated development environment"),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(MUTED),
        )),
    ]);
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
        },
    );
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
}
