//! Each folder opens as it was left, as VS Code does (F28): the folders open in the Explorer,
//! the tabs with their cursors, the active tab and the panels, kept per folder in
//! `sessions.toml` in the settings folder. Written within a second of a change and on quit.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{App, Focus};

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub(super) struct Session {
    expanded: Vec<PathBuf>,
    active: Option<PathBuf>,
    explorer: bool,
    bottom_terminal: bool,
    right_terminal: bool,
    explorer_width: u16,
    right_terminal_width: u16,
    bottom_terminal_height: u16,
    tabs: Vec<Tab>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
struct Tab {
    path: PathBuf,
    row: usize,
    column: usize,
}

/// Where the sessions are kept. Debug builds (tests, `cargo run`) neither read nor write them,
/// like the updater's settings; the tests set their own file.
pub(super) fn sessions_path() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    directories::ProjectDirs::from("", "", "SozoIDE")
        .map(|directories| directories.config_dir().join("sessions.toml"))
}

/// Every folder's session, by its full path; none when the file is missing or unreadable.
fn load(path: &Path) -> BTreeMap<String, Session> {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| toml::from_str(&contents).ok())
        .unwrap_or_default()
}

fn key(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

impl App {
    fn session(&self) -> Session {
        Session {
            expanded: self
                .entries
                .iter()
                .filter(|entry| entry.expanded)
                .map(|entry| entry.path.clone())
                .collect(),
            active: self
                .active_tab
                .and_then(|index| self.open_files.get(index))
                .map(|file| file.path.clone()),
            explorer: self.show_explorer,
            bottom_terminal: self.show_bottom_terminal,
            right_terminal: self.show_right_terminal,
            explorer_width: self.explorer_width,
            right_terminal_width: self.right_terminal_width,
            bottom_terminal_height: self.bottom_terminal_height,
            tabs: self
                .open_files
                .iter()
                .map(|file| Tab {
                    path: file.path.clone(),
                    row: file.cursor_row,
                    column: file.cursor_column,
                })
                .collect(),
        }
    }

    /// Writes this folder's session when it changed since it was last written; the other
    /// folders' sessions in the file stay.
    pub(super) fn save_session(&mut self) {
        let Some(path) = self.sessions.clone() else {
            return;
        };
        let session = self.session();
        if self.saved_session.as_ref() == Some(&session) {
            return;
        }
        let mut sessions = load(&path);
        sessions.insert(key(&self.root), session.clone());
        let written = path.parent().map_or(Ok(()), fs::create_dir_all).is_ok()
            && toml::to_string(&sessions)
                .ok()
                .is_some_and(|text| fs::write(&path, text).is_ok());
        if written {
            self.saved_session = Some(session);
        }
    }

    /// Puts back the folder as it was left: what no longer exists is skipped.
    pub(super) fn restore_session(&mut self) -> Result<()> {
        let Some(session) = self
            .sessions
            .as_deref()
            .map(load)
            .and_then(|mut sessions| sessions.remove(&key(&self.root)))
        else {
            return Ok(());
        };
        self.entries.clear();
        Self::collect_entries(&self.root, 0, &session.expanded, &mut self.entries)?;
        self.folders = self.folder_times();
        for tab in &session.tabs {
            if !tab.path.is_file() || self.open_file(tab.path.clone()).is_err() {
                continue;
            }
            let Some(file) = self.open_files.last_mut() else {
                continue;
            };
            file.move_cursor_to(tab.row, tab.column);
            // A video waits for Space instead of playing as the folder opens.
            if let Some(media) = file.media.as_mut()
                && media.video().is_some()
            {
                media.control(crate::media::Control::Play);
            }
        }
        if let Some(index) = session
            .active
            .and_then(|active| self.open_files.iter().position(|file| file.path == active))
        {
            self.select_tab(index);
        }
        self.focus = if self.open_files.is_empty() {
            Focus::Explorer
        } else {
            Focus::Editor
        };
        self.show_explorer = session.explorer;
        self.show_bottom_terminal = session.bottom_terminal;
        self.show_right_terminal = session.right_terminal;
        self.explorer_width = session.explorer_width.max(10);
        self.right_terminal_width = session.right_terminal_width.max(10);
        self.bottom_terminal_height = session.bottom_terminal_height.max(3);
        self.status = "Ready".into();
        self.saved_session = Some(self.session());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_opens_with_its_folders_tabs_cursors_and_panels_as_it_was_left() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        fs::create_dir_all(project.join("src/deep")).unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {\n    run();\n}\n").unwrap();
        fs::write(project.join("src/deep/gone.txt"), "soon deleted\n").unwrap();
        fs::write(project.join("README.md"), "# Read me\n").unwrap();
        image::RgbaImage::new(2, 2)
            .save(project.join("dot.png"))
            .unwrap();
        let sessions = directory.path().join("settings/sessions.toml");
        // Another folder's session in the file stays when this one is written.
        fs::create_dir_all(sessions.parent().unwrap()).unwrap();
        let other = BTreeMap::from([("/elsewhere".to_owned(), Session::default())]);
        fs::write(&sessions, toml::to_string(&other).unwrap()).unwrap();

        let mut app = App::new(project.clone()).unwrap();
        app.sessions = Some(sessions.clone());
        for folder in ["src", "src/deep"] {
            let index = app
                .entries
                .iter()
                .position(|entry| entry.path == project.join(folder))
                .unwrap();
            app.selected_entry = index;
            app.activate_selected();
        }
        app.open_file(project.join("src/deep/gone.txt")).unwrap();
        app.open_file(project.join("src/main.rs")).unwrap();
        app.open_files[1].move_cursor_to(1, 4);
        app.open_file(project.join("README.md")).unwrap();
        // A picture or a video has no lines: its saved cursor must not panic (E34).
        app.open_file(project.join("dot.png")).unwrap();
        app.select_tab(1);
        app.show_bottom_terminal = true;
        app.explorer_width = 44;
        app.save_session();
        let written = fs::read_to_string(&sessions).unwrap();
        app.save_session();
        assert_eq!(fs::read_to_string(&sessions).unwrap(), written, "unchanged");
        assert!(load(&sessions).contains_key("/elsewhere"));

        fs::remove_file(project.join("src/deep/gone.txt")).unwrap();
        let mut again = App::new(project.clone()).unwrap();
        again.sessions = Some(sessions);
        again.restore_session().unwrap();
        let expanded = again
            .entries
            .iter()
            .filter(|entry| entry.expanded)
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(expanded, [project.join("src"), project.join("src/deep")]);
        let tabs = again
            .open_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            tabs,
            ["main.rs", "README.md", "dot.png"],
            "the deleted file is skipped"
        );
        let active = again.active_file().unwrap();
        assert_eq!(active.name, "main.rs");
        assert_eq!((active.cursor_row, active.cursor_column), (1, 4));
        assert_eq!(again.focus, Focus::Editor);
        assert!(again.show_bottom_terminal && again.show_explorer);
        assert_eq!(again.explorer_width, 44);

        // A folder never opened before opens as always.
        let mut fresh = App::new(directory.path().join("settings")).unwrap();
        fresh.sessions = again.sessions.clone();
        fresh.restore_session().unwrap();
        assert!(fresh.open_files.is_empty() && fresh.focus == Focus::Explorer);
    }
}
