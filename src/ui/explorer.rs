//! The Explorer (F01, TODO 7.3.4): the project folder and its buttons on the title, folders
//! that open with `▸`/`▾` and guides down their rows, the file of the active tab marked, how
//! many files changed under each folder, and what to do when it is empty.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, ExplorerAction, Focus, Regions};

use super::{ACTIVE, MUTED, TEXT, editor::render_sideways_bar, panel_block, skip_columns};

/// The buttons on the title, right to left of the border's end.
const ACTIONS: [(ExplorerAction, &str); 4] = [
    (ExplorerAction::NewFile, "+f"),
    (ExplorerAction::NewFolder, "+d"),
    (ExplorerAction::Refresh, "⟳"),
    (ExplorerAction::CollapseAll, "⊟"),
];

pub(super) fn render_explorer(frame: &mut Frame, app: &App, regions: &mut Regions) {
    let area = regions.explorer;
    let name = app.root.file_name().map_or_else(
        || app.root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    // The filter typed after `/`, and how many entries are marked, on the title.
    let filter = app
        .explorer_filter
        .as_ref()
        .map(|text| format!("/{text}▏ "))
        .unwrap_or_default();
    let marked = match app.marked.len() {
        0 => String::new(),
        count => format!("· {count} marked "),
    };
    let title = format!(" {} {name} {filter}{marked}", app.icon("custom-folder"));
    let height = area.height.saturating_sub(2) as usize;
    let room = usize::from(area.width.saturating_sub(2));
    let widest = app.explorer_width_needed();
    let scroll_x = app.explorer_scroll_x.min(widest.saturating_sub(room));
    let changed = changed_files(app);
    let active = app
        .active_tab
        .and_then(|index| app.open_files.get(index))
        .map(|file| file.path.clone());
    let selected = app
        .entries
        .get(app.selected_entry)
        .map(|entry| entry.path.clone());
    let shown = app.shown_entries();
    let rows: Vec<_> = shown
        .iter()
        .skip(app.explorer_scroll)
        .take(height)
        .collect();
    let marks = rows
        .iter()
        .map(|entry| {
            let (status, change) = app.git_change(&entry.path)?;
            let (letter, colour) = crate::git::view::status_mark(status, change);
            let text = if entry.is_dir {
                let inside = entry.path.strip_prefix(&app.root).ok()?;
                changed.get(inside).map_or("*".into(), ToString::to_string)
            } else {
                letter.to_owned()
            };
            Some((text, colour))
        })
        .collect::<Vec<_>>();
    // Each level of a row: a guide `│` where a folder above goes on.
    let lines = rows
        .iter()
        .zip(&marks)
        .map(|(entry, mark)| {
            let guides = "│ ".repeat(entry.depth);
            let (icon, colour) = if entry.is_dir {
                let icon = if entry.expanded {
                    "custom-folder_open"
                } else {
                    "custom-folder"
                };
                (app.icon(icon), ACTIVE)
            } else {
                entry.icon
            };
            let arrow = match (entry.is_dir, entry.expanded) {
                (true, true) => "▾",
                (true, false) => "▸",
                _ => " ",
            };
            let spans =
                if selected.as_ref() == Some(&entry.path) || app.marked.contains(&entry.path) {
                    let style = Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED);
                    vec![Span::styled(
                        format!("{guides}{arrow} {icon} {}", entry.name),
                        style,
                    )]
                } else {
                    let mut name =
                        Style::default().fg(mark.as_ref().map_or(TEXT, |(_, colour)| *colour));
                    let ignored = app.dim_ignored
                        && app.gitignore.as_ref().is_some_and(|ignore| {
                            ignore
                                .matched_path_or_any_parents(&entry.path, entry.is_dir)
                                .is_ignore()
                        });
                    if ignored || entry.more > 0 {
                        name = name.fg(MUTED).add_modifier(Modifier::DIM);
                    }
                    if active.as_ref() == Some(&entry.path) {
                        name = name.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
                    }
                    vec![
                        Span::styled(guides, Style::default().fg(MUTED)),
                        Span::styled(format!("{arrow} "), Style::default().fg(MUTED)),
                        Span::styled(icon.to_string(), Style::default().fg(colour)),
                        Span::styled(format!(" {}", entry.name), name),
                    ]
                };
            Line::from(skip_columns(spans, scroll_x))
        })
        .collect::<Vec<_>>();
    let empty = app.entries.is_empty();
    drop(shown);
    let focused = app.focus == Focus::Explorer;
    let mut block = panel_block(title.clone(), focused);
    // The buttons, when the title leaves room for them.
    regions.explorer_actions.clear();
    let buttons: usize = ACTIONS
        .iter()
        .map(|(_, label)| label.chars().count() + 1)
        .sum();
    let title_width = Line::from(title.as_str()).width();
    if usize::from(area.width) >= title_width + buttons + 4 {
        let mut x = area.right() - 1 - buttons as u16;
        let mut spans = Vec::new();
        for (action, label) in ACTIONS {
            let width = label.chars().count() as u16;
            regions
                .explorer_actions
                .push((action, Rect::new(x, area.y, width, 1)));
            spans.push(Span::styled(
                label,
                Style::default().fg(if focused { ACTIVE } else { MUTED }),
            ));
            spans.push(Span::raw(" "));
            x += width + 1;
        }
        block = block.title_top(Line::from(spans).right_aligned());
    }
    let body = if empty {
        vec![
            Line::from(Span::styled(
                " This folder is empty.",
                Style::default().fg(MUTED),
            )),
            Line::from(vec![
                Span::styled(" N", Style::default().fg(ACTIVE)),
                Span::styled(" new file", Style::default().fg(MUTED)),
            ]),
            Line::from(vec![
                Span::styled(" Shift+N", Style::default().fg(ACTIVE)),
                Span::styled(" new folder", Style::default().fg(MUTED)),
            ]),
        ]
    } else {
        lines
    };
    frame.render_widget(Paragraph::new(body).block(block), area);
    // The marks stay at the right edge, over the end of a name too long for the panel.
    if area.width >= 6 {
        for (row, (text, colour)) in marks
            .into_iter()
            .enumerate()
            .filter_map(|(row, mark)| Some((row, mark?)))
        {
            let width = text.chars().count().min(4) as u16 + 1;
            let cell = Rect::new(
                area.right().saturating_sub(1 + width),
                area.y + 1 + row as u16,
                width,
                1,
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {text}"),
                    Style::default().fg(colour),
                )),
                cell,
            );
        }
    }
    if widest > room && area.height > 2 {
        let track = Rect::new(area.x + 1, area.bottom() - 1, room as u16, 1);
        render_sideways_bar(frame, track, widest, scroll_x, focused);
    }
}

/// How many changed files are under each folder, by the folder's path inside the project.
fn changed_files(app: &App) -> HashMap<&Path, usize> {
    let changes = &app.git_changes;
    let folders: HashSet<&Path> = changes
        .keys()
        .flat_map(|path| path.ancestors().skip(1))
        .collect();
    let mut counts = HashMap::new();
    for file in changes
        .keys()
        .filter(|path| !folders.contains(path.as_path()))
    {
        for folder in file.ancestors().skip(1) {
            if folder.as_os_str().is_empty() {
                break;
            }
            *counts.entry(folder).or_insert(0) += 1;
        }
    }
    counts
}
