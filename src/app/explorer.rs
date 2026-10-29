use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use ratatui::style::Color;

use super::App;
use crate::editor::file_icon;

pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    /// The file type icon, looked up once (directories use the folder icons).
    pub icon: (char, Color),
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

/// What the Explorer's right-click menu does (F01).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryAction {
    NewFile,
    NewFolder,
    Rename,
    Delete,
    /// The clipboard: files and folders in the Explorer, text in the editor.
    Cut,
    Copy,
    Paste,
}

impl EntryAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewFile => "New File",
            Self::NewFolder => "New Folder",
            Self::Rename => "Rename",
            Self::Delete => "Delete",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
        }
    }
}

/// The right-click menu: its entries, where it opened and the entry it acts on (the root for
/// the empty space below the tree, which cannot be renamed).
pub struct EntryMenu {
    pub target: PathBuf,
    pub actions: Vec<EntryAction>,
    pub position: (u16, u16),
    pub selected: usize,
    /// Opened in the editor text: Cut, Copy and Paste act on the text (F02).
    pub editor: bool,
}

/// The name being typed for a new file or folder, or for a rename.
pub struct NamePrompt {
    pub action: EntryAction,
    pub target: PathBuf,
    pub name: String,
    pub error: Option<String>,
}

/// A file or folder name: no separators, not `.` or `..`, not empty.
fn checked_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        bail!("Type a name");
    }
    if name == "." || name == ".." || name.contains(['/', '\\']) {
        bail!("A name cannot be . or .. or contain / or \\");
    }
    Ok(name)
}

/// Creates `name` inside `parent`, a folder when `folder`; never over something that exists.
pub fn create_entry(parent: &Path, name: &str, folder: bool) -> Result<PathBuf> {
    let path = parent.join(checked_name(name)?);
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    if folder {
        fs::create_dir(&path)
    } else {
        fs::File::create_new(&path).map(drop)
    }
    .with_context(|| format!("Could not create {}", path.display()))?;
    Ok(path)
}

/// Renames `path` to `name` in the same folder; never over something that exists.
pub fn rename_entry(path: &Path, name: &str) -> Result<PathBuf> {
    let parent = path
        .parent()
        .context("The project folder cannot be renamed")?;
    let renamed = parent.join(checked_name(name)?);
    if renamed == path {
        return Ok(renamed);
    }
    if renamed.exists() {
        bail!("{} already exists", renamed.display());
    }
    fs::rename(path, &renamed).with_context(|| format!("Could not rename {}", path.display()))?;
    Ok(renamed)
}

/// Deletes a file, or a folder with everything in it, for good (Q39); never `root`.
pub fn delete_entry(root: &Path, path: &Path) -> Result<()> {
    if path == root || !path.starts_with(root) {
        bail!("Only files and folders inside the project can be deleted");
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| format!("Could not delete {}", path.display()))
}

/// Copies a file, or a folder with everything in it, into `folder`; next to itself it takes a
/// free name: `name copy.rs`, `name copy 2.rs`...
pub fn copy_entry(path: &Path, folder: &Path) -> Result<PathBuf> {
    if folder.starts_with(path) {
        bail!("A folder cannot be copied into itself");
    }
    let name = path.file_name().context("Nothing to copy")?;
    let mut target = folder.join(name);
    let mut copies = 1;
    while target.exists() {
        target = folder.join(copy_name(path, copies));
        copies += 1;
    }
    copy_tree(path, &target).with_context(|| format!("Could not copy {}", path.display()))?;
    Ok(target)
}

/// `main copy.rs`, `main copy 2.rs`; a folder or a name without an extension ends with it.
fn copy_name(path: &Path, copies: usize) -> String {
    let suffix = match copies {
        1 => " copy".to_owned(),
        copies => format!(" copy {copies}"),
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match (path.is_dir(), path.file_stem(), path.extension()) {
        (false, Some(stem), Some(extension)) => format!(
            "{}{suffix}.{}",
            stem.to_string_lossy(),
            extension.to_string_lossy()
        ),
        _ => format!("{name}{suffix}"),
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if !fs::symlink_metadata(from)?.is_dir() {
        return fs::copy(from, to).map(drop);
    }
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

/// Moves a file or a folder into `folder`; never the project, over something that exists or
/// into itself.
pub fn move_entry(root: &Path, path: &Path, folder: &Path) -> Result<PathBuf> {
    if path == root || !path.starts_with(root) {
        bail!("Only files and folders inside the project can be moved");
    }
    if folder.starts_with(path) {
        bail!("A folder cannot be moved into itself");
    }
    let target = folder.join(path.file_name().context("Nothing to move")?);
    if target == path {
        return Ok(target);
    }
    if target.exists() {
        bail!("{} already exists", target.display());
    }
    fs::rename(path, &target).with_context(|| format!("Could not move {}", path.display()))?;
    Ok(target)
}

impl App {
    pub fn refresh_entries(&mut self) -> Result<()> {
        let selected_path = self
            .entries
            .get(self.selected_entry)
            .map(|entry| entry.path.clone());
        let expanded = self
            .entries
            .iter()
            .filter(|entry| entry.expanded)
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        self.entries.clear();
        Self::collect_entries(&self.root, 0, &expanded, &mut self.entries)?;
        if let Some(path) = selected_path
            && let Some(index) = self.entries.iter().position(|entry| entry.path == path)
        {
            self.selected_entry = index;
        }
        self.selected_entry = self
            .selected_entry
            .min(self.entries.len().saturating_sub(1));
        self.folders = self.folder_times();
        Ok(())
    }

    pub(super) fn collect_entries(
        dir: &Path,
        depth: usize,
        expanded: &[PathBuf],
        entries: &mut Vec<FileEntry>,
    ) -> Result<()> {
        let mut children = fs::read_dir(dir)
            .with_context(|| format!("Could not read {}", dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name();
                name != ".git" && name != "target"
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|entry| {
            let is_file = entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false);
            (is_file, entry.file_name().to_string_lossy().to_lowercase())
        });
        for child in children {
            let path = child.path();
            let is_dir = child.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let is_expanded = expanded.contains(&path);
            entries.push(FileEntry {
                icon: if is_dir {
                    (' ', Color::Reset)
                } else {
                    file_icon(&path)
                },
                name: child.file_name().to_string_lossy().into_owned(),
                path: path.clone(),
                depth,
                is_dir,
                expanded: is_expanded,
            });
            if is_dir && is_expanded {
                Self::collect_entries(&path, depth + 1, expanded, entries)?;
            }
        }
        Ok(())
    }

    pub fn activate_selected(&mut self) {
        let Some(entry) = self.entries.get(self.selected_entry) else {
            return;
        };
        let path = entry.path.clone();
        if entry.is_dir {
            self.toggle_directory(&path);
        } else if let Err(error) = self.open_file(path) {
            self.status = error.to_string();
        }
    }

    /// Opens the right-click menu at `position` for the entry at `index`, or for the root.
    pub fn open_entry_menu(&mut self, index: Option<usize>, position: (u16, u16)) {
        let target = index
            .and_then(|index| self.entries.get(index))
            .map_or_else(|| self.root.clone(), |entry| entry.path.clone());
        let mut actions = vec![EntryAction::NewFile, EntryAction::NewFolder];
        if target != self.root {
            actions.extend([EntryAction::Cut, EntryAction::Copy]);
        }
        if self.file_clipboard.is_some() {
            actions.push(EntryAction::Paste);
        }
        if target != self.root {
            actions.extend([EntryAction::Rename, EntryAction::Delete]);
        }
        if let Some(index) = index.filter(|index| *index < self.entries.len()) {
            self.selected_entry = index;
        }
        self.entry_menu = Some(EntryMenu {
            target,
            actions,
            position,
            selected: 0,
            editor: false,
        });
    }

    /// Runs a menu entry: the clipboard at once, otherwise a prompt for the name (the old one
    /// when renaming).
    pub fn choose_entry_action(&mut self, action: EntryAction) {
        let Some(menu) = self.entry_menu.take() else {
            return;
        };
        if menu.editor {
            return self.editor_action(action);
        }
        match action {
            EntryAction::Cut | EntryAction::Copy => {
                return self.hold_entry(menu.target, action == EntryAction::Cut);
            }
            EntryAction::Paste => return self.paste_entry(&menu.target),
            _ => {}
        }
        let name = if action == EntryAction::Rename {
            menu.target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.name_prompt = Some(NamePrompt {
            action,
            target: menu.target,
            name,
            error: None,
        });
    }

    /// Creates or renames as the prompt says; on an error the prompt stays with it.
    pub fn confirm_name(&mut self) {
        let Some(prompt) = &mut self.name_prompt else {
            return;
        };
        // New entries go inside a folder, next to a file.
        let parent = if prompt.target.is_dir() {
            prompt.target.clone()
        } else {
            prompt
                .target
                .parent()
                .map_or_else(|| self.root.clone(), Path::to_path_buf)
        };
        if prompt.action == EntryAction::Delete {
            let target = prompt.target.clone();
            match delete_entry(&self.root, &target) {
                Ok(()) => self.deleted(&target),
                Err(error) => prompt.error = Some(format!("{error:#}")),
            }
            return;
        }
        let result = match prompt.action {
            EntryAction::NewFile => create_entry(&parent, &prompt.name, false),
            EntryAction::NewFolder => create_entry(&parent, &prompt.name, true),
            _ => rename_entry(&prompt.target, &prompt.name),
        };
        let path = match result {
            Ok(path) => path,
            Err(error) => {
                prompt.error = Some(format!("{error:#}"));
                return;
            }
        };
        let (action, old) = (prompt.action, prompt.target.clone());
        self.name_prompt = None;
        if action == EntryAction::Rename {
            self.renamed(&old, &path);
        } else if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == parent) {
            entry.expanded = true;
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
        if let Some(index) = self.entries.iter().position(|entry| entry.path == path) {
            self.selected_entry = index;
        }
        if action == EntryAction::NewFile
            && let Err(error) = self.open_file(path)
        {
            self.status = error.to_string();
        }
    }

    /// Ctrl+C or Ctrl+X in the Explorer: the selected file or folder.
    pub(super) fn hold_selected_entry(&mut self, cut: bool) {
        if let Some(entry) = self.entries.get(self.selected_entry) {
            self.hold_entry(entry.path.clone(), cut);
        }
    }

    /// Keeps `path` to copy or move with Paste.
    fn hold_entry(&mut self, path: PathBuf, cut: bool) {
        let name = path.strip_prefix(&self.root).unwrap_or(&path).display();
        self.status = if cut {
            format!("Cut {name}: Paste moves it")
        } else {
            format!("Copied {name}")
        };
        self.file_clipboard = Some((path, cut));
    }

    /// Ctrl+V in the Explorer: into the selected folder, or next to the selected file.
    pub(super) fn paste_selected_entry(&mut self) {
        let target = self
            .entries
            .get(self.selected_entry)
            .map_or_else(|| self.root.clone(), |entry| entry.path.clone());
        self.paste_entry(&target);
    }

    /// Copies or moves what Copy or Cut kept into `target`, a folder, or next to a file.
    fn paste_entry(&mut self, target: &Path) {
        let Some((path, cut)) = self.file_clipboard.clone() else {
            self.status = "Copy or cut a file or folder first".into();
            return;
        };
        let folder = if target.is_dir() {
            target.to_path_buf()
        } else {
            target
                .parent()
                .map_or_else(|| self.root.clone(), Path::to_path_buf)
        };
        let result = if cut {
            move_entry(&self.root, &path, &folder)
        } else {
            copy_entry(&path, &folder)
        };
        let pasted = match result {
            Ok(pasted) => pasted,
            Err(error) => {
                self.status = format!("{error:#}");
                return;
            }
        };
        if cut {
            // A move is done once; its tabs follow it.
            self.file_clipboard = None;
            self.renamed(&path, &pasted);
        }
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == folder) {
            entry.expanded = true;
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
            return;
        }
        if let Some(index) = self.entries.iter().position(|entry| entry.path == pasted) {
            self.selected_entry = index;
        }
        self.status = format!(
            "{} {}",
            if cut { "Moved to" } else { "Pasted" },
            pasted.strip_prefix(&self.root).unwrap_or(&pasted).display()
        );
    }

    /// After a delete: the tabs of what was deleted close without saving it back.
    fn deleted(&mut self, path: &Path) {
        self.name_prompt = None;
        while let Some(index) = self
            .open_files
            .iter()
            .position(|file| file.path.starts_with(path))
        {
            self.open_files[index].dirty = false;
            self.close_tab(index);
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
        self.status = format!(
            "Deleted {}",
            path.strip_prefix(&self.root).unwrap_or(path).display()
        );
    }

    /// Follows a rename in the tree (expanded folders) and in the open tabs.
    fn renamed(&mut self, old: &Path, new: &Path) {
        for entry in &mut self.entries {
            if let Ok(rest) = entry.path.strip_prefix(old) {
                entry.path = new.join(rest);
            }
        }
        for file in &mut self.open_files {
            if let Ok(rest) = file.path.strip_prefix(old) {
                file.path = new.join(rest);
                file.name = file
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                file.icon = file_icon(&file.path);
            }
        }
    }

    fn toggle_directory(&mut self, path: &Path) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.expanded = !entry.expanded;
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_created_and_renamed_inside_their_folder() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let file = create_entry(root, " main.rs ", false).unwrap();
        assert_eq!(file, root.join("main.rs"));
        assert!(file.is_file());
        let folder = create_entry(root, "src", true).unwrap();
        assert!(folder.is_dir());
        for name in ["", "  ", ".", "..", "a/b", "a\\b", "main.rs"] {
            assert!(create_entry(root, name, false).is_err(), "{name:?}");
        }
        let renamed = rename_entry(&file, "lib.rs").unwrap();
        assert_eq!(renamed, root.join("lib.rs"));
        assert!(!file.exists() && renamed.is_file());
        assert!(rename_entry(&renamed, "src").is_err());
        // Deleting takes a folder with what it has, and never the project.
        fs::write(folder.join("inner.txt"), "x").unwrap();
        delete_entry(root, &folder).unwrap();
        delete_entry(root, &renamed).unwrap();
        assert!(!folder.exists() && !renamed.exists());
        assert!(delete_entry(root, root).is_err());
        assert!(delete_entry(root, &root.join("missing")).is_err());
        assert_eq!(rename_entry(&renamed, "lib.rs").unwrap(), renamed);
    }

    #[test]
    fn entries_are_copied_with_a_free_name_and_moved_never_over_others() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let src = create_entry(root, "src", true).unwrap();
        fs::create_dir(src.join("inner")).unwrap();
        fs::write(src.join("inner/a.rs"), "a").unwrap();
        let file = root.join("main.rs");
        fs::write(&file, "fn main() {}").unwrap();
        // Next to itself a copy takes a free name; a folder comes with what it has.
        assert_eq!(copy_entry(&file, root).unwrap(), root.join("main copy.rs"));
        assert_eq!(
            copy_entry(&file, root).unwrap(),
            root.join("main copy 2.rs")
        );
        let copied = copy_entry(&src, root).unwrap();
        assert_eq!(copied, root.join("src copy"));
        assert_eq!(fs::read_to_string(copied.join("inner/a.rs")).unwrap(), "a");
        assert!(copy_entry(&src, &src.join("inner")).is_err());
        let into = copy_entry(&file, &src).unwrap();
        assert_eq!(fs::read_to_string(into).unwrap(), "fn main() {}");

        // A move goes into a folder, never over something there, into itself or the project.
        assert!(move_entry(root, &file, &src).is_err(), "src/main.rs exists");
        let moved = move_entry(root, &root.join("main copy.rs"), &src).unwrap();
        assert_eq!(moved, src.join("main copy.rs"));
        assert!(!root.join("main copy.rs").exists() && moved.is_file());
        assert!(move_entry(root, &src, &src.join("inner")).is_err());
        assert!(move_entry(root, root, &src).is_err());
        assert_eq!(move_entry(root, &moved, &src).unwrap(), moved);
    }
}
