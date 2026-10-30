use chrono::{DateTime, FixedOffset, Local};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Wrap},
};

use crate::{
    git::{
        BranchRow, ChangeArea, ChangeEntry, CommitInfo, DiffLine, DiffLineKind, DiffRow,
        FileStatus, GitPanel, GitTab, LineChange, TimelineRow, branch_tree, line_changes, timeline,
    },
    ui::{ACTIVE, BORDER, MUTED, TEXT, busy, centered, panel_block, render_dialog, scale_length},
};

use super::controller::{GitController, GitMode, GitUiAction, TimelineStage};

/// `key` is the key the user gave Source Control (F29).
pub fn render(frame: &mut Frame, controller: &mut GitController, area: Rect, key: &str) {
    controller.regions.area = area;
    controller.regions.content = Rect::default();
    controller.regions.actions.clear();
    let block = panel_block(format!(" Source Control [{key}] "), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [tabs, toolbar, content, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(inner);
    let active = controller
        .panel
        .try_lock()
        .map(|panel| panel.active_tab)
        .ok();
    let tab_buttons =
        GitTab::ALL.map(|tab| (tab.label(), GitUiAction::Tab(tab), active == Some(tab)));
    buttons(frame, controller, tabs, &tab_buttons);
    controller.regions.content = content;
    let panel_handle = std::sync::Arc::clone(&controller.panel);
    let Ok(panel) = panel_handle.try_lock() else {
        let message = busy(controller.motion.spinner(), "Git operation in progress...");
        frame.render_widget(Paragraph::new(message.clone()), content);
        frame.render_widget(Paragraph::new(message), status);
        return;
    };
    render_toolbar(frame, controller, &panel, toolbar);
    if !panel.in_repository() {
        frame.render_widget(
            Paragraph::new("This directory is not inside a Git repository.")
                .style(Style::default().fg(MUTED)),
            content,
        );
    } else {
        match panel.active_tab {
            GitTab::Changes => render_changes(frame, controller, &panel, content),
            GitTab::CommitGraph => render_graph(frame, controller, &panel, content),
            GitTab::Branches => render_branches(frame, controller, &panel, content),
            GitTab::Diff => render_diff(frame, controller, &panel, content),
            GitTab::Launchpad => render_launchpad(frame, controller, &panel, content),
            GitTab::Inspect => render_inspect(frame, &panel, content),
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
        status,
    );
}

fn render_toolbar(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    let split = if panel.diff_split {
        "Unified [v]"
    } else {
        "Side by side [v]"
    };
    let actions: &[(&str, GitUiAction)] = match panel.active_tab {
        GitTab::Changes => &[
            ("Refresh", GitUiAction::Refresh),
            ("Stage [s]", GitUiAction::Stage),
            ("Unstage [u]", GitUiAction::Unstage),
            ("Stage All", GitUiAction::StageAll),
            ("Commit [c]", GitUiAction::OpenCommit),
            ("Diff [Enter]", GitUiAction::ShowDiff),
            ("Pull", GitUiAction::Pull),
            ("Push", GitUiAction::Push),
            ("Fetch", GitUiAction::Fetch),
            ("Rebase", GitUiAction::OpenRebase),
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
            ("Checkout [Enter]", GitUiAction::Checkout),
            ("Fetch", GitUiAction::Fetch),
            ("Pull", GitUiAction::Pull),
            ("Push", GitUiAction::Push),
            ("Rebase", GitUiAction::OpenRebase),
        ],
        GitTab::Diff => &[
            ("◀ Prev file [", GitUiAction::PreviousFile),
            ("Next file ▶ ]", GitUiAction::NextFile),
            ("Stage [s]", GitUiAction::StageDiff),
            ("Unstage [u]", GitUiAction::UnstageDiff),
            ("Open [o]", GitUiAction::OpenFile),
            (split, GitUiAction::ToggleSplit),
            ("Refresh", GitUiAction::Refresh),
        ],
        GitTab::Launchpad => &[
            ("Refresh", GitUiAction::Refresh),
            ("Fetch", GitUiAction::Fetch),
            ("Pull", GitUiAction::Pull),
            ("Push", GitUiAction::Push),
            ("Rebase", GitUiAction::OpenRebase),
        ],
        GitTab::Inspect => &[("Refresh", GitUiAction::Refresh)],
    };
    let row = Rect::new(area.x, area.y, area.width, 1);
    let actions = actions
        .iter()
        .map(|(label, action)| (*label, action.clone(), false))
        .collect::<Vec<_>>();
    buttons(frame, controller, row, &actions);
}

/// Buttons side by side, each as wide as its label; the ones that do not fit are left out.
fn buttons(
    frame: &mut Frame,
    controller: &mut GitController,
    area: Rect,
    buttons: &[(&str, GitUiAction, bool)],
) {
    let mut x = area.x;
    for (label, action, selected) in buttons {
        let width = Line::from(*label).width() as u16 + 2;
        if x + width > area.right() {
            break;
        }
        action_button(
            frame,
            controller,
            Rect::new(x, area.y, width, 1),
            label,
            action.clone(),
            *selected,
        );
        x += width + 1;
    }
}

/// The first row to draw so that row `selected` stays inside `height` rows.
fn scroll_to(selected: usize, height: usize) -> usize {
    (selected + 1).saturating_sub(height.max(1))
}

/// A whole row in reverse video when it is the selected one.
fn row_style(selected: bool) -> Style {
    if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    }
}

/// The letter and colour of a change: added, modified, deleted, renamed…
fn status_mark(status: FileStatus, area: ChangeArea) -> (&'static str, Color) {
    match (status, area) {
        (_, ChangeArea::Untracked) => ("U", Color::Green),
        (FileStatus::Added, _) => ("A", Color::Green),
        (FileStatus::Modified, _) => ("M", Color::Yellow),
        (FileStatus::Deleted, _) => ("D", Color::Red),
        (FileStatus::Renamed, _) => ("R", Color::Blue),
        (FileStatus::TypeChanged, _) => ("T", Color::Magenta),
        (FileStatus::Conflict, _) => ("!", Color::Red),
        (FileStatus::Unknown, _) => ("?", MUTED),
    }
}

/// How long ago `time` was, counted from `now`.
fn ago_from(now: DateTime<FixedOffset>, time: DateTime<FixedOffset>) -> String {
    let seconds = (now - time).num_seconds().max(0);
    let (amount, unit) = match seconds {
        0..60 => return "just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..2_592_000 => (seconds / 86_400, "day"),
        2_592_000..31_536_000 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    format!("{amount} {unit}{} ago", if amount == 1 { "" } else { "s" })
}

fn ago(time: DateTime<FixedOffset>) -> String {
    ago_from(Local::now().fixed_offset(), time)
}

fn render_changes(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    let mut lines = Vec::new();
    let mut targets = Vec::new();
    let mut index = 0;
    for (title, entries) in [
        ("Staged", &panel.status.staged),
        ("Unstaged", &panel.status.unstaged),
        ("Untracked", &panel.status.untracked),
    ] {
        lines.push(Line::from(Span::styled(
            format!("{title} ({})", entries.len()),
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        )));
        targets.push(None);
        for change in entries {
            lines.push(change_line(change, panel.selected == index));
            targets.push(Some(index));
            index += 1;
        }
    }
    if index == 0 {
        lines.push(Line::from("Working tree clean."));
        targets.push(None);
    }
    let selected_row = targets
        .iter()
        .position(|target| *target == Some(panel.selected))
        .unwrap_or(0);
    let offset = scroll_to(selected_row, area.height.into());
    for (row, target) in targets.iter().enumerate().skip(offset) {
        let y = area.y + (row - offset) as u16;
        if y >= area.bottom() {
            break;
        }
        if let Some(index) = target {
            controller.regions.actions.push((
                GitUiAction::Select(*index),
                Rect::new(area.x, y, area.width, 1),
            ));
        }
    }
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), area);
}

fn change_line(change: &ChangeEntry, selected: bool) -> Line<'static> {
    let (mark, color) = status_mark(change.status, change.area);
    let path = match &change.old_path {
        Some(old) => format!("{} → {}", old.display(), change.path.display()),
        None => change.path.display().to_string(),
    };
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            mark,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {path}")),
    ])
    .style(row_style(selected))
}

/// The colours of the timeline's lanes, from the terminal's palette (`DESIGN.md` §1.1).
const LANE_COLOURS: [Color; 6] = [
    Color::Cyan,
    Color::Green,
    Color::Magenta,
    Color::Yellow,
    Color::Blue,
    Color::Red,
];

fn lane_colour(colour: usize) -> Color {
    LANE_COLOURS[colour % LANE_COLOURS.len()]
}

/// The timeline (F06, Q36): every branch as a coloured lane of dots and lines; a commit
/// clicked opens its files on the right, and a file its diff over the whole tab.
fn render_graph(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    if controller.timeline == TimelineStage::Diff {
        render_timeline_diff(frame, controller, panel, area);
        return;
    }
    let rows = timeline(&panel.commits);
    let offset = scroll_to(panel.selected, area.height.into());
    let shown = offset..(offset + usize::from(area.height)).min(rows.len());
    let graph_width = rows[shown.clone()]
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(0);
    let lines = shown
        .map(|index| {
            controller.regions.actions.push((
                GitUiAction::OpenCommitFiles(index),
                Rect::new(area.x, area.y + (index - offset) as u16, area.width, 1),
            ));
            timeline_line(
                &rows[index],
                graph_width,
                &panel.commits[index],
                panel.selected == index,
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
    if controller.timeline == TimelineStage::Files {
        let width = scale_length(area.width.saturating_mul(9) / 20, controller.motion.side());
        let side = Rect::new(area.right() - width, area.y, width, area.height);
        render_commit_files(frame, controller, panel, side);
    }
}

fn timeline_line(
    row: &TimelineRow,
    width: usize,
    commit: &CommitInfo,
    selected: bool,
) -> Line<'static> {
    let colour = row
        .cells
        .iter()
        .find(|(symbol, _)| *symbol == '●')
        .and_then(|(_, colour)| *colour)
        .map_or(ACTIVE, lane_colour);
    let mut spans = row
        .cells
        .iter()
        .map(|(symbol, lane)| {
            Span::styled(
                symbol.to_string(),
                lane.map_or(Style::default(), |lane| {
                    Style::default().fg(lane_colour(lane))
                }),
            )
        })
        .collect::<Vec<_>>();
    spans.push(Span::raw(
        " ".repeat(width.saturating_sub(row.cells.len()) + 1),
    ));
    spans.push(Span::styled(
        commit.short_id.clone(),
        Style::default().fg(Color::Yellow),
    ));
    for label in &commit.refs {
        let style = if label.starts_with("HEAD") {
            Style::default().fg(colour).add_modifier(Modifier::BOLD)
        } else if label.starts_with("tag:") {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(colour)
        };
        spans.push(Span::styled(format!(" ({label})"), style));
    }
    spans.push(Span::raw(format!(" {}", commit.summary)));
    spans.push(Span::styled(
        format!("  {} · {}", commit.author, ago(commit.committed_at)),
        Style::default().fg(MUTED),
    ));
    Line::from(spans).style(row_style(selected))
}

/// The commit's files in a panel from the right edge, each with its status and line counts.
fn render_commit_files(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    if area.width < 4 {
        return;
    }
    frame.render_widget(ratatui::widgets::Clear, area);
    let commit = panel.selected_commit();
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACTIVE))
        .title(Span::styled(
            format!(" Commit {} ", commit.map_or("", |commit| &commit.short_id)),
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let details = panel
        .inspected_commit
        .as_ref()
        .filter(|details| Some(&details.commit.id) == commit.map(|commit| &commit.id));
    let Some(details) = details else {
        frame.render_widget(
            Paragraph::new(busy(controller.motion.spinner(), "Loading the files...")),
            inner,
        );
        return;
    };
    let (added, removed) = details.files.iter().fold((0, 0), |(added, removed), file| {
        (added + file.additions, removed + file.deletions)
    });
    let mut lines = vec![
        Line::from(Span::styled(
            details.commit.summary.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "{} · {}",
                details.commit.author,
                details.commit.committed_at.format("%Y-%m-%d %H:%M")
            ),
            Style::default().fg(MUTED),
        )),
        Line::from(vec![
            Span::raw(format!(
                "{} file{} ",
                details.files.len(),
                if details.files.len() == 1 { "" } else { "s" }
            )),
            Span::styled(format!("+{added} "), Style::default().fg(Color::Green)),
            Span::styled(format!("−{removed}"), Style::default().fg(Color::Red)),
        ]),
        Line::default(),
    ];
    let list_top = inner.y + lines.len() as u16;
    let room = inner.height.saturating_sub(lines.len() as u16 + 1) as usize;
    let first = scroll_to(controller.timeline_file, room);
    for (index, file) in details.files.iter().enumerate().skip(first).take(room) {
        let (mark, colour) = status_mark(file.status, ChangeArea::Staged);
        let y = list_top + (index - first) as u16;
        controller.regions.actions.push((
            GitUiAction::TimelineFile(index),
            Rect::new(inner.x, y, inner.width, 1),
        ));
        lines.push(
            Line::from(vec![
                Span::styled(
                    format!(" {mark} "),
                    Style::default().fg(colour).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("{} ", file.path.display())),
                Span::styled(
                    format!("+{} ", file.additions),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("−{}", file.deletions),
                    Style::default().fg(Color::Red),
                ),
            ])
            .style(row_style(index == controller.timeline_file)),
        );
    }
    frame.render_widget(Paragraph::new(lines), inner);
    let hint = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    frame.render_widget(
        Paragraph::new("Enter: diff  Esc: close").style(Style::default().fg(MUTED)),
        hint,
    );
}

/// The diff of one file of the commit (Q38): green added, red removed, yellow modified.
fn render_timeline_diff(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    let [header, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    let file = panel
        .inspected_commit
        .as_ref()
        .and_then(|details| details.files.get(controller.timeline_file));
    let Some(file) = file else {
        return;
    };
    let mut x = header.x;
    let back = "‹ Files";
    action_button(
        frame,
        controller,
        Rect::new(x, header.y, back.chars().count() as u16 + 2, 1),
        back,
        GitUiAction::TimelineBack,
        false,
    );
    x += back.chars().count() as u16 + 3;
    let (mark, colour) = status_mark(file.status, ChangeArea::Staged);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{mark} "),
                Style::default().fg(colour).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                file.path.display().to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  +{}", file.additions),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!(" −{}", file.deletions),
                Style::default().fg(Color::Red),
            ),
            Span::styled(
                "   [ ]: file  Up/Down: scroll  Esc: back",
                Style::default().fg(MUTED),
            ),
        ])),
        Rect::new(x, header.y, header.right().saturating_sub(x), 1),
    );
    let diff = panel.diff.files.iter().find(|diff| diff.path == file.path);
    let Some(diff) = diff.filter(|diff| !diff.binary) else {
        frame.render_widget(
            Paragraph::new("No text changes to show.").style(Style::default().fg(MUTED)),
            body,
        );
        return;
    };
    let digits = diff
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .flat_map(|line| line.old.into_iter().chain(line.new))
        .max()
        .unwrap_or(0)
        .to_string()
        .len();
    let mut lines = Vec::new();
    for hunk in &diff.hunks {
        lines.push(Line::from(Span::styled(
            hunk.header.trim_end().to_owned(),
            Style::default().fg(ACTIVE),
        )));
        for (line, change) in hunk.lines.iter().zip(line_changes(&hunk.lines)) {
            // A modified line shows its old text dimmed above the new one.
            let old = line.kind == DiffLineKind::Removed;
            let (sign, style) = match change {
                LineChange::Added => ("+", Style::default().fg(Color::Green)),
                LineChange::Removed => ("-", Style::default().fg(Color::Red)),
                LineChange::Modified if old => (
                    "-",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::DIM),
                ),
                LineChange::Modified => ("+", Style::default().fg(Color::Yellow)),
                LineChange::Context => (" ", Style::default().fg(TEXT)),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} {} ", number(line.old, digits), number(line.new, digits)),
                    Style::default().fg(MUTED),
                ),
                Span::styled(format!("{sign} {}", line.text), style),
            ]));
        }
    }
    controller.timeline_scroll = controller
        .timeline_scroll
        .min(lines.len().saturating_sub(usize::from(body.height)));
    frame.render_widget(
        Paragraph::new(lines).scroll((controller.timeline_scroll as u16, 0)),
        body,
    );
}

/// Local branches and each remote as a tree, folders for the `/` in the names; a click on a
/// branch asks to switch to it.
fn render_branches(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    let rows = branch_tree(&panel.branches);
    let selected_row = rows
        .iter()
        .position(|row| row.branch == Some(panel.selected))
        .unwrap_or(0);
    let offset = scroll_to(selected_row, area.height.into());
    let lines = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(area.height.into())
        .map(|(index, row)| {
            if let Some(branch) = row.branch {
                controller.regions.actions.push((
                    GitUiAction::Branch(branch),
                    Rect::new(area.x, area.y + (index - offset) as u16, area.width, 1),
                ));
            }
            branch_line(panel, &rows, index, row)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn branch_line(
    panel: &GitPanel,
    rows: &[BranchRow],
    index: usize,
    row: &BranchRow,
) -> Line<'static> {
    let guide = Style::default().fg(BORDER);
    let mut spans = row
        .guides
        .iter()
        .map(|line| Span::styled(if *line { "│  " } else { "   " }, guide))
        .collect::<Vec<_>>();
    if !row.group {
        spans.push(Span::styled(if row.last { "└─ " } else { "├─ " }, guide));
    }
    let Some(branch_index) = row.branch else {
        if row.group {
            let count = rows[index + 1..]
                .iter()
                .take_while(|next| !next.group)
                .filter(|next| next.branch.is_some())
                .count();
            spans.push(Span::styled(
                row.label.clone(),
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" ({count})"),
                Style::default().fg(MUTED),
            ));
        } else {
            spans.push(Span::styled(
                format!("{}/", row.label),
                Style::default().fg(Color::Blue),
            ));
        }
        return Line::from(spans);
    };
    let branch = &panel.branches[branch_index];
    if branch.is_head {
        spans.push(Span::styled(
            format!("● {}", row.label),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::raw(format!("  {}", row.label)));
    }
    if let Some(upstream) = &branch.upstream {
        spans.push(Span::styled(
            format!("  ⇄ {upstream}"),
            Style::default().fg(MUTED),
        ));
        for (count, arrow, color) in [
            (branch.ahead, "↑", Color::Green),
            (branch.behind, "↓", Color::Yellow),
        ] {
            if count > 0 {
                spans.push(Span::styled(
                    format!(" {arrow}{count}"),
                    Style::default().fg(color),
                ));
            }
        }
    }
    let short = branch
        .commit_id
        .as_deref()
        .map_or("", |id| &id[..id.len().min(7)]);
    spans.push(Span::styled(
        format!("  {short}"),
        Style::default().fg(Color::Yellow),
    ));
    spans.push(Span::styled(
        format!(" {}", branch.summary),
        Style::default().fg(MUTED),
    ));
    if let Some(time) = branch.committed_at {
        spans.push(Span::styled(
            format!(" · {}", ago(time)),
            Style::default().fg(MUTED),
        ));
    }
    Line::from(spans).style(row_style(panel.selected == branch_index))
}

/// The diff as rows: files and hunks that fold, line numbers before and after the change, and
/// the removed lines next to the added ones when side by side.
fn render_diff(frame: &mut Frame, controller: &mut GitController, panel: &GitPanel, area: Rect) {
    let diff = &panel.diff;
    if diff.files.is_empty() {
        let message = if diff.path.is_some() || diff.commit_id.is_some() {
            "No differences."
        } else {
            "Choose a change in Changes or Launchpad and press Enter to see its diff."
        };
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(MUTED)),
            area,
        );
        return;
    }
    let [title, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    let (added, removed) = diff.counts();
    let position = panel
        .diff_change()
        .map(|index| format!("   change {} of {}", index + 1, panel.changes().count()))
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                diff.title.clone(),
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("   +{added}"), Style::default().fg(Color::Green)),
            Span::styled(format!(" −{removed}"), Style::default().fg(Color::Red)),
            Span::styled(
                format!(
                    "   {} file{}{position}   n/p hunks  Enter folds",
                    diff.files.len(),
                    if diff.files.len() == 1 { "" } else { "s" }
                ),
                Style::default().fg(MUTED),
            ),
        ])),
        title,
    );
    let rows = panel.diff_rows();
    let digits = diff
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .flat_map(|hunk| &hunk.lines)
        .filter_map(|line| line.old.max(line.new))
        .max()
        .unwrap_or(0)
        .to_string()
        .len()
        .max(3);
    let offset = scroll_to(panel.selected, body.height.into());
    let half = usize::from(body.width.saturating_sub(1)) / 2;
    let lines = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(body.height.into())
        .map(|(index, row)| {
            controller.regions.actions.push((
                GitUiAction::DiffRow(index),
                Rect::new(body.x, body.y + (index - offset) as u16, body.width, 1),
            ));
            diff_line(panel, *row, digits, half).style(row_style(index == panel.selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), body);
}

fn diff_line(panel: &GitPanel, row: DiffRow, digits: usize, half: usize) -> Line<'static> {
    let fold = |key| {
        if panel.diff_folds.contains(&key) {
            "▸ "
        } else {
            "▾ "
        }
    };
    let file = &panel.diff.files[row.file()];
    match row {
        DiffRow::File(index) => {
            let (mark, color) = status_mark(file.status, ChangeArea::Unstaged);
            let path = match &file.old_path {
                Some(old) => format!("{} → {}", old.display(), file.path.display()),
                None => file.path.display().to_string(),
            };
            let (added, removed) = file.counts();
            Line::from(vec![
                Span::styled(fold((index, None)), Style::default().fg(ACTIVE)),
                Span::styled(
                    mark,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {path}"),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  +{added}"), Style::default().fg(Color::Green)),
                Span::styled(format!(" −{removed}"), Style::default().fg(Color::Red)),
                Span::styled(
                    if file.binary { "  binary file" } else { "" },
                    Style::default().fg(MUTED),
                ),
            ])
        }
        DiffRow::Hunk(index, hunk) => Line::from(vec![
            Span::raw("  "),
            Span::styled(fold((index, Some(hunk))), Style::default().fg(ACTIVE)),
            Span::styled(
                file.hunks[hunk].header.clone(),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        DiffRow::Line(_, hunk, line) => {
            let line = &file.hunks[hunk].lines[line];
            let mut spans = vec![Span::styled(
                format!(
                    "{} {} │",
                    number(line.old, digits),
                    number(line.new, digits)
                ),
                Style::default().fg(MUTED),
            )];
            spans.extend(diff_text(line));
            Line::from(spans)
        }
        DiffRow::Pair(_, hunk, left, right) => {
            let lines = &file.hunks[hunk].lines;
            let side = |line: Option<usize>, old: bool| {
                let Some(line) = line.map(|line| &lines[line]) else {
                    return vec![Span::raw(" ".repeat(half))];
                };
                let shown = if old { line.old } else { line.new };
                let mut spans = vec![Span::styled(
                    format!("{} │", number(shown, digits)),
                    Style::default().fg(MUTED),
                )];
                let mut text = diff_text(line);
                // Each half keeps its width: cut the text, fill the rest.
                let room = half.saturating_sub(digits + 2);
                let mut used = 0;
                for span in &mut text {
                    let content = span
                        .content
                        .chars()
                        .take(room.saturating_sub(used))
                        .collect::<String>();
                    used += Line::from(content.as_str()).width();
                    span.content = content.into();
                }
                spans.extend(text);
                spans.push(Span::raw(" ".repeat(room.saturating_sub(used))));
                spans
            };
            let mut spans = side(left, true);
            spans.push(Span::styled("┃", Style::default().fg(BORDER)));
            spans.extend(side(right, false));
            Line::from(spans)
        }
    }
}

fn number(number: Option<u32>, digits: usize) -> String {
    number.map_or_else(|| " ".repeat(digits), |number| format!("{number:>digits$}"))
}

fn diff_text(line: &DiffLine) -> Vec<Span<'static>> {
    let (sign, style) = match line.kind {
        DiffLineKind::Added => ("+", Style::default().fg(Color::Green)),
        DiffLineKind::Removed => ("-", Style::default().fg(Color::Red)),
        DiffLineKind::Context => (" ", Style::default().fg(TEXT)),
        DiffLineKind::Note => (
            "\\",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        ),
    };
    vec![Span::styled(format!("{sign} {}", line.text), style)]
}

/// The branch as cards: where it is, where it goes, how far apart they are and what is waiting
/// to be committed; every number leads to what resolves it.
fn render_launchpad(
    frame: &mut Frame,
    controller: &mut GitController,
    panel: &GitPanel,
    area: Rect,
) {
    let summary = &panel.launchpad;
    let [top, middle, bottom] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(4),
        Constraint::Min(4),
    ])
    .areas(area);
    let [branch_card, upstream_card] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);

    let inner = card(frame, branch_card, " Current branch ", Color::Green);
    let branch = match &summary.current_branch {
        Some(name) => Span::styled(
            format!("● {name}"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled("Detached HEAD", Style::default().fg(Color::Yellow)),
    };
    let last = summary.last_commit.as_ref().map_or_else(
        || Line::from(Span::styled("No commits yet", Style::default().fg(MUTED))),
        |commit| {
            Line::from(vec![
                Span::styled(commit.short_id.clone(), Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {}", commit.summary)),
                Span::styled(
                    format!(" · {}", ago(commit.committed_at)),
                    Style::default().fg(MUTED),
                ),
            ])
        },
    );
    frame.render_widget(Paragraph::new(vec![Line::from(branch), last]), inner);
    let third = row_of(inner, 2);
    match &summary.operation {
        Some(operation) => frame.render_widget(
            Paragraph::new(Span::styled(
                format!("{operation}: finish or abort it in a terminal"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            third,
        ),
        None => buttons(
            frame,
            controller,
            third,
            &[("Switch branch", GitUiAction::Tab(GitTab::Branches), false)],
        ),
    }

    let inner = card(frame, upstream_card, " Upstream ", Color::Blue);
    let upstream = match &summary.upstream {
        Some(upstream) => Line::from(Span::styled(
            format!("⇄ {upstream}"),
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        )),
        None => Line::from(Span::styled(
            "No upstream: Push publishes this branch",
            Style::default().fg(MUTED),
        )),
    };
    let remote = summary.remotes.first().map_or_else(
        || "No remotes".to_owned(),
        |remote| {
            format!(
                "{}  {}",
                remote.name,
                remote.url.as_deref().unwrap_or("no URL")
            )
        },
    );
    frame.render_widget(
        Paragraph::new(vec![
            upstream,
            Line::from(Span::styled(remote, Style::default().fg(MUTED))),
        ]),
        inner,
    );
    buttons(
        frame,
        controller,
        row_of(inner, 2),
        &[
            ("Fetch", GitUiAction::Fetch, false),
            ("Pull", GitUiAction::Pull, false),
            ("Push", GitUiAction::Push, false),
        ],
    );

    let inner = card(frame, middle, " Ahead / Behind ", Color::Yellow);
    let scale = summary.ahead.max(summary.behind).max(1);
    let bar =
        |count: usize| "█".repeat((count * 12).div_ceil(scale).min(12) * usize::from(count > 0));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("↑ {} ahead ", summary.ahead),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<13}", bar(summary.ahead)),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!("↓ {} behind ", summary.behind),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(bar(summary.behind), Style::default().fg(Color::Yellow)),
        ])),
        inner,
    );
    let plural = |count: usize| if count == 1 { "" } else { "s" };
    let advice = match (&summary.upstream, summary.ahead, summary.behind) {
        (None, _, _) => "Push to create the upstream branch".to_owned(),
        (Some(upstream), 0, 0) => format!("Up to date with {upstream}"),
        (Some(_), ahead, 0) => format!("Push {ahead} commit{}", plural(ahead)),
        (Some(_), 0, behind) => format!("Pull {behind} commit{}", plural(behind)),
        (Some(_), ahead, behind) => format!(
            "Diverged: pull {behind} commit{} (rebase if it cannot fast-forward), then push {ahead}",
            plural(behind)
        ),
    };
    let advice_row = row_of(inner, 1);
    frame.render_widget(
        Paragraph::new(Span::styled(advice.clone(), Style::default().fg(MUTED))),
        advice_row,
    );
    let start = (Line::from(advice.as_str()).width() as u16 + 2).min(advice_row.width);
    buttons(
        frame,
        controller,
        Rect::new(
            advice_row.x + start,
            advice_row.y,
            advice_row.width - start,
            1,
        ),
        &[
            ("Push", GitUiAction::Push, summary.ahead > 0),
            ("Pull", GitUiAction::Pull, summary.behind > 0),
        ],
    );

    let columns = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(bottom);
    let mut first = 0;
    for (column, (title, entries, color, actions)) in [
        (
            "Staged",
            &panel.status.staged,
            Color::Green,
            &[
                ("Commit [c]", GitUiAction::OpenCommit),
                ("Unstage [u]", GitUiAction::Unstage),
            ][..],
        ),
        (
            "Unstaged",
            &panel.status.unstaged,
            Color::Yellow,
            &[
                ("Stage [s]", GitUiAction::Stage),
                ("Stage all", GitUiAction::StageAll),
            ][..],
        ),
        (
            "Untracked",
            &panel.status.untracked,
            Color::Magenta,
            &[
                ("Stage [s]", GitUiAction::Stage),
                ("Stage all", GitUiAction::StageAll),
            ][..],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let inner = card(
            frame,
            columns[column],
            &format!(" {title} {} ", entries.len()),
            color,
        );
        let rows = usize::from(inner.height.saturating_sub(1));
        let local = panel
            .selected
            .checked_sub(first)
            .filter(|local| *local < entries.len());
        let offset = local.map_or(0, |local| scroll_to(local, rows));
        let mut lines = Vec::new();
        for (index, change) in entries.iter().enumerate().skip(offset).take(rows) {
            let row = (index - offset) as u16;
            controller.regions.actions.push((
                GitUiAction::OpenChange(first + index),
                Rect::new(inner.x, inner.y + row, inner.width, 1),
            ));
            lines.push(change_line(change, panel.selected == first + index));
        }
        if entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "Nothing here",
                Style::default().fg(MUTED),
            )));
        }
        frame.render_widget(Paragraph::new(lines), inner);
        let actions = actions
            .iter()
            .map(|(label, action)| (*label, action.clone(), false))
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            buttons(
                frame,
                controller,
                row_of(inner, inner.height.saturating_sub(1)),
                &actions,
            );
        }
        first += entries.len();
    }
}

/// A rounded card titled in `color`; returns the area inside it.
fn card(frame: &mut Frame, area: Rect, title: &str, color: Color) -> Rect {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Span::styled(
            title.to_owned(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// Row `row` of `area`, empty past its end.
fn row_of(area: Rect, row: u16) -> Rect {
    if row < area.height {
        Rect::new(area.x, area.y + row, area.width, 1)
    } else {
        Rect::default()
    }
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
        Style::default().fg(TEXT).add_modifier(Modifier::REVERSED)
    };
    frame.render_widget(Paragraph::new(format!(" {label} ")).style(style), area);
    controller.regions.actions.push((action, area));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_how_long_ago() {
        let now = DateTime::parse_from_rfc3339("2026-10-28T12:00:00+00:00").unwrap();
        let before = |seconds: i64| now - chrono::Duration::seconds(seconds);
        assert_eq!(ago_from(now, before(30)), "just now");
        assert_eq!(ago_from(now, before(60)), "1 minute ago");
        assert_eq!(ago_from(now, before(7_200)), "2 hours ago");
        assert_eq!(ago_from(now, before(3 * 86_400)), "3 days ago");
        assert_eq!(ago_from(now, before(400 * 86_400)), "1 year ago");
        assert_eq!(scroll_to(3, 10), 0);
        assert_eq!(scroll_to(12, 10), 3);
    }
}
