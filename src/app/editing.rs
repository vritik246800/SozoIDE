//! The editing of TODO 2.1–2.2 that needs the app: redo and comments with a word on the status
//! bar, go to a line, replace in the file and in the workspace (after a question), and the
//! question when a file with edits not saved changes on disk.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;
use crate::editor::{Planned, SearchScope, apply_replace, matcher, plan_replace};

/// Two files side by side: their names, each row's left and right line (none where one side
/// has no line), and the first row on screen.
pub struct Comparison {
    pub title: String,
    pub rows: Vec<(Option<String>, Option<String>)>,
    pub scroll: usize,
    /// Two columns of a list (the crates' licences), not a comparison: no colours.
    pub plain: bool,
}

/// The licence of every crate in the binary, written by `packaging/licenses.sh`.
pub const LICENSES: &str = include_str!("../../assets/licenses.txt");

/// `Settings ~/…  ·  Cache ~/…`, with the home folder as `~`.
pub fn folders_line() -> String {
    let Some(directories) = directories::ProjectDirs::from("", "", "SozoIDE") else {
        return String::new();
    };
    let home = directories::BaseDirs::new().map(|base| base.home_dir().to_path_buf());
    let short = |path: &std::path::Path| match home
        .as_ref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    format!(
        "Settings {}  ·  Cache {}",
        short(directories.config_dir()),
        short(directories.cache_dir())
    )
}

/// The rows of a side-by-side comparison from a unified diff: removed lines on the left, added
/// on the right, a run of each paired, the rest on both sides.
pub fn side_by_side(diff: &[String]) -> Vec<(Option<String>, Option<String>)> {
    let mut rows = Vec::new();
    let (mut left, mut right): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let flush = |rows: &mut Vec<_>, left: &mut Vec<String>, right: &mut Vec<String>| {
        for index in 0..left.len().max(right.len()) {
            rows.push((left.get(index).cloned(), right.get(index).cloned()));
        }
        left.clear();
        right.clear();
    };
    for line in diff {
        match line.chars().next() {
            Some('-') => left.push(line[1..].to_owned()),
            Some('+') => right.push(line[1..].to_owned()),
            Some('@') => {
                flush(&mut rows, &mut left, &mut right);
                rows.push((Some(line.clone()), Some(String::new())));
            }
            _ => {
                flush(&mut rows, &mut left, &mut right);
                let text = line.get(1..).unwrap_or("").to_owned();
                rows.push((Some(text.clone()), Some(text)));
            }
        }
    }
    flush(&mut rows, &mut left, &mut right);
    rows
}

impl App {
    /// `L` in About: the licences of the crates, one per row (TODO 6.3.1).
    pub(super) fn open_licenses(&mut self) {
        let rows = LICENSES
            .lines()
            .filter_map(|line| {
                let mut words = line.splitn(3, ' ');
                let (name, version, licence) = (words.next()?, words.next()?, words.next()?);
                Some((Some(format!("{name} {version}")), Some(licence.to_owned())))
            })
            .collect();
        self.comparison = Some(Comparison {
            title: " Licences of the crates in SozoIDE ".into(),
            rows,
            scroll: 0,
            plain: true,
        });
    }

    /// `Alt+D`: the file to compare the active one with, chosen like any file.
    pub(super) fn ask_comparison(&mut self) {
        if self.active_file().is_some_and(|file| file.is_text()) {
            self.open_finder(super::FinderMode::Files);
            if let Some(finder) = &mut self.finder {
                finder.compare = true;
            }
        } else {
            self.status = "Open a text file to compare it with another".into();
        }
    }

    /// The active file and `other` side by side.
    pub(super) fn compare_with(&mut self, other: PathBuf) {
        let Some(file) = self.active_file() else {
            return;
        };
        let mine = file.text();
        let theirs = match std::fs::read(&other) {
            Ok(bytes) if bytes.len() <= 16 * 1024 * 1024 => {
                String::from_utf8_lossy(&bytes).into_owned()
            }
            Ok(_) => {
                self.status = "That file is too large to compare".into();
                return;
            }
            Err(error) => {
                self.status = format!("Could not read {}: {error}", other.display());
                return;
            }
        };
        let name = |path: &std::path::Path| {
            path.strip_prefix(&self.root)
                .unwrap_or(path)
                .display()
                .to_string()
        };
        let rows = side_by_side(&crate::git::compare_texts(&mine, &theirs));
        if rows.is_empty() {
            self.status = format!("{} and {} are the same", file.name, name(&other));
            return;
        }
        self.comparison = Some(Comparison {
            title: format!(" {} ↔ {} ", name(&file.path), name(&other)),
            rows,
            scroll: 0,
            plain: false,
        });
    }

    pub(super) fn handle_comparison_key(&mut self, key: KeyEvent) {
        let Some(comparison) = &mut self.comparison else {
            return;
        };
        let last = comparison.rows.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.comparison = None,
            KeyCode::Up => comparison.scroll = comparison.scroll.saturating_sub(1),
            KeyCode::Down => comparison.scroll = (comparison.scroll + 1).min(last),
            KeyCode::PageUp => comparison.scroll = comparison.scroll.saturating_sub(20),
            KeyCode::PageDown => comparison.scroll = (comparison.scroll + 20).min(last),
            KeyCode::Home => comparison.scroll = 0,
            KeyCode::End => comparison.scroll = last,
            _ => {}
        }
    }

    /// `Alt+S`: the active file also in the other pane, the two kept the same as either is
    /// edited (TODO 5.2.6).
    pub(super) fn open_to_side(&mut self) {
        let Some(active) = self.active_tab else {
            return;
        };
        if !self.open_files[active].is_text() {
            return;
        }
        let path = self.open_files[active].path.clone();
        let pane = 1 - self.open_files[active].pane;
        if let Some(twin) = self
            .open_files
            .iter()
            .position(|file| file.path == path && file.pane == pane)
        {
            return self.select_tab(twin);
        }
        let mut twin = match crate::editor::OpenFile::load(path, &mut self.picker) {
            Ok(file) => file,
            Err(error) => {
                self.status = format!("{error:#}");
                return;
            }
        };
        twin.take_text_of(&self.open_files[active].lines);
        twin.pane = pane;
        self.open_files.push(twin);
        let index = self.open_files.len() - 1;
        if self.split.is_none() {
            self.split = Some(super::Split {
                direction: super::SplitDirection::Columns,
                other: active,
            });
        }
        self.select_tab(index);
    }

    /// In the tick: the copies of the active file in the other pane take its text.
    pub(super) fn sync_twins(&mut self) {
        let Some(active) = self.active_tab else {
            return;
        };
        let Some(file) = self.open_files.get(active) else {
            return;
        };
        let (path, pane) = (file.path.clone(), file.pane);
        let twins: Vec<usize> = (0..self.open_files.len())
            .filter(|index| {
                *index != active
                    && self.open_files[*index].path == path
                    && self.open_files[*index].pane != pane
            })
            .collect();
        if twins.is_empty() {
            return;
        }
        let lines = self.open_files[active].lines.clone();
        for twin in twins {
            self.open_files[twin].take_text_of(&lines);
        }
    }
}

/// A replace in the workspace waiting for `Enter`: what each file gets, and with what.
pub struct ReplacePlan {
    pub files: Vec<Planned>,
    pub query: String,
    pub with: String,
    pub options: crate::editor::SearchOptions,
}

impl App {
    pub(super) fn redo(&mut self, active: usize) {
        if !self.open_files[active].redo() {
            self.status = "Nothing to redo".into();
        }
    }

    pub(super) fn toggle_comment(&mut self, active: usize) {
        let file = &mut self.open_files[active];
        if !file.toggle_comment() {
            self.status = format!("No comment SozoIDE knows for {}", file.language);
        }
    }

    /// `Ctrl+L`: the box that takes `line` or `line:column`.
    pub(super) fn open_go_to_line(&mut self) {
        if self.active_tab.is_some() {
            self.go_to_line = Some(String::new());
        }
    }

    /// The keys of the Go to line box; digits and one `:` only.
    pub(super) fn handle_go_to_line_key(&mut self, key: KeyEvent) {
        let Some(text) = &mut self.go_to_line else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.go_to_line = None,
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(character @ ('0'..='9' | ':')) if text.len() < 16 => {
                if character != ':' || !text.contains(':') {
                    text.push(character);
                }
            }
            KeyCode::Enter => {
                let text = self.go_to_line.take().unwrap_or_default();
                self.go_to(&text);
            }
            _ => {}
        }
    }

    /// Moves the cursor to `line[:column]`, both from 1; past the end, the last.
    pub(super) fn go_to(&mut self, text: &str) {
        let Some(active) = self.active_tab else {
            return;
        };
        let mut parts = text.split(':');
        let line = parts.next().and_then(|line| line.parse::<usize>().ok());
        let column = parts.next().and_then(|column| column.parse::<usize>().ok());
        let Some(line) = line else {
            return;
        };
        // In a PDF the number is a page (TODO 5.1.6).
        if self.open_files[active].pdf.is_some() {
            return self.open_files[active].show_page(line.saturating_sub(1), &mut self.picker);
        }
        let file = &mut self.open_files[active];
        file.anchor = None;
        file.move_cursor_to(
            line.saturating_sub(1),
            column.unwrap_or(1).saturating_sub(1),
        );
        file.follow_cursor = true;
    }

    /// `Ctrl+H`: the search of the file with its replace row, or the row of an open search.
    pub(crate) fn open_replace(&mut self) {
        if self.search.is_none() {
            self.open_search(SearchScope::CurrentFile);
        }
        if let Some(search) = &mut self.search {
            search.replace = match search.replace.take() {
                Some(_) => None,
                None => Some(String::new()),
            };
            search.in_replace = search.replace.is_some();
        }
    }

    /// The search bar's own keys: `Alt+C`/`Alt+W`/`Alt+R` for its options, `Tab` between the
    /// query and the replace row, typing in the replace row, `Enter` to replace one and
    /// `Ctrl+Enter` all. True when the key was one of these.
    pub(super) fn handle_replace_key(&mut self, key: KeyEvent) -> bool {
        let (ctrl, alt) = (
            key.modifiers.contains(KeyModifiers::CONTROL),
            key.modifiers.contains(KeyModifiers::ALT),
        );
        let active = self.active_tab.and_then(|index| self.open_files.get(index));
        let Some(search) = &mut self.search else {
            return false;
        };
        if search.title.is_some() {
            return false;
        }
        if alt {
            let options = &mut search.options;
            match key.code {
                KeyCode::Char('c' | 'C') => options.case = !options.case,
                KeyCode::Char('w' | 'W') => options.word = !options.word,
                KeyCode::Char('r' | 'R') => options.regex = !options.regex,
                _ => return false,
            }
            search.refresh(&self.root, active);
            return true;
        }
        let Some(with) = &mut search.replace else {
            return false;
        };
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => search.in_replace = !search.in_replace,
            KeyCode::Enter if ctrl => self.replace_all(),
            KeyCode::Enter if search.in_replace => self.replace_one(),
            KeyCode::Backspace if search.in_replace => {
                with.pop();
            }
            KeyCode::Char(character) if search.in_replace && !ctrl => with.push(character),
            _ => return false,
        }
        true
    }

    /// Replaces the match chosen in the open file and goes to the next one.
    fn replace_one(&mut self) {
        let Some(active) = self.active_tab else {
            return;
        };
        let Some(search) = &self.search else {
            return;
        };
        if search.scope != SearchScope::CurrentFile {
            return self.replace_all();
        }
        let (Some(found), Some(with)) = (search.selected_match().cloned(), search.replace.clone())
        else {
            return;
        };
        let Ok(query) = matcher(&search.query, search.options) else {
            return;
        };
        let regex = search.options.regex;
        let line = &self.open_files[active].lines[found.line];
        let start = line
            .char_indices()
            .nth(found.column)
            .map_or(line.len(), |(at, _)| at);
        let end = line
            .char_indices()
            .nth(found.column + found.length)
            .map_or(line.len(), |(at, _)| at);
        let replaced = crate::editor::replace_first(&line[start..end], &query, &with, regex);
        self.open_files[active].replace_range(found.line, found.column, found.length, &replaced);
        let selected = search.selected;
        let active_file = self.open_files.get(active);
        if let Some(search) = &mut self.search {
            search.refresh(&self.root, active_file);
            search.selected = selected.min(search.matches.len().saturating_sub(1));
        }
        self.preview_match();
    }

    /// `Ctrl+Enter`: every match of the file at once (one step of undo), or the question for
    /// the whole workspace.
    fn replace_all(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        let Some(with) = search.replace.clone() else {
            return;
        };
        let Ok(query) = matcher(&search.query, search.options) else {
            return;
        };
        let regex = search.options.regex;
        if search.scope == SearchScope::Workspace {
            if search.searching {
                self.status = "Wait for the search to finish".into();
                return;
            }
            let files = plan_replace(&self.root, &search.matches);
            if files.is_empty() {
                self.status = "Nothing to replace".into();
                return;
            }
            self.replace_plan = Some(ReplacePlan {
                files,
                query: search.query.clone(),
                with,
                options: search.options,
            });
            return;
        }
        let Some(active) = self.active_tab else {
            return;
        };
        let count = self.open_files[active]
            .replace_lines(|line| crate::editor::replace_line(line, &query, &with, regex));
        self.status = format!("Replaced {count}");
        let active_file = self.open_files.get(active);
        if let Some(search) = &mut self.search {
            search.refresh(&self.root, active_file);
        }
    }

    /// The question of a replace in the workspace: `Enter` writes, `Esc` does not.
    pub(super) fn handle_replace_plan_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.replace_plan = None,
            KeyCode::Enter => {
                let Some(plan) = self.replace_plan.take() else {
                    return;
                };
                let Ok(query) = matcher(&plan.query, plan.options) else {
                    return;
                };
                // Open tabs are saved first, so that nothing typed is written over.
                self.flush_all_files();
                let (written, skipped) =
                    apply_replace(&plan.files, &query, &plan.with, plan.options.regex);
                for file in &mut self.open_files {
                    if written.contains(&file.path) {
                        let _ = file.reload();
                    }
                }
                self.status = match skipped.first() {
                    None => format!("Replaced in {} files", written.len()),
                    Some(first) => format!(
                        "Replaced in {} files; {} left as they were: {first}",
                        written.len(),
                        skipped.len()
                    ),
                };
                self.close_search();
            }
            _ => {}
        }
    }

    /// A file with edits not saved changed on disk: the question waits (TODO 2.2.6).
    pub(super) fn ask_about_disk(&mut self, path: PathBuf) {
        self.disk_prompt = Some(path);
    }

    /// `K` keeps mine, `R` reads the disk's (Ctrl+Z brings mine back), `C` compares.
    pub(super) fn handle_disk_key(&mut self, key: KeyEvent) {
        let Some(path) = self.disk_prompt.clone() else {
            return;
        };
        let Some(index) = self.open_files.iter().position(|file| file.path == path) else {
            self.disk_prompt = None;
            return;
        };
        match key.code {
            KeyCode::Char('k' | 'K') | KeyCode::Esc => {
                self.disk_prompt = None;
                self.disk_compare = None;
                self.status = format!("{}: your edits are kept", self.open_files[index].name);
            }
            KeyCode::Char('r' | 'R') => {
                self.disk_prompt = None;
                self.disk_compare = None;
                self.status = match self.open_files[index].reload() {
                    Ok(()) => format!("Reloaded {}: Ctrl+Z brings your edits back", path.display()),
                    Err(error) => error.to_string(),
                };
            }
            KeyCode::Char('c' | 'C') => {
                let disk = std::fs::read_to_string(&path).unwrap_or_default();
                let mine = self.open_files[index].text();
                self.disk_compare = Some(crate::git::compare_texts(&disk, &mine));
            }
            _ => {}
        }
    }
}
