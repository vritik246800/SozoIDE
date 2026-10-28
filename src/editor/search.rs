use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
};

use ignore::WalkBuilder;

use super::{OpenFile, is_image_path};

/// More results than anyone reads; the workspace search stops there (F05).
pub const MAX_MATCHES: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
    CurrentFile,
    Workspace,
}

/// One occurrence: its line, its column and length in characters, and the whole line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub length: usize,
    pub text: String,
}

enum Found {
    File(Vec<SearchMatch>),
    Done,
}

pub struct SearchState {
    pub scope: SearchScope,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub selected: usize,
    pub scroll: usize,
    /// The workspace search runs on a thread and its results arrive file by file.
    pub searching: bool,
    receiver: Option<Receiver<Found>>,
    cancelled: Arc<AtomicBool>,
}

impl SearchState {
    pub fn new(scope: SearchScope) -> Self {
        Self {
            scope,
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
            scroll: 0,
            searching: false,
            receiver: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn push(&mut self, character: char, root: &Path, active: Option<&OpenFile>) {
        self.query.push(character);
        self.refresh(root, active);
    }

    pub fn push_str(&mut self, text: &str, root: &Path, active: Option<&OpenFile>) {
        self.query
            .extend(text.chars().filter(|character| !character.is_control()));
        self.refresh(root, active);
    }

    pub fn backspace(&mut self, root: &Path, active: Option<&OpenFile>) {
        self.query.pop();
        self.refresh(root, active);
    }

    /// Searches again for the query: the current file at once, the workspace on a thread that
    /// a newer search stops.
    pub fn refresh(&mut self, root: &Path, active: Option<&OpenFile>) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.receiver = None;
        self.searching = false;
        self.matches.clear();
        self.selected = 0;
        self.scroll = 0;
        let query = lowercase(&self.query);
        if query.is_empty() {
            return;
        }
        match self.scope {
            SearchScope::CurrentFile => {
                if let Some(file) = active {
                    search_lines(&file.path, &file.lines, &query, &mut self.matches);
                }
            }
            SearchScope::Workspace => {
                let cancelled = Arc::new(AtomicBool::new(false));
                let (sender, receiver) = mpsc::channel();
                let root = root.to_path_buf();
                let stop = Arc::clone(&cancelled);
                thread::spawn(move || {
                    search_workspace(&root, &query, &stop, |found| sender.send(found).is_ok())
                });
                self.cancelled = cancelled;
                self.receiver = Some(receiver);
                self.searching = true;
            }
        }
    }

    /// Takes the results the workspace thread found since the last call; true when some came.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        let mut changed = false;
        while let Ok(found) = receiver.try_recv() {
            changed = true;
            match found {
                Found::File(matches) => self.matches.extend(matches),
                Found::Done => {
                    self.searching = false;
                    self.receiver = None;
                    break;
                }
            }
        }
        changed
    }

    pub fn select_next(&mut self) {
        if !self.matches.is_empty() {
            self.selected = (self.selected + 1) % self.matches.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.matches.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.matches.len() - 1);
        }
    }

    pub fn selected_match(&self) -> Option<&SearchMatch> {
        self.matches.get(self.selected)
    }

    /// How many files have results.
    pub fn files(&self) -> usize {
        let mut files = self
            .matches
            .iter()
            .map(|found| &found.path)
            .collect::<Vec<_>>();
        files.dedup();
        files.len()
    }
}

impl Drop for SearchState {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn lowercase(text: &str) -> Vec<char> {
    text.chars().map(lower).collect()
}

fn lower(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

/// Every file under `root` that is not ignored, binary or an image, one `Found::File` per file
/// with results; stops when `stop` is set, `send` fails or there are `MAX_MATCHES`.
fn search_workspace(root: &Path, query: &[char], stop: &AtomicBool, send: impl Fn(Found) -> bool) {
    let mut total = 0;
    for entry in WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            name != ".git" && name != "target"
        })
        .build()
        .filter_map(std::result::Result::ok)
    {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) || is_image_path(entry.path()) {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines = text.lines().collect::<Vec<_>>();
        let mut matches = Vec::new();
        search_lines(entry.path(), &lines, query, &mut matches);
        if matches.is_empty() {
            continue;
        }
        matches.truncate(MAX_MATCHES - total);
        total += matches.len();
        if !send(Found::File(matches)) {
            return;
        }
        if total >= MAX_MATCHES {
            break;
        }
    }
    send(Found::Done);
}

/// Every occurrence of `query` (lowercase) in `lines`, ignoring case, without overlaps.
fn search_lines<S: AsRef<str>>(
    path: &Path,
    lines: &[S],
    query: &[char],
    matches: &mut Vec<SearchMatch>,
) {
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        let characters = line.chars().map(lower).collect::<Vec<_>>();
        let mut column = 0;
        while column + query.len() <= characters.len() {
            if characters[column..column + query.len()] == *query {
                matches.push(SearchMatch {
                    path: path.to_path_buf(),
                    line: index,
                    column,
                    length: query.len(),
                    text: line.to_owned(),
                });
                column += query.len();
            } else {
                column += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_image::picker::Picker;

    #[test]
    fn searches_current_file_and_workspace() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("one.rs"), "alpha\nNeedle here\n").unwrap();
        fs::write(directory.path().join("two.txt"), "another needle\n").unwrap();
        fs::create_dir(directory.path().join("target")).unwrap();
        fs::write(directory.path().join("target/ignored.txt"), "needle").unwrap();
        let mut picker = Picker::halfblocks();
        let file = OpenFile::load(directory.path().join("one.rs"), &mut picker).unwrap();
        let mut search = SearchState::new(SearchScope::CurrentFile);
        for character in "needle".chars() {
            search.push(character, directory.path(), Some(&file));
        }
        assert_eq!(search.matches.len(), 1);
        assert_eq!(search.matches[0].line, 1);
        search.scope = SearchScope::Workspace;
        search.refresh(directory.path(), Some(&file));
        wait(&mut search);
        assert_eq!(search.matches.len(), 2);
    }

    fn wait(search: &mut SearchState) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while search.searching && std::time::Instant::now() < deadline {
            search.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!search.searching);
    }

    #[test]
    fn every_occurrence_is_found_with_its_column_and_length_in_characters() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("a.rs"),
            "let café = Café::new(); // CAFÉ\nnone here\ncafécafé\n",
        )
        .unwrap();
        fs::write(directory.path().join("b.txt"), "un café\n").unwrap();
        let mut picker = Picker::halfblocks();
        let file = OpenFile::load(directory.path().join("a.rs"), &mut picker).unwrap();
        let mut search = SearchState::new(SearchScope::CurrentFile);
        search.push_str("Café", directory.path(), Some(&file));
        let found = search
            .matches
            .iter()
            .map(|found| (found.line, found.column, found.length))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [(0, 4, 4), (0, 11, 4), (0, 27, 4), (2, 0, 4), (2, 4, 4)]
        );
        assert_eq!(search.files(), 1);
        search.select_previous();
        assert_eq!(search.selected, 4);
        search.select_next();
        assert_eq!(search.selected, 0);

        // The workspace counts every file; a new query drops the old results at once.
        search.scope = SearchScope::Workspace;
        search.refresh(directory.path(), Some(&file));
        assert!(search.searching);
        wait(&mut search);
        assert_eq!((search.matches.len(), search.files()), (6, 2));
        search.backspace(directory.path(), Some(&file));
        assert!(search.matches.is_empty() && search.searching);
        wait(&mut search);
        assert_eq!(search.matches.len(), 6);
        search.push_str("zzz", directory.path(), Some(&file));
        wait(&mut search);
        assert!(search.matches.is_empty());
    }
}
