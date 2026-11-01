//! IntelliSense through language servers (F26): diagnostics, completion, hover, definitions and
//! references (F31).
//! A server starts the first time a file of its language opens, in its own process; the app
//! only writes to it and reads what a thread already received, so typing never waits for it.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

pub(crate) mod client;

use client::{Client, Pending};

/// The servers SozoIDE talks to: name (the command), languages by extension and arguments.
const SERVERS: [(&str, &[&str], &[&str]); 11] = [
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
    ("bash-language-server", &["sh", "bash", "zsh"], &["start"]),
    ("yaml-language-server", &["yaml", "yml"], &["--stdio"]),
    ("intelephense", &["php"], &["--stdio"]),
    ("lua-language-server", &["lua"], &[]),
    // `vscode-langservers-extracted` brings one command per language; HTML stands for them.
    (
        "vscode-html-language-server",
        &["html", "htm"],
        &["--stdio"],
    ),
];

/// The servers of `vscode-langservers-extracted` besides HTML's, by extension.
const EXTRACTED: [(&str, &[&str]); 2] = [
    ("vscode-css-language-server", &["css", "scss", "less"]),
    ("vscode-json-language-server", &["json", "jsonc"]),
];

/// The server for `path` and the language id the protocol calls it by.
pub fn server_for(path: &Path) -> Option<(&'static str, &'static [&'static str], &'static str)> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let (name, _, arguments) = SERVERS
        .iter()
        .map(|(name, extensions, arguments)| (*name, *extensions, *arguments))
        .chain(
            EXTRACTED
                .iter()
                .map(|(name, extensions)| (*name, *extensions, &["--stdio"][..])),
        )
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
        "sh" | "bash" | "zsh" => "shellscript",
        "yaml" | "yml" => "yaml",
        "php" => "php",
        "lua" => "lua",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        "json" => "json",
        "jsonc" => "jsonc",
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

/// What a list of places is for: go there, show it in a box or list every use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Definition,
    Peek,
    References,
    Declaration,
    TypeDefinition,
    Implementation,
}

impl Goal {
    /// The method that asks for it.
    fn method(self) -> &'static str {
        match self {
            Self::Definition | Self::Peek => "textDocument/definition",
            Self::References => "textDocument/references",
            Self::Declaration => "textDocument/declaration",
            Self::TypeDefinition => "textDocument/typeDefinition",
            Self::Implementation => "textDocument/implementation",
        }
    }
}

/// A place in a file: rows and UTF-16 columns, as the protocol counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub path: PathBuf,
    pub row: usize,
    pub start: usize,
    pub end_row: usize,
    pub end: usize,
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
    /// What the server says about the symbol at `row` and `column` (characters) of `path` at
    /// edit `version`, in Markdown; empty when it says nothing.
    Hover {
        path: PathBuf,
        version: u64,
        row: usize,
        column: usize,
        text: String,
    },
    /// Where the symbol asked about in `path` at edit `version` is defined or used.
    Places {
        path: PathBuf,
        version: u64,
        goal: Goal,
        places: Vec<Place>,
    },
    Message(String),
}

/// Why a server is not running: it is not installed, it stopped before answering `initialize`
/// (it could not start: rustup's `rust-analyzer` without its component) or after, with the
/// last line it wrote on its errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Missing,
    Start(String),
    Stopped(String),
}

impl Failure {
    /// What the footer says about server `name`.
    pub fn message(&self, name: &str) -> String {
        let (what, why) = match self {
            Self::Missing => return format!("{name} is not installed"),
            Self::Start(why) => ("could not start", why),
            Self::Stopped(why) => ("stopped", why),
        };
        if why.is_empty() {
            format!("{name} {what}")
        } else {
            format!("{name} {what}: {why}")
        }
    }
}

pub struct Lsp {
    root: PathBuf,
    /// Off in the tests, which must not start the servers of the machine they run on.
    enabled: bool,
    clients: HashMap<&'static str, Client>,
    /// Servers that are not running and why: said once, not tried again until `restart`.
    failed: HashMap<&'static str, Failure>,
    /// The open documents, with the edit count last sent.
    documents: HashMap<PathBuf, u64>,
    pub diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    /// The requests a disabled `Lsp` (the tests) would have sent: method, file, row, column.
    pub asked: Vec<(String, PathBuf, usize, usize)>,
}

impl Lsp {
    pub fn new(root: PathBuf, enabled: bool) -> Self {
        Self {
            root,
            enabled,
            clients: HashMap::new(),
            failed: HashMap::new(),
            documents: HashMap::new(),
            diagnostics: HashMap::new(),
            asked: Vec::new(),
        }
    }

    /// The server of `path`, started now if it is not running; none for other languages.
    fn client(&mut self, path: &Path) -> Result<Option<&mut Client>, String> {
        let Some((name, arguments, _)) = server_for(path) else {
            return Ok(None);
        };
        if !self.enabled || self.failed.contains_key(name) {
            return Ok(None);
        }
        if !self.clients.contains_key(name) {
            let Some(command) = crate::update::deps::server_command(name, arguments) else {
                self.failed.insert(name, Failure::Missing);
                return Err(format!(
                    "Install {name} for IntelliSense: Help, then Dependencies"
                ));
            };
            match Client::start(&command, &self.root) {
                Ok(client) => {
                    self.clients.insert(name, client);
                }
                Err(error) => {
                    self.failed
                        .insert(name, Failure::Start(format!("{error:#}")));
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

    /// Asks for suggestions at the cursor (`column` in characters of `line`); typing asks, so
    /// nothing is said when no server can answer.
    pub fn complete(&mut self, path: &Path, row: usize, column: usize, line: &str) {
        let pending = Pending::Completion {
            path: path.to_path_buf(),
            row,
            column,
        };
        let _ = self.ask(path, "textDocument/completion", row, column, line, pending);
    }

    /// Asks what the symbol at `row` and `column` (characters of `line`) is, at edit `version`.
    pub fn hover(
        &mut self,
        path: &Path,
        version: u64,
        row: usize,
        column: usize,
        line: &str,
    ) -> Result<(), String> {
        let pending = Pending::Hover {
            path: path.to_path_buf(),
            version,
            row,
            column,
        };
        self.ask(path, "textDocument/hover", row, column, line, pending)
    }

    /// Asks where the symbol at `row` and `column` (characters of `line`) is defined, or used.
    pub fn places(
        &mut self,
        goal: Goal,
        path: &Path,
        version: u64,
        row: usize,
        column: usize,
        line: &str,
    ) -> Result<(), String> {
        let method = goal.method();
        let pending = Pending::Places {
            path: path.to_path_buf(),
            version,
            goal,
        };
        self.ask(path, method, row, column, line, pending)
    }

    /// Sends `method` for the position, or says why it cannot: no server for the language,
    /// a server that is not running, or one that does not answer it (its `capabilities`).
    fn ask(
        &mut self,
        path: &Path,
        method: &str,
        row: usize,
        column: usize,
        line: &str,
        pending: Pending,
    ) -> Result<(), String> {
        let Some((name, ..)) = server_for(path) else {
            return Err(
                match path.extension().and_then(|extension| extension.to_str()) {
                    Some(extension) => format!("No language server for .{extension} files"),
                    None => "No language server for this file".to_owned(),
                },
            );
        };
        let character = utf16_column(line, column);
        if let Some(failure) = self.failed.get(name) {
            return Err(failure.message(name));
        }
        if !self.enabled {
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
        let provider = match method {
            "textDocument/hover" => ("hoverProvider", "information on symbols"),
            "textDocument/references" => ("referencesProvider", "references"),
            "textDocument/definition" => ("definitionProvider", "definitions"),
            "textDocument/declaration" => ("declarationProvider", "declarations"),
            "textDocument/typeDefinition" => ("typeDefinitionProvider", "type definitions"),
            "textDocument/implementation" => ("implementationProvider", "implementations"),
            _ => ("completionProvider", "suggestions"),
        };
        // Before `initialize` is answered the request waits; the server then answers it or
        // returns an error.
        if client.ready && !provides(&client.capabilities, provider.0) {
            return Err(format!("{name} does not give {}", provider.1));
        }
        let mut params = json!({
            "textDocument": {"uri": uri::from_path(path)},
            "position": {"line": row, "character": character},
        });
        if method == "textDocument/references" {
            params["context"] = json!({"includeDeclaration": true});
        }
        client.request(method, params, pending);
        Ok(())
    }

    /// What the servers said since the last call; diagnostics go straight to `diagnostics`.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut stopped = Vec::new();
        for (name, client) in &mut self.clients {
            for message in client.received() {
                handle(client, message, &mut self.diagnostics, &mut events);
            }
            client.send_later();
            if !client.alive() {
                stopped.push(*name);
            }
        }
        for name in stopped {
            let Some(client) = self.clients.remove(name) else {
                continue;
            };
            let why = client.last_error();
            let failure = if client.ready {
                Failure::Stopped(why)
            } else {
                Failure::Start(why)
            };
            events.push(Event::Message(failure.message(name)));
            self.failed.insert(name, failure);
        }
        events
    }

    /// Why the server of `path` is not running, if it failed.
    pub fn failure(&self, path: &Path) -> Option<(&'static str, &Failure)> {
        let name = server_for(path)?.0;
        self.failed.get(name).map(|failure| (name, failure))
    }

    /// The server `path` needs that installing would bring: one not installed, or one that
    /// could not start.
    pub fn installable(&self, path: &Path) -> Option<&'static str> {
        self.failure(path)
            .filter(|(_, failure)| !matches!(failure, Failure::Stopped(_)))
            .map(|(name, _)| name)
    }

    /// Whether server `name` failed in a way installing it answers: not installed, or it could
    /// not start.
    pub fn missing(&self, name: &str) -> bool {
        self.failed
            .get(name)
            .is_some_and(|failure| !matches!(failure, Failure::Stopped(_)))
    }

    /// After installing `name`: its failure is forgotten and so are its documents and their
    /// problems, which the app opens again (the server starts with the first).
    pub fn restart(&mut self, name: &str) {
        self.failed.remove(name);
        self.clients.remove(name);
        let other = |path: &PathBuf| server_for(path).is_none_or(|server| server.0 != name);
        self.documents.retain(|path, _| other(path));
        self.diagnostics.retain(|path, _| other(path));
    }

    /// A server that failed, for the tests.
    #[cfg(test)]
    pub fn fail(&mut self, name: &'static str, failure: Failure) {
        self.failed.insert(name, failure);
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
            let Some(pending) = id.as_i64().and_then(|id| client.answered(id, &message)) else {
                return;
            };
            if pending == Pending::Initialize {
                client.capabilities = message["result"]["capabilities"].clone();
                client.initialized();
            } else if let Some(event) = answered(pending, &message) {
                events.push(event);
            }
        }
        (None, Some("textDocument/publishDiagnostics")) => {
            let params = &message["params"];
            if let Some(path) = params["uri"].as_str().and_then(uri::to_path) {
                diagnostics.insert(path, parse_diagnostics(&params["diagnostics"]));
            }
        }
        // rust-analyzer says when it has loaded the project.
        (None, Some("experimental/serverStatus")) => {
            client.quiescent = message["params"]["quiescent"].as_bool();
        }
        (None, Some("window/showMessage")) if message["params"]["type"] == 1 => {
            if let Some(text) = message["params"]["message"].as_str() {
                events.push(Event::Message(text.to_owned()));
            }
        }
        _ => {}
    }
}

/// The event of the answer `message` to a request of the app.
fn answered(pending: Pending, message: &Value) -> Option<Event> {
    let result = &message["result"];
    Some(match pending {
        Pending::Initialize => return None,
        Pending::Completion { path, row, column } => Event::Completion {
            path,
            row,
            column,
            items: completion_items(result),
        },
        Pending::Hover {
            path,
            version,
            row,
            column,
        } => Event::Hover {
            path,
            version,
            row,
            column,
            text: hover_text(&result["contents"]),
        },
        Pending::Places {
            path,
            version,
            goal,
        } => match message["error"]["message"].as_str() {
            Some(error) => Event::Message(error.to_owned()),
            None => Event::Places {
                path,
                version,
                goal,
                places: places(result),
            },
        },
    })
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

/// Whether `capabilities` say the server answers `provider` (`true` or its options).
pub(crate) fn provides(capabilities: &Value, provider: &str) -> bool {
    match &capabilities[provider] {
        Value::Bool(answers) => *answers,
        Value::Object(_) => true,
        _ => false,
    }
}

/// The Markdown of a hover: `MarkupContent`, a `MarkedString` (text, or code with its
/// language) or a list of them, one after the other.
fn hover_text(contents: &Value) -> String {
    match contents {
        Value::String(text) => text.trim().to_owned(),
        Value::Array(list) => list
            .iter()
            .map(hover_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(_) => {
            let value = contents["value"].as_str().unwrap_or_default().trim();
            match contents["language"].as_str() {
                Some(language) if !value.is_empty() => format!("```{language}\n{value}\n```"),
                _ => value.to_owned(),
            }
        }
        _ => String::new(),
    }
}

/// The places of a `Location`, a list of them or of `LocationLink`s (where the symbol's
/// name is, not the whole definition).
fn places(result: &Value) -> Vec<Place> {
    let list = match result {
        Value::Array(list) => list.as_slice(),
        Value::Object(_) => std::slice::from_ref(result),
        _ => &[],
    };
    list.iter()
        .filter_map(|location| {
            let uri = location["uri"]
                .as_str()
                .or_else(|| location["targetUri"].as_str())?;
            let range = if location.get("targetSelectionRange").is_some() {
                &location["targetSelectionRange"]
            } else {
                &location["range"]
            };
            let (row, start) = position(&range["start"]);
            let (end_row, end) = position(&range["end"]);
            Some(Place {
                path: uri::to_path(uri)?,
                row,
                start,
                end_row,
                end,
            })
        })
        .collect()
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

    #[test]
    fn hovers_and_places_are_read_from_every_answer_shape() {
        let markup = json!({"kind": "markdown", "value": " **fn** main() \n"});
        assert_eq!(hover_text(&markup), "**fn** main()");
        let marked = json!([{"language": "rust", "value": "fn main()"}, "", "Runs it."]);
        assert_eq!(hover_text(&marked), "```rust\nfn main()\n```\n\nRuns it.");
        assert_eq!(hover_text(&Value::Null), "");

        let range = |line, start, end| {
            json!({"start": {"line": line, "character": start},
                   "end": {"line": line, "character": end}})
        };
        let link = places(&json!([{
            "targetUri": "file:///src/lib.rs",
            "targetRange": range(1, 0, 9),
            "targetSelectionRange": range(4, 7, 13),
        }]));
        let place = |path: &str, row, start, end| Place {
            path: PathBuf::from(path),
            row,
            start,
            end_row: row,
            end,
        };
        assert_eq!(
            link,
            [place("/src/lib.rs", 4, 7, 13)],
            "the name, not the body"
        );
        let location = places(&json!({"uri": "file:///a.py", "range": range(2, 4, 8)}));
        assert_eq!(location, [place("/a.py", 2, 4, 8)]);
        assert!(places(&Value::Null).is_empty());

        // The version asked at comes back with the answer; an error is said.
        let answer = answered(
            Pending::Places {
                path: PathBuf::from("/a.py"),
                version: 7,
                goal: Goal::References,
            },
            &json!({"id": 3, "result": [{"uri": "file:///a.py", "range": range(2, 4, 8)}]}),
        );
        assert_eq!(
            answer,
            Some(Event::Places {
                path: PathBuf::from("/a.py"),
                version: 7,
                goal: Goal::References,
                places: location,
            })
        );
        let pending = Pending::Hover {
            path: PathBuf::from("/a.py"),
            version: 2,
            row: 1,
            column: 5,
        };
        let hover = answered(pending, &json!({"result": {"contents": "x: int"}}));
        assert!(
            matches!(hover, Some(Event::Hover { version: 2, ref text, .. }) if text == "x: int")
        );
        let failed = answered(
            Pending::Places {
                path: PathBuf::from("/a.py"),
                version: 1,
                goal: Goal::Definition,
            },
            &json!({"error": {"code": -32601, "message": "Unhandled method"}}),
        );
        assert_eq!(failed, Some(Event::Message("Unhandled method".into())));

        assert!(provides(&json!({"hoverProvider": true}), "hoverProvider"));
        let options = json!({"referencesProvider": {"workDoneProgress": true}});
        assert!(provides(&options, "referencesProvider"));
        assert!(!provides(&json!({"hoverProvider": false}), "hoverProvider"));
        assert!(!provides(&json!({}), "definitionProvider"));
    }

    #[test]
    fn requests_say_why_without_a_server_and_ask_in_utf16() {
        let mut lsp = Lsp::new(PathBuf::from("/project"), false);
        let notes = Path::new("/project/notes.md");
        assert_eq!(
            lsp.hover(notes, 1, 0, 0, "x"),
            Err("No language server for .md files".into())
        );
        let make = Path::new("/project/Makefile");
        assert_eq!(
            lsp.places(Goal::References, make, 1, 0, 0, ""),
            Err("No language server for this file".into())
        );
        let rust = Path::new("/project/a.rs");
        lsp.places(Goal::References, rust, 3, 2, 2, "é😀x").unwrap();
        lsp.places(Goal::Peek, rust, 3, 0, 1, "ab").unwrap();
        lsp.hover(Path::new("/project/b.py"), 1, 5, 0, "").unwrap();
        for goal in [
            Goal::Declaration,
            Goal::TypeDefinition,
            Goal::Implementation,
        ] {
            lsp.places(goal, rust, 1, 0, 0, "").unwrap();
        }
        let asked = lsp
            .asked
            .iter()
            .map(|(method, path, row, column)| (method.as_str(), path.clone(), *row, *column))
            .collect::<Vec<_>>();
        assert_eq!(
            asked,
            [
                ("textDocument/references", rust.to_path_buf(), 2, 3),
                ("textDocument/definition", rust.to_path_buf(), 0, 1),
                ("textDocument/hover", PathBuf::from("/project/b.py"), 5, 0),
                ("textDocument/declaration", rust.to_path_buf(), 0, 0),
                ("textDocument/typeDefinition", rust.to_path_buf(), 0, 0),
                ("textDocument/implementation", rust.to_path_buf(), 0, 0),
            ]
        );
    }

    #[test]
    fn failed_servers_say_why_and_start_again_after_installing() {
        let mut lsp = Lsp::new(PathBuf::from("/project"), false);
        let rust = Path::new("/project/a.rs");
        let python = Path::new("/project/b.py");
        let c = Path::new("/project/c.c");
        let why = "error: Unknown binary 'rust-analyzer' in official toolchain 'stable'.";
        lsp.fail("rust-analyzer", Failure::Start(why.into()));
        lsp.fail("pyright-langserver", Failure::Missing);
        lsp.fail("clangd", Failure::Stopped("killed".into()));
        assert_eq!(
            lsp.places(Goal::Definition, rust, 1, 0, 0, "x"),
            Err(format!("rust-analyzer could not start: {why}"))
        );
        assert_eq!(
            lsp.hover(python, 1, 0, 0, "x"),
            Err("pyright-langserver is not installed".into())
        );
        assert_eq!(
            lsp.places(Goal::References, c, 1, 0, 0, "x"),
            Err("clangd stopped: killed".into())
        );
        assert!(lsp.asked.is_empty());
        // Installing brings the ones that are missing or could not start, not a crash.
        assert_eq!(lsp.installable(rust), Some("rust-analyzer"));
        assert_eq!(lsp.installable(python), Some("pyright-langserver"));
        assert_eq!(lsp.installable(c), None);
        assert_eq!(lsp.installable(Path::new("/project/notes.md")), None);
        assert_eq!(
            Failure::Start(String::new()).message("gopls"),
            "gopls could not start"
        );

        // After installing, the server is asked again and its documents open again.
        lsp.documents.insert(rust.to_path_buf(), 4);
        lsp.documents.insert(python.to_path_buf(), 2);
        lsp.diagnostics.insert(rust.to_path_buf(), Vec::new());
        lsp.restart("rust-analyzer");
        assert!(!lsp.documents.contains_key(rust) && lsp.documents.contains_key(python));
        assert!(lsp.diagnostics.is_empty());
        assert_eq!(lsp.installable(rust), None);
        lsp.places(Goal::Definition, rust, 1, 0, 0, "x").unwrap();
        assert_eq!(lsp.asked.len(), 1);

        // A server that stops before answering `initialize` could not start; after, it
        // stopped. Both are said once and stay failed.
        let folder = tempfile::tempdir().unwrap();
        let command = ["git", "hash-object", "--stdin"].map(str::to_owned);
        for (ready, said) in [(false, "clangd could not start"), (true, "clangd stopped")] {
            let mut client = Client::start(&command, folder.path()).unwrap();
            client.ready = ready;
            client.kill();
            lsp.failed.remove("clangd");
            lsp.clients.insert("clangd", client);
            assert_eq!(lsp.poll(), [Event::Message(said.into())]);
            assert!(lsp.clients.is_empty());
            assert_eq!(lsp.poll(), []);
        }
        assert_eq!(
            lsp.failure(c),
            Some(("clangd", &Failure::Stopped(String::new())))
        );
    }

    #[test]
    fn the_server_says_when_it_has_loaded_the_project() {
        let folder = tempfile::tempdir().unwrap();
        let command = ["git", "hash-object", "--stdin"].map(str::to_owned);
        let mut client = Client::start(&command, folder.path()).unwrap();
        client.initialized();
        let mut diagnostics = HashMap::new();
        let mut events = Vec::new();
        for (quiescent, loading) in [(false, true), (true, false)] {
            let status = json!({
                "method": "experimental/serverStatus",
                "params": {"health": "ok", "quiescent": quiescent},
            });
            handle(&mut client, status, &mut diagnostics, &mut events);
            assert_eq!(client.quiescent, Some(quiescent));
            assert_eq!(client.loading(), loading);
        }
        assert!(events.is_empty());
    }

    #[test]
    fn more_languages_have_their_servers() {
        let server = |name: &str| server_for(Path::new(name)).map(|(name, _, id)| (name, id));
        assert_eq!(
            server("x.sh"),
            Some(("bash-language-server", "shellscript"))
        );
        assert_eq!(
            server("x.zsh"),
            Some(("bash-language-server", "shellscript"))
        );
        assert_eq!(server("x.yml"), Some(("yaml-language-server", "yaml")));
        assert_eq!(server("x.php"), Some(("intelephense", "php")));
        assert_eq!(server("x.lua"), Some(("lua-language-server", "lua")));
        assert_eq!(
            server("x.htm"),
            Some(("vscode-html-language-server", "html"))
        );
        assert_eq!(
            server("x.scss"),
            Some(("vscode-css-language-server", "scss"))
        );
        assert_eq!(
            server("x.json"),
            Some(("vscode-json-language-server", "json"))
        );
        assert_eq!(server_for(Path::new("x.json")).unwrap().1, ["--stdio"]);
        assert_eq!(server("x.toml"), None);
    }
}
