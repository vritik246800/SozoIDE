use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    editor::{SearchScope, SearchState},
    lsp::Goal,
    media::Control,
    run::Task,
    terminal::MouseAction,
    update::UpdateState,
};

use super::{
    Action, App, DropZone, EntryAction, Focus, HelpTab, NotesFilter, Overlay, Panel, PanelDrag,
    SplitDirection, TUTORIAL_STEPS, TabDrag, TerminalSelection, video_areas,
};

mod dialogs;
mod editor;
mod explorer;
mod mouse;
mod terminal;
#[cfg(test)]
mod tests;

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Help waiting for the new keys of a command takes every key first, Ctrl+Q too (F29).
        if self.overlay == Overlay::Help
            && !self.motion.overlay_closing()
            && (self.key_editor.capturing || self.key_editor.confirm_reset)
        {
            self.edit_shortcut(key);
            return;
        }
        // The commands go by the keys the user gave them (`keys.toml`, F29).
        let action = self.keys.action(key);
        if action == Some(Action::Quit) {
            self.flush_all_files();
            self.save_session();
            self.should_quit = true;
            return;
        }
        // The program in a terminal gets Ctrl and a letter, as in any terminal (Q81).
        let action = action.filter(|_| {
            !(self.terminal_has_the_keys()
                && key.modifiers == KeyModifiers::CONTROL
                && matches!(key.code, KeyCode::Char(_)))
        });
        // The first key closes the splash; while the IDE is drawn over it, keys already reach
        // the IDE.
        if self.splash_visible && !self.motion.splash_closing() {
            self.dismiss_splash();
            return;
        }
        if self.name_prompt.is_some() {
            self.handle_name_key(key.code);
            return;
        }
        if self.properties.is_some() {
            // Nothing to edit: Esc or Enter close it, and stop a folder's count.
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.properties = None;
            }
            return;
        }
        // A command waiting for Enter, then the choice of a project (F32, F33).
        if self.confirm.is_some() {
            self.handle_confirm_key(key);
            return;
        }
        if self.project_choice.is_some() {
            self.handle_choice_key(key);
            return;
        }
        if self.entry_menu.is_some() {
            self.handle_menu_key(key.code);
            return;
        }
        // The dialogs of TODO 2.1–2.2: a replace waiting for Enter, a file changed on disk,
        // Go to line, and the file finder or the palette.
        if self.replace_plan.is_some() {
            return self.handle_replace_plan_key(key);
        }
        // The debugger's boxes (TODO 3.2.2).
        if self.debug_confirm.is_some() {
            return self.handle_debug_confirm_key(key);
        }
        if self.debug_input.is_some() {
            return self.handle_debug_input_key(key);
        }
        // Code Intelligence boxes (TODO 3.2.1).
        if self.edit_preview.is_some() {
            return self.handle_preview_key(key);
        }
        if self.rename.is_some() {
            return self.handle_rename_key(key);
        }
        if self.action_menu.is_some() {
            return self.handle_action_key(key);
        }
        if self.signature.is_some() && key.code == KeyCode::Esc {
            self.signature = None;
            return;
        }
        if self.disk_prompt.is_some() {
            return self.handle_disk_key(key);
        }
        if self.go_to_line.is_some() {
            return self.handle_go_to_line_key(key);
        }
        if self.comparison.is_some() {
            return self.handle_comparison_key(key);
        }
        if self.layout_menu.is_some() {
            return self.handle_layout_key(key);
        }
        if self.finder.is_some() {
            return self.handle_finder_key(key);
        }
        if self.search.is_some() && !self.motion.search_closing() {
            self.handle_search_key(key);
            return;
        }
        if self.overlay == Overlay::Update && !self.motion.overlay_closing() {
            self.handle_update_key(key.code);
            return;
        }
        if self.overlay != Overlay::None {
            if self.motion.overlay_closing() {
                match action {
                    Some(Action::Help) => self.open_overlay(Overlay::Help),
                    Some(Action::About) => self.open_overlay(Overlay::About),
                    Some(Action::FindInFile) => self.open_search(SearchScope::CurrentFile),
                    Some(Action::FindInWorkspace) => self.open_search(SearchScope::Workspace),
                    _ => {}
                }
            } else {
                self.handle_overlay_key(key);
            }
            return;
        }

        // F2 in the Explorer renames, as in every file explorer; elsewhere it opens About.
        if self.focus == Focus::Explorer
            && !self.from_palette
            && key.code == KeyCode::F(2)
            && key.modifiers.is_empty()
        {
            return self.handle_explorer_key(key);
        }
        // F2 in the editor renames the symbol (Q94); elsewhere it is About.
        if self.focus == Focus::Editor
            && !self.from_palette
            && self.active_file().is_some_and(|file| file.is_text())
            && key.code == KeyCode::F(2)
            && key.modifiers.is_empty()
        {
            return self.start_rename();
        }
        match action {
            // The project's commands work from every panel (F32); Shift+F5 too, while
            // something runs, since running takes the focus to the terminal.
            Some(Action::Build) => return self.run_task(Task::Build),
            Some(Action::Clean) => return self.run_task(Task::Clean),
            Some(Action::Rebuild) => return self.run_task(Task::Rebuild),
            Some(Action::RunProject) => return self.run_task(Task::Run),
            Some(Action::StopDebug) if self.something_runs() => return self.stop(),
            Some(Action::Help) => {
                self.flush_active_file();
                self.open_overlay(Overlay::Help);
                return;
            }
            Some(Action::QuickOpen) => return self.open_finder(super::FinderMode::Files),
            // Named tasks, the last one again and the test under the cursor (TODO 5.2.5).
            Some(Action::Tasks) => return self.open_finder(super::FinderMode::Tasks),
            // The same file in the other pane, and two files side by side (TODO 5.2.6).
            Some(Action::OpenToSide) => return self.open_to_side(),
            // What the mouse does, from the keyboard (TODO 5.2.7).
            Some(Action::ContextMenu) => return self.context_menu(),
            Some(Action::MovePanelLeft) => return self.move_panel(super::DropZone::Left),
            Some(Action::MovePanelRight) => return self.move_panel(super::DropZone::Right),
            Some(Action::MovePanelUp) => return self.move_panel(super::DropZone::Top),
            Some(Action::MovePanelDown) => return self.move_panel(super::DropZone::Bottom),
            Some(Action::CompareFiles) => return self.ask_comparison(),
            Some(Action::LastTask) => return self.run_last_task(),
            Some(Action::TestAtCursor) => return self.run_test_at_cursor(),
            // The layout from the keyboard (TODO 5.2.3, `app/arrange.rs`).
            Some(Action::Maximize) => return self.toggle_maximized(),
            Some(Action::ResetLayout) => return self.reset_layout(),
            Some(Action::Layouts) => return self.open_layouts(),
            Some(Action::FocusLeft) => return self.focus_beside((-1, 0)),
            Some(Action::FocusRight) => return self.focus_beside((1, 0)),
            Some(Action::FocusUp) => return self.focus_beside((0, -1)),
            Some(Action::FocusDown) => return self.focus_beside((0, 1)),
            Some(Action::Problems) => return self.open_finder(super::FinderMode::Problems),
            Some(Action::CommandPalette) => return self.open_finder(super::FinderMode::Commands),
            Some(Action::About) => {
                self.flush_active_file();
                self.open_overlay(Overlay::About);
                return;
            }
            Some(Action::SourceControl) => {
                self.flush_active_file();
                self.show_git = !self.show_git;
                self.show_database = false;
                self.focus = if self.show_git {
                    self.git.open();
                    Focus::Git
                } else {
                    Focus::Editor
                };
                return;
            }
            Some(Action::Database) => {
                self.flush_active_file();
                self.show_database = !self.show_database;
                self.show_git = false;
                self.focus = if self.show_database {
                    Focus::Database
                } else {
                    Focus::Editor
                };
                return;
            }
            Some(Action::FindInFile | Action::FindInWorkspace) => {
                self.flush_active_file();
                let scope = if action == Some(Action::FindInWorkspace) {
                    SearchScope::Workspace
                } else {
                    SearchScope::CurrentFile
                };
                self.open_search(scope);
                return;
            }
            Some(Action::RightTerminal) => {
                self.flush_active_file();
                self.show_right_terminal = !self.show_right_terminal;
                self.focus = if self.show_right_terminal {
                    Focus::RightTerminal
                } else {
                    Focus::Editor
                };
                return;
            }
            Some(Action::Explorer) => {
                self.flush_active_file();
                self.show_explorer = !self.show_explorer;
                self.focus = if self.show_explorer {
                    Focus::Explorer
                } else {
                    Focus::Editor
                };
                return;
            }
            Some(Action::BottomTerminal) => {
                self.flush_active_file();
                self.show_bottom_terminal = !self.show_bottom_terminal;
                self.focus = if self.show_bottom_terminal {
                    Focus::BottomTerminal
                } else {
                    Focus::Editor
                };
                return;
            }
            // Out of a terminal that takes every Ctrl key (Q81).
            Some(Action::LeaveTerminal)
                if matches!(self.focus, Focus::BottomTerminal | Focus::RightTerminal) =>
            {
                self.terminal_selection = None;
                self.focus = Focus::Editor;
                return;
            }
            // The history of the terminal with the focus (TODO 5.1.2, 5.1.4, 5.2.4).
            Some(Action::FindInTerminal)
                if matches!(self.focus, Focus::BottomTerminal | Focus::RightTerminal) =>
            {
                return self.open_terminal_search();
            }
            Some(Action::ClearTerminal)
                if matches!(self.focus, Focus::BottomTerminal | Focus::RightTerminal) =>
            {
                self.terminals(self.focus).current().clear_history();
                self.status = "The terminal's history is cleared".into();
                return;
            }
            Some(Action::SplitTerminal)
                if matches!(self.focus, Focus::BottomTerminal | Focus::RightTerminal) =>
            {
                return self.split_terminal(self.focus);
            }
            // The tabs of the terminal with the focus (F04).
            Some(Action::NextTerminal | Action::PreviousTerminal)
                if matches!(self.focus, Focus::BottomTerminal | Focus::RightTerminal) =>
            {
                self.terminals(self.focus)
                    .step(action == Some(Action::NextTerminal));
                self.terminal_selection = None;
                return;
            }
            _ => {}
        }

        match self.focus {
            Focus::Explorer => self.handle_explorer_key(key),
            Focus::Editor => self.handle_editor_key(key),
            Focus::BottomTerminal | Focus::RightTerminal => {
                if self.terminal_box_key(key) {
                    return;
                }
                // Typing leaves the text chosen with the mouse.
                self.terminal_selection = None;
                self.terminals(self.focus).current_mut().send_key(key);
            }
            Focus::Database => {
                if !self.database.handle_key(key) {
                    self.show_database = false;
                    self.focus = Focus::Editor;
                }
            }
            Focus::Git => {
                if !self.git.handle_key(key) {
                    self.show_git = false;
                    self.focus = Focus::Editor;
                }
            }
        }
    }

    /// Whether a key goes to a terminal: it has the focus and nothing is open over it.
    pub(in crate::app) fn terminal_has_the_keys(&self) -> bool {
        matches!(self.focus, Focus::BottomTerminal | Focus::RightTerminal)
            && self.overlay == Overlay::None
            && self.search.is_none()
            && self.confirm.is_none()
            && self.project_choice.is_none()
            && self.entry_menu.is_none()
            && self.finder.is_none()
            && self.layout_menu.is_none()
            && self.comparison.is_none()
            && self.go_to_line.is_none()
            && self.disk_prompt.is_none()
            && self.replace_plan.is_none()
            && self.rename.is_none()
            && self.debug_input.is_none()
            && self.debug_confirm.is_none()
            && self.action_menu.is_none()
            && self.edit_preview.is_none()
            && self.name_prompt.is_none()
            && self.properties.is_none()
    }
}

fn sideways(mouse: MouseEvent) -> bool {
    mouse.modifiers.contains(KeyModifiers::SHIFT)
}

/// The source row and the visual column under `point` in a file drawn in `area`.
fn text_position(file: &crate::editor::OpenFile, area: Rect, point: (u16, u16)) -> (usize, usize) {
    let display = file.scroll as usize + point.1.saturating_sub(area.y.saturating_add(1)) as usize;
    // Below the last line: a row past the end, which the cursor clamps to the last one.
    let row = file.source_row(display).unwrap_or(file.lines.len());
    let gutter = file.lines.len().max(1).to_string().len() + 3;
    let text_start = area.x.saturating_add(1).saturating_add(gutter as u16);
    // On a wrapped row the columns go on from where the row before stopped (TODO 2.1.8).
    let column = point.0.saturating_sub(text_start) as usize
        + file.scroll_x as usize
        + file.display_offset(display);
    (row, column)
}

/// `area` inside its border.
fn inner_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

/// The cell (row, column) of a terminal under `point`, kept inside its border while a drag
/// goes past it.
fn clamped_cell(area: Rect, point: (u16, u16)) -> (u16, u16) {
    let inner = inner_area(area);
    let row = point
        .1
        .clamp(inner.y, inner.bottom().saturating_sub(1).max(inner.y));
    let column = point
        .0
        .clamp(inner.x, inner.right().saturating_sub(1).max(inner.x));
    (row - inner.y, column - inner.x)
}

pub(super) fn contains(area: Rect, point: (u16, u16)) -> bool {
    point.0 >= area.x
        && point.0 < area.x.saturating_add(area.width)
        && point.1 >= area.y
        && point.1 < area.y.saturating_add(area.height)
}
