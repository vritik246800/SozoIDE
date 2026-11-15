//! What opens over the workspace, one file per dialog.

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
    update::deps::purpose,
    update::{NOTES, Notes, UpdateState, deps::Found},
};

use super::{
    ACTIVE, BORDER, MATCH, MUTED, TEXT, bar, busy, centered, components::key_for, panel_block,
    render_dialog, scale_length, sweep_bar, unfold,
};

mod about;
mod code;
mod editing;
mod help;
mod search;
mod setup;
mod update;

pub(super) use about::{render_about, render_tutorial, render_whats_new};
pub(super) use code::{
    render_actions, render_debug_confirm, render_debug_input, render_edit_preview, render_rename,
    render_signature,
};
pub(super) use editing::{
    render_comparison, render_disk_prompt, render_finder, render_go_to_line, render_layouts,
    render_replace_plan,
};
pub(super) use help::render_help;
pub(super) use search::render_search;
pub(super) use setup::{render_choice, render_confirm, render_setup};
pub(super) use update::render_update;

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
