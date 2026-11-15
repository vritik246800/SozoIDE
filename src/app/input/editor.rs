//! Keys of the editor, and what the mouse rests on in its text.

use super::*;

impl App {
    pub(in crate::app) fn handle_editor_key(&mut self, key: KeyEvent) {
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
                KeyCode::Char('c' | 'C') => Some(Control::Captions),
                KeyCode::Char('a' | 'A') => Some(Control::AudioTrack),
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
            // Ctrl+L goes to a page; +, -, 0 and Shift+arrows zoom a page drawn whole.
            if self.keys.action(key) == Some(Action::GoToLine) {
                return self.open_go_to_line();
            }
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            let arrow = match key.code {
                KeyCode::Left if shift => Some((-1.0, 0.0)),
                KeyCode::Right if shift => Some((1.0, 0.0)),
                KeyCode::Up if shift => Some((0.0, -1.0)),
                KeyCode::Down if shift => Some((0.0, 1.0)),
                _ => None,
            };
            let character = match key.code {
                KeyCode::Char(character) if !ctrl => Some(character),
                _ => None,
            };
            let file = &mut self.open_files[active];
            if file.zoom_page(character, arrow, &mut self.picker) {
                return;
            }
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
            Some(Action::StepOut) => return self.debug_step("stepOut"),
            Some(Action::RunToCursor) => return self.run_to_cursor(),
            Some(Action::PauseDebug) => return self.pause_debug(),
            Some(Action::BreakpointOptions) => return self.open_breakpoint_options(),
            Some(Action::Evaluate) => {
                return self.open_debug_input(super::super::DebugInputKind::Evaluate);
            }
            Some(Action::AddWatch) => {
                return self.open_debug_input(super::super::DebugInputKind::Watch);
            }
            // The editing of TODO 2.1–2.2 (`app/editing.rs`).
            Some(Action::Redo) => return self.redo(active),
            Some(Action::ToggleComment) => return self.toggle_comment(active),
            Some(Action::WrapLines) => return self.open_files[active].toggle_wrap(),
            Some(Action::GoToLine) => return self.open_go_to_line(),
            Some(Action::Replace) => return self.open_replace(),
            Some(Action::FormatDocument) => return self.format_file(),
            Some(Action::CodeActions) => return self.ask_code_actions(),
            // Git of the open file (TODO 4.1.6, `app/blame.rs`).
            Some(Action::Blame) => return self.toggle_blame(active),
            Some(Action::FileHistory) => return self.file_history(active),
            _ => {}
        }
        let now = self.motion.now();
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if ctrl && shift && matches!(key.code, KeyCode::Char('z' | 'Z')) {
            return self.redo(active);
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
            // Lines: move (Alt+Up/Down), duplicate (Shift+Alt+Down), delete (Ctrl+Shift+K).
            KeyCode::Down if alt && shift => file.duplicate_lines(),
            KeyCode::Up | KeyCode::Down if alt => file.move_lines(key.code == KeyCode::Down),
            KeyCode::Char('k' | 'K') if ctrl && shift => file.delete_lines(),
            // Words: Ctrl+Left/Right move, Ctrl+Backspace/Delete remove.
            KeyCode::Left | KeyCode::Right if ctrl => {
                file.mark(shift);
                file.move_word(key.code == KeyCode::Right);
            }
            KeyCode::Backspace if ctrl => file.delete_word(false),
            KeyCode::Delete if ctrl => file.delete_word(true),
            KeyCode::Home => {
                file.mark(shift);
                file.smart_home();
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right | KeyCode::End => {
                // Shift selects as the cursor moves; without it the selection ends.
                file.mark(key.modifiers.contains(KeyModifiers::SHIFT));
                match key.code {
                    KeyCode::Up => file.move_cursor(-1, 0),
                    KeyCode::Down => file.move_cursor(1, 0),
                    KeyCode::Left => file.move_cursor(0, -1),
                    KeyCode::Right => file.move_cursor(0, 1),
                    _ => {
                        let column = file.lines[file.cursor_row].chars().count();
                        file.move_cursor_to(file.cursor_row, column);
                    }
                }
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                file.scroll = if key.code == KeyCode::PageUp {
                    file.scroll.saturating_sub(10)
                } else {
                    file.scroll.saturating_add(10)
                };
                // A page runs there in steps (`SCROLL`); the wheel and the cursor jump.
                file.shown_scroll
                    .set_target(usize::from(file.scroll), now, true);
            }
            KeyCode::Enter => file.new_line(),
            KeyCode::Tab if shift => file.outdent_lines(),
            KeyCode::Tab => file.indent_lines(),
            KeyCode::BackTab => file.outdent_lines(),
            KeyCode::Backspace => file.backspace(),
            KeyCode::Delete => file.delete(),
            KeyCode::Char('a' | 'A') if ctrl => file.select_all(),
            KeyCode::Char('c' | 'C') if ctrl => copied = file.selected_text(),
            KeyCode::Char('x' | 'X') if ctrl => {
                copied = file.selected_text();
                file.delete_selection();
            }
            KeyCode::Char(character @ ('(' | '[' | '{' | '"' | '\'' | '`')) if !ctrl => {
                file.insert_pair(character);
                self.typed(character);
            }
            KeyCode::Char(character @ (')' | ']' | '}')) if !ctrl => {
                file.insert_closing(character);
                self.typed(character);
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

    pub(in crate::app) fn keep_selection_visible(&mut self) {
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

    /// The cursor to `heading` of the active document (its text or its anchor).
    pub(in crate::app) fn go_to_heading(&mut self, heading: Option<&str>) {
        let (Some(heading), Some(active)) = (heading, self.active_tab) else {
            return;
        };
        let file = &mut self.open_files[active];
        match crate::editor::heading_row(&file.lines, heading) {
            Some(row) => {
                file.anchor = None;
                file.move_cursor_to(row, 0);
                file.scroll = file.display_row(row).saturating_sub(1) as u16;
                file.follow_cursor = false;
            }
            None => self.status = format!("No heading #{heading}"),
        }
    }

    /// A `[text](target)`: an address in the browser, a document of the project at its
    /// `#heading`, a heading of this one; a `[^note]`: its definition (TODO 5.1.9).
    pub(in crate::app) fn follow_pointer(&mut self, pointer: crate::editor::Pointer) {
        let Some(active) = self.active_tab else {
            return;
        };
        match pointer {
            crate::editor::Pointer::Note(label) => {
                let wanted = format!("[^{label}]:");
                let file = &mut self.open_files[active];
                match file.lines.iter().position(|line| line.starts_with(&wanted)) {
                    Some(row) => {
                        file.anchor = None;
                        file.move_cursor_to(row, 0);
                        file.follow_cursor = true;
                    }
                    None => self.status = format!("No note [^{label}]"),
                }
            }
            crate::editor::Pointer::Target(target) => {
                let lower = target.to_ascii_lowercase();
                if lower.starts_with("http://") || lower.starts_with("https://") {
                    self.status = match crate::terminal::web_address(&target) {
                        Some(url) if cfg!(test) => format!("Opened {url}"),
                        Some(url) => match crate::app::opener(std::env::consts::OS, &url)
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                        {
                            Ok(_) => format!("Opened {url}"),
                            Err(error) => format!("Could not open the browser: {error}"),
                        },
                        None => "That address does not open here".into(),
                    };
                    return;
                }
                if lower.contains(':') {
                    self.status = "Only web addresses and files of the project open".into();
                    return;
                }
                let (path, heading) = match target.split_once('#') {
                    Some((path, heading)) => (path.to_owned(), Some(heading.to_owned())),
                    None => (target.clone(), None),
                };
                if !path.is_empty() {
                    let from = self.open_files[active].path.clone();
                    let Some(found) = crate::editor::find_document(&path, Some(&from), &self.root)
                    else {
                        self.status = format!("No file {path} in the project");
                        return;
                    };
                    if let Err(error) = self.open_file(found) {
                        self.status = format!("{error:#}");
                        return;
                    }
                }
                self.go_to_heading(heading.as_deref());
            }
        }
    }

    /// Opens the document of a `[[ ]]`, or goes to its tab (F40).
    pub(in crate::app) fn open_wiki_link(&mut self, target: &str) {
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

    /// The file under `point` and the row and character there, when it is over the text of
    /// an editor with nothing drawn on top.
    pub(in crate::app) fn text_under(&self, point: (u16, u16)) -> Option<(usize, usize, usize)> {
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
    pub(in crate::app) fn hover(&mut self, point: (u16, u16)) {
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
