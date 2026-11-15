//! Debugging through the Debug Adapter Protocol (F27): lldb-dap or GDB for Rust, C and C++,
//! debugpy for Python, Delve for Go. The program is built with symbols in a thread, the adapter
//! runs in its own process and a thread reads it: the editor never waits for either.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, channel},
    thread,
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::{lsp::client::read_messages, update::deps};

/// Lines of program output kept for the panel.
const OUTPUT_LINES: usize = 200;

/// How a file is debugged: the adapter's command, what `launch` says and, for Delve, that it
/// talks over TCP.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub adapter: Vec<String>,
    pub launch: Value,
    pub tcp: bool,
}

/// The adapter for `file` with the tools this machine has (`locate`), and how to build it:
/// the command that compiles it with symbols, if it needs one.
pub fn plan(
    file: &Path,
    root: &Path,
    scratch: &Path,
    locate: impl Fn(&str) -> Option<String>,
) -> Result<(Plan, Option<Build>), String> {
    let extension = file
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let folder = file.parent().unwrap_or(root).to_path_buf();
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let program = scratch.join(if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem
    });
    let native = || {
        locate("lldb-dap")
            .or_else(|| locate("lldb-vscode"))
            .map(|lldb| vec![lldb])
            .or_else(|| locate("gdb").map(|gdb| vec![gdb, "-i".into(), "dap".into()]))
            .ok_or_else(|| "Install lldb-dap or gdb to debug this file".to_owned())
    };
    let launch = |program: &Path, cwd: &Path| {
        json!({
            "type": "lldb",
            "request": "launch",
            "name": "SozoIDE",
            "program": program,
            "cwd": cwd,
            "args": [],
            "stopOnEntry": false,
        })
    };
    let compiler = |tools: &[&str]| {
        tools
            .iter()
            .find_map(|tool| locate(tool))
            .ok_or_else(|| format!("Install {} to build this file", tools[0]))
    };
    match extension.as_str() {
        "rs" => {
            let manifest = file
                .ancestors()
                .skip(1)
                .take_while(|folder| folder.starts_with(root))
                .map(|folder| folder.join("Cargo.toml"))
                .find(|manifest| manifest.is_file());
            let adapter = native()?;
            match manifest {
                Some(manifest) => {
                    let cwd = manifest.parent().unwrap_or(root).to_path_buf();
                    let cargo = compiler(&["cargo"])?;
                    Ok((
                        Plan {
                            adapter,
                            launch: launch(Path::new(""), &cwd),
                            tcp: false,
                        },
                        Some(Build::Cargo {
                            cargo,
                            manifest,
                            file: file.to_path_buf(),
                        }),
                    ))
                }
                None => Ok((
                    Plan {
                        adapter,
                        launch: launch(&program, &folder),
                        tcp: false,
                    },
                    Some(Build::Command(vec![
                        compiler(&["rustc"])?,
                        "-g".into(),
                        file.display().to_string(),
                        "-o".into(),
                        program.display().to_string(),
                    ])),
                )),
            }
        }
        "c" | "cpp" | "cc" | "cxx" => {
            let tools: &[&str] = if extension == "c" {
                &["clang", "gcc", "cc"]
            } else {
                &["clang++", "g++", "c++"]
            };
            Ok((
                Plan {
                    adapter: native()?,
                    launch: launch(&program, &folder),
                    tcp: false,
                },
                Some(Build::Command(vec![
                    compiler(tools)?,
                    "-g".into(),
                    "-O0".into(),
                    file.display().to_string(),
                    "-o".into(),
                    program.display().to_string(),
                ])),
            ))
        }
        "py" => {
            let python = locate("debugpy")
                .ok_or_else(|| "Install debugpy to debug Python files".to_owned())?;
            Ok((
                Plan {
                    adapter: vec![python, "-m".into(), "debugpy.adapter".into()],
                    launch: json!({
                        "type": "python",
                        "request": "launch",
                        "name": "SozoIDE",
                        "program": file,
                        "cwd": folder,
                        "console": "internalConsole",
                        "justMyCode": true,
                    }),
                    tcp: false,
                },
                None,
            ))
        }
        "go" => {
            let dlv = locate("dlv").ok_or_else(|| "Install dlv to debug Go files".to_owned())?;
            Ok((
                Plan {
                    adapter: vec![dlv, "dap".into(), "--listen=127.0.0.1:0".into()],
                    launch: json!({
                        "type": "go",
                        "request": "launch",
                        "name": "SozoIDE",
                        "mode": "debug",
                        "program": file,
                        "cwd": folder,
                    }),
                    tcp: true,
                },
                None,
            ))
        }
        _ => Err(format!(
            "SozoIDE does not know how to debug .{extension} files"
        )),
    }
}

/// The plan and, for Cargo, the program it built: what a build thread sends back.
pub type Built = Result<(Plan, Option<PathBuf>), String>;

/// What builds the program first: a command, or Cargo, whose JSON says where the executable
/// went.
#[derive(Clone, Debug, PartialEq)]
pub enum Build {
    Command(Vec<String>),
    Cargo {
        cargo: String,
        manifest: PathBuf,
        file: PathBuf,
    },
}

impl Build {
    /// Builds, and gives the program to launch for Cargo (none for a command, which knows it).
    pub fn run(&self, scratch: &Path) -> Result<Option<PathBuf>> {
        std::fs::create_dir_all(scratch).ok();
        match self {
            Self::Command(command) => {
                let output = Command::new(&command[0])
                    .args(&command[1..])
                    .stdin(Stdio::null())
                    .output()
                    .with_context(|| format!("Could not run {}", command[0]))?;
                if !output.status.success() {
                    bail!("{}", first_error(&String::from_utf8_lossy(&output.stderr)));
                }
                Ok(None)
            }
            Self::Cargo {
                cargo,
                manifest,
                file,
            } => {
                let output = Command::new(cargo)
                    .args(["build", "--message-format=json-render-diagnostics"])
                    .arg("--manifest-path")
                    .arg(manifest)
                    .stdin(Stdio::null())
                    .output()
                    .context("Could not run cargo")?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !output.status.success() {
                    bail!("{}", first_error(&String::from_utf8_lossy(&output.stderr)));
                }
                executable(&stdout, file)
                    .map(Some)
                    .context("cargo built no program to debug")
            }
        }
    }
}

/// The first line that says what went wrong in a compiler's output.
fn first_error(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| line.contains("error"))
        .or_else(|| stderr.lines().next())
        .unwrap_or("The build failed")
        .trim()
        .to_owned()
}

/// The executable Cargo built: the one whose source is `file`, or the last one.
fn executable(messages: &str, file: &Path) -> Option<PathBuf> {
    let artifacts = messages
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter_map(|message| {
            let program = message["executable"].as_str()?.to_owned();
            let source = message["target"]["src_path"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Some((PathBuf::from(source), PathBuf::from(program)))
        })
        .collect::<Vec<_>>();
    artifacts
        .iter()
        .find(|(source, _)| source == file)
        .or(artifacts.last())
        .map(|(_, program)| program.clone())
}

/// One frame of the call stack.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    pub path: Option<PathBuf>,
    pub row: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Starting,
    Running,
    Paused(String),
    Ended(String),
}

/// What the app shows after a message of the adapter.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// Paused at `row` (from 0) of `path`: the editor goes there.
    Stopped {
        path: PathBuf,
        row: usize,
    },
    Ended,
    Message(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    Initialize,
    StackTrace,
    Scopes,
    Variables,
    Threads,
    /// A watched expression (its index) or the console (none).
    Evaluate(Option<usize>),
    /// The fields of the variable with this reference.
    Children(i64),
    Other,
}

mod more;
#[cfg(all(test, feature = "servers", target_os = "linux"))]
mod real;
pub use more::{Breakpoint, Launch, Variable, launch_of};

pub struct Session {
    child: Child,
    writer: Box<dyn Write + Send>,
    messages: Receiver<Value>,
    seq: i64,
    pending: HashMap<i64, Ask>,
    launch: Value,
    /// The breakpoints sent once the adapter says `initialized`.
    breakpoints: Vec<(PathBuf, Vec<usize>)>,
    thread: Option<i64>,
    pub state: State,
    pub frames: Vec<Frame>,
    pub variables: Vec<(String, String)>,
    pub output: VecDeque<String>,
    /// What each breakpoint waits for (TODO 3.1.8), and the one of run to cursor.
    options: HashMap<(PathBuf, usize), Breakpoint>,
    temporary: Option<(PathBuf, usize)>,
    /// The frame and the thread chosen, and the threads there are (TODO 3.1.9).
    pub frame: usize,
    pub threads: Vec<(i64, String)>,
    /// The variables with their references, the fields asked for and the ones open.
    pub tree: Vec<Variable>,
    pub children: HashMap<i64, Vec<Variable>>,
    pub expanded: std::collections::BTreeSet<i64>,
    /// Watched expressions with their values, and the console's questions and answers.
    pub watches: Vec<(String, String)>,
    pub console: Vec<(String, String)>,
}

impl Session {
    /// Starts the adapter and asks it to `initialize`; `launch` follows its answer.
    pub fn start(
        plan: &Plan,
        program: Option<&Path>,
        breakpoints: &HashMap<PathBuf, BTreeSet<usize>>,
    ) -> Result<Self> {
        let mut child = Command::new(&plan.adapter[0])
            .args(&plan.adapter[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Could not start {}", plan.adapter[0]))?;
        let stdout = child.stdout.take().context("No stdout")?;
        let (sender, messages) = channel();
        let writer: Box<dyn Write + Send> = if plan.tcp {
            // Delve prints where it listens, then takes one connection.
            let mut stdout = BufReader::new(stdout);
            let mut line = String::new();
            stdout.read_line(&mut line)?;
            let address = line
                .rsplit(' ')
                .next()
                .map(str::trim)
                .context("The debugger said nowhere to connect")?;
            let stream = TcpStream::connect(address)?;
            stream.set_read_timeout(None)?;
            let reader = BufReader::new(stream.try_clone()?);
            thread::spawn(move || read_messages(reader, &sender));
            Box::new(stream)
        } else {
            thread::spawn(move || read_messages(BufReader::new(stdout), &sender));
            Box::new(child.stdin.take().context("No stdin")?)
        };
        let mut launch = plan.launch.clone();
        if let Some(program) = program {
            launch["program"] = json!(program);
        }
        let mut session = Self {
            child,
            writer,
            messages,
            seq: 0,
            pending: HashMap::new(),
            launch,
            breakpoints: breakpoints
                .iter()
                .map(|(path, rows)| (path.clone(), rows.iter().copied().collect()))
                .collect(),
            thread: None,
            state: State::Starting,
            frames: Vec::new(),
            variables: Vec::new(),
            output: VecDeque::new(),
            options: HashMap::new(),
            temporary: None,
            frame: 0,
            threads: Vec::new(),
            tree: Vec::new(),
            children: HashMap::new(),
            expanded: std::collections::BTreeSet::new(),
            watches: Vec::new(),
            console: Vec::new(),
        };
        session.request(
            "initialize",
            json!({
                "clientID": "sozoide",
                "clientName": "SozoIDE",
                "adapterID": "sozoide",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
                "supportsRunInTerminalRequest": false,
            }),
            Ask::Initialize,
        );
        Ok(session)
    }

    fn request(&mut self, command: &str, arguments: Value, ask: Ask) {
        self.seq += 1;
        self.pending.insert(self.seq, ask);
        let message = json!({
            "seq": self.seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        })
        .to_string();
        let _ = write!(
            self.writer,
            "Content-Length: {}\r\n\r\n{message}",
            message.len()
        );
        let _ = self.writer.flush();
    }

    /// The breakpoints of `path` (rows from 0), now and whenever the adapter is ready.
    pub fn set_breakpoints(&mut self, path: &Path, rows: &BTreeSet<usize>) {
        self.breakpoints.retain(|(file, _)| file != path);
        self.breakpoints
            .push((path.to_path_buf(), rows.iter().copied().collect()));
        if self.state != State::Starting {
            self.send_breakpoints(path, &rows.iter().copied().collect::<Vec<_>>());
        }
    }

    fn send_breakpoints(&mut self, path: &Path, rows: &[usize]) {
        let lines = self.breakpoint_list(path, rows);
        self.request(
            "setBreakpoints",
            json!({"source": {"path": path}, "breakpoints": lines}),
            Ask::Other,
        );
    }

    /// F5 once paused, F10, F11.
    pub fn resume(&mut self, command: &str) {
        let Some(thread) = self.thread else {
            return;
        };
        if matches!(self.state, State::Paused(_)) {
            self.request(command, json!({"threadId": thread}), Ask::Other);
            self.state = State::Running;
            self.frames.clear();
            self.variables.clear();
            self.tree.clear();
            self.children.clear();
            self.expanded.clear();
        }
    }

    /// Shift+F5: ends the program and the adapter.
    pub fn stop(&mut self) {
        self.request("disconnect", json!({"terminateDebuggee": true}), Ask::Other);
        self.state = State::Ended("Stopped".into());
    }

    /// What the adapter said since the last call.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        for message in self.messages.try_iter().collect::<Vec<_>>() {
            self.handle(&message, &mut events);
        }
        if !matches!(self.state, State::Ended(_)) && !matches!(self.child.try_wait(), Ok(None)) {
            self.state = State::Ended("The debugger stopped".into());
            events.push(Event::Ended);
        }
        events
    }

    fn handle(&mut self, message: &Value, events: &mut Vec<Event>) {
        let body = &message["body"];
        match message["type"].as_str() {
            Some("response") => {
                let ask = message["request_seq"]
                    .as_i64()
                    .and_then(|seq| self.pending.remove(&seq))
                    .unwrap_or(Ask::Other);
                if message["success"] == false {
                    let text =
                        more::clean(message["message"].as_str().unwrap_or("The debugger failed"));
                    // A watch or the console that fails shows why in its place.
                    match ask {
                        Ask::Evaluate(Some(index)) => {
                            if let Some(watch) = self.watches.get_mut(index) {
                                watch.1 = text;
                            }
                        }
                        Ask::Evaluate(None) => {
                            if let Some(last) = self.console.last_mut() {
                                last.1 = text;
                            }
                        }
                        _ => events.push(Event::Message(text)),
                    }
                    return;
                }
                match ask {
                    Ask::Initialize => {
                        let launch = self.launch.clone();
                        self.request("launch", launch, Ask::Other);
                    }
                    Ask::StackTrace => {
                        self.frames = body["stackFrames"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|frame| Frame {
                                id: frame["id"].as_i64().unwrap_or(0),
                                name: frame["name"].as_str().unwrap_or("?").to_owned(),
                                path: frame["source"]["path"].as_str().map(PathBuf::from),
                                row: (frame["line"].as_u64().unwrap_or(1) as usize)
                                    .saturating_sub(1),
                            })
                            .collect();
                        self.frame = self
                            .frames
                            .iter()
                            .position(|frame| frame.path.is_some())
                            .unwrap_or(0);
                        let first = self.frames.get(self.frame).cloned();
                        self.evaluate_watches();
                        if let Some(frame) = first {
                            self.request("scopes", json!({"frameId": frame.id}), Ask::Scopes);
                            if let Some(path) = frame.path {
                                events.push(Event::Stopped {
                                    path,
                                    row: frame.row,
                                });
                            }
                        }
                    }
                    Ask::Scopes => {
                        let reference = body["scopes"]
                            .as_array()
                            .and_then(|scopes| scopes.first())
                            .and_then(|scope| scope["variablesReference"].as_i64());
                        if let Some(reference) = reference {
                            self.request(
                                "variables",
                                json!({"variablesReference": reference}),
                                Ask::Variables,
                            );
                        }
                    }
                    Ask::Variables => {
                        self.tree = more::variables(body);
                        self.variables = body["variables"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .take(100)
                            .map(|variable| {
                                (
                                    variable["name"].as_str().unwrap_or("?").to_owned(),
                                    variable["value"].as_str().unwrap_or_default().to_owned(),
                                )
                            })
                            .collect();
                    }
                    Ask::Children(reference) => {
                        self.children.insert(reference, more::variables(body));
                    }
                    Ask::Threads => {
                        self.threads = body["threads"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .take(100)
                            .map(|thread| {
                                (
                                    thread["id"].as_i64().unwrap_or(0),
                                    more::clean(thread["name"].as_str().unwrap_or("?")),
                                )
                            })
                            .collect();
                    }
                    Ask::Evaluate(watch) => {
                        let value = more::clean(body["result"].as_str().unwrap_or_default());
                        match watch {
                            Some(index) => {
                                if let Some(watch) = self.watches.get_mut(index) {
                                    watch.1 = value;
                                }
                            }
                            None => {
                                if let Some(last) = self.console.last_mut() {
                                    last.1 = value;
                                }
                            }
                        }
                    }
                    Ask::Other => {}
                }
            }
            Some("event") => match message["event"].as_str() {
                Some("initialized") => {
                    self.state = State::Running;
                    for (path, rows) in self.breakpoints.clone() {
                        self.send_breakpoints(&path, &rows);
                    }
                    self.request("configurationDone", json!({}), Ask::Other);
                }
                Some("stopped") => {
                    let reason = body["reason"].as_str().unwrap_or("paused").to_owned();
                    self.thread = body["threadId"].as_i64().or(self.thread);
                    self.state = State::Paused(reason);
                    self.paused_again();
                    if let Some(thread) = self.thread {
                        self.request(
                            "stackTrace",
                            json!({"threadId": thread, "startFrame": 0, "levels": 20}),
                            Ask::StackTrace,
                        );
                    }
                }
                Some("continued") => self.state = State::Running,
                Some("thread") if self.thread.is_none() => {
                    self.thread = body["threadId"].as_i64();
                }
                Some("output") if body["category"] != "telemetry" => {
                    let text = body["output"].as_str().unwrap_or_default();
                    for line in text.lines() {
                        if self.output.len() == OUTPUT_LINES {
                            self.output.pop_front();
                        }
                        self.output.push_back(line.to_owned());
                    }
                }
                Some("exited") => {
                    let code = body["exitCode"].as_i64().unwrap_or(0);
                    self.state = State::Ended(format!("Exited with code {code}"));
                }
                Some("terminated") => {
                    if !matches!(self.state, State::Ended(_)) {
                        self.state = State::Ended("Ended".into());
                    }
                    events.push(Event::Ended);
                }
                _ => {}
            },
            // A request of the adapter (`runInTerminal` and such): not supported, answered.
            Some("request") => {}
            _ => {}
        }
    }
}

/// A session in `state` with no adapter behind it, for the tests of the app and the UI.
#[cfg(test)]
pub fn fake(state: State, frames: Vec<Frame>, variables: Vec<(String, String)>) -> Session {
    let child = Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let (_sender, messages) = channel();
    Session {
        child,
        writer: Box::new(std::io::sink()),
        messages,
        seq: 0,
        pending: HashMap::new(),
        launch: Value::Null,
        breakpoints: Vec::new(),
        thread: Some(1),
        state,
        tree: variables
            .iter()
            .map(|(name, value)| Variable {
                name: name.clone(),
                value: value.clone(),
                reference: 0,
            })
            .collect(),
        frames,
        variables,
        output: VecDeque::from(["hello".to_owned()]),
        options: HashMap::new(),
        temporary: None,
        frame: 0,
        threads: Vec::new(),
        children: HashMap::new(),
        expanded: std::collections::BTreeSet::new(),
        watches: Vec::new(),
        console: Vec::new(),
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The command to find each debugging tool: `locate`, `xcrun` for lldb-dap on macOS and, for
/// debugpy, the Python that can import it.
pub fn locate(tool: &str) -> Option<String> {
    let found = deps::locate(tool).map(|path| path.display().to_string());
    match tool {
        "lldb-dap" if found.is_none() && cfg!(target_os = "macos") => {
            let output = crate::process::output(
                Command::new("xcrun").args(["--find", "lldb-dap"]),
                crate::process::QUICK,
            )?;
            let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (output.status.success() && !path.is_empty()).then_some(path)
        }
        "debugpy" => ["python3", "python"].into_iter().find_map(|python| {
            let python = deps::locate(python)?;
            crate::process::succeeds(
                Command::new(&python).args(["-c", "import debugpy"]),
                crate::process::QUICK,
            )
            .then(|| python.display().to_string())
        }),
        _ => found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_gets_its_adapter_and_build() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("app/src")).unwrap();
        std::fs::write(root.join("app/Cargo.toml"), "").unwrap();
        let scratch = Path::new("/tmp/sozo-debug");
        let tools = |present: &'static [&'static str]| {
            move |tool: &str| present.contains(&tool).then(|| format!("/bin/{tool}"))
        };
        let (plan_c, build) =
            plan(&root.join("main.c"), root, scratch, tools(&["gdb", "gcc"])).unwrap();
        assert_eq!(
            plan_c.adapter,
            ["/bin/gdb", "-i", "dap"],
            "gdb without lldb-dap"
        );
        let program = scratch.join(if cfg!(windows) { "main.exe" } else { "main" });
        assert_eq!(plan_c.launch["program"], json!(program));
        let Some(Build::Command(command)) = build else {
            panic!("a C file is built");
        };
        assert_eq!(command[..3], ["/bin/gcc", "-g", "-O0"]);
        let (plan_rust, build) = plan(
            &root.join("app/src/main.rs"),
            root,
            scratch,
            tools(&["lldb-dap", "gdb", "cargo"]),
        )
        .unwrap();
        assert_eq!(plan_rust.adapter, ["/bin/lldb-dap"]);
        assert!(matches!(build, Some(Build::Cargo { .. })));
        let (plan_python, build) =
            plan(&root.join("a.py"), root, scratch, tools(&["debugpy"])).unwrap();
        assert_eq!(
            plan_python.adapter,
            ["/bin/debugpy", "-m", "debugpy.adapter"]
        );
        assert!(build.is_none());
        let (plan_go, _) = plan(&root.join("a.go"), root, scratch, tools(&["dlv"])).unwrap();
        assert!(plan_go.tcp && plan_go.launch["mode"] == "debug");
        assert_eq!(
            plan(&root.join("a.py"), root, scratch, tools(&[])).unwrap_err(),
            "Install debugpy to debug Python files"
        );
        assert!(plan(&root.join("a.md"), root, scratch, tools(&["gdb"])).is_err());

        // Cargo's JSON: the program of this file, or the last one.
        let messages = [
            r#"{"reason":"compiler-artifact","target":{"src_path":"/p/src/lib.rs"},"executable":null}"#,
            r#"{"reason":"compiler-artifact","target":{"src_path":"/p/src/main.rs"},"executable":"/p/target/debug/p"}"#,
            r#"{"reason":"compiler-artifact","target":{"src_path":"/p/src/bin/tool.rs"},"executable":"/p/target/debug/tool"}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");
        assert_eq!(
            executable(&messages, Path::new("/p/src/main.rs")),
            Some(PathBuf::from("/p/target/debug/p"))
        );
        assert_eq!(
            executable(&messages, Path::new("/p/src/other.rs")),
            Some(PathBuf::from("/p/target/debug/tool"))
        );
        assert_eq!(
            first_error("warning: x\nerror[E0425]: cannot find value\n"),
            "error[E0425]: cannot find value"
        );
    }

    /// What a session writes, to read back in the tests.
    #[derive(Clone, Default)]
    struct Sent(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for Sent {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Sent {
        /// The commands of the requests written so far, then forgets them.
        fn commands(&self) -> Vec<String> {
            let text = String::from_utf8(std::mem::take(&mut *self.0.lock().unwrap())).unwrap();
            text.split("Content-Length")
                .filter_map(|frame| frame.split_once("\r\n\r\n"))
                .map(|(_, body)| serde_json::from_str::<Value>(body).unwrap())
                .map(|request| request["command"].as_str().unwrap().to_owned())
                .collect()
        }
    }

    #[test]
    fn a_session_launches_stops_at_a_breakpoint_and_shows_the_frame_and_variables() {
        let sent = Sent::default();
        // Any process will do: the adapter's answers are handed in below.
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let (_sender, messages) = channel();
        let mut session = Session {
            child,
            writer: Box::new(sent.clone()),
            messages,
            seq: 1,
            pending: HashMap::from([(1, Ask::Initialize)]),
            launch: json!({"program": "/p/main"}),
            breakpoints: vec![(PathBuf::from("/p/main.c"), vec![4])],
            thread: None,
            state: State::Starting,
            frames: Vec::new(),
            variables: Vec::new(),
            output: VecDeque::new(),
            options: HashMap::new(),
            temporary: None,
            frame: 0,
            threads: Vec::new(),
            tree: Vec::new(),
            children: HashMap::new(),
            expanded: std::collections::BTreeSet::new(),
            watches: Vec::new(),
            console: Vec::new(),
        };
        let mut events = Vec::new();
        let mut answer = |session: &mut Session, message: Value| {
            session.handle(&message, &mut events);
        };
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 1, "success": true}),
        );
        assert_eq!(sent.commands(), ["launch"]);
        answer(
            &mut session,
            json!({"type": "event", "event": "initialized"}),
        );
        assert_eq!(sent.commands(), ["setBreakpoints", "configurationDone"]);
        assert_eq!(session.state, State::Running);
        answer(
            &mut session,
            json!({"type": "event", "event": "output", "body": {"category": "stdout", "output": "one\ntwo\n"}}),
        );
        answer(
            &mut session,
            json!({"type": "event", "event": "stopped", "body": {"reason": "breakpoint", "threadId": 7}}),
        );
        // A pause asks the threads too (TODO 3.1.9).
        assert_eq!(sent.commands(), ["threads", "stackTrace"]);
        assert_eq!(session.state, State::Paused("breakpoint".into()));
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 5, "success": true, "body": {"threads": [
                {"id": 7, "name": "main"}, {"id": 8, "name": "worker\u{1b}"},
            ]}}),
        );
        assert_eq!(
            session.threads,
            [(7, "main".to_owned()), (8, "worker".to_owned())]
        );
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 6, "success": true, "body": {"stackFrames": [
                {"id": 3, "name": "main", "source": {"path": "/p/main.c"}, "line": 5},
            ]}}),
        );
        assert_eq!(sent.commands(), ["scopes"]);
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 7, "success": true,
                   "body": {"scopes": [{"name": "Locals", "variablesReference": 9}]}}),
        );
        assert_eq!(sent.commands(), ["variables"]);
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 8, "success": true,
                   "body": {"variables": [{"name": "counter", "value": "1"}, {"name": "point", "value": "{...}", "variablesReference": 12}]}}),
        );
        // A variable with fields opens and its fields are asked once.
        session.toggle_variable(12);
        assert_eq!(sent.commands(), ["variables"]);
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 9, "success": true,
                   "body": {"variables": [{"name": "x", "value": "2"}]}}),
        );
        assert_eq!(session.children[&12][0].name, "x");
        session.toggle_variable(12);
        session.toggle_variable(12);
        assert!(sent.commands().is_empty(), "the fields are not asked again");
        // A watch and the console are evaluated in the frame; a failure says why in place.
        session.add_watch("counter * 2");
        assert_eq!(sent.commands(), ["evaluate"]);
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 10, "success": true, "body": {"result": "2"}}),
        );
        assert_eq!(
            session.watches,
            [("counter * 2".to_owned(), "2".to_owned())]
        );
        session.evaluate("nope(");
        answer(
            &mut session,
            json!({"type": "response", "request_seq": 11, "success": false, "message": "syntax error"}),
        );
        assert_eq!(
            session.console,
            [("nope(".to_owned(), "syntax error".to_owned())]
        );
        // A breakpoint with a condition, a count and a message goes as the protocol says.
        session.set_breakpoint_options(
            Path::new("/p/main.c"),
            4,
            Breakpoint {
                condition: Some("i > 3".into()),
                hits: Some("2".into()),
                log: Some("i={i}".into()),
            },
        );
        let sent_text = sent.0.lock().unwrap().clone();
        let text = String::from_utf8_lossy(&sent_text);
        assert!(
            text.contains(r#""condition":"i > 3""#)
                && text.contains(r#""hitCondition":"2""#)
                && text.contains(r#""logMessage":"i={i}""#),
            "{text}"
        );
        sent.commands();
        assert_eq!(session.variables[0], ("counter".to_owned(), "1".to_owned()));
        assert_eq!(session.tree[1].reference, 12);
        assert_eq!(session.output, ["one", "two"]);
        session.resume("next");
        assert_eq!(sent.commands(), ["next"]);
        assert!(session.variables.is_empty() && session.state == State::Running);
        session.resume("next");
        assert!(sent.commands().is_empty(), "no step while it runs");
        answer(
            &mut session,
            json!({"type": "event", "event": "exited", "body": {"exitCode": 3}}),
        );
        answer(
            &mut session,
            json!({"type": "event", "event": "terminated"}),
        );
        assert_eq!(session.state, State::Ended("Exited with code 3".into()));
        assert_eq!(
            events,
            [
                Event::Stopped {
                    path: PathBuf::from("/p/main.c"),
                    row: 4
                },
                Event::Ended
            ]
        );
    }
}
