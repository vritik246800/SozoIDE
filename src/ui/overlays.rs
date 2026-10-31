use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::{
    animation::spinner,
    app::{
        Action, App, AppMotion, Dependencies, HelpTab, KeyEditor, Keymap, NotesFilter,
        TUTORIAL_STEPS,
    },
    branding::{self, ASCII_LOGO, ASCII_LOGO_LARGE},
    editor::{MAX_MATCHES, SearchMatch, SearchScope, file_icon},
    update::{NOTES, Notes, UpdateState, deps::Found},
};

use super::{
    ACTIVE, BORDER, MATCH, MUTED, TEXT, bar, busy, centered, components::key_for, panel_block,
    render_dialog, scale_length, sweep_bar, unfold,
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

/// The search (F05): Ctrl+P is a bar over the bottom of the editor, which stays in view with
/// every match marked; Ctrl+Shift+P a dialog with the results grouped by file.
pub(super) fn render_search(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(search) = &app.search else {
        return;
    };
    let progress = app.motion.search();
    if search.scope == SearchScope::CurrentFile {
        // Over the editor, or over the screen when a panel hides it.
        let editor = if app.regions.editor_body.is_empty() {
            area
        } else {
            app.regions.editor_body
        };
        // At the bottom right, away from the match, which scrolls to near the top.
        let (width, height) = (52.min(editor.width), 3.min(editor.height));
        let bar = Rect::new(
            editor.right() - width,
            editor.bottom() - height,
            width,
            height,
        );
        let visible = unfold(bar, progress);
        if visible.is_empty() {
            return;
        }
        frame.render_widget(Clear, visible);
        let label = app.keys.label(Action::FindInFile);
        let block = panel_block(format!(" Find in file [{label}] "), true);
        let inner = block.inner(bar).intersection(visible);
        frame.render_widget(block, visible);
        app.regions.search_match_items.clear();
        app.regions.search_results = Rect::default();
        search_header(frame, app, inner);
        return;
    }
    // References and definitions (F31) are a list with its own title.
    let title = match &search.title {
        Some(title) => format!(" {title} "),
        None => format!(
            " Find in workspace [{}] ",
            app.keys.label(Action::FindInWorkspace)
        ),
    };
    render_dialog(
        frame,
        centered(area, 84, 70),
        progress,
        title,
        |frame, inner| render_search_results(frame, app, inner),
    );
}

/// The query on the left; on the right the spinner while the workspace is searched and the
/// counter `◂ 3/12 ▸`, whose arrows choose the previous and the next result.
fn search_header(frame: &mut Frame, app: &mut App, area: Rect) {
    app.regions.search_input = area;
    app.regions.search_previous = Rect::default();
    app.regions.search_next = Rect::default();
    let Some(search) = &app.search else {
        return;
    };
    let total = search.matches.len();
    let counter = if total == 0 {
        if search.query.is_empty() {
            String::new()
        } else {
            "No results".to_owned()
        }
    } else {
        format!(
            "{}/{}{}",
            search.selected + 1,
            total,
            if total >= MAX_MATCHES { "+" } else { "" }
        )
    };
    let spinner = if search.searching {
        format!("{} ", spinner(app.motion.elapsed()))
    } else {
        String::new()
    };
    let arrows = total > 0;
    let right = spinner.chars().count() as u16
        + counter.chars().count() as u16
        + if arrows { 4 } else { 0 };
    let right = right.min(area.width);
    let [query, counter_area] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(right + 1)]).areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(ACTIVE)),
            Span::styled(search.query.clone(), Style::default().fg(TEXT)),
            // A list given whole takes no typing: no cursor.
            Span::styled(
                if search.title.is_some() { "" } else { "█" },
                Style::default().fg(ACTIVE),
            ),
        ])),
        query,
    );
    let mut spans = vec![Span::styled(spinner, Style::default().fg(ACTIVE))];
    let x = counter_area.x + 1 + spans[0].content.chars().count() as u16;
    if arrows {
        spans.push(Span::styled("◂ ", Style::default().fg(ACTIVE)));
        app.regions.search_previous = Rect::new(x, counter_area.y, 2, 1);
    }
    let count_style = if total == 0 { MUTED } else { TEXT };
    spans.push(Span::styled(
        counter.clone(),
        Style::default().fg(count_style),
    ));
    if arrows {
        spans.push(Span::styled(" ▸", Style::default().fg(ACTIVE)));
        let next = x + 2 + counter.chars().count() as u16;
        app.regions.search_next = Rect::new(next, counter_area.y, 2, 1);
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Right),
        counter_area,
    );
}

/// The rows of the workspace results: a header for each file, then its matches.
enum ResultRow {
    File(usize, usize),
    Match(usize),
}

fn render_search_results(frame: &mut Frame, app: &mut App, inner: Rect) {
    if inner.height < 4 {
        return;
    }
    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    search_header(frame, app, sections[0]);
    let root = app.root.clone();
    let Some(search) = &mut app.search else {
        return;
    };
    let summary = match (search.matches.len(), search.files()) {
        (0, _) if search.searching => "Searching...".to_owned(),
        (0, _) => String::new(),
        (results, files) => format!(
            "{results}{} result{} in {files} file{}",
            if results >= MAX_MATCHES { "+" } else { "" },
            if results == 1 { "" } else { "s" },
            if files == 1 { "" } else { "s" },
        ),
    };
    frame.render_widget(
        Paragraph::new(summary).style(Style::default().fg(MUTED)),
        sections[1],
    );
    let mut rows = Vec::new();
    for (index, found) in search.matches.iter().enumerate() {
        if index == 0 || search.matches[index - 1].path != found.path {
            let count = search.matches[index..]
                .iter()
                .take_while(|other| other.path == found.path)
                .count();
            rows.push(ResultRow::File(index, count));
        }
        rows.push(ResultRow::Match(index));
    }
    let list = sections[2];
    app.regions.search_results = list;
    let height = list.height as usize;
    let selected = search.selected;
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, ResultRow::Match(index) if *index == selected))
        .unwrap_or(0);
    // The file of the selected match stays in view above it when it fits.
    let wanted = selected_row.saturating_sub(1);
    if wanted < search.scroll {
        search.scroll = wanted;
    } else if selected_row >= search.scroll + height {
        search.scroll = selected_row + 1 - height;
    }
    app.regions.search_match_items.clear();
    let width = list.width as usize;
    let lines = rows
        .iter()
        .enumerate()
        .skip(search.scroll)
        .take(height)
        .map(|(row, entry)| match entry {
            ResultRow::File(first, count) => {
                let path = &search.matches[*first].path;
                let (icon, color) = file_icon(path);
                let shown = path.strip_prefix(&root).unwrap_or(path).display();
                Line::from(vec![
                    Span::styled(format!("{icon} "), Style::default().fg(color)),
                    Span::styled(
                        shown.to_string(),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" ({count})"), Style::default().fg(MUTED)),
                ])
            }
            ResultRow::Match(index) => {
                let y = list.y + (row - search.scroll) as u16;
                app.regions
                    .search_match_items
                    .push((*index, Rect::new(list.x, y, list.width, 1)));
                match_line(&search.matches[*index], *index == selected, width)
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), list);
    frame.render_widget(
        Paragraph::new("Up/Down or ◂/▸: results  Enter: open  Esc: close")
            .style(Style::default().fg(MUTED)),
        sections[3],
    );
}

/// A result: its line number, then the line around the match with the match marked; the
/// selected one has a `›` and the colour of the focus.
pub(super) fn match_line(found: &SearchMatch, selected: bool, width: usize) -> Line<'static> {
    let characters = found.text.chars().collect::<Vec<_>>();
    let number = format!("{:>5}  ", found.line + 1);
    let room = width.saturating_sub(number.chars().count() + 2);
    // Leading spaces go, and a match far to the right keeps some text before it.
    let indent = characters
        .iter()
        .take_while(|character| character.is_whitespace())
        .count();
    let start = indent
        .max(found.column.saturating_sub(room / 3))
        .min(found.column);
    let cut = |from: usize, to: usize| {
        characters[from.min(characters.len())..to.min(characters.len())]
            .iter()
            .collect::<String>()
    };
    let end = found.column + found.length;
    let text_style = if selected {
        Style::default().fg(ACTIVE)
    } else {
        Style::default().fg(TEXT)
    };
    let mut spans = vec![
        Span::styled(
            if selected { " › " } else { "   " },
            Style::default().fg(ACTIVE),
        ),
        Span::styled(number, Style::default().fg(MUTED)),
    ];
    if start > indent {
        spans.push(Span::styled("…", Style::default().fg(MUTED)));
    }
    spans.push(Span::styled(cut(start, found.column), text_style));
    spans.push(Span::styled(cut(found.column, end), MATCH));
    spans.push(Span::styled(cut(end, start + room), text_style));
    Line::from(spans)
}

/// A key in Help: written as it is, or the key a command has now, which a click changes (F29).
#[derive(Clone, Copy)]
enum HelpKey {
    Fixed(&'static str),
    Bound(Action),
}

/// One area of the Help: its title and colour, and its keys with what they do. An empty key
/// carries the line above on.
struct HelpSection {
    title: &'static str,
    color: Color,
    keys: Vec<(HelpKey, &'static str)>,
}

/// A command's key with what it does.
fn bound(action: Action) -> (HelpKey, &'static str) {
    (HelpKey::Bound(action), action.title())
}

/// The Help for `os` (`std::env::consts::OS`): the keys that reach SozoIDE differ per system.
fn help_sections(os: &str) -> Vec<HelpSection> {
    use HelpKey::Fixed;
    let mac = os == "macos";
    let mut panels = vec![
        bound(Action::Explorer),
        bound(Action::BottomTerminal),
        bound(Action::RightTerminal),
    ];
    if mac {
        panels.push((
            Fixed(""),
            "with Option as Meta: Terminal > Settings > Profiles > Keyboard, or iTerm2 > \
             Profiles > Keys > Left Option key: Esc+",
        ));
    }
    panels.extend([
        bound(Action::SourceControl),
        bound(Action::Database),
        (Fixed("Drag a divider"), "Resize the panels"),
        (Fixed("Mouse wheel"), "Scroll a panel or terminal history"),
        (
            Fixed(if mac {
                "fn+Up/fn+Down"
            } else {
                "PageUp/PageDown"
            }),
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
            title: "Terminals",
            color: Color::LightYellow,
            keys: vec![
                bound(Action::NextTerminal),
                bound(Action::PreviousTerminal),
                (
                    Fixed("+ / x on the tabs"),
                    "New / close a bottom terminal (1 to 3)",
                ),
                (Fixed("Drag the mouse"), "Select text; letting go copies it"),
                (Fixed("Shift+Enter"), "New line without running it"),
            ],
        },
        HelpSection {
            title: "Files and tabs",
            color: Color::Blue,
            keys: vec![
                (Fixed("Enter"), "Open a file or a folder"),
                (
                    Fixed("Right-click"),
                    "New, rename, delete, properties (Explorer)",
                ),
                (Fixed("Ctrl+X/C/V"), "Cut, copy, paste (Explorer)"),
                bound(Action::CloseTab),
                bound(Action::Run),
                bound(Action::Build),
                bound(Action::Clean),
                bound(Action::Rebuild),
                bound(Action::RunProject),
                (
                    Fixed("Wheel on the tabs"),
                    "Scroll the tabs, or click < and >",
                ),
                (Fixed("Drag a tab"), "To an editor edge: split the editor"),
                (Fixed(""), "To the other pane: move it there"),
            ],
        },
        HelpSection {
            title: "Editing",
            color: Color::Green,
            keys: vec![
                (Fixed("Type or paste"), "Edit the file; it saves by itself"),
                (
                    Fixed(if mac {
                        "Arrows/fn+Arrows"
                    } else {
                        "Arrows/Home/End"
                    }),
                    "Move the cursor",
                ),
                (Fixed("Shift+arrows"), "Select, or drag with the mouse"),
                (Fixed("Ctrl+A"), "Select all"),
                (Fixed("Ctrl+Z"), "Undo the last edit"),
                (Fixed("Ctrl+C / Ctrl+X"), "Copy / cut the selection"),
                (Fixed("Ctrl+V"), "Paste"),
                bound(Action::Suggestions),
                bound(Action::Definition),
                bound(Action::Peek),
                bound(Action::References),
                bound(Action::Hover),
                (Fixed("Rest the mouse"), "On a word: the same information"),
                (Fixed("Ctrl+click"), "Go to the definition"),
                bound(Action::Back),
                (Fixed("Right-click"), "Cut, copy, paste the text"),
                (
                    Fixed(if mac {
                        "Delete/fn+Delete"
                    } else {
                        "Backspace/Delete"
                    }),
                    "Delete before / after the cursor",
                ),
                (Fixed("Shift+wheel"), "Scroll sideways (editor, Explorer)"),
                (Fixed("Autosave"), "After 2 idle seconds or a focus change"),
            ],
        },
        HelpSection {
            title: "Search",
            color: Color::Yellow,
            keys: vec![
                bound(Action::FindInFile),
                bound(Action::FindInWorkspace),
                (Fixed("Up/Down or < >"), "Previous / next result"),
                (Fixed("Enter"), "Open the result, selected"),
                (Fixed("Esc"), "Close a dialog or a panel"),
            ],
        },
        HelpSection {
            title: "PDF",
            color: Color::Red,
            keys: vec![
                (
                    Fixed(if mac {
                        "fn+Down / Right"
                    } else {
                        "PageDown / Right"
                    }),
                    "Next page, or the wheel",
                ),
                (
                    Fixed(if mac { "fn+Up / Left" } else { "PageUp / Left" }),
                    "Previous page",
                ),
                (
                    Fixed(if mac {
                        "fn+Left / fn+Right"
                    } else {
                        "Home / End"
                    }),
                    "First / last page",
                ),
                (Fixed("Shift+arrows"), "Select in a page of text"),
            ],
        },
        HelpSection {
            title: "Debug",
            color: Color::LightRed,
            keys: vec![
                bound(Action::Breakpoint),
                bound(Action::Debug),
                bound(Action::StepOver),
                bound(Action::StepInto),
                bound(Action::StopDebug),
            ],
        },
        HelpSection {
            title: "3D models",
            color: Color::LightMagenta,
            keys: vec![
                (Fixed("Left / Right"), "Turn the model"),
                (Fixed("Up / Down"), "Tilt the model"),
                (Fixed("+ / - or wheel"), "Zoom in / out"),
                (Fixed("Drag"), "Turn it with the mouse"),
            ],
        },
        HelpSection {
            title: "Videos",
            color: Color::LightCyan,
            keys: vec![
                (Fixed("Space or click"), "Play / pause"),
                (Fixed("S"), "Stop at the start"),
                (Fixed("Left / Right"), "Back / forward 5 s"),
                (Fixed("Shift+Left / Right"), "Back / forward 30 s"),
                (Fixed("- / + or wheel"), "Volume down / up"),
                (Fixed("M"), "Mute"),
                (Fixed("[ / ]"), "Slower / faster"),
                (Fixed("F"), "Full screen"),
                (Fixed("Esc"), "Leave full screen, close"),
                (Fixed("Click the bar"), "Go to that time"),
            ],
        },
        HelpSection {
            title: "Source Control",
            color: Color::Magenta,
            keys: vec![
                (Fixed("Tab"), "Next tab: Changes, Graph, Branches..."),
                (Fixed("Enter"), "Diff, commit files, or switch branch"),
                (Fixed("Click a commit"), "Its files; Enter on one: its diff"),
                (Fixed("Esc"), "Back from the diff, the files"),
                (Fixed("Click a branch"), "Switch to it, after asking"),
                (Fixed("s / u / c"), "Stage, unstage, commit"),
                (Fixed("n / p"), "Next / previous hunk in the Diff"),
                (Fixed("[ / ]"), "Previous / next changed file"),
                (Fixed("v"), "Side by side or unified diff"),
                (Fixed("o"), "Open the file in the editor"),
            ],
        },
        HelpSection {
            title: "Database",
            color: Color::LightBlue,
            keys: vec![
                (
                    HelpKey::Bound(Action::Database),
                    "Connections, then tables and views",
                ),
                (Fixed("Enter / Space"), "Add a table, tick a column, change"),
                (Fixed(""), "a cell of the builder"),
                (
                    Fixed(if mac { "fn+F6" } else { "F6" }),
                    "Tables, tab, results: next part",
                ),
                (Fixed("Delete"), "Remove the builder row"),
                (
                    Fixed(if mac {
                        "Ctrl+Enter/fn+F5"
                    } else {
                        "Ctrl+Enter/F5"
                    }),
                    "Run the query",
                ),
                (Fixed("Shift+wheel"), "Scroll the results sideways"),
            ],
        },
        HelpSection {
            title: "Updates",
            color: Color::LightGreen,
            keys: vec![
                (
                    Fixed("Enter / U"),
                    match os {
                        "macos" => "Install it (replaces SozoIDE.app)",
                        "windows" => "Install it (runs the MSI installer)",
                        _ => "Install it (DEB/RPM ask for sudo)",
                    },
                ),
                (Fixed("S"), "Skip that version"),
                (Fixed("Esc"), "Later, or cancel the download"),
            ],
        },
        HelpSection {
            title: "Application",
            color: Color::LightMagenta,
            keys: vec![
                bound(Action::Help),
                bound(Action::About),
                (Fixed("G (in About)"), "Open the repository to contribute"),
                (Fixed("U (in About)"), "Check for updates now"),
                (
                    Fixed("N (in About)"),
                    "Notes of every version; Tab: new or fixed",
                ),
                bound(Action::Quit),
            ],
        },
    ]
}

/// What the key column of a row shows: its text, the key its command has now, or the prompt
/// while Help waits for the new keys.
fn help_key(key: HelpKey, keys: &Keymap, editor: &KeyEditor) -> String {
    match key {
        HelpKey::Fixed(text) => text.to_string(),
        HelpKey::Bound(action) if editor.capturing && editor.selected == Some(action) => {
            "Press keys".to_string()
        }
        HelpKey::Bound(action) => keys.label(action),
    }
}

/// A section in a box of `width` columns: the title and the keys in the section colour, what
/// they do in the text colour; a command's key underlined (a click changes it), in reverse when
/// chosen. Each row comes with the command whose key it shows.
fn help_box(
    section: &HelpSection,
    width: usize,
    keys: &Keymap,
    editor: &KeyEditor,
) -> Vec<(Line<'static>, Option<Action>)> {
    let border = Style::default().fg(BORDER);
    let accent = Style::default()
        .fg(section.color)
        .add_modifier(Modifier::BOLD);
    let inner = width.saturating_sub(4);
    let labels = section
        .keys
        .iter()
        .map(|(key, _)| help_key(*key, keys, editor))
        .collect::<Vec<_>>();
    let column = labels
        .iter()
        .map(|label| Line::from(label.as_str()).width())
        .max()
        .unwrap_or(0);
    let title = Line::from(section.title).width();
    let mut lines = vec![(
        Line::from(vec![
            Span::styled("╭─ ", border),
            Span::styled(section.title, accent),
            Span::styled(
                format!(" {}╮", "─".repeat(width.saturating_sub(title + 5))),
                border,
            ),
        ]),
        None,
    )];
    let room = inner.saturating_sub(column + 2).max(1);
    for ((key, description), label) in section.keys.iter().zip(labels) {
        let action = match key {
            HelpKey::Bound(action) => Some(*action),
            HelpKey::Fixed(_) => None,
        };
        let style = match action {
            Some(action) if editor.capturing && editor.selected == Some(action) => Style::default()
                .fg(ACTIVE)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            Some(action) if editor.selected == Some(action) => {
                accent.add_modifier(Modifier::REVERSED)
            }
            Some(_) => accent.add_modifier(Modifier::UNDERLINED),
            None => accent,
        };
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
            let shown = if index == 0 { label.as_str() } else { "" };
            let pad = column.saturating_sub(Line::from(shown).width());
            let fill = room.saturating_sub(Line::from(part.as_str()).width());
            lines.push((
                Line::from(vec![
                    Span::styled("│ ", border),
                    Span::styled(shown.to_string(), style),
                    Span::raw(format!("{}  {part}{}", " ".repeat(pad), " ".repeat(fill))),
                    Span::styled(" │", border),
                ]),
                action.filter(|_| index == 0),
            ));
        }
    }
    lines.push((
        Line::styled(format!("╰{}╯", "─".repeat(width.saturating_sub(2))), border),
        None,
    ));
    lines
}

/// Where a command's key is in the Help page: line, column and width of its row.
type KeyPlace = (usize, u16, u16, Action);

/// The Help sections in boxes, in two columns of `width` when `two` fit, the shorter column
/// taking the next box; and where each command's key is: line, column and width of its row.
fn help_page(
    os: &str,
    width: usize,
    two: bool,
    keys: &Keymap,
    editor: &KeyEditor,
) -> (Vec<Line<'static>>, Vec<KeyPlace>) {
    let boxes = help_sections(os)
        .iter()
        .map(|section| help_box(section, width, keys, editor))
        .collect::<Vec<_>>();
    let mut places = Vec::new();
    if !two {
        let lines = boxes
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(row, (line, action))| {
                if let Some(action) = action {
                    places.push((row, 0, width as u16, action));
                }
                line
            });
        let lines = lines.collect();
        return (lines, places);
    }
    let total = boxes.iter().map(Vec::len).sum::<usize>();
    let mut columns: [Vec<(Line<'static>, Option<Action>)>; 2] = [Vec::new(), Vec::new()];
    for lines in boxes {
        // Left until it holds half the lines, then right, keeping the order of the areas.
        let column = usize::from(columns[0].len() >= total / 2);
        columns[column].extend(lines);
    }
    let [left, right] = columns;
    let height = left.len().max(right.len());
    let blank = || (Line::from(" ".repeat(width)), None);
    let lines = (0..height)
        .map(|row| {
            let (left, left_action) = left.get(row).cloned().unwrap_or_else(blank);
            let (right, right_action) = right.get(row).cloned().unwrap_or_else(blank);
            for (action, x) in [(left_action, 0), (right_action, width + 1)] {
                if let Some(action) = action {
                    places.push((row, x as u16, width as u16, action));
                }
            }
            let mut spans = left.spans;
            spans.push(Span::raw(" "));
            spans.extend(right.spans);
            Line::from(spans)
        })
        .collect();
    (lines, places)
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
    let (page, places) = help_page(os, width, two, &app.keys, &app.key_editor);
    let content = if two { width * 2 + 1 } else { width };
    let dialog = dialog_area(area, content as u16 + 4, page.len() as u16 + 6);
    let visible = dialog.height.saturating_sub(5);
    let most = (page.len() as u16).saturating_sub(visible);
    // Tab brings the chosen command into view.
    if std::mem::take(&mut app.key_editor.follow)
        && let Some(row) = places
            .iter()
            .find(|place| Some(place.3) == app.key_editor.selected)
            .map(|place| place.0 as u16)
    {
        if row < app.overlay_scroll {
            app.overlay_scroll = row;
        } else if row >= app.overlay_scroll + visible {
            app.overlay_scroll = row + 1 - visible;
        }
    }
    app.overlay_scroll = app.overlay_scroll.min(most);
    let scroll = app.overlay_scroll;
    let progress = app.motion.overlay();
    let editor = app.key_editor.clone();
    let mut keys = Vec::new();
    let mut reset = Rect::default();
    let tab = app.help_tab;
    let mut tabs = Vec::new();
    let mut rows = Vec::new();
    let dependencies = app
        .dependencies
        .as_ref()
        .map(|dependencies| dependency_table(dependencies, app.motion.elapsed()));
    render_dialog(
        frame,
        dialog,
        progress,
        format!(" Help - all commands ({}) ", os_name(os)),
        |frame, inner| {
            let [bar, body, edit, hint] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(inner);
            // Shortcuts and Dependencies, like the tabs of Source Control (F33).
            let mut x = bar.x + 1;
            for (each, label) in [
                (HelpTab::Shortcuts, " Shortcuts "),
                (HelpTab::Dependencies, " Dependencies "),
            ] {
                let style = if each == tab {
                    Style::default()
                        .fg(ACTIVE)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::default().fg(TEXT).add_modifier(Modifier::REVERSED)
                };
                let button = Rect::new(x, bar.y, label.len() as u16, 1).intersection(bar);
                frame.render_widget(Paragraph::new(label).style(style), button);
                tabs.push((each, button));
                x += label.len() as u16 + 1;
            }
            frame.render_widget(
                Paragraph::new(" Left/Right: tab").style(Style::default().fg(MUTED)),
                Rect::new(x, bar.y, bar.right().saturating_sub(x), 1).intersection(bar),
            );
            let body = Rect::new(
                body.x + 1,
                body.y,
                body.width.saturating_sub(1),
                body.height,
            );
            if tab == HelpTab::Dependencies {
                render_dependencies(frame, dependencies.as_ref(), body, &mut rows);
                frame.render_widget(
                    Paragraph::new(
                        " Up/Down or a click: choose   Enter: install or update   R: check again   Esc: close",
                    )
                    .style(Style::default().fg(MUTED)),
                    hint,
                );
                return;
            }
            frame.render_widget(Paragraph::new(page).scroll((scroll, 0)), body);
            for (row, x, width, action) in &places {
                let Some(y) = (*row as u16).checked_sub(scroll) else {
                    continue;
                };
                if y < body.height.saturating_sub(1) {
                    keys.push((*action, Rect::new(body.x + x, body.y + y, *width, 1)));
                }
            }
            // The row of the shortcuts (F29): Reset, then what Help waits for or did.
            let button = " Reset shortcuts [R] ";
            reset = Rect::new(edit.x + 1, edit.y, button.len() as u16, 1).intersection(edit);
            let (text, style) = if editor.confirm_reset {
                (
                    "Reset every shortcut to the default? Enter: reset   Esc: cancel".to_string(),
                    Style::default().fg(Color::Yellow),
                )
            } else if let Some(action) = editor.selected.filter(|_| editor.capturing) {
                (
                    format!("Press the new keys for {}   Esc: cancel", action.title()),
                    Style::default().fg(ACTIVE),
                )
            } else if let Some(message) = &editor.message {
                (message.clone(), Style::default().fg(TEXT))
            } else {
                (
                    "Click a key, or Tab and Enter, then press the new keys".to_string(),
                    Style::default().fg(MUTED),
                )
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        button,
                        Style::default().fg(TEXT).add_modifier(Modifier::REVERSED),
                    ),
                    Span::styled(format!("  {text}"), style),
                ])),
                edit,
            );
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
    app.regions.help_keys = keys;
    app.regions.help_reset = reset;
    app.regions.help_tabs = tabs;
    app.regions.dependency_rows = rows;
}

/// The Dependencies tab as the dialog draws it: the summary, the rows (component, kind,
/// installed, status and its colour, action) and the chosen one.
struct DependencyTable {
    summary: Line<'static>,
    rows: Vec<[String; 4]>,
    status: Vec<(String, Color)>,
    selected: usize,
}

fn dependency_table(dependencies: &Dependencies, elapsed: std::time::Duration) -> DependencyTable {
    let summary = match dependencies.counts() {
        Some((missing, installed)) => Line::from(vec![
            Span::styled(
                format!("{missing} missing"),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                format!("   {installed} installed"),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                "   versions as each program says them",
                Style::default().fg(MUTED),
            ),
        ]),
        None => busy(spinner(elapsed), "Checking..."),
    };
    let mut rows = Vec::new();
    let mut status = Vec::new();
    for (index, component) in dependencies.components.iter().enumerate() {
        let found = dependencies.found.as_ref().map(|found| &found[index]);
        let (installed, state, action) = match found {
            None => (String::new(), ("Checking".to_owned(), MUTED), None),
            Some(Found::Missing) => (
                "—".to_owned(),
                ("Missing".to_owned(), Color::Yellow),
                component.install.as_ref().map(|_| "Install"),
            ),
            Some(Found::Installed(version)) => (
                version.clone().unwrap_or_else(|| "found".into()),
                ("Installed".to_owned(), Color::Green),
                component.update.as_ref().map(|_| "Update"),
            ),
        };
        let action = match (found, action) {
            (None, _) => String::new(),
            (_, Some(action)) => action.to_owned(),
            (_, None) => "—".to_owned(),
        };
        rows.push([
            component.name.to_owned(),
            component.kind.to_owned(),
            installed,
            action,
        ]);
        status.push(state);
    }
    DependencyTable {
        summary,
        rows,
        status,
        selected: dependencies.selected,
    }
}

/// The summary, the header and the rows of the Dependencies tab in `area`; where each row is.
fn render_dependencies(
    frame: &mut Frame,
    table: Option<&DependencyTable>,
    area: Rect,
    places: &mut Vec<(usize, Rect)>,
) {
    let Some(table) = table else {
        return;
    };
    let width = usize::from(area.width);
    let cut = |text: &str, room: usize| {
        let text = text
            .chars()
            .take(room.saturating_sub(1))
            .collect::<String>();
        format!("{text:<room$}")
    };
    let (name, kind, status, action) = (28, 16, 11, 8);
    let installed = width.saturating_sub(name + kind + status + action);
    let mut lines = vec![
        table.summary.clone(),
        Line::from(Span::styled(
            format!(
                "{}{}{}{}{}",
                cut("Component", name),
                cut("Kind", kind),
                cut("Installed", installed),
                cut("Status", status),
                cut("Action", action)
            ),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
    ];
    let room = usize::from(area.height).saturating_sub(lines.len());
    let first = (table.selected + 1).saturating_sub(room);
    for (index, row) in table.rows.iter().enumerate().skip(first).take(room) {
        let (text, colour) = &table.status[index];
        let mut line = Line::from(vec![
            Span::styled(cut(&row[0], name), Style::default().fg(TEXT)),
            Span::styled(cut(&row[1], kind), Style::default().fg(MUTED)),
            Span::styled(cut(&row[2], installed), Style::default().fg(TEXT)),
            Span::styled(cut(text, status), Style::default().fg(*colour)),
            Span::styled(cut(&row[3], action), Style::default().fg(ACTIVE)),
        ]);
        if index == table.selected {
            line = line.style(Style::default().add_modifier(Modifier::REVERSED));
        }
        let y = area.y + lines.len() as u16;
        places.push((index, Rect::new(area.x, y, area.width, 1)));
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// A command waiting for Enter (F32, F33): what it is, then the command, over everything.
pub(super) fn render_confirm(frame: &mut Frame, app: &App, area: Rect) {
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
pub(super) fn render_choice(frame: &mut Frame, app: &mut App, area: Rect) {
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
    let update = " Check for updates [U] ";
    let notes = " Release notes [N] ";
    let tagline = "A terminal-native integrated development environment";
    let button_style = Style::default()
        .fg(ACTIVE)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    // What the last check found, in place of the hint: the dialog keeps its size.
    let last = app
        .updater
        .checked
        .clone()
        .unwrap_or_else(|| "Press Esc to close".to_string());
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
        Line::from(Span::styled(button, button_style)),
        // Two buttons on one row: About keeps its height, and the large flower its room.
        Line::from(vec![
            Span::styled(update, button_style),
            Span::raw("  "),
            Span::styled(notes, button_style),
        ]),
        Line::from(Span::styled(last, Style::default().fg(MUTED))),
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
            let centred = |label: &str, row: u16| {
                let width = (label.len() as u16).min(inner.width);
                if row < inner.height {
                    Rect::new(inner.x + (inner.width - width) / 2, inner.y + row, width, 1)
                } else {
                    Rect::default()
                }
            };
            app.regions.about_repository = centred(button, button_row);
            let pair = centred(&format!("{update}  {notes}"), button_row + 1);
            app.regions.about_update = Rect {
                width: (update.len() as u16).min(pair.width),
                ..pair
            };
            app.regions.about_notes = Rect::new(
                pair.x + update.len() as u16 + 2,
                pair.y,
                notes.len() as u16,
                1,
            )
            .intersection(pair);
        },
    );
}

/// The tutorial pages for `os` (`std::env::consts::OS`): what each panel does and its key.
fn tutorial_steps(os: &str, keys: &Keymap) -> [(&'static str, String); TUTORIAL_STEPS] {
    let key = |action| keys.label(action);
    let fixed = |label| key_for(os, label);
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
                key(Action::Explorer)
            ),
        ),
        (
            "Terminals",
            format!(
                "{} opens the bottom terminal and {} the right one. Drag the dividers to resize \
                 the panels; the mouse wheel scrolls the history.",
                key(Action::BottomTerminal),
                key(Action::RightTerminal)
            ),
        ),
        (
            "Source Control",
            format!(
                "{} opens changes, diffs, the commit graph, branches and history. Stage, commit, \
                 pull and push are one click away, and risky steps ask first.",
                key(Action::SourceControl)
            ),
        ),
        (
            "Database",
            format!(
                "{} connects to SQLite, PostgreSQL or MySQL. Write a query and run it with \
                 {} or {}.",
                key(Action::Database),
                fixed("Ctrl+Enter"),
                fixed("F5")
            ),
        ),
        (
            "Search and help",
            format!(
                "{} finds in the open file and {} in the whole folder. {} lists every command, \
                 {} shows About and {} quits.",
                key(Action::FindInFile),
                key(Action::FindInWorkspace),
                key(Action::Help),
                key(Action::About),
                key(Action::Quit)
            ),
        ),
    ]
}

/// The first start (F17): one page at a time, with Next and Skip always in reach.
pub(super) fn render_tutorial(frame: &mut Frame, app: &mut App, area: Rect) {
    let steps = tutorial_steps(std::env::consts::OS, &app.keys);
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
/// The notes of `notes` that `filter` lets through, version by version, newest first; a
/// version with none of them is left out.
fn note_lines(notes: &[&Notes], filter: NotesFilter) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for version in notes {
        let sections = [
            (
                "New",
                version.new,
                Color::Green,
                filter != NotesFilter::Fixed,
            ),
            (
                "Fixed",
                version.fixed,
                Color::Yellow,
                filter != NotesFilter::New,
            ),
        ];
        if sections
            .iter()
            .all(|(_, items, _, shown)| !shown || items.is_empty())
        {
            continue;
        }
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                format!("Version {}", version.version),
                Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD),
            )),
        ]);
        for (title, items, color, shown) in sections {
            if !shown || items.is_empty() {
                continue;
            }
            lines.push(Line::from(Span::styled(
                format!("  {title}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            lines.extend(items.iter().map(|item| Line::from(format!("    - {item}"))));
        }
    }
    if lines.is_empty() {
        lines.extend([
            Line::from(""),
            Line::from(Span::styled(
                format!(
                    "Nothing {} in these versions",
                    filter.label().to_lowercase()
                ),
                Style::default().fg(MUTED),
            )),
        ]);
    }
    lines
}

/// What's new after an update (F18), or the notes of every version from About's Release
/// notes; All, New and Fixed on top choose which notes.
pub(super) fn render_whats_new(frame: &mut Frame, app: &mut App, area: Rect) {
    let notes = if app.notes.is_empty() {
        vec![&NOTES[0]]
    } else {
        app.notes.clone()
    };
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut lines = vec![if app.all_notes {
        Line::from(Span::styled(
            format!("SozoIDE {}: the notes of every version", branding::VERSION),
            bold,
        ))
    } else {
        Line::from(Span::styled(
            format!("SozoIDE is now on version {}", branding::VERSION),
            bold,
        ))
    }];
    if notes.len() > 1 && !app.all_notes {
        lines.push(Line::from(Span::styled(
            format!("{} versions since the one you had", notes.len()),
            Style::default().fg(MUTED),
        )));
    }
    lines.extend(note_lines(&notes, app.notes_filter));
    // Long notes wrap: the rows they take inside the borders, counted as the paragraph wraps
    // them (an estimate scrolled past the end once the notes grew, E32).
    let inner = 80
        .min(area.width.saturating_sub(4))
        .saturating_sub(2)
        .max(1);
    let rows = Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .line_count(inner) as u16;
    let dialog = dialog_area(area, 80, rows + 5);
    let visible = dialog.height.saturating_sub(5);
    let most = rows.saturating_sub(visible);
    app.overlay_scroll = app.overlay_scroll.min(most);
    let scroll = app.overlay_scroll;
    let close = " Close [Enter] ";
    let progress = app.motion.overlay();
    let chosen = app.notes_filter;
    let title = if app.all_notes {
        " Release notes "
    } else {
        " What's new "
    };
    render_dialog(
        frame,
        dialog,
        progress,
        title.to_string(),
        |frame, inner| {
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);
            // All, New and Fixed, like the tabs of Source Control: Tab or a click.
            let mut x = rows[0].x + 1;
            for filter in NotesFilter::ALL {
                let label = format!(" {} ", filter.label());
                let style = if filter == chosen {
                    Style::default()
                        .fg(ACTIVE)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::default().fg(TEXT).add_modifier(Modifier::REVERSED)
                };
                let width = label.len() as u16;
                let button = Rect::new(x, rows[0].y, width, 1).intersection(rows[0]);
                frame.render_widget(Paragraph::new(label).style(style), button);
                app.regions.notes_filters.push((filter, button));
                x += width + 1;
            }
            frame.render_widget(
                Paragraph::new(" Tab: filter").style(Style::default().fg(MUTED)),
                Rect::new(x, rows[0].y, rows[0].right().saturating_sub(x), 1).intersection(rows[0]),
            );
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0)),
                rows[1],
            );
            let (close_area, _) = buttons(frame, rows[2], close, "");
            app.regions.dialog_next = close_area;
            if most > 0 {
                let hint = Rect::new(
                    close_area.right() + 2,
                    rows[2].y,
                    rows[2].width.saturating_sub(close_area.width + 2),
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

    fn help_text(os: &'static str) -> String {
        let keys = Keymap::defaults(os);
        help_sections(os)
            .iter()
            .flat_map(|section| {
                std::iter::once(section.title.to_owned()).chain(section.keys.iter().map(
                    |(key, description)| {
                        let key = help_key(*key, &keys, &KeyEditor::default());
                        format!("{key} = {description}")
                    },
                ))
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
        let keys = Keymap::defaults("linux");
        let editor = KeyEditor::default();
        let (page, places) = help_page("linux", 52, true, &keys, &editor);
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
        // Each command's key is on its row, in the left or the right column.
        let row = |action| {
            let (row, x, ..) = places.iter().find(|place| place.3 == action).unwrap();
            (&text[*row], *x as usize)
        };
        let (line, x) = row(Action::Explorer);
        assert!(line[x..].starts_with("│ Ctrl+B"), "{line}");
        let (line, x) = row(Action::Quit);
        assert!(
            line.chars()
                .skip(x)
                .collect::<String>()
                .starts_with("│ Ctrl+Q"),
            "{line}"
        );
        assert_eq!(places.len(), 30, "29 commands and Database twice");
        let (single, places) = help_page("linux", 40, false, &keys, &editor);
        assert!(single.len() > page.len());
        assert!(single.iter().all(|line| line.width() == 40));
        assert!(places.iter().all(|place| place.1 == 0));
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
        let steps = |os| tutorial_steps(os, &Keymap::defaults(os));
        let mac = steps("macos").map(|(_, text)| text).join("\n");
        assert!(mac.contains("Ctrl+Option+B") && mac.contains("fn+F1"));
        let linux = steps("linux").map(|(_, text)| text).join("\n");
        assert!(linux.contains("Ctrl+Alt+B") && !linux.contains("fn+"));
        let windows = steps("windows").map(|(_, text)| text).join("\n");
        assert!(windows.contains("Ctrl+Shift+B"));
    }

    #[test]
    fn the_tutorial_says_what_was_counted() {
        let (_, welcome) = &tutorial_steps("linux", &Keymap::defaults("linux"))[0];
        assert!(welcome.contains("anonymously") && welcome.contains("SOZO_NO_TELEMETRY=1"));
    }

    #[test]
    fn notes_are_shown_all_new_or_fixed_without_empty_versions() {
        const BOTH: Notes = Notes {
            version: "9.1.0",
            new: &["a new thing"],
            fixed: &["a fixed thing"],
        };
        const NEW_ONLY: Notes = Notes {
            version: "9.0.0",
            new: &["an older new thing"],
            fixed: &[],
        };
        let text = |notes: &[&Notes], filter| {
            note_lines(notes, filter)
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let all = text(&[&BOTH, &NEW_ONLY], NotesFilter::All);
        for shown in [
            "Version 9.1.0",
            "  New",
            "a new thing",
            "  Fixed",
            "a fixed thing",
            "Version 9.0.0",
            "an older new thing",
        ] {
            assert!(all.contains(shown), "{shown}");
        }
        let new = text(&[&BOTH, &NEW_ONLY], NotesFilter::New);
        assert!(new.contains("a new thing") && new.contains("an older new thing"));
        assert!(!new.contains("Fixed") && !new.contains("a fixed thing"));
        // A version with nothing fixed is left out.
        let fixed = text(&[&BOTH, &NEW_ONLY], NotesFilter::Fixed);
        assert!(fixed.contains("a fixed thing") && !fixed.contains("New"));
        assert!(!fixed.contains("Version 9.0.0"));
        assert_eq!(
            text(&[&NEW_ONLY], NotesFilter::Fixed).trim(),
            "Nothing fixed in these versions"
        );
        assert_eq!(NotesFilter::All.step(true), NotesFilter::New);
        assert_eq!(NotesFilter::All.step(false), NotesFilter::Fixed);
        assert_eq!(NotesFilter::Fixed.step(true), NotesFilter::All);
    }
}
