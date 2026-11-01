use ratatui::{
    Frame,
    buffer::CellDiffOption,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};
use ratatui_image::{Resize, StatefulImage, protocol::StatefulProtocol};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::{
    app::{
        Action, App, BOTTOM_TERMINALS, DropZone, EntryAction, Focus, FolderCount, Keymap, Kind,
        PreviewRow, Regions, SHOWN_ITEMS, SplitDirection, goal_names, grouped, human_size,
        video_areas,
    },
    branding::ASCII_LOGO,
    editor::{MarkdownDocument, OpenFile, Row as MarkdownRow, file_icon, highlight_lines},
    git::LineMark,
    lsp::{Diagnostic, char_column},
    media::Video,
};

use super::{
    ACTIVE, BORDER, CURRENT_MATCH, MATCH, MUTED, TEXT, busy, mark_columns, panel_block,
    select_columns, skip_columns,
};

pub(super) fn render_explorer(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" {} Explorer ", app.icon("custom-folder"));
    let height = area.height.saturating_sub(2) as usize;
    // Long names scroll sideways (F01); the scroll never passes the widest one.
    let room = usize::from(area.width.saturating_sub(2));
    let widest = app.explorer_width_needed();
    let scroll_x = app.explorer_scroll_x.min(widest.saturating_sub(room));
    // Git's mark of each row (F01): a file's change with the letters of Source Control, `*` on
    // a folder with a change below it, in the colour of its strongest change.
    let marks = app
        .entries
        .iter()
        .skip(app.explorer_scroll)
        .take(height)
        .map(|entry| {
            let (status, change) = app.git_change(&entry.path)?;
            let (letter, colour) = crate::git::view::status_mark(status, change);
            Some((if entry.is_dir { "*" } else { letter }, colour))
        })
        .collect::<Vec<_>>();
    let lines = app
        .entries
        .iter()
        .enumerate()
        .skip(app.explorer_scroll)
        .take(height)
        .zip(&marks)
        .map(|((index, entry), mark)| {
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
            let spans = if index == app.selected_entry {
                let style = Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED);
                vec![Span::styled(
                    format!("{indentation}{marker} {icon} {}", entry.name),
                    style,
                )]
            } else {
                vec![
                    Span::styled(format!("{indentation}{marker} "), Style::default().fg(TEXT)),
                    Span::styled(icon.to_string(), Style::default().fg(color)),
                    Span::styled(
                        format!(" {}", entry.name),
                        Style::default().fg(mark.map_or(TEXT, |(_, colour)| colour)),
                    ),
                ]
            };
            Line::from(skip_columns(spans, scroll_x))
        })
        .collect::<Vec<_>>();
    let focused = app.focus == Focus::Explorer;
    frame.render_widget(
        Paragraph::new(lines).block(panel_block(title, focused)),
        area,
    );
    // The marks stay at the right edge, over the end of a name too long for the panel.
    if area.width >= 6 {
        for (row, (letter, colour)) in marks
            .into_iter()
            .enumerate()
            .filter_map(|(row, mark)| Some((row, mark?)))
        {
            let cell = Rect::new(area.right() - 3, area.y + 1 + row as u16, 2, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {letter}"),
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

/// The horizontal scroll bar on a bottom border: `━` over `─`, for `content` columns seen
/// `track.width` at a time from `position`.
fn render_sideways_bar(
    frame: &mut Frame,
    track: Rect,
    content: usize,
    position: usize,
    focused: bool,
) {
    let view = usize::from(track.width);
    let mut state = ScrollbarState::new(content + 1 - view.min(content))
        .position(position)
        .viewport_content_length(view);
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

/// The Explorer's right-click menu at the mouse, kept on screen (F01).
pub(super) fn render_entry_menu(frame: &mut Frame, app: &mut App, screen: Rect) {
    app.regions.entry_menu_items.clear();
    app.regions.preview_rows.clear();
    let Some(menu) = &app.entry_menu else {
        return;
    };
    // As wide as its longest entry (the editor's questions to the language server, F31), with
    // room for the `▸` of the answers beside it (Q73).
    let longest = menu
        .actions
        .iter()
        .map(|action| action.label().len())
        .max()
        .unwrap_or(0);
    let width = (longest as u16 + 6).max(16);
    let height = menu.actions.len() as u16 + 2;
    let x = menu.position.0.min(screen.right().saturating_sub(width));
    let y = menu.position.1.min(screen.bottom().saturating_sub(height));
    let area = Rect::new(x, y, width, height).intersection(screen);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACTIVE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut entry = None;
    for (index, action) in menu.actions.iter().enumerate() {
        let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1).intersection(inner);
        let selected = index == menu.selected;
        let style = if selected {
            Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(TEXT)
        };
        let mut spans = vec![Span::raw(format!(" {:<longest$} ", action.label()))];
        if let EntryAction::Code(goal) = action {
            spans.push(Span::styled("▸", Style::default().fg(MUTED)));
            if selected {
                entry = Some((*goal, row.y));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), row);
        app.regions.entry_menu_items.push((*action, row));
    }
    if let Some((goal, y)) = entry.filter(|_| menu.editor) {
        render_menu_preview(frame, app, area, goal, y, screen);
    }
}

/// The answers beside the editor's menu for its code entry `goal` on row `y` (F31, Q73): at the
/// entry's height, right of the menu, or left of it without room there.
fn render_menu_preview(
    frame: &mut Frame,
    app: &mut App,
    menu: Rect,
    goal: crate::lsp::Goal,
    y: u16,
    screen: Rect,
) {
    const MOST_PLACES: usize = 8;
    const WIDEST: usize = 58;
    let Some(preview) = app
        .menu_preview
        .as_ref()
        .filter(|preview| preview.goal == goal)
    else {
        return;
    };
    let shown = |path: &std::path::Path| {
        path.strip_prefix(&app.root)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    let (name, plural) = goal_names(goal);
    let mut title = format!(" {plural} ");
    let mut rows: Vec<(Option<PreviewRow>, Line<'static>)> = Vec::new();
    match (&preview.failure, &preview.places) {
        (Some(why), _) => {
            for line in wrap_words(why, WIDEST) {
                rows.push((
                    None,
                    Line::styled(format!(" {line} "), Style::default().fg(TEXT)),
                ));
            }
            if preview.install {
                rows.push((None, Line::raw("")));
                rows.push((
                    Some(PreviewRow::Install),
                    Line::styled(
                        " Install ",
                        Style::default()
                            .fg(ACTIVE)
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                    ),
                ));
            }
        }
        (None, None) => {
            let spinner = crate::animation::spinner(app.motion.elapsed());
            let mut line = busy(spinner, "Looking\u{2026}");
            line.spans[1].style = Style::default().fg(MUTED);
            line.spans.insert(0, Span::raw(" "));
            rows.push((None, line));
        }
        (None, Some(places)) if places.is_empty() => {
            rows.push((
                None,
                Line::styled(format!(" No {name} found "), Style::default().fg(MUTED)),
            ));
        }
        (None, Some(places)) if goal == crate::lsp::Goal::Peek => {
            let found = &places[0];
            title = format!(" {}:{} ", shown(&found.path), found.line + 1);
            let end = (found.line + MOST_PLACES).min(preview.lines.len());
            let lines = &preview.lines[found.line.min(end)..end];
            let width = end.max(1).to_string().len();
            for (offset, spans) in highlight_lines(&found.path, lines).into_iter().enumerate() {
                let row = found.line + offset;
                let number = if row == found.line { ACTIVE } else { MUTED };
                let mut line = vec![Span::styled(
                    format!(" {:>width$} │ ", row + 1),
                    Style::default().fg(number),
                )];
                line.extend(spans);
                line.push(Span::raw(" "));
                rows.push((Some(PreviewRow::Place(0)), Line::from(line)));
            }
        }
        (None, Some(places)) => {
            title = format!(" {plural} ({}) ", places.len());
            for (index, place) in places.iter().take(MOST_PLACES).enumerate() {
                let row = PreviewRow::Place(index);
                let line = Line::from(vec![
                    Span::styled(
                        format!(" {}:{}  ", shown(&place.path), place.line + 1),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(format!("{} ", place.text.trim()), Style::default().fg(TEXT)),
                ]);
                rows.push((Some(row), line));
            }
            if places.len() > MOST_PLACES {
                rows.push((
                    Some(PreviewRow::More),
                    Line::styled(
                        format!(" +{} more ", places.len() - MOST_PLACES),
                        Style::default().fg(MUTED),
                    ),
                ));
            }
        }
    }
    let widest = rows
        .iter()
        .map(|(_, line)| line.width())
        .chain([title.chars().count()])
        .max()
        .unwrap_or(0)
        .clamp(16, WIDEST + 2) as u16;
    let (width, height) = (widest + 2, rows.len() as u16 + 2);
    let x = if menu.right() + width <= screen.right() {
        menu.right()
    } else {
        menu.x.saturating_sub(width)
    };
    let y = y
        .saturating_sub(1)
        .min(screen.bottom().saturating_sub(height))
        .max(screen.y);
    let area = Rect::new(x, y, width, height).intersection(screen);
    frame.render_widget(Clear, area);
    let block = intelligence_block().title(Span::styled(title, Style::default().fg(ACTIVE)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // The answer under the mouse in ACTIVE inverse; Peek's code is one place, not marked.
    let hovered = preview
        .hovered
        .filter(|row| *row != PreviewRow::Install && goal != crate::lsp::Goal::Peek);
    for (offset, (row, mut line)) in rows.into_iter().enumerate() {
        let area = Rect::new(inner.x, inner.y + offset as u16, inner.width, 1).intersection(inner);
        let marked = row.is_some() && row == hovered;
        let style = Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED);
        if marked {
            for span in &mut line.spans {
                span.style = style;
            }
        }
        let paragraph = Paragraph::new(line);
        frame.render_widget(
            if marked {
                paragraph.style(style)
            } else {
                paragraph
            },
            area,
        );
        if let Some(row) = row {
            app.regions.preview_rows.push((row, area));
        }
    }
}

/// `text` in lines of at most `width` characters, broken between words.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let Some(last) = lines.last_mut() else {
            break;
        };
        if !last.is_empty() && last.chars().count() + 1 + word.chars().count() > width {
            lines.push(word.to_owned());
        } else {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }
    lines
}

/// The Properties of an Explorer entry (F30): a folder says «Calculating…» until its count
/// arrives, then its size and what it contains.
pub(super) fn render_properties(frame: &mut Frame, app: &mut App, screen: Rect) {
    let Some(dialog) = &app.properties else {
        app.regions.properties = Rect::default();
        return;
    };
    let read = &dialog.properties;
    let (icon, color) = if read.kind == Kind::Folder {
        (app.icon("custom-folder"), ACTIVE)
    } else {
        file_icon(&read.path)
    };
    let name = read
        .path
        .file_name()
        .unwrap_or(read.path.as_os_str())
        .to_string_lossy();
    // Where it is: inside the project from the project folder's name, the project itself in full.
    let parent = read.path.parent().unwrap_or(&read.path);
    let inside = parent.strip_prefix(app.root.parent().unwrap_or(&app.root));
    let location = match inside {
        Ok(inside) if !inside.as_os_str().is_empty() => inside.display().to_string(),
        _ => parent.display().to_string(),
    };
    let spinner = crate::animation::spinner(app.motion.elapsed());
    let calculating = || busy(spinner, "Calculating\u{2026}");
    let size = match (read.size, dialog.count) {
        (Some(bytes), _) | (None, Some(FolderCount { bytes, .. })) => human_size(bytes).into(),
        (None, None) => calculating(),
    };
    let plural = |count: u64, one: &str| {
        format!(
            "{} {one}{}",
            grouped(count),
            if count == 1 { "" } else { "s" }
        )
    };
    let date = |time: Option<chrono::DateTime<chrono::Local>>| {
        time.map_or("Unknown".into(), |time| {
            time.format("%Y-%m-%d %H:%M:%S").to_string()
        })
    };
    let mut rows = vec![
        ("Kind", Line::from(read.kind.label())),
        ("Location", Line::from(location)),
        ("Size", size),
    ];
    if read.kind == Kind::Folder {
        let contains = dialog.count.map_or_else(calculating, |count| {
            Line::from(format!(
                "{}, {}",
                plural(count.files, "file"),
                plural(count.folders, "folder")
            ))
        });
        rows.push(("Contains", contains));
    }
    rows.extend([
        ("Created", Line::from(date(read.created))),
        ("Modified", Line::from(date(read.modified))),
        ("Accessed", Line::from(date(read.accessed))),
        ("Permissions", Line::from(read.permissions.clone())),
    ]);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::styled(
                name.into_owned(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::default(),
    ];
    lines.extend(rows.into_iter().map(|(label, value)| {
        let mut spans = vec![Span::styled(
            format!("{label:<13}"),
            Style::default().fg(MUTED),
        )];
        spans.extend(value.spans);
        Line::from(spans)
    }));
    if let Some(unreadable) = dialog
        .count
        .map(|count| count.unreadable)
        .filter(|n| *n > 0)
    {
        lines.push(Line::styled(
            format!("{} could not be read", plural(unreadable, "item")),
            Style::default().fg(MUTED),
        ));
    }
    lines.extend([
        Line::default(),
        Line::styled("Esc: close", Style::default().fg(MUTED)),
    ]);
    let width = 64.min(screen.width);
    let height = (lines.len() as u16 + 2).min(screen.height);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    app.regions.properties = area;
    super::render_dialog(frame, area, 1.0, " Properties ".into(), |frame, inner| {
        let inner = Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );
        frame.render_widget(Paragraph::new(lines), inner);
    });
}

/// The name of a new file or folder, or of a rename: the icon follows the extension typed.
pub(super) fn render_name_prompt(frame: &mut Frame, app: &App, screen: Rect) {
    let Some(prompt) = &app.name_prompt else {
        return;
    };
    let folder = match prompt.action {
        EntryAction::NewFolder => true,
        EntryAction::NewFile => false,
        _ => prompt.target.is_dir(),
    };
    let (icon, color) = if folder {
        (app.icon("custom-folder"), ACTIVE)
    } else {
        file_icon(std::path::Path::new(&prompt.name))
    };
    let place = if matches!(prompt.action, EntryAction::Rename | EntryAction::Delete) {
        prompt.target.clone()
    } else if prompt.target.is_dir() {
        prompt.target.join("")
    } else {
        prompt.target.parent().unwrap_or(&app.root).join("")
    };
    let place = place
        .strip_prefix(&app.root)
        .unwrap_or(&place)
        .display()
        .to_string();
    let width = 60.min(screen.width);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + screen.height.saturating_sub(7) / 2,
        width,
        7.min(screen.height),
    );
    if prompt.action == EntryAction::Delete {
        // Nothing to type: what goes, that it does not come back, and the two keys (Q39).
        let (icon, color) = if folder {
            (app.icon("custom-folder"), ACTIVE)
        } else {
            file_icon(&prompt.target)
        };
        let what = if folder {
            "The folder and everything in it are deleted for good."
        } else {
            "The file is deleted for good."
        };
        let lines = vec![
            Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(
                    format!("{place}?"),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                what,
                Style::default().fg(ratatui::style::Color::Yellow),
            )),
            Line::from(Span::styled(
                "This cannot be undone.",
                Style::default().fg(ratatui::style::Color::Yellow),
            )),
            Line::from(Span::styled(
                prompt.error.clone().unwrap_or_default(),
                Style::default().fg(ratatui::style::Color::Red),
            )),
            Line::from(Span::styled(
                "Enter: delete  Esc: cancel",
                Style::default().fg(MUTED),
            )),
        ];
        super::render_dialog(frame, area, 1.0, " Delete ".into(), |frame, inner| {
            frame.render_widget(Paragraph::new(lines), inner);
        });
        return;
    }
    let verb = if prompt.action == EntryAction::Rename {
        "rename"
    } else {
        "create"
    };
    let lines = vec![
        Line::from(Span::styled(
            if place.is_empty() {
                "in the project folder".to_owned()
            } else {
                format!(
                    "{} {place}",
                    if prompt.action == EntryAction::Rename {
                        "was"
                    } else {
                        "in"
                    }
                )
            },
            Style::default().fg(MUTED),
        )),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(ACTIVE)),
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::styled(prompt.name.clone(), Style::default().fg(TEXT)),
            Span::styled("█", Style::default().fg(ACTIVE)),
        ]),
        Line::from(Span::styled(
            prompt.error.clone().unwrap_or_default(),
            Style::default().fg(ratatui::style::Color::Red),
        )),
        Line::from(Span::styled(
            format!("Enter: {verb}  Esc: cancel"),
            Style::default().fg(MUTED),
        )),
    ];
    super::render_dialog(
        frame,
        area,
        1.0,
        format!(" {} ", prompt.action.label()),
        |frame, inner| frame.render_widget(Paragraph::new(lines), inner),
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
            Some(index) => {
                let marks = search_marks(app, index);
                let empty = Vec::new();
                let path = &app.open_files[index].path;
                let problems = app.lsp.diagnostics.get(path).unwrap_or(&empty);
                let none = Default::default();
                let margin = Margin {
                    breakpoints: app.breakpoints.get(path).unwrap_or(&none),
                    paused: app.paused_row(path),
                    git: app.line_marks.get(path).map(|(_, marks)| marks),
                };
                render_file(
                    frame,
                    &mut app.open_files[index],
                    body,
                    focused && pane == active_pane,
                    &marks,
                    problems,
                    &margin,
                );
                if pane == active_pane {
                    render_completion(frame, app, index, body);
                    render_peek(frame, app, regions, index, body);
                }
                render_hover(frame, app, regions, index, body);
                // The word under the mouse while Ctrl is held, as a link (F31).
                if let Some((link, row, start, end)) = app.link
                    && link == index
                {
                    let (x, y) = cell_of(&app.open_files[index], body, row, start);
                    let inner = Rect::new(
                        body.x + 1,
                        body.y + 1,
                        body.width.saturating_sub(2),
                        body.height.saturating_sub(2),
                    );
                    let word = Rect::new(x, y, (end - start) as u16, 1).intersection(inner);
                    frame
                        .buffer_mut()
                        .set_style(word, Style::default().add_modifier(Modifier::UNDERLINED));
                }
            }
            None => render_empty(frame, &app.keys, focused, body),
        }
    }
    regions.editor_body = if split.is_some() {
        area
    } else {
        regions.pane_bodies[0]
    };
    render_debug(frame, app, regions.editor_body);
    if let Some(zone) = app.tab_drop_zone() {
        render_drop_preview(frame, regions, zone);
    }
}

fn render_empty(frame: &mut Frame, keys: &Keymap, focused: bool, area: Rect) {
    let block = panel_block(" Editor ".to_string(), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let hints = [
        "Select a file in Explorer".to_string(),
        format!(
            "{}: search file  |  {}: search workspace",
            keys.label(Action::FindInFile),
            keys.label(Action::FindInWorkspace)
        ),
        format!("{}: all commands", keys.label(Action::Help)),
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

/// The search matches in the file of tab `index` while the search is open: row, column and
/// length in characters, and whether it is the one the search is on.
fn search_marks(app: &App, index: usize) -> Vec<(usize, usize, usize, bool)> {
    let Some(search) = &app.search else {
        return Vec::new();
    };
    let path = &app.open_files[index].path;
    search
        .matches
        .iter()
        .enumerate()
        .filter(|(_, found)| &found.path == path)
        .map(|(number, found)| {
            (
                found.line,
                found.column,
                found.length,
                number == search.selected,
            )
        })
        .collect()
}

/// The debugger's panel on the right of the editor (F27): what it is doing, the call stack,
/// the variables of the paused frame and the program's output.
fn render_debug(frame: &mut Frame, app: &App, body: Rect) {
    use crate::debug::State;
    let session = app.debug.as_ref();
    if session.is_none() && !app.debug_building() {
        return;
    }
    let width = (body.width / 2).clamp(24, 48);
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
        Some(State::Running) => ("Running".to_owned(), Color::Green),
        Some(State::Paused(reason)) => (format!("Paused: {reason}"), Color::Yellow),
        Some(State::Ended(how)) => (how.clone(), MUTED),
    };
    let mut lines = vec![Line::styled(cut(state), Style::default().fg(colour))];
    if let Some(session) = session {
        if !session.frames.is_empty() {
            lines.push(heading("Call stack"));
            lines.extend(session.frames.iter().take(6).map(|frame| {
                let place = frame.path.as_ref().map_or_else(String::new, |path| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    format!("  {name}:{}", frame.row + 1)
                });
                Line::from(vec![
                    Span::styled(cut(frame.name.clone()), Style::default().fg(TEXT)),
                    Span::styled(place, Style::default().fg(MUTED)),
                ])
            }));
        }
        if !session.variables.is_empty() {
            lines.push(heading("Variables"));
            let shown = usize::from(body.height / 2).max(3);
            lines.extend(session.variables.iter().take(shown).map(|(name, value)| {
                Line::from(vec![
                    Span::styled(format!("{name} = "), Style::default().fg(TEXT)),
                    Span::styled(cut(value.clone()), Style::default().fg(MUTED)),
                ])
            }));
        }
        if !session.output.is_empty() {
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
    }
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACTIVE))
                .title(" Debug ")
                .title_bottom(Line::from(" F5 F10 F11  Shift+F5 stop ").right_aligned()),
        ),
        area,
    );
}

/// What the margin marks for the debugger (F27).
struct Margin<'a> {
    breakpoints: &'a std::collections::BTreeSet<usize>,
    paused: Option<usize>,
    /// Each row's change since the last commit (F34).
    git: Option<&'a std::collections::HashMap<usize, LineMark>>,
}

/// The cell between a line's number and its text: its Git mark (F34), or the bar in the
/// number's colour.
fn git_mark(mark: Option<&LineMark>, number: Style) -> Span<'static> {
    let (glyph, colour) = match mark {
        Some(LineMark::Added) => ("▎", Color::Green),
        Some(LineMark::Modified) => ("▎", Color::Yellow),
        Some(LineMark::DeletedBelow) => ("▁", Color::Red),
        Some(LineMark::DeletedAbove) => ("▔", Color::Red),
        None => return Span::styled("|", number),
    };
    Span::styled(glyph, Style::default().fg(colour))
}

/// The suggestions of the language server under the cursor of the active file (F26).
fn render_completion(frame: &mut Frame, app: &App, index: usize, body: Rect) {
    let Some((items, selected, _)) = app.completion_view() else {
        return;
    };
    if items.is_empty() {
        return;
    }
    let file = &app.open_files[index];
    let gutter = file.lines.len().max(1).to_string().len() + 3;
    let row = file
        .display_row(file.cursor_row)
        .saturating_sub(file.scroll as usize);
    let column = file.cursor_x().saturating_sub(file.scroll_x as usize);
    // The list scrolls with the selection, a page of `SHOWN_ITEMS` at a time.
    let first = selected / SHOWN_ITEMS * SHOWN_ITEMS;
    let shown = &items[first..items.len().min(first + SHOWN_ITEMS)];
    let width = shown
        .iter()
        .map(|item| item.label.chars().count() + item.detail.chars().count().min(30) + 3)
        .max()
        .unwrap_or(10)
        .clamp(16, 60) as u16
        + 2;
    let height = shown.len() as u16 + 2;
    let below = body.y + 1 + row as u16 + 1;
    let y = if below + height <= body.bottom() {
        below
    } else {
        (body.y + 1 + row as u16).saturating_sub(height)
    };
    let x = (body.x + 1 + (gutter + column) as u16).min(body.right().saturating_sub(width));
    let area = Rect::new(x, y, width, height).intersection(body);
    frame.render_widget(Clear, area);
    let lines = shown
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let detail = item.detail.chars().take(30).collect::<String>();
            let room = (width as usize).saturating_sub(item.label.chars().count() + 3);
            let line = Line::from(vec![
                Span::styled(format!(" {}", item.label), Style::default().fg(TEXT)),
                Span::styled(format!("{detail:>room$} "), Style::default().fg(MUTED)),
            ]);
            if first + position == selected {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect::<Vec<_>>();
    let title = format!(" {}/{} ", selected + 1, items.len());
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACTIVE))
                .title_bottom(Line::from(title).right_aligned()),
        ),
        area,
    );
}

/// A box of Code Intelligence (F31) by the cell `anchor` in `body`: below its row when it
/// fits, above otherwise (where there is more room), never over the row itself.
fn box_by(anchor: (u16, u16), width: u16, height: u16, body: Rect) -> Rect {
    let width = width.min(body.width);
    let below = anchor.1.saturating_add(1);
    let room_below = body.bottom().saturating_sub(below);
    let room_above = anchor.1.saturating_sub(body.y);
    let (y, height) = if height <= room_below || room_below >= room_above {
        (below, height.min(room_below))
    } else {
        let height = height.min(room_above);
        (anchor.1 - height, height)
    };
    let x = anchor.0.min(body.right().saturating_sub(width)).max(body.x);
    Rect::new(x, y, width, height)
}

/// The screen cell of character `column` of `row` of `file` drawn in `body`.
fn cell_of(file: &OpenFile, body: Rect, row: usize, column: usize) -> (u16, u16) {
    let gutter = file.lines.len().max(1).to_string().len() + 3;
    let y = file.display_row(row).saturating_sub(file.scroll as usize);
    let prefix = file
        .lines
        .get(row)
        .map(|line| line.chars().take(column).collect::<String>())
        .unwrap_or_default();
    let x = gutter
        + Line::from(prefix)
            .width()
            .saturating_sub(file.scroll_x as usize);
    (
        body.x.saturating_add(1 + x as u16),
        body.y.saturating_add(1 + y as u16),
    )
}

fn intelligence_block() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACTIVE))
}

/// What the language server says about the word under the mouse or the cursor (F31), in
/// Markdown drawn as the editor draws it, wrapped and scrolled.
fn render_hover(frame: &mut Frame, app: &mut App, regions: &mut Regions, index: usize, body: Rect) {
    let file = &app.open_files[index];
    let Some((hover, text)) = app.hover.as_ref().and_then(|hover| {
        let text = hover.text.as_deref().filter(|_| hover.path == file.path)?;
        Some((hover, text))
    }) else {
        return;
    };
    let anchor = hover
        .mouse
        .unwrap_or_else(|| cell_of(file, body, hover.row, hover.start));
    let lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let highlighted = highlight_lines(std::path::Path::new("hover.md"), &lines);
    let document = MarkdownDocument::parse(&lines);
    // The frames of code blocks and tables are wider than their source (E49).
    // ponytail: a frame wider than the box still wraps; let the box grow past 60 if it matters.
    let inner_width = lines
        .iter()
        .map(|line| Line::from(line.as_str()).width())
        .max()
        .unwrap_or(0)
        .max(document.width())
        .clamp(10, 58)
        .min(usize::from(body.width.saturating_sub(2)));
    let rendered = lines
        .iter()
        .zip(&highlighted)
        .enumerate()
        .map(|(row, (line, spans))| {
            Line::from(
                document
                    .render(MarkdownRow::plain(row), &[], inner_width, line, spans)
                    .unwrap_or_else(|| spans.clone()),
            )
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(rendered).wrap(Wrap { trim: false });
    let total = paragraph.line_count(inner_width as u16) as u16;
    // Small, as the right-click menu (Q72).
    let area = box_by(anchor, inner_width as u16 + 2, total.min(10) + 2, body);
    let overflow = total.saturating_sub(area.height.saturating_sub(2));
    let scroll = hover.scroll.min(overflow);
    let mut block = intelligence_block();
    if overflow > 0 {
        block = block.title_bottom(
            Line::from(Span::styled(" ↑/↓ ", Style::default().fg(MUTED))).right_aligned(),
        );
    }
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph.scroll((scroll, 0)).block(block), area);
    regions.hover = area;
    if let Some(hover) = &mut app.hover {
        hover.overflow = overflow;
        hover.scroll = scroll;
    }
}

/// The definition in a box under the cursor without leaving the tab (Peek, F31).
fn render_peek(frame: &mut Frame, app: &App, regions: &mut Regions, index: usize, body: Rect) {
    let Some(peek) = &app.peek else {
        return;
    };
    let file = &app.open_files[index];
    let (_, row) = cell_of(file, body, file.cursor_row, file.cursor_column);
    let area = box_by((body.x + 1, row), body.width.saturating_sub(2), 14, body);
    let end = (peek.top + usize::from(area.height.saturating_sub(2))).min(peek.lines.len());
    let shown = &peek.lines[peek.top.min(end)..end];
    // ponytail: only the rows on screen are highlighted, so a comment opened above them is
    // not seen; highlight from the top of the file if that matters.
    let highlighted = highlight_lines(&peek.path, shown);
    let number_width = peek.lines.len().max(1).to_string().len();
    let lines = highlighted
        .into_iter()
        .enumerate()
        .map(|(offset, spans)| {
            let row = peek.top + offset;
            let style = if row == peek.row {
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            };
            let mut line = vec![Span::styled(
                format!("{:>number_width$} │ ", row + 1),
                style,
            )];
            line.extend(spans);
            Line::from(line)
        })
        .collect::<Vec<_>>();
    let shown_path = peek.path.strip_prefix(&app.root).unwrap_or(&peek.path);
    let block = intelligence_block()
        .title(format!(
            " Peek ─ {}:{} ",
            shown_path.display(),
            peek.row + 1
        ))
        .title_bottom(
            Line::from(Span::styled(
                " Up/Down: scroll  Enter: go  Esc: close ",
                Style::default().fg(MUTED),
            ))
            .right_aligned(),
        );
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
    regions.peek = area;
}

/// A picture, GIF, video or model in `area`; false when `file` is none of those.
pub(super) fn render_media(
    frame: &mut Frame,
    file: &mut OpenFile,
    area: Rect,
    focused: bool,
) -> bool {
    let title = format!(" {} — {} ", file.path.display(), file.language);
    let Some(media) = &mut file.media else {
        return false;
    };
    let mut block = panel_block(title, focused);
    // The keys of a model or a video on the right of the border, like the page of a PDF.
    if let Some(hint) = media.hint() {
        block = block.title_top(Line::from(format!(" {hint} ")).right_aligned());
    }
    if let Some(video) = media.video() {
        // The frame, then the progress row and the buttons (F23).
        let [picture, progress, buttons] = video_areas(area);
        render_video_controls(frame, video, progress, buttons);
        frame.render_widget(block, area);
        let lines = media.lines(picture.width, picture.height).to_vec();
        frame.render_widget(Paragraph::new(lines), picture);
        return true;
    }
    let inner = block.inner(area);
    let lines = media.lines(inner.width, inner.height).to_vec();
    frame.render_widget(Paragraph::new(lines).block(block), area);
    true
}

/// A picture in the terminal's picture protocol. In Kitty each row of it, and the picture
/// itself on the first frame, is text in the row's first cell; ratatui counts that text as that
/// many columns and skips as many cells after it, so the rest of the screen stayed undrawn
/// until another tab redrew it (E35). One column it is.
pub(super) fn render_picture(
    frame: &mut Frame,
    image: &mut StatefulProtocol,
    area: Rect,
    resize: Resize,
) {
    frame.render_stateful_widget(StatefulImage::new().resize(resize), area, image);
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        if let Some(cell) = buffer.cell_mut((area.x, y)) {
            cell.set_diff_option(CellDiffOption::ForcedWidth(std::num::NonZeroU16::MIN));
        }
    }
}

/// Under a video: the time, the progress bar and the length; then the buttons, with the
/// volume and the speed on the right.
fn render_video_controls(frame: &mut Frame, video: &Video, progress: Rect, buttons: Rect) {
    let (shown, _, length, played, total) = video.progress(progress.width);
    let bar = vec![
        Span::styled(format!(" {shown} "), Style::default().fg(MUTED)),
        Span::styled("━".repeat(usize::from(played)), Style::default().fg(ACTIVE)),
        Span::styled(
            "─".repeat(usize::from(length - played)),
            Style::default().fg(BORDER),
        ),
        Span::styled(format!(" {total} "), Style::default().fg(MUTED)),
    ];
    frame.render_widget(Paragraph::new(Line::from(bar)), progress);
    let mut row = vec![Span::raw(" ")];
    for (_, label) in video.buttons() {
        row.push(Span::styled(
            format!(" {label} "),
            Style::default().fg(TEXT).add_modifier(Modifier::REVERSED),
        ));
        row.push(Span::raw(" "));
    }
    row.push(Span::styled(
        format!(" {}", video.status()),
        Style::default().fg(MUTED),
    ));
    frame.render_widget(Paragraph::new(Line::from(row)), buttons);
}

/// The rows of `file` on screen, cut at its horizontal scroll, with a scrollbar on the bottom
/// border when the text is wider than the pane, and `marks` (search matches) marked.
fn render_file(
    frame: &mut Frame,
    file: &mut OpenFile,
    area: Rect,
    focused: bool,
    marks: &[(usize, usize, usize, bool)],
    problems: &[Diagnostic],
    margin: &Margin,
) {
    if render_media(frame, file, area, focused) {
        return;
    }
    let title = format!(" {} — {} ", file.path.display(), file.language);
    if let Some(image) = &mut file.image {
        let mut block = panel_block(title, focused);
        // The page of a PDF on the right of the border, where a long path does not hide it.
        if let Some((page, pages)) = file.pdf.as_ref().map(|pdf| (pdf.page, pdf.pages())) {
            block =
                block.title_top(Line::from(format!(" Page {}/{pages} ", page + 1)).right_aligned());
        }
        let inner = block.inner(area);
        render_picture(frame, image, inner, Resize::Fit(None));
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
            let (number, mut spans) = file.display_line(display, text_width + scroll_x);
            let selected = number
                .and_then(|_| file.source_row(display))
                .and_then(|row| file.selected_columns(row));
            if let Some((from, to)) = selected {
                spans = select_columns(spans, from, to);
            }
            let row = number.and_then(|_| file.source_row(display));
            for &(_, column, length, current) in marks.iter().filter(|mark| Some(mark.0) == row) {
                let line = row
                    .and_then(|row| file.lines.get(row))
                    .map_or("", String::as_str);
                let from = Span::raw(line.chars().take(column).collect::<String>()).width();
                let to = from
                    + Span::raw(line.chars().skip(column).take(length).collect::<String>()).width();
                let style = if current { CURRENT_MATCH } else { MATCH };
                spans = mark_columns(spans, from, to, style);
            }
            // Problems from the language server: the number in red (error) or yellow, the text
            // underlined in that colour (F26).
            let line_text = row
                .and_then(|row| file.lines.get(row))
                .map_or("", String::as_str);
            let mut number_style = Style::default().fg(MUTED);
            for problem in problems.iter().filter(|problem| {
                row.is_some_and(|row| (problem.row..=problem.end_row).contains(&row))
            }) {
                let colour = if problem.error {
                    Color::Red
                } else {
                    Color::Yellow
                };
                if number_style.fg == Some(MUTED) || problem.error {
                    number_style = Style::default().fg(colour);
                }
                let row = row.unwrap_or_default();
                let chars = line_text.chars().count();
                let from = if row == problem.row {
                    char_column(line_text, problem.start)
                } else {
                    0
                };
                let to = if row == problem.end_row {
                    char_column(line_text, problem.end)
                } else {
                    chars
                };
                let to = to.max(from + 1);
                let width = |column: usize| {
                    Span::raw(line_text.chars().take(column).collect::<String>()).width()
                };
                spans = mark_columns(
                    spans,
                    width(from),
                    width(to).max(width(from) + 1),
                    Style::default()
                        .add_modifier(Modifier::UNDERLINED)
                        .underline_color(colour),
                );
            }
            let number = number.map_or_else(
                || " ".repeat(number_width),
                |number| format!("{number:>number_width$}"),
            );
            // Before the `|`: a breakpoint (●) or the line the debugger is paused at (▶).
            let mark = match row {
                Some(row) if margin.paused == Some(row) => {
                    Span::styled("▶", Style::default().fg(Color::Yellow))
                }
                Some(row) if margin.breakpoints.contains(&row) => {
                    Span::styled("●", Style::default().fg(Color::Red))
                }
                _ => Span::raw(" "),
            };
            let git = margin.git.zip(row).and_then(|(marks, row)| marks.get(&row));
            let mut line = vec![
                Span::styled(number, number_style),
                mark,
                git_mark(git, number_style),
                Span::styled(" ", number_style),
            ];
            line.extend(skip_columns(spans, scroll_x));
            Line::from(line)
        })
        .collect::<Vec<_>>();
    let mut block = panel_block(title, focused);
    // A page of text of a PDF says which page it is, like a page drawn as a picture.
    if let Some((page, pages)) = file.page() {
        block = block.title_top(Line::from(format!(" Page {}/{pages} ", page + 1)).right_aligned());
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
    if file.max_width > text_width && area.height > 2 {
        let track = Rect::new(
            area.x + 1 + gutter as u16,
            area.bottom() - 1,
            text_width as u16,
            1,
        );
        render_sideways_bar(frame, track, file.max_width, scroll_x, focused);
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

/// A terminal panel: the shell of the tab on screen, its tabs on the right of the top border
/// (F04, Q59) and the text chosen with the mouse in reverse (Q57).
pub(super) fn render_terminal(
    frame: &mut Frame,
    app: &App,
    regions: &mut Regions,
    area: Rect,
    panel: Focus,
) {
    let bottom = panel == Focus::BottomTerminal;
    let (terminals, name, action) = if bottom {
        (
            &app.bottom_terminal,
            "Bottom terminal",
            Action::BottomTerminal,
        )
    } else {
        (&app.right_terminal, "Right terminal", Action::RightTerminal)
    };
    let focused = app.focus == panel;
    let terminal = terminals.current();
    // ` 1 x `, ` 2 x ` and ` + ` below three shells at the bottom; ` 1 ` and ` 2 ` on the right.
    let count = terminals.tabs.len();
    let closable = bottom && count > 1;
    let labels = (1..=count)
        .map(|number| {
            if closable {
                format!(" {number} x ")
            } else {
                format!(" {number} ")
            }
        })
        .collect::<Vec<_>>();
    let add = bottom && count < BOTTOM_TERMINALS;
    let tabs_width = labels
        .iter()
        .map(|label| label.len() as u16 + 1)
        .sum::<u16>()
        + if add { 4 } else { 0 };
    let scrollback = terminal.scrollback();
    let icon = app.icon("dev-terminal");
    let history = if scrollback > 0 {
        format!(" [history: {scrollback}]")
    } else {
        String::new()
    };
    // The title gives way to the tabs: without its key, then only the icon.
    let room = usize::from(area.width.saturating_sub(tabs_width + 3));
    let title = [
        format!(" {icon} {name} [{}]{history} ", app.keys.label(action)),
        format!(" {icon} {name}{history} "),
        format!(" {icon} "),
    ]
    .into_iter()
    .find(|title| Line::from(title.as_str()).width() <= room)
    .unwrap_or_default();
    if let Ok(parser) = terminal.parser.read() {
        let cursor = Cursor::default().visibility(focused && scrollback == 0);
        frame.render_widget(
            PseudoTerminal::new(parser.screen())
                .block(panel_block(title, focused))
                .cursor(cursor),
            area,
        );
    }
    if let Some(selection) = app
        .terminal_selection
        .filter(|chosen| chosen.panel == panel)
    {
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let (start, end) = selection.bounds();
        let buffer = frame.buffer_mut();
        for row in start.0..=end.0.min(inner.height.saturating_sub(1)) {
            let from = if row == start.0 { start.1 } else { 0 };
            let to = if row == end.0 { end.1 } else { inner.width };
            for column in from..=to.min(inner.width.saturating_sub(1)) {
                buffer[(inner.x + column, inner.y + row)]
                    .modifier
                    .toggle(Modifier::REVERSED);
            }
        }
    }
    if area.height == 0 || area.width < tabs_width + 4 {
        return;
    }
    let mut x = area.right() - 1 - tabs_width;
    let button = Style::default().fg(TEXT).add_modifier(Modifier::REVERSED);
    let shown = Style::default()
        .fg(ACTIVE)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    for (index, label) in labels.iter().enumerate() {
        let width = label.len() as u16;
        let style = if index == terminals.active {
            shown
        } else {
            button
        };
        frame.buffer_mut().set_string(x, area.y, label, style);
        let (tab, close) = if closable {
            (
                Rect::new(x, area.y, width - 2, 1),
                Rect::new(x + width - 2, area.y, 2, 1),
            )
        } else {
            (Rect::new(x, area.y, width, 1), Rect::default())
        };
        regions.terminal_tabs.push((panel, index, tab, close));
        x += width + 1;
    }
    if add {
        frame.buffer_mut().set_string(x, area.y, " + ", button);
        regions.terminal_add = Rect::new(x, area.y, 3, 1);
    }
}
