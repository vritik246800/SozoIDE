//! The search bar of the file and the dialog of the workspace search (F05).

use super::*;

/// The search (F05): Ctrl+P is a bar over the bottom of the editor, which stays in view with
/// every match marked; Ctrl+Shift+P a dialog with the results grouped by file.
pub(in crate::ui) fn render_search(frame: &mut Frame, app: &mut App, area: Rect) {
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
        // At the bottom right, away from the match, which scrolls to near the top; a row for
        // the options (TODO 2.3.1) and one more for the replace.
        let rows = 4 + u16::from(search.replace.is_some());
        let (width, height) = (60.min(editor.width), rows.min(editor.height));
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
        let parts = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
        search_header(frame, app, parts[0]);
        options_row(frame, app, parts[1]);
        replace_row(frame, app, parts[2]);
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
            // A list given whole takes no typing: no cursor; nor while the replace row types.
            Span::styled(
                if search.title.is_some() || search.in_replace {
                    ""
                } else {
                    "█"
                },
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

/// `Aa`, `ab` and `.*`, lit when on, with their keys; a regular expression that does not
/// compile says why, in red (TODO 2.3.1).
fn options_row(frame: &mut Frame, app: &App, area: Rect) {
    let Some(search) = &app.search else {
        return;
    };
    let toggle = |label: &'static str, on: bool| {
        Span::styled(
            format!(" {label} "),
            if on {
                Style::default().fg(Color::Black).bg(ACTIVE)
            } else {
                Style::default().fg(MUTED)
            },
        )
    };
    let options = search.options;
    let mut spans = vec![
        toggle("Aa", options.case),
        Span::raw(" "),
        toggle("ab", options.word),
        Span::raw(" "),
        toggle(".*", options.regex),
        Span::raw("  "),
    ];
    match &search.error {
        Some(error) => spans.push(Span::styled(error.clone(), Style::default().fg(Color::Red))),
        None => spans.push(Span::styled(
            if search.replace.is_some() {
                "Tab  Enter: one  Ctrl+Enter: all"
            } else {
                "Alt+C/W/R"
            },
            Style::default().fg(MUTED),
        )),
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The text that replaces each match (TODO 2.1.3).
fn replace_row(frame: &mut Frame, app: &App, area: Rect) {
    let Some(search) = &app.search else {
        return;
    };
    let Some(with) = &search.replace else {
        return;
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("⇄ ", Style::default().fg(ACTIVE)),
            Span::styled(with.clone(), Style::default().fg(TEXT)),
            Span::styled(
                if search.in_replace { "█" } else { "" },
                Style::default().fg(ACTIVE),
            ),
        ])),
        area,
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
    let replacing = app
        .search
        .as_ref()
        .is_some_and(|search| search.replace.is_some());
    let given = app
        .search
        .as_ref()
        .is_some_and(|search| search.title.is_some());
    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(!given)),
        Constraint::Length(u16::from(replacing)),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    search_header(frame, app, sections[0]);
    if !given {
        options_row(frame, app, sections[1]);
    }
    replace_row(frame, app, sections[2]);
    let sections = [sections[0], sections[3], sections[4], sections[5]];
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
pub(in crate::ui) fn match_line(
    found: &SearchMatch,
    selected: bool,
    width: usize,
) -> Line<'static> {
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
