use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::{
    git::{ChangeArea, CommitInfo, GitPanel, GitTab},
    ui::{ACTIVE, MUTED, TEXT, busy, centered, panel_block, render_dialog},
};

use super::controller::{GitController, GitMode, GitUiAction};

pub fn render(frame: &mut Frame, controller: &mut GitController, area: Rect) {
    controller.regions.area = area;
    controller.regions.content = Rect::default();
    controller.regions.actions.clear();
    let block = panel_block(" Source Control [Ctrl+G] ".into(), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(inner);
    for (index, tab) in GitTab::ALL.into_iter().enumerate() {
        let row = index / 5;
        let column = index % 5;
        let width = sections[0].width / 5;
        let button_area = Rect::new(
            sections[0].x + column as u16 * width,
            sections[0].y + row as u16,
            width.saturating_sub(1),
            1,
        );
        let selected = controller
            .panel
            .try_lock()
            .map(|panel| panel.active_tab == tab)
            .unwrap_or(false);
        action_button(
            frame,
            controller,
            button_area,
            tab.label(),
            GitUiAction::Tab(tab),
            selected,
        );
    }
    let content_area = sections[2];
    controller.regions.content = content_area;
    let panel_handle = std::sync::Arc::clone(&controller.panel);
    let Ok(panel) = panel_handle.try_lock() else {
        let message = busy(controller.motion.spinner(), "Git operation in progress...");
        frame.render_widget(Paragraph::new(message.clone()), content_area);
        frame.render_widget(Paragraph::new(message), sections[3]);
        return;
    };
    render_toolbar(frame, controller, &panel, sections[1]);
    if !panel.in_repository() {
        frame.render_widget(
            Paragraph::new("This directory is not inside a Git repository.")
                .style(Style::default().fg(MUTED)),
            content_area,
        );
    } else {
        match panel.active_tab {
            GitTab::Changes => render_changes(frame, controller, &panel, content_area),
            GitTab::CommitGraph => render_graph(frame, controller, &panel, content_area),
            GitTab::Branches => render_branches(frame, controller, &panel, content_area),
            GitTab::Diff => render_diff(frame, &panel, content_area),
            GitTab::Launchpad => render_launchpad(frame, &panel, content_area),
            GitTab::Inspect => render_inspect(frame, &panel, content_area),
            GitTab::LineHistory => render_line_history(frame, controller, &panel, content_area),
            GitTab::FileHistory => render_file_history(frame, controller, &panel, content_area),
            GitTab::VisualFileHistory => {
                render_visual_history(frame, controller, &panel, content_area)
            }
            GitTab::Health => render_health(frame, controller, &panel, content_area),
        }
    }
    let message = if controller.operation {
        busy(controller.motion.spinner(), "Working...")
    } else {
        Line::from(panel.last_error.clone().unwrap_or_else(|| {
            panel
                .last_output
                .as_ref()
                .map(|output| format!("{} — {}", output.command, panel.status_message))
                .unwrap_or_else(|| panel.status_message.clone())
        }))
    };
    let error = !controller.operation && panel.last_error.is_some();
    frame.render_widget(
        Paragraph::new(message).style(Style::default().fg(if error { Color::Red } else { MUTED })),
        sections[3],
    );
}

fn render_toolbar(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    let actions: &[(&str, GitUiAction)] = match panel.active_tab {
        GitTab::Changes => &[
            ("Refresh", GitUiAction::Refresh),
            ("Stage", GitUiAction::Stage),
            ("Unstage", GitUiAction::Unstage),
            ("Stage All", GitUiAction::StageAll),
            ("Commit", GitUiAction::OpenCommit),
            ("Diff", GitUiAction::ShowDiff),
            ("Pull", GitUiAction::Pull),
            ("Push", GitUiAction::Push),
            ("Fetch", GitUiAction::Fetch),
            ("Rebase", GitUiAction::OpenRebase),
            ("Line History", GitUiAction::LineHistory),
            ("File History", GitUiAction::FileHistory),
        ],
        GitTab::CommitGraph => &[
            ("Refresh", GitUiAction::Refresh),
            ("Inspect", GitUiAction::Inspect),
            ("Pull", GitUiAction::Pull),
            ("Push", GitUiAction::Push),
            ("Fetch", GitUiAction::Fetch),
            ("Rebase", GitUiAction::OpenRebase),
        ],
        GitTab::Branches => &[
            ("Refresh", GitUiAction::Refresh),
            ("Checkout", GitUiAction::Checkout),
            ("Pull", GitUiAction::Pull),
            ("Push", GitUiAction::Push),
            ("Fetch", GitUiAction::Fetch),
            ("Rebase", GitUiAction::OpenRebase),
        ],
        GitTab::Launchpad | GitTab::Health => &[
            ("Refresh", GitUiAction::Refresh),
            ("Pull", GitUiAction::Pull),
            ("Push", GitUiAction::Push),
            ("Fetch", GitUiAction::Fetch),
            ("Rebase", GitUiAction::OpenRebase),
        ],
        GitTab::FileHistory => &[
            ("Refresh", GitUiAction::Refresh),
            ("Visual File History", GitUiAction::VisualFileHistory),
        ],
        _ => &[("Refresh", GitUiAction::Refresh)],
    };
    let width = (area.width / 6).max(12);
    for (index, (label, action)) in actions.iter().enumerate() {
        let row = index / 6;
        let column = index % 6;
        if row > 1 {
            break;
        }
        action_button(
            frame,
            controller,
            Rect::new(
                area.x + column as u16 * width,
                area.y + row as u16,
                width.saturating_sub(1),
                1,
            ),
            label,
            action.clone(),
            false,
        );
    }
}

fn render_changes(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    let mut lines = Vec::new();
    let mut index = 0;
    for (title, area_kind, entries) in [
        ("Staged", ChangeArea::Staged, &panel.status.staged),
        ("Unstaged", ChangeArea::Unstaged, &panel.status.unstaged),
        ("Untracked", ChangeArea::Untracked, &panel.status.untracked),
    ] {
        lines.push(Line::from(Span::styled(
            format!("{title} ({})", entries.len()),
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        )));
        for change in entries {
            let row = area.y + lines.len() as u16;
            if row < area.bottom() {
                controller.regions.actions.push((
                    GitUiAction::Select(index),
                    Rect::new(area.x, row, area.width, 1),
                ));
            }
            let style = if panel.selected == index {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "  {:?}  {:?}  {}",
                    area_kind,
                    change.status,
                    change.path.display()
                ),
                style,
            )));
            index += 1;
        }
    }
    if index == 0 {
        lines.push(Line::from("Working tree clean."));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_graph(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut assigned = HashMap::<String, usize>::new();
    let lines = panel
        .commits
        .iter()
        .enumerate()
        .map(|(index, commit)| {
            let lane = assigned
                .remove(&commit.id)
                .or_else(|| {
                    lanes
                        .iter()
                        .position(|entry| entry == &Some(commit.id.clone()))
                })
                .unwrap_or_else(|| allocate_lane(&mut lanes));
            if lane >= lanes.len() {
                lanes.resize(lane + 1, None);
            }
            lanes[lane] = commit.parent_ids.first().cloned();
            if let Some(parent) = commit.parent_ids.first() {
                assigned.insert(parent.clone(), lane);
            }
            for parent in commit.parent_ids.iter().skip(1) {
                let parent_lane = allocate_lane(&mut lanes);
                lanes[parent_lane] = Some(parent.clone());
                assigned.insert(parent.clone(), parent_lane);
            }
            let graph = if commit.parent_ids.len() > 1 {
                format!("{}*─╮", "  ".repeat(lane))
            } else {
                format!("{}* ", "  ".repeat(lane))
            };
            selectable_line(controller, index, panel.selected, area, graph, commit)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn allocate_lane(lanes: &mut Vec<Option<String>>) -> usize {
    if let Some(index) = lanes.iter().position(Option::is_none) {
        index
    } else {
        lanes.push(None);
        lanes.len() - 1
    }
}

fn selectable_line(
    controller: &mut GitController,
    index: usize,
    selected: usize,
    area: Rect,
    graph: String,
    commit: &CommitInfo,
) -> Line<'static> {
    let row = area.y + index as u16;
    if row < area.bottom() {
        controller.regions.actions.push((
            GitUiAction::Select(index),
            Rect::new(area.x, row, area.width, 1),
        ));
    }
    Line::from(Span::styled(
        format!(
            "{graph} {} {}  {}  {}",
            commit.short_id,
            commit.committed_at.format("%Y-%m-%d"),
            commit.author,
            commit.summary
        ),
        if selected == index {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        },
    ))
}

fn render_branches(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    let lines = panel
        .branches
        .iter()
        .enumerate()
        .map(|(index, branch)| {
            let row = area.y + index as u16;
            if row < area.bottom() {
                controller.regions.actions.push((
                    GitUiAction::Select(index),
                    Rect::new(area.x, row, area.width, 1),
                ));
            }
            Line::from(Span::styled(
                format!(
                    "{} {}{}  {}  {}  {}  +{} -{}",
                    if branch.is_head { "*" } else { " " },
                    branch.name,
                    if branch.is_remote { " [remote]" } else { "" },
                    branch.full_name,
                    branch.commit_id.as_deref().unwrap_or("unborn"),
                    branch.upstream.as_deref().unwrap_or("no upstream"),
                    branch.ahead,
                    branch.behind
                ),
                if panel.selected == index {
                    Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                },
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_diff(frame: &mut Frame, panel: &GitPanel, area: Rect) {
    let lines = std::iter::once(Line::from(Span::styled(
        panel.diff.title.clone(),
        Style::default().fg(ACTIVE),
    )))
    .chain(panel.diff.text.lines().skip(panel.selected).map(|line| {
        let color = if line.starts_with('+') {
            Color::Green
        } else if line.starts_with('-') {
            Color::Red
        } else if line.starts_with('@') {
            Color::Cyan
        } else {
            TEXT
        };
        Line::from(Span::styled(line.to_owned(), Style::default().fg(color)))
    }))
    .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_launchpad(frame: &mut Frame, panel: &GitPanel, area: Rect) {
    let summary = &panel.launchpad;
    let lines = vec![
        Line::from(format!(
            "Current branch: {}",
            summary.current_branch.as_deref().unwrap_or("detached HEAD")
        )),
        Line::from(format!(
            "Upstream: {}",
            summary.upstream.as_deref().unwrap_or("none")
        )),
        Line::from(format!(
            "Ahead: {}  Behind: {}",
            summary.ahead, summary.behind
        )),
        Line::from(format!(
            "Staged: {}  Unstaged: {}  Untracked: {}",
            summary.staged_count, summary.unstaged_count, summary.untracked_count
        )),
        Line::from(format!(
            "Last commit: {}",
            summary
                .last_commit
                .as_ref()
                .map(|commit| commit.summary.as_str())
                .unwrap_or("none")
        )),
        Line::from(""),
        Line::from(Span::styled("Remotes", Style::default().fg(ACTIVE))),
    ];
    let lines = lines
        .into_iter()
        .chain(summary.remotes.iter().map(|remote| {
            Line::from(format!(
                "{}  {}",
                remote.name,
                remote.url.as_deref().unwrap_or("no URL")
            ))
        }))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_inspect(frame: &mut Frame, panel: &GitPanel, area: Rect) {
    let Some(details) = &panel.inspected_commit else {
        frame.render_widget(
            Paragraph::new("Select a commit in Commit Graph and choose Inspect."),
            area,
        );
        return;
    };
    let mut lines = vec![
        Line::from(format!("Commit: {}", details.commit.id)),
        Line::from(format!(
            "Author: {} <{}>",
            details.commit.author, details.commit.author_email
        )),
        Line::from(format!(
            "Date: {}",
            details.commit.committed_at.to_rfc2822()
        )),
        Line::from(""),
        Line::from(details.message.clone()),
        Line::from(""),
        Line::from(Span::styled("Files", Style::default().fg(ACTIVE))),
    ];
    lines.extend(details.files.iter().map(|file| {
        Line::from(format!(
            "{:?}  {}{}  +{} -{}",
            file.status,
            file.old_path
                .as_ref()
                .map(|path| format!("{} -> ", path.display()))
                .unwrap_or_default(),
            file.path.display(),
            file.additions,
            file.deletions
        ))
    }));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_line_history(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    let lines = panel
        .line_history
        .iter()
        .enumerate()
        .skip(panel.selected)
        .map(|(index, line)| {
            selectable_row(
                controller,
                index,
                panel.selected,
                area,
                format!(
                    "{:>5} (origin {:>5})  {}  {}  {}  {:<18}  {}  {}",
                    line.line_number,
                    line.original_line_number,
                    line.short_id,
                    line.commit_id,
                    line.committed_at.format("%Y-%m-%d"),
                    line.author,
                    line.summary,
                    line.content
                ),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_file_history(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    let selected_path = panel
        .selected_file_history()
        .map(|entry| entry.path.clone())
        .unwrap_or_default();
    let lines = std::iter::once(Line::from(Span::styled(
        format!("History for {}", selected_path.display()),
        Style::default().fg(ACTIVE),
    )))
    .chain(panel.file_history.iter().enumerate().map(|(index, entry)| {
        selectable_line(
            controller,
            index,
            panel.selected,
            area,
            format!(
                "{:?}  {}{}  ",
                entry.status,
                entry
                    .old_path
                    .as_ref()
                    .map(|path| format!("{} -> ", path.display()))
                    .unwrap_or_default(),
                entry.path.display()
            ),
            &entry.commit,
        )
    }))
    .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_visual_history(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    let lines = panel
        .visual_file_history
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let graph = format!(
                "{}*{}",
                "  ".repeat(entry.lane),
                if entry.parent_ids.len() > 1 {
                    "─╮"
                } else {
                    "──"
                }
            );
            selectable_row(
                controller,
                index,
                panel.selected,
                area,
                format!(
                    "{graph} {}  {}  {}  {}  {:?}",
                    entry.short_id,
                    entry.commit_id,
                    entry.committed_at.format("%Y-%m-%d"),
                    entry.summary,
                    entry.status
                ),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_health(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    let health = &panel.health;
    let mut lines = vec![
        Line::from(format!(
            "Working tree: {}",
            if health.is_clean {
                "clean"
            } else {
                "changes present"
            }
        )),
        Line::from(format!("Upstream configured: {}", health.has_upstream)),
        Line::from(format!(
            "Ahead: {}  Behind: {}",
            health.ahead, health.behind
        )),
        Line::from(format!(
            "Operation: {}",
            health.operation.as_deref().unwrap_or("none")
        )),
        Line::from(format!("Conflicts: {}", health.conflicts.len())),
    ];
    lines.extend(health.conflicts.iter().enumerate().map(|(index, path)| {
        selectable_row(
            controller,
            index,
            panel.selected,
            area,
            format!("CONFLICT  {}", path.display()),
        )
    }));
    frame.render_widget(Paragraph::new(lines), area);
}

fn selectable_row(
    controller: &mut GitController,
    index: usize,
    selected: usize,
    area: Rect,
    text: String,
) -> Line<'static> {
    let row = area.y + index as u16;
    if row < area.bottom() {
        controller.regions.actions.push((
            GitUiAction::Select(index),
            Rect::new(area.x, row, area.width, 1),
        ));
    }
    Line::from(Span::styled(
        text,
        if index == selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        },
    ))
}

/// The Commit, Rebase and confirmation dialogs, drawn by `ui::draw` over everything else.
pub fn render_mode(frame: &mut Frame, controller: &mut GitController, area: Rect) {
    let (title, prompt, confirmation) = match &controller.mode {
        GitMode::Commit(message) => ("Commit changes", format!("Message: {message}"), false),
        GitMode::Rebase(target) => ("Rebase", format!("Target branch/commit: {target}"), false),
        GitMode::ConfirmRebase(target) => (
            "Confirm rebase",
            format!("Rebase onto {target}? This can rewrite history."),
            true,
        ),
        GitMode::ConfirmCheckout(branch) => (
            "Confirm checkout",
            format!("Checkout {branch}? The working tree must be clean."),
            true,
        ),
        GitMode::None => return,
    };
    let progress = controller.motion.dialog();
    render_dialog(
        frame,
        centered(area, 60, 34),
        progress,
        format!(" {title} "),
        |frame, inner| {
            if inner.height < 2 {
                return;
            }
            let sections = Layout::vertical([
                Constraint::Min(2),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new(prompt).wrap(Wrap { trim: false }),
                sections[0],
            );
            action_button(
                frame,
                controller,
                Rect::new(sections[1].x, sections[1].y, 18, 1),
                if confirmation { "Confirm" } else { "Continue" },
                GitUiAction::Submit,
                true,
            );
            action_button(
                frame,
                controller,
                Rect::new(sections[1].x + 20, sections[1].y, 12, 1),
                "Cancel",
                GitUiAction::Cancel,
                false,
            );
            frame.render_widget(
                Paragraph::new("Enter: confirm  Esc: cancel").style(Style::default().fg(MUTED)),
                sections[2],
            );
        },
    );
}

fn action_button(
    frame: &mut Frame,
    controller: &mut GitController,
    area: Rect,
    label: &str,
    action: GitUiAction,
    selected: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let style = if selected {
        Style::default()
            .fg(ACTIVE)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(TEXT)
    };
    frame.render_widget(Paragraph::new(format!(" {label} ")).style(style), area);
    controller.regions.actions.push((action, area));
}
