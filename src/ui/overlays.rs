use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::{
    app::App,
    branding::{self, ASCII_LOGO},
    editor::SearchScope,
};

use super::{ACTIVE, MUTED, TEXT, centered, panel_block, scale_centered};

pub(super) fn render_splash(frame: &mut Frame, area: Rect, progress: f32) {
    let area = scale_centered(area, progress);
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(Clear, area);
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
        Line::from(Span::styled("Starting...", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            "Press any key to continue",
            Style::default().fg(MUTED),
        )),
    ]);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

pub(super) fn render_search(frame: &mut Frame, app: &mut App, area: Rect) {
    let area = scale_centered(centered(area, 84, 70), app.motion.search());
    if area.width < 2 || area.height < 4 {
        return;
    }
    frame.render_widget(Clear, area);
    let root = app.root.clone();
    let Some(search) = &mut app.search else {
        return;
    };
    let scope = match search.scope {
        SearchScope::CurrentFile => "Find in current file [Ctrl+P]",
        SearchScope::Workspace => "Find in workspace [Ctrl+Shift+P]",
    };
    let block = panel_block(format!(" {scope} "), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
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

pub(super) fn render_help(frame: &mut Frame, area: Rect, progress: f32) {
    let area = scale_centered(centered(area, 76, 84), progress);
    if area.width < 2 || area.height < 2 {
        return;
    }
    frame.render_widget(Clear, area);
    let lines = [
        "Navigation and panels",
        "  Ctrl+B             Toggle Explorer",
        "  Ctrl+J             Toggle bottom terminal",
        "  Ctrl+Shift+B       Toggle right terminal",
        "  Ctrl+G             Open Source Control",
        "  Ctrl+D             Open Database",
        "  Mouse drag         Resize panel dividers",
        "  Mouse wheel        Scroll panel or terminal history",
        "  PageUp/PageDown    Scroll focused editor or terminal",
        "",
        "Files and search",
        "  Enter              Open file or expand directory",
        "  Ctrl+W             Close active tab",
        "  Ctrl+P             Find in current file",
        "  Ctrl+Shift+P       Find in workspace",
        "  Esc                Close dialog or panel",
        "",
        "Editing",
        "  Type/paste         Edit the active text file",
        "  Arrows/Home/End    Move the cursor",
        "  Enter/Tab          Insert a new line or tab",
        "  Backspace/Delete   Remove text",
        "  Autosave           Save after 2 idle seconds or focus change",
        "",
        "Source Control",
        "  Ctrl+G             Changes, graph, branches and history",
        "  Buttons/tabs       Stage, commit, diff, pull, push and rebase",
        "",
        "Database",
        "  Ctrl+D             Connections, query editor and results",
        "  Ctrl+Enter         Run query",
        "",
        "Application",
        "  F1                 Help",
        "  F2                 About",
        "  Ctrl+Q             Quit",
    ];
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .block(panel_block(" Help - all commands ".to_string(), true)),
        area,
    );
}

pub(super) fn render_about(frame: &mut Frame, area: Rect, progress: f32) {
    let area = scale_centered(centered(area, 66, 84), progress);
    if area.width < 2 || area.height < 2 {
        return;
    }
    frame.render_widget(Clear, area);
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
        Line::from(format!("Created by {}", branding::AUTHOR)),
        Line::from("A terminal-native integrated development environment"),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(MUTED),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false })
            .block(panel_block(" About SozoIDE ".to_string(), true)),
        area,
    );
}
