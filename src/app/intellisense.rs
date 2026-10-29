//! The editor's side of IntelliSense (F26): the text sent to the language server a few times a
//! second, the list of suggestions under the cursor and go to definition.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;
use crate::lsp::{Event, Item, char_column};

/// How often edited text goes to the server: typing never waits for it.
pub const SYNC_INTERVAL: Duration = Duration::from_millis(250);
/// Rows of the list of suggestions.
pub const SHOWN_ITEMS: usize = 8;

/// The suggestions open under the cursor: for the word that starts at `start` on `row`.
pub struct Completion {
    pub items: Vec<Item>,
    pub selected: usize,
    pub row: usize,
    pub start: usize,
}

impl Completion {
    /// The suggestions that match `typed`, best first: starting with it, then containing it.
    pub fn visible<'a>(&'a self, typed: &str) -> Vec<&'a Item> {
        let typed = typed.to_lowercase();
        let (mut starting, mut containing) = (Vec::new(), Vec::new());
        for item in &self.items {
            let label = item.label.to_lowercase();
            if label.starts_with(&typed) {
                starting.push(item);
            } else if label.contains(&typed) {
                containing.push(item);
            }
        }
        starting.extend(containing);
        starting
    }
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

impl App {
    /// The word before the cursor of the active file: its row, first column and text.
    fn word_before_cursor(&self) -> Option<(usize, usize, String)> {
        let file = self.open_files.get(self.active_tab?)?;
        let line = file.lines.get(file.cursor_row)?;
        let before = line.chars().take(file.cursor_column).collect::<Vec<_>>();
        let start = before.len()
            - before
                .iter()
                .rev()
                .take_while(|character| is_word(**character))
                .count();
        let word = before[start..].iter().collect();
        Some((file.cursor_row, start, word))
    }

    /// The suggestions on screen and the word they complete.
    pub fn completion_view(&self) -> Option<(Vec<&Item>, usize, String)> {
        let completion = self.completion.as_ref()?;
        let (row, start, word) = self.word_before_cursor()?;
        if row != completion.row || start != completion.start {
            return None;
        }
        Some((completion.visible(&word), completion.selected, word))
    }

    /// Ctrl+Space, a trigger character or the second letter of a word: the server is sent the
    /// text as it is now and asked what comes next.
    pub(super) fn request_completion(&mut self) {
        let Some(index) = self.active_tab else {
            return;
        };
        let file = &self.open_files[index];
        if self.lsp.behind(&file.path, file.edits) {
            self.lsp.change(&file.path, &file.text(), file.edits);
        }
        let line = file.lines.get(file.cursor_row).cloned().unwrap_or_default();
        self.lsp
            .complete(&file.path, file.cursor_row, file.cursor_column, &line);
    }

    /// After a character is typed: ask for suggestions when it calls for them.
    pub(super) fn typed(&mut self, character: char) {
        if self.completion.is_some() {
            if !is_word(character) {
                self.completion = None;
            }
            return;
        }
        let Some((row, column)) = self
            .active_tab
            .and_then(|index| self.open_files.get(index))
            .map(|file| (file.cursor_row, file.cursor_column))
        else {
            return;
        };
        let line = &self.open_files[self.active_tab.unwrap_or(0)].lines[row];
        let before = line.chars().take(column).collect::<Vec<_>>();
        let trigger = match before.as_slice() {
            [.., '.'] => true,
            [.., ':', ':'] | [.., '-', '>'] => true,
            // The second letter of a word, not every letter: fewer requests while typing.
            [.., previous, last] => {
                is_word(*last)
                    && is_word(*previous)
                    && before.len() - before.iter().rev().take_while(|c| is_word(**c)).count()
                        == column - 2
            }
            _ => false,
        };
        if trigger {
            self.request_completion();
        }
    }

    /// The keys of the open list: arrows choose, Enter or Tab take, Esc closes. False when the
    /// key is the editor's.
    pub(super) fn handle_completion_key(&mut self, key: KeyEvent) -> bool {
        let Some((count, word)) = self
            .completion_view()
            .map(|(items, _, word)| (items.len(), word))
        else {
            self.completion = None;
            return false;
        };
        if count == 0 {
            self.completion = None;
            return false;
        }
        let Some(completion) = &mut self.completion else {
            return false;
        };
        match key.code {
            KeyCode::Up => completion.selected = (completion.selected + count - 1) % count,
            KeyCode::Down => completion.selected = (completion.selected + 1) % count,
            KeyCode::Esc => self.completion = None,
            KeyCode::Enter | KeyCode::Tab => {
                let selected = completion.selected.min(count - 1);
                let insert = completion.visible(&word)[selected].insert.clone();
                let start = completion.start;
                self.completion = None;
                if let Some(index) = self.active_tab {
                    let file = &mut self.open_files[index];
                    file.anchor = Some((file.cursor_row, start));
                    file.insert_str(&insert);
                }
            }
            KeyCode::Char(_) | KeyCode::Backspace
                if !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Typing narrows the list from the start again.
                completion.selected = 0;
                return false;
            }
            _ => {
                self.completion = None;
                return false;
            }
        }
        true
    }

    /// F12: asks where the symbol under the cursor is defined.
    pub(super) fn go_to_definition(&mut self) {
        let Some(file) = self.active_tab.and_then(|index| self.open_files.get(index)) else {
            return;
        };
        let line = file.lines.get(file.cursor_row).cloned().unwrap_or_default();
        self.lsp
            .definition(&file.path, file.cursor_row, file.cursor_column, &line);
        self.status = "Looking for the definition...".into();
    }

    /// In the tick: edited text to the servers, then what they answered.
    pub(super) fn tick_lsp(&mut self, now: Instant) {
        if now.duration_since(self.lsp_synced) >= SYNC_INTERVAL {
            self.lsp_synced = now;
            for file in &self.open_files {
                if self.lsp.behind(&file.path, file.edits) {
                    self.lsp.change(&file.path, &file.text(), file.edits);
                }
            }
        }
        for event in self.lsp.poll() {
            self.lsp_event(event);
        }
    }

    pub fn lsp_event(&mut self, event: Event) {
        match event {
            Event::Completion {
                path,
                row,
                column,
                items,
            } => {
                let Some(file) = self.active_tab.and_then(|index| self.open_files.get(index))
                else {
                    return;
                };
                // The cursor moved to another line or file since: the answer is late.
                if file.path != path || file.cursor_row != row || items.is_empty() {
                    return;
                }
                let start = file.lines[row]
                    .chars()
                    .take(column.min(file.cursor_column))
                    .collect::<Vec<_>>();
                let start = start.len() - start.iter().rev().take_while(|c| is_word(**c)).count();
                self.completion = Some(Completion {
                    items,
                    selected: 0,
                    row,
                    start,
                });
                if self
                    .completion_view()
                    .is_none_or(|(items, ..)| items.is_empty())
                {
                    self.completion = None;
                }
            }
            Event::Definition { path, row, column } => match self.open_file(path) {
                Ok(()) => {
                    if let Some(index) = self.active_tab {
                        let file = &mut self.open_files[index];
                        let column = file
                            .lines
                            .get(row)
                            .map_or(0, |line| char_column(line, column));
                        file.mark(false);
                        file.move_cursor_to(row, column);
                    }
                }
                Err(error) => self.status = error.to_string(),
            },
            Event::Message(message) => self.status = message,
        }
    }
}
