//! One language server: its process, JSON-RPC over stdio (`Content-Length` frames) and the
//! requests waiting for an answer. A thread reads the server; the app never waits for it.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{Receiver, Sender, channel},
    thread,
};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::uri;

/// What a request was for, to read its answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    Initialize,
    Completion {
        path: PathBuf,
        row: usize,
        column: usize,
    },
    Definition,
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
}

impl Client {
    pub fn start(command: &[String], root: &Path) -> Result<Self> {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Could not start {}", command[0]))?;
        let stdin = child.stdin.take().context("No stdin")?;
        let stdout = child.stdout.take().context("No stdout")?;
        let (sender, messages) = channel();
        thread::spawn(move || read_messages(BufReader::new(stdout), &sender));
        let mut client = Self {
            child,
            stdin,
            messages,
            next_id: 0,
            pending: HashMap::new(),
            ready: false,
            queue: Vec::new(),
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
                        "definition": {},
                        "publishDiagnostics": {},
                    },
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

    /// After `initialize`: `initialized`, then what waited.
    pub fn initialized(&mut self) {
        self.ready = true;
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
        let id = self.take_id();
        self.pending.insert(id, pending);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
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

/// Reads `Content-Length` frames until the server closes its output.
fn read_messages(mut reader: impl BufRead, sender: &Sender<Value>) {
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
}
