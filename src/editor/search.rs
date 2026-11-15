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
use regex::{Regex, RegexBuilder};

use super::OpenFile;

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

/// How the query matches (TODO 2.1.2): `Aa` keeps the case, the word only whole, `.*` as a
/// regular expression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

/// The query as a regular expression, or why it is not one (said under the search bar).
pub fn matcher(query: &str, options: SearchOptions) -> Result<Regex, String> {
    let pattern = if options.regex {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    let pattern = if options.word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!options.case)
        // A pattern someone pastes cannot take the memory or the time of the keys.
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|error| match error {
            regex::Error::Syntax(text) => text
                .lines()
                .last()
                .unwrap_or("not a regular expression")
                .trim()
                .to_owned(),
            regex::Error::CompiledTooBig(_) => "the expression is too large".to_owned(),
            _ => "not a regular expression".to_owned(),
        })
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
    /// A list given whole, with its title (references, definitions — F31): nothing is typed.
    pub title: Option<String>,
    pub options: SearchOptions,
    /// The text that replaces each match, when the replace row is open (TODO 2.1.3).
    pub replace: Option<String>,
    /// The replace row has the keys instead of the query.
    pub in_replace: bool,
    /// Why the query matches nothing: a regular expression that does not compile.
    pub error: Option<String>,
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
            title: None,
            options: SearchOptions::default(),
            replace: None,
            in_replace: false,
            error: None,
        }
    }

    /// The workspace results `matches` under `title`, for `query` (the symbol).
    pub fn fixed(title: String, query: String, matches: Vec<SearchMatch>) -> Self {
        let mut search = Self::new(SearchScope::Workspace);
        search.query = query;
        search.matches = matches;
        search.title = Some(title);
        search
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
        self.error = None;
        if self.query.is_empty() {
            return;
        }
        let query = match matcher(&self.query, self.options) {
            Ok(query) => query,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        match self.scope {
            SearchScope::CurrentFile => {
                // A PDF is searched on every page: a match's line counts across them, and
                // `PdfDocument::lines` says which page it is on (TODO 5.1.6).
                match active.and_then(|file| Some((file, file.pdf.as_ref()?))) {
                    Some((file, document)) => {
                        let lines: Vec<String> = document
                            .lines()
                            .into_iter()
                            .map(|(page, _, text)| format!("p. {}  {text}", page + 1))
                            .collect();
                        search_lines(&file.path, &lines, &query, &mut self.matches);
                    }
                    None => {
                        if let Some(file) = active {
                            search_lines(&file.path, &file.lines, &query, &mut self.matches);
                        }
                    }
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

/// Every file under `root` that is not ignored, binary or an image, one `Found::File` per file
/// with results; stops when `stop` is set, `send` fails or there are `MAX_MATCHES`.
fn search_workspace(root: &Path, query: &Regex, stop: &AtomicBool, send: impl Fn(Found) -> bool) {
    let mut total = 0;
    // By name: `readdir` has an order of its own on each file system (E57).
    for entry in WalkBuilder::new(root)
        .hidden(false)
        .sort_by_file_name(Ord::cmp)
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
        if !entry.file_type().is_some_and(|kind| kind.is_file())
            || crate::media::kind(entry.path()).is_some()
        {
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

/// Every occurrence of `query` in `lines`, without overlaps or empty matches, with its column
/// and length in characters.
fn search_lines<S: AsRef<str>>(
    path: &Path,
    lines: &[S],
    query: &Regex,
    matches: &mut Vec<SearchMatch>,
) {
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        for found in query.find_iter(line).filter(|found| !found.is_empty()) {
            let column = line[..found.start()].chars().count();
            matches.push(SearchMatch {
                path: path.to_path_buf(),
                line: index,
                column,
                length: found.as_str().chars().count(),
                text: line.to_owned(),
            });
        }
    }
}

/// `line` with every match of `query` replaced (`$1` names a group when `regex`), and how many.
pub fn replace_line(line: &str, query: &Regex, with: &str, regex: bool) -> (String, usize) {
    let mut count = 0;
    let replaced = query.replace_all(line, |captures: &regex::Captures| {
        count += 1;
        let mut out = String::new();
        if regex {
            captures.expand(with, &mut out);
        } else {
            out.push_str(with);
        }
        out
    });
    (replaced.into_owned(), count)
}

/// `text` (one match) with its first match replaced.
pub fn replace_first(text: &str, query: &Regex, with: &str, regex: bool) -> String {
    query
        .replace(text, |captures: &regex::Captures| {
            let mut out = String::new();
            if regex {
                captures.expand(with, &mut out);
            } else {
                out.push_str(with);
            }
            out
        })
        .into_owned()
}

/// A file of the workspace that a replace will write, as it was when the list was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub path: PathBuf,
    pub count: usize,
    stamp: Option<(std::time::SystemTime, u64)>,
}

/// What a replace in the whole workspace changes, file by file, before anything is written.
pub fn plan_replace(root: &Path, matches: &[SearchMatch]) -> Vec<Planned> {
    let mut planned: Vec<Planned> = Vec::new();
    for found in matches {
        match planned.last_mut() {
            Some(last) if last.path == found.path => last.count += 1,
            _ => planned.push(Planned {
                path: found.path.clone(),
                count: 1,
                stamp: stamp(&found.path),
            }),
        }
    }
    let inside = root.canonicalize().ok();
    planned.retain(|plan| {
        // Never a link out of the project, never a file that is not text.
        plan.path
            .canonicalize()
            .ok()
            .zip(inside.as_ref())
            .is_some_and(|(real, root)| real.starts_with(root))
    });
    planned
}

fn stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let data = fs::symlink_metadata(path).ok()?;
    (!data.is_symlink() && data.is_file()).then_some(())?;
    Some((data.modified().ok()?, data.len()))
}

/// Writes the replace planned in `plan`: each file only if it is still as it was when listed,
/// still text and writable. The files written and, for the others, why not.
pub fn apply_replace(
    plan: &[Planned],
    query: &Regex,
    with: &str,
    regex: bool,
) -> (Vec<PathBuf>, Vec<String>) {
    let (mut written, mut skipped) = (Vec::new(), Vec::new());
    for planned in plan {
        let name = planned.path.display();
        if planned.stamp.is_none() || stamp(&planned.path) != planned.stamp {
            skipped.push(format!("{name} changed since the list was made"));
            continue;
        }
        let Ok(bytes) = fs::read(&planned.path) else {
            skipped.push(format!("{name} could not be read"));
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            skipped.push(format!("{name} is not UTF-8 text"));
            continue;
        };
        if text.contains('\0') {
            skipped.push(format!("{name} is a binary file"));
            continue;
        }
        let mut out = String::with_capacity(text.len());
        for piece in text.split_inclusive('\n') {
            let (line, end) = match piece.strip_suffix("\r\n") {
                Some(line) => (line, "\r\n"),
                None => match piece.strip_suffix('\n') {
                    Some(line) => (line, "\n"),
                    None => (piece, ""),
                },
            };
            out.push_str(&replace_line(line, query, with, regex).0);
            out.push_str(end);
        }
        match fs::write(&planned.path, out) {
            Ok(()) => written.push(planned.path.clone()),
            Err(error) => skipped.push(format!("{name}: {error}")),
        }
    }
    (written, skipped)
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

    #[test]
    fn workspace_results_come_in_order_of_name_on_every_file_system() {
        let directory = tempfile::tempdir().unwrap();
        let names = (0..40).map(|index| format!("f{index:02}.txt")).rev();
        for name in names {
            fs::write(directory.path().join(name), "needle\n").unwrap();
        }
        let mut search = SearchState::new(SearchScope::Workspace);
        search.push_str("needle", directory.path(), None);
        wait(&mut search);
        let found = search
            .matches
            .iter()
            .map(|found| found.path.clone())
            .collect::<Vec<_>>();
        let mut sorted = found.clone();
        sorted.sort();
        assert_eq!(found.len(), 40);
        assert_eq!(found, sorted);
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
    fn options_keep_case_whole_words_and_regular_expressions_and_bad_ones_say_why() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("a.rs"),
            "Foo foo food fo+o\nlet x2 = y22;\n",
        )
        .unwrap();
        let mut picker = Picker::halfblocks();
        let file = OpenFile::load(directory.path().join("a.rs"), &mut picker).unwrap();
        let count = |query: &str, options: SearchOptions| {
            let mut search = SearchState::new(SearchScope::CurrentFile);
            search.options = options;
            search.push_str(query, directory.path(), Some(&file));
            (search.matches.len(), search.error.clone())
        };
        let plain = SearchOptions::default();
        assert_eq!(count("foo", plain), (3, None));
        assert_eq!(
            count(
                "foo",
                SearchOptions {
                    case: true,
                    ..plain
                }
            ),
            (2, None)
        );
        assert_eq!(
            count(
                "foo",
                SearchOptions {
                    word: true,
                    ..plain
                }
            ),
            (2, None)
        );
        // Without `.*` the characters of an expression are text.
        assert_eq!(count("fo+o", plain), (1, None));
        assert_eq!(
            count(
                r"y\d+",
                SearchOptions {
                    regex: true,
                    ..plain
                }
            ),
            (1, None)
        );
        let (none, error) = count(
            "(",
            SearchOptions {
                regex: true,
                ..plain
            },
        );
        assert!(none == 0 && error.is_some());
        // A pattern that would match empty text everywhere finds nothing, without looping.
        assert_eq!(
            count(
                "x*",
                SearchOptions {
                    regex: true,
                    ..plain
                }
            )
            .0,
            1
        );
        let huge = "a{1000}{1000}";
        assert!(
            count(
                huge,
                SearchOptions {
                    regex: true,
                    ..plain
                }
            )
            .1
            .is_some()
        );
    }

    #[test]
    fn a_replace_in_the_workspace_writes_only_what_it_listed() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("a.txt"), "old one\r\nold two\n").unwrap();
        fs::write(root.join("b.txt"), "old\n").unwrap();
        fs::write(root.join("c.txt"), "old\n").unwrap();
        let mut search = SearchState::new(SearchScope::Workspace);
        search.push_str("old", root, None);
        wait(&mut search);
        let query = matcher("old", SearchOptions::default()).unwrap();
        let plan = plan_replace(root, &search.matches);
        assert_eq!(plan.iter().map(|plan| plan.count).sum::<usize>(), 4);
        // b.txt changes between the list and Enter: it is left as it is.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(root.join("b.txt"), "old and more\n").unwrap();
        let (written, skipped) = apply_replace(&plan, &query, "new", false);
        assert_eq!(written.len(), 2);
        assert!(skipped[0].contains("changed since"), "{skipped:?}");
        // Line ends stay as they were, one by one.
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "new one\r\nnew two\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "old and more\n"
        );
        // Groups in a regular expression.
        let swap = matcher(
            r"(\w+) (\w+)",
            SearchOptions {
                regex: true,
                ..SearchOptions::default()
            },
        )
        .unwrap();
        assert_eq!(replace_line("ab cd", &swap, "$2 $1", true).0, "cd ab");
        assert_eq!(replace_line("ab cd", &swap, "$2 $1", false).0, "$2 $1");
    }

    #[cfg(unix)]
    #[test]
    fn a_replace_leaves_read_only_and_binary_files_as_they_are_and_says_why() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("locked.txt"), "old\n").unwrap();
        fs::set_permissions(root.join("locked.txt"), fs::Permissions::from_mode(0o444)).unwrap();
        fs::write(root.join("latin.txt"), b"old \xE9\n").unwrap();
        let found = |name: &str| SearchMatch {
            path: root.join(name),
            line: 0,
            column: 0,
            length: 3,
            text: "old".into(),
        };
        let plan = plan_replace(root, &[found("locked.txt"), found("latin.txt")]);
        let query = matcher("old", SearchOptions::default()).unwrap();
        let (written, skipped) = apply_replace(&plan, &query, "new", false);
        assert!(written.is_empty(), "{written:?}");
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(skipped.iter().any(|why| why.contains("not UTF-8")));
        assert_eq!(fs::read(root.join("latin.txt")).unwrap(), b"old \xE9\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_replace_never_writes_through_a_link_out_of_the_project() {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("x.txt"), "old\n").unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path().join("x.txt"), directory.path().join("x.txt"))
            .unwrap();
        let found = SearchMatch {
            path: directory.path().join("x.txt"),
            line: 0,
            column: 0,
            length: 3,
            text: "old".into(),
        };
        assert!(plan_replace(directory.path(), &[found]).is_empty());
        assert_eq!(
            fs::read_to_string(outside.path().join("x.txt")).unwrap(),
            "old\n"
        );
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
