use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};
use ratatui_image::{Resize, StatefulImage};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::{
    app::{App, DropZone, EntryAction, Focus, Regions, SHOWN_ITEMS, SplitDirection, video_areas},
    branding::ASCII_LOGO,
    editor::{OpenFile, file_icon},
    lsp::{Diagnostic, char_column},
    media::Video,
};

use super::{
    ACTIVE, BORDER, CURRENT_MATCH, MATCH, MUTED, TEXT, key, mark_columns, panel_block,
    select_columns, skip_columns,
};

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

/// The Explorer's right-click menu at the mouse, kept on screen (F01).
pub(super) fn render_entry_menu(frame: &mut Frame, app: &mut App, screen: Rect) {
    app.regions.entry_menu_items.clear();
    let Some(menu) = &app.entry_menu else {
        return;
    };
    let width = 16;
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
    for (index, action) in menu.actions.iter().enumerate() {
        let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1).intersection(inner);
        let style = if index == menu.selected {
            Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(TEXT)
        };
        frame.render_widget(
            Paragraph::new(format!(" {}", action.label())).style(style),
            row,
        );
        app.regions.entry_menu_items.push((*action, row));
    }
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
                }
            }
            None => render_empty(frame, focused, body),
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
        match file.image.as_mut().filter(|_| video.error.is_none()) {
            // ponytail: each frame is scaled to the pane and sent whole; encode in a
            // thread (ratatui-image `thread`) if it slows the keys in Sixel terminals.
            Some(image) => frame.render_stateful_widget(
                StatefulImage::new().resize(Resize::Scale(None)),
                picture,
                image,
            ),
            None => {
                let lines = media.lines(picture.width, picture.height).to_vec();
                frame.render_widget(Paragraph::new(lines), picture);
            }
        }
        return true;
    }
    let inner = block.inner(area);
    let lines = media.lines(inner.width, inner.height).to_vec();
    frame.render_widget(Paragraph::new(lines).block(block), area);
    true
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
            let mut line = vec![
                Span::styled(number, number_style),
                mark,
                Span::styled("| ", number_style),
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
