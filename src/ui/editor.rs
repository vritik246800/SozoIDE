use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{Resize, StatefulImage};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::{
    app::{App, Focus, Regions},
    branding::ASCII_LOGO,
    editor::language_name,
};

use super::{ACTIVE, BORDER, MUTED, TEXT, panel_block};

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
            let icon = if entry.is_dir {
                app.icon(if entry.expanded {
                    "custom-folder_open"
                } else {
                    "custom-folder"
                })
            } else {
                app.icon("custom-file")
            };
            let marker = if entry.is_dir {
                if entry.expanded { "-" } else { "+" }
            } else {
                " "
            };
            let style = if index == app.selected_entry {
                Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(TEXT)
            };
            Line::from(Span::styled(
                format!("{indentation}{marker} {icon} {}", entry.name),
                style,
            ))
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
        let text = format!(" {} {}{}  x ", app.icon("custom-file"), file.name, dirty);
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

pub(super) fn render_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(active) = app.active_tab else {
        let mut lines = vec![Line::from("")];
        lines.extend(ASCII_LOGO.iter().map(|line| {
            Line::from(Span::styled(
                *line,
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            ))
        }));
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                "Select a file in Explorer",
                Style::default().fg(MUTED),
            )),
            Line::from(Span::styled(
                "Ctrl+P: search file  |  Ctrl+Shift+P: search workspace",
                Style::default().fg(MUTED),
            )),
        ]);
        frame.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .block(panel_block(
                    " Editor ".to_string(),
                    app.focus == Focus::Editor,
                )),
            area,
        );
        return;
    };
    let focused = app.focus == Focus::Editor;
    let file = &mut app.open_files[active];
    let title = format!(" {} — {} ", file.path.display(), language_name(&file.path));
    if let Some(image) = &mut file.image {
        let block = panel_block(title, focused);
        let inner = block.inner(area);
        frame.render_stateful_widget(StatefulImage::new().resize(Resize::Fit(None)), inner, image);
        frame.render_widget(block, area);
        return;
    }
    let viewport_height = area.height.saturating_sub(2) as usize;
    if file.cursor_row < file.scroll as usize {
        file.scroll = file.cursor_row as u16;
    } else if viewport_height > 0 && file.cursor_row >= file.scroll as usize + viewport_height {
        file.scroll = (file.cursor_row + 1 - viewport_height) as u16;
    }
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
        let y = area
            .y
            .saturating_add(1 + (file.cursor_row - file.scroll as usize) as u16);
        if x < area.right().saturating_sub(1) && y < area.bottom().saturating_sub(1) {
            frame.set_cursor_position((x, y));
        }
    }
}

pub(super) fn render_terminal(frame: &mut Frame, app: &App, area: Rect, bottom: bool) {
    let (terminal, focused, title) = if bottom {
        (
            &app.bottom_terminal,
            app.focus == Focus::BottomTerminal,
            " Bottom terminal [Ctrl+J] ",
        )
    } else {
        (
            &app.right_terminal,
            app.focus == Focus::RightTerminal,
            if cfg!(target_os = "macos") {
                " Right terminal [Ctrl+Option+B] "
            } else {
                " Right terminal [Ctrl+Shift+B] "
            },
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
