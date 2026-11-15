//! What the Database and Source Control panels ask of the app, which owns the clipboard, the
//! editor and the project folder.

use super::App;

impl App {
    pub(super) fn panel_requests(&mut self) {
        // Copy SQL in the SQL tab.
        if let Some(text) = self.database.copy_request.take() {
            self.copy(text);
        }
        // Results exported (TODO 4.1.9): to the clipboard, or to a new file in the project.
        if let Some((text, extension)) = self.database.export_request.take() {
            self.status = match extension {
                None => {
                    self.copy(text);
                    "Results copied".into()
                }
                Some(extension) => match self.export_file(&text, extension) {
                    Ok(name) => format!("Results saved to {name}"),
                    Err(error) => format!("Could not save the results: {error}"),
                },
            };
        }
        // The Diff tab asked for its file in the editor.
        if let Some(path) = self.git.open_request.take() {
            match self.open_file(path) {
                Ok(()) => self.show_git = false,
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    /// `sozo FILE:LINE:COLUMN`: the file open at its place (TODO 6.1.1).
    pub fn open_file_at(&mut self, path: std::path::PathBuf, line: usize, column: usize) {
        match self.open_file(path) {
            Ok(()) => {
                self.go_to(&format!("{line}:{column}"));
                self.focus = super::Focus::Editor;
            }
            Err(error) => self.status = format!("{error:#}"),
        }
    }

    /// A new file `results-<time>.<extension>` in the project folder; one that exists is never
    /// written over (`create_new`).
    fn export_file(&self, text: &str, extension: &str) -> std::io::Result<String> {
        use std::io::Write;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut last = None;
        for attempt in 0..100 {
            let name = match attempt {
                0 => format!("results-{stamp}.{extension}"),
                n => format!("results-{stamp}-{n}.{extension}"),
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.root.join(&name))
            {
                Ok(mut file) => {
                    file.write_all(text.as_bytes())?;
                    return Ok(name);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    last = Some(error)
                }
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
    }
}

#[cfg(test)]
mod tests {
    use crate::app::App;

    #[test]
    fn exported_results_go_to_a_new_file_and_never_over_one() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.database.export_request = Some(("a,b\r\n".into(), Some("csv")));
        app.panel_requests();
        app.database.export_request = Some(("c,d\r\n".into(), Some("csv")));
        app.panel_requests();
        let mut files: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("results-"))
            .collect();
        files.sort();
        assert_eq!(files.len(), 2, "{files:?}");
        let texts: Vec<String> = files
            .iter()
            .map(|name| std::fs::read_to_string(directory.path().join(name)).unwrap())
            .collect();
        assert!(texts.contains(&"a,b\r\n".to_owned()) && texts.contains(&"c,d\r\n".to_owned()));
        app.database.export_request = Some(("x".into(), None));
        app.panel_requests();
        assert_eq!(app.status, "Results copied");
    }

    #[test]
    fn a_file_opens_at_the_line_and_column_the_command_line_gave() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.rs"), "one\ntwo\nthree\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file_at(directory.path().join("a.rs"), 3, 2);
        let file = app.active_file().unwrap();
        assert_eq!((file.cursor_row, file.cursor_column), (2, 1));
        app.open_file_at(directory.path().join("gone.rs"), 1, 1);
        assert!(!app.status.is_empty());
    }
}
