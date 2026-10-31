//! The editor's side of IntelliSense (F26): the text sent to the language server a few times a
//! second and the list of suggestions under the cursor; and Code Intelligence (F31): hover,
//! definitions, Peek, references and Back.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;
use crate::{
    editor::{SearchMatch, SearchState},
    lsp::{Event, Goal, Item, Place, char_column},
};

/// How often edited text goes to the server: typing never waits for it.
pub const SYNC_INTERVAL: Duration = Duration::from_millis(250);
/// Rows of the list of suggestions.
pub const SHOWN_ITEMS: usize = 8;
/// How long the mouse rests on a word before its hover is asked (F31).
pub const HOVER_DELAY: Duration = Duration::from_millis(400);
/// Places Back remembers.
const MOST_BACK: usize = 50;

/// The hover of a word (F31): its place at edit `version`, where the mouse rests (none when a
/// key asked), whether it was asked and, once answered, the Markdown and how far it scrolled.
pub struct Hover {
    pub path: PathBuf,
    pub version: u64,
    pub row: usize,
    pub start: usize,
    pub mouse: Option<(u16, u16)>,
    since: Instant,
    asked: bool,
    pub text: Option<String>,
    pub scroll: u16,
    /// How far the box can scroll, as the view last drew it.
    pub overflow: u16,
}

/// The definition shown in a box over the editor without leaving the tab (Peek, F31): the
/// file's lines, the definition's row and column (characters) and the first row shown.
pub struct Peek {
    pub path: PathBuf,
    pub row: usize,
    pub column: usize,
    pub lines: Vec<String>,
    pub top: usize,
}

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

/// Where the word at character `column` of `line`, or the one just before it, starts.
fn word_at(line: &str, column: usize) -> Option<usize> {
    let characters = line.chars().collect::<Vec<_>>();
    let at = if characters.get(column).is_some_and(|c| is_word(*c)) {
        column
    } else if column > 0 && characters.get(column - 1).is_some_and(|c| is_word(*c)) {
        column - 1
    } else {
        return None;
    };
    Some(
        at - characters[..at]
            .iter()
            .rev()
            .take_while(|c| is_word(**c))
            .count(),
    )
}

/// The lines of the file at `path`: the open tab's, or the disk's.
fn file_lines(app: &App, path: &Path) -> Vec<String> {
    match app.open_files.iter().find(|file| file.path == path) {
        Some(file) => file.lines.clone(),
        None => std::fs::read_to_string(path)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default(),
    }
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

    /// The server of the active file gets its edits before a question about a position.
    fn sync_active(&mut self) -> Option<usize> {
        let index = self.active_tab?;
        let file = self.open_files.get(index)?;
        if self.lsp.behind(&file.path, file.edits) {
            self.lsp.change(&file.path, &file.text(), file.edits);
        }
        Some(index)
    }

    /// F12, Alt+F12, Shift+F12 and Ctrl+click: where the symbol at the cursor is defined, or
    /// used.
    pub(super) fn ask_places(&mut self, goal: Goal) {
        let Some(index) = self.sync_active() else {
            return;
        };
        let file = &self.open_files[index];
        let line = file.lines.get(file.cursor_row).cloned().unwrap_or_default();
        let asked = self.lsp.places(
            goal,
            &file.path,
            file.edits,
            file.cursor_row,
            file.cursor_column,
            &line,
        );
        self.status = match asked {
            Ok(()) if goal == Goal::References => "Looking for the references...".into(),
            Ok(()) => "Looking for the definition...".into(),
            Err(message) => message,
        };
    }

    /// Ctrl+K: the hover of the word at the cursor, asked at once.
    pub(super) fn ask_hover(&mut self) {
        let Some(index) = self.sync_active() else {
            return;
        };
        let file = &self.open_files[index];
        let line = file.lines.get(file.cursor_row).cloned().unwrap_or_default();
        let Some(start) = word_at(&line, file.cursor_column) else {
            self.status = "No symbol at the cursor".into();
            return;
        };
        let hover = Hover {
            path: file.path.clone(),
            version: file.edits,
            row: file.cursor_row,
            start,
            mouse: None,
            since: Instant::now(),
            asked: true,
            text: None,
            scroll: 0,
            overflow: 0,
        };
        match self
            .lsp
            .hover(&hover.path, hover.version, hover.row, start, &line)
        {
            Ok(()) => self.hover = Some(hover),
            Err(message) => self.status = message,
        }
    }

    /// The mouse moved to `point`, over character `column` of `row` in the file at `index`
    /// (none outside the text): on a word, its hover is asked once it rests there; elsewhere a
    /// hover the mouse opened closes, unless the mouse is over its box.
    pub(super) fn rest_mouse(&mut self, point: (u16, u16), at: Option<(usize, usize, usize)>) {
        let word = at.and_then(|(index, row, column)| {
            let file = self.open_files.get(index)?;
            let line = file.lines.get(row)?;
            line.chars().nth(column).filter(|c| is_word(*c))?;
            let start = word_at(line, column)?;
            Some((file, row, start))
        });
        if let Some((file, row, start)) = word {
            let same = self.hover.as_ref().is_some_and(|hover| {
                hover.path == file.path && (hover.row, hover.start) == (row, start)
            });
            if !same {
                self.hover = Some(Hover {
                    path: file.path.clone(),
                    version: file.edits,
                    row,
                    start,
                    mouse: Some(point),
                    since: Instant::now(),
                    asked: false,
                    text: None,
                    scroll: 0,
                    overflow: 0,
                });
            }
        } else if self
            .hover
            .as_ref()
            .is_some_and(|hover| hover.mouse.is_some())
            && !super::input::contains(self.regions.hover, point)
        {
            self.hover = None;
        }
    }

    /// Ctrl held over a word marks it as a link to its definition (F31); without Ctrl, or off
    /// a word, the mark goes.
    pub(super) fn mark_link(&mut self, at: Option<(usize, usize, usize)>, ctrl: bool) {
        let link = at.filter(|_| ctrl).and_then(|(index, row, column)| {
            let line = self.open_files.get(index)?.lines.get(row)?;
            line.chars().nth(column).filter(|c| is_word(*c))?;
            let start = word_at(line, column)?;
            let end = start + line.chars().skip(start).take_while(|c| is_word(*c)).count();
            Some((index, row, start, end))
        });
        if link.is_some() && self.link != link {
            self.status = "Ctrl+click: go to the definition".into();
        }
        self.link = link;
    }

    /// The hover of the word the mouse rested on for `HOVER_DELAY`, asked once.
    fn tick_hover(&mut self, now: Instant) {
        let Some(hover) = self
            .hover
            .as_mut()
            .filter(|hover| !hover.asked && now.duration_since(hover.since) >= HOVER_DELAY)
        else {
            return;
        };
        hover.asked = true;
        let (path, row, start) = (hover.path.clone(), hover.row, hover.start);
        let Some(file) = self.open_files.iter().find(|file| file.path == path) else {
            self.hover = None;
            return;
        };
        if self.lsp.behind(&file.path, file.edits) {
            self.lsp.change(&file.path, &file.text(), file.edits);
        }
        let line = file.lines.get(row).cloned().unwrap_or_default();
        // Without a server the mouse says nothing: it only passed by.
        if self
            .lsp
            .hover(&path, file.edits, row, start, &line)
            .is_err()
        {
            self.hover = None;
        }
    }

    /// The keys of an open hover or Peek; false when the key goes on to the editor (and the
    /// hover closes: the text it describes may change).
    pub(super) fn handle_intelligence_key(&mut self, key: KeyEvent) -> bool {
        if let Some(peek) = &mut self.peek {
            let last = peek.lines.len().saturating_sub(1);
            match key.code {
                KeyCode::Esc => self.peek = None,
                KeyCode::Up => peek.top = peek.top.saturating_sub(1),
                KeyCode::Down => peek.top = (peek.top + 1).min(last),
                KeyCode::PageUp => peek.top = peek.top.saturating_sub(10),
                KeyCode::PageDown => peek.top = (peek.top + 10).min(last),
                KeyCode::Enter => {
                    let (path, row, column) = (peek.path.clone(), peek.row, peek.column);
                    self.peek = None;
                    self.jump_to(path, row, column);
                }
                _ => {
                    self.peek = None;
                    return false;
                }
            }
            return true;
        }
        let Some(hover) = &mut self.hover else {
            return false;
        };
        match key.code {
            KeyCode::Esc if hover.text.is_some() => self.hover = None,
            KeyCode::Up if hover.overflow > 0 => hover.scroll = hover.scroll.saturating_sub(1),
            KeyCode::Down if hover.overflow > 0 => {
                hover.scroll = (hover.scroll + 1).min(hover.overflow);
            }
            _ => {
                self.hover = None;
                return false;
            }
        }
        true
    }

    /// Opens `path` at `row` and character `column`; where the cursor was goes to Back.
    pub(super) fn jump_to(&mut self, path: PathBuf, row: usize, column: usize) {
        if let Some(file) = self.active_tab.and_then(|index| self.open_files.get(index)) {
            self.back
                .push((file.path.clone(), file.cursor_row, file.cursor_column));
            if self.back.len() > MOST_BACK {
                self.back.remove(0);
            }
        }
        self.open_at(path, row, column);
    }

    fn open_at(&mut self, path: PathBuf, row: usize, column: usize) {
        match self.open_file(path) {
            Ok(()) => {
                if let Some(index) = self.active_tab {
                    let file = &mut self.open_files[index];
                    file.mark(false);
                    file.move_cursor_to(row, column);
                }
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// Alt+Left: back to the place before the last jump.
    pub(super) fn go_back(&mut self) {
        match self.back.pop() {
            Some((path, row, column)) => self.open_at(path, row, column),
            None => self.status = "No earlier place to go back to".into(),
        }
    }

    /// The answer to F12, Alt+F12 or Shift+F12 for the active file at edit `version`.
    fn places_answered(&mut self, path: PathBuf, version: u64, goal: Goal, places: Vec<Place>) {
        let Some(file) = self.active_tab.and_then(|index| self.open_files.get(index)) else {
            return;
        };
        // Another file, or text changed since: the places may be elsewhere now.
        if file.path != path || file.edits != version {
            return;
        }
        let (name, title) = match goal {
            Goal::References => ("references", "References"),
            Goal::Declaration => ("declaration", "Declarations"),
            Goal::TypeDefinition => ("type definition", "Type definitions"),
            Goal::Implementation => ("implementations", "Implementations"),
            Goal::Definition | Goal::Peek => ("definition", "Definitions"),
        };
        if places.is_empty() {
            self.status = format!("No {name} found");
            return;
        }
        let mut matches = Vec::new();
        let mut read: Option<(PathBuf, Vec<String>)> = None;
        for place in &places {
            if read.as_ref().is_none_or(|(path, _)| *path != place.path) {
                read = Some((place.path.clone(), file_lines(self, &place.path)));
            }
            let lines = read.as_ref().map(|(_, lines)| lines).expect("read above");
            let text = lines.get(place.row).cloned().unwrap_or_default();
            let column = char_column(&text, place.start);
            let end = if place.end_row == place.row {
                char_column(&text, place.end)
            } else {
                text.chars().count()
            };
            matches.push(SearchMatch {
                path: place.path.clone(),
                line: place.row,
                column,
                length: end.saturating_sub(column),
                text,
            });
        }
        self.status.clear();
        match goal {
            // One place goes there; references are always a list.
            _ if !matches!(goal, Goal::Peek | Goal::References) && matches.len() == 1 => {
                let found = matches.remove(0);
                self.jump_to(found.path, found.line, found.column);
            }
            Goal::Peek => {
                let found = matches.remove(0);
                self.peek = Some(Peek {
                    lines: file_lines(self, &found.path),
                    path: found.path,
                    row: found.line,
                    column: found.column,
                    top: found.line,
                });
            }
            _ => {
                matches.sort_by(|left, right| {
                    (&left.path, left.line, left.column).cmp(&(
                        &right.path,
                        right.line,
                        right.column,
                    ))
                });
                let first = &matches[0];
                let symbol = first
                    .text
                    .chars()
                    .skip(first.column)
                    .take(first.length)
                    .collect::<String>();
                self.search = Some(SearchState::fixed(title.into(), symbol, matches));
                self.motion.open_search(Instant::now());
            }
        }
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
        self.tick_hover(now);
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
            Event::Hover {
                path,
                version,
                row,
                column,
                text,
            } => {
                let current = self
                    .open_files
                    .iter()
                    .find(|file| file.path == path)
                    .map(|file| file.edits);
                let Some(hover) = self.hover.as_mut().filter(|hover| {
                    hover.path == path
                        && (hover.version, hover.row, hover.start) == (version, row, column)
                        && current == Some(version)
                }) else {
                    return;
                };
                if text.is_empty() {
                    if hover.mouse.is_none() {
                        self.status = "Nothing to show for this symbol".into();
                    }
                    self.hover = None;
                } else {
                    hover.text = Some(text);
                }
            }
            Event::Places {
                path,
                version,
                goal,
                places,
            } => self.places_answered(path, version, goal, places),
            Event::Message(message) => self.status = message,
        }
    }
}
