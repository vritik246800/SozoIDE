use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::{
    app::{App, AppMotion, TUTORIAL_STEPS},
    branding::{self, ASCII_LOGO, ASCII_LOGO_LARGE},
    editor::SearchScope,
    update::{NOTES, UpdateState},
};

use super::{
    ACTIVE, BORDER, MUTED, TEXT, bar, centered, components::key_for, render_dialog, scale_length,
    sweep_bar,
};

/// The splash prints line by line, its bar sweeps while SozoIDE starts, and then the IDE is
/// drawn over it line by line from the top (`DESIGN.md` §1.3).
pub(super) fn render_splash(frame: &mut Frame, area: Rect, motion: &AppMotion) {
    let cut = scale_length(area.height, motion.splash_exit());
    let visible = Rect::new(area.x, area.y + cut, area.width, area.height - cut);
    if visible.is_empty() {
        return;
    }
    let mut lines = ASCII_LOGO
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::default().fg(ACTIVE))))
        .collect::<Vec<_>>();
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            branding::NAME,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Version {}", branding::VERSION)),
        Line::from(""),
        sweep_bar(30, motion.elapsed()),
        Line::from(Span::styled("Starting...", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            "Press any key to continue",
            Style::default().fg(MUTED),
        )),
    ]);
    let top = area.height.saturating_sub(lines.len() as u16) / 2;
    let printed = (motion.splash_draw() * lines.len() as f32).ceil() as usize;
    lines.truncate(printed);
    frame.render_widget(Clear, visible);
    let first = top.max(cut);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .scroll((first - top, 0)),
        Rect::new(area.x, area.y + first, area.width, area.height - first),
    );
}

pub(super) fn render_search(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(search) = &app.search else {
        return;
    };
    let scope = match search.scope {
        SearchScope::CurrentFile => "Find in current file [Ctrl+P]",
        SearchScope::Workspace => "Find in workspace [Ctrl+Shift+P]",
    };
    let progress = app.motion.search();
    render_dialog(
        frame,
        centered(area, 84, 70),
        progress,
        format!(" {scope} "),
        |frame, inner| render_search_results(frame, app, inner),
    );
}

fn render_search_results(frame: &mut Frame, app: &mut App, inner: Rect) {
    if inner.height < 2 {
        return;
    }
    let root = app.root.clone();
    let Some(search) = &mut app.search else {
        return;
    };
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(inner);
    app.regions.search_input = sections[0];
    app.regions.search_results = sections[1];
    frame.render_widget(
        Paragraph::new(format!("> {}", search.query)).style(Style::default().fg(TEXT)),
        sections[0],
    );
    let height = sections[1].height as usize;
    let selected = search.selected;
    if selected >= search.scroll + height {
        search.scroll = selected + 1 - height;
    } else if selected < search.scroll {
        search.scroll = selected;
    }
    app.regions.search_match_items.clear();
    let lines = search
        .matches
        .iter()
        .enumerate()
        .skip(search.scroll)
        .take(height)
        .map(|(index, found)| {
            let row = sections[1].y + (index - search.scroll) as u16;
            app.regions
                .search_match_items
                .push((index, Rect::new(sections[1].x, row, sections[1].width, 1)));
            let path = found
                .path
                .strip_prefix(&root)
                .unwrap_or(&found.path)
                .display();
            let style = if index == selected {
                Style::default().fg(ACTIVE).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(TEXT)
            };
            Line::from(Span::styled(
                format!(
                    "{}:{}:{}  {}",
                    path,
                    found.line + 1,
                    found.column + 1,
                    found.text
                ),
                style,
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), sections[1]);
    frame.render_widget(
        Paragraph::new(format!(
            "{} results  |  Up/Down: navigate  Enter: open  Esc: close",
            search.matches.len()
        ))
        .style(Style::default().fg(MUTED)),
        sections[2],
    );
}

/// One area of the Help: its title and colour, and its keys with what they do. An empty key
/// carries the line above on.
struct HelpSection {
    title: &'static str,
    color: Color,
    keys: Vec<(&'static str, &'static str)>,
}

/// The Help for `os` (`std::env::consts::OS`): the keys that reach SozoIDE differ per system.
fn help_sections(os: &str) -> Vec<HelpSection> {
    let mac = os == "macos";
    let key = |label| key_for(os, label);
    let mut panels = vec![
        ("Ctrl+B", "Explorer"),
        ("Ctrl+J", "Bottom terminal"),
        (key("Ctrl+Shift+B"), "Right terminal"),
    ];
    if mac {
        panels.push((
            "",
            "with Option as Meta: Terminal > Settings > Profiles > Keyboard, or iTerm2 > \
             Profiles > Keys > Left Option key: Esc+",
        ));
    }
    panels.extend([
        ("Ctrl+G", "Source Control"),
        ("Ctrl+D", "Database"),
        ("Drag a divider", "Resize the panels"),
        ("Mouse wheel", "Scroll a panel or terminal history"),
        (
            if mac {
                "fn+Up/fn+Down"
            } else {
                "PageUp/PageDown"
            },
            "Scroll the editor or terminal",
        ),
    ]);
    vec![
        HelpSection {
            title: "Panels",
            color: Color::Cyan,
            keys: panels,
        },
        HelpSection {
            title: "Files and tabs",
            color: Color::Blue,
            keys: vec![
                ("Enter", "Open a file or a folder"),
                ("Ctrl+W", "Close the tab"),
                ("Wheel on the tabs", "Scroll the tabs, or click < and >"),
                ("Drag a tab", "To an editor edge: split the editor"),
                ("", "To the other pane: move it there"),
            ],
        },
        HelpSection {
            title: "Editing",
            color: Color::Green,
            keys: vec![
                ("Type or paste", "Edit the file; it saves by itself"),
                (
                    if mac {
                        "Arrows/fn+Arrows"
                    } else {
                        "Arrows/Home/End"
                    },
                    "Move the cursor",
                ),
                ("Shift+arrows", "Select, or drag with the mouse"),
                ("Ctrl+A", "Select all"),
                ("Ctrl+C / Ctrl+X", "Copy / cut the selection"),
                (
                    if mac {
                        "Delete/fn+Delete"
                    } else {
                        "Backspace/Delete"
                    },
                    "Delete before / after the cursor",
                ),
                ("Shift+wheel", "Scroll sideways"),
                ("Autosave", "After 2 idle seconds or a focus change"),
            ],
        },
        HelpSection {
            title: "Search",
            color: Color::Yellow,
            keys: vec![
                ("Ctrl+P", "Find in the file"),
                ("Ctrl+Shift+P", "Find in the workspace"),
                ("Esc", "Close a dialog or a panel"),
            ],
        },
        HelpSection {
            title: "PDF",
            color: Color::Red,
            keys: vec![
                (
                    if mac {
                        "fn+Down / Right"
                    } else {
                        "PageDown / Right"
                    },
                    "Next page, or the wheel",
                ),
                (
                    if mac { "fn+Up / Left" } else { "PageUp / Left" },
                    "Previous page",
                ),
                (
                    if mac {
                        "fn+Left / fn+Right"
                    } else {
                        "Home / End"
                    },
                    "First / last page",
                ),
            ],
        },
        HelpSection {
            title: "Source Control",
            color: Color::Magenta,
            keys: vec![
                ("Tab", "Next tab: Changes, Graph, Branches..."),
                ("Enter", "Diff, inspect, or switch branch"),
                ("Click a branch", "Switch to it, after asking"),
                ("s / u / c", "Stage, unstage, commit"),
                ("n / p", "Next / previous hunk in the Diff"),
                ("[ / ]", "Previous / next changed file"),
                ("v", "Side by side or unified diff"),
                ("o", "Open the file in the editor"),
            ],
        },
        HelpSection {
            title: "Database",
            color: Color::LightBlue,
            keys: vec![
                ("Ctrl+D", "Connections, query and results"),
                (
                    if mac {
                        "Ctrl+Enter/fn+F5"
                    } else {
                        "Ctrl+Enter/F5"
                    },
                    "Run the query",
                ),
            ],
        },
        HelpSection {
            title: "Updates",
            color: Color::LightGreen,
            keys: vec![
                (
                    "Enter / U",
                    match os {
                        "macos" => "Install it (replaces SozoIDE.app)",
                        "windows" => "Install it (runs the MSI installer)",
                        _ => "Install it (DEB/RPM ask for sudo)",
                    },
                ),
                ("S", "Skip that version"),
                ("Esc", "Later, or cancel the download"),
            ],
        },
        HelpSection {
            title: "Application",
            color: Color::LightMagenta,
            keys: vec![
                (key("F1"), "Help"),
                (key("F2"), "About"),
                ("G (in About)", "Open the repository to contribute"),
                ("Ctrl+Q", "Quit"),
            ],
        },
    ]
}

/// A section in a box of `width` columns: the title and the keys in the section colour, what
/// they do in the text colour.
fn help_box(section: &HelpSection, width: usize) -> Vec<Line<'static>> {
    let border = Style::default().fg(BORDER);
    let accent = Style::default()
        .fg(section.color)
        .add_modifier(Modifier::BOLD);
    let inner = width.saturating_sub(4);
    let keys = section
        .keys
        .iter()
        .map(|(key, _)| Line::from(*key).width())
        .max()
        .unwrap_or(0);
    let title = Line::from(section.title).width();
    let mut lines = vec![Line::from(vec![
        Span::styled("╭─ ", border),
        Span::styled(section.title, accent),
        Span::styled(
            format!(" {}╮", "─".repeat(width.saturating_sub(title + 5))),
            border,
        ),
    ])];
    let room = inner.saturating_sub(keys + 2).max(1);
    for (key, description) in &section.keys {
        // Long descriptions go on under themselves, with the key column empty.
        let mut parts = vec![String::new()];
        for word in description.split_whitespace() {
            let part = parts.last_mut().unwrap();
            if !part.is_empty() && Line::from(part.as_str()).width() + 1 + word.len() > room {
                parts.push(word.to_owned());
            } else {
                if !part.is_empty() {
                    part.push(' ');
                }
                part.push_str(word);
            }
        }
        for (index, part) in parts.into_iter().enumerate() {
            let key = if index == 0 { *key } else { "" };
            let fill = room.saturating_sub(Line::from(part.as_str()).width());
            lines.push(Line::from(vec![
                Span::styled("│ ", border),
                Span::styled(format!("{key:<keys$}"), accent),
                Span::raw(format!("  {part}{}", " ".repeat(fill))),
                Span::styled(" │", border),
            ]));
        }
    }
    lines.push(Line::styled(
        format!("╰{}╯", "─".repeat(width.saturating_sub(2))),
        border,
    ));
    lines
}

/// The Help sections in boxes, in two columns of `width` when `two` fit, the shorter column
/// taking the next box.
fn help_page(os: &str, width: usize, two: bool) -> Vec<Line<'static>> {
    let boxes = help_sections(os)
        .iter()
        .map(|section| help_box(section, width))
        .collect::<Vec<_>>();
    if !two {
        return boxes.into_iter().flatten().collect();
    }
    let total = boxes.iter().map(Vec::len).sum::<usize>();
    let mut columns: [Vec<Line<'static>>; 2] = [Vec::new(), Vec::new()];
    for lines in boxes {
        // Left until it holds half the lines, then right, keeping the order of the areas.
        let column = usize::from(columns[0].len() >= total / 2);
        columns[column].extend(lines);
    }
    let [left, right] = columns;
    let height = left.len().max(right.len());
    let blank = || Line::from(" ".repeat(width));
    (0..height)
        .map(|row| {
            let mut spans = left.get(row).cloned().unwrap_or_else(blank).spans;
            spans.push(Span::raw(" "));
            spans.extend(right.get(row).cloned().unwrap_or_else(blank).spans);
            Line::from(spans)
        })
        .collect()
}

pub(super) fn render_help(frame: &mut Frame, app: &mut App, area: Rect) {
    let os = std::env::consts::OS;
    const BOX: usize = 52;
    let two = usize::from(area.width) >= BOX * 2 + 1 + 6;
    let width = if two {
        BOX
    } else {
        usize::from(area.width.saturating_sub(6)).clamp(20, BOX)
    };
    let page = help_page(os, width, two);
    let content = if two { width * 2 + 1 } else { width };
    let dialog = dialog_area(area, content as u16 + 4, page.len() as u16 + 4);
    let visible = dialog.height.saturating_sub(3);
    let most = (page.len() as u16).saturating_sub(visible);
    app.overlay_scroll = app.overlay_scroll.min(most);
    let scroll = app.overlay_scroll;
    let progress = app.motion.overlay();
    render_dialog(
        frame,
        dialog,
        progress,
        format!(" Help - all commands ({}) ", os_name(os)),
        |frame, inner| {
            let [body, hint] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            let body = Rect::new(
                body.x + 1,
                body.y,
                body.width.saturating_sub(1),
                body.height,
            );
            frame.render_widget(Paragraph::new(page).scroll((scroll, 0)), body);
            let position = if most > 0 {
                format!(
                    "   {}/{}",
                    scroll + visible.min(body.height),
                    most + visible
                )
            } else {
                String::new()
            };
            let pages = if os == "macos" {
                "fn+Up/fn+Down"
            } else {
                "PageUp/PageDown"
            };
            frame.render_widget(
                Paragraph::new(format!(
                    " Up/Down, {pages} or the wheel: scroll   Esc: close{position}"
                ))
                .style(Style::default().fg(MUTED)),
                hint,
            );
        },
    );
}

fn os_name(os: &str) -> &'static str {
    match os {
        "macos" => "macOS",
        "windows" => "Windows",
        _ => "Linux",
    }
}

/// The logo `art` grown up to `progress` (0 to 1): characters appear by their distance from
/// the centre of the flower, and the newest ones sprout as `.` first (`DESIGN.md` §1.3).
fn bloom(art: &[&str], progress: f32) -> Vec<String> {
    let center_x = art[0].chars().count() as f32 / 2.0;
    let center_y = art.len() as f32 / 2.0;
    // Terminal cells are about twice as tall as they are wide.
    let distance = |x: usize, y: usize| ((x as f32 - center_x) / 2.0).hypot(y as f32 - center_y);
    let farthest = art
        .iter()
        .enumerate()
        .flat_map(|(y, line)| line.chars().enumerate().map(move |(x, _)| distance(x, y)))
        .fold(0.0, f32::max);
    let radius = progress.clamp(0.0, 1.0) * (farthest + 2.0);
    art.iter()
        .enumerate()
        .map(|(y, line)| {
            line.chars()
                .enumerate()
                .map(|(x, character)| match distance(x, y) {
                    _ if character == ' ' => ' ',
                    reach if reach <= radius - 1.5 => character,
                    reach if reach <= radius => '.',
                    _ => ' ',
                })
                .collect()
        })
        .collect()
}

/// About: the flower in the middle, half as big again when the terminal has room for it, and
/// what SozoIDE is under it.
pub(super) fn render_about(frame: &mut Frame, app: &mut App, area: Rect) {
    let button = " Contribute on GitHub [G] ";
    let tagline = "A terminal-native integrated development environment";
    let mut info = vec![
        Line::from(""),
        Line::from(Span::styled(
            branding::NAME,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Version {}", branding::VERSION)),
        Line::from(format!("Created by {}", branding::AUTHOR)),
        Line::from(tagline),
        Line::from(""),
        Line::from(Span::styled(
            button,
            Style::default()
                .fg(ACTIVE)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(MUTED),
        )),
    ];
    // The dialog takes a row over the flower, the borders and a spare row, and keeps one row
    // free above and below it.
    let fits = |art: &[&str]| {
        usize::from(area.width) >= art[0].len().max(tagline.len()) + 8
            && usize::from(area.height) >= art.len() + info.len() + 6
    };
    let art = if fits(ASCII_LOGO_LARGE) {
        ASCII_LOGO_LARGE
    } else {
        ASCII_LOGO
    };
    let mut lines = bloom(art, app.motion.bloom())
        .into_iter()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(ACTIVE))))
        .collect::<Vec<_>>();
    // One row above the flower, as below the hint.
    lines.insert(0, Line::from(""));
    let button_row = (lines.len() + info.len() - 3) as u16;
    lines.append(&mut info);
    let width = art[0].len().max(tagline.len()) as u16 + 6;
    let dialog = dialog_area(area, width, lines.len() as u16 + 3);
    let progress = app.motion.overlay();
    render_dialog(
        frame,
        dialog,
        progress,
        " About SozoIDE ".to_string(),
        |frame, inner| {
            frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
            let width = (button.len() as u16).min(inner.width);
            app.regions.about_repository = if button_row < inner.height {
                Rect::new(
                    inner.x + (inner.width - width) / 2,
                    inner.y + button_row,
                    width,
                    1,
                )
            } else {
                Rect::default()
            };
        },
    );
}

/// The tutorial pages for `os` (`std::env::consts::OS`): what each panel does and its key.
fn tutorial_steps(os: &str) -> [(&'static str, String); TUTORIAL_STEPS] {
    let key = |label| key_for(os, label);
    [
        (
            "Welcome to SozoIDE",
            "An IDE that lives in your terminal: files, editor, two shells, Git and SQL side by \
             side. This tour shows where everything is and takes a minute. SozoIDE counted this \
             installation once, anonymously (system, architecture and version only); set \
             SOZO_NO_TELEMETRY=1 to turn that off."
                .to_string(),
        ),
        (
            "Explorer and editor",
            format!(
                "{} shows or hides the Explorer. Enter opens a file in a tab and the editor saves \
                 by itself 2 seconds after you stop typing. Drag a tab to an edge of the editor \
                 to split it; each side keeps its own tabs.",
                key("Ctrl+B")
            ),
        ),
        (
            "Terminals",
            format!(
                "{} opens the bottom terminal and {} the right one. Drag the dividers to resize \
                 the panels; the mouse wheel scrolls the history.",
                key("Ctrl+J"),
                key("Ctrl+Shift+B")
            ),
        ),
        (
            "Source Control",
            format!(
                "{} opens changes, diffs, the commit graph, branches and history. Stage, commit, \
                 pull and push are one click away, and risky steps ask first.",
                key("Ctrl+G")
            ),
        ),
        (
            "Database",
            format!(
                "{} connects to SQLite, PostgreSQL or MySQL. Write a query and run it with \
                 {} or {}.",
                key("Ctrl+D"),
                key("Ctrl+Enter"),
                key("F5")
            ),
        ),
        (
            "Search and help",
            format!(
                "{} finds in the open file and {} in the whole folder. {} lists every command, \
                 {} shows About and {} quits.",
                key("Ctrl+P"),
                key("Ctrl+Shift+P"),
                key("F1"),
                key("F2"),
                key("Ctrl+Q")
            ),
        ),
    ]
}

/// The first start (F17): one page at a time, with Next and Skip always in reach.
pub(super) fn render_tutorial(frame: &mut Frame, app: &mut App, area: Rect) {
    let steps = tutorial_steps(std::env::consts::OS);
    let step = app.tutorial_step.min(TUTORIAL_STEPS - 1);
    let (title, text) = &steps[step];
    let last = step + 1 == TUTORIAL_STEPS;
    let next = if last {
        " Start [Enter] "
    } else {
        " Next [Enter] "
    };
    let skip = " Skip tutorial [Esc] ";
    let area = dialog_area(area, 66, 13);
    let progress = app.motion.overlay();
    render_dialog(
        frame,
        area,
        progress,
        format!(" Tutorial {}/{} ", step + 1, TUTORIAL_STEPS),
        |frame, inner| {
            let rows = Layout::vertical([
                Constraint::Length(2),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    *title,
                    Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
                )),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(text.as_str()).wrap(Wrap { trim: true }),
                rows[1],
            );
            let (next_area, skip_area) = buttons(frame, rows[2], next, skip);
            app.regions.dialog_next = next_area;
            app.regions.dialog_skip = skip_area;
            let dots = (0..TUTORIAL_STEPS)
                .map(|index| if index == step { "●" } else { "○" })
                .collect::<Vec<_>>()
                .join(" ");
            frame.render_widget(
                Paragraph::new(format!("{dots}   Left: back")).style(Style::default().fg(MUTED)),
                rows[3],
            );
        },
    );
}

/// After an update (F18): what each version since the one that ran before brings and fixes,
/// newest first; the list scrolls when it does not fit.
pub(super) fn render_whats_new(frame: &mut Frame, app: &mut App, area: Rect) {
    let notes = if app.notes.is_empty() {
        vec![&NOTES[0]]
    } else {
        app.notes.clone()
    };
    let mut lines = vec![Line::from(Span::styled(
        format!("SozoIDE is now on version {}", branding::VERSION),
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if notes.len() > 1 {
        lines.push(Line::from(Span::styled(
            format!("{} versions since the one you had", notes.len()),
            Style::default().fg(MUTED),
        )));
    }
    for version in &notes {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                format!("Version {}", version.version),
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            )),
        ]);
        for (title, items, color) in [
            ("New", version.new, Color::Green),
            ("Fixed", version.fixed, Color::Yellow),
        ] {
            if items.is_empty() {
                continue;
            }
            lines.push(Line::from(Span::styled(
                format!("  {title}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            lines.extend(items.iter().map(|item| Line::from(format!("    - {item}"))));
        }
    }
    // Long notes wrap: count the rows they take inside the borders, with one to spare for
    // words that wrap early.
    let inner = usize::from(80.min(area.width.saturating_sub(4)).saturating_sub(2)).max(1);
    let rows = lines
        .iter()
        .map(|line| match line.width() {
            width if width <= inner => 1,
            width => width.div_ceil(inner) + 1,
        })
        .sum::<usize>() as u16;
    let dialog = dialog_area(area, 80, rows + 4);
    let visible = dialog.height.saturating_sub(4);
    let most = rows.saturating_sub(visible);
    app.overlay_scroll = app.overlay_scroll.min(most);
    let scroll = app.overlay_scroll;
    let close = " Close [Enter] ";
    let progress = app.motion.overlay();
    render_dialog(
        frame,
        dialog,
        progress,
        " What's new ".to_string(),
        |frame, inner| {
            let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0)),
                rows[0],
            );
            let (close_area, _) = buttons(frame, rows[1], close, "");
            app.regions.dialog_next = close_area;
            if most > 0 {
                let hint = Rect::new(
                    close_area.right() + 2,
                    rows[1].y,
                    rows[1].width.saturating_sub(close_area.width + 2),
                    1,
                );
                frame.render_widget(
                    Paragraph::new("Up/Down or the wheel: more").style(Style::default().fg(MUTED)),
                    hint,
                );
            }
        },
    );
}

/// A dialog of `width` × `height` cells in the middle of `area`, smaller if it does not fit.
fn dialog_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// The main button and, if there is one, the second; returns where they were drawn.
fn buttons(frame: &mut Frame, row: Rect, main: &str, second: &str) -> (Rect, Rect) {
    let parts = Layout::horizontal([
        Constraint::Length(main.len() as u16),
        Constraint::Length(2),
        Constraint::Length(second.len() as u16),
    ])
    .split(row);
    frame.render_widget(
        Paragraph::new(main.to_string()).style(
            Style::default()
                .fg(ACTIVE)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        parts[0],
    );
    if !second.is_empty() {
        frame.render_widget(
            Paragraph::new(second.to_string())
                .style(Style::default().fg(TEXT).add_modifier(Modifier::REVERSED)),
            parts[2],
        );
    }
    (
        parts[0],
        if second.is_empty() {
            Rect::default()
        } else {
            parts[2]
        },
    )
}

pub(super) fn render_update(frame: &mut Frame, app: &mut App, area: Rect, progress: f32) {
    let width = area.width.saturating_sub(4).min(64);
    let height = area.height.min(9);
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help_text(os: &str) -> String {
        help_sections(os)
            .iter()
            .flat_map(|section| {
                std::iter::once(section.title.to_owned()).chain(
                    section
                        .keys
                        .iter()
                        .map(|(key, description)| format!("{key} = {description}")),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn help_adapts_to_each_os() {
        let mac = help_text("macos");
        assert!(mac.contains("Ctrl+Option+B = Right terminal"));
        assert!(mac.contains("Option as Meta"));
        assert!(mac.contains("fn+F1 = Help"));
        assert!(mac.contains("fn+F5"));
        assert!(mac.contains("replaces SozoIDE.app"));
        assert!(!mac.contains("Ctrl+Shift+B"));

        for (os, installer, right) in [
            ("windows", "MSI installer", "Ctrl+Shift+B = Right terminal"),
            ("linux", "sudo", "Ctrl+Alt+B = Right terminal"),
        ] {
            let text = help_text(os);
            assert!(text.contains(right), "{os}");
            assert!(text.contains("F1 = Help"));
            assert!(text.contains("Ctrl+Enter/F5"));
            assert!(text.contains(installer));
            assert!(!text.contains("Option"));
            assert!(!text.contains("fn+"));
        }
        assert_eq!(os_name("windows"), "Windows");
        for section in help_sections("linux") {
            assert!(
                !matches!(
                    section.color,
                    Color::Rgb(..) | Color::Indexed(_) | Color::Reset
                ),
                "{} uses a colour of the terminal theme",
                section.title
            );
        }
    }

    #[test]
    fn help_boxes_line_up_in_two_columns() {
        let page = help_page("linux", 52, true);
        let widths = page.iter().map(Line::width).collect::<Vec<_>>();
        assert!(
            widths.iter().all(|width| *width == 52 * 2 + 1),
            "{widths:?}"
        );
        let text = page
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(text[0].starts_with("╭─ Panels "), "{}", text[0]);
        assert!(text.iter().any(|line| line.contains("╭─ Source Control ")));
        let single = help_page("linux", 40, false);
        assert!(single.len() > page.len());
        assert!(single.iter().all(|line| line.width() == 40));
    }

    #[test]
    fn the_flower_grows_from_its_centre() {
        for art in [ASCII_LOGO, ASCII_LOGO_LARGE] {
            assert!(bloom(art, 0.0).iter().all(|line| line.trim().is_empty()));
            assert_eq!(bloom(art, 1.0), art);
            let half = bloom(art, 0.45);
            let middle = &half[art.len() / 2];
            assert!(middle.contains('@'), "{middle}");
            assert!(half[0].trim().is_empty(), "the top petal is not out yet");
            assert!(
                half.iter().any(|line| line.contains('.')),
                "sprouts at the edge"
            );
        }
        // The large flower is the same flower: the same stamens and as many seeds.
        let count = |art: &[&str], character| {
            art.iter()
                .map(|line| line.matches(character).count())
                .sum::<usize>()
        };
        assert_eq!(count(ASCII_LOGO, 'o'), count(ASCII_LOGO_LARGE, 'o'));
        assert_eq!(count(ASCII_LOGO, '@'), count(ASCII_LOGO_LARGE, '@'));
        assert!(
            ASCII_LOGO_LARGE
                .iter()
                .all(|line| line.len() == ASCII_LOGO_LARGE[0].len())
        );
        assert_eq!(ASCII_LOGO_LARGE.len(), ASCII_LOGO.len() * 3 / 2);
    }

    #[test]
    fn the_tutorial_names_the_keys_of_each_system() {
        let mac = tutorial_steps("macos").map(|(_, text)| text).join("\n");
        assert!(mac.contains("Ctrl+Option+B") && mac.contains("fn+F1"));
        let linux = tutorial_steps("linux").map(|(_, text)| text).join("\n");
        assert!(linux.contains("Ctrl+Alt+B") && !linux.contains("fn+"));
        let windows = tutorial_steps("windows").map(|(_, text)| text).join("\n");
        assert!(windows.contains("Ctrl+Shift+B"));
    }

    #[test]
    fn the_tutorial_says_what_was_counted() {
        let (_, welcome) = &tutorial_steps("linux")[0];
        assert!(welcome.contains("anonymously") && welcome.contains("SOZO_NO_TELEMETRY=1"));
    }
}
