//! IntelliSense through language servers (F26): diagnostics, completion and go to definition.
//! A server starts the first time a file of its language opens, in its own process; the app
//! only writes to it and reads what a thread already received, so typing never waits for it.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

pub(crate) mod client;

use client::{Client, Pending};

/// The servers SozoIDE talks to: name (the command), languages by extension and arguments.
const SERVERS: [(&str, &[&str], &[&str]); 6] = [
    ("rust-analyzer", &["rs"], &[]),
    (
        "clangd",
        &["c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        &[],
    ),
    ("pyright-langserver", &["py", "pyi"], &["--stdio"]),
    ("jdtls", &["java"], &[]),
    (
        "typescript-language-server",
        &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"],
        &["--stdio"],
    ),
    ("gopls", &["go"], &[]),
];

/// The server for `path` and the language id the protocol calls it by.
pub fn server_for(path: &Path) -> Option<(&'static str, &'static [&'static str], &'static str)> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let (name, _, arguments) = SERVERS
        .iter()
        .find(|(_, extensions, _)| extensions.contains(&extension.as_str()))?;
    let language = match extension.as_str() {
        "rs" => "rust",
        "c" | "h" => "c",
        "py" | "pyi" => "python",
        "java" => "java",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "go" => "go",
        _ => "cpp",
    };
    Some((name, arguments, language))
}

/// A problem a server found: where (rows and UTF-16 columns, as the protocol counts) and what.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub row: usize,
    pub start: usize,
    pub end_row: usize,
    pub end: usize,
    pub error: bool,
    pub message: String,
}

/// One suggestion: what the list shows and what replaces the word being typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub detail: String,
    pub insert: String,
}

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Suggestions for the cursor at `row` and `column` (characters) of `path`.
    Completion {
        path: PathBuf,
        row: usize,
        column: usize,
        items: Vec<Item>,
    },
    /// Where a symbol is defined: a row and a UTF-16 column.
    Definition {
        path: PathBuf,
        row: usize,
        column: usize,
    },
    Message(String),
}

pub struct Lsp {
    root: PathBuf,
    /// Off in the tests, which must not start the servers of the machine they run on.
    enabled: bool,
    clients: HashMap<&'static str, Client>,
    /// Servers that could not start: said once, not tried again.
    failed: HashSet<&'static str>,
    /// The open documents, with the edit count last sent.
    documents: HashMap<PathBuf, u64>,
    pub diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
}

impl Lsp {
    pub fn new(root: PathBuf, enabled: bool) -> Self {
        Self {
            root,
            enabled,
            clients: HashMap::new(),
            failed: HashSet::new(),
            documents: HashMap::new(),
            diagnostics: HashMap::new(),
        }
    }

    /// The server of `path`, started now if it is not running; none for other languages.
    fn client(&mut self, path: &Path) -> Result<Option<&mut Client>, String> {
        let Some((name, arguments, _)) = server_for(path) else {
            return Ok(None);
        };
        if !self.enabled || self.failed.contains(name) {
            return Ok(None);
        }
        if !self.clients.contains_key(name) {
            let Some(command) = crate::update::deps::server_command(name, arguments) else {
                self.failed.insert(name);
                return Err(format!(
                    "Install {name} for IntelliSense: SozoIDE installs it by itself when it can"
                ));
            };
            match Client::start(&command, &self.root) {
                Ok(client) => {
                    self.clients.insert(name, client);
                }
                Err(error) => {
                    self.failed.insert(name);
                    return Err(format!("{error:#}"));
                }
            }
        }
        Ok(self.clients.get_mut(name))
    }

    /// A file opened in a tab: its server learns its text (and starts if it must).
    pub fn open(&mut self, path: &Path, text: &str, edits: u64) -> Option<String> {
        let language = server_for(path)?.2;
        let client = match self.client(path) {
            Ok(Some(client)) => client,
            Ok(None) => return None,
            Err(message) => return Some(message),
        };
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri::from_path(path),
                "languageId": language,
                "version": edits,
                "text": text,
            }}),
        );
        self.documents.insert(path.to_path_buf(), edits);
        None
    }

    /// Whether the server of `path` has not seen its last `edits` yet.
    pub fn behind(&self, path: &Path, edits: u64) -> bool {
        self.documents.get(path).is_some_and(|sent| *sent != edits)
    }

    /// The whole text again, after edits (the app sends it at most a few times a second).
    pub fn change(&mut self, path: &Path, text: &str, edits: u64) {
        if !self.documents.contains_key(path) {
            return;
        }
        if let Ok(Some(client)) = self.client(path) {
            client.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri::from_path(path), "version": edits},
                    "contentChanges": [{"text": text}],
                }),
            );
        }
        self.documents.insert(path.to_path_buf(), edits);
    }

    pub fn close(&mut self, path: &Path) {
        if self.documents.remove(path).is_none() {
            return;
        }
        self.diagnostics.remove(path);
        if let Ok(Some(client)) = self.client(path) {
            client.notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uri::from_path(path)}}),
            );
        }
    }

    /// Asks for suggestions at the cursor (`column` in characters of `line`).
    pub fn complete(&mut self, path: &Path, row: usize, column: usize, line: &str) {
        self.ask(path, "textDocument/completion", row, column, line, |path| {
            Pending::Completion { path, row, column }
        });
    }

    /// Asks where the symbol at the cursor is defined.
    pub fn definition(&mut self, path: &Path, row: usize, column: usize, line: &str) {
        self.ask(path, "textDocument/definition", row, column, line, |_| {
            Pending::Definition
        });
    }

    fn ask(
        &mut self,
        path: &Path,
        method: &str,
        row: usize,
        column: usize,
        line: &str,
        pending: impl FnOnce(PathBuf) -> Pending,
    ) {
        if !self.documents.contains_key(path) {
            return;
        }
        if let Ok(Some(client)) = self.client(path) {
            let params = json!({
                "textDocument": {"uri": uri::from_path(path)},
                "position": {"line": row, "character": utf16_column(line, column)},
            });
            client.request(method, params, pending(path.to_path_buf()));
        }
    }

    /// What the servers said since the last call; diagnostics go straight to `diagnostics`.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut stopped = Vec::new();
        for (name, client) in &mut self.clients {
            for message in client.received() {
                handle(client, message, &mut self.diagnostics, &mut events);
            }
            if !client.alive() {
                stopped.push(*name);
            }
        }
        for name in stopped {
            self.clients.remove(name);
            self.failed.insert(name);
            events.push(Event::Message(format!("{name} stopped")));
        }
        events
    }
}

fn handle(
    client: &mut Client,
    message: Value,
    diagnostics: &mut HashMap<PathBuf, Vec<Diagnostic>>,
    events: &mut Vec<Event>,
) {
    let method = message["method"].as_str();
    match (message.get("id").cloned(), method) {
        // A request of the server: configuration gets one empty answer per item asked for.
        (Some(id), Some(method)) => {
            let result = if method == "workspace/configuration" {
                let count = message["params"]["items"].as_array().map_or(0, Vec::len);
                Value::Array(vec![Value::Null; count])
            } else {
                Value::Null
            };
            client.answer(id, result);
        }
        (Some(id), None) => {
            let Some(pending) = id.as_i64().and_then(|id| client.pending.remove(&id)) else {
                return;
            };
            let result = &message["result"];
            match pending {
                Pending::Initialize => client.initialized(),
                Pending::Completion { path, row, column } => events.push(Event::Completion {
                    path,
                    row,
                    column,
                    items: completion_items(result),
                }),
                Pending::Definition => {
                    if let Some(event) = definition(result) {
                        events.push(event);
                    }
                }
            }
        }
        (None, Some("textDocument/publishDiagnostics")) => {
            let params = &message["params"];
            if let Some(path) = params["uri"].as_str().and_then(uri::to_path) {
                diagnostics.insert(path, parse_diagnostics(&params["diagnostics"]));
            }
        }
        (None, Some("window/showMessage")) if message["params"]["type"] == 1 => {
            if let Some(text) = message["params"]["message"].as_str() {
                events.push(Event::Message(text.to_owned()));
            }
        }
        _ => {}
    }
}

fn position(value: &Value) -> (usize, usize) {
    let number = |key: &str| value[key].as_u64().unwrap_or(0) as usize;
    (number("line"), number("character"))
}

fn parse_diagnostics(list: &Value) -> Vec<Diagnostic> {
    let mut diagnostics = list
        .as_array()
        .into_iter()
        .flatten()
        .map(|diagnostic| {
            let (row, start) = position(&diagnostic["range"]["start"]);
            let (end_row, end) = position(&diagnostic["range"]["end"]);
            Diagnostic {
                row,
                start,
                end_row,
                end,
                // No severity counts as an error, as the protocol says.
                error: diagnostic["severity"]
                    .as_u64()
                    .is_none_or(|severity| severity == 1),
                message: diagnostic["message"]
                    .as_str()
                    .unwrap_or_default()
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            }
        })
        .collect::<Vec<_>>();
    // Errors before warnings on the same row: the footer shows the first one.
    diagnostics.sort_by_key(|diagnostic| (diagnostic.row, !diagnostic.error));
    diagnostics
}

/// At most this many suggestions are kept: the list shows a few, typing narrows it.
const MOST_ITEMS: usize = 200;

fn completion_items(result: &Value) -> Vec<Item> {
    let list = result
        .as_array()
        .or_else(|| result["items"].as_array())
        .cloned()
        .unwrap_or_default();
    let mut items = list
        .iter()
        .map(|item| {
            let label = item["label"].as_str().unwrap_or_default().trim().to_owned();
            let edit = &item["textEdit"]["newText"];
            let insert = edit
                .as_str()
                .or_else(|| item["insertText"].as_str())
                .unwrap_or(&label)
                .to_owned();
            let sort = item["sortText"].as_str().unwrap_or(&label).to_owned();
            let detail = item["detail"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_owned();
            (
                sort,
                Item {
                    label,
                    detail,
                    insert,
                },
            )
        })
        .filter(|(_, item)| !item.label.is_empty())
        .collect::<Vec<_>>();
    items.sort_by(|(left, _), (right, _)| left.cmp(right));
    items
        .into_iter()
        .take(MOST_ITEMS)
        .map(|(_, item)| item)
        .collect()
}

fn definition(result: &Value) -> Option<Event> {
    let location = result
        .as_array()
        .and_then(|list| list.first())
        .unwrap_or(result);
    let uri = location["uri"]
        .as_str()
        .or_else(|| location["targetUri"].as_str())?;
    let range = if location.get("targetSelectionRange").is_some() {
        &location["targetSelectionRange"]
    } else {
        &location["range"]
    };
    let (row, column) = position(&range["start"]);
    Some(Event::Definition {
        path: uri::to_path(uri)?,
        row,
        column,
    })
}

/// The UTF-16 units before character `column` of `line` (the protocol's columns).
pub fn utf16_column(line: &str, column: usize) -> usize {
    line.chars().take(column).map(char::len_utf16).sum()
}

/// The character at UTF-16 unit `units` of `line`.
pub fn char_column(line: &str, units: usize) -> usize {
    let mut counted = 0;
    line.chars()
        .take_while(|character| {
            counted += character.len_utf16();
            counted <= units
        })
        .count()
}

/// `file://` addresses: percent-encoded paths, `/C:/…` on Windows.
pub mod uri {
    use std::path::{Path, PathBuf};

    pub fn from_path(path: &Path) -> String {
        let text = path.display().to_string().replace('\\', "/");
        let text = if text.starts_with('/') {
            text
        } else {
            format!("/{text}")
        };
        let mut uri = String::from("file://");
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
                uri.push(byte as char);
            } else {
                uri.push_str(&format!("%{byte:02X}"));
            }
        }
        uri
    }

    pub fn to_path(uri: &str) -> Option<PathBuf> {
        let encoded = uri.strip_prefix("file://")?.as_bytes();
        let mut bytes = Vec::with_capacity(encoded.len());
        let mut index = 0;
        while index < encoded.len() {
            if encoded[index] == b'%'
                && let Some(byte) = encoded
                    .get(index + 1..index + 3)
                    .and_then(|hex| std::str::from_utf8(hex).ok())
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                bytes.push(byte);
                index += 3;
            } else {
                bytes.push(encoded[index]);
                index += 1;
            }
        }
        let text = String::from_utf8(bytes).ok()?;
        // `/C:/x` is `C:/x` on Windows.
        let text = match text.as_bytes() {
            [b'/', _, b':', ..] if cfg!(windows) => text[1..].to_owned(),
            _ => text,
        };
        Some(PathBuf::from(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_paths_and_columns_speak_the_protocol() {
        assert_eq!(server_for(Path::new("main.rs")).unwrap().0, "rust-analyzer");
        assert_eq!(server_for(Path::new("a.tsx")).unwrap().2, "typescriptreact");
        assert_eq!(server_for(Path::new("a.hpp")).unwrap().2, "cpp");
        assert!(server_for(Path::new("notes.md")).is_none());

        let path = Path::new("/GIT (Production)/só/main.rs");
        let address = uri::from_path(path);
        assert_eq!(address, "file:///GIT%20%28Production%29/s%C3%B3/main.rs");
        assert_eq!(uri::to_path(&address).unwrap(), path);

        // «é» is one unit, an emoji two.
        let line = "é😀x";
        assert_eq!(utf16_column(line, 2), 3);
        assert_eq!(char_column(line, 3), 2);
        assert_eq!(char_column(line, 99), 3);
    }

    #[test]
    fn answers_become_suggestions_places_and_problems() {
        let items = completion_items(&json!({"items": [
            {"label": "zeta", "sortText": "2"},
            {"label": " push", "detail": "fn(&mut self, T)", "sortText": "1",
             "textEdit": {"newText": "push", "range": {}}},
            {"label": "len", "insertText": "len()", "sortText": "3"},
        ]}));
        let labels = items
            .iter()
            .map(|item| (item.label.as_str(), item.insert.as_str()));
        assert_eq!(
            labels.collect::<Vec<_>>(),
            [("push", "push"), ("zeta", "zeta"), ("len", "len()")]
        );
        assert_eq!(items[0].detail, "fn(&mut self, T)");

        let place = definition(&json!([{
            "targetUri": "file:///src/lib.rs",
            "targetRange": {"start": {"line": 1, "character": 0}},
            "targetSelectionRange": {"start": {"line": 4, "character": 7}},
        }]));
        assert_eq!(
            place,
            Some(Event::Definition {
                path: PathBuf::from("/src/lib.rs"),
                row: 4,
                column: 7
            })
        );
        assert_eq!(definition(&Value::Null), None);

        let problems = parse_diagnostics(&json!([
            {"range": {"start": {"line": 3, "character": 1}, "end": {"line": 3, "character": 4}},
             "severity": 2, "message": "unused\nmore"},
            {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 2}},
             "message": "expected `;`"},
        ]));
        assert_eq!(problems[0].message, "expected `;`");
        assert!(problems[0].error && !problems[1].error);
        assert_eq!(problems[1].message, "unused", "the first line only");
    }
}
