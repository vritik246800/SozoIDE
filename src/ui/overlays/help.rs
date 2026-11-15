//! Help: the shortcuts by section, the user's own keys (F29) and the Dependencies tab (F33).

use super::*;

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
                bound(Action::LeaveTerminal),
                (Fixed("Ctrl+letter"), "Goes to the program in the terminal"),
                (Fixed("Wheel, mouse"), "Go to programs that use them"),
                (Fixed("Shift+drag"), "Select text over such a program"),
                (
                    Fixed("+ / x on the tabs"),
                    "New / close a bottom terminal (1 to 3)",
                ),
                (Fixed("Drag the mouse"), "Select text; letting go copies it"),
                (Fixed("Shift+Enter"), "New line without running it"),
                bound(Action::FindInTerminal),
                bound(Action::ClearTerminal),
                bound(Action::SplitTerminal),
                (Fixed("Ctrl+click"), "Open a web address or file:line"),
                (Fixed("Right-click a tab"), "Name the tab"),
            ],
        },
        HelpSection {
            title: "Layout",
            color: Color::Magenta,
            keys: vec![
                (
                    Fixed("Drag a title or corner"),
                    "Move the panel; a box shows where it lands",
                ),
                (Fixed("Drag a border"), "Resize it, wherever the panel is"),
                bound(Action::Maximize),
                bound(Action::ResetLayout),
                bound(Action::Layouts),
                bound(Action::FocusLeft),
                bound(Action::FocusRight),
                bound(Action::FocusUp),
                bound(Action::FocusDown),
                bound(Action::MovePanelLeft),
                bound(Action::MovePanelRight),
                bound(Action::MovePanelUp),
                bound(Action::MovePanelDown),
                bound(Action::ContextMenu),
            ],
        },
        HelpSection {
            title: "Files and tabs",
            color: Color::Blue,
            keys: vec![
                bound(Action::OpenToSide),
                bound(Action::CompareFiles),
                (Fixed("Enter"), "Open a file or a folder"),
                (
                    Fixed("Right-click"),
                    "New, duplicate, rename, delete, copy path, reveal (Explorer)",
                ),
                (Fixed("Ctrl+X/C/V"), "Cut, copy, paste (Explorer)"),
                (Fixed("Left / Right"), "Close or open a folder (Explorer)"),
                (
                    Fixed("F2, N"),
                    "Rename, new file (Explorer; F2 elsewhere: About)",
                ),
                (Fixed("Shift+N"), "New folder (Explorer)"),
                (Fixed("Del"), "To the Trash; Shift+Del for good (Explorer)"),
                (Fixed("Ctrl+Z"), "Undo a move, rename or delete (Explorer)"),
                (
                    Fixed("Ctrl+click"),
                    "Mark several; Shift+click a range (Explorer)",
                ),
                (Fixed("/, I"), "Find a name; dim ignored files (Explorer)"),
                (Fixed("Drag a row"), "Onto a folder: move it, after Enter"),
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
                bound(Action::Redo),
                (Fixed("Ctrl+Shift+Z"), "Redo, too"),
                bound(Action::ToggleComment),
                (
                    Fixed("Tab / Shift+Tab"),
                    "Indent / outdent the selected lines",
                ),
                (Fixed("Alt+Up / Alt+Down"), "Move the lines up / down"),
                (Fixed("Shift+Alt+Down"), "Duplicate the lines"),
                (Fixed("Ctrl+Shift+K"), "Delete the lines"),
                (Fixed("Ctrl+Left/Right"), "A word back / forward"),
                (Fixed("Ctrl+Backspace"), "Delete the word before"),
                (Fixed("Double click"), "Select the word; three: the line"),
                bound(Action::WrapLines),
                bound(Action::GoToLine),
                (Fixed("Ctrl+C / Ctrl+X"), "Copy / cut the selection"),
                (Fixed("Ctrl+V"), "Paste"),
                bound(Action::Suggestions),
                bound(Action::Definition),
                bound(Action::Peek),
                bound(Action::References),
                bound(Action::Hover),
                (Fixed("Ctrl+mouse 2 s"), "On a word: the same information"),
                (Fixed("Ctrl+click"), "Go to the definition"),
                bound(Action::Back),
                (Fixed("F2 (editor)"), "Rename the symbol in every file"),
                bound(Action::FormatDocument),
                bound(Action::CodeActions),
                bound(Action::Problems),
                (
                    Fixed("@ or # (Ctrl+O)"),
                    "Symbols of the file / of the project",
                ),
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
                bound(Action::Replace),
                (
                    Fixed("Alt+C / W / R"),
                    "Case, whole word, regular expression",
                ),
                (
                    Fixed("Ctrl+Enter"),
                    "Replace every match (asks in a folder)",
                ),
                bound(Action::QuickOpen),
                bound(Action::CommandPalette),
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
                bound(Action::StepOut),
                bound(Action::RunToCursor),
                bound(Action::PauseDebug),
                bound(Action::BreakpointOptions),
                bound(Action::Evaluate),
                bound(Action::AddWatch),
                (
                    Fixed("Click (panel)"),
                    "A frame, a variable that opens, a thread",
                ),
                bound(Action::StopDebug),
                bound(Action::Tasks),
                bound(Action::LastTask),
                bound(Action::TestAtCursor),
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
                bound(Action::Blame),
                bound(Action::FileHistory),
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
                (
                    Fixed("Enter / Esc (Trust)"),
                    "Trust the folder: its language servers, debugger and commands run; Esc: not now",
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
    // Only the rows with the text typed after `/`, in their key, what they do or their area.
    let filter = editor.filter.as_deref().unwrap_or("").to_lowercase();
    let sections = help_sections(os)
        .into_iter()
        .filter_map(|mut section| {
            if filter.is_empty() || section.title.to_lowercase().contains(&filter) {
                return Some(section);
            }
            section.keys.retain(|(key, what)| {
                what.to_lowercase().contains(&filter)
                    || help_key(*key, keys, editor)
                        .to_lowercase()
                        .contains(&filter)
            });
            (!section.keys.is_empty()).then_some(section)
        })
        .collect::<Vec<_>>();
    let boxes = sections
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

pub(in crate::ui) fn render_help(frame: &mut Frame, app: &mut App, area: Rect) {
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
        .map(|dependencies| dependency_table(dependencies, app.motion.elapsed(), &app.lsp.usage));
    render_dialog(
        frame,
        dialog,
        progress,
        match &app.key_editor.filter {
            Some(filter) => format!(" Help - /{filter}▏ ({}) ", os_name(os)),
            None => format!(" Help - all commands, / finds ({}) ", os_name(os)),
        },
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
                        " Up/Down or a click: choose   Enter: install or update   R: check again   T: tmux on/off   Esc: close",
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
                    " Up/Down, {pages} or the wheel: scroll   T: tutorial   Esc: close{position}"
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

fn dependency_table(
    dependencies: &Dependencies,
    elapsed: std::time::Duration,
    usage: &std::collections::HashMap<&'static str, (u64, f32)>,
) -> DependencyTable {
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
            // A language server that runs says what it takes (TODO 3.2.3).
            Some(Found::Installed(version)) => (
                match usage.get(component.name) {
                    Some((memory, cpu)) => format!(
                        "{} · {}",
                        version.clone().unwrap_or_else(|| "found".into()),
                        crate::lsp::usage_label(*memory, *cpu)
                    ),
                    None => version.clone().unwrap_or_else(|| "found".into()),
                },
                // What its package manager says, when it can (TODO 6.2.2).
                match dependencies.outdated.get(index).copied().flatten() {
                    Some(true) => ("Update available".to_owned(), Color::Yellow),
                    Some(false) => ("Up to date".to_owned(), Color::Green),
                    None => ("Installed".to_owned(), Color::Green),
                },
                component.update.as_ref().map(|_| "Update"),
            ),
        };
        // The row being installed or updated says so until it changes (F35).
        let watched = dependencies
            .watching
            .as_ref()
            .is_some_and(|(row, _)| *row == index);
        let state = match found {
            Some(found) if watched => (
                format!(
                    "{} {}",
                    spinner(elapsed),
                    if *found == Found::Missing {
                        "Installing…"
                    } else {
                        "Updating…"
                    }
                ),
                ACTIVE,
            ),
            _ => state,
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
    let (name, kind, status, action) = (28, 16, 18, 8);
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

fn os_name(os: &str) -> &'static str {
    match os {
        "macos" => "macOS",
        "windows" => "Windows",
        _ => "Linux",
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
        assert_eq!(places.len(), 69, "68 commands and Database twice");
        // `/` narrows Help to the rows with the text (TODO 2.3.5); nothing matches, nothing.
        let mut narrowed = KeyEditor {
            filter: Some("redo".into()),
            ..KeyEditor::default()
        };
        let (found, places) = help_page("linux", 52, true, &keys, &narrowed);
        let text = found
            .iter()
            .map(|line| line.to_string())
            .collect::<String>();
        assert!(text.contains("Redo") && !text.contains("Quit"), "{text}");
        assert_eq!(places.len(), 1);
        narrowed.filter = Some("zzzz-nothing".into());
        assert!(help_page("linux", 52, true, &keys, &narrowed).0.is_empty());
        let (single, places) = help_page("linux", 40, false, &keys, &editor);
        assert!(single.len() > page.len());
        assert!(single.iter().all(|line| line.width() == 40));
        assert!(places.iter().all(|place| place.1 == 0));
    }
}
