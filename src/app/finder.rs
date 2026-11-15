//! Open a file by its name and the command palette (F41, TODO 2.2.3, 2.3.2): one dialog in two
//! modes, with the same fuzzy matching — every letter typed, in order, anywhere in the name.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

use super::{App, keys::ACTIONS};

/// At most this many files are indexed; a bigger project is found with `Ctrl+Shift+P`.
const MOST_FILES: usize = 50_000;
/// Rows shown at once.
pub const FINDER_ROWS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinderMode {
    Files,
    Commands,
    /// Every problem the language servers found in the project (F26, TODO 3.1.7), and those
    /// a build printed in the bottom terminal (TODO 5.2.5).
    Problems,
    /// The named tasks of `.sozo.toml` (TODO 5.2.5).
    Tasks,
}

/// One row of the dialog, whatever it lists.
#[derive(Clone, Debug)]
pub enum FinderRow {
    File(PathBuf),
    Command(super::Action, String, String),
    /// A symbol: `@` lists the file's, `#` the project's (TODO 3.1.5).
    Symbol(crate::lsp::Symbol),
    Problem(PathBuf, crate::lsp::Diagnostic),
    Task(String, Vec<String>),
}

pub struct Finder {
    pub mode: FinderMode,
    pub query: String,
    pub selected: usize,
    /// The project's files, relative to its folder, read on a thread.
    files: Vec<PathBuf>,
    reader: Option<Receiver<Vec<PathBuf>>>,
    /// The symbols the server sent for `@` or `#`.
    pub symbols: Vec<crate::lsp::Symbol>,
    /// The query the symbols were asked for, to ask again when it changes.
    pub asked: Option<String>,
    /// What a build printed (Problems) or the named tasks (Tasks), read as it opens.
    pub printed: Vec<(PathBuf, crate::lsp::Diagnostic)>,
    pub tasks: Vec<(String, Vec<String>)>,
    /// The file chosen is compared with the active one instead of opened (`Alt+D`).
    pub compare: bool,
}

/// How well `query` matches `text`: none when a letter is missing; higher for letters next to
/// each other, at the start of a word and in the name rather than the folders.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let text_chars: Vec<char> = text.chars().collect();
    let name_start = text
        .rfind(['/', '\\'])
        .map_or(0, |at| text[..=at].chars().count());
    let mut score = 0i64;
    let mut position = 0usize;
    let mut previous: Option<usize> = None;
    for wanted in query.chars().flat_map(char::to_lowercase) {
        if wanted == ' ' {
            continue;
        }
        let found = (position..text_chars.len())
            .find(|&index| text_chars[index].to_lowercase().eq(std::iter::once(wanted)))?;
        score += 1;
        if previous.is_some_and(|previous| previous + 1 == found) {
            score += 5;
        }
        let boundary = found == 0
            || matches!(text_chars[found - 1], '/' | '\\' | '_' | '-' | '.' | ' ')
            || text_chars[found].is_uppercase() && text_chars[found - 1].is_lowercase();
        if boundary {
            score += 8;
        }
        if found >= name_start {
            score += 2;
        }
        previous = Some(found);
        position = found + 1;
    }
    // Shorter texts first among equals.
    Some(score * 1_000 - text_chars.len() as i64)
}

impl Finder {
    pub fn new(mode: FinderMode, root: &Path) -> Self {
        let reader = (mode == FinderMode::Files).then(|| {
            let (sender, receiver) = mpsc::channel();
            let root = root.to_path_buf();
            std::thread::spawn(move || {
                let files: Vec<PathBuf> = ignore::WalkBuilder::new(&root)
                    .hidden(false)
                    .filter_entry(|entry| {
                        entry.file_name() != ".git" && entry.file_name() != "target"
                    })
                    .build()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
                    .filter_map(|entry| {
                        entry.path().strip_prefix(&root).ok().map(Path::to_path_buf)
                    })
                    .take(MOST_FILES)
                    .collect();
                let _ = sender.send(files);
            });
            receiver
        });
        Self {
            mode,
            query: String::new(),
            selected: 0,
            files: Vec::new(),
            reader,
            symbols: Vec::new(),
            asked: None,
            printed: Vec::new(),
            tasks: Vec::new(),
            compare: false,
        }
    }

    /// In the tick: the files the thread read, once they arrive.
    pub fn poll(&mut self) {
        if let Some(files) = self
            .reader
            .as_ref()
            .and_then(|reader| reader.try_recv().ok())
        {
            self.files = files;
            self.reader = None;
        }
    }

    pub fn indexing(&self) -> bool {
        self.reader.is_some()
    }

    /// The files that match, best first.
    pub fn file_results(&self) -> Vec<&PathBuf> {
        let mut found: Vec<(i64, &PathBuf)> = self
            .files
            .iter()
            .filter_map(|path| Some((fuzzy_score(&self.query, &path.to_string_lossy())?, path)))
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        found.into_iter().map(|(_, path)| path).collect()
    }
}

impl App {
    /// What the dialog lists now: files (or, after `@`/`#`, symbols), commands or problems,
    /// the best first.
    pub fn finder_rows(&self) -> Vec<FinderRow> {
        let Some(finder) = &self.finder else {
            return Vec::new();
        };
        match finder.mode {
            FinderMode::Files => match finder.query.chars().next() {
                Some('@' | '#') => {
                    let query = &finder.query[1..];
                    let mut found: Vec<(i64, &crate::lsp::Symbol)> = finder
                        .symbols
                        .iter()
                        .filter_map(|symbol| Some((fuzzy_score(query, &symbol.name)?, symbol)))
                        .collect();
                    if finder.query.starts_with('@') && query.is_empty() {
                        // The file's symbols in their order.
                        return finder
                            .symbols
                            .iter()
                            .cloned()
                            .map(FinderRow::Symbol)
                            .collect();
                    }
                    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
                    found
                        .into_iter()
                        .map(|(_, symbol)| FinderRow::Symbol(symbol.clone()))
                        .collect()
                }
                _ => finder
                    .file_results()
                    .into_iter()
                    .cloned()
                    .map(FinderRow::File)
                    .collect(),
            },
            FinderMode::Commands => self
                .command_results()
                .into_iter()
                .map(|(action, name, key)| FinderRow::Command(action, name, key))
                .collect(),
            FinderMode::Problems => {
                let mut problems: Vec<(PathBuf, crate::lsp::Diagnostic)> = self
                    .lsp
                    .diagnostics
                    .iter()
                    .flat_map(|(path, list)| {
                        list.iter()
                            .map(move |problem| (path.clone(), problem.clone()))
                    })
                    .chain(finder.printed.iter().cloned())
                    .filter(|(path, problem)| {
                        let name = path
                            .strip_prefix(&self.root)
                            .unwrap_or(path)
                            .display()
                            .to_string();
                        fuzzy_score(&finder.query, &format!("{name} {}", problem.message)).is_some()
                    })
                    .collect();
                // Errors first, then by file and row.
                problems
                    .sort_by(|a, b| (!a.1.error, &a.0, a.1.row).cmp(&(!b.1.error, &b.0, b.1.row)));
                problems
                    .into_iter()
                    .map(|(path, problem)| FinderRow::Problem(path, problem))
                    .collect()
            }
            FinderMode::Tasks => finder
                .tasks
                .iter()
                .filter(|(name, _)| fuzzy_score(&finder.query, name).is_some())
                .map(|(name, command)| FinderRow::Task(name.clone(), command.clone()))
                .collect(),
        }
    }

    /// After a key in the dialog: `@` asks the file's symbols once, `#` the project's for the
    /// text after it.
    fn ask_symbols(&mut self) {
        let Some(path) = self.active_file().map(|file| file.path.clone()) else {
            return;
        };
        let Some(finder) = &mut self.finder else {
            return;
        };
        let ask = match finder.query.chars().next() {
            Some('@') if finder.asked.as_deref() != Some("@") => Some(("@".to_owned(), None)),
            Some('#')
                if finder.asked.as_deref() != Some(finder.query.as_str())
                    && finder.query.len() > 1 =>
            {
                Some((finder.query.clone(), Some(finder.query[1..].to_owned())))
            }
            _ => None,
        };
        // A PDF's table of contents stands for its symbols (TODO 5.1.6): `row` is the page.
        // A PDF's table of contents, or a Markdown file's headings (TODO 5.1.6, 5.1.9).
        let pages = self.active_file().is_some_and(|file| file.pdf.is_some());
        let outline = self.active_file().and_then(|file| match &file.pdf {
            Some(document) => Some(document.outline()),
            None => crate::editor::is_markdown(&file.path).then(|| {
                crate::editor::headings(&file.lines)
                    .into_iter()
                    .map(|(level, text, row)| (level - 1, text, row))
                    .collect()
            }),
        });
        if let Some(outline) = outline
            && let Some(finder) = &mut self.finder
            && finder.query.starts_with('@')
            && finder.asked.as_deref() != Some("@")
        {
            finder.asked = Some("@".into());
            finder.symbols = outline
                .into_iter()
                .map(|(depth, title, page)| crate::lsp::Symbol {
                    name: format!("{}{title}", "  ".repeat(depth)),
                    kind: if pages { "page" } else { "heading" },
                    path: path.clone(),
                    row: page,
                    start: 0,
                })
                .collect();
            return;
        }
        let Some(finder) = &mut self.finder else {
            return;
        };
        if let Some((asked, query)) = ask {
            finder.asked = Some(asked);
            finder.symbols.clear();
            if let Err(message) = self.lsp.symbols(&path, query.as_deref()) {
                self.status = message;
            }
        }
    }

    /// The keys of the dialog: typing filters, `↑`/`↓` choose, `Enter` opens or runs, `Esc`.
    pub(super) fn handle_finder_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let count = self.finder_rows().len();
        let Some(finder) = &mut self.finder else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.finder = None,
            KeyCode::Enter => self.choose_finder(),
            KeyCode::Up => {
                finder.selected = finder
                    .selected
                    .checked_sub(1)
                    .unwrap_or(count.saturating_sub(1))
            }
            KeyCode::Down => {
                finder.selected = if count == 0 {
                    0
                } else {
                    (finder.selected + 1) % count
                }
            }
            KeyCode::Backspace => {
                finder.query.pop();
                finder.selected = 0;
                self.ask_symbols();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if finder.query.chars().count() < 200 {
                    finder.query.push(character);
                }
                finder.selected = 0;
                self.ask_symbols();
            }
            _ => {}
        }
    }

    pub fn open_finder(&mut self, mode: FinderMode) {
        self.flush_active_file();
        let mut finder = Finder::new(mode, &self.root);
        match mode {
            // What the last build printed in the bottom terminal (TODO 5.2.5).
            FinderMode::Problems => {
                let lines = self.bottom_terminal.current().lines();
                finder.printed = crate::run::problems_in(&lines, &self.root)
                    .into_iter()
                    .map(|printed| {
                        let problem = crate::lsp::Diagnostic {
                            row: printed.row,
                            start: printed.column,
                            end_row: printed.row,
                            end: printed.column + 1,
                            error: printed.error,
                            message: format!("{} (build)", printed.message),
                        };
                        (printed.path, problem)
                    })
                    .collect();
            }
            FinderMode::Tasks => {
                finder.tasks = crate::run::named_tasks(&self.root);
                if finder.tasks.is_empty() {
                    self.status = format!(
                        "No named tasks: add [tasks.test] or [tasks.lint] to {}",
                        crate::run::TASKS_FILE
                    );
                    return;
                }
            }
            _ => {}
        }
        self.finder = Some(finder);
    }

    /// The commands that match the palette's text: their name and their key.
    pub fn command_results(&self) -> Vec<(super::Action, String, String)> {
        let Some(finder) = &self.finder else {
            return Vec::new();
        };
        let mut found: Vec<(i64, super::Action, String, String)> = ACTIONS
            .iter()
            .filter_map(|action| {
                let name = action.title().to_owned();
                let score = fuzzy_score(&finder.query, &name)?;
                Some((score, *action, name, self.keys.label(*action)))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
        found
            .into_iter()
            .map(|(_, action, name, key)| (action, name, key))
            .collect()
    }

    /// Enter in the dialog: the file opens, or the command runs as if its key was pressed.
    pub fn choose_finder(&mut self) {
        let rows = self.finder_rows();
        let Some(finder) = self.finder.take() else {
            return;
        };
        let Some(row) = rows.into_iter().nth(finder.selected) else {
            return;
        };
        match row {
            FinderRow::File(path) if finder.compare => self.compare_with(self.root.join(path)),
            FinderRow::File(path) => {
                if let Err(error) = self.open_file(self.root.join(path)) {
                    self.status = error.to_string();
                }
            }
            // As if its key was pressed.
            // As if its key was pressed, without the keys of a panel (F2 renames there).
            FinderRow::Command(action, ..) => {
                let key = self.keys.binding(action).event();
                self.from_palette = true;
                self.handle_key(key);
                self.from_palette = false;
            }
            FinderRow::Symbol(symbol) => self.go_to_place(&symbol.path, symbol.row, symbol.start),
            FinderRow::Problem(path, problem) => {
                self.go_to_place(&path, problem.row, problem.start)
            }
            FinderRow::Task(name, command) => self.run_named(&name, &command),
        }
    }

    /// Opens `path` with the cursor at `row` and UTF-16 column `units`.
    fn go_to_place(&mut self, path: &std::path::Path, row: usize, units: usize) {
        if let Err(error) = self.open_file(path.to_path_buf()) {
            self.status = error.to_string();
            return;
        }
        if let Some(active) = self.active_tab
            && self.open_files[active].pdf.is_some()
        {
            return self.open_files[active].show_page(row, &mut self.picker);
        }
        if let Some(file) = self
            .active_tab
            .and_then(|index| self.open_files.get_mut(index))
        {
            let column = file
                .lines
                .get(row)
                .map_or(0, |line| crate::lsp::char_column(line, units));
            file.anchor = None;
            file.move_cursor_to(row, column);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_in_order_find_names_and_the_best_comes_first() {
        assert!(fuzzy_score("mrs", "src/main.rs").is_some());
        assert!(fuzzy_score("xyz", "src/main.rs").is_none());
        assert!(fuzzy_score("srm", "src/main.rs").is_some());
        assert!(fuzzy_score("mn", "src/main.rs").is_some());
        assert!(fuzzy_score("nm", "src/main.rs").is_none(), "order matters");
        let name = fuzzy_score("main", "src/main.rs").unwrap();
        let scattered = fuzzy_score("main", "src/my_animation.rs").unwrap();
        assert!(name > scattered);
        assert!(
            fuzzy_score("MAIN", "src/main.rs").is_some(),
            "case does not matter"
        );
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert!(fuzzy_score("é", "café.txt").is_some());
        assert!(fuzzy_score(&"a".repeat(5_000), "a").is_none());
    }
}
