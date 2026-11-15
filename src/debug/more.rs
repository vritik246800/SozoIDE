//! The debugger beyond F5, F10 and F11 (F27, TODO 3.1.8–3.1.10): breakpoints with a condition,
//! a count or a message (logpoints), step out, run to the cursor and pause; watched expressions
//! and evaluating at a pause, variables as a tree, another frame of the stack or another thread;
//! and the launch of a project written in `.sozo.toml`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Ask, Session, State};

/// What a breakpoint waits for, besides its line: an expression that must be true, a number of
/// hits, or a message to print instead of stopping (a logpoint).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Breakpoint {
    pub condition: Option<String>,
    pub hits: Option<String>,
    pub log: Option<String>,
}

/// One variable: its name, its value and, when it has fields, the reference to ask for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub reference: i64,
}

pub(super) fn variables(body: &Value) -> Vec<Variable> {
    body["variables"]
        .as_array()
        .into_iter()
        .flatten()
        .take(200)
        .map(|variable| Variable {
            name: clean(variable["name"].as_str().unwrap_or("?")),
            value: clean(variable["value"].as_str().unwrap_or_default()),
            reference: variable["variablesReference"].as_i64().unwrap_or(0),
        })
        .collect()
}

/// Text from the debugger as it can be shown: no control characters, at most 300 of them.
pub(super) fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(300).collect()
}

impl Session {
    /// The breakpoint of `path` at `row` with what it waits for; resent at once.
    pub fn set_breakpoint_options(&mut self, path: &Path, row: usize, options: Breakpoint) {
        if options == Breakpoint::default() {
            self.options.remove(&(path.to_path_buf(), row));
        } else {
            self.options.insert((path.to_path_buf(), row), options);
        }
        self.resend(path);
    }

    /// The breakpoints of `path` again, with their options and the cursor's for run to cursor.
    fn resend(&mut self, path: &Path) {
        let rows = self
            .breakpoints
            .iter()
            .find(|(file, _)| file == path)
            .map(|(_, rows)| rows.clone())
            .unwrap_or_default();
        if self.state != State::Starting {
            self.send_breakpoints(path, &rows);
        }
    }

    /// The `breakpoints` argument of `setBreakpoints` for `path`: each row with its options,
    /// and the cursor's row while running to it.
    pub(super) fn breakpoint_list(&self, path: &Path, rows: &[usize]) -> Vec<Value> {
        let mut list: Vec<Value> = rows
            .iter()
            .map(|row| {
                let mut breakpoint = json!({"line": row + 1});
                if let Some(options) = self.options.get(&(path.to_path_buf(), *row)) {
                    if let Some(condition) = &options.condition {
                        breakpoint["condition"] = json!(condition);
                    }
                    if let Some(hits) = &options.hits {
                        breakpoint["hitCondition"] = json!(hits);
                    }
                    if let Some(log) = &options.log {
                        breakpoint["logMessage"] = json!(log);
                    }
                }
                breakpoint
            })
            .collect();
        if let Some((file, row)) = &self.temporary
            && file == path
            && !rows.contains(row)
        {
            list.push(json!({"line": row + 1}));
        }
        list
    }

    /// Pauses a program that runs.
    pub fn pause(&mut self) {
        if self.state == State::Running
            && let Some(thread) = self.thread
        {
            self.request("pause", json!({"threadId": thread}), Ask::Other);
        }
    }

    /// Runs to `row` of `path`: a breakpoint there until the next pause, then continue.
    pub fn run_to(&mut self, path: &Path, row: usize) {
        if !matches!(self.state, State::Paused(_)) {
            return;
        }
        self.temporary = Some((path.to_path_buf(), row));
        if !self.breakpoints.iter().any(|(file, _)| file == path) {
            self.breakpoints.push((path.to_path_buf(), Vec::new()));
        }
        self.resend(path);
        self.resume("continue");
    }

    /// After a pause: the breakpoint of run to cursor goes.
    pub(super) fn paused_again(&mut self) {
        if let Some((path, _)) = self.temporary.take() {
            self.resend(&path);
        }
        self.request("threads", json!({}), Ask::Threads);
    }

    /// The variables of frame `index` of the stack.
    pub fn select_frame(&mut self, index: usize) {
        let Some(frame) = self.frames.get(index) else {
            return;
        };
        self.frame = index;
        let id = frame.id;
        self.request("scopes", json!({"frameId": id}), Ask::Scopes);
        self.evaluate_watches();
    }

    /// The stack of thread `id`.
    pub fn select_thread(&mut self, id: i64) {
        if !matches!(self.state, State::Paused(_)) {
            return;
        }
        self.thread = Some(id);
        self.request(
            "stackTrace",
            json!({"threadId": id, "startFrame": 0, "levels": 20}),
            Ask::StackTrace,
        );
    }

    /// Opens or closes a variable with fields; its fields are asked for the first time.
    pub fn toggle_variable(&mut self, reference: i64) {
        if reference <= 0 {
            return;
        }
        if !self.expanded.remove(&reference) {
            self.expanded.insert(reference);
            if !self.children.contains_key(&reference) {
                self.request(
                    "variables",
                    json!({"variablesReference": reference}),
                    Ask::Children(reference),
                );
            }
        }
    }

    pub fn add_watch(&mut self, expression: &str) {
        let expression = clean(expression);
        if expression.is_empty() || self.watches.len() >= 50 {
            return;
        }
        self.watches.push((expression, String::new()));
        self.evaluate_watches();
    }

    pub fn remove_watch(&mut self, index: usize) {
        if index < self.watches.len() {
            self.watches.remove(index);
        }
    }

    /// The watched expressions again, in the frame chosen.
    pub(super) fn evaluate_watches(&mut self) {
        let Some(frame) = self.frames.get(self.frame).map(|frame| frame.id) else {
            return;
        };
        for index in 0..self.watches.len() {
            let expression = self.watches[index].0.clone();
            self.request(
                "evaluate",
                json!({"expression": expression, "frameId": frame, "context": "watch"}),
                Ask::Evaluate(Some(index)),
            );
        }
    }

    /// The console of the panel: `expression` evaluated in the frame chosen, at a pause.
    pub fn evaluate(&mut self, expression: &str) {
        let expression = clean(expression);
        if expression.is_empty() || !matches!(self.state, State::Paused(_)) {
            return;
        }
        let frame = self.frames.get(self.frame).map(|frame| frame.id);
        self.console.push((expression.clone(), "…".into()));
        if self.console.len() > 100 {
            self.console.remove(0);
        }
        self.request(
            "evaluate",
            json!({"expression": expression, "frameId": frame, "context": "repl"}),
            Ask::Evaluate(None),
        );
    }
}

/// `[debug]` of `.sozo.toml`: what the project launches (TODO 3.1.10).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Launch {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    debug: Option<Launch>,
}

/// The `[debug]` of the project's `.sozo.toml`, if it has one; a broken file says why.
pub fn launch_of(root: &Path) -> Result<Option<Launch>, String> {
    let Ok(text) = std::fs::read_to_string(root.join(crate::run::TASKS_FILE)) else {
        return Ok(None);
    };
    let file: File =
        toml::from_str(&text).map_err(|error| format!(".sozo.toml: {}", error.message()))?;
    let Some(launch) = file.debug else {
        return Ok(None);
    };
    // Each value goes to the debugger as itself, never through a shell: only what cannot be
    // passed that way is refused.
    let bad = |text: &str| text.contains('\0');
    if launch.args.len() > 200
        || launch.args.iter().any(|arg| bad(arg))
        || launch
            .env
            .iter()
            .any(|(key, value)| key.is_empty() || key.contains('=') || bad(key) || bad(value))
        || launch.program.as_deref().is_some_and(bad)
        || launch.cwd.as_deref().is_some_and(bad)
    {
        return Err(".sozo.toml: [debug] has a value that cannot be passed to a program".into());
    }
    Ok(Some(launch))
}

impl Launch {
    /// The launch request of the adapter with what `.sozo.toml` says: arguments as a list,
    /// variables as a map, relative paths from the project's folder.
    pub fn apply(&self, launch: &mut Value, root: &Path) {
        let path = |text: &str| -> PathBuf {
            let path = Path::new(text);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            }
        };
        if let Some(program) = &self.program {
            launch["program"] = json!(path(program));
        }
        if !self.args.is_empty() {
            launch["args"] = json!(self.args);
        }
        if !self.env.is_empty() {
            launch["env"] = json!(self.env);
        }
        if let Some(cwd) = &self.cwd {
            launch["cwd"] = json!(path(cwd));
        }
    }

    /// What the dialog before the first launch shows.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = Vec::new();
        if let Some(program) = &self.program {
            rows.push(("Program", program.clone()));
        }
        if !self.args.is_empty() {
            rows.push(("Arguments", format!("{:?}", self.args)));
        }
        for (key, value) in &self.env {
            rows.push(("Variable", format!("{key}={value}")));
        }
        if let Some(cwd) = &self.cwd {
            rows.push(("Folder", cwd.clone()));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_from_sozo_toml_goes_as_lists_and_maps_never_as_a_line() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        assert_eq!(launch_of(root), Ok(None));
        std::fs::write(
            root.join(".sozo.toml"),
            "[debug]\nprogram = \"target/app\"\nargs = [\"--name\", \"a b; rm -rf ~\", \"$(x)\"]\ncwd = \"data\"\n[debug.env]\nMODE = \"test\"\n",
        )
        .unwrap();
        let launch = launch_of(root).unwrap().unwrap();
        let mut request = json!({"type": "lldb"});
        launch.apply(&mut request, root);
        assert_eq!(request["args"], json!(["--name", "a b; rm -rf ~", "$(x)"]));
        assert_eq!(request["env"], json!({"MODE": "test"}));
        assert_eq!(request["program"], json!(root.join("target/app")));
        assert_eq!(request["cwd"], json!(root.join("data")));
        assert_eq!(launch.rows().len(), 4);
        for hostile in [
            "[debug]\nargs = [\"a\\u0000b\"]\n",
            "[debug.env]\n\"A=B\" = \"x\"\n",
            "[debug.env]\n\"\" = \"x\"\n",
            "[debug]\nargs = 7\n",
            "[debug\n",
        ] {
            std::fs::write(root.join(".sozo.toml"), hostile).unwrap();
            assert!(launch_of(root).is_err(), "{hostile}");
        }
    }
}
