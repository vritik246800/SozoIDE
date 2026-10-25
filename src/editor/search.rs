use std::{fs, path::PathBuf};

use ignore::WalkBuilder;

use super::{OpenFile, is_image_path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
    CurrentFile,
    Workspace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub text: String,
}

pub struct SearchState {
    pub scope: SearchScope,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub selected: usize,
    pub scroll: usize,
}

impl SearchState {
    pub fn new(scope: SearchScope) -> Self {
        Self {
            scope,
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
            scroll: 0,
        }
    }

    pub fn push(&mut self, character: char, root: &std::path::Path, active: Option<&OpenFile>) {
        self.query.push(character);
        self.refresh(root, active);
    }

    pub fn backspace(&mut self, root: &std::path::Path, active: Option<&OpenFile>) {
        self.query.pop();
        self.refresh(root, active);
    }

    pub fn refresh(&mut self, root: &std::path::Path, active: Option<&OpenFile>) {
        self.matches.clear();
        self.selected = 0;
        self.scroll = 0;
        let query = self.query.to_lowercase();
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
                for entry in WalkBuilder::new(root)
                    .hidden(false)
                    .filter_entry(|entry| {
                        let name = entry.file_name().to_string_lossy();
                        name != ".git" && name != "target"
                    })
                    .build()
                    .filter_map(std::result::Result::ok)
                {
                    if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                        continue;
                    }
                    let Ok(bytes) = fs::read(entry.path()) else {
                        continue;
                    };
                    if bytes.contains(&0) || is_image_path(entry.path()) {
                        continue;
                    }
                    let text = String::from_utf8_lossy(&bytes);
                    let lines = text.lines().collect::<Vec<_>>();
                    search_lines(entry.path(), &lines, &query, &mut self.matches);
                }
            }
        }
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
}

fn search_lines<S: AsRef<str>>(
    path: &std::path::Path,
    lines: &[S],
    query: &str,
    matches: &mut Vec<SearchMatch>,
) {
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if let Some(column) = line.to_lowercase().find(query) {
            matches.push(SearchMatch {
                path: path.to_path_buf(),
                line: index,
                column,
                text: line.to_owned(),
            });
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
        assert_eq!(search.matches.len(), 2);
    }
}
