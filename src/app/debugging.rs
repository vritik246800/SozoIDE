//! The editor's side of debugging (F27): breakpoints, F5 to build and start (or continue),
//! F10 and F11 to step, Shift+F5 to stop, and the editor following the paused line.

use std::{path::PathBuf, sync::mpsc::channel, thread};

use crossterm::event::{KeyCode, KeyEvent};

use super::App;
use crate::debug::{self, Breakpoint, Event, Launch, Plan, Session, State};

/// What the small box of the debugger asks for (TODO 3.2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugInputKind {
    Evaluate,
    Watch,
    /// The condition, the count and the message of the breakpoint of `path` at `row`.
    Breakpoint(PathBuf, usize),
}

/// The box: what it asks and its fields (one, or three for a breakpoint), and the one typed in.
pub struct DebugInput {
    pub kind: DebugInputKind,
    pub fields: Vec<String>,
    pub field: usize,
}

/// A row of the debugger's panel that a click acts on (TODO 3.3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugTarget {
    Frame(usize),
    Variable(i64),
    Thread(i64),
    Watch(usize),
}

impl App {
    /// A click on a row of the panel: a frame, a variable that opens, a thread, a watch.
    pub(super) fn debug_click(&mut self, target: DebugTarget) {
        let Some(session) = &mut self.debug else {
            return;
        };
        match target {
            DebugTarget::Frame(index) => session.select_frame(index),
            DebugTarget::Variable(reference) => session.toggle_variable(reference),
            DebugTarget::Thread(id) => session.select_thread(id),
            DebugTarget::Watch(index) => session.remove_watch(index),
        }
    }
}

/// The launch of `.sozo.toml` waiting for Enter the first time (TODO 3.1.10).
pub struct DebugConfirm {
    pub plan: Plan,
    pub program: Option<PathBuf>,
    pub launch: Launch,
}

impl App {
    /// F9: a breakpoint on the cursor's line, or none if it had one.
    pub(super) fn toggle_breakpoint(&mut self) {
        let Some(file) = self.active_tab.and_then(|index| self.open_files.get(index)) else {
            return;
        };
        let (path, row) = (file.path.clone(), file.cursor_row);
        let rows = self.breakpoints.entry(path.clone()).or_default();
        if !rows.remove(&row) {
            rows.insert(row);
        }
        let rows = rows.clone();
        if let Some(session) = &mut self.debug {
            session.set_breakpoints(&path, &rows);
        }
    }

    /// F5: continues when paused; otherwise saves, builds the active file with symbols in a
    /// thread and starts the debugger when that is done (`tick_debug`).
    pub(super) fn debug_start_or_continue(&mut self) {
        if self.debug.is_none() && !self.trusted(true) {
            return;
        }
        if let Some(session) = &mut self.debug {
            match session.state {
                State::Paused(_) => return session.resume("continue"),
                State::Ended(_) => self.debug = None,
                _ => return,
            }
        }
        if self.debug_build.is_some() {
            return;
        }
        let Some(file) = self
            .active_tab
            .and_then(|index| self.open_files.get(index))
            .map(|file| file.path.clone())
        else {
            return;
        };
        self.flush_all_files();
        let (root, scratch) = (self.root.clone(), crate::storage::private_folder("debug"));
        let (sender, receiver) = channel();
        let name = file.display().to_string();
        thread::spawn(move || {
            let result =
                debug::plan(&file, &root, &scratch, debug::locate).and_then(|(plan, build)| {
                    match build {
                        Some(build) => build
                            .run(&scratch)
                            .map(|program| (plan, program))
                            .map_err(|error| format!("{error:#}")),
                        None => Ok((plan, None)),
                    }
                });
            let _ = sender.send(result);
        });
        self.debug_build = Some(receiver);
        self.status = format!("Building {name} to debug it...");
    }

    /// F10 and F11.
    pub(super) fn debug_step(&mut self, command: &str) {
        if let Some(session) = &mut self.debug {
            session.resume(command);
        }
    }

    /// Shift+F5: stops the program and closes the panel.
    pub(super) fn debug_stop(&mut self) {
        if let Some(mut session) = self.debug.take() {
            if !matches!(session.state, State::Ended(_)) {
                session.stop();
            }
            self.status = "Debugging stopped".into();
        }
        self.debug_build = None;
    }

    /// Starts the adapter with `plan`, the breakpoints and what each waits for.
    fn launch_session(&mut self, plan: Plan, program: Option<PathBuf>) {
        match Session::start(&plan, program.as_deref(), &self.breakpoints) {
            Ok(mut session) => {
                for ((path, row), options) in &self.breakpoint_options {
                    session.set_breakpoint_options(path, *row, options.clone());
                }
                self.debug = Some(session);
                self.status = "Debugging: F5 continue, F10 over, F11 into, Shift+F5 stop".into();
            }
            Err(error) => self.status = format!("{error:#}"),
        }
    }

    /// In the tick: the build that finished starts the debugger (after the question, the first
    /// time `.sozo.toml` says how to launch); what the debugger said moves the editor.
    pub(super) fn tick_debug(&mut self) {
        if let Some(result) = self
            .debug_build
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.debug_build = None;
            match result.map(|(plan, program)| (plan, program, debug::launch_of(&self.root))) {
                Err(error) => self.status = error,
                Ok((_, _, Err(error))) => self.status = error,
                Ok((mut plan, program, Ok(Some(launch)))) => {
                    launch.apply(&mut plan.launch, &self.root);
                    let key = format!("debug {:?}", launch.rows());
                    if self.confirmed.contains(&key) {
                        self.launch_session(plan, program);
                    } else {
                        self.debug_confirm = Some(DebugConfirm {
                            plan,
                            program,
                            launch,
                        });
                    }
                }
                Ok((plan, program, Ok(None))) => self.launch_session(plan, program),
            }
        }
        let events = self.debug.as_mut().map(Session::poll).unwrap_or_default();
        for event in events {
            match event {
                Event::Stopped { path, row } => self.show_paused(path, row),
                Event::Ended => {
                    if let Some(State::Ended(how)) = self.debug.as_ref().map(|s| s.state.clone()) {
                        self.status = format!("{how}: Shift+F5 closes the panel");
                    }
                }
                Event::Message(message) => self.status = message,
            }
        }
    }

    /// Enter starts the launch of `.sozo.toml` (and it is not asked again); Esc does not.
    pub(super) fn handle_debug_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if let Some(confirm) = self.debug_confirm.take() {
                    self.confirmed
                        .insert(format!("debug {:?}", confirm.launch.rows()));
                    self.launch_session(confirm.plan, confirm.program);
                }
            }
            KeyCode::Esc => {
                self.debug_confirm = None;
                self.status = "Not launched".into();
            }
            _ => {}
        }
    }

    /// Ctrl+F10.
    pub(super) fn run_to_cursor(&mut self) {
        let Some(file) = self.active_file() else {
            return;
        };
        let (path, row) = (file.path.clone(), file.cursor_row);
        if let Some(session) = &mut self.debug {
            session.run_to(&path, row);
        }
    }

    /// F8.
    pub(super) fn pause_debug(&mut self) {
        if let Some(session) = &mut self.debug {
            session.pause();
        }
    }

    /// Shift+F9: the box of the breakpoint at the cursor (made if there is none).
    pub(super) fn open_breakpoint_options(&mut self) {
        let Some(file) = self.active_file() else {
            return;
        };
        let (path, row) = (file.path.clone(), file.cursor_row);
        if !self
            .breakpoints
            .get(&path)
            .is_some_and(|rows| rows.contains(&row))
        {
            self.toggle_breakpoint();
        }
        let options = self
            .breakpoint_options
            .get(&(path.clone(), row))
            .cloned()
            .unwrap_or_default();
        self.debug_input = Some(DebugInput {
            kind: DebugInputKind::Breakpoint(path, row),
            fields: vec![
                options.condition.unwrap_or_default(),
                options.hits.unwrap_or_default(),
                options.log.unwrap_or_default(),
            ],
            field: 0,
        });
    }

    /// Ctrl+Shift+E and Ctrl+Shift+W: the box of an expression, at a pause.
    pub(super) fn open_debug_input(&mut self, kind: DebugInputKind) {
        if self.debug.is_none() {
            self.status = "Start debugging first (F5)".into();
            return;
        }
        self.debug_input = Some(DebugInput {
            kind,
            fields: vec![String::new()],
            field: 0,
        });
    }

    /// The keys of the box: typing, Tab between the fields of a breakpoint, Enter, Esc.
    pub(super) fn handle_debug_input_key(&mut self, key: KeyEvent) {
        let Some(input) = &mut self.debug_input else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.debug_input = None,
            KeyCode::Tab | KeyCode::BackTab => input.field = (input.field + 1) % input.fields.len(),
            KeyCode::Backspace => {
                input.fields[input.field].pop();
            }
            KeyCode::Char(character)
                if !character.is_control() && input.fields[input.field].len() < 300 =>
            {
                input.fields[input.field].push(character);
            }
            KeyCode::Enter => {
                let Some(input) = self.debug_input.take() else {
                    return;
                };
                let text = |index: usize| {
                    Some(input.fields[index].trim().to_owned()).filter(|text| !text.is_empty())
                };
                match input.kind {
                    DebugInputKind::Evaluate => {
                        if let (Some(session), Some(expression)) = (&mut self.debug, text(0)) {
                            session.evaluate(&expression);
                        }
                    }
                    DebugInputKind::Watch => {
                        if let (Some(session), Some(expression)) = (&mut self.debug, text(0)) {
                            session.add_watch(&expression);
                        }
                    }
                    DebugInputKind::Breakpoint(path, row) => {
                        let options = Breakpoint {
                            condition: text(0),
                            hits: text(1),
                            log: text(2),
                        };
                        if options == Breakpoint::default() {
                            self.breakpoint_options.remove(&(path.clone(), row));
                        } else {
                            self.breakpoint_options
                                .insert((path.clone(), row), options.clone());
                        }
                        if let Some(session) = &mut self.debug {
                            session.set_breakpoint_options(&path, row, options);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn show_paused(&mut self, path: PathBuf, row: usize) {
        if !path.is_file() {
            return;
        }
        match self.open_file(path) {
            Ok(()) => {
                if let Some(index) = self.active_tab {
                    let file = &mut self.open_files[index];
                    file.mark(false);
                    file.move_cursor_to(row, 0);
                }
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// A build for the debugger is running.
    pub fn debug_building(&self) -> bool {
        self.debug_build.is_some()
    }

    /// The row the debugger is paused at in `path`, for the margin.
    pub fn paused_row(&self, path: &std::path::Path) -> Option<usize> {
        let session = self.debug.as_ref()?;
        if !matches!(session.state, State::Paused(_)) {
            return None;
        }
        session
            .frames
            .iter()
            .find(|frame| frame.path.is_some())
            .filter(|frame| frame.path.as_deref() == Some(path))
            .map(|frame| frame.row)
    }
}
