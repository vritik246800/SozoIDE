use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    editor::{SearchScope, SearchState},
    update::UpdateState,
};

use super::{App, DragTarget, Focus, Overlay};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        // macOS terminals send Ctrl+Shift+B as Ctrl+B, so the Mac uses Ctrl+Option+B.
        let right_terminal_modifier = if cfg!(target_os = "macos") {
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
            } else if matches!(key.code, KeyCode::Esc | KeyCode::F(1) | KeyCode::F(2)) {
                self.close_overlay();
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
                    let current_file = self
                        .active_tab
                        .and_then(|index| self.open_files.get(index))
                        .map(|file| file.path.clone());
                    self.git.open(current_file);
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
        match key.code {
            KeyCode::Esc => self.close_search(),
            KeyCode::Up => {
                if let Some(search) = &mut self.search {
                    search.select_previous();
                }
            }
            KeyCode::Down => {
                if let Some(search) = &mut self.search {
                    search.select_next();
                }
            }
            KeyCode::Enter => {
                let selected = self
                    .search
                    .as_ref()
                    .and_then(SearchState::selected_match)
                    .cloned();
                if let Some(found) = selected {
                    if let Err(error) = self.open_file(found.path) {
                        self.status = error.to_string();
                    } else if let Some(active) = self.active_tab {
                        self.open_files[active].scroll = found.line as u16;
                    }
                    self.close_search();
                }
            }
            KeyCode::Backspace => {
                if let Some(search) = &mut self.search {
                    search.backspace(
                        &self.root,
                        self.active_tab.and_then(|index| self.open_files.get(index)),
                    );
                }
            }
            KeyCode::Char(character) if !ctrl => {
                if let Some(search) = &mut self.search {
                    search.push(
                        character,
                        &self.root,
                        self.active_tab.and_then(|index| self.open_files.get(index)),
                    );
                }
            }
            _ => {}
        }
    }

    pub fn search_paste(&mut self, text: &str) {
        if self.motion.search_closing() {
            return;
        }
        for character in text.chars().filter(|character| !character.is_control()) {
            if let Some(search) = &mut self.search {
                search.push(
                    character,
                    &self.root,
                    self.active_tab.and_then(|index| self.open_files.get(index)),
                );
            }
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
        self.overlay = overlay;
        self.motion.open_overlay(std::time::Instant::now());
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
        if self.open_files[active].image.is_some() {
            if key.code == KeyCode::Char('w') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.close_tab(active);
            }
            return;
        }
        let file = &mut self.open_files[active];
        match key.code {
            KeyCode::Up => file.move_cursor(-1, 0),
            KeyCode::Down => file.move_cursor(1, 0),
            KeyCode::Left => file.move_cursor(0, -1),
            KeyCode::Right => file.move_cursor(0, 1),
            KeyCode::Home => file.move_cursor_to(file.cursor_row, 0),
            KeyCode::End => {
                let column = file.lines[file.cursor_row].chars().count();
                file.move_cursor_to(file.cursor_row, column);
            }
            KeyCode::PageUp => file.scroll = file.scroll.saturating_sub(10),
            KeyCode::PageDown => file.scroll = file.scroll.saturating_add(10),
            KeyCode::Enter => file.insert('\n'),
            KeyCode::Tab => file.insert('\t'),
            KeyCode::Backspace => file.backspace(),
            KeyCode::Delete => file.delete(),
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.close_tab(active)
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                file.insert(character);
            }
            _ => {}
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
                        self.active_tab = Some(index);
                    }
                } else if self.show_bottom_terminal && contains(self.regions.bottom_terminal, point)
                {
                    self.focus = Focus::BottomTerminal;
                } else if self.show_right_terminal && contains(self.regions.right_terminal, point) {
                    self.focus = Focus::RightTerminal;
                } else if contains(self.regions.editor, point) {
                    self.focus = Focus::Editor;
                    if !self.show_git
                        && !self.show_database
                        && let Some(file) = self
                            .active_tab
                            .and_then(|index| self.open_files.get_mut(index))
                        && file.image.is_none()
                    {
                        let area = self.regions.editor;
                        let row = file.scroll as usize
                            + mouse.row.saturating_sub(area.y.saturating_add(1)) as usize;
                        let gutter = file.lines.len().max(1).to_string().len() + 3;
                        let text_start = area.x.saturating_add(1).saturating_add(gutter as u16);
                        let visual_column = mouse.column.saturating_sub(text_start) as usize;
                        let column = file.cursor_column_for_click(row, visual_column);
                        file.move_cursor_to(row, column);
                    }
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
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => self.drag = None,
            MouseEventKind::ScrollUp => self.scroll_at(point, true),
            MouseEventKind::ScrollDown => self.scroll_at(point, false),
            _ => {}
        }
    }

    fn scroll_at(&mut self, point: (u16, u16), up: bool) {
        if self.show_database && contains(self.database.regions.content, point) {
            self.database.scroll(up);
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
        } else if contains(self.regions.editor, point)
            && let Some(active) = self.active_tab
        {
            self.open_files[active].scroll = if up {
                self.open_files[active].scroll.saturating_sub(3)
            } else {
                self.open_files[active].scroll.saturating_add(3)
            };
        }
    }
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
        let right_terminal_key = if cfg!(target_os = "macos") {
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
