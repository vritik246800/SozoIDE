//! The requests of TODO 3.1.1–3.1.6: rename (after `prepareRename` when the server has it),
//! formatting, code actions, the signature at the cursor, symbols of a file and of the project,
//! and the answer to a server's `workspace/applyEdit`.

use std::path::Path;

use serde_json::{Value, json};

use super::{Lsp, Pending, provides, server_for, uri, utf16_column};

impl Lsp {
    /// Sends `method` with `params` to the server of `path`, or says why it cannot.
    fn send(
        &mut self,
        path: &Path,
        method: &str,
        params: Value,
        pending: Pending,
        provider: (&str, &str),
    ) -> Result<(), String> {
        let Some((name, ..)) = server_for(path) else {
            return Err("No language server for this file".into());
        };
        if let Some(failure) = self.failed.get(name) {
            return Err(failure.message(name));
        }
        if !self.enabled {
            let row = params["position"]["line"].as_u64().unwrap_or(0) as usize;
            let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
            self.asked
                .push((method.to_owned(), path.to_path_buf(), row, character));
            return Ok(());
        }
        if !self.documents.contains_key(path) {
            return Err(format!("{name} is not running"));
        }
        let client = match self.client(path) {
            Ok(Some(client)) => client,
            Ok(None) => return Err(format!("{name} is not running")),
            Err(message) => return Err(message),
        };
        if client.ready && !provides(&client.capabilities, provider.0) {
            return Err(format!("{name} does not give {}", provider.1));
        }
        client.request(method, params, pending);
        Ok(())
    }

    fn position(path: &Path, row: usize, column: usize, line: &str) -> Value {
        json!({
            "textDocument": {"uri": uri::from_path(path)},
            "position": {"line": row, "character": utf16_column(line, column)},
        })
    }

    /// F2: whether the symbol at the cursor can be renamed (`prepareRename`, when the server
    /// has it), then the box; without it the box opens at once.
    pub fn prepare_rename(
        &mut self,
        path: &Path,
        version: u64,
        row: usize,
        column: usize,
        line: &str,
    ) -> Result<bool, String> {
        let prepares = server_for(path)
            .and_then(|(name, ..)| self.clients.get(name))
            .is_some_and(|client| client.capabilities["renameProvider"]["prepareProvider"] == true);
        if !prepares {
            return Ok(false);
        }
        let pending = Pending::PrepareRename {
            path: path.to_path_buf(),
            version,
        };
        let params = Self::position(path, row, column, line);
        self.send(
            path,
            "textDocument/prepareRename",
            params,
            pending,
            ("renameProvider", "renaming"),
        )?;
        Ok(true)
    }

    pub fn rename(
        &mut self,
        path: &Path,
        version: u64,
        row: usize,
        column: usize,
        line: &str,
        name: &str,
    ) -> Result<(), String> {
        let mut params = Self::position(path, row, column, line);
        params["newName"] = json!(name);
        let pending = Pending::Edit {
            path: path.to_path_buf(),
            version,
            goal: super::EditGoal::Rename,
        };
        self.send(
            path,
            "textDocument/rename",
            params,
            pending,
            ("renameProvider", "renaming"),
        )
    }

    /// The whole document, or the rows `range` (both included), formatted with the file's
    /// indentation.
    pub fn format(
        &mut self,
        path: &Path,
        version: u64,
        tab_size: usize,
        spaces: bool,
        range: Option<(usize, usize)>,
    ) -> Result<(), String> {
        let mut params = json!({
            "textDocument": {"uri": uri::from_path(path)},
            "options": {"tabSize": tab_size, "insertSpaces": spaces},
        });
        let pending = Pending::Edit {
            path: path.to_path_buf(),
            version,
            goal: super::EditGoal::Format,
        };
        match range {
            Some((first, last)) => {
                params["range"] = json!({"start": {"line": first, "character": 0}, "end": {"line": last + 1, "character": 0}});
                self.send(
                    path,
                    "textDocument/rangeFormatting",
                    params,
                    pending,
                    (
                        "documentRangeFormattingProvider",
                        "formatting of a selection",
                    ),
                )
            }
            None => self.send(
                path,
                "textDocument/formatting",
                params,
                pending,
                ("documentFormattingProvider", "formatting"),
            ),
        }
    }

    /// The code actions and quick fixes of row `row`, with the problems the server found there.
    pub fn code_actions(
        &mut self,
        path: &Path,
        version: u64,
        row: usize,
        line: &str,
    ) -> Result<(), String> {
        let problems: Vec<Value> = self
            .diagnostics
            .get(path)
            .into_iter()
            .flatten()
            .filter(|problem| (problem.row..=problem.end_row).contains(&row))
            .map(|problem| json!({
                "range": {"start": {"line": problem.row, "character": problem.start}, "end": {"line": problem.end_row, "character": problem.end}},
                "message": problem.message,
                "severity": if problem.error { 1 } else { 2 },
            }))
            .collect();
        let params = json!({
            "textDocument": {"uri": uri::from_path(path)},
            "range": {"start": {"line": row, "character": 0}, "end": {"line": row, "character": utf16_column(line, line.chars().count())}},
            "context": {"diagnostics": problems},
        });
        let pending = Pending::Actions {
            path: path.to_path_buf(),
            version,
        };
        self.send(
            path,
            "textDocument/codeAction",
            params,
            pending,
            ("codeActionProvider", "code actions"),
        )
    }

    /// A code action that is a command: the server runs it and sends its edits back with
    /// `workspace/applyEdit`.
    pub fn run_command(&mut self, path: &Path, command: &Value) -> Result<(), String> {
        let params =
            json!({"command": command["command"], "arguments": command["arguments"].clone()});
        self.send(
            path,
            "workspace/executeCommand",
            params,
            Pending::Command,
            ("executeCommandProvider", "commands"),
        )
    }

    /// The signature of the call at the cursor, after `(` and `,`.
    pub fn signature(
        &mut self,
        path: &Path,
        version: u64,
        row: usize,
        column: usize,
        line: &str,
    ) -> Result<(), String> {
        let params = Self::position(path, row, column, line);
        let pending = Pending::Signature {
            path: path.to_path_buf(),
            version,
        };
        self.send(
            path,
            "textDocument/signatureHelp",
            params,
            pending,
            ("signatureHelpProvider", "signatures"),
        )
    }

    /// The symbols of `path` (`@` in the finder) or, with a query, of the project (`#`).
    pub fn symbols(&mut self, path: &Path, query: Option<&str>) -> Result<(), String> {
        match query {
            None => {
                let params = json!({"textDocument": {"uri": uri::from_path(path)}});
                let pending = Pending::Symbols {
                    path: Some(path.to_path_buf()),
                };
                self.send(
                    path,
                    "textDocument/documentSymbol",
                    params,
                    pending,
                    ("documentSymbolProvider", "symbols"),
                )
            }
            Some(query) => {
                let params = json!({"query": query});
                let pending = Pending::Symbols { path: None };
                self.send(
                    path,
                    "workspace/symbol",
                    params,
                    pending,
                    ("workspaceSymbolProvider", "symbols of the project"),
                )
            }
        }
    }

    /// What each running server takes, by `ps`, at most every 5 s (macOS and Linux; Windows
    /// shows nothing).
    pub fn measure(&mut self, now: std::time::Instant) {
        if cfg!(windows)
            || self
                .measured
                .is_some_and(|at| now.duration_since(at) < std::time::Duration::from_secs(5))
        {
            return;
        }
        self.measured = Some(now);
        self.usage.clear();
        for (name, client) in &self.clients {
            let mut command = std::process::Command::new("ps");
            command.args(["-o", "rss=,%cpu=", "-p", &client.pid().to_string()]);
            let Some(output) = crate::process::output(&mut command, crate::process::QUICK) else {
                continue;
            };
            if let Some(usage) = parse_usage(&String::from_utf8_lossy(&output.stdout)) {
                self.usage.insert(name, usage);
            }
        }
    }

    /// The answer to a server's `workspace/applyEdit`, once the user said yes or no.
    pub fn answer_apply(&mut self, server: &str, id: Value, applied: bool) {
        if let Some(client) = self.clients.get_mut(server) {
            client.answer(id, json!({"applied": applied}));
        }
    }
}

/// `ps -o rss=,%cpu=`: KiB and percent; none for anything else.
pub fn parse_usage(text: &str) -> Option<(u64, f32)> {
    let mut parts = text.split_whitespace();
    let memory = parts.next()?.parse().ok()?;
    let cpu = parts.next()?.replace(',', ".").parse().ok()?;
    Some((memory, cpu))
}

/// How the Help says what a server takes.
pub fn usage_label(memory: u64, cpu: f32) -> String {
    format!("{} MB, {cpu:.0}% CPU", memory / 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_server_takes_is_read_from_ps_and_nonsense_is_ignored() {
        assert_eq!(parse_usage("  204800  3.5\n"), Some((204_800, 3.5)));
        assert_eq!(parse_usage("1024 0,7"), Some((1024, 0.7)));
        assert_eq!(parse_usage(""), None);
        assert_eq!(parse_usage("ps: no such process"), None);
        assert_eq!(usage_label(204_800, 3.49), "200 MB, 3% CPU");
    }
}
