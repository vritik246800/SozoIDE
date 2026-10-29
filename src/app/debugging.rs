//! The editor's side of debugging (F27): breakpoints, F5 to build and start (or continue),
//! F10 and F11 to step, Shift+F5 to stop, and the editor following the paused line.

use std::{path::PathBuf, sync::mpsc::channel, thread};

use super::App;
use crate::debug::{self, Event, Session, State};

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
        let (root, scratch) = (self.root.clone(), std::env::temp_dir().join("sozo-debug"));
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

    /// In the tick: the build that finished starts the debugger; what the debugger said moves
    /// the editor to the paused line.
    pub(super) fn tick_debug(&mut self) {
        if let Some(result) = self
            .debug_build
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.debug_build = None;
            match result.and_then(|(plan, program)| {
                Session::start(&plan, program.as_deref(), &self.breakpoints)
                    .map_err(|error| format!("{error:#}"))
            }) {
                Ok(session) => {
                    self.debug = Some(session);
                    self.status =
                        "Debugging: F5 continue, F10 over, F11 into, Shift+F5 stop".into();
                }
                Err(error) => self.status = error,
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
