//! The boxes of Code Intelligence of TODO 3.3.1: rename, code actions, the signature at the
//! cursor and the edits of a server waiting for Enter.

use super::*;

fn top_box(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + 2,
        width,
        height.min(area.height),
    )
}

pub(in crate::ui) fn render_rename(frame: &mut Frame, app: &App, area: Rect) {
    let Some(rename) = &app.rename else {
        return;
    };
    render_dialog(
        frame,
        top_box(area, 50, 4),
        1.0,
        " Rename the symbol ".into(),
        |frame, inner| {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled("> ", Style::default().fg(ACTIVE)),
                        Span::raw(rename.name.clone()),
                        Span::styled("█", Style::default().fg(ACTIVE)),
                    ]),
                    Line::from(Span::styled(
                        "Enter: rename everywhere  Esc: cancel",
                        Style::default().fg(MUTED),
                    )),
                ]),
                inner,
            );
        },
    );
}

pub(in crate::ui) fn render_actions(frame: &mut Frame, app: &App, area: Rect) {
    let Some(menu) = &app.action_menu else {
        return;
    };
    let height = menu.actions.len().min(12) as u16 + 2;
    render_dialog(
        frame,
        top_box(area, 70, height),
        1.0,
        " Code actions ".into(),
        |frame, inner| {
            let lines = menu
                .actions
                .iter()
                .enumerate()
                .take(usize::from(inner.height))
                .map(|(index, action)| {
                    let line = Line::from(format!(" {}", action.title));
                    if index == menu.selected {
                        line.style(Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED))
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(lines), inner);
        },
    );
}

/// The signature of the call at the cursor over the bottom of the editor, its active parameter
/// in bold.
pub(in crate::ui) fn render_signature(frame: &mut Frame, app: &App, editor: Rect) {
    let Some((_, label, active)) = &app.signature else {
        return;
    };
    let width = (label.chars().count() as u16 + 4).min(editor.width);
    let place =
        Rect::new(editor.x + 1, editor.bottom().saturating_sub(4), width, 3).intersection(editor);
    frame.render_widget(Clear, place);
    let chars: Vec<char> = label.chars().collect();
    let (start, end) = active.unwrap_or((0, 0));
    let part = |from: usize, to: usize| {
        chars[from.min(chars.len())..to.min(chars.len())]
            .iter()
            .collect::<String>()
    };
    let line = Line::from(vec![
        Span::raw(part(0, start)),
        Span::styled(
            part(start, end),
            Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
        ),
        Span::raw(part(end, chars.len())),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(panel_block(String::new(), true)),
        place,
    );
}

pub(in crate::ui) fn render_edit_preview(frame: &mut Frame, app: &App, area: Rect) {
    let Some(preview) = &app.edit_preview else {
        return;
    };
    let total: usize = preview.files.iter().map(|file| file.edits.len()).sum();
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{total} changes in {} files", preview.files.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    for file in preview.files.iter().take(14) {
        let name = file.path.strip_prefix(&app.root).unwrap_or(&file.path);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:>5} ", file.edits.len()),
                Style::default().fg(ACTIVE),
            ),
            Span::raw(name.display().to_string()),
        ]));
    }
    if preview.files.len() > 14 {
        lines.push(Line::from(Span::styled(
            format!("      and {} more", preview.files.len() - 14),
            Style::default().fg(MUTED),
        )));
    }
    lines.extend([
        Line::default(),
        Line::from(Span::styled(
            "Open files change as one Ctrl+Z each; the others are written.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "Enter: apply  Esc: cancel",
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
    render_dialog(
        frame,
        place,
        1.0,
        format!(" {} ", preview.title),
        |frame, inner| {
            frame.render_widget(Paragraph::new(lines), inner);
        },
    );
}

/// The debugger's box: an expression, or the condition, count and message of a breakpoint.
pub(in crate::ui) fn render_debug_input(frame: &mut Frame, app: &App, area: Rect) {
    use crate::app::DebugInputKind;
    let Some(input) = &app.debug_input else {
        return;
    };
    let (title, labels): (&str, &[&str]) = match &input.kind {
        DebugInputKind::Evaluate => (" Evaluate ", &["Expression"]),
        DebugInputKind::Watch => (" Watch ", &["Expression"]),
        DebugInputKind::Breakpoint(..) => {
            (" Breakpoint ", &["Stop if", "After hits", "Log instead"])
        }
    };
    let mut lines: Vec<Line> = labels
        .iter()
        .zip(&input.fields)
        .enumerate()
        .map(|(index, (label, text))| {
            Line::from(vec![
                Span::styled(
                    format!("{label:>11}: "),
                    Style::default().fg(if index == input.field { ACTIVE } else { MUTED }),
                ),
                Span::raw(text.clone()),
                Span::styled(
                    if index == input.field { "█" } else { "" },
                    Style::default().fg(ACTIVE),
                ),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        if labels.len() > 1 {
            "Tab: next field  Enter: save  Esc: cancel"
        } else {
            "Enter  Esc"
        },
        Style::default().fg(MUTED),
    )));
    let height = lines.len() as u16 + 2;
    render_dialog(
        frame,
        top_box(area, 60, height),
        1.0,
        title.into(),
        |frame, inner| {
            frame.render_widget(Paragraph::new(lines), inner);
        },
    );
}

/// The first launch of what `.sozo.toml` says: what runs, waiting for Enter (TODO 3.1.10).
pub(in crate::ui) fn render_debug_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let Some(confirm) = &app.debug_confirm else {
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            "Debug as .sozo.toml says?",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    for (label, value) in confirm.launch.rows() {
        lines.push(Line::from(vec![
            Span::styled(format!("{label:>10}: "), Style::default().fg(MUTED)),
            Span::raw(value),
        ]));
    }
    lines.extend([
        Line::default(),
        Line::from(Span::styled(
            "Enter: debug  Esc: cancel",
            Style::default().fg(MUTED),
        )),
    ]);
    let height = lines.len() as u16 + 2;
    render_dialog(
        frame,
        top_box(area, 72, height),
        1.0,
        " Debug ".into(),
        |frame, inner| {
            frame.render_widget(Paragraph::new(lines), inner);
        },
    );
}
