//! The dialogs before something runs or installs: the command (F32), the choice of a project
//! and the Setup panel (F36).

use super::*;

/// A command waiting for Enter (F32, F33): what it is, then the command, over everything.
pub(in crate::ui) fn render_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let Some(confirm) = &app.confirm else {
        return;
    };
    let width = 120.min(area.width.saturating_sub(4));
    let label = confirm
        .rows
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0)
        + 2;
    let mut lines = confirm
        .rows
        .iter()
        .map(|(name, value)| {
            let colour = if *name == "Command" { ACTIVE } else { TEXT };
            Line::from(vec![
                Span::styled(format!("{name:<label$}"), Style::default().fg(MUTED)),
                Span::styled(value.clone(), Style::default().fg(colour)),
            ])
        })
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter: run it in the bottom terminal   Esc: cancel",
        Style::default().fg(MUTED),
    )));
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let height = paragraph.line_count(width.saturating_sub(4)) as u16 + 2;
    render_dialog(
        frame,
        dialog_area(area, width, height),
        1.0,
        format!(" {} ", confirm.title),
        |frame, inner| {
            let inner = Rect::new(
                inner.x + 1,
                inner.y,
                inner.width.saturating_sub(2),
                inner.height,
            );
            frame.render_widget(paragraph, inner);
        },
    );
}

/// The projects of a monorepo to choose from (F32).
pub(in crate::ui) fn render_choice(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(choice) = &app.project_choice else {
        return;
    };
    let lines = choice
        .projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            let folder = project
                .folder
                .strip_prefix(&app.root)
                .unwrap_or(&project.folder);
            let line = Line::from(vec![
                Span::styled(
                    format!(" {:<16}", project.kind),
                    Style::default().fg(ACTIVE),
                ),
                Span::styled(folder.display().to_string(), Style::default().fg(TEXT)),
            ]);
            if index == choice.selected {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect::<Vec<_>>();
    let count = lines.len() as u16;
    let dialog = dialog_area(area, 60, count + 4);
    let mut places = Vec::new();
    render_dialog(
        frame,
        dialog,
        1.0,
        format!(" Choose the project: {} ", choice.task.label()),
        |frame, inner| {
            let [list, hint] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            for row in 0..count.min(list.height) {
                places.push((row as usize, Rect::new(list.x, list.y + row, list.width, 1)));
            }
            frame.render_widget(Paragraph::new(lines), list);
            frame.render_widget(
                Paragraph::new(" Up/Down, Enter or a click: choose   Esc: cancel")
                    .style(Style::default().fg(MUTED)),
                hint,
            );
        },
    );
    app.regions.choice_rows = places;
}

/// The first start (F17): one page at a time, with Next and Skip always in reach.
/// The first thing a new installation sees, and a new version when it brings a tool (F36):
/// what makes SozoIDE better, to install now or skip.
pub(in crate::ui) fn render_setup(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(setup) = &app.setup else {
        return;
    };
    let later = format!(
        "Skipped something? Help ({}), then Dependencies, installs it any time.",
        app.keys.label(Action::Help)
    );
    let rows = setup.components.len() as u16;
    let area = dialog_area(area, 84, rows + 8);
    let progress = app.motion.overlay();
    let mut places = Vec::new();
    let (mut next_area, mut skip_area) = (Rect::default(), Rect::default());
    render_dialog(
        frame,
        area,
        progress,
        " Best experience ".to_owned(),
        |frame, inner| {
            let parts = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(rows),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new(
                    "SozoIDE runs without these. Each one you add turns on a part of it:",
                )
                .style(Style::default().fg(TEXT)),
                parts[0],
            );
            for (index, component) in setup.components.iter().enumerate() {
                let found = setup.found.as_ref().map(|found| &found[index]);
                let (mark, state, colour) = match found {
                    None => ("   ", "Checking".to_owned(), MUTED),
                    Some(Found::Installed(_)) => ("   ", "Installed".to_owned(), Color::Green),
                    Some(Found::Missing) if setup.chosen[index] => {
                        ("[x]", "Missing".to_owned(), Color::Yellow)
                    }
                    Some(Found::Missing) if setup.can_install(index) => {
                        ("[ ]", "Missing".to_owned(), Color::Yellow)
                    }
                    Some(Found::Missing) => ("   ", "By hand".to_owned(), Color::Yellow),
                };
                let mut line = Line::from(vec![
                    Span::styled(format!("{mark} "), Style::default().fg(ACTIVE)),
                    Span::styled(format!("{:<8}", component.name), Style::default().fg(TEXT)),
                    Span::styled(format!("{state:<11}"), Style::default().fg(colour)),
                    Span::styled(purpose(component.program), Style::default().fg(MUTED)),
                ]);
                if index == setup.selected {
                    line = line.style(Style::default().add_modifier(Modifier::REVERSED));
                }
                let row = Rect::new(parts[1].x, parts[1].y + index as u16, parts[1].width, 1);
                frame.render_widget(Paragraph::new(line), row);
                places.push((index, row));
            }
            frame.render_widget(
                Paragraph::new(later.as_str())
                    .style(Style::default().fg(MUTED))
                    .wrap(Wrap { trim: true }),
                parts[2],
            );
            (next_area, skip_area) = buttons(frame, parts[3], " Install [Enter] ", " Skip [Esc] ");
        },
    );
    app.regions.dependency_rows = places;
    app.regions.dialog_next = next_area;
    app.regions.dialog_skip = skip_area;
}
