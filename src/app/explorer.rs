use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
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
        Ok(())
    }

    fn collect_entries(
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

    fn toggle_directory(&mut self, path: &Path) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.expanded = !entry.expanded;
        }
        if let Err(error) = self.refresh_entries() {
            self.status = error.to_string();
        }
    }
}
