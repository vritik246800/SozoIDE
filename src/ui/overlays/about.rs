//! About with its flower, the tutorial (F17) and the notes of each version (F18).

use super::*;

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
pub(in crate::ui) fn render_about(frame: &mut Frame, app: &mut App, area: Rect) {
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
        // The licence, the crates' licences and where SozoIDE keeps its files (TODO 6.3.1).
        Line::from(Span::styled(
            format!(
                "{} licence  ·  crates' licences [L]",
                env!("CARGO_PKG_LICENSE")
            ),
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            crate::app::folders_line(),
            Style::default().fg(MUTED),
        )),
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
                "{} opens the bottom terminal and {} the right one, and {} leaves them: there \
                 Ctrl and a letter go to the program. Drag a divider to resize the panels, or a \
                 panel's title to move it anywhere.",
                key(Action::BottomTerminal),
                key(Action::RightTerminal),
                key(Action::LeaveTerminal)
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
            "New in this version",
            format!(
                "{} finds in a terminal's history, {} shows who changed each line, {} keeps \
                 layouts with a name and {} runs the test under the cursor. Help ({}) lists \
                 everything, and T there brings this tour back.",
                key(Action::FindInTerminal),
                key(Action::Blame),
                key(Action::Layouts),
                key(Action::TestAtCursor),
                key(Action::Help)
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

pub(in crate::ui) fn render_tutorial(frame: &mut Frame, app: &mut App, area: Rect) {
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
pub(in crate::ui) fn render_whats_new(frame: &mut Frame, app: &mut App, area: Rect) {
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
                Paragraph::new(format!(
                    " Tab: filter   {}: every key in Help",
                    app.keys.label(Action::Help)
                ))
                .style(Style::default().fg(MUTED)),
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

#[cfg(test)]
mod tests {
    use super::*;

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
