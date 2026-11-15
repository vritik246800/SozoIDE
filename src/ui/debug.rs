//! The debugger's panel (F27, TODO 3.3.3): its state and threads, the call stack (a click
//! chooses a frame), the variables as a tree (a click opens one), the watched expressions,
//! the breakpoints with their conditions, the console and the program's output.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};

use super::{ACTIVE, MUTED, TEXT};
use crate::app::{App, DebugTarget, Regions};
use crate::debug::{State, Variable};

pub(super) fn render_debug(frame: &mut Frame, app: &App, regions: &mut Regions, body: Rect) {
    regions.debug_rows.clear();
    let session = app.debug.as_ref();
    if session.is_none() && !app.debug_building() {
        return;
    }
    let width = (body.width / 2).clamp(24, 52);
    let area = Rect::new(
        body.right().saturating_sub(width),
        body.y,
        width,
        body.height,
    );
    let room = width.saturating_sub(4) as usize;
    let cut = |text: String| text.chars().take(room).collect::<String>();
    let heading = |text: &str| Line::styled(text.to_owned(), Style::default().fg(ACTIVE));
    let (state, colour) = match session.map(|session| &session.state) {
        None => ("Building...".to_owned(), MUTED),
        Some(State::Starting) => ("Starting...".to_owned(), MUTED),
        Some(State::Running) => ("Running  F8 pauses".to_owned(), Color::Green),
        Some(State::Paused(reason)) => (format!("Paused: {reason}"), Color::Yellow),
        Some(State::Ended(how)) => (how.clone(), MUTED),
    };
    let mut lines = vec![Line::styled(cut(state), Style::default().fg(colour))];
    let mut targets: Vec<(usize, DebugTarget)> = Vec::new();
    if let Some(session) = session {
        if session.threads.len() > 1 {
            lines.push(heading("Threads"));
            for (id, name) in session.threads.iter().take(6) {
                targets.push((lines.len(), DebugTarget::Thread(*id)));
                lines.push(Line::from(Span::styled(
                    cut(format!(" {name}")),
                    Style::default().fg(TEXT),
                )));
            }
        }
        if !session.frames.is_empty() {
            lines.push(heading("Call stack"));
            for (index, frame_info) in session.frames.iter().enumerate().take(8) {
                let place = frame_info.path.as_ref().map_or_else(String::new, |path| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    format!("  {name}:{}", frame_info.row + 1)
                });
                targets.push((lines.len(), DebugTarget::Frame(index)));
                let style = if index == session.frame {
                    Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT)
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        cut(format!(
                            "{}{}",
                            if index == session.frame { "▸" } else { " " },
                            frame_info.name
                        )),
                        style,
                    ),
                    Span::styled(place, Style::default().fg(MUTED)),
                ]));
            }
        }
        if !session.tree.is_empty() {
            lines.push(heading("Variables"));
            let most = usize::from(body.height / 2).max(4);
            let mut rows = Vec::new();
            tree_rows(&session.tree, session, 0, &mut rows);
            for (depth, variable) in rows.into_iter().take(most) {
                let arrow = match (
                    variable.reference > 0,
                    session.expanded.contains(&variable.reference),
                ) {
                    (false, _) => " ",
                    (true, true) => "▾",
                    (true, false) => "▸",
                };
                targets.push((lines.len(), DebugTarget::Variable(variable.reference)));
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}{arrow}{} = ", "  ".repeat(depth), variable.name),
                        Style::default().fg(TEXT),
                    ),
                    Span::styled(cut(variable.value.clone()), Style::default().fg(MUTED)),
                ]));
            }
        }
        if !session.watches.is_empty() {
            lines.push(heading("Watch  (click removes)"));
            for (index, (expression, value)) in session.watches.iter().enumerate() {
                targets.push((lines.len(), DebugTarget::Watch(index)));
                lines.push(Line::from(vec![
                    Span::styled(format!("{expression} = "), Style::default().fg(TEXT)),
                    Span::styled(cut(value.clone()), Style::default().fg(MUTED)),
                ]));
            }
        }
        if !session.console.is_empty() {
            lines.push(heading("Console"));
            for (expression, value) in session.console.iter().rev().take(4).rev() {
                lines.push(Line::from(vec![
                    Span::styled(format!("> {expression}  "), Style::default().fg(ACTIVE)),
                    Span::styled(cut(value.clone()), Style::default().fg(TEXT)),
                ]));
            }
        }
    }
    let marked: Vec<String> = app
        .breakpoint_options
        .iter()
        .map(|((path, row), options)| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let what = options
                .condition
                .as_ref()
                .map(|condition| format!("if {condition}"))
                .or_else(|| options.hits.as_ref().map(|hits| format!("hit {hits}")))
                .or_else(|| options.log.as_ref().map(|log| format!("log {log}")))
                .unwrap_or_default();
            format!("{name}:{} {what}", row + 1)
        })
        .collect();
    if !marked.is_empty() {
        lines.push(heading("Breakpoints"));
        lines.extend(
            marked
                .into_iter()
                .take(6)
                .map(|text| Line::styled(cut(text), Style::default().fg(TEXT))),
        );
    }
    if let Some(session) = session
        && !session.output.is_empty()
    {
        lines.push(heading("Output"));
        let left = usize::from(body.height.saturating_sub(2)).saturating_sub(lines.len());
        let skip = session.output.len().saturating_sub(left);
        lines.extend(
            session
                .output
                .iter()
                .skip(skip)
                .map(|line| Line::styled(cut(line.clone()), Style::default().fg(TEXT))),
        );
    }
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACTIVE))
                .title(" Debug ")
                .title_bottom(
                    Line::from(" F5 F10 F11 Shift+F11  Ctrl+Shift+E W  Shift+F5 ").right_aligned(),
                ),
        ),
        area,
    );
    for (line, target) in targets {
        let y = area.y + 1 + line as u16;
        if y + 1 < area.bottom() {
            regions.debug_rows.push((
                target,
                Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1),
            ));
        }
    }
}

/// The variables in order, with the fields of the open ones under them.
fn tree_rows<'a>(
    list: &'a [Variable],
    session: &'a crate::debug::Session,
    depth: usize,
    rows: &mut Vec<(usize, &'a Variable)>,
) {
    for variable in list {
        rows.push((depth, variable));
        if depth < 8
            && session.expanded.contains(&variable.reference)
            && let Some(children) = session.children.get(&variable.reference)
        {
            tree_rows(children, session, depth + 1, rows);
        }
    }
}
