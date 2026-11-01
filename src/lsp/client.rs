//! One language server: its process, JSON-RPC over stdio (`Content-Length` frames) and the
//! requests waiting for an answer. Threads read the server and its errors; the app never waits
//! for it.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::uri;

/// How many times a request the server cancelled is sent again.
const MOST_TRIES: u8 = 3;
/// How long a server that does not say when it has loaded the project counts as loading it,
/// after it answers `initialize`.
const SETTLING: Duration = Duration::from_secs(3);

/// What a request was for, to read its answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    Initialize,
    Completion {
        path: PathBuf,
        row: usize,
        column: usize,
    },
    Hover {
        path: PathBuf,
        version: u64,
        row: usize,
        column: usize,
    },
    Places {
        path: PathBuf,
        version: u64,
        goal: super::Goal,
    },
}

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    next_id: i64,
    pub pending: HashMap<i64, Pending>,
    /// Until the server answers `initialize`, what is sent waits here.
    pub ready: bool,
    queue: Vec<Value>,
    /// What the server said it answers (`hoverProvider`, `referencesProvider`…).
    pub capabilities: Value,
    /// Each request waiting for its answer: the method, the parameters and how many times it
    /// went, to send it again.
    sent: HashMap<i64, (String, Value, u8)>,
    /// The last line the server wrote on its errors: why it stopped, when it stops.
    last_error: Arc<Mutex<String>>,
    /// Whether the server says it has done loading the project (rust-analyzer's
    /// `experimental/serverStatus`), when it says; when it answered `initialize`.
    pub quiescent: Option<bool>,
    ready_at: Option<Instant>,
    /// Questions answered empty while it loaded, to ask again once it is done.
    later: Vec<(String, Value, Pending, u8)>,
}

impl Client {
    pub fn start(command: &[String], root: &Path) -> Result<Self> {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Could not start {}", command[0]))?;
        let stdin = child.stdin.take().context("No stdin")?;
        let stdout = child.stdout.take().context("No stdout")?;
        let stderr = child.stderr.take().context("No stderr")?;
        let (sender, messages) = channel();
        thread::spawn(move || read_messages(BufReader::new(stdout), &sender));
        let last_error = Arc::new(Mutex::new(String::new()));
        let last = Arc::clone(&last_error);
        thread::spawn(move || read_errors(BufReader::new(stderr), &last));
        let mut client = Self {
            child,
            stdin,
            messages,
            next_id: 0,
            pending: HashMap::new(),
            ready: false,
            queue: Vec::new(),
            capabilities: Value::Null,
            sent: HashMap::new(),
            last_error,
            quiescent: None,
            ready_at: None,
            later: Vec::new(),
        };
        let root_uri = uri::from_path(root);
        let id = client.request_now(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{"uri": root_uri, "name": "workspace"}],
                "capabilities": {
                    "textDocument": {
                        "synchronization": {"didSave": true},
                        "completion": {"completionItem": {"snippetSupport": false}},
                        "definition": {"linkSupport": true},
                        "references": {},
                        "declaration": {"linkSupport": true},
                        "typeDefinition": {"linkSupport": true},
                        "implementation": {"linkSupport": true},
                        "hover": {"contentFormat": ["markdown", "plaintext"]},
                        "publishDiagnostics": {},
                    },
                    "experimental": {"serverStatusNotification": true},
                    "workspace": {"workspaceFolders": true, "configuration": true},
                    "general": {"positionEncodings": ["utf-16"]},
                },
            }),
        )?;
        client.pending.insert(id, Pending::Initialize);
        Ok(client)
    }

    /// The messages the server sent since the last call.
    pub fn received(&mut self) -> Vec<Value> {
        self.messages.try_iter().collect()
    }

    /// Whether the server is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Stops the server, for the tests.
    #[cfg(test)]
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// The last line the server wrote on its errors.
    pub fn last_error(&self) -> String {
        self.last_error
            .lock()
            .map(|line| line.clone())
            .unwrap_or_default()
    }

    /// After `initialize`: `initialized`, then what waited.
    pub fn initialized(&mut self) {
        self.ready = true;
        self.ready_at = Some(Instant::now());
        let _ = self.write(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        for message in std::mem::take(&mut self.queue) {
            let _ = self.write(&message);
        }
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// Sends a request; its answer comes back from `received` with this id.
    pub fn request(&mut self, method: &str, params: Value, pending: Pending) -> i64 {
        self.request_again(method.to_owned(), params, pending, 1)
    }

    fn request_again(&mut self, method: String, params: Value, pending: Pending, tries: u8) -> i64 {
        let id = self.take_id();
        self.pending.insert(id, pending);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        self.sent.insert(id, (method, params, tries));
        id
    }

    /// The answer `message` to request `id`: what it was for, or none when there is nothing
    /// to read yet — the server cancelled it because the text or the project changed
    /// (`ContentModified`, `ServerCancelled`: rust-analyzer when it ends loading the project)
    /// and it went again.
    pub fn answered(&mut self, id: i64, message: &Value) -> Option<Pending> {
        let pending = self.pending.remove(&id)?;
        let Some((method, params, tries)) = self.sent.remove(&id) else {
            return Some(pending);
        };
        if matches!(message["error"]["code"].as_i64(), Some(-32801 | -32802)) && tries < MOST_TRIES
        {
            self.request_again(method, params, pending, tries + 1);
            return None;
        }
        // A place or a hover answered empty while the server still loads the project
        // (rust-analyzer just after it starts) is asked once more when it is done (E48).
        let empty = message.get("error").is_none()
            && (message["result"].is_null()
                || message["result"].as_array().is_some_and(Vec::is_empty));
        if empty
            && tries < MOST_TRIES
            && matches!(pending, Pending::Places { .. } | Pending::Hover { .. })
            && self.loading()
        {
            self.later.push((method, params, pending, MOST_TRIES));
            return None;
        }
        Some(pending)
    }

    /// Whether the server is still loading the project: before it answers `initialize`,
    /// while it says it is not quiescent, or for the first seconds of one that does not say.
    pub fn loading(&self) -> bool {
        match (self.ready_at, self.quiescent) {
            (None, _) => true,
            (Some(_), Some(quiescent)) => !quiescent,
            (Some(at), None) => at.elapsed() < SETTLING,
        }
    }

    /// The questions answered empty while the server loaded, asked again once it is done.
    pub fn send_later(&mut self) {
        if self.later.is_empty() || self.loading() {
            return;
        }
        for (method, params, pending, tries) in std::mem::take(&mut self.later) {
            self.request_again(method, params, pending, tries);
        }
    }

    /// Answers a request of the server: nothing, or one `null` per item asked for.
    pub fn answer(&mut self, id: Value, result: Value) {
        let _ = self.write(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn request_now(&mut self, method: &str, params: Value) -> Result<i64> {
        let id = self.take_id();
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        Ok(id)
    }

    fn take_id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }

    fn send(&mut self, message: Value) {
        if self.ready {
            let _ = self.write(&message);
        } else {
            self.queue.push(message);
        }
    }

    fn write(&mut self, message: &Value) -> Result<()> {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len())?;
        self.stdin.flush()?;
        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Keeps the last line that is not empty of what the server writes on its errors, reading it
/// all (a full pipe would stop the server).
fn read_errors(mut reader: impl BufRead, last: &Mutex<String>) {
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if !text.is_empty()
            && let Ok(mut last) = last.lock()
        {
            *last = text.chars().take(200).collect();
        }
    }
}

/// Reads `Content-Length` frames until the server closes its output.
pub(crate) fn read_messages(mut reader: impl BufRead, sender: &Sender<Value>) {
    loop {
        let mut length = None;
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let header = header.trim();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header
                .split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            {
                length = value.1.trim().parse::<usize>().ok();
            }
        }
        let Some(length) = length else {
            continue;
        };
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        if let Ok(message) = serde_json::from_slice(&body)
            && sender.send(message).is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_read_one_message_at_a_time() {
        let first = r#"{"id":1,"result":null}"#;
        let second = r#"{"method":"window/logMessage","params":{"message":"é"}}"#;
        let stream = format!(
            "Content-Length: {}\r\nContent-Type: x\r\n\r\n{first}content-length: {}\r\n\r\n{second}Content-Length: 99\r\n\r\n{{",
            first.len(),
            second.len()
        );
        let (sender, receiver) = channel();
        read_messages(stream.as_bytes(), &sender);
        drop(sender);
        let messages = receiver.iter().collect::<Vec<_>>();
        assert_eq!(messages.len(), 2, "the cut-off third is dropped");
        assert_eq!(messages[1]["params"]["message"], "é");
    }

    #[test]
    fn errors_keep_their_last_line_and_cancelled_requests_go_again() {
        let last = Mutex::new(String::new());
        read_errors(
            "error: first\n\n  error: Unknown binary 'x'  \r\n\n".as_bytes(),
            &last,
        );
        assert_eq!(*last.lock().unwrap(), "error: Unknown binary 'x'");

        // `git hash-object --stdin` reads what it is sent and answers nothing until the end,
        // on every system the tests run on: a server that never answers `initialize`.
        let folder = tempfile::tempdir().unwrap();
        let command = ["git", "hash-object", "--stdin"].map(str::to_owned);
        let mut client = Client::start(&command, folder.path()).unwrap();
        let pending = Pending::Places {
            path: PathBuf::from("a.rs"),
            version: 2,
            goal: crate::lsp::Goal::Definition,
        };
        let params = json!({"position": {"line": 4, "character": 9}});
        let mut id = client.request("textDocument/definition", params.clone(), pending.clone());
        let error = |id: i64, code: i64| json!({"id": id, "error": {"code": code, "message": "x"}});
        // Cancelled because the project changed: sent again with the same method and
        // parameters, three times in all.
        for code in [-32801, -32802] {
            assert_eq!(client.answered(id, &error(id, code)), None);
            assert!(!client.pending.contains_key(&id));
            id = *client
                .pending
                .iter()
                .find(|(_, pending)| **pending != Pending::Initialize)
                .unwrap()
                .0;
            let again = client.queue.last().unwrap();
            assert_eq!(again["method"], "textDocument/definition");
            assert_eq!(again["params"], params);
            assert_eq!(again["id"], id);
        }
        assert_eq!(
            client.answered(id, &error(id, -32801)),
            Some(pending.clone())
        );
        assert_eq!(
            client.pending.values().collect::<Vec<_>>(),
            [&Pending::Initialize]
        );
        assert!(client.sent.is_empty());
        // Other errors and answers are read at once, once the server has loaded.
        let id = client.request("textDocument/hover", json!({}), pending.clone());
        assert_eq!(
            client.answered(id, &error(id, -32601)),
            Some(pending.clone())
        );
        client.initialized();
        client.quiescent = Some(true);
        let id = client.request("textDocument/hover", json!({}), pending.clone());
        assert_eq!(
            client.answered(id, &json!({"id": id, "result": null})),
            Some(pending)
        );
        assert_eq!(
            client.answered(id, &json!({"id": id, "result": null})),
            None
        );
    }

    #[test]
    fn empty_answers_while_the_server_loads_are_asked_again_once_it_is_done() {
        let folder = tempfile::tempdir().unwrap();
        let command = ["git", "hash-object", "--stdin"].map(str::to_owned);
        let mut client = Client::start(&command, folder.path()).unwrap();
        let places = Pending::Places {
            path: PathBuf::from("a.rs"),
            version: 1,
            goal: crate::lsp::Goal::Definition,
        };
        let empty = |id: i64| json!({"id": id, "result": null});
        let asked = |client: &Client| {
            client
                .pending
                .iter()
                .find(|(_, pending)| **pending != Pending::Initialize)
                .map(|(id, _)| *id)
        };
        assert!(client.loading(), "before it answers initialize");
        client.initialized();
        // rust-analyzer loading the project: an empty answer waits.
        client.quiescent = Some(false);
        let id = client.request("textDocument/definition", json!({"x": 1}), places.clone());
        assert_eq!(client.answered(id, &empty(id)), None);
        client.send_later();
        assert_eq!(asked(&client), None, "not while it loads");
        // Done: asked again with the same method and parameters, once.
        client.quiescent = Some(true);
        client.send_later();
        let id = asked(&client).unwrap();
        assert_eq!(client.sent[&id].0, "textDocument/definition");
        assert_eq!(client.sent[&id].1, json!({"x": 1}));
        client.quiescent = Some(false);
        assert_eq!(client.answered(id, &empty(id)), Some(places.clone()));
        // Suggestions are not asked again.
        let completion = Pending::Completion {
            path: PathBuf::from("a.rs"),
            row: 0,
            column: 0,
        };
        let id = client.request("textDocument/completion", json!({}), completion.clone());
        assert_eq!(
            client.answered(id, &json!({"id": id, "result": []})),
            Some(completion)
        );
        // A server that does not say counts as loading only for its first seconds.
        client.quiescent = None;
        assert!(client.loading());
        client.ready_at = Instant::now().checked_sub(SETTLING * 2);
        assert!(!client.loading());
        let id = client.request("textDocument/references", json!({}), places.clone());
        assert_eq!(
            client.answered(id, &json!({"id": id, "result": []})),
            Some(places)
        );
    }
}
