//! Git of the open file (TODO 4.1.6): who changed each line, on request, in the margin and on
//! the status bar, and the commits that changed it, in the Inspect tab of Source Control.

use super::App;

impl App {
    /// `Alt+G`: the blame of the file on, or off.
    pub(super) fn toggle_blame(&mut self, active: usize) {
        let path = self.open_files[active].path.clone();
        if self.blame.as_ref().is_some_and(|(shown, _)| *shown == path) {
            self.blame = None;
            return;
        }
        // Saved first: the blame is of the file on disk.
        self.flush_active_file();
        let Ok(mut panel) = self.git.panel.try_lock() else {
            self.status = "Source Control is busy: try again in a moment".into();
            return;
        };
        match panel.blame(&path) {
            Ok(lines) => {
                self.status = format!("Blame of {}: Alt+G hides it", self.open_files[active].name);
                self.blame = Some((path, lines));
            }
            Err(error) => self.status = format!("No blame: {error}"),
        }
    }

    /// `Alt+H`: the commits that changed the file, in Source Control.
    pub(super) fn file_history(&mut self, active: usize) {
        let path = self.open_files[active].path.clone();
        let relative = path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf();
        self.show_git = true;
        self.git.action(crate::git::controller::GitUiAction::Run(
            crate::git::GitAction::FileHistory(relative),
        ));
    }

    /// The blame of the line under the cursor, for the status bar.
    pub(crate) fn blame_of_cursor(&self) -> Option<String> {
        let (path, lines) = self.blame.as_ref()?;
        let file = self.open_files.get(self.active_tab?)?;
        if file.path != *path {
            return None;
        }
        let line = lines.get(file.cursor_row)?;
        Some(format!(" {} {} {} ", line.commit, line.author, line.date))
    }
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::process::Command;

    #[test]
    fn alt_b_shows_who_changed_each_line_and_alt_h_the_history_in_source_control() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .current_dir(&root)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "{args:?}"
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Blame Test"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "first"]);
        let mut app = App::new(root.clone()).unwrap();
        app.open_file(root.join("a.txt")).unwrap();
        let alt = |app: &mut App, character: char| {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::ALT));
        };
        alt(&mut app, 'g');
        let (path, lines) = app
            .blame
            .clone()
            .unwrap_or_else(|| panic!("{}", app.status));
        assert_eq!((path, lines.len()), (root.join("a.txt"), 2));
        assert!(app.blame_of_cursor().unwrap().contains("Blame Test"));
        // In the margin of each line (the status bar has it when there is room).
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 24)).unwrap();
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            text.matches("Blame Test").count() >= 2,
            "both lines\n{text}"
        );
        alt(&mut app, 'g');
        assert!(app.blame.is_none(), "Alt+G again hides it");
        alt(&mut app, 'h');
        assert!(app.show_git);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.git.operation && std::time::Instant::now() < deadline {
            app.tick();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let panel = app.git.panel.lock().unwrap();
        let (path, history) = panel.file_history.clone().unwrap();
        assert_eq!(
            (path.as_path(), history.len()),
            (std::path::Path::new("a.txt"), 1)
        );
        assert_eq!(history[0].summary, "first");
        assert_eq!(panel.active_tab, crate::git::GitTab::Inspect);
    }
}
