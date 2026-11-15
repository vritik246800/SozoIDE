//! More than one entry at a time in the Explorer, and what a file explorer does with paths
//! (F01, TODO 5.2.1): marks with `Shift` and `Ctrl`, a filter of the whole project (`/`),
//! copying a path, showing a file in the system's file manager, duplicating, and moving by
//! dragging onto a folder after a question.

use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use super::{App, EntryAction, FileEntry, NamePrompt, explorer::copy_entry, explorer::move_entry};

/// At most this many names come out of a filter: the rest is said, not listed.
const MOST_FOUND: usize = 2_000;

impl App {
    /// Shift+click or Shift+arrows: the entries from the one selected to `index`, marked.
    pub(super) fn mark_range(&mut self, index: usize) {
        let (from, to) = if index < self.selected_entry {
            (index, self.selected_entry)
        } else {
            (self.selected_entry, index)
        };
        for entry in self.entries.iter().take(to + 1).skip(from) {
            self.marked.insert(entry.path.clone());
        }
        self.selected_entry = index.min(self.entries.len().saturating_sub(1));
    }

    /// Ctrl+click: one more entry, or one less.
    pub(super) fn toggle_mark(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        if !self.marked.remove(&entry.path) {
            self.marked.insert(entry.path.clone());
        }
        self.selected_entry = index;
    }

    /// What an operation acts on: the marked entries, or the selected one.
    pub(super) fn chosen_entries(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            self.entries
                .get(self.selected_entry)
                .map(|entry| vec![entry.path.clone()])
                .unwrap_or_default()
        } else {
            // A folder and something inside it: the folder is enough.
            let marked: Vec<&PathBuf> = self.marked.iter().collect();
            marked
                .iter()
                .filter(|path| {
                    !marked
                        .iter()
                        .any(|other| other != *path && path.starts_with(other))
                })
                .map(|path| (*path).clone())
                .collect()
        }
    }

    /// The marked entries to the trash, after one question for all of them.
    pub(super) fn trash_marked(&mut self) -> anyhow::Result<()> {
        let chosen = self.chosen_entries();
        let mut sent = Vec::new();
        for path in &chosen {
            self.trash_entry(path)?;
            sent.push(path.clone());
        }
        self.marked.clear();
        if sent.len() > 1 {
            self.file_undo = Some(super::tree::FileUndo::Many(sent));
        }
        Ok(())
    }

    /// `/` and typing: every file and folder of the project whose name has the text, with the
    /// folders above them open; `.gitignore` is respected. Empty: the tree as it was.
    pub(super) fn apply_filter(&mut self) {
        let Some(filter) = self.explorer_filter.clone().filter(|text| !text.is_empty()) else {
            if let Err(error) = self.refresh_entries() {
                self.status = error.to_string();
            }
            return;
        };
        let needle = filter.to_lowercase();
        let mut found: Vec<PathBuf> = Vec::new();
        let mut more = false;
        for entry in ignore::WalkBuilder::new(&self.root)
            .hidden(false)
            .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != "target")
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.depth() > 0)
        {
            if entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .contains(&needle)
            {
                if found.len() == MOST_FOUND {
                    more = true;
                    break;
                }
                found.push(entry.into_path());
            }
        }
        // The matches and every folder above them, in the tree's order.
        let mut shown: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        for path in &found {
            for ancestor in path
                .ancestors()
                .take_while(|ancestor| *ancestor != self.root)
            {
                shown.insert(ancestor.to_path_buf());
            }
        }
        let mut entries = Vec::new();
        self.filtered_tree(&self.root.clone(), 0, &shown, &mut entries);
        let count = found.len();
        self.entries = entries;
        self.selected_entry = self
            .entries
            .iter()
            .position(|entry| found.contains(&entry.path))
            .unwrap_or(0);
        self.explorer_scroll = 0;
        self.status = match (count, more) {
            (0, _) => format!("No names with \"{filter}\""),
            (_, true) => format!("The first {MOST_FOUND} names with \"{filter}\""),
            (count, false) => format!("{count} names with \"{filter}\": Esc shows the tree again"),
        };
    }

    fn filtered_tree(
        &self,
        folder: &Path,
        depth: usize,
        shown: &std::collections::BTreeSet<PathBuf>,
        entries: &mut Vec<FileEntry>,
    ) {
        let mut children: Vec<&PathBuf> = shown
            .iter()
            .filter(|path| path.parent() == Some(folder))
            .collect();
        children.sort_by_cached_key(|path| {
            (
                !path.is_dir(),
                super::tree::natural_key(&path.file_name().unwrap_or_default().to_string_lossy()),
            )
        });
        for path in children {
            let is_dir = path.is_dir();
            entries.push(FileEntry {
                icon: if is_dir {
                    (' ', ratatui::style::Color::Reset)
                } else {
                    crate::editor::file_icon(path)
                },
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path: path.clone(),
                depth,
                is_dir,
                expanded: is_dir,
                more: 0,
            });
            if is_dir {
                self.filtered_tree(path, depth + 1, shown, entries);
            }
        }
    }

    /// Copy Path and Copy Relative Path: the path of what is chosen, one per line.
    pub(super) fn copy_paths(&mut self, relative: bool) {
        let paths = self
            .chosen_entries()
            .iter()
            .map(|path| {
                if relative {
                    path.strip_prefix(&self.root)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                } else {
                    path.display().to_string()
                }
            })
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return;
        }
        self.copy(paths.join("\n"));
        self.status = format!("Copied {}", paths.join(", "));
    }

    /// Duplicate: a copy next to it, with a free name (`name copy.rs`).
    pub(super) fn duplicate_chosen(&mut self) {
        let mut last = None;
        for path in self.chosen_entries() {
            let Some(folder) = path.parent() else {
                continue;
            };
            match copy_entry(&path, folder) {
                Ok(copy) => last = Some(copy),
                Err(error) => {
                    self.status = format!("{error:#}");
                    return;
                }
            }
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
        if let Some(copy) = last {
            self.status = format!(
                "Duplicated as {}",
                copy.strip_prefix(&self.root).unwrap_or(&copy).display()
            );
            self.reveal(&copy);
        }
    }

    /// Reveal in File Manager: the system's own file manager, with the entry selected where it
    /// can be (Finder, File Explorer) and its folder elsewhere.
    pub(super) fn reveal_in_system(&mut self) {
        let Some(path) = self.chosen_entries().into_iter().next() else {
            return;
        };
        if cfg!(test) {
            self.status = format!("Shown {}", path.display());
            return;
        }
        let mut command = match std::env::consts::OS {
            "macos" => {
                let mut command = Command::new("open");
                command.arg("-R").arg("--").arg(&path);
                command
            }
            "windows" => {
                let mut command = Command::new("explorer");
                // `explorer` takes `/select,` and the path as one argument.
                let mut select = std::ffi::OsString::from("/select,");
                select.push(&path);
                command.arg(select);
                command
            }
            _ => {
                let folder = if path.is_dir() {
                    path.clone()
                } else {
                    path.parent().unwrap_or(&self.root).to_path_buf()
                };
                let mut command = Command::new("xdg-open");
                command.arg(folder);
                command
            }
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            // Nothing waits on it: a thread reaps it so that it never stays a zombie.
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
                self.status = format!("Shown {} in the file manager", path.display());
            }
            Err(error) => self.status = format!("Could not open the file manager: {error}"),
        }
    }

    /// A drag that ended on another row: the question to move what is chosen into its folder.
    pub(super) fn drop_entries(&mut self, from: usize, onto: usize) {
        let (Some(source), Some(target)) = (self.entries.get(from), self.entries.get(onto)) else {
            return;
        };
        let folder = if target.is_dir {
            target.path.clone()
        } else {
            target.path.parent().unwrap_or(&self.root).to_path_buf()
        };
        if source.path.parent() == Some(folder.as_path()) || source.path == folder {
            return;
        }
        if !self.marked.contains(&source.path) {
            self.marked.clear();
            self.selected_entry = from;
        }
        self.move_into = Some(folder);
        self.name_prompt = Some(NamePrompt {
            action: EntryAction::MoveHere,
            target: source.path.clone(),
            name: String::new(),
            error: None,
        });
    }

    /// Enter on the question of a drag: everything chosen into the folder, never over a name
    /// that is there.
    pub(super) fn confirm_move(&mut self) -> anyhow::Result<()> {
        let folder = self
            .move_into
            .take()
            .ok_or_else(|| anyhow::anyhow!("Nowhere to move to"))?;
        let mut moved = Vec::new();
        for path in self.chosen_entries() {
            let to = move_entry(&self.root, &path, &folder)?;
            self.follow_rename(&path, &to);
            moved.push((path, to));
        }
        self.marked.clear();
        if let Some((from, to)) = moved.last().cloned() {
            self.file_undo = Some(super::tree::FileUndo::Moved { from, to });
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
        self.status = format!(
            "Moved {} into {}",
            moved.len(),
            folder.strip_prefix(&self.root).unwrap_or(&folder).display()
        );
        Ok(())
    }
}
