use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    editor::{SearchScope, SearchState},
    update::UpdateState,
};

use super::{App, DragTarget, DropZone, EntryAction, Focus, Overlay, TUTORIAL_STEPS, TabDrag};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        // macOS and Linux terminals send Ctrl+Shift+B as Ctrl+B (E11, E18), so they use
        // Ctrl+Option+B and Ctrl+Alt+B.
        let right_terminal_modifier = if cfg!(any(target_os = "macos", target_os = "linux")) {
            key.modifiers.contains(KeyModifiers::ALT)
        } else {
            shift
        };
        if ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            self.flush_all_files();
            self.should_quit = true;
            return;
        }
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
                match key.code {
                    KeyCode::F(1) => self.open_overlay(Overlay::Help),
                    KeyCode::F(2) => self.open_overlay(Overlay::About),
                    KeyCode::Char('p') | KeyCode::Char('P') if ctrl => {
                        let scope = if shift {
                            SearchScope::Workspace
                        } else {
                            SearchScope::CurrentFile
                        };
                        self.open_search(scope);
                    }
                    _ => {}
                }
            } else {
                self.handle_overlay_key(key.code);
            }
            return;
        }

        match key.code {
            KeyCode::F(1) => {
                self.flush_active_file();
                self.open_overlay(Overlay::Help);
                return;
            }
            KeyCode::F(2) => {
                self.flush_active_file();
                self.open_overlay(Overlay::About);
                return;
            }
            KeyCode::Char('g') | KeyCode::Char('G') if ctrl => {
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
            KeyCode::Char('d') | KeyCode::Char('D') if ctrl => {
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
            KeyCode::Char('p') | KeyCode::Char('P') if ctrl => {
                self.flush_active_file();
                let scope = if shift {
                    SearchScope::Workspace
                } else {
                    SearchScope::CurrentFile
                };
                self.open_search(scope);
                return;
            }
            KeyCode::Char('b') | KeyCode::Char('B') if ctrl && right_terminal_modifier => {
                self.flush_active_file();
                self.show_right_terminal = !self.show_right_terminal;
                self.focus = if self.show_right_terminal {
                    Focus::RightTerminal
                } else {
                    Focus::Editor
                };
                return;
            }
            KeyCode::Char('b') | KeyCode::Char('B') if ctrl => {
                self.flush_active_file();
                self.show_explorer = !self.show_explorer;
                self.focus = if self.show_explorer {
                    Focus::Explorer
                } else {
                    Focus::Editor
                };
                return;
            }
            KeyCode::Char('j') | KeyCode::Char('J') if ctrl => {
                self.flush_active_file();
                self.show_bottom_terminal = !self.show_bottom_terminal;
                self.focus = if self.show_bottom_terminal {
                    Focus::BottomTerminal
                } else {
                    Focus::Editor
                };
                return;
            }
            _ => {}
        }

        match self.focus {
            Focus::Explorer => self.handle_explorer_key(key),
            Focus::Editor => self.handle_editor_key(key),
            Focus::BottomTerminal => self.bottom_terminal.send_key(key),
            Focus::RightTerminal => self.right_terminal.send_key(key),
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

    fn handle_search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')) {
            let scope = if key.modifiers.contains(KeyModifiers::SHIFT) {
                SearchScope::Workspace
            } else {
                SearchScope::CurrentFile
            };
            self.open_search(scope);
            return;
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
            KeyCode::Backspace => search.backspace(&self.root, active),
            KeyCode::Char(character) if !ctrl => search.push(character, &self.root, active),
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

    /// Opens the file of `found` with the match selected a few lines below the top.
    fn show_match(&mut self, found: &crate::editor::SearchMatch) {
        if let Err(error) = self.open_file(found.path.clone()) {
            self.status = error.to_string();
        } else if let Some(active) = self.active_tab {
            let file = &mut self.open_files[active];
            file.move_cursor_to(found.line, found.column + found.length);
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
    }

    pub fn search_paste(&mut self, text: &str) {
        if self.motion.search_closing() {
            return;
        }
        let active = self.active_tab.and_then(|index| self.open_files.get(index));
        if let Some(search) = &mut self.search {
            search.push_str(text, &self.root, active);
        }
        self.preview_match();
    }

    /// Esc, F1 and F2 close every dialog; the tutorial pages with Enter and the arrows, About
    /// opens the repository with G.
    fn handle_overlay_key(&mut self, code: KeyCode) {
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
            (_, KeyCode::Esc | KeyCode::F(1) | KeyCode::F(2)) => self.close_overlay(),
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

    pub(super) fn open_overlay(&mut self, overlay: Overlay) {
        let now = std::time::Instant::now();
        if overlay == Overlay::About && self.overlay != Overlay::About {
            self.motion.restart_bloom(now);
        }
        self.overlay = overlay;
        self.overlay_scroll = 0;
        self.motion.open_overlay(now);
    }

    fn close_overlay(&mut self) {
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
        match key.code {
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
        if ctrl && matches!(key.code, KeyCode::Char('w' | 'W')) {
            self.close_tab(active);
            return;
        }
        let file = &mut self.open_files[active];
        if file.image.is_some() {
            // A PDF turns its pages; an image has nothing to edit.
            if let Some((page, _)) = file.page() {
                let target = match key.code {
                    KeyCode::PageDown | KeyCode::Right | KeyCode::Down | KeyCode::Char(' ') => {
                        Some(page + 1)
                    }
                    KeyCode::PageUp | KeyCode::Left | KeyCode::Up => Some(page.saturating_sub(1)),
                    KeyCode::Home => Some(0),
                    KeyCode::End => Some(usize::MAX),
                    _ => None,
                };
                if let Some(target) = target {
                    file.show_page(target, &mut self.picker);
                }
            }
            return;
        }
        // Ctrl+Z on every system: on a Mac, Cmd+Z stays with the terminal.
        if ctrl && matches!(key.code, KeyCode::Char('z' | 'Z')) {
            if !file.undo() {
                self.status = "Nothing to undo".into();
            }
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
            KeyCode::Char(character) if !ctrl => file.insert(character),
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
                if self.name_prompt.is_some() {
                    return;
                }
                if self.entry_menu.is_some() {
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
                if self.show_explorer && contains(self.regions.explorer_handle, point) {
                    self.drag = Some(DragTarget::Explorer);
                    return;
                }
                if self.show_right_terminal && contains(self.regions.right_handle, point) {
                    self.drag = Some(DragTarget::RightTerminal);
                    return;
                }
                if self.show_bottom_terminal && contains(self.regions.bottom_handle, point) {
                    self.drag = Some(DragTarget::BottomTerminal);
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
                if self.overlay != Overlay::None && !self.motion.overlay_closing() {
                    let regions = &self.regions;
                    let key = if contains(regions.about_repository, point) {
                        Some(KeyCode::Char('g'))
                    } else if contains(regions.dialog_next, point) {
                        Some(KeyCode::Enter)
                    } else if contains(regions.dialog_skip, point) {
                        Some(KeyCode::Esc)
                    } else {
                        None
                    };
                    if let Some(key) = key {
                        self.handle_overlay_key(key);
                    }
                    return;
                }
                if self.overlay != Overlay::None {
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
                } else if self.show_bottom_terminal && contains(self.regions.bottom_terminal, point)
                {
                    self.focus = Focus::BottomTerminal;
                } else if self.show_right_terminal && contains(self.regions.right_terminal, point) {
                    self.focus = Focus::RightTerminal;
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
                    if let Some(file) = self
                        .active_tab
                        .and_then(|index| self.open_files.get_mut(index))
                        && file.image.is_none()
                        && contains(area, point)
                    {
                        // A click starts a selection that a drag makes longer; Shift extends
                        // the one there is.
                        let extend = mouse.modifiers.contains(KeyModifiers::SHIFT);
                        let (row, visual_column) = text_position(file, area, point);
                        let column = file.cursor_column_for_click(row, visual_column);
                        file.mark(extend);
                        file.move_cursor_to(row, column);
                        if !extend {
                            file.anchor = Some((file.cursor_row, file.cursor_column));
                        }
                        self.selecting = true;
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                let explorer = self.regions.explorer;
                let free = self.overlay == Overlay::None
                    && self.search.is_none()
                    && self.name_prompt.is_none()
                    && !self.splash_visible;
                if free && self.show_explorer && contains(explorer, point) && point.1 > explorer.y {
                    self.focus = Focus::Explorer;
                    let index = self.explorer_scroll + usize::from(point.1 - explorer.y - 1);
                    self.open_entry_menu((index < self.entries.len()).then_some(index), point);
                } else {
                    self.entry_menu = None;
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) if self.selecting => {
                let area = self.regions.pane_bodies[self.active_pane()];
                if let Some(file) = self
                    .active_tab
                    .and_then(|index| self.open_files.get_mut(index))
                    && file.image.is_none()
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
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => match self.drag {
                Some(DragTarget::Explorer) => {
                    self.explorer_width = mouse.column.saturating_add(1).clamp(18, 80)
                }
                Some(DragTarget::RightTerminal) => {
                    self.right_terminal_width = self
                        .regions
                        .body
                        .width
                        .saturating_sub(mouse.column.saturating_sub(self.regions.body.x))
                        .clamp(24, 100);
                }
                Some(DragTarget::BottomTerminal) => {
                    self.bottom_terminal_height = self
                        .regions
                        .body
                        .height
                        .saturating_sub(mouse.row.saturating_sub(self.regions.body.y))
                        .clamp(6, 40);
                }
                None => {}
            },
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                self.drag = None;
                self.selecting = false;
                if let Some(drag) = self.tab_drag.take()
                    && let Some(zone) = drag.zone
                {
                    self.drop_tab(drag, zone);
                }
            }
            MouseEventKind::Moved => self.hover(point),
            // Shift turns the wheel sideways, for terminals without a horizontal wheel.
            MouseEventKind::ScrollUp => self.scroll_at(point, true, sideways(mouse)),
            MouseEventKind::ScrollDown => self.scroll_at(point, false, sideways(mouse)),
            MouseEventKind::ScrollLeft => self.scroll_at(point, true, true),
            MouseEventKind::ScrollRight => self.scroll_at(point, false, true),
            _ => {}
        }
    }

    fn scroll_at(&mut self, point: (u16, u16), up: bool, sideways: bool) {
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
        } else if contains(self.regions.explorer, point) {
            self.explorer_scroll = if up {
                self.explorer_scroll.saturating_sub(3)
            } else {
                self.explorer_scroll
                    .saturating_add(3)
                    .min(self.entries.len().saturating_sub(1))
            };
        } else if contains(self.regions.bottom_terminal, point) {
            self.bottom_terminal.scroll(up, 3);
        } else if contains(self.regions.right_terminal, point) {
            self.right_terminal.scroll(up, 3);
        } else if let Some(index) = self.pane_at(point) {
            if let Some((page, _)) = self.open_files[index].page() {
                // One page per turn of the wheel, however many events it sends.
                let now = Instant::now();
                if self
                    .page_turned
                    .is_some_and(|last| now.duration_since(last) < Duration::from_millis(250))
                {
                    return;
                }
                self.page_turned = Some(now);
                let page = if up { page.saturating_sub(1) } else { page + 1 };
                self.open_files[index].show_page(page, &mut self.picker);
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

fn contains(area: Rect, point: (u16, u16)) -> bool {
    point.0 >= area.x
        && point.0 < area.x.saturating_add(area.width)
        && point.1 >= area.y
        && point.1 < area.y.saturating_add(area.height)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
        app.regions.explorer_handle = Rect::new(30, 1, 1, 20);
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
        assert_eq!(app.explorer_width, 40);
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
        app.insert_editor_character('!');
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
        // docs is the first row: Delete is the last entry of its menu.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        assert_eq!(
            app.entry_menu.as_ref().unwrap().actions.last(),
            Some(&EntryAction::Delete)
        );
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
        assert_eq!(menu.actions, [EntryAction::NewFile, EntryAction::NewFolder]);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.entry_menu.is_none());

        // On the src folder: a new file goes inside it, opens and is selected.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 3, 1);
        assert_eq!(app.entry_menu.as_ref().unwrap().actions.len(), 4);
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
        // Up from the top goes round to Delete, then Rename.
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
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
}
