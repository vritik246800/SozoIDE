use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use ratatui_image::{Resize, StatefulImage};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::{
    app::{App, DropZone, Focus, Regions, SplitDirection},
    branding::ASCII_LOGO,
    editor::{OpenFile, language_name},
};

use super::{ACTIVE, BORDER, MUTED, TEXT, key, panel_block, skip_columns};

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

/// The tab bar of `pane`: it keeps the tab on screen in view when that changes and shows
/// `‹`/`›` when tabs are hidden on a side; the wheel or those arrows scroll it.
pub(super) fn render_tabs(
    frame: &mut Frame,
    app: &mut App,
    regions: &mut Regions,
    area: Rect,
    pane: usize,
) {
    let tabs = app.pane_tabs(pane);
    let shown = app.shown_tab(pane);
    let focused_pane = pane == app.active_pane();
    let labels = tabs
        .iter()
        .map(|index| {
            let file = &app.open_files[*index];
            let dirty = if file.dirty { "*" } else { "" };
            let text = format!(" {} {}{}  x ", file.icon.0, file.name, dirty);
            let width = Line::from(text.as_str()).width() as u16;
            (*index, text, width)
        })
        .collect::<Vec<_>>();
    let bar = &mut app.tab_bars[pane];
    if bar.shown != shown {
        bar.shown = shown;
        if let Some(position) = shown.and_then(|shown| tabs.iter().position(|tab| *tab == shown)) {
            // Room for both arrows, so the tab is still whole once they are drawn.
            let room = area.width.saturating_sub(4);
            if position < bar.scroll {
                bar.scroll = position;
            }
            while bar.scroll < position
                && labels[bar.scroll..=position]
                    .iter()
                    .map(|(_, _, width)| width)
                    .sum::<u16>()
                    > room
            {
                bar.scroll += 1;
            }
        }
    }
    bar.scroll = bar.scroll.min(tabs.len().saturating_sub(1));
    let scroll = bar.scroll;

    let arrow = Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut x = area.x;
    if scroll > 0 {
        spans.push(Span::styled("‹ ", arrow));
        regions
            .tab_arrows
            .push((pane, false, Rect::new(x, area.y, 2, 1)));
        x += 2;
    }
    let total = labels[scroll..]
        .iter()
        .map(|(_, _, width)| width)
        .sum::<u16>();
    let overflow = x.saturating_add(total) > area.right();
    let end = if overflow {
        area.right().saturating_sub(2)
    } else {
        area.right()
    };
    for (index, text, width) in labels.into_iter().skip(scroll) {
        if x.saturating_add(width) > end {
            break;
        }
        let style = match (shown == Some(index), focused_pane) {
            (true, true) => Style::default()
                .fg(ACTIVE)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            (true, false) => Style::default().fg(MUTED).add_modifier(Modifier::REVERSED),
            _ => Style::default().fg(MUTED),
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
    if overflow {
        let right = Rect::new(end, area.y, 2, 1);
        frame.render_widget(Paragraph::new(Span::styled(" ›", arrow)), right);
        regions.tab_arrows.push((pane, true, right));
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER)),
        area,
    );
    regions.tab_bars[pane] = area;
}

/// The editor: one pane, or two when split (F19), each with its tabs above its file. With no
/// file, the logo and the first keys sit in the middle of whatever space the panels leave.
pub(super) fn render_editor(frame: &mut Frame, app: &mut App, regions: &mut Regions, area: Rect) {
    let focused = app.focus == Focus::Editor;
    let split = app
        .split
        .filter(|split| split.other < app.open_files.len() && app.active_tab.is_some());
    let panes = match split {
        Some(split) => {
            let halves = [Constraint::Percentage(50), Constraint::Percentage(50)];
            let [first, second] = match split.direction {
                SplitDirection::Columns => Layout::horizontal(halves).areas(area),
                SplitDirection::Rows => Layout::vertical(halves).areas(area),
            };
            vec![first, second]
        }
        None => vec![area],
    };
    let active_pane = app.active_pane();
    for (pane, pane_area) in panes.into_iter().enumerate() {
        let [tabs, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(pane_area);
        render_tabs(frame, app, regions, tabs, pane);
        regions.pane_areas[pane] = pane_area;
        regions.pane_bodies[pane] = body;
        if pane == active_pane {
            regions.tabs = tabs;
            regions.editor = body;
        }
        match app.shown_tab(pane) {
            Some(index) => render_file(
                frame,
                &mut app.open_files[index],
                body,
                focused && pane == active_pane,
            ),
            None => render_empty(frame, focused, body),
        }
    }
    regions.editor_body = if split.is_some() {
        area
    } else {
        regions.pane_bodies[0]
    };
    if let Some(zone) = app.tab_drop_zone() {
        render_drop_preview(frame, regions, zone);
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

/// The half of the editor a dragged tab would take, all of it for the centre, or the pane
/// that would take it.
fn render_drop_preview(frame: &mut Frame, regions: &Regions, zone: DropZone) {
    let area = regions.editor_body;
    let halves = [Constraint::Percentage(50), Constraint::Percentage(50)];
    let [left, right] = Layout::horizontal(halves).areas(area);
    let [top, bottom] = Layout::vertical(halves).areas(area);
    let (target, title) = match zone {
        DropZone::Left => (left, " Split left "),
        DropZone::Right => (right, " Split right "),
        DropZone::Top => (top, " Split up "),
        DropZone::Bottom => (bottom, " Split down "),
        DropZone::Center => (area, " Single editor "),
        DropZone::Pane(pane) => (regions.pane_areas[pane], " Move here "),
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

/// The rows of `file` on screen, cut at its horizontal scroll, with a scrollbar on the bottom
/// border when the text is wider than the pane.
fn render_file(frame: &mut Frame, file: &mut OpenFile, area: Rect, focused: bool) {
    let title = format!(" {} — {} ", file.path.display(), language_name(&file.path));
    if let Some(image) = &mut file.image {
        let block = panel_block(title, focused);
        let inner = block.inner(area);
        frame.render_stateful_widget(StatefulImage::new().resize(Resize::Fit(None)), inner, image);
        frame.render_widget(block, area);
        return;
    }
    let number_width = file.lines.len().max(1).to_string().len();
    let gutter = number_width + 3;
    let height = area.height.saturating_sub(2) as usize;
    let text_width = (area.width.saturating_sub(2) as usize).saturating_sub(gutter);
    file.scroll_to_cursor(height, text_width);
    let (scroll, scroll_x) = (file.scroll as usize, file.scroll_x as usize);
    let lines = (scroll..(scroll + height).min(file.display_rows()))
        .map(|display| {
            let (number, spans) = file.display_line(display, text_width + scroll_x);
            let number = number.map_or_else(
                || " ".repeat(number_width),
                |number| format!("{number:>number_width$}"),
            );
            let mut line = vec![Span::styled(
                format!("{number} | "),
                Style::default().fg(MUTED),
            )];
            line.extend(skip_columns(spans, scroll_x));
            Line::from(line)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).block(panel_block(title, focused)),
        area,
    );
    if file.max_width > text_width && area.height > 2 {
        let track = Rect::new(
            area.x + 1 + gutter as u16,
            area.bottom() - 1,
            text_width as u16,
            1,
        );
        let mut state = ScrollbarState::new(file.max_width + 1 - text_width)
            .position(scroll_x)
            .viewport_content_length(text_width);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("─"))
                .thumb_symbol("━")
                .track_style(Style::default().fg(BORDER))
                .thumb_style(Style::default().fg(if focused { ACTIVE } else { MUTED })),
            track,
            &mut state,
        );
    }
    if focused {
        let column = file.cursor_x().checked_sub(scroll_x);
        let row = file.display_row(file.cursor_row).checked_sub(scroll);
        if let (Some(column), Some(row)) = (column, row) {
            let x = area
                .x
                .saturating_add((1 + gutter + column).min(u16::MAX as usize) as u16);
            let y = area
                .y
                .saturating_add((1 + row).min(u16::MAX as usize) as u16);
            if x < area.right().saturating_sub(1) && y < area.bottom().saturating_sub(1) {
                frame.set_cursor_position((x, y));
            }
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
