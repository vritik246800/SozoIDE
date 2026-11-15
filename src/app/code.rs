//! Code Intelligence beyond places (F31, TODO 3.2.1): rename, format, code actions and quick
//! fixes, the signature while typing a call, and the edits of a server shown before they touch
//! more than the file in front of you. Late answers (an older version of the text) are dropped.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use super::App;
use crate::lsp::{CodeAction, EditGoal, FileEdit, apply_edits};

/// The signature at the cursor: its file, its label and the columns of its active parameter.
pub type Signature = (PathBuf, String, Option<(usize, usize)>);

/// The box of a rename: the symbol's row and column and the new name being typed.
pub struct RenameBox {
    pub path: PathBuf,
    pub version: u64,
    pub row: usize,
    pub column: usize,
    pub name: String,
}

/// Code actions of a row, to choose one.
pub struct ActionMenu {
    pub path: PathBuf,
    pub actions: Vec<CodeAction>,
    pub selected: usize,
}

/// Edits of a server that change more than one file (or that the server asked to apply):
/// they wait for `Enter`, and what each file gets is listed first.
pub struct EditPreview {
    pub title: String,
    pub files: Vec<FileEdit>,
    /// The server and the id to answer, for a `workspace/applyEdit`.
    pub answer: Option<(&'static str, Value)>,
}

impl App {
    fn word_at_cursor(&self) -> Option<(usize, usize, String)> {
        let file = self.active_file()?;
        let line: Vec<char> = file.lines.get(file.cursor_row)?.chars().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = file.cursor_column.min(line.len());
        while start > 0 && word(line[start - 1]) {
            start -= 1;
        }
        let end = (start..line.len())
            .find(|&at| !word(line[at]))
            .unwrap_or(line.len());
        (end > start).then(|| (file.cursor_row, start, line[start..end].iter().collect()))
    }

    /// F2 in the editor: the rename box, after the server says it can (`prepareRename`).
    pub(super) fn start_rename(&mut self) {
        let Some(index) = self.active_tab else { return };
        if !self.trusted(false) {
            return;
        }
        let Some((row, column, word)) = self.word_at_cursor() else {
            self.status = "Put the cursor on a symbol to rename it".into();
            return;
        };
        let file = &self.open_files[index];
        let (path, version, line) = (file.path.clone(), file.edits, file.lines[row].clone());
        self.rename = Some(RenameBox {
            path: path.clone(),
            version,
            row,
            column,
            name: word,
        });
        match self.lsp.prepare_rename(&path, version, row, column, &line) {
            Ok(_) => {}
            Err(message) => {
                self.rename = None;
                self.status = message;
            }
        }
    }

    /// The keys of the rename box: typing, Backspace, Enter sends, Esc closes.
    pub(super) fn handle_rename_key(&mut self, key: KeyEvent) {
        let Some(rename) = &mut self.rename else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.rename = None,
            KeyCode::Backspace => {
                rename.name.pop();
            }
            KeyCode::Char(character) if !character.is_control() && rename.name.len() < 200 => {
                rename.name.push(character);
            }
            KeyCode::Enter => {
                let Some(rename) = self.rename.take() else {
                    return;
                };
                if rename.name.trim().is_empty() {
                    return;
                }
                let line = self
                    .open_files
                    .iter()
                    .find(|file| file.path == rename.path)
                    .and_then(|file| file.lines.get(rename.row).cloned())
                    .unwrap_or_default();
                if let Err(message) = self.lsp.rename(
                    &rename.path,
                    rename.version,
                    rename.row,
                    rename.column,
                    &line,
                    &rename.name,
                ) {
                    self.status = message;
                }
            }
            _ => {}
        }
    }

    /// Shift+Alt+F: the selection's rows, or the whole file, formatted by the server.
    pub(super) fn format_file(&mut self) {
        if self.active_file().is_none() || !self.trusted(false) {
            return;
        }
        let Some(file) = self.active_file() else {
            return;
        };
        let range = file.selection().map(|((start, _), (end, _))| (start, end));
        let (path, version, indent) = (file.path.clone(), file.edits, file.indent);
        if let Err(message) = self
            .lsp
            .format(&path, version, indent.width, !indent.tabs, range)
        {
            self.status = message;
        }
    }

    /// After a save, with `format_on_save` (F42, TODO 3.1.2): the server formats the file; its
    /// edits come back as one Ctrl+Z and the next autosave keeps them. Only in a trusted folder.
    pub(super) fn format_after_save(&mut self, index: usize) {
        self.format_saved(index, crate::settings::get().format_on_save);
    }

    pub(super) fn format_saved(&mut self, index: usize, wanted: bool) {
        if !wanted || self.trust != Some(true) {
            return;
        }
        let Some(file) = self.open_files.get(index) else {
            return;
        };
        let (path, version, indent) = (file.path.clone(), file.edits, file.indent);
        let _ = self
            .lsp
            .format(&path, version, indent.width, !indent.tabs, None);
    }

    /// Ctrl+.: the code actions and quick fixes of the cursor's row.
    pub(super) fn ask_code_actions(&mut self) {
        if self.active_file().is_none() || !self.trusted(false) {
            return;
        }
        let Some(file) = self.active_file() else {
            return;
        };
        let (path, version, row) = (file.path.clone(), file.edits, file.cursor_row);
        let line = file.lines[row].clone();
        if let Err(message) = self.lsp.code_actions(&path, version, row, &line) {
            self.status = message;
        }
    }

    /// The keys of the code actions menu.
    pub(super) fn handle_action_key(&mut self, key: KeyEvent) {
        let Some(menu) = &mut self.action_menu else {
            return;
        };
        let count = menu.actions.len();
        match key.code {
            KeyCode::Esc => self.action_menu = None,
            KeyCode::Up => menu.selected = menu.selected.checked_sub(1).unwrap_or(count - 1),
            KeyCode::Down => menu.selected = (menu.selected + 1) % count,
            KeyCode::Enter => {
                let Some(menu) = self.action_menu.take() else {
                    return;
                };
                let action = menu.actions[menu.selected].clone();
                if let Some(edit) = &action.edit {
                    match crate::lsp::workspace_edit(edit, &self.root) {
                        Ok(files) => self.receive_edits(&action.title, files, None),
                        Err(message) => self.status = format!("Not applied: {message}"),
                    }
                }
                if let Some(command) = &action.command
                    && let Err(message) = self.lsp.run_command(&menu.path, command)
                {
                    self.status = message;
                }
            }
            _ => {}
        }
    }

    /// After `(` and `,` the signature of the call; `)` closes it.
    pub(super) fn typed_signature(&mut self, character: char) {
        match character {
            '(' | ',' => {
                let Some(file) = self.active_file() else {
                    return;
                };
                let (path, version, row, column) = (
                    file.path.clone(),
                    file.edits,
                    file.cursor_row,
                    file.cursor_column,
                );
                let line = file.lines[row].clone();
                let _ = self.lsp.signature(&path, version, row, column, &line);
            }
            ')' => self.signature = None,
            _ => {}
        }
    }

    /// Edits that arrived: one file open in the editor applies at once (one Ctrl+Z); more than
    /// one file, or a file not open, waits for Enter in a preview.
    pub(super) fn receive_edits(
        &mut self,
        title: &str,
        files: Vec<FileEdit>,
        answer: Option<(&'static str, Value)>,
    ) {
        let open_one = files.len() == 1
            && answer.is_none()
            && self
                .open_files
                .iter()
                .any(|file| file.path == files[0].path);
        if files.iter().all(|file| file.edits.is_empty()) {
            self.status = format!("{title}: nothing to change");
            if let Some((server, id)) = answer {
                self.lsp.answer_apply(server, id, true);
            }
            return;
        }
        if open_one {
            let count = files[0].edits.len();
            match self.apply_file_edits(&files) {
                Ok(()) => {
                    self.status = format!(
                        "{title}: {count} change{}",
                        if count == 1 { "" } else { "s" }
                    )
                }
                Err(message) => self.status = format!("Not applied: {message}"),
            }
            return;
        }
        self.edit_preview = Some(EditPreview {
            title: title.to_owned(),
            files,
            answer,
        });
    }

    /// The keys of the preview: Enter applies everything, Esc nothing.
    pub(super) fn handle_preview_key(&mut self, key: KeyEvent) {
        let Some(preview) = self.edit_preview.take() else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                let result = self.apply_file_edits(&preview.files);
                if let Some((server, id)) = preview.answer {
                    self.lsp.answer_apply(server, id, result.is_ok());
                }
                self.status = match result {
                    Ok(()) => format!("{}: {} files changed", preview.title, preview.files.len()),
                    Err(message) => format!("Not applied: {message}"),
                };
            }
            KeyCode::Esc => {
                if let Some((server, id)) = preview.answer {
                    self.lsp.answer_apply(server, id, false);
                }
                self.status = format!("{}: not applied", preview.title);
            }
            _ => self.edit_preview = Some(preview),
        }
    }

    /// Applies every file's edits, all or none: each one is worked out first, and only when
    /// every file takes its edits is anything changed (open tabs as one Ctrl+Z each, the
    /// others written).
    pub(super) fn apply_file_edits(&mut self, files: &[FileEdit]) -> Result<(), String> {
        let mut results: Vec<(PathBuf, Vec<String>)> = Vec::new();
        for file in files {
            let lines = match self.open_files.iter().find(|open| open.path == file.path) {
                Some(open) if open.read_only => return Err(format!("{} is read only", open.name)),
                Some(open) => open.lines.clone(),
                None => text_lines(&file.path)?,
            };
            let changed = apply_edits(&lines, &file.edits)
                .map_err(|error| format!("{}: {error}", file.path.display()))?;
            results.push((file.path.clone(), changed));
        }
        for (path, lines) in results {
            match self.open_files.iter_mut().find(|open| open.path == path) {
                Some(open) => open.replace_text(lines),
                None => write_lines(&path, &lines)?,
            }
        }
        Ok(())
    }

    /// The events of TODO 3.1.1–3.1.5 from the servers.
    pub(super) fn code_event(&mut self, event: crate::lsp::Event) {
        use crate::lsp::Event;
        let current = |app: &App, path: &Path, version: u64| {
            app.open_files
                .iter()
                .any(|file| file.path == path && file.edits == version)
        };
        match event {
            Event::RenameReady {
                path,
                version,
                result,
            } => match result {
                _ if !current(self, &path, version) => self.rename = None,
                Ok(Some(placeholder)) => {
                    if let Some(rename) = &mut self.rename {
                        rename.name = placeholder;
                    }
                }
                Ok(None) => {}
                Err(message) => {
                    self.rename = None;
                    self.status = message;
                }
            },
            Event::Edits {
                path,
                version,
                goal,
                result,
            } => {
                // An answer for an older text is dropped: it would land in the wrong places.
                if !current(self, &path, version) {
                    self.status = "The file changed: ask again".into();
                    return;
                }
                let title = match goal {
                    EditGoal::Rename => "Rename",
                    EditGoal::Format => "Format",
                };
                match result {
                    Ok(files) => self.receive_edits(title, files, None),
                    Err(message) => self.status = format!("{title} not applied: {message}"),
                }
            }
            Event::Actions {
                path,
                version,
                actions,
            } => {
                if !current(self, &path, version) {
                    return;
                }
                if actions.is_empty() {
                    self.status = "No code actions here".into();
                } else {
                    self.action_menu = Some(ActionMenu {
                        path,
                        actions,
                        selected: 0,
                    });
                }
            }
            Event::Signature {
                path,
                version,
                signature,
            } => {
                self.signature = signature
                    .filter(|_| current(self, &path, version))
                    .map(|(label, active)| (path, label, active));
            }
            Event::Symbols { symbols, .. } => {
                if let Some(finder) = &mut self.finder {
                    finder.symbols = symbols;
                    finder.selected = 0;
                }
            }
            Event::ApplyEdit {
                server,
                id,
                label,
                result,
            } => match result {
                Ok(files) => self.receive_edits(&label, files, Some((server, id))),
                Err(message) => {
                    self.lsp.answer_apply(server, id, false);
                    self.status = format!("Not applied: {message}");
                }
            },
            Event::Restart(name) if self.trust == Some(true) => {
                for file in &self.open_files {
                    if file.is_text()
                        && crate::lsp::server_for(&file.path).is_some_and(|server| server.0 == name)
                    {
                        let _ = self.lsp.open(&file.path, &file.text(), file.edits);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The lines of a file that is not open, for an edit of a server: text only.
fn text_lines(path: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text =
        String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8 text", path.display()))?;
    Ok(text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect())
}

fn write_lines(path: &Path, lines: &[String]) -> Result<(), String> {
    let original = std::fs::read_to_string(path).unwrap_or_default();
    let end = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    std::fs::write(path, lines.join(end)).map_err(|error| format!("{}: {error}", path.display()))
}
