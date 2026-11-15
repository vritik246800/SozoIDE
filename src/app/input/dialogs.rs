//! Keys of what opens over the workspace: the search, the prompt and the menu of the Explorer,
//! Help, About, the update notice and the splash.

use super::*;

impl App {
    pub(in crate::app) fn handle_search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match self.keys.action(key) {
            Some(Action::FindInFile) => return self.open_search(SearchScope::CurrentFile),
            Some(Action::FindInWorkspace) => return self.open_search(SearchScope::Workspace),
            Some(Action::Replace) => return self.open_replace(),
            _ => {}
        }
        if self.motion.search_closing() {
            return;
        }
        if self.handle_replace_key(key) {
            return self.preview_match();
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
    pub(in crate::app) fn preview_match(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        if search.scope != SearchScope::CurrentFile {
            return;
        }
        if let (Some(found), Some(active)) = (search.selected_match().cloned(), self.active_tab) {
            let file = &mut self.open_files[active];
            // A PDF: the page of the match, and the line on it (TODO 5.1.6).
            let place = file.pdf.as_ref().and_then(|document| {
                document
                    .lines()
                    .get(found.line)
                    .map(|(page, line, _)| (*page, *line))
            });
            let line = match place {
                Some((page, line)) => {
                    file.show_page(page, &mut self.picker);
                    line
                }
                None => found.line,
            };
            let file = &mut self.open_files[active];
            file.scroll = file.display_row(line).saturating_sub(3) as u16;
            file.follow_cursor = false;
        }
    }

    /// Opens the file of `found` with the match selected a few lines below the top; where the
    /// cursor was goes to Back (F31).
    pub(in crate::app) fn show_match(&mut self, found: &crate::editor::SearchMatch) {
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
    pub(in crate::app) fn step_match(&mut self, forward: bool) {
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
    pub(in crate::app) fn handle_name_key(&mut self, code: KeyCode) {
        let Some(prompt) = &mut self.name_prompt else {
            return;
        };
        let typing = !prompt.action.asks();
        match code {
            KeyCode::Esc => self.cancel_name(),
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
            .filter(|prompt| !prompt.action.asks())
        {
            prompt
                .name
                .extend(text.chars().filter(|character| !character.is_control()));
            prompt.error = None;
        }
    }

    /// The Explorer's right-click menu: arrows choose, Enter runs, Esc closes.
    pub(in crate::app) fn handle_menu_key(&mut self, code: KeyCode) {
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
    pub(in crate::app) fn menu_mouse(&mut self, point: (u16, u16)) {
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
    pub(in crate::app) fn handle_overlay_key(&mut self, key: KeyEvent) {
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
            // T on the Shortcuts tab: the tutorial again (TODO 6.2.1).
            if self.help_tab == HelpTab::Shortcuts && matches!(key.code, KeyCode::Char('t' | 'T')) {
                self.tutorial_step = 0;
                self.open_overlay(Overlay::Tutorial);
                return;
            }
        }
        // F1 in What's new: Help, where every key of what is new is (TODO 6.2.1).
        if self.overlay == Overlay::WhatsNew && self.keys.action(key) == Some(Action::Help) {
            self.open_overlay(Overlay::Help);
            return;
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
            (Overlay::About, KeyCode::Char('l' | 'L')) => self.open_licenses(),
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
    pub(in crate::app) fn handle_update_key(&mut self, code: KeyCode) {
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
    pub(in crate::app) fn open_release_notes(&mut self) {
        self.notes = crate::update::NOTES.iter().collect();
        self.all_notes = true;
        self.notes_filter = NotesFilter::All;
        self.open_overlay(Overlay::WhatsNew);
    }

    /// Tab, BackTab or a click on All, New or Fixed: those notes, from the top.
    pub(in crate::app) fn filter_notes(&mut self, filter: NotesFilter) {
        self.notes_filter = filter;
        self.overlay_scroll = 0;
    }

    pub(in crate::app) fn open_overlay(&mut self, overlay: Overlay) {
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

    pub(in crate::app) fn close_overlay(&mut self) {
        self.motion.close_overlay(std::time::Instant::now());
    }

    pub(in crate::app) fn open_search(&mut self, scope: SearchScope) {
        self.search = Some(SearchState::new(scope));
        self.motion.open_search(std::time::Instant::now());
    }

    pub(in crate::app) fn close_search(&mut self) {
        self.motion.close_search(std::time::Instant::now());
    }

    pub(in crate::app) fn dismiss_splash(&mut self) {
        self.motion.close_splash(std::time::Instant::now());
    }
}
