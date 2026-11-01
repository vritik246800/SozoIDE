//! The editor's side of IntelliSense (F26): the text sent to the language server a few times a
//! second and the list of suggestions under the cursor; and Code Intelligence (F31): hover,
//! definitions, Peek, references and Back.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, EntryAction, keys::Action, tasks::Confirm};
use crate::{
    editor::{SearchMatch, SearchState},
    lsp::{Event, Goal, Item, Place, char_column, server_for},
    run::shell_line,
};

/// How often edited text goes to the server: typing never waits for it.
pub const SYNC_INTERVAL: Duration = Duration::from_millis(250);
/// Rows of the list of suggestions.
pub const SHOWN_ITEMS: usize = 8;
/// How long the mouse rests on a word with Ctrl before its hover is asked (F31, Q72).
pub const HOVER_DELAY: Duration = Duration::from_secs(2);
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

/// A question to a language server about the symbol at a place of the file at `path` (F31):
/// kept to ask it again once its server is installed (Q74).
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Places {
        goal: Goal,
        path: PathBuf,
        row: usize,
        column: usize,
    },
    /// At the word that starts at `start`.
    Hover {
        path: PathBuf,
        row: usize,
        start: usize,
    },
}

impl Request {
    pub fn path(&self) -> &Path {
        match self {
            Self::Places { path, .. } | Self::Hover { path, .. } => path,
        }
    }

    /// What it was for, as the install dialog says it.
    fn purpose(&self) -> String {
        let what = match self {
            Self::Places { goal, .. } => EntryAction::Code(*goal).label(),
            Self::Hover { .. } => Action::Hover.title(),
        };
        match self.path().extension() {
            Some(extension) => format!("{what} in .{} files", extension.to_string_lossy()),
            None => what.to_owned(),
        }
    }
}

/// A language server being installed in the bottom terminal (Q74): once it is there, the
/// request that needed it is asked again.
pub struct ServerWatch {
    pub name: &'static str,
    arrived: Receiver<()>,
    request: Request,
}

/// A row of the menu's preview that a click acts on: a place, the rest of the list or the
/// installation of the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewRow {
    Place(usize),
    More,
    Install,
}

/// The answers beside the editor's menu for its code entry under the mouse (F31, Q73), about
/// the symbol at the cursor when the menu opened: asked once, then the places (with the lines
/// of the definition's file for Peek) or why there are none.
pub struct MenuPreview {
    pub goal: Goal,
    pub path: PathBuf,
    pub version: u64,
    pub row: usize,
    pub column: usize,
    pub places: Option<Vec<SearchMatch>>,
    pub lines: Vec<String>,
    pub failure: Option<String>,
    /// Whether installing the server would answer it.
    pub install: bool,
    /// The row under the mouse.
    pub hovered: Option<PreviewRow>,
    /// The entry was clicked before the answer: it acts when the answer comes.
    act: bool,
}

impl MenuPreview {
    fn request(&self) -> Request {
        Request::Places {
            goal: self.goal,
            path: self.path.clone(),
            row: self.row,
            column: self.column,
        }
    }
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

/// What the answers to `goal` are called: in a sentence and as a title.
pub fn goal_names(goal: Goal) -> (&'static str, &'static str) {
    match goal {
        Goal::References => ("references", "References"),
        Goal::Declaration => ("declaration", "Declarations"),
        Goal::TypeDefinition => ("type definition", "Type definitions"),
        Goal::Implementation => ("implementations", "Implementations"),
        Goal::Definition | Goal::Peek => ("definition", "Definitions"),
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
        self.ask(Request::Places {
            goal,
            path: file.path.clone(),
            row: file.cursor_row,
            column: file.cursor_column,
        });
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
        self.ask(Request::Hover {
            path: file.path.clone(),
            row: file.cursor_row,
            start,
        });
    }

    /// Asks `request` about its file as it is now (its edits go to the server first): the
    /// footer says what it waits for, or why it cannot, and a server that is missing offers
    /// to install itself (Q74).
    fn ask(&mut self, request: Request) {
        let Some(file) = self
            .open_files
            .iter()
            .find(|file| file.path == request.path())
        else {
            return;
        };
        if self.lsp.behind(&file.path, file.edits) {
            self.lsp.change(&file.path, &file.text(), file.edits);
        }
        let version = file.edits;
        let asked = match &request {
            Request::Places {
                goal,
                path,
                row,
                column,
            } => {
                let line = file.lines.get(*row).cloned().unwrap_or_default();
                let asked = self.lsp.places(*goal, path, version, *row, *column, &line);
                if asked.is_ok() {
                    self.status = if *goal == Goal::References {
                        "Looking for the references...".into()
                    } else {
                        "Looking for the definition...".into()
                    };
                }
                asked
            }
            Request::Hover { path, row, start } => {
                let line = file.lines.get(*row).cloned().unwrap_or_default();
                let asked = self.lsp.hover(path, version, *row, *start, &line);
                if asked.is_ok() {
                    self.hover = Some(Hover {
                        path: path.clone(),
                        version,
                        row: *row,
                        start: *start,
                        mouse: None,
                        since: Instant::now(),
                        asked: true,
                        text: None,
                        scroll: 0,
                        overflow: 0,
                    });
                }
                asked
            }
        };
        match asked {
            Ok(()) => self.waiting = Some(request),
            Err(message) => self.offer_install(request, message),
        }
    }

    /// `request` failed with `message`: when its server is missing or could not start and
    /// this system can install it, the command dialog offers to (Q74, only after Enter as
    /// Q65 asks) and the request runs again once it is there; otherwise the footer says why.
    pub(super) fn offer_install(&mut self, request: Request, message: String) {
        let Some(name) = self.lsp.installable(request.path()) else {
            self.status = message;
            return;
        };
        if self
            .server_watch
            .as_ref()
            .is_some_and(|watch| watch.name == name)
        {
            self.status = format!("Waiting for {name} to be installed...");
            return;
        }
        let Some(command) = (self.installs)(name) else {
            self.status = format!("{message}. No package manager here installs it: do it by hand");
            return;
        };
        let line = shell_line(&command, std::env::consts::OS);
        self.confirm = Some(Confirm {
            title: format!("Install {name}"),
            rows: vec![
                ("For", request.purpose()),
                ("Why", message),
                ("Component", format!("{name} (Language server)")),
                ("Command", line.clone()),
            ],
            running: format!("Installing {name}: {line}"),
            line,
            task: false,
            server: Some((name, request)),
            component: None,
        });
    }

    /// Enter on the install dialog: the server is watched until it is there.
    pub(super) fn watch_server(&mut self, name: &'static str, request: Request) {
        self.server_watch = Some(ServerWatch {
            name,
            arrived: (self.watcher)(name),
            request,
        });
    }

    /// In the tick: a server installed is started again with the files of its language, and
    /// the request that needed it is asked again; one that never came is said.
    fn tick_install(&mut self) {
        let Some(watch) = &self.server_watch else {
            return;
        };
        match watch.arrived.try_recv() {
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.status = format!("{} is still not installed", watch.name);
                self.server_watch = None;
            }
            Ok(()) => {
                let Some(watch) = self.server_watch.take() else {
                    return;
                };
                self.start_again(watch.name);
                self.ask(watch.request);
            }
        }
    }

    /// Server `name`, now installed, starts again with the open files of its language.
    pub(super) fn start_again(&mut self, name: &str) {
        self.lsp.restart(name);
        for file in &self.open_files {
            if file.is_text()
                && server_for(&file.path).is_some_and(|server| server.0 == name)
                && let Some(message) = self.lsp.open(&file.path, &file.text(), file.edits)
            {
                self.status = message;
            }
        }
    }

    /// In the tick: a request still waiting when its server failed (it stopped before
    /// answering: rustup's `rust-analyzer` without its component) offers the installation;
    /// so does the menu's preview, which says why.
    fn tick_failures(&mut self) {
        if let Some(request) = self.waiting.clone()
            && let Some((name, failure)) = self.lsp.failure(request.path())
        {
            let message = failure.message(name);
            self.waiting = None;
            if request.path()
                == self
                    .hover
                    .as_ref()
                    .map_or(Path::new(""), |hover| &hover.path)
            {
                self.hover = None;
            }
            self.offer_install(request, message);
        }
        if self.entry_menu.is_none()
            && self
                .menu_preview
                .as_ref()
                .is_some_and(|preview| !preview.act)
        {
            self.menu_preview = None;
        }
        let failed = self.menu_preview.as_ref().and_then(|preview| {
            let waiting = preview.places.is_none() && preview.failure.is_none();
            let (name, failure) = self.lsp.failure(&preview.path).filter(|_| waiting)?;
            Some((
                failure.message(name),
                self.lsp.installable(&preview.path).is_some(),
            ))
        });
        if let Some((message, install)) = failed
            && let Some(preview) = &mut self.menu_preview
        {
            if preview.act {
                let request = preview.request();
                self.menu_preview = None;
                self.offer_install(request, message);
            } else {
                preview.failure = Some(message);
                preview.install = install;
            }
        }
    }

    /// The menu's entry under the mouse or the arrows changed (Q73): a code entry asks its
    /// question about the symbol at the cursor once, and its answers fill the preview beside
    /// the menu; other entries have none.
    pub(super) fn preview_entry(&mut self) {
        let goal = match self
            .entry_menu
            .as_ref()
            .filter(|menu| menu.editor)
            .and_then(|menu| menu.actions.get(menu.selected))
        {
            Some(EntryAction::Code(goal)) => *goal,
            _ => {
                self.menu_preview = None;
                return;
            }
        };
        if self
            .menu_preview
            .as_ref()
            .is_some_and(|preview| preview.goal == goal)
        {
            return;
        }
        let Some(index) = self.sync_active() else {
            return;
        };
        let file = &self.open_files[index];
        let line = file.lines.get(file.cursor_row).cloned().unwrap_or_default();
        let mut preview = MenuPreview {
            goal,
            path: file.path.clone(),
            version: file.edits,
            row: file.cursor_row,
            column: file.cursor_column,
            places: None,
            lines: Vec::new(),
            failure: None,
            install: false,
            hovered: None,
            act: false,
        };
        if let Err(message) = self.lsp.places(
            goal,
            &preview.path,
            preview.version,
            preview.row,
            preview.column,
            &line,
        ) {
            preview.install = self.lsp.installable(&preview.path).is_some();
            preview.failure = Some(message);
        }
        self.menu_preview = Some(preview);
    }

    /// A code entry of the editor's menu clicked: what its answers say, now or once they come;
    /// a server that is missing offers to install itself.
    pub(super) fn code_entry(&mut self, goal: Goal) {
        let Some(preview) = self
            .menu_preview
            .as_mut()
            .filter(|preview| preview.goal == goal)
        else {
            self.menu_preview = None;
            return self.ask_places(goal);
        };
        if let Some(message) = preview.failure.clone() {
            let request = preview.request();
            self.menu_preview = None;
            return self.offer_install(request, message);
        }
        match preview.places.take() {
            Some(matches) => {
                self.menu_preview = None;
                self.act_on(goal, matches);
            }
            None => {
                preview.act = true;
                self.status = if goal == Goal::References {
                    "Looking for the references...".into()
                } else {
                    "Looking for the definition...".into()
                };
            }
        }
    }

    /// A click on a row of the preview: a place opens (in its own tab when it is another
    /// file), More shows them all as the entry does, Install offers the server.
    pub(super) fn choose_preview_row(&mut self, row: PreviewRow) {
        self.entry_menu = None;
        let Some(mut preview) = self.menu_preview.take() else {
            return;
        };
        match row {
            PreviewRow::Place(index) => {
                if let Some(found) = preview
                    .places
                    .as_mut()
                    .filter(|places| index < places.len())
                    .map(|places| places.swap_remove(index))
                {
                    self.jump_to(found.path, found.line, found.column);
                }
            }
            PreviewRow::More => {
                if let Some(matches) = preview.places.take() {
                    self.act_on(preview.goal, matches);
                }
            }
            PreviewRow::Install => {
                let message = preview.failure.take().unwrap_or_default();
                self.offer_install(preview.request(), message);
            }
        }
    }

    /// The mouse moved to `point`, over character `column` of `row` in the file at `index`
    /// (none outside the text), with Ctrl held or not: on a word with Ctrl, its hover is asked
    /// once it rests there (Q72; the terminal sends no Ctrl alone, so the time counts from the
    /// last move with it); without Ctrl a hover not asked yet is dropped and an open one stays
    /// on its word and its box; elsewhere a hover the mouse opened closes.
    pub(super) fn rest_mouse(
        &mut self,
        point: (u16, u16),
        at: Option<(usize, usize, usize)>,
        ctrl: bool,
    ) {
        if super::input::contains(self.regions.hover, point) {
            return;
        }
        let word = at.and_then(|(index, row, column)| {
            let file = self.open_files.get(index)?;
            let line = file.lines.get(row)?;
            line.chars().nth(column).filter(|c| is_word(*c))?;
            let start = word_at(line, column)?;
            Some((file, row, start))
        });
        let mouse = self
            .hover
            .as_ref()
            .is_some_and(|hover| hover.mouse.is_some());
        if let Some((file, row, start)) = word {
            let same = self.hover.as_ref().is_some_and(|hover| {
                hover.path == file.path && (hover.row, hover.start) == (row, start)
            });
            if same && !ctrl && self.hover.as_ref().is_some_and(|hover| !hover.asked) {
                self.hover = None;
            } else if !same && ctrl {
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
            } else if !same && mouse {
                self.hover = None;
            }
        } else if mouse {
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
        if let Err(message) = self.lsp.hover(&path, file.edits, row, start, &line) {
            // A server that failed says why in the box, and how to install it (Q75); without
            // one for the language the mouse says nothing.
            let why = self
                .lsp
                .failure(&path)
                .map(|_| match self.lsp.installable(&path) {
                    Some(_) => format!(
                        "{message}\n\n{} or Ctrl+click offers to install it",
                        self.keys.label(super::keys::Action::Definition)
                    ),
                    None => message,
                });
            match (why, self.hover.as_mut()) {
                (Some(why), Some(hover)) => hover.text = Some(why),
                _ => self.hover = None,
            }
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

    /// The answer to F12, Alt+F12, Shift+F12 or a code entry of the editor's menu for the
    /// active file at edit `version`; the menu's preview takes the one it waits for.
    fn places_answered(&mut self, path: PathBuf, version: u64, goal: Goal, places: Vec<Place>) {
        let previewed = self.menu_preview.as_ref().is_some_and(|preview| {
            preview.goal == goal
                && preview.path == path
                && preview.version == version
                && preview.places.is_none()
                && preview.failure.is_none()
        });
        if previewed {
            let matches = self.matches_of(&places);
            let lines = match matches.first() {
                Some(first) if goal == Goal::Peek => file_lines(self, &first.path),
                _ => Vec::new(),
            };
            let Some(preview) = &mut self.menu_preview else {
                return;
            };
            if preview.act {
                self.menu_preview = None;
                self.act_on(goal, matches);
            } else {
                preview.places = Some(matches);
                preview.lines = lines;
            }
            return;
        }
        let Some(file) = self.active_tab.and_then(|index| self.open_files.get(index)) else {
            return;
        };
        // Another file, or text changed since: the places may be elsewhere now.
        if file.path != path || file.edits != version {
            return;
        }
        if self
            .waiting
            .as_ref()
            .is_some_and(|request| request.path() == path)
        {
            self.waiting = None;
        }
        let matches = self.matches_of(&places);
        self.act_on(goal, matches);
    }

    /// The places of an answer, with their line of text and the symbol's columns in
    /// characters.
    fn matches_of(&self, places: &[Place]) -> Vec<SearchMatch> {
        let mut matches = Vec::new();
        let mut read: Option<(PathBuf, Vec<String>)> = None;
        for place in places {
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
        matches
    }

    /// What the places answer `goal`: one goes there, Peek shows it in a box, several (and
    /// references, always) are a list; none is said.
    fn act_on(&mut self, goal: Goal, mut matches: Vec<SearchMatch>) {
        let (name, title) = goal_names(goal);
        if matches.is_empty() {
            self.status = format!("No {name} found");
            return;
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
        self.tick_failures();
        self.tick_install();
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
                if hover.mouse.is_none() {
                    self.waiting = None;
                }
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
