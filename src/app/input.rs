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
    fn terminal_has_the_keys(&self) -> bool {
        matches!(self.focus, Focus::BottomTerminal | Focus::RightTerminal)
            && self.overlay == Overlay::None
            && self.search.is_none()
            && self.confirm.is_none()
            && self.project_choice.is_none()
            && self.entry_menu.is_none()
            && self.name_prompt.is_none()
            && self.properties.is_none()
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match self.keys.action(key) {
            Some(Action::FindInFile) => return self.open_search(SearchScope::CurrentFile),
            Some(Action::FindInWorkspace) => return self.open_search(SearchScope::Workspace),
            _ => {}
        }
        if self.motion.search_closing() {
            return;
        }
        let active = self.active_tab.and_then(|index| self.open_files.get(index));
        let Some(search) = &mut self.search else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_search(),
            KeyCode::Up => search.select_previous(),
            KeyCode::Down => search.select_next(),
            KeyCode::Enter => {
                if let Some(found) = search.selected_match().cloned() {
                    self.show_match(&found);
                    self.close_search();
                }
                return;
            }
            // References and definitions are a list given whole (F31).
            KeyCode::Backspace if search.title.is_none() => search.backspace(&self.root, active),
            KeyCode::Char(character) if !ctrl && search.title.is_none() => {
                search.push(character, &self.root, active);
            }
            _ => {}
        }
        self.preview_match();
    }

    /// Ctrl+P keeps the editor in view: it scrolls to the match chosen as the query and the
    /// arrows change it.
    pub(super) fn preview_match(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        if search.scope != SearchScope::CurrentFile {
            return;
        }
        if let (Some(found), Some(active)) = (search.selected_match(), self.active_tab) {
            let file = &mut self.open_files[active];
            file.scroll = file.display_row(found.line).saturating_sub(3) as u16;
            file.follow_cursor = false;
        }
    }

    /// Opens the file of `found` with the match selected a few lines below the top; where the
    /// cursor was goes to Back (F31).
    fn show_match(&mut self, found: &crate::editor::SearchMatch) {
        self.jump_to(found.path.clone(), found.line, found.column + found.length);
        if let Some(active) = self.active_tab
            && self.open_files[active].path == found.path
        {
            let file = &mut self.open_files[active];
            file.anchor = Some((found.line, found.column));
            file.scroll = file.display_row(found.line).saturating_sub(3) as u16;
            file.follow_cursor = false;
        }
    }

    /// The `◂`/`▸` of the search: the previous or the next result.
    pub(super) fn step_match(&mut self, forward: bool) {
        if let Some(search) = &mut self.search {
            if forward {
                search.select_next();
            } else {
                search.select_previous();
            }
        }
        self.preview_match();
    }

    /// The Explorer's name prompt: typing, Enter creates or renames, Esc cancels.
    fn handle_name_key(&mut self, code: KeyCode) {
        let Some(prompt) = &mut self.name_prompt else {
            return;
        };
        let typing = prompt.action != EntryAction::Delete;
        match code {
            KeyCode::Esc => self.name_prompt = None,
            KeyCode::Enter => self.confirm_name(),
            _ if !typing => {}
            KeyCode::Backspace => {
                prompt.name.pop();
                prompt.error = None;
            }
            KeyCode::Char(character) => {
                prompt.name.push(character);
                prompt.error = None;
            }
            _ => {}
        }
    }

    pub fn name_paste(&mut self, text: &str) {
        if let Some(prompt) = self
            .name_prompt
            .as_mut()
            .filter(|prompt| prompt.action != EntryAction::Delete)
        {
            prompt
                .name
                .extend(text.chars().filter(|character| !character.is_control()));
            prompt.error = None;
        }
    }

    /// The Explorer's right-click menu: arrows choose, Enter runs, Esc closes.
    fn handle_menu_key(&mut self, code: KeyCode) {
        let Some(menu) = &mut self.entry_menu else {
            return;
        };
        let count = menu.actions.len();
        match code {
            KeyCode::Esc => self.entry_menu = None,
            KeyCode::Up => menu.selected = (menu.selected + count - 1) % count,
            KeyCode::Down => menu.selected = (menu.selected + 1) % count,
            KeyCode::Enter => {
                let action = menu.actions[menu.selected];
                self.choose_entry_action(action);
            }
            _ => {}
        }
        if matches!(code, KeyCode::Up | KeyCode::Down) {
            self.preview_entry();
        }
    }

    /// The mouse over the open menu: the entry under it is chosen (a code entry shows its
    /// answers beside the menu, Q73), the preview's row under it is marked.
    fn menu_mouse(&mut self, point: (u16, u16)) {
        let entry = self
            .regions
            .entry_menu_items
            .iter()
            .position(|(_, area)| contains(*area, point));
        if let Some(index) = entry
            && let Some(menu) = &mut self.entry_menu
        {
            menu.selected = index;
            self.preview_entry();
        }
        let row = self
            .regions
            .preview_rows
            .iter()
            .find(|(_, area)| contains(*area, point))
            .map(|(row, _)| *row);
        if let Some(preview) = &mut self.menu_preview
            && (row.is_some() || entry.is_some())
        {
            preview.hovered = row;
        }
    }

    pub fn search_paste(&mut self, text: &str) {
        if self.motion.search_closing() {
            return;
        }
        let active = self.active_tab.and_then(|index| self.open_files.get(index));
        if let Some(search) = self.search.as_mut().filter(|search| search.title.is_none()) {
            search.push_str(text, &self.root, active);
        }
        self.preview_match();
    }

    /// Esc and the keys of Help and About close every dialog; the tutorial pages with Enter
    /// and the arrows, About opens the repository with G, Help changes shortcuts (F29).
    fn handle_overlay_key(&mut self, key: KeyEvent) {
        // Help's tabs: Left and Right, then the keys of the tab (F33).
        if self.overlay == Overlay::Help {
            match key.code {
                KeyCode::Left if self.help_tab == HelpTab::Dependencies => {
                    self.help_tab = HelpTab::Shortcuts;
                    return;
                }
                KeyCode::Right if self.help_tab == HelpTab::Shortcuts => {
                    return self.open_dependencies();
                }
                _ => {}
            }
            let handled = if self.help_tab == HelpTab::Dependencies {
                self.handle_dependencies_key(key)
            } else {
                self.handle_shortcut_key(key)
            };
            if handled {
                return;
            }
        }
        if self.overlay == Overlay::Setup {
            self.handle_setup_key(key);
            return;
        }
        let code = key.code;
        let closes = code == KeyCode::Esc
            || matches!(self.keys.action(key), Some(Action::Help | Action::About));
        match (self.overlay, code) {
            (Overlay::Tutorial, KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ')) => {
                if self.tutorial_step + 1 < TUTORIAL_STEPS {
                    self.tutorial_step += 1;
                } else {
                    self.close_overlay();
                }
            }
            (Overlay::Tutorial, KeyCode::Left | KeyCode::Backspace) => {
                self.tutorial_step = self.tutorial_step.saturating_sub(1);
            }
            (Overlay::Tutorial, KeyCode::Char('s' | 'S')) | (Overlay::WhatsNew, KeyCode::Enter) => {
                self.close_overlay()
            }
            (Overlay::About, KeyCode::Char('g' | 'G')) => self.open_repository(),
            (Overlay::About, KeyCode::Char('n' | 'N')) => self.open_release_notes(),
            // The notes dialog: Tab and BackTab go through All, New and Fixed (F18).
            (Overlay::WhatsNew, KeyCode::Tab | KeyCode::BackTab) => {
                let forward = code == KeyCode::Tab && !key.modifiers.contains(KeyModifiers::SHIFT);
                self.filter_notes(self.notes_filter.step(forward));
            }
            // A check from About also offers a version skipped before (F16).
            (Overlay::About, KeyCode::Char('u' | 'U')) => self.updater.check_now(),
            _ if closes => self.close_overlay(),
            (Overlay::Help | Overlay::WhatsNew, code) => {
                // The dialog clamps the scroll when it draws.
                let scroll = self.overlay_scroll;
                self.overlay_scroll = match code {
                    KeyCode::Up => scroll.saturating_sub(1),
                    KeyCode::Down => scroll.saturating_add(1),
                    KeyCode::PageUp => scroll.saturating_sub(10),
                    KeyCode::PageDown => scroll.saturating_add(10),
                    KeyCode::Home => 0,
                    KeyCode::End => u16::MAX,
                    _ => scroll,
                };
            }
            _ => {}
        }
    }

    /// Enter or U updates, S skips this version, Esc leaves it for later or cancels the download.
    fn handle_update_key(&mut self, code: KeyCode) {
        match (&self.updater.state, code) {
            (UpdateState::Available, KeyCode::Enter | KeyCode::Char('u' | 'U')) => {
                self.updater.start_download();
            }
            (UpdateState::Available, KeyCode::Char('s' | 'S')) => {
                self.updater.skip();
                if self.updater.state == UpdateState::Idle {
                    self.close_overlay();
                }
            }
            (_, KeyCode::Esc) => {
                self.updater.dismiss();
                self.close_overlay();
            }
            _ => {}
        }
    }

    /// About's Release notes [N] (F18): the notes of every version, in About's place.
    fn open_release_notes(&mut self) {
        self.notes = crate::update::NOTES.iter().collect();
        self.all_notes = true;
        self.notes_filter = NotesFilter::All;
        self.open_overlay(Overlay::WhatsNew);
    }

    /// Tab, BackTab or a click on All, New or Fixed: those notes, from the top.
    fn filter_notes(&mut self, filter: NotesFilter) {
        self.notes_filter = filter;
        self.overlay_scroll = 0;
    }

    pub(super) fn open_overlay(&mut self, overlay: Overlay) {
        let now = std::time::Instant::now();
        if overlay == Overlay::About && self.overlay != Overlay::About {
            self.motion.restart_bloom(now);
        }
        self.reset_key_editor(overlay);
        self.overlay = overlay;
        // The Dependencies tab checks again after an installation (F33).
        if overlay == Overlay::Help && self.help_tab == HelpTab::Dependencies {
            self.open_dependencies();
        }
        self.overlay_scroll = 0;
        self.motion.open_overlay(now);
    }

    pub(super) fn close_overlay(&mut self) {
        self.motion.close_overlay(std::time::Instant::now());
    }

    fn open_search(&mut self, scope: SearchScope) {
        self.search = Some(SearchState::new(scope));
        self.motion.open_search(std::time::Instant::now());
    }

    fn close_search(&mut self) {
        self.motion.close_search(std::time::Instant::now());
    }

    fn dismiss_splash(&mut self) {
        self.motion.close_splash(std::time::Instant::now());
    }

    fn handle_explorer_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            _ if self.keys.is(Action::Run, key) => {
                if let Some(entry) = self
                    .entries
                    .get(self.selected_entry)
                    .filter(|entry| !entry.is_dir)
                {
                    let path = entry.path.clone();
                    self.run_file(path);
                }
            }
            KeyCode::Char('c' | 'C') if ctrl => self.hold_selected_entry(false),
            KeyCode::Char('x' | 'X') if ctrl => self.hold_selected_entry(true),
            KeyCode::Char('v' | 'V') if ctrl => self.paste_selected_entry(),
            KeyCode::Up => self.selected_entry = self.selected_entry.saturating_sub(1),
            KeyCode::Down => {
                self.selected_entry =
                    (self.selected_entry + 1).min(self.entries.len().saturating_sub(1));
            }
            KeyCode::Enter | KeyCode::Right => self.activate_selected(),
            _ => {}
        }
        self.keep_selection_visible();
    }

    fn handle_editor_key(&mut self, key: KeyEvent) {
        let Some(active) = self.active_tab else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.keys.is(Action::CloseTab, key) {
            self.close_tab(active);
            return;
        }
        if self.keys.is(Action::Run, key) {
            let path = self.open_files[active].path.clone();
            return self.run_file(path);
        }
        let file = &mut self.open_files[active];
        if file
            .media
            .as_ref()
            .is_some_and(|media| media.video().is_some())
        {
            // A video's keys (F23); Esc leaves the full screen, then closes the tab.
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            let control = match key.code {
                _ if ctrl => None,
                KeyCode::Char(' ') => Some(Control::Play),
                KeyCode::Char('s' | 'S') => Some(Control::Stop),
                KeyCode::Left => Some(Control::Seek(if shift { -30 } else { -5 })),
                KeyCode::Right => Some(Control::Seek(if shift { 30 } else { 5 })),
                KeyCode::Char('-' | '_') => Some(Control::Quieter),
                KeyCode::Char('+' | '=') => Some(Control::Louder),
                KeyCode::Char('m' | 'M') => Some(Control::Mute),
                KeyCode::Char('[') => Some(Control::Slower),
                KeyCode::Char(']') => Some(Control::Faster),
                KeyCode::Char('f' | 'F') => Some(Control::Fullscreen),
                KeyCode::Esc if self.fullscreen => {
                    self.fullscreen = false;
                    None
                }
                KeyCode::Esc => return self.close_tab(active),
                _ => None,
            };
            if let Some(control) = control {
                self.control_video(active, control);
            }
            return;
        }
        if let Some(media) = &mut file.media {
            // A model turns, tilts and zooms.
            let arrow = match key.code {
                KeyCode::Left => Some((-1.0, 0.0)),
                KeyCode::Right => Some((1.0, 0.0)),
                KeyCode::Up => Some((0.0, -1.0)),
                KeyCode::Down => Some((0.0, 1.0)),
                _ => None,
            };
            let character = match key.code {
                KeyCode::Char(character) if !ctrl => Some(character),
                _ => None,
            };
            media.handle_key(character, arrow);
            return;
        }
        if file.pdf.is_some() || file.image.is_some() {
            // A PDF turns its pages; an image has nothing to edit. On a page of text the cursor
            // moves and selects and Ctrl+C copies, but nothing is typed into it (F21).
            let Some((page, _)) = file.page() else {
                return;
            };
            let picture = file.image.is_some();
            let target = match key.code {
                KeyCode::PageDown => Some(page + 1),
                KeyCode::PageUp => Some(page.saturating_sub(1)),
                KeyCode::Home => Some(0),
                KeyCode::End => Some(usize::MAX),
                KeyCode::Right | KeyCode::Down | KeyCode::Char(' ') if picture => Some(page + 1),
                KeyCode::Left | KeyCode::Up if picture => Some(page.saturating_sub(1)),
                _ => None,
            };
            if let Some(target) = target {
                file.show_page(target, &mut self.picker);
                return;
            }
            let step = match key.code {
                KeyCode::Up => (-1, 0),
                KeyCode::Down => (1, 0),
                KeyCode::Left => (0, -1),
                KeyCode::Right => (0, 1),
                _ => (0, 0),
            };
            match key.code {
                _ if picture => {}
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
                    file.mark(key.modifiers.contains(KeyModifiers::SHIFT));
                    file.move_cursor(step.0, step.1);
                }
                KeyCode::Char('a' | 'A') if ctrl => file.select_all(),
                KeyCode::Char('c' | 'C') if ctrl => {
                    if let Some(text) = file.selected_text() {
                        self.copy(text);
                    }
                }
                _ => {}
            }
            return;
        }
        // An open Peek or hover (F31), then the list of suggestions (F26), take their keys
        // first.
        if (self.peek.is_some() || self.hover.is_some()) && self.handle_intelligence_key(key) {
            return;
        }
        if self.completion.is_some() && self.handle_completion_key(key) {
            return;
        }
        // IntelliSense (F26) and debugging (F27); Shift+F5 also arrives as F17 (`Binding`).
        match self.keys.action(key) {
            Some(Action::Suggestions) => return self.request_completion(),
            Some(Action::Definition) => return self.ask_places(Goal::Definition),
            Some(Action::Peek) => return self.ask_places(Goal::Peek),
            Some(Action::References) => return self.ask_places(Goal::References),
            Some(Action::Hover) => return self.ask_hover(),
            Some(Action::Back) => return self.go_back(),
            Some(Action::StopDebug) => return self.stop(),
            Some(Action::Debug) => return self.debug_start_or_continue(),
            Some(Action::Breakpoint) => return self.toggle_breakpoint(),
            Some(Action::StepOver) => return self.debug_step("next"),
            Some(Action::StepInto) => return self.debug_step("stepIn"),
            _ => {}
        }
        let file = &mut self.open_files[active];
        // Ctrl+Z on every system: on a Mac, Cmd+Z stays with the terminal.
        if ctrl && matches!(key.code, KeyCode::Char('z' | 'Z')) {
            if !file.undo() {
                self.status = "Nothing to undo".into();
            }
            return;
        }
        if ctrl && matches!(key.code, KeyCode::Char('v' | 'V')) {
            self.paste_clipboard();
            return;
        }
        let mut copied = None;
        match key.code {
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End => {
                // Shift selects as the cursor moves; without it the selection ends.
                file.mark(key.modifiers.contains(KeyModifiers::SHIFT));
                match key.code {
                    KeyCode::Up => file.move_cursor(-1, 0),
                    KeyCode::Down => file.move_cursor(1, 0),
                    KeyCode::Left => file.move_cursor(0, -1),
                    KeyCode::Right => file.move_cursor(0, 1),
                    KeyCode::Home => file.move_cursor_to(file.cursor_row, 0),
                    _ => {
                        let column = file.lines[file.cursor_row].chars().count();
                        file.move_cursor_to(file.cursor_row, column);
                    }
                }
            }
            KeyCode::PageUp => file.scroll = file.scroll.saturating_sub(10),
            KeyCode::PageDown => file.scroll = file.scroll.saturating_add(10),
            KeyCode::Enter => file.insert('\n'),
            KeyCode::Tab => file.insert('\t'),
            KeyCode::Backspace => file.backspace(),
            KeyCode::Delete => file.delete(),
            KeyCode::Char('a' | 'A') if ctrl => file.select_all(),
            KeyCode::Char('c' | 'C') if ctrl => copied = file.selected_text(),
            KeyCode::Char('x' | 'X') if ctrl => {
                copied = file.selected_text();
                file.delete_selection();
            }
            KeyCode::Char(character) if !ctrl => {
                file.insert(character);
                self.typed(character);
            }
            _ => {}
        }
        if let Some(text) = copied {
            self.copy(text);
        }
    }

    fn keep_selection_visible(&mut self) {
        let height = self.regions.explorer.height.saturating_sub(2) as usize;
        if height == 0 {
            return;
        }
        if self.selected_entry < self.explorer_scroll {
            self.explorer_scroll = self.selected_entry;
        } else if self.selected_entry >= self.explorer_scroll + height {
            self.explorer_scroll = self.selected_entry + 1 - height;
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.splash_visible && !self.motion.splash_closing() {
            if mouse.kind != MouseEventKind::Moved {
                self.dismiss_splash();
            }
            return;
        }
        let point = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                self.selecting = false;
                self.terminal_selection = None;
                // A click in a hover or Peek box keeps it; anywhere else closes it (F31).
                if self.peek.is_some() || self.hover.is_some() {
                    if contains(self.regions.peek, point) || contains(self.regions.hover, point) {
                        return;
                    }
                    self.peek = None;
                    self.hover = None;
                }
                if self.name_prompt.is_some() {
                    return;
                }
                if self.properties.is_some() {
                    if !contains(self.regions.properties, point) {
                        self.properties = None;
                    }
                    return;
                }
                // The command dialog takes Enter or Esc; a project is chosen with a click.
                if self.confirm.is_some() {
                    return;
                }
                if self.project_choice.is_some() {
                    match self
                        .regions
                        .choice_rows
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                    {
                        Some((index, _)) => self.choose_project(Some(*index)),
                        None => self.project_choice = None,
                    }
                    return;
                }
                if self.entry_menu.is_some() {
                    if let Some(row) = self
                        .regions
                        .preview_rows
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .map(|(row, _)| *row)
                    {
                        self.choose_preview_row(row);
                        return;
                    }
                    let action = self
                        .regions
                        .entry_menu_items
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .map(|(action, _)| *action);
                    match action {
                        Some(action) => self.choose_entry_action(action),
                        None => self.entry_menu = None,
                    }
                    return;
                }
                if !contains(self.regions.editor, point) {
                    self.flush_active_file();
                }
                if let Some(divider) = self
                    .regions
                    .tiling
                    .dividers
                    .iter()
                    .find(|divider| contains(divider.handle, point))
                    .cloned()
                    .filter(|_| self.overlay == Overlay::None)
                {
                    self.drag = Some(divider);
                    return;
                }
                if contains(self.regions.header_help, point) {
                    self.open_overlay(Overlay::Help);
                    return;
                }
                if contains(self.regions.header_about, point) {
                    self.open_overlay(Overlay::About);
                    return;
                }
                if self.search.is_some() {
                    if self.motion.search_closing() {
                        return;
                    }
                    if contains(self.regions.search_previous, point) {
                        self.step_match(false);
                        return;
                    }
                    if contains(self.regions.search_next, point) {
                        self.step_match(true);
                        return;
                    }
                    if let Some((index, _)) = self
                        .regions
                        .search_match_items
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                    {
                        if let Some(search) = &mut self.search {
                            search.selected = *index;
                        }
                        self.handle_search_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    }
                    return;
                }
                if self.overlay == Overlay::Update && !self.motion.overlay_closing() {
                    if contains(self.regions.update_install, point) {
                        self.handle_update_key(KeyCode::Enter);
                    } else if contains(self.regions.update_skip, point) {
                        self.handle_update_key(KeyCode::Char('s'));
                    }
                    return;
                }
                if self.overlay == Overlay::Help && !self.motion.overlay_closing() {
                    // The tabs, then the Dependencies rows (F33).
                    if let Some((tab, _)) = self
                        .regions
                        .help_tabs
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                    {
                        match tab {
                            HelpTab::Shortcuts => self.help_tab = tab,
                            HelpTab::Dependencies => self.open_dependencies(),
                        }
                        return;
                    }
                    if self.help_tab == HelpTab::Dependencies {
                        let row = self
                            .regions
                            .dependency_rows
                            .iter()
                            .find(|(_, area)| contains(*area, point))
                            .map(|(row, _)| *row);
                        if let (Some(row), Some(dependencies)) = (row, &mut self.dependencies) {
                            // A click chooses the row; a click on the chosen one asks.
                            if dependencies.selected == row {
                                self.ask_to_install(Some(row));
                            } else {
                                dependencies.selected = row;
                            }
                        }
                        return;
                    }
                    // A command's key waits for new keys; Reset asks first (F29).
                    if contains(self.regions.help_reset, point) {
                        self.ask_to_reset_shortcuts();
                    } else if let Some((action, _)) = self
                        .regions
                        .help_keys
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                    {
                        self.choose_shortcut(action);
                    } else {
                        self.key_editor.capturing = false;
                        self.key_editor.confirm_reset = false;
                    }
                    return;
                }
                if self.overlay == Overlay::WhatsNew
                    && !self.motion.overlay_closing()
                    && let Some((filter, _)) = self
                        .regions
                        .notes_filters
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                {
                    self.filter_notes(filter);
                    return;
                }
                if self.overlay == Overlay::Setup
                    && !self.motion.overlay_closing()
                    && let Some((row, _)) = self
                        .regions
                        .dependency_rows
                        .iter()
                        .find(|(_, area)| contains(*area, point))
                        .copied()
                {
                    self.click_setup_row(row);
                    return;
                }
                if self.overlay != Overlay::None && !self.motion.overlay_closing() {
                    let regions = &self.regions;
                    let key = if contains(regions.about_repository, point) {
                        Some(KeyCode::Char('g'))
                    } else if contains(regions.about_update, point) {
                        Some(KeyCode::Char('u'))
                    } else if contains(regions.about_notes, point) {
                        Some(KeyCode::Char('n'))
                    } else if contains(regions.dialog_next, point) {
                        Some(KeyCode::Enter)
                    } else if contains(regions.dialog_skip, point) {
                        Some(KeyCode::Esc)
                    } else {
                        None
                    };
                    if let Some(key) = key {
                        self.handle_overlay_key(KeyEvent::from(key));
                    }
                    return;
                }
                if self.overlay != Overlay::None {
                    return;
                }
                if let Some(divider) = self.divider_beside(point) {
                    self.drag = Some(divider);
                    return;
                }
                if let Some(panel) = self.panel_handle_at(point) {
                    self.focus = match panel {
                        Panel::Explorer => Focus::Explorer,
                        Panel::Editor if self.show_git => Focus::Git,
                        Panel::Editor if self.show_database => Focus::Database,
                        Panel::Editor => Focus::Editor,
                        Panel::BottomTerminal => Focus::BottomTerminal,
                        Panel::RightTerminal => Focus::RightTerminal,
                    };
                    self.panel_drag = Some(PanelDrag {
                        panel,
                        from: point,
                        zone: None,
                    });
                    return;
                }
                if self.show_database && contains(self.database.regions.area, point) {
                    self.focus = Focus::Database;
                    self.database.click(point);
                    return;
                }
                if self.show_git && contains(self.git.regions.area, point) {
                    self.focus = Focus::Git;
                    self.git.click(point);
                    return;
                }
                if self.show_explorer && contains(self.regions.explorer, point) {
                    self.focus = Focus::Explorer;
                    let row = mouse.row.saturating_sub(self.regions.explorer.y + 1) as usize;
                    let index = self.explorer_scroll + row;
                    if index < self.entries.len() {
                        self.selected_entry = index;
                        self.activate_selected();
                    }
                } else if let Some((pane, right, _)) = self
                    .regions
                    .tab_arrows
                    .iter()
                    .find(|(_, _, area)| contains(*area, point))
                    .copied()
                {
                    self.scroll_tabs(pane, right);
                } else if let Some((index, tab, close)) = self
                    .regions
                    .tab_items
                    .iter()
                    .find(|(_, tab, close)| contains(*tab, point) || contains(*close, point))
                    .cloned()
                {
                    self.focus = Focus::Editor;
                    if contains(close, point) {
                        self.close_tab(index);
                    } else if contains(tab, point) {
                        self.tab_drag = Some(TabDrag {
                            tab: index,
                            previous: self.active_tab,
                            zone: None,
                        });
                        self.select_tab(index);
                    }
                } else if let Some((panel, index, _, close)) = self
                    .regions
                    .terminal_tabs
                    .iter()
                    .find(|(_, _, tab, close)| contains(*tab, point) || contains(*close, point))
                    .copied()
                {
                    // A terminal's tab shows its shell; its `x` closes it (F04).
                    if contains(close, point) {
                        self.close_terminal(index);
                    } else {
                        self.terminals(panel).select(index);
                        self.focus = panel;
                    }
                } else if contains(self.regions.terminal_add, point) {
                    self.add_terminal();
                } else if self.show_bottom_terminal && contains(self.regions.bottom_terminal, point)
                {
                    self.press_terminal(Focus::BottomTerminal, mouse, point);
                } else if self.show_right_terminal && contains(self.regions.right_terminal, point) {
                    self.press_terminal(Focus::RightTerminal, mouse, point);
                } else if contains(self.regions.editor_body, point) {
                    self.focus = Focus::Editor;
                    if self.show_git || self.show_database {
                        return;
                    }
                    let Some(pane) = self.pane_under(point) else {
                        return;
                    };
                    if pane != self.active_pane()
                        && let Some(split) = self.split
                    {
                        self.flush_active_file();
                        self.select_tab(split.other);
                    }
                    let area = self.regions.pane_bodies[pane];
                    if let Some(index) = self.active_tab.filter(|index| {
                        self.open_files
                            .get(*index)
                            .and_then(|file| file.media.as_ref())
                            .is_some_and(|media| media.video().is_some())
                    }) {
                        return self.click_video(index, area, point);
                    }
                    if self
                        .active_tab
                        .and_then(|index| self.open_files.get(index))
                        .is_some_and(|file| file.media.is_some())
                    {
                        self.turning = Some(point);
                        return;
                    }
                    if let Some(file) = self
                        .active_tab
                        .and_then(|index| self.open_files.get_mut(index))
                        && file.shows_text()
                        && contains(area, point)
                    {
                        // A click starts a selection that a drag makes longer; Shift extends
                        // the one there is.
                        let extend = mouse.modifiers.contains(KeyModifiers::SHIFT);
                        let (row, visual_column) = text_position(file, area, point);
                        let column = file.cursor_column_for_click(row, visual_column);
                        // A `[[ ]]` opens its document (F40).
                        if mouse.modifiers.is_empty()
                            && let Some(target) = file.wiki_link_at(row, column)
                        {
                            return self.open_wiki_link(&target);
                        }
                        file.mark(extend);
                        file.move_cursor_to(row, column);
                        if !extend {
                            file.anchor = Some((file.cursor_row, file.cursor_column));
                        }
                        // Ctrl+click goes to the definition, where the terminal sends the Ctrl
                        // with the click (F31).
                        if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                            file.anchor = None;
                            self.ask_places(Goal::Definition);
                        } else {
                            self.selecting = true;
                        }
                    }
                }
            }
            MouseEventKind::Down(button @ (MouseButton::Right | MouseButton::Middle))
                if self.terminal_wants_mouse(point) =>
            {
                let number = u8::from(button == MouseButton::Right) + 1;
                if let Some(panel) = self.terminal_at(point) {
                    self.focus = panel;
                    self.terminal_mouse = Some((panel, number));
                    self.mouse_to_terminal(panel, MouseAction::Press(number), mouse);
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                let explorer = self.regions.explorer;
                let free = self.overlay == Overlay::None
                    && self.search.is_none()
                    && self.name_prompt.is_none()
                    && self.properties.is_none()
                    && !self.splash_visible;
                if free && self.show_explorer && contains(explorer, point) && point.1 > explorer.y {
                    self.focus = Focus::Explorer;
                    let index = self.explorer_scroll + usize::from(point.1 - explorer.y - 1);
                    self.open_entry_menu((index < self.entries.len()).then_some(index), point);
                } else if free
                    && let Some(index) = self
                        .pane_at(point)
                        .filter(|index| self.open_files[*index].is_text())
                {
                    if Some(index) != self.active_tab {
                        self.flush_active_file();
                        self.select_tab(index);
                    }
                    self.focus = Focus::Editor;
                    // Without a selection the click puts the cursor on the word the menu's
                    // questions are about (F31).
                    if let Some((_, row, column)) = self.text_under(point) {
                        let file = &mut self.open_files[index];
                        if file.selection().is_none() {
                            file.move_cursor_to(row, column);
                        }
                    }
                    self.open_editor_menu(point);
                } else {
                    self.entry_menu = None;
                }
            }
            MouseEventKind::Drag(_) if self.terminal_mouse.is_some() => {
                if let Some((panel, button)) = self.terminal_mouse {
                    self.mouse_to_terminal(panel, MouseAction::Motion(Some(button)), mouse);
                }
            }
            MouseEventKind::Up(_) if self.terminal_mouse.is_some() => {
                if let Some((panel, button)) = self.terminal_mouse.take() {
                    self.mouse_to_terminal(panel, MouseAction::Release(button), mouse);
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left)
                if self.panel_drag.is_some() =>
            {
                self.drag_panel(point);
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.scrubbing => {
                self.scrub_video(point, false);
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.turning.is_some() => {
                let (column, row) = self.turning.replace(point).unwrap_or(point);
                if let Some(media) = self
                    .active_tab
                    .and_then(|index| self.open_files.get_mut(index))
                    .and_then(|file| file.media.as_mut())
                {
                    let columns = i32::from(point.0) - i32::from(column);
                    media.drag(columns, i32::from(point.1) - i32::from(row));
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.terminal_dragging => {
                if let Some(selection) = self.terminal_selection {
                    let area = self.terminal_area(selection.panel);
                    let end = clamped_cell(area, point);
                    self.terminal_selection = Some(TerminalSelection { end, ..selection });
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.selecting => {
                let area = self.regions.pane_bodies[self.active_pane()];
                if let Some(file) = self
                    .active_tab
                    .and_then(|index| self.open_files.get_mut(index))
                    && file.shows_text()
                {
                    // Past the top or the bottom of the text the view follows the selection.
                    let (row, visual_column) = if point.1 <= area.y {
                        let display = (file.scroll as usize).saturating_sub(1);
                        (file.source_row(display).unwrap_or(0), 0)
                    } else {
                        text_position(file, area, point)
                    };
                    let column = file.cursor_column_for_click(row, visual_column);
                    file.move_cursor_to(row, column);
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left)
                if self.drag.is_none() && self.tab_drag.is_some() =>
            {
                let body = self.regions.editor_body;
                let editor = !self.show_git && !self.show_database;
                let pane = (0..2).find(|pane| contains(self.regions.pane_areas[*pane], point));
                let several = self.open_files.len() > 1;
                if let Some(drag) = &mut self.tab_drag {
                    let from = self.open_files.get(drag.tab).map(|file| file.pane);
                    drag.zone = match self.split {
                        // Split: another pane takes the tab.
                        Some(_) => pane
                            .filter(|pane| editor && Some(*pane) != from)
                            .map(DropZone::Pane),
                        None => (editor && several && contains(body, point))
                            .then(|| DropZone::at(body, point)),
                    };
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => self.drag_divider(point),
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                if self.scrubbing {
                    self.scrub_video(point, true);
                }
                if let Some(drag) = self.panel_drag.take()
                    && let Some((target, zone)) = drag.zone
                {
                    self.tiles = self.tiles.clone().moved(drag.panel, target, zone);
                }
                self.drag = None;
                self.selecting = false;
                self.turning = None;
                if std::mem::take(&mut self.terminal_dragging) {
                    self.copy_terminal_selection();
                }
                if let Some(drag) = self.tab_drag.take()
                    && let Some(zone) = drag.zone
                {
                    self.drop_tab(drag, zone);
                }
            }
            MouseEventKind::Moved if self.entry_menu.is_some() => self.menu_mouse(point),
            MouseEventKind::Moved => {
                if let Some(panel) = self.terminal_at(point) {
                    self.mouse_to_terminal(panel, MouseAction::Motion(None), mouse);
                }
                self.hover(point);
                let at = self.text_under(point);
                let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
                self.mark_link(at, ctrl);
                self.rest_mouse(point, at, ctrl);
            }
            // Shift turns the wheel sideways, for terminals without a horizontal wheel.
            MouseEventKind::ScrollUp => self.scroll_at(point, true, sideways(mouse)),
            MouseEventKind::ScrollDown => self.scroll_at(point, false, sideways(mouse)),
            MouseEventKind::ScrollLeft => self.scroll_at(point, true, true),
            MouseEventKind::ScrollRight => self.scroll_at(point, false, true),
            _ => {}
        }
    }

    fn scroll_at(&mut self, point: (u16, u16), up: bool, sideways: bool) {
        // The hover and Peek boxes scroll under the mouse (F31).
        if let Some(hover) = self
            .hover
            .as_mut()
            .filter(|_| contains(self.regions.hover, point))
        {
            hover.scroll = if up {
                hover.scroll.saturating_sub(1)
            } else {
                (hover.scroll + 1).min(hover.overflow)
            };
            return;
        }
        if let Some(peek) = self
            .peek
            .as_mut()
            .filter(|_| contains(self.regions.peek, point))
        {
            peek.top = if up {
                peek.top.saturating_sub(3)
            } else {
                (peek.top + 3).min(peek.lines.len().saturating_sub(1))
            };
            return;
        }
        if matches!(self.overlay, Overlay::Help | Overlay::WhatsNew) {
            self.overlay_scroll = if up {
                self.overlay_scroll.saturating_sub(3)
            } else {
                self.overlay_scroll.saturating_add(3)
            };
            return;
        }
        if let Some(pane) = (0..2).find(|pane| contains(self.regions.tab_bars[*pane], point)) {
            self.scroll_tabs(pane, !up);
        } else if self.show_database && contains(self.database.regions.area, point) {
            self.database.scroll(point, up, sideways);
        } else if self.show_git && contains(self.git.regions.content, point) {
            self.git.scroll(up);
        } else if contains(self.regions.explorer, point) && sideways {
            // Long names scroll sideways, as far as the widest one (F01).
            let room = usize::from(self.regions.explorer.width.saturating_sub(2));
            let most = self.explorer_width_needed().saturating_sub(room);
            self.explorer_scroll_x = if up {
                self.explorer_scroll_x.saturating_sub(6)
            } else {
                self.explorer_scroll_x.saturating_add(6).min(most)
            };
        } else if contains(self.regions.explorer, point) {
            self.explorer_scroll = if up {
                self.explorer_scroll.saturating_sub(3)
            } else {
                self.explorer_scroll
                    .saturating_add(3)
                    .min(self.entries.len().saturating_sub(1))
            };
        } else if let Some(panel) = [Focus::BottomTerminal, Focus::RightTerminal]
            .into_iter()
            .find(|panel| contains(self.terminal_area(*panel), point))
        {
            self.terminal_selection = None;
            let cell = clamped_cell(self.terminal_area(panel), point);
            self.terminals(panel)
                .current_mut()
                .wheel(up, cell, KeyModifiers::NONE);
        } else if let Some(index) = self.pane_at(point) {
            // The wheel zooms a model; pictures and videos do not scroll.
            if let Some(media) = &mut self.open_files[index].media {
                media.wheel(up);
                return;
            }
            let file = &self.open_files[index];
            // A page of text scrolls first and turns at its top or its end (F21).
            let height = self.regions.pane_bodies[file.pane.min(1)]
                .height
                .saturating_sub(2);
            let turns = file.image.is_some()
                || if up {
                    file.scroll == 0
                } else {
                    usize::from(file.scroll) + usize::from(height) >= file.display_rows()
                };
            if let Some((page, _)) = file.page().filter(|_| turns && !sideways) {
                // One page per turn of the wheel, however many events it sends.
                let now = Instant::now();
                if self
                    .page_turned
                    .is_some_and(|last| now.duration_since(last) < Duration::from_millis(250))
                {
                    return;
                }
                let page = if up { page.saturating_sub(1) } else { page + 1 };
                self.open_files[index].show_page(page, &mut self.picker);
                // The gesture counts from when the page is on screen: drawing a page can take
                // longer than the gesture itself on a slow machine (E27).
                self.page_turned = Some(Instant::now());
                return;
            }
            // The editor clamps both scrolls to the text when it draws.
            let file = &mut self.open_files[index];
            let scroll = if sideways {
                &mut file.scroll_x
            } else {
                &mut file.scroll
            };
            let step = if sideways { 6 } else { 3 };
            *scroll = if up {
                scroll.saturating_sub(step)
            } else {
                scroll.saturating_add(step)
            };
        }
    }

    /// A click on a video (Q52): a button does what it says, the progress bar starts a seek,
    /// the picture plays or pauses.
    fn click_video(&mut self, index: usize, area: Rect, point: (u16, u16)) {
        let [picture, progress, buttons] = video_areas(area);
        let Some(video) = self.open_files[index]
            .media
            .as_ref()
            .and_then(|media| media.video())
        else {
            return;
        };
        if contains(progress, point) {
            if video
                .fraction_at(progress.width, point.0 - progress.x)
                .is_some()
            {
                self.scrubbing = true;
                self.scrub_video(point, false);
            }
        } else if contains(buttons, point) {
            if let Some(control) = video.button_at(point.0 - buttons.x) {
                self.control_video(index, control);
            }
        } else if contains(picture, point) {
            self.control_video(index, Control::Play);
        }
    }

    /// The mouse holds the progress bar at `point`, kept on the bar; letting go seeks there.
    fn scrub_video(&mut self, point: (u16, u16), release: bool) {
        self.scrubbing = !release;
        let Some(index) = self.active_tab else {
            return;
        };
        let pane = self.open_files[index].pane.min(1);
        let [_, progress, _] = video_areas(self.regions.pane_bodies[pane]);
        if let Some(media) = self.open_files[index].media.as_mut()
            && let Some(video) = media.video()
        {
            let (_, start, length, _, _) = video.progress(progress.width);
            let column = point
                .0
                .saturating_sub(progress.x)
                .clamp(start, start + length.saturating_sub(1));
            if let Some(fraction) = video.fraction_at(progress.width, column) {
                media.scrub(fraction, release);
            }
        }
    }

    /// The area of the terminal panel `panel` names.
    fn terminal_area(&self, panel: Focus) -> Rect {
        if panel == Focus::RightTerminal {
            self.regions.right_terminal
        } else {
            self.regions.bottom_terminal
        }
    }

    /// The terminal drawn under `point`, inside its border.
    fn terminal_at(&self, point: (u16, u16)) -> Option<Focus> {
        if self.overlay != Overlay::None {
            return None;
        }
        [Focus::BottomTerminal, Focus::RightTerminal]
            .into_iter()
            .find(|panel| contains(inner_area(self.terminal_area(*panel)), point))
    }

    /// Whether the program in the terminal under `point` asked for the mouse (F37).
    fn terminal_wants_mouse(&self, point: (u16, u16)) -> bool {
        self.terminal_at(point).is_some_and(|panel| {
            let terminals = if panel == Focus::RightTerminal {
                &self.right_terminal
            } else {
                &self.bottom_terminal
            };
            terminals.current().wants_mouse()
        })
    }

    fn mouse_to_terminal(&mut self, panel: Focus, action: MouseAction, mouse: MouseEvent) {
        let cell = clamped_cell(self.terminal_area(panel), (mouse.column, mouse.row));
        self.terminals(panel)
            .current_mut()
            .send_mouse(action, cell, mouse.modifiers);
    }

    /// A left press in a terminal: to the program that asked for the mouse, unless Shift says
    /// to choose text, as terminals do (F37, Q57).
    fn press_terminal(&mut self, panel: Focus, mouse: MouseEvent, point: (u16, u16)) {
        self.focus = panel;
        if self.terminal_wants_mouse(point) && !mouse.modifiers.contains(KeyModifiers::SHIFT) {
            self.terminal_mouse = Some((panel, 0));
            self.mouse_to_terminal(panel, MouseAction::Press(0), mouse);
        } else {
            self.start_terminal_selection(panel, point);
        }
    }

    /// Whether a tab, an arrow or a button of a tab bar is under `point`.
    fn tab_bar_button_at(&self, point: (u16, u16)) -> bool {
        let regions = &self.regions;
        regions
            .tab_items
            .iter()
            .any(|(_, tab, close)| contains(*tab, point) || contains(*close, point))
            || regions
                .tab_arrows
                .iter()
                .any(|(_, _, area)| contains(*area, point))
            || regions
                .terminal_tabs
                .iter()
                .any(|(_, _, tab, close)| contains(*tab, point) || contains(*close, point))
            || contains(regions.terminal_add, point)
    }

    /// The panel that can be taken at `point` (F39): by the title on its top border, by a
    /// corner of its border, or, for the editor, by the free part of its tab bar.
    fn panel_handle_at(&self, point: (u16, u16)) -> Option<Panel> {
        if self.tab_bar_button_at(point) {
            return None;
        }
        self.regions.tiling.panels.iter().find_map(|(panel, area)| {
            let rows = if *panel == Panel::Editor { 2 } else { 1 };
            let title = Rect::new(area.x, area.y, area.width, area.height.min(rows));
            let bottom_corner =
                point.1 + 1 == area.bottom() && (point.0 == area.x || point.0 + 1 == area.right());
            (contains(title, point) || (contains(*area, point) && bottom_corner)).then_some(*panel)
        })
    }

    /// The divider next to the side or bottom border of a panel under `point`: those borders
    /// resize as the divider does (F39). The corners and the top border move the panel.
    fn divider_beside(&self, point: (u16, u16)) -> Option<super::layout::Divider> {
        if self.tab_bar_button_at(point) || self.panel_handle_at(point).is_some() {
            return None;
        }
        let regions = &self.regions;
        let (_, area) = regions
            .tiling
            .panels
            .iter()
            .find(|(_, area)| contains(*area, point))?;
        let beside = |handle: Rect| {
            let (x, y) = point;
            let in_rows = y >= handle.y && y < handle.bottom();
            let in_columns = x >= handle.x && x < handle.right();
            (x == area.x && handle.right() == area.x && in_rows)
                || (x + 1 == area.right() && handle.x == area.right() && in_rows)
                || (y + 1 == area.bottom() && handle.y == area.bottom() && in_columns)
        };
        regions
            .tiling
            .dividers
            .iter()
            .find(|divider| beside(divider.handle))
            .cloned()
    }

    /// Where the panel held would end up, in the layout it would make (F39).
    pub fn panel_landing(&self) -> Option<Rect> {
        let drag = self.panel_drag?;
        let (target, zone) = drag.zone?;
        let tiles = self.tiles.clone().moved(drag.panel, target, zone);
        let length = |panel, direction| self.panel_length(panel, direction);
        Some(tiles.place(self.regions.body, &length).area(drag.panel))
    }

    /// Where a panel held at `point` lands: an edge of the window takes that whole edge, a
    /// side of another panel shares its place, its centre changes places with it.
    fn panel_drop(&self, panel: Panel, point: (u16, u16)) -> Option<(Option<Panel>, DropZone)> {
        let body = self.regions.body;
        if !contains(body, point) {
            return None;
        }
        let edge = if point.0 == body.x {
            Some(DropZone::Left)
        } else if point.0 + 1 == body.right() {
            Some(DropZone::Right)
        } else if point.1 == body.y {
            Some(DropZone::Top)
        } else if point.1 + 1 == body.bottom() {
            Some(DropZone::Bottom)
        } else {
            None
        };
        if let Some(zone) = edge {
            return Some((None, zone));
        }
        let (under, area) = self
            .regions
            .tiling
            .panels
            .iter()
            .find(|(_, area)| contains(*area, point))?;
        (*under != panel).then(|| (Some(*under), DropZone::at(*area, point)))
    }

    fn drag_panel(&mut self, point: (u16, u16)) {
        let Some(mut drag) = self.panel_drag else {
            return;
        };
        if point != drag.from {
            drag.zone = self.panel_drop(drag.panel, point);
            self.status = format!(
                "{}: drop on a side of a panel, its centre or the edge of the window",
                drag.panel.name()
            );
        }
        self.panel_drag = Some(drag);
    }

    /// A divider held with the mouse: its fixed side takes the length to the pointer.
    fn drag_divider(&mut self, point: (u16, u16)) {
        let Some(divider) = self.drag.clone() else {
            return;
        };
        let Some(direction) = divider.direction else {
            return;
        };
        let (position, start, end) = match direction {
            SplitDirection::Columns => (point.0, divider.area.x, divider.area.right()),
            SplitDirection::Rows => (point.1, divider.area.y, divider.area.bottom()),
        };
        let value = if divider.first {
            position.saturating_sub(start)
        } else {
            end.saturating_sub(position + 1)
        };
        let length = |panel, direction| self.panel_length(panel, direction);
        let sized = self.tiles.resized(&divider, value, &length);
        for (panel, value) in sized {
            self.set_panel_length(panel, direction, value);
        }
    }

    /// Opens the document of a `[[ ]]`, or goes to its tab (F40).
    fn open_wiki_link(&mut self, target: &str) {
        let from = self
            .active_tab
            .and_then(|index| self.open_files.get(index))
            .map(|file| file.path.clone());
        let found = crate::editor::find_document(target, from.as_deref(), &self.root);
        let Some(path) = found else {
            self.status = format!("No document for [[{target}]]");
            return;
        };
        let same = |open: &std::path::PathBuf| open.canonicalize().is_ok_and(|open| open == path);
        let path = self
            .open_files
            .iter()
            .find(|file| same(&file.path))
            .map_or_else(|| path.clone(), |file| file.path.clone());
        if let Err(error) = self.open_file(path) {
            self.status = format!("{error:#}");
        }
    }

    /// A press inside a terminal's border starts choosing its text (Q57).
    fn start_terminal_selection(&mut self, panel: Focus, point: (u16, u16)) {
        let inner = inner_area(self.terminal_area(panel));
        if contains(inner, point) {
            let cell = (point.1 - inner.y, point.0 - inner.x);
            self.terminal_selection = Some(TerminalSelection {
                panel,
                anchor: cell,
                end: cell,
            });
            self.terminal_dragging = true;
        }
    }

    /// Letting go after a drag in a terminal copies the text it chose; a click chooses nothing.
    fn copy_terminal_selection(&mut self) {
        let Some(selection) = self.terminal_selection else {
            return;
        };
        if selection.anchor == selection.end {
            self.terminal_selection = None;
            return;
        }
        let text = self
            .terminals(selection.panel)
            .current()
            .text_between(selection.anchor, selection.end);
        // The empty rows under the output are not copied.
        self.copy(text.trim_end_matches('\n').to_string());
    }

    /// The width of the widest row of the Explorer, as `ui::editor` draws it.
    pub fn explorer_width_needed(&self) -> usize {
        self.entries
            .iter()
            .map(|entry| {
                // Indentation, the `+`/`-` marker, the icon and the spaces between them, and
                // Git's mark at the right edge, which the end of the name scrolls clear of.
                let mark = 2 * usize::from(self.git_change(&entry.path).is_some());
                2 * entry.depth + 4 + ratatui::text::Line::from(entry.name.as_str()).width() + mark
            })
            .max()
            .unwrap_or(0)
    }

    /// Moves a tab bar by one tab; it stops at the last one.
    fn scroll_tabs(&mut self, pane: usize, right: bool) {
        let count = self.pane_tabs(pane).len();
        let bar = &mut self.tab_bars[pane];
        bar.scroll = if right {
            (bar.scroll + 1).min(count.saturating_sub(1))
        } else {
            bar.scroll.saturating_sub(1)
        };
    }

    /// The editor pane whose text is under `point`.
    fn pane_under(&self, point: (u16, u16)) -> Option<usize> {
        (0..2).find(|pane| contains(self.regions.pane_bodies[*pane], point))
    }

    /// The file drawn under `point`, in either pane.
    fn pane_at(&self, point: (u16, u16)) -> Option<usize> {
        if self.show_git || self.show_database {
            return None;
        }
        self.pane_under(point)
            .and_then(|pane| self.shown_tab(pane))
            .filter(|index| *index < self.open_files.len())
    }

    /// The file under `point` and the row and character there, when it is over the text of
    /// an editor with nothing drawn on top.
    fn text_under(&self, point: (u16, u16)) -> Option<(usize, usize, usize)> {
        if self.overlay != Overlay::None || self.search.is_some() {
            return None;
        }
        let pane = self.pane_under(point)?;
        let index = self.pane_at(point)?;
        let file = &self.open_files[index];
        let area = self.regions.pane_bodies[pane];
        let gutter = file.lines.len().max(1).to_string().len() as u16 + 3;
        if !file.shows_text() || !contains(inner_area(area), point) || point.0 < area.x + 1 + gutter
        {
            return None;
        }
        let (row, visual_column) = text_position(file, area, point);
        Some((index, row, file.cursor_column_for_click(row, visual_column)))
    }

    /// The row under the mouse shows its Markdown source, like the cursor row.
    fn hover(&mut self, point: (u16, u16)) {
        let pane = self.pane_at(point);
        let area = self
            .pane_under(point)
            .map(|pane| self.regions.pane_bodies[pane])
            .unwrap_or_default();
        for (index, file) in self.open_files.iter_mut().enumerate() {
            file.hover_row = (Some(index) == pane).then(|| text_position(file, area, point).0);
        }
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
    let column = point.0.saturating_sub(text_start) as usize + file.scroll_x as usize;
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{Dependencies, Tile};
    use super::*;

    fn test_app() -> App {
        App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn shortcuts_toggle_all_panels() {
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(!app.show_explorer);
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        assert!(app.show_bottom_terminal);
        let right_terminal_key = if cfg!(any(target_os = "macos", target_os = "linux")) {
            KeyEvent::new(
                KeyCode::Char('b'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            )
        } else {
            KeyEvent::new(
                KeyCode::Char('B'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            )
        };
        app.handle_key(right_terminal_key);
        assert!(!app.show_explorer);
        assert!(app.show_right_terminal);
        // Each one took the focus; out of the terminal the Ctrl keys are the commands (Q81).
        app.handle_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
        assert!(app.show_git);
        assert!(!app.show_database);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(app.show_database);
        assert!(!app.show_git);
    }

    #[test]
    fn mouse_opens_files_and_controls_tabs() {
        let mut app = test_app();
        app.regions.explorer = Rect::new(0, 0, 30, 20);
        let file_index = app
            .entries
            .iter()
            .position(|entry| entry.path == app.root.join("Cargo.toml"))
            .unwrap();
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: file_index as u16 + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.open_files.len(), 1);
        app.open_file(app.root.join("src/main.rs")).unwrap();
        app.regions.explorer = Rect::default();
        app.regions.tab_items = vec![
            (0, Rect::new(0, 0, 12, 1), Rect::new(12, 0, 3, 1)),
            (1, Rect::new(15, 0, 12, 1), Rect::new(27, 0, 3, 1)),
        ];
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 3,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.active_tab, Some(0));
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 13,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.open_files.len(), 1);
    }

    #[test]
    fn mouse_changes_focus_and_scrolls_panels() {
        let mut app = test_app();
        app.open_file(app.root.join("src/main.rs")).unwrap();
        app.show_bottom_terminal = true;
        app.show_right_terminal = true;
        app.regions.editor = Rect::new(30, 2, 60, 20);
        app.regions.editor_body = app.regions.editor;
        app.regions.pane_bodies[0] = app.regions.editor;
        app.regions.bottom_terminal = Rect::new(30, 22, 60, 10);
        app.regions.right_terminal = Rect::new(90, 0, 40, 32);
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.open_files[0].scroll, 3);
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 40,
            row: 25,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 95,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        for (column, row, expected) in [
            (40, 25, Focus::BottomTerminal),
            (95, 5, Focus::RightTerminal),
            (40, 5, Focus::Editor),
        ] {
            app.handle_mouse(MouseEvent {
                kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(app.focus, expected);
        }
    }

    #[test]
    fn dragging_resizers_changes_panel_dimensions() {
        let mut app = test_app();
        app.regions.body = Rect::new(0, 1, 120, 38);
        app.regions.tiling = app.tiles.place(app.regions.body, &|panel, direction| {
            app.panel_length(panel, direction)
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 30,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: 39,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: 39,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.explorer_width, 39);
    }

    /// The app drawn in a window, for the tests that click what is on screen.
    fn drawn(columns: u16, rows: u16) -> (App, ratatui::Terminal<ratatui::backend::TestBackend>) {
        let app = test_app();
        let terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(columns, rows)).unwrap();
        (app, terminal)
    }

    fn draw(app: &mut App, terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>) {
        app.tick();
        let (explorer, bottom) = (app.show_explorer, app.show_bottom_terminal);
        app.motion
            .settle_panels(explorer, bottom, app.show_right_terminal);
        terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
    }

    fn show_all(app: &mut App, terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>) {
        app.show_explorer = true;
        app.show_bottom_terminal = true;
        app.show_right_terminal = true;
        draw(app, terminal);
        draw(app, terminal);
    }

    fn drag_panel(app: &mut App, from: (u16, u16), to: (u16, u16)) {
        mouse(app, MouseEventKind::Down(MouseButton::Left), from.0, from.1);
        mouse(app, MouseEventKind::Drag(MouseButton::Left), to.0, to.1);
        assert!(app.panel_drag.is_some_and(|drag| drag.zone.is_some()));
        mouse(app, MouseEventKind::Up(MouseButton::Left), to.0, to.1);
        assert!(app.panel_drag.is_none());
    }

    #[test]
    fn a_panel_held_by_its_title_lands_where_it_is_dropped() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        let right = app.regions.right_terminal;
        let bottom = app.regions.bottom_terminal;
        // A click on a title only gives the focus.
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            right.x + 2,
            right.y,
        );
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            right.x + 2,
            right.y,
        );
        assert_eq!(app.focus, Focus::RightTerminal);
        assert_eq!(app.tiles, Tile::default());

        // The Explorer on top of the right terminal: the rest follows on the next frame.
        let title = app.regions.explorer.y;
        drag_panel(
            &mut app,
            (2, title),
            (right.x + right.width / 2, right.y + 2),
        );
        draw(&mut app, &mut terminal);
        assert_eq!(app.regions.explorer.x, right.x);
        assert!(app.regions.right_terminal.y > app.regions.explorer.y);
        assert_eq!(app.regions.editor.x, 0, "the editor takes the room it left");

        // The bottom terminal over the whole top of the window.
        let (title, top) = (app.regions.bottom_terminal.y, app.regions.body.y);
        drag_panel(&mut app, (bottom.x + 2, title), (40, top));
        draw(&mut app, &mut terminal);
        assert_eq!(app.regions.bottom_terminal.y, app.regions.body.y);
        assert_eq!(app.regions.bottom_terminal.width, 120);
        assert!(app.tiles.is_complete());
    }

    #[test]
    fn the_editor_moves_from_the_free_part_of_its_tab_bar_and_tabs_still_click() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.txt");
        std::fs::write(&path, "text\n").unwrap();
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        app.open_file(path).unwrap();
        draw(&mut app, &mut terminal);
        let editor = app.regions.tiling.area(Panel::Editor);
        let right = app.regions.right_terminal;
        // On the tab it is the tab that moves, not the panel.
        let (_, tab, _) = app.regions.tab_items[0];
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            tab.x + 1,
            tab.y,
        );
        assert!(app.panel_drag.is_none() && app.tab_drag.is_some());
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            tab.x + 1,
            tab.y,
        );

        drag_panel(
            &mut app,
            (editor.right() - 2, editor.y),
            (right.x + right.width / 2, right.y + right.height / 2),
        );
        draw(&mut app, &mut terminal);
        let moved = app.regions.tiling.area(Panel::Editor);
        assert!(moved.x > app.regions.right_terminal.x, "swapped");
        assert_eq!(app.regions.right_terminal.x, editor.x);
    }

    #[test]
    fn the_preview_shows_where_a_panel_really_ends_up() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        let right = app.regions.right_terminal;
        let title = app.regions.explorer;
        for (to, swap) in [
            // A side of a panel, its centre and the edge of the window.
            ((right.x + right.width / 2, right.y + 2), false),
            (
                (right.x + right.width / 2, right.y + right.height / 2),
                true,
            ),
            ((60, app.regions.body.bottom() - 1), false),
        ] {
            let tiles = app.tiles.clone();
            mouse(
                &mut app,
                MouseEventKind::Down(MouseButton::Left),
                2,
                title.y,
            );
            assert_eq!(app.panel_landing(), None, "held, not moved yet");
            mouse(
                &mut app,
                MouseEventKind::Drag(MouseButton::Left),
                to.0,
                to.1,
            );
            let landing = app.panel_landing().unwrap();
            draw(&mut app, &mut terminal);
            let screen = terminal.backend().to_string();
            let name = if swap {
                "Swap with Right terminal"
            } else {
                "Explorer here"
            };
            assert!(screen.contains(name), "{name}: {screen}");
            // Nothing moved yet, and letting go puts it exactly there.
            assert_eq!(app.tiles, tiles);
            mouse(&mut app, MouseEventKind::Up(MouseButton::Left), to.0, to.1);
            draw(&mut app, &mut terminal);
            assert_eq!(app.regions.explorer, landing);
            // Back to the start for the next one.
            app.tiles = Tile::default();
            draw(&mut app, &mut terminal);
        }
    }

    #[test]
    fn a_panel_moves_by_any_corner_of_its_border() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        let bottom = app.regions.bottom_terminal;
        for corner in [
            (bottom.x, bottom.y),
            (bottom.right() - 1, bottom.bottom() - 1),
            (bottom.x, bottom.bottom() - 1),
        ] {
            mouse(
                &mut app,
                MouseEventKind::Down(MouseButton::Left),
                corner.0,
                corner.1,
            );
            assert_eq!(
                app.panel_drag.map(|drag| drag.panel),
                Some(Panel::BottomTerminal),
                "{corner:?}"
            );
            assert!(app.terminal_selection.is_none());
            mouse(
                &mut app,
                MouseEventKind::Up(MouseButton::Left),
                corner.0,
                corner.1,
            );
        }
        // Inside the border the mouse is the terminal's again.
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            bottom.x + 3,
            bottom.y + 3,
        );
        assert!(app.panel_drag.is_none() && app.terminal_selection.is_some());
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            bottom.x + 3,
            bottom.y + 3,
        );
    }

    #[test]
    fn borders_resize_a_panel_in_both_directions_wherever_it_is() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        // The right terminal under the Explorer, then the two as one column on the left.
        app.tiles = app.tiles.clone().moved(
            Panel::RightTerminal,
            Some(Panel::Explorer),
            DropZone::Bottom,
        );
        draw(&mut app, &mut terminal);
        let explorer = app.regions.explorer;
        let right = app.regions.right_terminal;
        assert_eq!(explorer.x, right.x);
        // Its right border makes the column wider, for the two.
        let (x, y) = (right.right() - 1, right.y + 5);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x + 8, y);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x + 8, y);
        draw(&mut app, &mut terminal);
        // The divider comes under the pointer, a column from the border.
        assert_eq!(app.regions.right_terminal.width, right.width + 7);
        assert_eq!(app.regions.explorer.width, right.width + 7);
        // The bottom border of the Explorer makes it taller and the terminal shorter.
        let explorer = app.regions.explorer;
        let right = app.regions.right_terminal;
        let (x, y) = (explorer.x + 4, explorer.bottom() - 1);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x, y + 4);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y + 4);
        draw(&mut app, &mut terminal);
        assert_eq!(app.regions.explorer.height, explorer.height + 3);
        assert_eq!(app.regions.right_terminal.y, right.y + 3);
        assert!(app.panel_drag.is_none());
    }

    #[test]
    fn a_border_with_nothing_beside_it_does_not_take_the_click() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        // The left border of the Explorer: the window ends there.
        let explorer = app.regions.explorer;
        assert!(app.divider_beside((explorer.x, explorer.y + 5)).is_none());
        // The tabs of a terminal keep their clicks on its top border.
        let (_, _, tab, _) = app.regions.terminal_tabs[0];
        assert!(app.panel_handle_at((tab.x, tab.y)).is_none());
        assert!(app.divider_beside((tab.x, tab.y)).is_none());
    }

    #[test]
    fn dividers_resize_panels_on_any_side() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        // The right terminal, by the divider on its left.
        let divider = app.regions.tiling.dividers[1].handle;
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            divider.x,
            5,
        );
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            divider.x - 5,
            5,
        );
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            divider.x - 5,
            5,
        );
        assert_eq!(app.right_terminal_width, 47);
        // The Explorer on top of the window: its divider sets its height.
        app.tiles = app
            .tiles
            .clone()
            .moved(Panel::Explorer, None, DropZone::Top);
        draw(&mut app, &mut terminal);
        let divider = app.regions.tiling.dividers[0].handle;
        assert_eq!(divider.height, 1);
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            5,
            divider.y,
        );
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            5,
            divider.y + 3,
        );
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            5,
            divider.y + 3,
        );
        assert_eq!(app.explorer_height, 15);
        draw(&mut app, &mut terminal);
        assert_eq!(app.regions.explorer.height, 15);
    }

    /// The program in the terminal of `panel` reads what is written to it, in a file.
    fn program_in(app: &mut App, panel: Focus, output: &[u8]) {
        let terminal = app.terminals(panel).current_mut();
        terminal.parser.write().unwrap().process(output);
    }

    #[test]
    fn a_click_goes_to_the_program_that_asked_for_the_mouse_and_shift_chooses_text() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        let area = app.regions.bottom_terminal;
        let (x, y) = (area.x + 3, area.y + 2);
        program_in(
            &mut app,
            Focus::BottomTerminal,
            b"\x1b[?1000h\x1b[?1002h\x1b[?1006h",
        );
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(app.focus, Focus::BottomTerminal);
        assert!(
            app.terminal_selection.is_none(),
            "the program has the click"
        );
        assert_eq!(app.terminal_mouse, Some((Focus::BottomTerminal, 0)));
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x + 1, y);
        assert!(app.terminal_selection.is_none());
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x + 1, y);
        assert_eq!(app.terminal_mouse, None);

        // With Shift, text as in any terminal.
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::SHIFT,
        });
        assert!(app.terminal_selection.is_some());
        assert_eq!(app.terminal_mouse, None);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);

        // The right button also reaches it; a shell keeps its menu-less press.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), x, y);
        assert_eq!(app.terminal_mouse, Some((Focus::BottomTerminal, 2)));
        mouse(&mut app, MouseEventKind::Up(MouseButton::Right), x, y);
        assert_eq!(app.terminal_mouse, None);
    }

    #[test]
    fn the_wheel_does_not_move_the_history_under_a_full_screen_program() {
        let (mut app, mut terminal) = drawn(120, 40);
        show_all(&mut app, &mut terminal);
        let lines = (0..200)
            .map(|index| format!("line {index}\r\n"))
            .collect::<String>();
        program_in(&mut app, Focus::RightTerminal, lines.as_bytes());
        let area = app.regions.right_terminal;
        mouse(&mut app, MouseEventKind::ScrollUp, area.x + 3, area.y + 3);
        assert_eq!(app.right_terminal.current().scrollback(), 3);
        mouse(&mut app, MouseEventKind::ScrollDown, area.x + 3, area.y + 3);
        assert_eq!(app.right_terminal.current().scrollback(), 0);
        program_in(
            &mut app,
            Focus::RightTerminal,
            b"\x1b[?1049h\x1b[?1006h\x1b[?1000h",
        );
        mouse(&mut app, MouseEventKind::ScrollUp, area.x + 3, area.y + 3);
        assert_eq!(app.right_terminal.current().scrollback(), 0);
    }

    #[test]
    fn ctrl_keys_belong_to_the_terminal_with_the_focus_and_f6_leaves_it() {
        let mut app = test_app();
        app.show_bottom_terminal = true;
        app.show_git = false;
        app.focus = Focus::BottomTerminal;
        let ctrl = |letter| KeyEvent::new(KeyCode::Char(letter), KeyModifiers::CONTROL);
        for letter in ['b', 'j', 'g', 'd', 'p'] {
            app.handle_key(ctrl(letter));
            assert_eq!(app.focus, Focus::BottomTerminal, "Ctrl+{letter}");
            assert!(app.show_explorer && app.show_bottom_terminal);
            assert!(!app.show_git && !app.show_database && app.search.is_none());
        }
        // What is not Ctrl and a letter stays the IDE's.
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::Help);
        app.overlay = Overlay::None;
        app.handle_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Editor);
        // Out of the terminal the same keys are the commands again.
        app.handle_key(ctrl('b'));
        assert!(!app.show_explorer);
        // Ctrl+Q always quits.
        app.focus = Focus::BottomTerminal;
        app.handle_key(ctrl('q'));
        assert!(app.should_quit);
    }

    fn tools() -> Vec<crate::update::deps::Component> {
        crate::update::deps::own_tools(crate::update::deps::components(
            "linux",
            |tool| tool == "apt-get",
            &[],
        ))
    }

    fn answered(found: Vec<crate::update::deps::Found>, app: &mut App) {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(found).unwrap();
        app.setup.as_mut().unwrap().set_reader(receiver);
        app.tick_dependencies();
    }

    #[test]
    fn the_setup_dialog_installs_what_is_chosen_after_saying_it_and_can_be_skipped() {
        use crate::update::deps::Found;
        let (mut app, mut terminal) = drawn(120, 40);
        let key = |code| KeyEvent::from(code);
        app.setup_checker = |_| std::sync::mpsc::channel().1;
        app.open_setup(tools());
        assert_eq!(app.overlay, Overlay::Setup);
        // Still checking: nothing to install yet.
        app.handle_key(key(KeyCode::Enter));
        assert!(app.confirm.is_none() && app.status.contains("checking"));
        answered(
            vec![
                Found::Installed(Some("git 2".into())),
                Found::Missing,
                Found::Missing,
                Found::Missing,
            ],
            &mut app,
        );
        draw(&mut app, &mut terminal);
        let screen = terminal.backend().to_string();
        for text in [
            "Best experience",
            "Installed",
            "[x]",
            "Videos in the editor",
            "Dependencies",
        ] {
            assert!(screen.contains(text), "{text}: {screen}");
        }
        // git is there and cannot be chosen; curl is left out by Space, ffmpeg by a click.
        app.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(
            app.setup.as_ref().unwrap().chosen,
            [false, true, true, true]
        );
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Char(' ')));
        let row = app.regions.dependency_rows[2].1;
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            row.x + 1,
            row.y,
        );
        assert_eq!(
            app.setup.as_ref().unwrap().chosen,
            [false, false, false, true]
        );
        app.handle_key(key(KeyCode::Enter));
        let confirm = app.confirm.as_ref().unwrap();
        assert!(
            confirm.line.ends_with("install -y tmux"),
            "{}",
            confirm.line
        );
        // Enter again runs it in the bottom terminal, not before.
        assert!(!app.show_bottom_terminal);
        app.handle_key(key(KeyCode::Enter));
        assert!(app.show_bottom_terminal && app.focus == Focus::BottomTerminal);
        assert_eq!(app.overlay, Overlay::None);

        let mut skipping = test_app();
        skipping.setup_checker = |_| std::sync::mpsc::channel().1;
        skipping.open_setup(tools());
        skipping.handle_key(key(KeyCode::Esc));
        assert!(skipping.motion.overlay_closing());
        assert!(skipping.status.contains("Dependencies"));
        assert!(!skipping.show_bottom_terminal);
    }

    #[test]
    fn a_tool_this_machine_cannot_install_is_not_chosen() {
        use crate::update::deps::Found;
        let mut app = test_app();
        app.setup_checker = |_| std::sync::mpsc::channel().1;
        let mut components = tools();
        components.truncate(1);
        components[0].install = None;
        app.open_setup(components);
        answered(vec![Found::Missing], &mut app);
        let setup = app.setup.as_ref().unwrap();
        assert_eq!(setup.chosen, [false]);
        assert!(!setup.can_install(0));
        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert_eq!(app.setup.as_ref().unwrap().chosen, [false]);
        // Nothing chosen: Enter closes it.
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(app.confirm.is_none() && app.motion.overlay_closing());
    }

    #[test]
    fn t_in_dependencies_turns_tmux_on_only_when_it_can_run() {
        let directory = tempfile::tempdir().unwrap();
        let settings = directory.path().join("update.toml");
        let mut app = test_app();
        app.updater = crate::update::Updater::offering("9.9.9", settings);
        app.dependencies = Some(Dependencies::checked(Vec::new(), None));
        app.tmux_usable = || false;
        app.toggle_tmux();
        assert!(!app.updater.tmux() && app.status.contains("install it"));
        app.tmux_usable = || true;
        app.toggle_tmux();
        assert!(app.updater.tmux() && app.status.contains("tmux on"));
        app.toggle_tmux();
        assert!(!app.updater.tmux() && app.status.contains("tmux off"));
    }

    #[test]
    fn a_click_on_a_wiki_link_opens_its_document_or_goes_to_its_tab() {
        let directory = tempfile::tempdir().unwrap();
        let notes = directory.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(directory.path().join("guide.md"), "# Guide\n").unwrap();
        std::fs::write(notes.join("deep.md"), "# Deep\n").unwrap();
        std::fs::write(
            directory.path().join("index.md"),
            "# Top\n\nSee [[guide|the guide]] and [[deep]] and [[gone]]\n",
        )
        .unwrap();
        let (mut app, mut terminal) = drawn(120, 30);
        app.root = directory.path().to_path_buf();
        app.show_explorer = false;
        app.open_file(directory.path().join("index.md")).unwrap();
        // The cursor elsewhere: the row shows only the text.
        app.open_files[0].move_cursor_to(0, 0);
        draw(&mut app, &mut terminal);
        draw(&mut app, &mut terminal);
        let screen = terminal.backend().to_string();
        assert!(
            screen.contains("See the guide and deep and gone"),
            "{screen}"
        );
        assert!(!screen.contains("[["));

        let click = |app: &mut App, terminal: &mut _, word: &str| {
            draw(app, terminal);
            let (row, line) = terminal_lines(terminal)
                .into_iter()
                .enumerate()
                .find(|(_, line)| line.contains(word))
                .unwrap();
            let column = line.chars().take(line.find(word).unwrap()).count() as u16;
            mouse(app, MouseEventKind::Moved, column, row as u16);
            draw(app, terminal);
            mouse(
                app,
                MouseEventKind::Down(MouseButton::Left),
                column,
                row as u16,
            );
            mouse(
                app,
                MouseEventKind::Up(MouseButton::Left),
                column,
                row as u16,
            );
        };
        click(&mut app, &mut terminal, "the guide");
        assert_eq!(app.open_files.len(), 2);
        assert_eq!(app.open_files[1].name, "guide.md");
        assert_eq!(app.active_tab, Some(1));

        // Open already: the same tab comes on screen.
        app.select_tab(0);
        click(&mut app, &mut terminal, "the guide");
        assert_eq!(app.open_files.len(), 2);
        assert_eq!(app.active_tab, Some(1));

        app.select_tab(0);
        click(&mut app, &mut terminal, "deep");
        assert_eq!(app.open_files[2].name, "deep.md");

        app.select_tab(0);
        click(&mut app, &mut terminal, "gone");
        assert_eq!(app.open_files.len(), 3);
        assert_eq!(app.status, "No document for [[gone]]");
    }

    fn terminal_lines(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Vec<String> {
        terminal
            .backend()
            .to_string()
            .lines()
            .map(|line| line.trim_matches('"').to_owned())
            .collect()
    }

    #[test]
    fn help_about_and_search_shortcuts_work() {
        let mut app = test_app();
        app.open_file(app.root.join("Cargo.toml")).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::Help);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::About);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(app.search.as_ref().unwrap().scope, SearchScope::CurrentFile);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(
            KeyCode::Char('P'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(app.search.as_ref().unwrap().scope, SearchScope::Workspace);
    }

    #[test]
    fn closing_surfaces_keeps_content_until_the_transition_ends() {
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let now = std::time::Instant::now();
        app.tick_at(now);
        assert_eq!(app.overlay, Overlay::Help);
        app.tick_at(now + std::time::Duration::from_millis(200));
        assert_eq!(app.overlay, Overlay::None);

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let now = std::time::Instant::now();
        app.tick_at(now);
        assert!(app.search.is_some());
        app.tick_at(now + std::time::Duration::from_millis(200));
        assert!(app.search.is_none());

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::Help);
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.handle_key(KeyEvent::new(code, modifiers));
    }

    fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        app.handle_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }

    /// The mouse with Ctrl held, as the terminal sends it.
    fn ctrl_mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        app.handle_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::CONTROL,
        });
    }

    #[test]
    fn the_search_follows_its_match_in_the_editor_and_enter_selects_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("long.txt");
        let text = (0..60)
            .map(|line| {
                if line % 20 == 5 {
                    format!("line {line} Needle")
                } else {
                    format!("line {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, text).unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        press(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        app.search_paste("needle");
        let active = app.active_tab.unwrap();
        assert_eq!(app.search.as_ref().unwrap().matches.len(), 3);
        assert_eq!(app.open_files[active].scroll, 2);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.open_files[active].scroll, 22);
        app.step_match(true);
        assert_eq!(app.open_files[active].scroll, 42);
        app.step_match(true);
        assert_eq!(app.open_files[active].scroll, 2);
        app.step_match(false);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let file = &app.open_files[active];
        assert_eq!(file.anchor, Some((45, 8)));
        assert_eq!((file.cursor_row, file.cursor_column), (45, 14));
        assert_eq!(file.selected_text().as_deref(), Some("Needle"));
    }

    #[test]
    fn delete_in_the_explorer_menu_asks_then_removes_and_closes_the_tabs() {
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("docs");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("a.md"), "a").unwrap();
        std::fs::write(directory.path().join("b.txt"), "b").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.show_explorer = true;
        app.regions.explorer = Rect::new(0, 0, 30, 20);
        app.open_file(folder.join("a.md")).unwrap();
        app.editor_paste("!");
        // The project folder has no Delete.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 10);
        assert!(
            !app.entry_menu
                .as_ref()
                .unwrap()
                .actions
                .contains(&EntryAction::Delete)
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        // docs is the first row: Delete comes just before Properties, the last entry.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        assert!(
            app.entry_menu
                .as_ref()
                .unwrap()
                .actions
                .ends_with(&[EntryAction::Delete, EntryAction::Properties])
        );
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.name_prompt.as_ref().unwrap().action,
            EntryAction::Delete
        );
        // Nothing to type, and Esc keeps everything.
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        app.name_paste("y");
        assert!(app.name_prompt.as_ref().unwrap().name.is_empty());
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(folder.exists());
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!folder.exists() && app.name_prompt.is_none());
        // The tab of the file inside closed without saving it back.
        assert!(app.open_files.is_empty());
        app.flush_all_files();
        assert!(!folder.exists());
        assert!(
            app.entries
                .iter()
                .all(|entry| !entry.path.starts_with(&folder))
        );
        assert!(directory.path().join("b.txt").exists());
    }

    #[test]
    fn ctrl_z_undoes_in_the_editor_and_reaches_the_terminals_elsewhere() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        app.focus = Focus::Editor;
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        for character in " two".chars() {
            press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
        }
        let active = app.active_tab.unwrap();
        assert_eq!(app.open_files[active].lines, ["one two"]);
        press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.open_files[active].lines, ["one"]);
        press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.status, "Nothing to undo");
        // In a terminal Ctrl+Z belongs to the shell; the editor keeps its text.
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        app.focus = Focus::BottomTerminal;
        press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(app.open_files[active].lines, ["onex"]);
    }

    #[test]
    fn right_clicks_in_the_explorer_create_and_rename_files_and_folders() {
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("src")).unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.show_explorer = true;
        app.regions.explorer = Rect::new(0, 0, 30, 20);
        // Below the tree: the root, which cannot be renamed.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 10);
        let menu = app.entry_menu.as_ref().unwrap();
        assert_eq!(menu.target, directory.path());
        assert_eq!(
            menu.actions,
            [
                EntryAction::NewFile,
                EntryAction::NewFolder,
                EntryAction::Properties
            ]
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.entry_menu.is_none());

        // On the src folder: a new file goes inside it, opens and is selected.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        assert_eq!(app.entry_menu.as_ref().unwrap().actions.len(), 7);
        app.regions.entry_menu_items = vec![(EntryAction::NewFile, Rect::new(3, 2, 14, 1))];
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 5, 2);
        assert!(app.entry_menu.is_none());
        for character in "main.rs".chars() {
            press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let created = directory.path().join("src/main.rs");
        assert!(created.is_file());
        assert!(app.name_prompt.is_none());
        assert_eq!(app.entries[app.selected_entry].path, created);
        assert_eq!(app.open_files[app.active_tab.unwrap()].path, created);

        // A name that exists keeps the prompt open with the error.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.name_prompt.as_ref().unwrap().action,
            EntryAction::NewFolder
        );
        app.name_paste("main.rs");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.name_prompt.as_ref().unwrap().error.is_some());
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.name_prompt.is_none());

        // Renaming the open file follows it in its tab.
        let row = app
            .entries
            .iter()
            .position(|entry| entry.path == created)
            .unwrap();
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Right),
            3,
            row as u16 + 1,
        );
        // Up from the top goes round to Properties, then Delete, then Rename.
        for _ in 0..3 {
            press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.name_prompt.as_ref().unwrap().name, "main.rs");
        for _ in 0.."main.rs".len() {
            press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        }
        app.name_paste("lib.rs");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let renamed = directory.path().join("src/lib.rs");
        assert!(renamed.is_file() && !created.exists());
        let tab = &app.open_files[app.active_tab.unwrap()];
        assert_eq!(
            (tab.path.as_path(), tab.name.as_str()),
            (renamed.as_path(), "lib.rs")
        );
    }

    #[test]
    fn properties_open_from_the_menu_count_the_folder_and_close_with_esc_or_a_click() {
        use crossterm::event::MouseButton;
        use std::time::{Duration, Instant};
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("docs/sub")).unwrap();
        std::fs::write(root.join("docs/a.txt"), "abc").unwrap();
        std::fs::write(root.join("docs/sub/b.txt"), "hello").unwrap();
        std::fs::write(root.join("top.txt"), "top").unwrap();
        let mut app = App::new(root.to_path_buf()).unwrap();
        app.show_explorer = true;
        app.regions.explorer = Rect::new(0, 0, 30, 20);
        // docs is the first row; Up from the top goes round to Properties, the last entry.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let dialog = app.properties.as_ref().unwrap();
        assert_eq!(dialog.properties.path, root.join("docs"));
        assert_eq!(dialog.properties.kind, crate::app::Kind::Folder);
        // The count arrives in the tick; until then the dialog says it is calculating.
        assert!(dialog.count.is_none());
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.properties.as_ref().unwrap().count.is_none() {
            assert!(Instant::now() < deadline, "the count never came");
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        let count = app.properties.as_ref().unwrap().count.unwrap();
        assert_eq!(
            (count.files, count.folders, count.bytes, count.unreadable),
            (2, 1, 8, 0)
        );
        // Keys other than Esc and Enter, and right-clicks, leave it as it is.
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 2);
        assert!(app.properties.is_some() && app.entry_menu.is_none());
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.properties.is_none());

        // A file: its size at once, nothing to count; a click inside stays, outside closes.
        let row = app
            .entries
            .iter()
            .position(|entry| entry.path == root.join("top.txt"))
            .unwrap();
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Right),
            3,
            row as u16 + 1,
        );
        app.choose_entry_action(EntryAction::Properties);
        let dialog = app.properties.as_ref().unwrap();
        assert_eq!((dialog.properties.size, dialog.count), (Some(3), None));
        app.regions.properties = Rect::new(40, 5, 30, 10);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 45, 8);
        assert!(app.properties.is_some());
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 1);
        assert!(app.properties.is_none());

        // The empty space below the tree is the project folder; Enter closes too.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 15);
        assert_eq!(
            app.entry_menu.as_ref().unwrap().actions.last(),
            Some(&EntryAction::Properties)
        );
        app.choose_entry_action(EntryAction::Properties);
        assert_eq!(app.properties.as_ref().unwrap().properties.path, root);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.properties.is_none());
    }

    #[test]
    fn the_explorer_cuts_copies_and_pastes_with_keys_and_the_menu() {
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("a.txt"), "a").unwrap();
        let mut app = App::new(root.to_path_buf()).unwrap();
        app.regions.explorer = Rect::new(0, 0, 30, 20);
        let select = |app: &mut App, name: &str| {
            app.focus = Focus::Explorer;
            app.selected_entry = app
                .entries
                .iter()
                .position(|entry| entry.path == root.join(name))
                .unwrap();
        };
        // Nothing kept: Paste is not in the menu and Ctrl+V says what to do.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        assert!(
            !app.entry_menu
                .as_ref()
                .unwrap()
                .actions
                .contains(&EntryAction::Paste)
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        select(&mut app, "a.txt");
        press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(app.status.contains("first"));

        // Ctrl+C, Ctrl+V next to the file: a copy with a free name, selected.
        press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(root.join("a copy.txt").is_file());
        assert_eq!(
            app.entries[app.selected_entry].path,
            root.join("a copy.txt")
        );
        // The menu of a folder pastes into it, which opens.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        let menu = app.entry_menu.as_ref().unwrap();
        assert_eq!(menu.target, root.join("docs"));
        assert_eq!(
            menu.actions,
            [
                EntryAction::NewFile,
                EntryAction::NewFolder,
                EntryAction::Cut,
                EntryAction::Copy,
                EntryAction::Paste,
                EntryAction::Rename,
                EntryAction::Delete,
                EntryAction::Properties
            ]
        );
        app.choose_entry_action(EntryAction::Paste);
        assert!(root.join("docs/a.txt").is_file());
        assert!(
            app.entries
                .iter()
                .any(|entry| entry.path == root.join("docs/a.txt"))
        );

        // Ctrl+X moves once, and the open tab follows; never over a file there.
        app.open_file(root.join("a copy.txt")).unwrap();
        select(&mut app, "a.txt");
        press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        select(&mut app, "docs");
        press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(app.status.contains("already exists") && root.join("a.txt").is_file());
        select(&mut app, "a copy.txt");
        press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        select(&mut app, "docs");
        press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(root.join("docs/a copy.txt").is_file() && !root.join("a copy.txt").exists());
        assert_eq!(app.open_files[0].path, root.join("docs/a copy.txt"));
        assert!(app.file_clipboard.is_none(), "a move is done once");
    }

    #[test]
    fn the_editor_menu_and_ctrl_v_cut_copy_and_paste_text() {
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "alpha beta\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path).unwrap();
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        let right_click = |app: &mut App| {
            mouse(app, MouseEventKind::Down(MouseButton::Right), 40, 4);
            let menu = app.entry_menu.as_ref().unwrap();
            assert!(menu.editor);
            menu.actions.clone()
        };
        // Without a selection there is only Paste, and nothing to paste yet; then the
        // questions to the language server (F31).
        use crate::lsp::Goal;
        let code = [
            Goal::Definition,
            Goal::Peek,
            Goal::Declaration,
            Goal::TypeDefinition,
            Goal::Implementation,
            Goal::References,
        ]
        .map(EntryAction::Code);
        assert_eq!(
            right_click(&mut app),
            [&[EntryAction::Paste][..], &code].concat()
        );
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.status, "The clipboard is empty");
        // Without a selection the click put the cursor where it was.
        let file = app.active_file().unwrap();
        assert_eq!((file.cursor_row, file.cursor_column), (0, 5));
        press(&mut app, KeyCode::Home, KeyModifiers::NONE);
        for _ in 0..5 {
            press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        }
        assert_eq!(
            right_click(&mut app),
            [
                &[EntryAction::Cut, EntryAction::Copy, EntryAction::Paste][..],
                &code
            ]
            .concat()
        );
        app.choose_entry_action(EntryAction::Copy);
        assert_eq!(app.clipboard.take().as_deref(), Some("alpha"));
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(app.active_file().unwrap().lines, ["alpha betaalpha"]);
        press(&mut app, KeyCode::Home, KeyModifiers::SHIFT);
        right_click(&mut app);
        app.choose_entry_action(EntryAction::Cut);
        assert_eq!(app.active_file().unwrap().lines, [""]);
        right_click(&mut app);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.active_file().unwrap().lines, ["alpha betaalpha"]);
        press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(
            app.active_file().unwrap().lines,
            [""],
            "a paste undoes at once"
        );
    }

    #[test]
    fn changes_by_other_programs_reach_the_open_tabs_and_the_explorer() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        std::fs::write(root.join("b.txt"), "two\n").unwrap();
        let mut app = App::new(root.to_path_buf()).unwrap();
        app.open_file(root.join("a.txt")).unwrap();
        app.open_file(root.join("b.txt")).unwrap();
        app.editor_paste("!");
        std::fs::write(root.join("a.txt"), "uno\n").unwrap();
        std::fs::write(root.join("b.txt"), "dos dos\n").unwrap();
        std::fs::write(root.join("c.txt"), "").unwrap();
        let now = Instant::now();
        app.tick_at(now);
        assert_eq!(app.open_files[0].lines, ["one"], "compared once a second");
        // Before the autosave of the edit in b.txt (2 s).
        app.tick_at(now + Duration::from_millis(1500));
        assert_eq!(app.open_files[0].lines, ["uno"]);
        assert_eq!(app.open_files[1].lines, ["!two"]);
        assert!(app.open_files[1].dirty);
        assert_eq!(app.status, "b.txt changed on disk: your edits are kept");
        assert!(
            app.entries
                .iter()
                .any(|entry| entry.path == root.join("c.txt"))
        );
    }

    #[test]
    fn models_turn_and_zoom_with_keys_and_the_mouse() {
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("ship.glb"),
            crate::media::sample_glb(),
        )
        .unwrap();
        std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(directory.path().join("ship.glb")).unwrap();
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        let drawn = |app: &mut App| {
            let media = app.open_files[0].media.as_mut().unwrap();
            media.lines(40, 15).to_vec()
        };
        let mut last = drawn(&mut app);
        let mut changed = |app: &mut App, what: &str| {
            let now = drawn(app);
            assert_ne!(now, last, "{what}");
            last = now;
        };
        press(&mut app, KeyCode::Right, KeyModifiers::NONE);
        changed(&mut app, "→ turns");
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        changed(&mut app, "↑ tilts");
        press(&mut app, KeyCode::Char('+'), KeyModifiers::NONE);
        changed(&mut app, "+ comes closer");
        press(&mut app, KeyCode::Char('-'), KeyModifiers::NONE);
        changed(&mut app, "- goes back");
        mouse(&mut app, MouseEventKind::ScrollUp, 50, 10);
        changed(&mut app, "the wheel zooms");
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 50, 10);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 56, 12);
        changed(&mut app, "a drag turns");
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 56, 12);
        assert!(app.turning.is_none() && !app.selecting);
        assert!(
            app.open_files[0].lines.is_empty(),
            "nothing typed into a model"
        );
    }

    #[test]
    fn videos_answer_to_their_keys_buttons_bar_and_wheel() {
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("clip.mp4"), "not a video").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(directory.path().join("clip.mp4")).unwrap();
        // A video of 75.5 s with sound, playing; seeking starts `ffmpeg` on the file above,
        // which ends at once: the state is what is checked.
        app.open_files[0].media = Some(crate::media::sample_video());
        app.regions.editor = Rect::new(30, 2, 60, 20);
        app.regions.editor_body = app.regions.editor;
        app.regions.pane_bodies[0] = app.regions.editor;
        let video = |app: &App| {
            let media = app.open_files[0].media.as_ref().unwrap();
            let video = media.video().unwrap();
            (
                video.paused,
                video.volume,
                video.muted,
                video.speed(),
                video.position().as_secs(),
            )
        };
        let key = |app: &mut App, code| press(app, code, KeyModifiers::NONE);

        key(&mut app, KeyCode::Char(' '));
        assert!(video(&app).0, "Space pauses");
        key(&mut app, KeyCode::Right);
        assert_eq!(video(&app).4, 5, "→ 5 s on");
        press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(video(&app).4, 35, "Shift+→ 30 s on");
        key(&mut app, KeyCode::Left);
        assert_eq!(video(&app).4, 30);
        assert!(video(&app).0, "seeking keeps it paused");
        key(&mut app, KeyCode::Char('-'));
        key(&mut app, KeyCode::Char('m'));
        assert_eq!(video(&app).1, 90);
        assert!(video(&app).2, "M mutes");
        key(&mut app, KeyCode::Char(']'));
        assert_eq!(video(&app).3, 1.25);
        key(&mut app, KeyCode::Char('['));
        key(&mut app, KeyCode::Char('['));
        assert_eq!(video(&app).3, 0.75);
        key(&mut app, KeyCode::Char('s'));
        assert_eq!(
            (video(&app).0, video(&app).4),
            (true, 0),
            "S stops at the start"
        );
        app.editor_paste("text");
        key(&mut app, KeyCode::Char('x'));
        assert!(
            app.open_files[0].lines.is_empty(),
            "nothing typed into a video"
        );

        // The border is at 30, 2: the picture from row 3, the bar on row 19, buttons on 20.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 50, 10);
        assert!(!video(&app).0, "a click on the picture plays");
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 32, 20);
        assert!(video(&app).0, "the first button pauses");
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 46, 20);
        assert_eq!(video(&app).4, 0, "-5s stops at the start");
        mouse(&mut app, MouseEventKind::ScrollUp, 50, 10);
        assert_eq!(
            (video(&app).1, video(&app).2),
            (100, false),
            "the wheel turns it up"
        );
        mouse(&mut app, MouseEventKind::ScrollDown, 50, 10);
        assert_eq!(video(&app).1, 90);
        // The bar is the 44 cells from column 38: a drag only moves the bar, letting go
        // in its middle goes to the middle of the video.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 40, 19);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 60, 19);
        let scrub = |app: &App| {
            let media = app.open_files[0].media.as_ref().unwrap();
            media.video().unwrap().scrub
        };
        assert!(scrub(&app).is_some() && video(&app).4 == 0);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 59, 19);
        assert!(scrub(&app).is_none());
        assert!((36..=40).contains(&video(&app).4), "{}", video(&app).4);

        // F fills the screen; Esc leaves it, then closes the tab.
        key(&mut app, KeyCode::Char('f'));
        assert_eq!(app.fullscreen_video(), Some(0));
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(
            app.fullscreen_video(),
            None,
            "Help shows over the normal layout"
        );
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.tick_at(Instant::now() + Duration::from_millis(200));
        assert_eq!(app.fullscreen_video(), Some(0), "and back once it closed");
        key(&mut app, KeyCode::Esc);
        assert!(app.fullscreen_video().is_none() && app.open_files.len() == 1);
        key(&mut app, KeyCode::Esc);
        assert!(app.open_files.is_empty(), "Esc closes the video");
    }

    #[test]
    fn a_gif_plays_in_its_tab() {
        use image::{Delay, Frame, Rgba, RgbaImage, codecs::gif::GifEncoder};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("blink.gif");
        let mut encoder = GifEncoder::new(std::fs::File::create(&path).unwrap());
        for colour in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            let frame = RgbaImage::from_pixel(4, 4, Rgba(colour));
            let delay = Delay::from_numer_denom_ms(100, 1);
            encoder
                .encode_frame(Frame::from_parts(frame, 0, 0, delay))
                .unwrap();
        }
        drop(encoder);
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path).unwrap();
        let hint = |app: &App| app.active_file().unwrap().media.as_ref().unwrap().hint();
        assert_eq!(hint(&app).as_deref(), Some("Frame 1/2"));
        app.tick_at(Instant::now() + Duration::from_millis(150));
        assert_eq!(hint(&app).as_deref(), Some("Frame 2/2"));
    }

    #[test]
    fn ctrl_r_runs_the_file_in_the_bottom_terminal_and_stays_with_the_shell_there() {
        let directory = tempfile::tempdir().unwrap();
        let script = if cfg!(windows) {
            "hello.bat"
        } else {
            "hello.sh"
        };
        std::fs::write(directory.path().join(script), "echo hello\n").unwrap();
        std::fs::write(directory.path().join("notes.md"), "# notes\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(directory.path().join("notes.md")).unwrap();
        app.editor_paste("unsaved");
        press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(app.status, "SozoIDE does not know how to run .md files");
        assert!(!app.show_bottom_terminal);
        assert!(!app.open_files[0].dirty, "Ctrl+R saves first");

        app.open_file(directory.path().join(script)).unwrap();
        press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert!(app.status.starts_with("Running"), "{}", app.status);
        assert!(app.show_bottom_terminal && app.focus == Focus::BottomTerminal);
        // In the terminal Ctrl+R belongs to the shell (history search).
        app.status.clear();
        press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert!(app.status.is_empty());

        // The Explorer runs the selected file.
        app.focus = Focus::Explorer;
        app.show_bottom_terminal = false;
        app.selected_entry = app
            .entries
            .iter()
            .position(|entry| entry.path.ends_with(script))
            .unwrap();
        press(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert!(app.show_bottom_terminal && app.status.starts_with("Running"));
    }

    #[test]
    fn suggestions_narrow_as_you_type_and_the_chosen_one_replaces_the_word() {
        use crate::lsp::{Event, Item};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.rs");
        std::fs::write(&path, "fn main() {\n    let list = Vec::new();\n}\n").unwrap();
        std::fs::write(directory.path().join("lib.rs"), "pub fn helper() {}\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        app.editor_paste("\n    list.pu");
        let item = |label: &str, insert: &str| Item {
            label: label.into(),
            detail: String::new(),
            insert: insert.into(),
        };
        let answer = |app: &mut App, row| {
            app.lsp_event(Event::Completion {
                path: path.clone(),
                row,
                column: 11,
                items: vec![
                    item("push", "push"),
                    item("pop", "pop"),
                    item("as_ptr", "as_ptr"),
                ],
            })
        };
        // A late answer for another row is dropped.
        answer(&mut app, 0);
        assert!(app.completion.is_none());
        answer(&mut app, 2);
        let labels = |app: &App| {
            let (items, ..) = app.completion_view().unwrap();
            items
                .iter()
                .map(|item| item.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(&app), ["push"], "pu: push only");
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(
            labels(&app),
            ["push", "pop", "as_ptr"],
            "p: starting with it, then the rest"
        );
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.active_file().unwrap().lines[2], "    list.pop");
        assert!(app.completion.is_none());
        // Esc closes it; a key the list does not use reaches the editor.
        answer(&mut app, 2);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.completion.is_none());
        answer(&mut app, 2);
        press(&mut app, KeyCode::Char('('), KeyModifiers::NONE);
        assert!(app.completion.is_none());
        assert_eq!(app.active_file().unwrap().lines[2], "    list.pop(");

        // A definition elsewhere opens there; messages reach the status bar.
        let version = app.active_file().unwrap().edits;
        app.lsp_event(Event::Places {
            path: path.clone(),
            version,
            goal: crate::lsp::Goal::Definition,
            places: vec![crate::lsp::Place {
                path: directory.path().join("lib.rs"),
                row: 0,
                start: 7,
                end_row: 0,
                end: 13,
            }],
        });
        let file = app.active_file().unwrap();
        assert!(file.path.ends_with("lib.rs"));
        assert_eq!((file.cursor_row, file.cursor_column), (0, 7));
        app.lsp_event(Event::Message("rust-analyzer stopped".into()));
        assert_eq!(app.status, "rust-analyzer stopped");
        // Without a server Ctrl+Space and F12 do nothing harmful.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
        assert!(app.completion.is_none());
    }

    /// The requests a test `Lsp` wrote down: method, row and UTF-16 column.
    fn asked(app: &App) -> Vec<(String, usize, usize)> {
        app.lsp
            .asked
            .iter()
            .map(|(method, _, row, column)| (method.clone(), *row, *column))
            .collect()
    }

    #[test]
    fn ctrl_and_the_mouse_resting_on_a_word_ask_for_its_hover_once_and_leaving_closes_it() {
        use crate::{app::intellisense::HOVER_DELAY, lsp::Event};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.rs");
        std::fs::write(&path, "fn main() {\n    let total = compute();\n}\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        let text_x = 30 + 1 + 4;
        let hover = ("textDocument/hover".to_owned(), 1, 8);
        assert_eq!(HOVER_DELAY, Duration::from_secs(2));
        // «total» is at columns 8 to 12 of row 1: the mouse alone asks nothing (Q72).
        mouse(&mut app, MouseEventKind::Moved, text_x + 9, 4);
        app.tick_at(Instant::now() + HOVER_DELAY * 2);
        assert!(asked(&app).is_empty() && app.hover.is_none());
        // With Ctrl, nothing before it rests 2 s.
        ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 9, 4);
        app.tick_at(Instant::now() + Duration::from_millis(400));
        assert!(asked(&app).is_empty());
        // Inside the same word it keeps waiting; then it is asked once, at the word's start.
        ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 11, 4);
        let rested = Instant::now() + HOVER_DELAY + Duration::from_millis(50);
        app.tick_at(rested);
        app.tick_at(rested + Duration::from_millis(500));
        assert_eq!(asked(&app), std::slice::from_ref(&hover));
        ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 10, 4);
        app.tick_at(rested + Duration::from_secs(1));
        assert_eq!(
            asked(&app),
            std::slice::from_ref(&hover),
            "not again inside the same word"
        );
        // An answer for another edit is dropped; the right one fills the box.
        let version = app.active_file().unwrap().edits;
        let answer = |app: &mut App, version, text: &str| {
            app.lsp_event(Event::Hover {
                path: path.clone(),
                version,
                row: 1,
                column: 8,
                text: text.into(),
            })
        };
        answer(&mut app, version + 1, "stale");
        assert!(app.hover.as_ref().unwrap().text.is_none());
        answer(&mut app, version, "```rust\nlet total: u32\n```");
        assert_eq!(
            app.hover.as_ref().unwrap().text.as_deref(),
            Some("```rust\nlet total: u32\n```")
        );
        // Ctrl let go, it stays on its word and over its box; off the word and the box it
        // closes. Spaces ask nothing.
        mouse(&mut app, MouseEventKind::Moved, text_x + 12, 4);
        assert!(app.hover.is_some());
        app.regions.hover = Rect::new(40, 5, 20, 3);
        mouse(&mut app, MouseEventKind::Moved, 45, 6);
        assert!(app.hover.is_some());
        mouse(&mut app, MouseEventKind::Moved, text_x + 2, 4);
        assert!(app.hover.is_none());
        app.regions.hover = Rect::default();
        app.tick_at(rested + Duration::from_secs(1));
        assert_eq!(asked(&app).len(), 1);
        // Ctrl let go before the 2 s: nothing is asked. «compute» starts at column 16.
        ctrl_mouse(&mut app, MouseEventKind::Moved, text_x + 17, 4);
        assert!(app.hover.is_some());
        mouse(&mut app, MouseEventKind::Moved, text_x + 18, 4);
        assert!(app.hover.is_none());
        app.tick_at(Instant::now() + HOVER_DELAY * 2);
        assert_eq!(asked(&app).len(), 1);

        // Ctrl+K asks at the cursor, also just after the word.
        app.open_files[0].move_cursor_to(1, 13);
        app.lsp.asked.clear();
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(asked(&app), [hover]);
        answer(&mut app, version, "");
        assert!(app.hover.is_none());
        assert_eq!(app.status, "Nothing to show for this symbol");
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        answer(&mut app, version, "let total: u32");
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.hover.is_none());
        // A key that edits closes it and reaches the text.
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        answer(&mut app, version, "let total: u32");
        press(&mut app, KeyCode::Char('s'), KeyModifiers::NONE);
        assert!(app.hover.is_none());
        assert_eq!(
            app.active_file().unwrap().lines[1],
            "    let totals = compute();"
        );
        // Text pasted while it was asked: the answer is for the text before.
        app.open_files[0].move_cursor_to(1, 9);
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        let before = app.active_file().unwrap().edits;
        app.editor_paste("x");
        answer(&mut app, before, "let totals: u32");
        assert!(app.hover.as_ref().is_none_or(|hover| hover.text.is_none()));
        app.hover = None;
        // On a space the cursor has no symbol.
        app.open_files[0].move_cursor_to(1, 2);
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(app.status, "No symbol at the cursor");
    }

    #[test]
    fn a_failed_server_says_why_in_the_hover_and_how_to_install_it() {
        use crate::{app::intellisense::HOVER_DELAY, lsp::Failure};
        let directory = tempfile::tempdir().unwrap();
        let rust = directory.path().join("main.rs");
        let notes = directory.path().join("notes.md");
        std::fs::write(&rust, "fn main() {\n    let total = compute();\n}\n").unwrap();
        std::fs::write(&notes, "a few words\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        let text_x = 30 + 1 + 4;
        let rest = |app: &mut App, x: u16, y: u16| {
            app.hover = None;
            ctrl_mouse(app, MouseEventKind::Moved, x, y);
            assert!(app.hover.is_some(), "the mouse rests on a word");
            app.tick_at(Instant::now() + HOVER_DELAY * 2);
            app.hover.as_ref().and_then(|hover| hover.text.clone())
        };
        let why = "error: Unknown binary 'rust-analyzer' in official toolchain 'stable'.";
        app.lsp.fail("rust-analyzer", Failure::Start(why.into()));
        app.open_file(rust.clone()).unwrap();
        let install = format!(
            "{} or Ctrl+click offers to install it",
            app.keys.label(Action::Definition)
        );
        assert_eq!(
            rest(&mut app, text_x + 9, 4),
            Some(format!("rust-analyzer could not start: {why}\n\n{install}"))
        );
        // A server that stopped cannot be installed again: only why.
        app.lsp.restart("rust-analyzer");
        app.lsp
            .fail("rust-analyzer", Failure::Stopped("killed".into()));
        assert_eq!(
            rest(&mut app, text_x + 9, 4),
            Some("rust-analyzer stopped: killed".into())
        );
        // Without a server for the language the mouse says nothing.
        app.open_file(notes).unwrap();
        assert_eq!(rest(&mut app, text_x + 3, 3), None);
        assert!(app.hover.is_none());
    }

    #[test]
    fn definitions_peeks_and_references_open_and_alt_left_goes_back() {
        use crate::lsp::{Event, Goal, Place};
        let directory = tempfile::tempdir().unwrap();
        let main = directory.path().join("main.rs");
        let lib = directory.path().join("lib.rs");
        std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
        std::fs::write(
            &lib,
            "pub fn compute() -> u32 {\n    1\n}\n// compute again\n",
        )
        .unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(main.clone()).unwrap();
        app.open_files[0].move_cursor_to(1, 16);
        let place = |path: &PathBuf, row, start, end| Place {
            path: path.clone(),
            row,
            start,
            end_row: row,
            end,
        };
        let answer = |app: &mut App, goal, places: Vec<Place>| {
            let file = app.active_file().unwrap();
            let (path, version) = (file.path.clone(), file.edits);
            app.lsp_event(Event::Places {
                path,
                version,
                goal,
                places,
            });
        };
        let at = |app: &App| {
            let file = app.active_file().unwrap();
            (
                file.path.file_name().unwrap().to_str().unwrap().to_owned(),
                file.cursor_row,
                file.cursor_column,
            )
        };
        press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
        assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 16)]);
        assert_eq!(app.status, "Looking for the definition...");
        // An answer for an older edit is dropped.
        let version = app.active_file().unwrap().edits;
        app.lsp_event(Event::Places {
            path: main.clone(),
            version: version + 1,
            goal: Goal::Definition,
            places: vec![place(&lib, 0, 7, 14)],
        });
        assert_eq!(at(&app), ("main.rs".into(), 1, 16));
        // One definition opens there; Alt+Left comes back.
        answer(&mut app, Goal::Definition, vec![place(&lib, 0, 7, 14)]);
        assert_eq!(at(&app), ("lib.rs".into(), 0, 7));
        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(at(&app), ("main.rs".into(), 1, 16));

        // Several open a list to choose from, which takes no typing; Enter opens one.
        answer(
            &mut app,
            Goal::Definition,
            vec![place(&lib, 3, 3, 10), place(&lib, 0, 7, 14)],
        );
        let search = app.search.as_ref().unwrap();
        assert_eq!(search.title.as_deref(), Some("Definitions"));
        assert_eq!(search.query, "compute");
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.search.as_ref().unwrap().query, "compute");
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let file = app.active_file().unwrap();
        assert_eq!(
            at(&app),
            ("lib.rs".into(), 3, 10),
            "the second, sorted by row"
        );
        assert_eq!(file.anchor, Some((3, 3)), "the symbol selected");
        // The ESC b of the Terminal.app's Option+Left goes back too.
        press(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(at(&app), ("main.rs".into(), 1, 16));
        app.tick_at(Instant::now() + Duration::from_secs(1));

        // References: the same list, with every use and its line.
        app.lsp.asked.clear();
        press(&mut app, KeyCode::F(12), KeyModifiers::SHIFT);
        assert_eq!(asked(&app), [("textDocument/references".into(), 1, 16)]);
        answer(&mut app, Goal::References, Vec::new());
        assert_eq!(app.status, "No references found");
        answer(
            &mut app,
            Goal::References,
            vec![place(&main, 1, 16, 23), place(&lib, 0, 7, 14)],
        );
        let search = app.search.as_ref().unwrap();
        assert_eq!(search.title.as_deref(), Some("References"));
        let lines = search
            .matches
            .iter()
            .map(|found| (found.line, found.column, found.length, found.text.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            lines,
            [
                (0, 7, 7, "pub fn compute() -> u32 {"),
                (1, 16, 7, "    let total = compute();"),
            ]
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.tick_at(Instant::now() + Duration::from_secs(1));
        assert!(app.search.is_none());

        // Peek shows the definition without leaving the tab; the arrows scroll, Enter goes.
        press(&mut app, KeyCode::F(12), KeyModifiers::ALT);
        answer(&mut app, Goal::Peek, vec![place(&lib, 0, 7, 14)]);
        let peek = app.peek.as_ref().unwrap();
        assert_eq!((peek.row, peek.column, peek.top), (0, 7, 0));
        assert_eq!(peek.lines[3], "// compute again");
        assert_eq!(at(&app).0, "main.rs");
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.peek.as_ref().unwrap().top, 1);
        assert_eq!(at(&app), ("main.rs".into(), 1, 16), "the cursor stays");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.peek.is_none());
        assert_eq!(at(&app), ("lib.rs".into(), 0, 7));
        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(at(&app), ("main.rs".into(), 1, 16));
        // Esc closes it where it was.
        press(&mut app, KeyCode::F(12), KeyModifiers::ALT);
        answer(&mut app, Goal::Peek, vec![place(&lib, 0, 7, 14)]);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.peek.is_none());
        assert_eq!(at(&app).0, "main.rs");

        // Ctrl+click asks for the definition of what was clicked.
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        app.lsp.asked.clear();
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 30 + 1 + 4 + 9,
            row: 4,
            modifiers: KeyModifiers::CONTROL,
        });
        assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 9)]);
        assert!(!app.selecting && app.active_file().unwrap().anchor.is_none());
    }

    #[test]
    fn code_intelligence_says_why_without_a_server() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.md");
        std::fs::write(&path, "# Notes\nsome words\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path).unwrap();
        app.open_files[0].move_cursor_to(1, 2);
        for (code, modifiers) in [
            (KeyCode::F(12), KeyModifiers::NONE),
            (KeyCode::F(12), KeyModifiers::ALT),
            (KeyCode::F(12), KeyModifiers::SHIFT),
            (KeyCode::Char('k'), KeyModifiers::CONTROL),
        ] {
            app.status.clear();
            press(&mut app, code, modifiers);
            assert_eq!(app.status, "No language server for .md files", "{code:?}");
        }
        assert!(app.hover.is_none() && app.peek.is_none() && app.search.is_none());
        assert_eq!(app.active_file().unwrap().lines, ["# Notes", "some words"]);
        // The mouse resting on a word says nothing.
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        app.status.clear();
        mouse(&mut app, MouseEventKind::Moved, 30 + 1 + 4 + 1, 4);
        app.tick_at(Instant::now() + Duration::from_secs(1));
        assert!(app.hover.is_none() && app.status.is_empty());
        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(app.status, "No earlier place to go back to");
    }

    #[test]
    fn f7_builds_the_project_after_showing_the_command_once_and_shift_f5_stops_it() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("Cargo.toml"), "[package]\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        let rows = |app: &App| app.confirm.as_ref().unwrap().rows.clone();
        press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        let shown = rows(&app);
        assert_eq!(shown[0], ("Project", "Cargo".to_owned()));
        assert_eq!(shown[2], ("From", "Cargo.toml".to_owned()));
        assert!(shown[3].1.ends_with("&& cargo build"), "{:?}", shown[3]);
        assert_eq!(
            app.confirm.as_ref().unwrap().title,
            "Build the Cargo project"
        );
        // Esc runs nothing; Enter types it into the bottom terminal, which takes the focus.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.confirm.is_none() && !app.show_bottom_terminal);
        assert_eq!(app.status, "Cancelled");
        press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.confirm.is_none() && app.show_bottom_terminal);
        assert_eq!(app.focus, Focus::BottomTerminal);
        assert!(app.status.starts_with("Building: cd "), "{}", app.status);
        // The same command does not ask again; Shift+F5 stops it from the terminal.
        press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        assert!(app.confirm.is_none());
        press(&mut app, KeyCode::F(5), KeyModifiers::SHIFT);
        assert_eq!(app.status, "Stopped");
        press(&mut app, KeyCode::F(5), KeyModifiers::SHIFT);
        assert_eq!(
            app.status, "Stopped",
            "with nothing running the key is the shell's"
        );
        app.open_file(directory.path().join("Cargo.toml")).unwrap();
        app.focus = Focus::Editor;
        press(&mut app, KeyCode::F(17), KeyModifiers::NONE);
        assert_eq!(app.status, "Nothing is running");
        // Clean, Rebuild and Run ask for their own commands.
        for (code, modifiers, command) in [
            (KeyCode::F(7), KeyModifiers::SHIFT, "&& cargo clean"),
            (
                KeyCode::F(7),
                KeyModifiers::CONTROL,
                "&& cargo clean && cargo build",
            ),
            (KeyCode::F(5), KeyModifiers::CONTROL, "&& cargo run"),
        ] {
            press(&mut app, code, modifiers);
            assert!(rows(&app)[3].1.ends_with(command), "{:?}", rows(&app));
            press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        }
    }

    #[test]
    fn several_projects_are_chosen_and_a_folder_without_one_says_how() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let mut app = App::new(root.to_path_buf()).unwrap();
        press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        assert_eq!(
            app.status,
            "Failed: no project here. Add [tasks.build] to .sozo.toml"
        );
        press(&mut app, KeyCode::F(5), KeyModifiers::CONTROL);
        assert_eq!(
            app.status,
            "Failed: no project here. Add [tasks.run] to .sozo.toml"
        );
        // Two folders with a project: chosen once for the session.
        for name in ["api", "web"] {
            std::fs::create_dir(root.join(name)).unwrap();
            std::fs::write(root.join(name).join("Cargo.toml"), "").unwrap();
        }
        press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        let choice = app.project_choice.as_ref().unwrap();
        let folders = choice
            .projects
            .iter()
            .map(|project| project.folder.clone())
            .collect::<Vec<_>>();
        assert_eq!(folders, [root.join("api"), root.join("web")]);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.project_choice.is_none());
        let confirm = app.confirm.as_ref().unwrap();
        // Joined as the app joins it: `\\` between the parts on Windows.
        let web = std::path::Path::new(root.file_name().unwrap()).join("web");
        assert_eq!(confirm.rows[1], ("Folder", web.display().to_string()));
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        assert!(app.project_choice.is_none(), "the choice is kept");
        assert_eq!(
            app.confirm.as_ref().unwrap().rows[1].1,
            web.display().to_string()
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

        // A command nobody can know, and a tool that is not there, say so.
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("CMakeLists.txt"), "").unwrap();
        let mut app = App::new(other.path().to_path_buf()).unwrap();
        press(&mut app, KeyCode::F(5), KeyModifiers::CONTROL);
        assert_eq!(
            app.status,
            "Failed: This CMake project has no run command: add [tasks.run] to .sozo.toml"
        );
        std::fs::write(
            other.path().join(".sozo.toml"),
            "[tasks.build]\nprogram = \"sozo-no-such-tool\"\n",
        )
        .unwrap();
        press(&mut app, KeyCode::F(7), KeyModifiers::NONE);
        assert_eq!(
            app.status,
            "Failed: sozo-no-such-tool is missing: Help, then Dependencies"
        );
        assert!(app.confirm.is_none());
    }

    #[test]
    fn dependencies_check_on_opening_and_install_only_after_enter() {
        use crate::{
            app::HelpTab,
            update::deps::{Component, Found},
        };
        let mut app = test_app();
        press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        press(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(app.help_tab, HelpTab::Dependencies);
        assert!(
            app.dependencies.as_ref().unwrap().found.is_none(),
            "checking"
        );
        // Two components of our own: one there, one missing.
        let words = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        app.dependencies.as_mut().unwrap().components = vec![
            Component {
                name: "Cargo",
                kind: "Build tool",
                program: "cargo",
                probe: words(&["cargo", "--version"]),
                install: None,
                update: None,
            },
            Component {
                name: "Fake",
                kind: "Tool",
                program: "sozo-no-such-tool",
                probe: words(&["sozo-no-such-tool", "--version"]),
                install: Some(words(&["echo", "installed"])),
                update: None,
            },
        ];
        press(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        let started = Instant::now();
        while app.dependencies.as_ref().unwrap().found.is_none() {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
            app.tick_at(Instant::now());
        }
        let dependencies = app.dependencies.as_ref().unwrap();
        let found = dependencies.found.clone().unwrap();
        assert!(
            matches!(&found[0], Found::Installed(Some(version)) if version.starts_with("cargo "))
        );
        assert_eq!(found[1], Found::Missing);
        assert_eq!(dependencies.counts(), Some((1, 1)));
        // No way to update here: said, nothing asked.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.confirm.is_none());
        assert_eq!(
            app.status,
            "No package manager here can update Cargo: do it by hand"
        );
        // The missing one: its command waits for Enter; Esc runs nothing.
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let confirm = app.confirm.as_ref().unwrap();
        assert_eq!(confirm.title, "Install Fake");
        assert_eq!(confirm.rows[2], ("Command", "echo installed".to_owned()));
        assert!(!app.show_bottom_terminal);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.confirm.is_none() && !app.show_bottom_terminal);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.show_bottom_terminal && app.overlay == Overlay::None);
        assert_eq!(app.status, "Installing Fake: echo installed");
        // The row is watched; Help opens on the tab again and checks everything again.
        let dependencies = app.dependencies.as_ref().unwrap();
        assert_eq!(dependencies.watching.as_ref().map(|(row, _)| *row), Some(1));
        assert!(
            dependencies.found.is_some(),
            "what was found stays on screen"
        );
        press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(app.help_tab, HelpTab::Dependencies);
        assert!(
            app.dependencies.as_ref().unwrap().reader.is_some(),
            "checking again"
        );
        press(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.help_tab, HelpTab::Shortcuts);
    }

    #[test]
    fn an_installation_changes_its_row_by_itself_and_a_server_that_arrives_starts() {
        use crate::{
            app::{Dependencies, tasks::Confirm},
            lsp::Failure,
            update::deps::{Component, Found},
        };
        let mut app = test_app();
        let words = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        let server = Component {
            name: "rust-analyzer",
            kind: "Language server",
            program: "rust-analyzer",
            probe: words(&["rust-analyzer", "--version"]),
            install: Some(words(&["rustup", "component", "add", "rust-analyzer"])),
            update: None,
        };
        app.dependencies = Some(Dependencies::checked(
            vec![server],
            Some(vec![Found::Missing]),
        ));
        app.lsp.fail(
            "rust-analyzer",
            Failure::Start("error: Unknown binary".into()),
        );
        // The installation ends: the watcher says what is there now.
        app.component_watcher = |_, _, before| {
            assert_eq!(before, Found::Missing);
            let (sender, receiver) = std::sync::mpsc::channel();
            sender
                .send(Found::Installed(Some("rust-analyzer 1.99.1".into())))
                .unwrap();
            receiver
        };
        app.confirm = Some(Confirm {
            title: "Install rust-analyzer".into(),
            rows: Vec::new(),
            line: "rustup component add rust-analyzer".into(),
            running: "Installing rust-analyzer".into(),
            task: false,
            server: None,
            component: Some(0),
        });
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.lsp.missing("rust-analyzer"), "not before the tick");
        app.tick_at(Instant::now());
        let dependencies = app.dependencies.as_ref().unwrap();
        assert_eq!(
            dependencies.found.as_ref().unwrap()[0],
            Found::Installed(Some("rust-analyzer 1.99.1".into()))
        );
        assert!(dependencies.watching.is_none());
        // The server starts at once, without opening SozoIDE again.
        assert!(!app.lsp.missing("rust-analyzer"));
        assert_eq!(app.status, "rust-analyzer is installed");
        // One that fails again waits for the next thing found: no start at every tick.
        app.lsp.fail("rust-analyzer", Failure::Start(String::new()));
        app.tick_at(Instant::now());
        assert!(app.lsp.missing("rust-analyzer"));
    }

    #[test]
    fn the_editor_menu_previews_each_code_entry_beside_it_and_a_click_opens_a_place() {
        use crate::{
            app::PreviewRow,
            lsp::{Event, Goal, Place},
        };
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        let main = directory.path().join("main.rs");
        let lib = directory.path().join("lib.rs");
        std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
        std::fs::write(&lib, "pub fn compute() -> u32 {\n    1\n}\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(main.clone()).unwrap();
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        let text_x = 30 + 1 + 4;
        // The menu's entries as the view draws them: Paste, then the six questions.
        let open_menu = |app: &mut App| {
            mouse(
                app,
                MouseEventKind::Down(MouseButton::Right),
                text_x + 18,
                4,
            );
            let actions = app.entry_menu.as_ref().unwrap().actions.clone();
            app.regions.entry_menu_items = actions
                .iter()
                .enumerate()
                .map(|(index, action)| (*action, Rect::new(50, 6 + index as u16, 24, 1)))
                .collect();
        };
        let version = app.active_file().unwrap().edits;
        let place = |path: &std::path::Path, row, start, end| Place {
            path: path.to_path_buf(),
            row,
            start,
            end_row: row,
            end,
        };
        let answer = |app: &mut App, goal, version, places| {
            app.lsp_event(Event::Places {
                path: main.clone(),
                version,
                goal,
                places,
            })
        };
        open_menu(&mut app);
        assert!(app.menu_preview.is_none(), "Paste has nothing beside it");
        // The mouse on Go to Definition asks about «compute» once, and nothing jumps.
        mouse(&mut app, MouseEventKind::Moved, 55, 7);
        mouse(&mut app, MouseEventKind::Moved, 60, 7);
        assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 18)]);
        answer(
            &mut app,
            Goal::Definition,
            version + 1,
            vec![place(&lib, 0, 7, 14)],
        );
        assert!(
            app.menu_preview.as_ref().unwrap().places.is_none(),
            "another edit"
        );
        answer(
            &mut app,
            Goal::Definition,
            version,
            vec![place(&lib, 0, 7, 14), place(&main, 1, 16, 23)],
        );
        let preview = app.menu_preview.as_ref().unwrap();
        let places = preview.places.as_ref().unwrap();
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].text, "pub fn compute() -> u32 {");
        assert!(app.entry_menu.is_some() && app.search.is_none());
        assert_eq!(app.active_file().unwrap().path, main);
        // Paste has none; the arrow down to Peek asks for it and shows the file's lines.
        mouse(&mut app, MouseEventKind::Moved, 55, 6);
        assert!(app.menu_preview.is_none());
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.menu_preview.as_ref().unwrap().goal, Goal::Peek);
        answer(&mut app, Goal::Peek, version, vec![place(&lib, 0, 7, 14)]);
        let preview = app.menu_preview.as_ref().unwrap();
        assert_eq!(preview.lines, ["pub fn compute() -> u32 {", "    1", "}"]);
        // A click on it opens the place in its own tab; Back returns.
        app.regions.preview_rows = vec![(PreviewRow::Place(0), Rect::new(75, 8, 30, 3))];
        mouse(&mut app, MouseEventKind::Moved, 80, 9);
        assert_eq!(
            app.menu_preview.as_ref().unwrap().hovered,
            Some(PreviewRow::Place(0))
        );
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 80, 9);
        assert!(app.entry_menu.is_none() && app.menu_preview.is_none());
        let file = app.active_file().unwrap();
        assert_eq!(
            (file.path.clone(), file.cursor_row, file.cursor_column),
            (lib.clone(), 0, 7)
        );
        assert_eq!(app.open_files.len(), 2);
        app.go_back();
        assert_eq!(app.active_file().unwrap().path, main);

        // The entry clicked before the answer acts when it comes: references are a list.
        app.regions.preview_rows.clear();
        open_menu(&mut app);
        mouse(&mut app, MouseEventKind::Moved, 55, 12);
        assert_eq!(app.menu_preview.as_ref().unwrap().goal, Goal::References);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 55, 12);
        assert!(app.entry_menu.is_none());
        assert_eq!(app.status, "Looking for the references...");
        answer(
            &mut app,
            Goal::References,
            version,
            vec![place(&main, 1, 16, 23), place(&lib, 0, 7, 14)],
        );
        assert!(app.menu_preview.is_none());
        assert_eq!(
            app.search.as_ref().unwrap().title.as_deref(),
            Some("References")
        );
        app.search = None;
        // With the answer already there, the entry acts at once; a closed menu takes its
        // preview with it.
        open_menu(&mut app);
        mouse(&mut app, MouseEventKind::Moved, 55, 9);
        answer(
            &mut app,
            Goal::Declaration,
            version,
            vec![place(&lib, 0, 7, 14)],
        );
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 55, 9);
        assert_eq!(app.active_file().unwrap().path, lib);
        app.go_back();
        open_menu(&mut app);
        mouse(&mut app, MouseEventKind::Moved, 55, 10);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.tick_at(Instant::now() + Duration::from_secs(1));
        assert!(app.menu_preview.is_none());
    }

    #[test]
    fn a_missing_server_offers_its_installation_and_the_request_runs_once_it_is_there() {
        use crate::{
            app::PreviewRow,
            lsp::{Failure, Goal},
        };
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        let main = directory.path().join("main.rs");
        let script = directory.path().join("run.py");
        std::fs::write(&main, "fn main() {\n    let total = compute();\n}\n").unwrap();
        std::fs::write(&script, "total = 1\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.installs = |name| {
            Some(vec![
                "rustup".into(),
                "component".into(),
                "add".into(),
                name.into(),
            ])
        };
        // Here the server is there as soon as it is watched.
        app.watcher = |_| {
            let (sender, receiver) = std::sync::mpsc::channel();
            sender.send(()).unwrap();
            receiver
        };
        app.open_file(main.clone()).unwrap();
        app.open_files[0].move_cursor_to(1, 18);
        let why = "error: Unknown binary 'rust-analyzer' in official toolchain 'stable'.";
        app.lsp.fail("rust-analyzer", Failure::Start(why.into()));
        // F12 with the server unable to start: the dialog with the command, nothing yet.
        press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
        let confirm = app.confirm.as_ref().unwrap();
        assert_eq!(confirm.title, "Install rust-analyzer");
        assert_eq!(
            confirm.rows,
            [
                ("For", "Go to Definition in .rs files".to_owned()),
                ("Why", format!("rust-analyzer could not start: {why}")),
                ("Component", "rust-analyzer (Language server)".to_owned()),
                ("Command", "rustup component add rust-analyzer".to_owned()),
            ]
        );
        assert!(asked(&app).is_empty() && app.server_watch.is_none());
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.confirm.is_none() && app.server_watch.is_none());
        assert_eq!(app.status, "Cancelled");
        // Enter installs in the bottom terminal; once it is there the request runs again.
        press(&mut app, KeyCode::F(12), KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.status,
            "Installing rust-analyzer: rustup component add rust-analyzer"
        );
        assert!(app.show_bottom_terminal && app.server_watch.is_some());
        app.tick_at(Instant::now() + Duration::from_secs(1));
        assert!(app.server_watch.is_none());
        assert_eq!(app.lsp.installable(&main), None);
        assert_eq!(asked(&app), [("textDocument/definition".into(), 1, 18)]);
        assert_eq!(app.status, "Looking for the definition...");

        // A request already sent when the server fails (it stopped before answering)
        // offers the installation too.
        app.lsp.asked.clear();
        app.focus = Focus::Editor;
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert!(app.hover.is_some() && app.confirm.is_none());
        app.lsp.fail("rust-analyzer", Failure::Start(String::new()));
        app.tick_at(Instant::now() + Duration::from_secs(2));
        assert!(app.hover.is_none());
        assert_eq!(
            app.confirm.as_ref().unwrap().rows[0].1,
            "Information on the symbol in .rs files"
        );
        // A server that never comes is said.
        app.watcher = |_| std::sync::mpsc::channel().1;
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        app.tick_at(Instant::now() + Duration::from_secs(3));
        assert_eq!(app.status, "rust-analyzer is still not installed");
        assert!(app.server_watch.is_none());

        // The preview of the menu says why and its Install row offers the same.
        app.focus = Focus::Editor;
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Right),
            30 + 1 + 4 + 18,
            4,
        );
        let actions = app.entry_menu.as_ref().unwrap().actions.clone();
        app.regions.entry_menu_items = actions
            .iter()
            .enumerate()
            .map(|(index, action)| (*action, Rect::new(50, 6 + index as u16, 24, 1)))
            .collect();
        mouse(&mut app, MouseEventKind::Moved, 55, 7);
        let preview = app.menu_preview.as_ref().unwrap();
        assert_eq!(
            preview.failure.as_deref(),
            Some("rust-analyzer could not start")
        );
        assert!(preview.install);
        app.choose_preview_row(PreviewRow::Install);
        assert_eq!(app.confirm.as_ref().unwrap().title, "Install rust-analyzer");
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

        // Without a package manager for it, or after a crash, the footer only says why.
        app.installs = |_| None;
        app.lsp.fail("pyright-langserver", Failure::Missing);
        app.open_file(script).unwrap();
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert!(app.confirm.is_none());
        assert_eq!(
            app.status,
            "pyright-langserver is not installed. No package manager here installs it: do it by hand"
        );
        app.lsp.restart("pyright-langserver");
        app.lsp
            .fail("pyright-langserver", Failure::Stopped("killed".into()));
        app.ask_places(Goal::References);
        assert!(app.confirm.is_none());
        assert_eq!(app.status, "pyright-langserver stopped: killed");
    }

    #[test]
    fn the_editor_menu_goes_to_declarations_types_and_implementations_and_ctrl_marks_the_link() {
        use crate::lsp::{Event, Goal, Place};
        use crossterm::event::MouseButton;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.rs");
        std::fs::write(&path, "fn main() {\n    let total = compute();\n}\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        let text_x = 30 + 1 + 4;
        // The right-click puts the cursor on «compute»; the menu asks about it.
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Right),
            text_x + 18,
            4,
        );
        app.choose_entry_action(EntryAction::Code(Goal::Declaration));
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Right),
            text_x + 18,
            4,
        );
        app.choose_entry_action(EntryAction::Code(Goal::Implementation));
        assert_eq!(
            asked(&app),
            [
                ("textDocument/declaration".into(), 1, 18),
                ("textDocument/implementation".into(), 1, 18)
            ]
        );
        let version = app.active_file().unwrap().edits;
        let place = |row, start, end| Place {
            path: path.clone(),
            row,
            start,
            end_row: row,
            end,
        };
        let answer = |app: &mut App, goal, places| {
            app.lsp_event(Event::Places {
                path: path.clone(),
                version,
                goal,
                places,
            })
        };
        answer(
            &mut app,
            Goal::Implementation,
            vec![place(0, 3, 7), place(1, 8, 13)],
        );
        assert_eq!(
            app.search.as_ref().unwrap().title.as_deref(),
            Some("Implementations")
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.tick_at(Instant::now() + Duration::from_secs(1));
        answer(&mut app, Goal::TypeDefinition, vec![place(0, 3, 7)]);
        let file = app.active_file().unwrap();
        assert_eq!(
            (file.cursor_row, file.cursor_column),
            (0, 3),
            "one place goes there"
        );
        answer(&mut app, Goal::Declaration, Vec::new());
        assert_eq!(app.status, "No declaration found");

        // Ctrl over a word marks it as a link; without Ctrl, or off it, the mark goes.
        let moved = |app: &mut App, column, modifiers| {
            app.handle_mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row: 4,
                modifiers,
            })
        };
        moved(&mut app, text_x + 10, KeyModifiers::CONTROL);
        assert_eq!(app.link, Some((0, 1, 8, 13)));
        assert_eq!(app.status, "Ctrl+click: go to the definition");
        moved(&mut app, text_x + 2, KeyModifiers::CONTROL);
        assert_eq!(app.link, None, "a space is no link");
        moved(&mut app, text_x + 10, KeyModifiers::NONE);
        assert_eq!(app.link, None);
    }

    #[test]
    fn f9_marks_breakpoints_f5_builds_in_the_background_and_shift_f5_stops() {
        use crate::debug::{Frame, State, fake};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.md");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        assert_eq!(
            app.breakpoints[&path].iter().copied().collect::<Vec<_>>(),
            [1]
        );
        press(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        assert!(app.breakpoints[&path].is_empty(), "F9 again takes it off");

        // F5 plans and builds in a thread; the answer comes in a tick.
        press(&mut app, KeyCode::F(5), KeyModifiers::NONE);
        assert!(app.debug_building());
        let start = Instant::now();
        while app.debug_building() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
            app.tick_at(Instant::now());
        }
        assert_eq!(app.status, "SozoIDE does not know how to debug .md files");
        assert!(app.debug.is_none());

        // Paused: F10 steps, the margin knows the line; Shift+F5 (or F17) closes the panel.
        let frame = Frame {
            id: 1,
            name: "main".into(),
            path: Some(path.clone()),
            row: 2,
        };
        app.debug = Some(fake(
            State::Paused("breakpoint".into()),
            vec![frame],
            Vec::new(),
        ));
        assert_eq!(app.paused_row(&path), Some(2));
        press(&mut app, KeyCode::F(10), KeyModifiers::NONE);
        assert_eq!(app.debug.as_ref().unwrap().state, State::Running);
        assert_eq!(app.paused_row(&path), None);
        press(&mut app, KeyCode::F(5), KeyModifiers::SHIFT);
        assert!(app.debug.is_none());
        assert_eq!(app.status, "Debugging stopped");
        app.debug = Some(fake(
            State::Ended("Exited with code 0".into()),
            Vec::new(),
            Vec::new(),
        ));
        press(&mut app, KeyCode::F(17), KeyModifiers::NONE);
        assert!(app.debug.is_none());
    }

    #[test]
    fn the_keyboard_and_the_mouse_select_copy_and_cut_text() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "alpha beta\ngamma\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path).unwrap();
        let shift = KeyModifiers::SHIFT;
        press(&mut app, KeyCode::Right, shift);
        press(&mut app, KeyCode::Right, shift);
        press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.clipboard.take().as_deref(), Some("al"));
        press(&mut app, KeyCode::Down, shift);
        press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(app.clipboard.take().as_deref(), Some("alpha beta\nga"));
        assert_eq!(app.active_file().unwrap().lines, ["mma"]);
        press(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('Z'), shift);
        assert_eq!(app.active_file().unwrap().lines, ["Z"]);

        // A drag from the first column to the third selects two characters.
        app.editor_paste("\nsecond line");
        app.regions.editor_body = Rect::new(30, 2, 60, 20);
        app.regions.pane_bodies[0] = app.regions.editor_body;
        let text_x = 30 + 1 + 4;
        mouse(
            &mut app,
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            text_x,
            4,
        );
        mouse(
            &mut app,
            MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            text_x + 3,
            4,
        );
        mouse(
            &mut app,
            MouseEventKind::Up(crossterm::event::MouseButton::Left),
            text_x + 3,
            4,
        );
        assert_eq!(
            app.active_file().unwrap().selected_text().as_deref(),
            Some("sec")
        );
        press(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert!(app.active_file().unwrap().selection().is_none());
    }

    #[test]
    fn pdf_tabs_turn_their_pages_with_keys_and_the_wheel() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manual.pdf");
        std::fs::write(&path, crate::editor::sample_pdf(3)).unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path).unwrap();
        let page = |app: &App| app.active_file().unwrap().page().unwrap().0;
        press(&mut app, KeyCode::PageDown, KeyModifiers::NONE);
        press(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(page(&app), 2);
        press(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(page(&app), 2, "the last page");
        press(&mut app, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(page(&app), 0);
        app.regions.pane_bodies[0] = Rect::new(30, 2, 60, 20);
        mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
        mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
        assert_eq!(page(&app), 1, "one page per turn of the wheel");
        press(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert!(app.open_files.is_empty());
    }

    #[test]
    fn a_text_page_of_a_pdf_is_read_selected_copied_and_scrolled_into_the_next() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        let long = (1..=30)
            .map(|line| format!("({line}. text) Tj 0 -3 Td"))
            .collect::<Vec<_>>()
            .join(" ");
        let first = format!("BT /F1 2 Tf 5 96 Td {long} ET");
        let second = "BT /F1 10 Tf 5 80 Td (Second page) Tj ET";
        std::fs::write(&path, crate::editor::sample_pdf_pages(&[&first, second])).unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        let page = |app: &App| app.active_file().unwrap().page().unwrap().0;
        fn file(app: &App) -> &crate::editor::OpenFile {
            app.active_file().unwrap()
        }
        assert_eq!(file(&app).lines.len(), 30);
        // The arrows move the cursor and select; Ctrl+C copies; nothing is typed.
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        assert_eq!(page(&app), 0, "arrows stay on a page of text");
        press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.copied.as_deref(), Some("2. text\n"));
        for code in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Backspace] {
            press(&mut app, code, KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(file(&app).lines[1], "2. text");
        assert!(!file(&app).dirty);
        // The wheel scrolls the 30 lines in 18 rows, then turns the page at the end.
        app.regions.pane_bodies[0] = Rect::new(30, 2, 60, 20);
        for _ in 0..4 {
            mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
        }
        assert_eq!((page(&app), file(&app).scroll), (0, 12));
        mouse(&mut app, MouseEventKind::ScrollDown, 40, 5);
        assert_eq!(page(&app), 1);
        assert_eq!(file(&app).lines, ["Second page"]);
        app.page_turned = None;
        mouse(&mut app, MouseEventKind::ScrollUp, 40, 5);
        assert_eq!(page(&app), 0, "and back at the top");
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(page(&app), 1);
        press(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(page(&app), 0);
    }

    #[test]
    fn long_dialogs_scroll_with_the_keys_and_the_wheel() {
        let mut app = test_app();
        press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(app.overlay_scroll, 11);
        mouse(&mut app, MouseEventKind::ScrollUp, 10, 10);
        assert_eq!(app.overlay_scroll, 8);
        press(&mut app, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(app.overlay_scroll, 0);
    }

    #[test]
    fn quit_shortcut_stops_the_application() {
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn about_checks_for_updates_and_opens_the_notice_in_its_place() {
        use crate::update::Updater;
        let directory = tempfile::tempdir().unwrap();
        let mut app = test_app();
        press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::About);
        app.regions.about_update = Rect::new(40, 20, 23, 1);
        mouse(
            &mut app,
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            45,
            20,
        );
        assert_eq!(
            app.updater.checked.as_deref(),
            Some("Checking for updates...")
        );
        app.updater.checked = None;
        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(
            app.updater.checked.as_deref(),
            Some("Checking for updates...")
        );
        assert_eq!(app.overlay, Overlay::About, "About stays while it checks");

        // The answer: a newer version, also one skipped before, takes About's place.
        app.updater = Updater::offering("99.0.0", directory.path().join("update.toml"));
        app.tick();
        assert_eq!(app.overlay, Overlay::Update);
    }

    #[test]
    fn update_notice_skips_waits_and_quits_to_install() {
        use crate::update::Updater;
        let directory = tempfile::tempdir().unwrap();
        let settings = directory.path().join("update.toml");
        let mut app = test_app();
        app.updater = Updater::offering("99.0.0", settings.clone());
        app.tick();
        assert_eq!(app.overlay, Overlay::Update);
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(app.overlay, Overlay::Update);

        app.regions.update_skip = Rect::new(30, 12, 23, 1);
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 35,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.updater.state, UpdateState::Idle);
        assert!(
            std::fs::read_to_string(&settings)
                .unwrap()
                .contains("99.0.0")
        );
        let now = std::time::Instant::now();
        app.tick_at(now + std::time::Duration::from_millis(200));
        assert_eq!(app.overlay, Overlay::None);

        app.updater = Updater::offering("99.0.1", settings);
        app.tick();
        assert_eq!(app.overlay, Overlay::Update);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.updater.state, UpdateState::Idle);
        app.tick_at(std::time::Instant::now() + std::time::Duration::from_millis(200));
        assert_eq!(app.overlay, Overlay::None);
        assert!(!app.should_quit);

        app.updater.state = UpdateState::Ready(directory.path().join("SozoIDE-macos-arm64.dmg"));
        app.tick();
        assert!(app.should_quit);
    }

    #[test]
    fn changed_shortcuts_run_their_commands_and_the_old_keys_do_not() {
        use super::super::keys::Binding;
        let mut app = test_app();
        let key = |text| Binding::parse(text).unwrap();
        app.keys.set(Action::Explorer, key("Ctrl+E"));
        press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
        assert!(!app.show_explorer);
        press(&mut app, KeyCode::Char('b'), KeyModifiers::CONTROL);
        assert!(!app.show_explorer, "Ctrl+B is nobody's key now");
        press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
        assert!(app.show_explorer);

        // The editor's commands follow too: Alt+W closes the tab and F7 marks a breakpoint.
        let path = app.root.join("Cargo.toml");
        app.open_file(path.clone()).unwrap();
        app.keys.set(Action::CloseTab, key("Alt+W"));
        app.keys.set(Action::Breakpoint, key("F8"));
        press(&mut app, KeyCode::F(9), KeyModifiers::NONE);
        assert!(
            app.breakpoints
                .get(&path)
                .is_none_or(|rows| rows.is_empty())
        );
        press(&mut app, KeyCode::F(8), KeyModifiers::NONE);
        assert!(app.breakpoints[&path].contains(&0));
        press(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(app.open_files.len(), 1);
        press(&mut app, KeyCode::Char('w'), KeyModifiers::ALT);
        assert!(app.open_files.is_empty());

        app.keys.set(Action::Quit, key("Alt+Q"));
        press(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(!app.should_quit);
        press(&mut app, KeyCode::Char('q'), KeyModifiers::ALT);
        assert!(app.should_quit);
    }

    #[test]
    fn help_changes_a_shortcut_by_tab_or_a_click_keeps_it_and_resets_them_all() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keys.toml");
        let mut app = test_app();
        app.keys_path = Some(path.clone());
        press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::Help);
        press(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(app.key_editor.selected, Some(Action::Quit));
        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.key_editor.selected, Some(Action::Explorer));
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.key_editor.capturing);
        // Keys that type or edit are refused and Help keeps waiting.
        press(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);
        assert!(app.key_editor.capturing);
        assert!(app.key_editor.message.as_deref().unwrap().contains("types"));
        press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.key_editor.message.as_deref(), Some("Ctrl+C stays copy"));
        press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
        assert!(!app.key_editor.capturing);
        assert_eq!(app.keys.label(Action::Explorer), "Ctrl+E");
        assert_eq!(app.key_editor.message.as_deref(), Some("Explorer: Ctrl+E"));
        let kept = std::fs::read_to_string(&path).unwrap();
        assert!(kept.contains("explorer = \"Ctrl+E\""), "{kept}");

        // A click on Source Control's key, then the key the Explorer took: they swap.
        app.regions.help_keys = vec![(Action::SourceControl, Rect::new(10, 10, 52, 1))];
        mouse(
            &mut app,
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            12,
            10,
        );
        assert!(app.key_editor.capturing);
        assert_eq!(app.key_editor.selected, Some(Action::SourceControl));
        press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
        assert_eq!(app.keys.label(Action::SourceControl), "Ctrl+E");
        assert_eq!(app.keys.label(Action::Explorer), "Ctrl+G");
        assert_eq!(
            app.key_editor.message.as_deref(),
            Some("Source Control: Ctrl+E; Explorer took Ctrl+G")
        );

        // While Help waits, Ctrl+Q is a key like the others; Esc gives up and Help stays.
        app.choose_shortcut(Action::Run);
        press(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(!app.should_quit);
        assert_eq!(app.keys.label(Action::Run), "Ctrl+Q");
        assert_eq!(app.keys.label(Action::Quit), "Ctrl+R");
        app.choose_shortcut(Action::Run);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.key_editor.capturing);
        assert!(app.overlay == Overlay::Help && !app.motion.overlay_closing());

        // R asks first: Esc keeps the keys, Enter puts every default back and the file goes.
        press(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        assert!(app.key_editor.confirm_reset);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.key_editor.confirm_reset);
        assert_eq!(app.keys.label(Action::Explorer), "Ctrl+G");
        app.regions.help_reset = Rect::new(2, 30, 21, 1);
        mouse(
            &mut app,
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            3,
            30,
        );
        assert!(app.key_editor.confirm_reset);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.keys, crate::app::Keymap::defaults(std::env::consts::OS));
        assert!(!path.exists());
        assert_eq!(
            app.key_editor.message.as_deref(),
            Some("Every shortcut is back to the default")
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.motion.overlay_closing());
    }

    #[test]
    fn dragging_over_a_terminal_chooses_its_text_and_letting_go_copies_it() {
        use crate::terminal::Terminal;
        let mut app = test_app();
        app.show_bottom_terminal = true;
        app.bottom_terminal.tabs = vec![Terminal::unavailable("hello world\r\nsecond line".into())];
        // Inside the border the text starts at column 31, row 23.
        app.regions.bottom_terminal = Rect::new(30, 22, 60, 10);
        let left = crossterm::event::MouseButton::Left;
        mouse(&mut app, MouseEventKind::Down(left), 31, 23);
        mouse(&mut app, MouseEventKind::Drag(left), 35, 23);
        mouse(&mut app, MouseEventKind::Up(left), 35, 23);
        assert_eq!(app.focus, Focus::BottomTerminal);
        assert_eq!(app.clipboard.take().as_deref(), Some("hello"));
        assert_eq!(app.status, "Copied 5 characters");
        assert!(
            app.terminal_selection.is_some(),
            "the choice stays on screen"
        );
        // A key goes to the shell and the choice goes.
        press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.terminal_selection, None);

        // Backwards over two rows.
        mouse(&mut app, MouseEventKind::Down(left), 36, 24);
        mouse(&mut app, MouseEventKind::Drag(left), 37, 23);
        mouse(&mut app, MouseEventKind::Up(left), 37, 23);
        assert_eq!(app.clipboard.take().as_deref(), Some("world\nsecond"));
        // Past the border the drag stays inside it; the empty rows are left out.
        mouse(&mut app, MouseEventKind::Down(left), 31, 23);
        mouse(&mut app, MouseEventKind::Drag(left), 200, 100);
        mouse(&mut app, MouseEventKind::Up(left), 200, 100);
        assert_eq!(
            app.clipboard.take().as_deref(),
            Some("hello world\nsecond line")
        );
        assert_eq!(
            app.terminal_selection.map(|selection| selection.end),
            Some((7, 57))
        );
        // The wheel scrolls the history and the choice goes; a click chooses nothing.
        mouse(&mut app, MouseEventKind::ScrollUp, 40, 25);
        assert_eq!(app.terminal_selection, None);
        mouse(&mut app, MouseEventKind::Down(left), 40, 25);
        mouse(&mut app, MouseEventKind::Up(left), 40, 25);
        assert_eq!((app.clipboard.take(), app.terminal_selection), (None, None));
    }

    #[test]
    fn the_explorer_scrolls_sideways_up_to_its_widest_name() {
        let directory = tempfile::tempdir().unwrap();
        let long = "a_very_long_file_name_that_does_not_fit_in_the_explorer.txt";
        std::fs::write(directory.path().join(long), "").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.regions.explorer = Rect::new(0, 1, 30, 20);
        let wheel = |app: &mut App, kind, modifiers| {
            app.handle_mouse(MouseEvent {
                kind,
                column: 5,
                row: 4,
                modifiers,
            });
        };
        wheel(&mut app, MouseEventKind::ScrollDown, KeyModifiers::SHIFT);
        assert_eq!(app.explorer_scroll_x, 6);
        // Never past the end of the widest name: 4 columns before it and the name, 28 in view.
        for _ in 0..20 {
            wheel(&mut app, MouseEventKind::ScrollRight, KeyModifiers::NONE);
        }
        assert_eq!(app.explorer_scroll_x, 4 + long.len() - 28);
        wheel(&mut app, MouseEventKind::ScrollLeft, KeyModifiers::NONE);
        assert_eq!(app.explorer_scroll_x, 4 + long.len() - 28 - 6);
        // The wheel alone still scrolls down the tree.
        wheel(&mut app, MouseEventKind::ScrollDown, KeyModifiers::NONE);
        assert_eq!(app.explorer_scroll_x, 4 + long.len() - 34);
    }

    #[test]
    fn terminal_tabs_add_close_and_switch_with_their_keys() {
        let mut app = test_app();
        app.show_bottom_terminal = true;
        app.focus = Focus::BottomTerminal;
        for _ in 0..4 {
            app.add_terminal();
        }
        assert_eq!(app.bottom_terminal.tabs.len(), 3, "three at most");
        assert_eq!(app.bottom_terminal.active, 2);
        press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
        assert_eq!(app.bottom_terminal.active, 0);
        press(&mut app, KeyCode::PageUp, KeyModifiers::CONTROL);
        assert_eq!(app.bottom_terminal.active, 2);
        for _ in 0..4 {
            app.close_terminal(0);
        }
        assert_eq!(app.bottom_terminal.tabs.len(), 1, "the last one stays");
        // The right terminal always has its two tabs.
        assert_eq!(app.right_terminal.tabs.len(), 2);
        app.show_right_terminal = true;
        app.focus = Focus::RightTerminal;
        press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
        assert_eq!(
            (app.right_terminal.active, app.bottom_terminal.active),
            (1, 0)
        );
        // Outside a terminal the keys reach the editor.
        app.focus = Focus::Editor;
        press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
        assert_eq!(app.right_terminal.active, 1);
    }

    #[test]
    fn about_opens_the_notes_of_every_version_and_tab_or_a_click_filters_them() {
        let mut app = test_app();
        press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::WhatsNew);
        assert!(app.all_notes);
        assert_eq!(app.notes.len(), crate::update::NOTES.len());
        assert_eq!(app.notes_filter, NotesFilter::All);
        // Tab and BackTab go round All, New and Fixed, from the top each time.
        app.overlay_scroll = 9;
        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(
            (app.notes_filter, app.overlay_scroll),
            (NotesFilter::New, 0)
        );
        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.notes_filter, NotesFilter::Fixed);
        press(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(app.notes_filter, NotesFilter::New);
        app.regions.notes_filters = vec![(NotesFilter::All, Rect::new(10, 5, 5, 1))];
        mouse(
            &mut app,
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            12,
            5,
        );
        assert_eq!(app.notes_filter, NotesFilter::All);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.motion.overlay_closing());

        // The button of About does the same, and What's new after an update shows all its notes.
        app.tick_at(std::time::Instant::now() + std::time::Duration::from_secs(1));
        press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        app.notes_filter = NotesFilter::Fixed;
        app.regions.about_notes = Rect::new(40, 21, 19, 1);
        mouse(
            &mut app,
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            45,
            21,
        );
        assert_eq!(
            (app.overlay, app.notes_filter),
            (Overlay::WhatsNew, NotesFilter::All)
        );
    }
}
