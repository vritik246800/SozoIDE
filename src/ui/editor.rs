use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{Resize, StatefulImage};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::{
    app::{App, DropZone, Focus, Regions, SplitDirection},
    branding::ASCII_LOGO,
    editor::{OpenFile, language_name},
};

use super::{ACTIVE, BORDER, MUTED, TEXT, key, panel_block};

pub(super) fn render_explorer(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" {} Explorer ", app.icon("custom-folder"));
    let height = area.height.saturating_sub(2) as usize;
    let lines = app
        .entries
        .iter()
        .enumerate()
        .skip(app.explorer_scroll)
        .take(height)
        .map(|(index, entry)| {
            let indentation = "  ".repeat(entry.depth);
            let (icon, color) = if entry.is_dir {
                let name = if entry.expanded {
                    "custom-folder_open"
                } else {
                    "custom-folder"
                };
                (app.icon(name), ACTIVE)
            } else {
                entry.icon
            };
            let marker = if entry.is_dir {
                if entry.expanded { "-" } else { "+" }
            } else {
                " "
            };
            if index == app.selected_entry {
                let style = Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED);
                return Line::from(Span::styled(
                    format!("{indentation}{marker} {icon} {}", entry.name),
                    style,
                ));
            }
            Line::from(vec![
                Span::styled(format!("{indentation}{marker} "), Style::default().fg(TEXT)),
                Span::styled(icon.to_string(), Style::default().fg(color)),
                Span::styled(format!(" {}", entry.name), Style::default().fg(TEXT)),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).block(panel_block(title, app.focus == Focus::Explorer)),
        area,
    );
}

pub(super) fn render_tabs(frame: &mut Frame, app: &App, regions: &mut Regions, area: Rect) {
    let mut spans = Vec::new();
    let mut x = area.x;
    for (index, file) in app.open_files.iter().enumerate() {
        let active = app.active_tab == Some(index);
        let dirty = if file.dirty { "*" } else { "" };
        let text = format!(" {} {}{}  x ", file.icon.0, file.name, dirty);
        let width = Line::from(text.clone()).width() as u16;
        if x.saturating_add(width) > area.x.saturating_add(area.width) {
            break;
        }
        let style = if active {
            Style::default()
                .fg(ACTIVE)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(MUTED)
        };
        spans.push(Span::styled(text, style));
        let tab = Rect::new(x, area.y, width.saturating_sub(3), 1);
        let close = Rect::new(x + width.saturating_sub(3), area.y, 3, 1);
        regions.tab_items.push((index, tab, close));
        x = x.saturating_add(width);
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER)),
        area,
    );
}

/// The active file, and beside it the other pane of a split (F19). With no file, the logo and
/// the first keys sit in the middle of whatever space the panels leave.
pub(super) fn render_editor(frame: &mut Frame, app: &mut App, regions: &mut Regions, area: Rect) {
    let Some(active) = app.active_tab else {
        render_empty(frame, app.focus == Focus::Editor, area);
        return;
    };
    let focused = app.focus == Focus::Editor;
    let split = app
        .split
        .filter(|split| split.other != active && split.other < app.open_files.len());
    if let Some(split) = split {
        let halves = [Constraint::Percentage(50), Constraint::Percentage(50)];
        let [first, second] = match split.direction {
            SplitDirection::Columns => Layout::horizontal(halves).areas(area),
            SplitDirection::Rows => Layout::vertical(halves).areas(area),
        };
        let (mine, other) = if split.active_first {
            (first, second)
        } else {
            (second, first)
        };
        regions.editor = mine;
        regions.split_editor = other;
        render_file(frame, &mut app.open_files[split.other], other, false);
        render_file(frame, &mut app.open_files[active], mine, focused);
    } else {
        render_file(frame, &mut app.open_files[active], area, focused);
    }
    if let Some(zone) = app.tab_drop_zone() {
        render_drop_preview(frame, area, zone);
    }
}

fn render_empty(frame: &mut Frame, focused: bool, area: Rect) {
    let block = panel_block(" Editor ".to_string(), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let hints = [
        "Select a file in Explorer".to_string(),
        format!(
            "{}: search file  |  {}: search workspace",
            key("Ctrl+P"),
            key("Ctrl+Shift+P")
        ),
        format!("{}: all commands", key("F1")),
    ];
    let logo_width = ASCII_LOGO.iter().map(|line| line.len()).max().unwrap_or(0) as u16;
    let logo_fits =
        inner.width >= logo_width && usize::from(inner.height) > ASCII_LOGO.len() + hints.len();
    let mut lines = Vec::new();
    if logo_fits {
        lines.extend(ASCII_LOGO.iter().map(|line| {
            Line::from(Span::styled(
                *line,
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            ))
        }));
        lines.push(Line::from(""));
    }
    lines.extend(
        hints
            .into_iter()
            .map(|hint| Line::from(Span::styled(hint, Style::default().fg(MUTED)))),
    );
    let height = (lines.len() as u16).min(inner.height);
    let top = inner.y + (inner.height - height) / 2;
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        Rect::new(inner.x, top, inner.width, height),
    );
}

/// The half of the editor a dragged tab would take, or all of it for the centre.
fn render_drop_preview(frame: &mut Frame, area: Rect, zone: DropZone) {
    let halves = [Constraint::Percentage(50), Constraint::Percentage(50)];
    let [left, right] = Layout::horizontal(halves).areas(area);
    let [top, bottom] = Layout::vertical(halves).areas(area);
    let (target, title) = match zone {
        DropZone::Left => (left, " Split left "),
        DropZone::Right => (right, " Split right "),
        DropZone::Top => (top, " Split up "),
        DropZone::Bottom => (bottom, " Split down "),
        DropZone::Center => (area, " Single editor "),
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Double)
            .border_style(Style::default().fg(ACTIVE))
            .title(Span::styled(
                title,
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            )),
        target,
    );
}

fn render_file(frame: &mut Frame, file: &mut OpenFile, area: Rect, focused: bool) {
    let title = format!(" {} — {} ", file.path.display(), language_name(&file.path));
    if let Some(image) = &mut file.image {
        let block = panel_block(title, focused);
        let inner = block.inner(area);
        frame.render_stateful_widget(StatefulImage::new().resize(Resize::Fit(None)), inner, image);
        frame.render_widget(block, area);
        return;
    }
    file.scroll_to_cursor(area.height.saturating_sub(2) as usize);
    let number_width = file.lines.len().max(1).to_string().len();
    let lines = file
        .lines
        .iter()
        .zip(&file.highlighted)
        .enumerate()
        .map(|(index, (line, highlighted))| {
            let mut spans = vec![Span::styled(
                format!("{:>number_width$} | ", index + 1),
                Style::default().fg(MUTED),
            )];
            if let Some(rendered) = file.rendered_line(index) {
                spans.extend(rendered);
            } else if highlighted.is_empty() {
                spans.push(Span::raw(line.clone()));
            } else {
                spans.extend(highlighted.clone());
            }
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((file.scroll, 0))
            .block(panel_block(title, focused)),
        area,
    );
    if focused {
        let prefix = file.lines[file.cursor_row]
            .chars()
            .take(file.cursor_column)
            .collect::<String>();
        let x = area
            .x
            .saturating_add(1 + number_width as u16 + 3 + Line::from(prefix).width() as u16);
        let row = file.cursor_row.checked_sub(file.scroll as usize);
        let y = area
            .y
            .saturating_add(1)
            .saturating_add(row.unwrap_or(usize::MAX).min(u16::MAX as usize) as u16);
        if row.is_some()
            && x < area.right().saturating_sub(1)
            && y < area.bottom().saturating_sub(1)
        {
            frame.set_cursor_position((x, y));
        }
    }
}

pub(super) fn render_terminal(frame: &mut Frame, app: &App, area: Rect, bottom: bool) {
    let (terminal, focused, title) = if bottom {
        (
            &app.bottom_terminal,
            app.focus == Focus::BottomTerminal,
            " Bottom terminal [Ctrl+J] ".to_string(),
        )
    } else {
        (
            &app.right_terminal,
            app.focus == Focus::RightTerminal,
            format!(" Right terminal [{}] ", key("Ctrl+Shift+B")),
        )
    };
    if let Ok(parser) = terminal.parser.read() {
        let scrollback = terminal.scrollback();
        let title = if scrollback > 0 {
            format!(
                " {} {title} [history: {scrollback}] ",
                app.icon("dev-terminal")
            )
        } else {
            format!(" {} {title}", app.icon("dev-terminal"))
        };
        let cursor = Cursor::default().visibility(focused && scrollback == 0);
        frame.render_widget(
            PseudoTerminal::new(parser.screen())
                .block(panel_block(title, focused))
                .cursor(cursor),
            area,
        );
    }
}
