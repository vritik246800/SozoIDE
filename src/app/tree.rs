//! The Explorer as a file explorer (F01, TODO 7.2.2): the natural order of names, moving in
//! the tree by the keyboard, revealing the file of the active tab and folding everything.

use std::path::{Path, PathBuf};

use super::{App, EntryAction, FileEntry, NamePrompt};
use crate::animation::{ROWS, Tween};

/// The buttons on the Explorer's title (TODO 7.3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerAction {
    NewFile,
    NewFolder,
    Refresh,
    CollapseAll,
}

/// The last thing the Explorer did to a file that `Ctrl+Z` undoes (R14).
pub enum FileUndo {
    /// A rename or a move: back from `to` to `from`.
    Moved { from: PathBuf, to: PathBuf },
    /// Sent to the system's trash from `path`.
    Trashed { path: PathBuf },
    /// A link removed (a link never goes to the trash, which could follow it): made again.
    Unlinked { path: PathBuf, target: PathBuf },
    /// Several marked entries sent to the trash at once (F01).
    Many(Vec<PathBuf>),
}

/// Sends `path` to the system's trash. The tests never touch the trash of whoever runs them:
/// there it is a folder of the test's own.
#[cfg(not(test))]
fn to_trash(path: &Path) -> anyhow::Result<()> {
    trash::delete(path).map_err(|error| anyhow::anyhow!("Could not move it to the Trash: {error}"))
}

#[cfg(test)]
pub(crate) fn test_trash() -> PathBuf {
    std::env::temp_dir().join(format!("sozo-test-trash-{}", std::process::id()))
}

#[cfg(test)]
fn to_trash(path: &Path) -> anyhow::Result<()> {
    let folder = test_trash();
    std::fs::create_dir_all(&folder)?;
    let name = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    std::fs::rename(path, folder.join(name))?;
    Ok(())
}

/// Brings `path` back from the system's trash where the system can say where it went.
#[cfg(all(not(test), any(windows, all(unix, not(target_os = "macos")))))]
fn from_trash(path: &Path) -> anyhow::Result<()> {
    let item = trash::os_limited::list()
        .map_err(|error| anyhow::anyhow!("Could not read the Trash: {error}"))?
        .into_iter()
        .filter(|item| item.original_path() == path)
        .max_by_key(|item| item.time_deleted)
        .ok_or_else(|| anyhow::anyhow!("It is no longer in the Trash"))?;
    trash::os_limited::restore_all([item])
        .map_err(|error| anyhow::anyhow!("Could not bring it back: {error}"))
}

#[cfg(all(not(test), target_os = "macos"))]
fn from_trash(_: &Path) -> anyhow::Result<()> {
    anyhow::bail!("It is in the Trash: Put Back from the Finder brings it back")
}

#[cfg(test)]
fn from_trash(path: &Path) -> anyhow::Result<()> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let found = std::fs::read_dir(test_trash())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|item| {
            item.file_name()
                .is_some_and(|file| file.to_string_lossy().ends_with(&format!("-{name}")))
        })
        .max()
        .ok_or_else(|| anyhow::anyhow!("It is no longer in the Trash"))?;
    std::fs::rename(found, path)?;
    Ok(())
}

/// The rows of a folder that opens or closes, drawn as they come in or go (`ROWS`): from the
/// row `at`, `count` new rows, or the rows that were there (`removed`).
pub struct RowsMotion {
    pub at: usize,
    pub count: usize,
    pub removed: Vec<FileEntry>,
    pub tween: Tween,
}

/// A name split into its runs of digits and of everything else: `file10` sorts after `file2`.
pub(crate) fn natural_key(name: &str) -> Vec<(u8, String)> {
    let mut key = Vec::new();
    let mut run = String::new();
    let mut digits = false;
    for character in name.chars().flat_map(char::to_lowercase) {
        let digit = character.is_ascii_digit();
        if !run.is_empty() && digit != digits {
            key.push(chunk(std::mem::take(&mut run), digits));
        }
        digits = digit;
        run.push(character);
    }
    if !run.is_empty() {
        key.push(chunk(run, digits));
    }
    key
}

/// A run of digits compares by its value: shorter (without leading zeros) is smaller.
fn chunk(run: String, digits: bool) -> (u8, String) {
    if digits {
        let value = run.trim_start_matches('0');
        // The length first, as a fixed-width prefix, then the digits themselves.
        (0, format!("{:020}{value}", value.len()))
    } else {
        (1, run)
    }
}

#[cfg(test)]
pub(crate) fn natural_order(a: &str, b: &str) -> std::cmp::Ordering {
    natural_key(a).cmp(&natural_key(b))
}

impl App {
    /// `←`: an open folder closes; anything else goes to its folder.
    pub(super) fn explorer_left(&mut self) {
        let Some(entry) = self.entries.get(self.selected_entry) else {
            return;
        };
        if entry.is_dir && entry.expanded {
            let path = entry.path.clone();
            return self.set_folder(&path, false);
        }
        let depth = entry.depth;
        if let Some(parent) = (0..self.selected_entry)
            .rev()
            .find(|&index| self.entries[index].depth < depth)
        {
            self.selected_entry = parent;
        }
    }

    /// `→`: a closed folder opens, an open one goes to its first entry, a file opens.
    pub(super) fn explorer_right(&mut self) {
        let Some(entry) = self.entries.get(self.selected_entry) else {
            return;
        };
        match (entry.is_dir, entry.expanded) {
            (true, false) => {
                let path = entry.path.clone();
                self.set_folder(&path, true);
            }
            (true, true) => {
                let next = self.selected_entry + 1;
                if self
                    .entries
                    .get(next)
                    .is_some_and(|child| child.depth > entry.depth)
                {
                    self.selected_entry = next;
                }
            }
            (false, _) => self.activate_selected(),
        }
    }

    /// `PageUp`/`PageDown`: a screen of rows of the Explorer.
    pub(super) fn explorer_page(&mut self, down: bool) {
        let page = usize::from(self.regions.explorer.height.saturating_sub(3)).max(1);
        let last = self.entries.len().saturating_sub(1);
        self.selected_entry = if down {
            (self.selected_entry + page).min(last)
        } else {
            self.selected_entry.saturating_sub(page)
        };
    }

    /// Opens or closes `folder` and reads the tree again, keeping the selection on it; its
    /// rows come in or go (`ROWS`).
    pub(super) fn set_folder(&mut self, folder: &Path, open: bool) {
        let Some(index) = self.entries.iter().position(|entry| entry.path == folder) else {
            return;
        };
        let depth = self.entries[index].depth;
        let below = |entries: &[FileEntry]| {
            entries[index + 1..]
                .iter()
                .take_while(|entry| entry.depth > depth)
                .count()
        };
        let removed = if open {
            Vec::new()
        } else {
            let count = below(&self.entries);
            self.entries[index + 1..index + 1 + count].to_vec()
        };
        self.entries[index].expanded = open;
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
        self.select_path(folder);
        let now = self.motion.now();
        let count = if open { below(&self.entries) } else { 0 };
        let mut tween = Tween::new(now, if open { 0.0 } else { 1.0 }, ROWS);
        tween.set_target(if open { 1.0 } else { 0.0 }, now);
        self.explorer_rows = (count > 0 || !removed.is_empty()).then_some(RowsMotion {
            at: index + 1,
            count,
            removed,
            tween,
        });
    }

    /// The Explorer's rows as they are drawn this frame: the rows of a folder that opens or
    /// closes come in or go one by one; the keys always act on `entries`.
    pub fn shown_entries(&self) -> Vec<&FileEntry> {
        let Some(rows) = self
            .explorer_rows
            .as_ref()
            .filter(|rows| !rows.tween.is_finished(self.motion.now()))
            .filter(|rows| rows.at <= self.entries.len())
        else {
            return self.entries.iter().collect();
        };
        let progress = rows.tween.value(self.motion.now());
        let (head, tail) = self.entries.split_at(rows.at);
        if rows.removed.is_empty() {
            let count = rows.count.min(tail.len());
            let shown = ((count as f32 * progress).ceil() as usize).min(count);
            head.iter()
                .chain(&tail[..shown])
                .chain(&tail[count..])
                .collect()
        } else {
            let count = rows.removed.len();
            let shown = ((count as f32 * progress).ceil() as usize).min(count);
            head.iter()
                .chain(&rows.removed[..shown])
                .chain(tail)
                .collect()
        }
    }

    /// The branch checked out and how many files changed, for the status bar (TODO 7.3.3): the
    /// branch from `.git/HEAD` (a few bytes, no Git process), the changes from the Explorer's.
    pub fn branch_and_changes(&self) -> Option<(String, usize)> {
        let head = std::fs::read_to_string(self.root.join(".git/HEAD")).ok()?;
        let head = head.trim();
        let branch = match head.strip_prefix("ref: refs/heads/") {
            Some(branch) => branch.to_owned(),
            // A detached HEAD: the short commit.
            None => head.get(..7)?.to_owned(),
        };
        if branch.is_empty() || branch.chars().any(char::is_control) {
            return None;
        }
        let folders: std::collections::HashSet<&Path> = self
            .git_changes
            .keys()
            .flat_map(|path| path.ancestors().skip(1))
            .collect();
        let changes = self
            .git_changes
            .keys()
            .filter(|path| !folders.contains(path.as_path()))
            .count();
        Some((branch, changes))
    }

    /// Sends a file or folder of the project to the trash; a link is removed (and remembered)
    /// so that nothing outside the project is ever touched through it.
    pub(super) fn trash_entry(&mut self, path: &Path) -> anyhow::Result<()> {
        if path == self.root || !path.starts_with(&self.root) {
            anyhow::bail!("Only files and folders inside the project can be deleted");
        }
        let data = std::fs::symlink_metadata(path)?;
        if data.is_symlink() {
            let target = std::fs::read_link(path)?;
            std::fs::remove_file(path)?;
            self.file_undo = Some(FileUndo::Unlinked {
                path: path.to_path_buf(),
                target,
            });
            return Ok(());
        }
        to_trash(path)?;
        self.file_undo = Some(FileUndo::Trashed {
            path: path.to_path_buf(),
        });
        Ok(())
    }

    /// `Ctrl+Z` in the Explorer: the last rename, move or delete undone, never over a file
    /// that took its place.
    pub(super) fn undo_file_operation(&mut self) {
        let Some(undo) = self.file_undo.take() else {
            self.status = "Nothing to undo in the Explorer".into();
            return;
        };
        let result = match &undo {
            FileUndo::Moved { from, to } => {
                if from.exists() || std::fs::symlink_metadata(from).is_ok() {
                    Err(anyhow::anyhow!("{} exists again", from.display()))
                } else {
                    std::fs::rename(to, from).map_err(anyhow::Error::from)
                }
            }
            FileUndo::Trashed { path } | FileUndo::Unlinked { path, .. }
                if std::fs::symlink_metadata(path).is_ok() =>
            {
                Err(anyhow::anyhow!("{} exists again", path.display()))
            }
            FileUndo::Trashed { path } => from_trash(path),
            FileUndo::Unlinked { path, target } => link(target, path),
            FileUndo::Many(paths) => paths
                .iter()
                .filter(|path| std::fs::symlink_metadata(path).is_err())
                .try_for_each(|path| from_trash(path)),
        };
        let (path, from) = match &undo {
            FileUndo::Moved { from, to } => (from.clone(), Some(to.clone())),
            FileUndo::Trashed { path } | FileUndo::Unlinked { path, .. } => (path.clone(), None),
            FileUndo::Many(paths) => match paths.first() {
                Some(path) => (path.clone(), None),
                None => return,
            },
        };
        match result {
            Ok(()) => {
                if let Some(to) = from {
                    self.follow_rename(&to, &path);
                }
                if let Err(error) = self.refresh_entries() {
                    self.status = error.to_string();
                    return;
                }
                self.reveal(&path);
                self.status = format!(
                    "Undone: {} is back",
                    path.strip_prefix(&self.root).unwrap_or(&path).display()
                );
            }
            Err(error) => self.status = format!("Could not undo: {error:#}"),
        }
    }

    /// A button on the Explorer's title.
    pub fn explorer_action(&mut self, action: ExplorerAction) {
        match action {
            ExplorerAction::NewFile => self.prompt_selected(EntryAction::NewFile),
            ExplorerAction::NewFolder => self.prompt_selected(EntryAction::NewFolder),
            ExplorerAction::Refresh => self.refresh_explorer(),
            ExplorerAction::CollapseAll => self.collapse_all(),
        }
    }

    fn select_path(&mut self, path: &Path) {
        if let Some(index) = self.entries.iter().position(|entry| entry.path == path) {
            self.selected_entry = index;
        }
    }

    /// Every folder closed: the project as it opened.
    pub fn collapse_all(&mut self) {
        let selected = self
            .entries
            .get(self.selected_entry)
            .map(|entry| self.top_folder_of(&entry.path));
        for entry in &mut self.entries {
            entry.expanded = false;
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
        if let Some(path) = selected {
            self.select_path(&path);
        }
        self.keep_selection_visible();
    }

    /// The entry at the top of the tree that holds `path`.
    fn top_folder_of(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .ok()
            .and_then(|rest| rest.components().next())
            .map_or_else(|| path.to_path_buf(), |first| self.root.join(first))
    }

    /// The tree read again from the disk, with the same folders open (the title's Refresh).
    pub fn refresh_explorer(&mut self) {
        match self.refresh_entries() {
            Ok(()) => self.status = "Explorer read again".into(),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// Opens every folder above `path` and selects it: the Explorer follows the active tab.
    pub fn reveal(&mut self, path: &Path) {
        // A path from elsewhere (a language server, a test) may name the folder another way.
        let canonical;
        let path = if path.starts_with(&self.root) {
            path
        } else {
            match path.canonicalize() {
                Ok(found) if found.starts_with(&self.root) => {
                    canonical = found;
                    canonical.as_path()
                }
                _ => return,
            }
        };
        if path == self.root {
            return;
        }
        if self
            .entries
            .get(self.selected_entry)
            .is_some_and(|entry| entry.path == path)
        {
            return;
        }
        let mut closed = false;
        for folder in path
            .ancestors()
            .skip(1)
            .take_while(|folder| *folder != self.root)
        {
            match self.entries.iter_mut().find(|entry| entry.path == folder) {
                Some(entry) if !entry.expanded => {
                    entry.expanded = true;
                    closed = true;
                }
                Some(_) => {}
                // A folder still closed above it is not in the list yet: read and look again.
                None => closed = true,
            }
        }
        if closed {
            let mut expanded: Vec<PathBuf> = self
                .entries
                .iter()
                .filter(|entry| entry.expanded)
                .map(|entry| entry.path.clone())
                .collect();
            expanded.extend(
                path.ancestors()
                    .skip(1)
                    .take_while(|folder| *folder != self.root)
                    .map(Path::to_path_buf),
            );
            let mut entries = Vec::new();
            if Self::collect_entries(&self.root, 0, &expanded, &mut entries).is_ok() {
                self.entries = entries;
            }
        }
        self.select_path(path);
        self.keep_selection_visible();
    }

    /// The name prompt for `action` on the selected entry, without the menu: `F2`, `Delete`,
    /// `N` and `Shift+N` in the Explorer.
    pub(crate) fn prompt_selected(&mut self, action: EntryAction) {
        let target = self
            .entries
            .get(self.selected_entry)
            .map_or_else(|| self.root.clone(), |entry| entry.path.clone());
        if matches!(
            action,
            EntryAction::Rename | EntryAction::Delete | EntryAction::DeleteForever
        ) && target == self.root
        {
            return;
        }
        let name = if action == EntryAction::Rename {
            target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.name_prompt = Some(NamePrompt {
            action,
            target,
            name,
            error: None,
        });
    }
}

#[cfg(unix)]
fn link(target: &Path, path: &Path) -> anyhow::Result<()> {
    Ok(std::os::unix::fs::symlink(target, path)?)
}

#[cfg(windows)]
fn link(target: &Path, path: &Path) -> anyhow::Result<()> {
    Ok(if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, path)
    } else {
        std::os::windows::fs::symlink_file(target, path)
    }?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn the_status_bar_reads_the_branch_and_ignores_a_strange_head() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        let mut app = App::new(root.to_path_buf()).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        app.git_changes = [
            (
                "src",
                (
                    crate::git::FileStatus::Modified,
                    crate::git::ChangeArea::Unstaged,
                ),
            ),
            (
                "src/a.rs",
                (
                    crate::git::FileStatus::Modified,
                    crate::git::ChangeArea::Unstaged,
                ),
            ),
            (
                "b.rs",
                (
                    crate::git::FileStatus::Added,
                    crate::git::ChangeArea::Untracked,
                ),
            ),
        ]
        .into_iter()
        .map(|(path, mark)| (PathBuf::from(path), mark))
        .collect();
        assert_eq!(app.branch_and_changes(), Some(("feature/x".into(), 2)));
        std::fs::write(root.join(".git/HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(app.branch_and_changes().unwrap().0, "0123456");
        for hostile in ["ref: refs/heads/\u{1b}[31mred\n", "", "abc"] {
            std::fs::write(root.join(".git/HEAD"), hostile).unwrap();
            assert!(app.branch_and_changes().is_none(), "{hostile:?}");
        }
    }

    #[test]
    fn names_sort_as_people_read_them() {
        let mut names = vec![
            "file10", "File2", "file1", "a", "file02x", "file2", "B", "10", "9",
        ];
        names.sort_by(|a, b| natural_order(a, b));
        assert_eq!(
            names,
            [
                "9", "10", "a", "B", "file1", "File2", "file2", "file02x", "file10"
            ]
        );
        // Huge numbers, Unicode and empty names never panic and keep an order.
        let long = "x".repeat(10) + &"9".repeat(40);
        assert_eq!(natural_order(&long, "x1"), Ordering::Greater);
        assert_eq!(natural_order("", "a"), Ordering::Less);
        assert_eq!(natural_order("Ébano2", "ébano10"), Ordering::Less);
    }
}
