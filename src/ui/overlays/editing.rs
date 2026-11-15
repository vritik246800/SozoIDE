//! The dialogs of the editing of TODO 2.2–2.3: open a file by its name and the command palette
//! (one dialog, two modes), Go to line, the question of a replace in the workspace, and a file
//! with edits not saved that changed on disk.

use super::*;
use crate::app::{FINDER_ROWS, FinderMode};

/// The file finder or the palette, high in the middle (F41).
pub(in crate::ui) fn render_finder(frame: &mut Frame, app: &App, area: Rect) {
    let Some(finder) = &app.finder else {
        return;
    };
    use crate::app::FinderRow;
    let rows: Vec<(String, String)> = app
        .finder_rows()
        .into_iter()
        .map(|row| match row {
            FinderRow::File(path) => (path.display().to_string(), String::new()),
            FinderRow::Command(_, name, key) => (name, key),
            FinderRow::Symbol(symbol) => {
                let file = symbol.path.strip_prefix(&app.root).unwrap_or(&symbol.path);
                (
                    symbol.name,
                    format!("{} {}:{}", symbol.kind, file.display(), symbol.row + 1),
                )
            }
            FinderRow::Problem(path, problem) => {
                let file = path.strip_prefix(&app.root).unwrap_or(&path);
                let mark = if problem.error { "✖" } else { "⚠" };
                (
                    format!("{mark} {}", problem.message),
                    format!("{}:{}", file.display(), problem.row + 1),
                )
            }
            FinderRow::Task(name, command) => (name, command.join(" ")),
        })
        .collect();
    let (title, key) = match finder.mode {
        FinderMode::Files => (
            "Open file, @ symbols, # in the project",
            app.keys.label(Action::QuickOpen),
        ),
        FinderMode::Commands => ("Commands", app.keys.label(Action::CommandPalette)),
        FinderMode::Problems => ("Problems", app.keys.label(Action::Problems)),
        FinderMode::Tasks => ("Tasks", app.keys.label(Action::Tasks)),
    };
    let width = 72.min(area.width);
    let height = (FINDER_ROWS as u16 + 4).min(area.height);
    let place = Rect::new(area.x + (area.width - width) / 2, area.y + 2, width, height);
    render_dialog(
        frame,
        place,
        1.0,
        format!(" {title} [{key}] "),
        |frame, inner| {
            let [query, list] =
                Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
            let summary = if finder.indexing() {
                format!("{} reading the project…", spinner(app.motion.elapsed()))
            } else {
                format!("{} found", rows.len())
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled("> ", Style::default().fg(ACTIVE)),
                        Span::styled(finder.query.clone(), Style::default().fg(TEXT)),
                        Span::styled("█", Style::default().fg(ACTIVE)),
                    ]),
                    Line::from(Span::styled(summary, Style::default().fg(MUTED))),
                ]),
                query,
            );
            let first = finder.selected / FINDER_ROWS * FINDER_ROWS;
            let lines = rows
                .iter()
                .enumerate()
                .skip(first)
                .take(usize::from(list.height))
                .map(|(index, (name, key))| {
                    let room = usize::from(list.width).saturating_sub(key.chars().count() + 2);
                    let name = crate::ui::middle_ellipsis(name, room);
                    let line = Line::from(vec![
                        Span::raw(format!(" {name:<room$}")),
                        Span::styled(format!("{key} "), Style::default().fg(MUTED)),
                    ]);
                    if index == finder.selected {
                        line.style(Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED))
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(lines), list);
        },
    );
}

/// `Ctrl+L`: a small box at the top of the editor (TODO 2.2.2).
/// Two files side by side (TODO 5.2.6): removed lines red on the left, added green on the
/// right, the same on both; `↑`/`↓`/pages scroll, `Esc` closes.
pub(in crate::ui) fn render_comparison(frame: &mut Frame, app: &App, area: Rect) {
    let Some(comparison) = &app.comparison else {
        return;
    };
    let place = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    render_dialog(
        frame,
        place,
        1.0,
        comparison.title.clone(),
        |frame, inner| {
            let half = usize::from(inner.width.saturating_sub(1)) / 2;
            let cut = |text: &str| {
                let text: String = text.chars().take(half).collect();
                format!("{text:<half$}")
            };
            let lines: Vec<Line> = comparison
                .rows
                .iter()
                .skip(comparison.scroll)
                .take(usize::from(inner.height.saturating_sub(1)))
                .map(|(left, right)| {
                    let changed = left != right && !comparison.plain;
                    let style = |text: &Option<String>, colour: Color| match (text, changed) {
                        (None, _) => Style::default().fg(MUTED),
                        (Some(text), _) if text.starts_with("@@") => Style::default().fg(ACTIVE),
                        (Some(_), true) => Style::default().fg(colour),
                        (Some(_), false) => Style::default().fg(TEXT),
                    };
                    Line::from(vec![
                        Span::styled(cut(left.as_deref().unwrap_or("")), style(left, Color::Red)),
                        Span::styled("│", Style::default().fg(MUTED)),
                        Span::styled(
                            cut(right.as_deref().unwrap_or("")),
                            style(right, Color::Green),
                        ),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), inner);
            let hint = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
            frame.render_widget(
                Paragraph::new("↑/↓ PageUp/PageDown scroll   Esc close")
                    .style(Style::default().fg(MUTED)),
                hint,
            );
        },
    );
}

/// The Layouts dialog (TODO 5.2.3): the three layouts kept, by number and name.
pub(in crate::ui) fn render_layouts(frame: &mut Frame, app: &App, area: Rect) {
    let Some(menu) = &app.layout_menu else {
        return;
    };
    let width = 50.min(area.width);
    let place = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + 2,
        width,
        8.min(area.height),
    );
    render_dialog(frame, place, 1.0, " Layouts ".into(), |frame, inner| {
        let mut lines: Vec<Line> = menu
            .layouts
            .iter()
            .enumerate()
            .map(|(index, layout)| {
                let name = match (&menu.naming, layout) {
                    (Some(typed), _) if index == menu.selected => format!("{typed}█"),
                    (_, Some(layout)) => layout.name.clone(),
                    (_, None) => "(empty)".into(),
                };
                let style = if index == menu.selected {
                    Style::default()
                        .fg(ACTIVE)
                        .add_modifier(ratatui::style::Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!(" {}  {name} ", index + 1), style))
            })
            .collect();
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Enter or 1-3 use  S keep this one  Delete forget  Esc",
            Style::default().fg(MUTED),
        )));
        frame.render_widget(Paragraph::new(lines), inner);
    });
}

pub(in crate::ui) fn render_go_to_line(frame: &mut Frame, app: &App, area: Rect) {
    let Some(text) = &app.go_to_line else {
        return;
    };
    let lines = app
        .active_tab
        .and_then(|index| app.open_files.get(index))
        .map_or(0, |file| file.lines.len());
    let width = 40.min(area.width);
    let place = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + 2,
        width,
        4.min(area.height),
    );
    render_dialog(frame, place, 1.0, " Go to line ".into(), |frame, inner| {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("> ", Style::default().fg(ACTIVE)),
                    Span::raw(text.clone()),
                    Span::styled("█", Style::default().fg(ACTIVE)),
                ]),
                Line::from(Span::styled(
                    format!("line or line:column, 1 to {lines}"),
                    Style::default().fg(MUTED),
                )),
            ]),
            inner,
        );
    });
}

/// The question before a replace in the workspace writes anything (TODO 2.1.3).
pub(in crate::ui) fn render_replace_plan(frame: &mut Frame, app: &App, area: Rect) {
    let Some(plan) = &app.replace_plan else {
        return;
    };
    let total: usize = plan.files.iter().map(|file| file.count).sum();
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "Replace {total} match{} of \"{}\" with \"{}\" in {} file{}?",
                if total == 1 { "" } else { "es" },
                plan.query,
                plan.with,
                plan.files.len(),
                if plan.files.len() == 1 { "" } else { "s" }
            ),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    for file in plan.files.iter().take(12) {
        let name = file.path.strip_prefix(&app.root).unwrap_or(&file.path);
        lines.push(Line::from(vec![
            Span::styled(format!("{:>5} ", file.count), Style::default().fg(ACTIVE)),
            Span::raw(name.display().to_string()),
        ]));
    }
    if plan.files.len() > 12 {
        lines.push(Line::from(Span::styled(
            format!("      and {} more files", plan.files.len() - 12),
            Style::default().fg(MUTED),
        )));
    }
    lines.extend([
        Line::default(),
        Line::from(Span::styled(
            "Files changed since the list was made are left as they are.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "Enter: replace  Esc: cancel",
            Style::default().fg(MUTED),
        )),
    ]);
    let height = (lines.len() as u16 + 2).min(area.height);
    let width = 76.min(area.width);
    let place = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    render_dialog(frame, place, 1.0, " Replace ".into(), |frame, inner| {
        frame.render_widget(Paragraph::new(lines), inner);
    });
}

/// A file with edits not saved changed on disk: `K`, `R` or `C` (TODO 2.2.6).
pub(in crate::ui) fn render_disk_prompt(frame: &mut Frame, app: &App, area: Rect) {
    let Some(path) = &app.disk_prompt else {
        return;
    };
    let name = path
        .strip_prefix(&app.root)
        .unwrap_or(path)
        .display()
        .to_string();
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{name} changed on disk, and you have edits not saved."),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(vec![
            Span::styled("K", Style::default().fg(ACTIVE)),
            Span::raw(" keep mine (they are saved over it)   "),
            Span::styled("R", Style::default().fg(ACTIVE)),
            Span::raw(" reload (Ctrl+Z brings mine back)   "),
            Span::styled("C", Style::default().fg(ACTIVE)),
            Span::raw(" compare"),
        ]),
    ];
    if let Some(compare) = &app.disk_compare {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "- on disk   + yours",
            Style::default().fg(MUTED),
        )));
        for line in compare
            .iter()
            .take(usize::from(area.height.saturating_sub(12)))
        {
            let colour = match line.chars().next() {
                Some('-') => Color::Red,
                Some('+') => Color::Green,
                Some('@') => ACTIVE,
                _ => TEXT,
            };
            lines.push(Line::from(Span::styled(
                line.clone(),
                Style::default().fg(colour),
            )));
        }
    }
    let height = (lines.len() as u16 + 2).min(area.height);
    let width = 90.min(area.width);
    let place = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    render_dialog(
        frame,
        place,
        1.0,
        " Changed on disk ".into(),
        |frame, inner| {
            frame.render_widget(Paragraph::new(lines), inner);
        },
    );
}
