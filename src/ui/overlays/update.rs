//! The update notice, its download and its errors (F16).

use super::*;

pub(in crate::ui) fn render_update(frame: &mut Frame, app: &mut App, area: Rect, progress: f32) {
    let width = area.width.saturating_sub(4).min(64);
    // The notes of the version offered, under the buttons (TODO 6.1.2).
    let notes = if matches!(app.updater.state, UpdateState::Available) {
        app.updater.notes.len().min(12) as u16
    } else {
        0
    };
    let height = area.height.min(9 + if notes > 0 { notes + 1 } else { 0 });
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
            if !app.updater.notes.is_empty() && inner.height > 8 {
                let lines: Vec<Line> = std::iter::once(Line::from(Span::styled(
                    "What's new:",
                    Style::default().fg(ACTIVE),
                )))
                .chain(
                    app.updater
                        .notes
                        .iter()
                        .take(12)
                        .map(|note| Line::from(format!("- {note}"))),
                )
                .collect();
                frame.render_widget(
                    Paragraph::new(lines),
                    Rect::new(inner.x, inner.y + 7, inner.width, inner.height - 7),
                );
            }
        }
    }
}
