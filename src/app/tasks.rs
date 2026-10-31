//! Build, clean, rebuild and run the project (F32) and Help's Dependencies tab (F33): what is
//! run is shown first and typed into the bottom terminal only after Enter.

use std::sync::mpsc::{Receiver, TryRecvError};

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Focus};
use crate::{
    run::{Project, Task, detect, shell_line, task_line},
    update::deps::{self, Component, Found},
};

/// A command waiting for Enter: what the dialog says about it and the line the bottom
/// terminal gets.
pub struct Confirm {
    pub title: String,
    pub rows: Vec<(&'static str, String)>,
    pub line: String,
    /// The status bar once it runs.
    pub running: String,
    /// A project task (Shift+F5 stops it); an installation goes back to Help to be checked.
    pub task: bool,
}

/// The projects of a monorepo to choose from, for `task`.
pub struct ProjectChoice {
    pub projects: Vec<Project>,
    pub selected: usize,
    pub task: Task,
}

/// Help's two tabs (F33).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HelpTab {
    #[default]
    Shortcuts,
    Dependencies,
}

/// The Dependencies tab: every component, what was found of each once checked, the chosen
/// row and the check running.
pub struct Dependencies {
    pub components: Vec<Component>,
    pub found: Option<Vec<Found>>,
    pub selected: usize,
    reader: Option<Receiver<Vec<Found>>>,
}

impl Dependencies {
    /// A tab already checked, for the tests.
    #[cfg(test)]
    pub fn checked(components: Vec<Component>, found: Option<Vec<Found>>) -> Self {
        Self {
            components,
            found,
            selected: 0,
            reader: None,
        }
    }

    /// How many are missing and how many are there, once checked.
    pub fn counts(&self) -> Option<(usize, usize)> {
        let found = self.found.as_ref()?;
        let missing = found
            .iter()
            .filter(|found| **found == Found::Missing)
            .count();
        Some((missing, found.len() - missing))
    }
}

/// What a task is called while it runs.
fn doing(task: Task) -> &'static str {
    match task {
        Task::Build => "Building",
        Task::Clean => "Cleaning",
        Task::Rebuild => "Rebuilding",
        Task::Run => "Running",
    }
}

impl App {
    /// F7, Shift+F7, Ctrl+F7 and Ctrl+F5: the project's command, after the dialog the first
    /// time; several projects are chosen first; without one, Run runs the open file (F25).
    pub(super) fn run_task(&mut self, task: Task) {
        self.flush_all_files();
        let os = std::env::consts::OS;
        let projects = detect(&self.root, os);
        if projects.is_empty() {
            match (
                task,
                self.active_tab
                    .and_then(|index| self.open_files.get(index))
                    .map(|file| file.path.clone()),
            ) {
                (Task::Run, Some(path)) => self.run_file(path),
                _ => {
                    self.status = format!(
                        "Failed: no project here. Add [tasks.{}] to {}",
                        task.label().to_lowercase(),
                        crate::run::TASKS_FILE
                    );
                }
            }
            return;
        }
        let chosen = self.chosen_project.as_ref().and_then(|folder| {
            projects
                .iter()
                .find(|project| project.folder == *folder)
                .cloned()
        });
        let project = match chosen {
            Some(project) => project,
            None if projects.len() == 1 => projects[0].clone(),
            None => {
                self.project_choice = Some(ProjectChoice {
                    projects,
                    selected: 0,
                    task,
                });
                return;
            }
        };
        self.start_task(&project, task);
    }

    fn start_task(&mut self, project: &Project, task: Task) {
        let line = match task_line(project, task, std::env::consts::OS, deps::on_path) {
            Ok(line) => line,
            Err(why) => {
                self.status = format!("Failed: {why}");
                return;
            }
        };
        // From the project folder's name, as the Properties dialog says where things are.
        let folder = project
            .folder
            .strip_prefix(self.root.parent().unwrap_or(&self.root))
            .unwrap_or(&project.folder)
            .display()
            .to_string();
        let running = format!("{}: {line}", doing(task));
        if self.confirmed.contains(&line) {
            self.type_command(&line, running, true);
            return;
        }
        self.confirm = Some(Confirm {
            title: format!("{} the {} project", task.label(), project.kind),
            rows: vec![
                ("Project", project.kind.to_owned()),
                ("Folder", folder),
                ("From", project.source.clone()),
                ("Command", line.clone()),
            ],
            line,
            running,
            task: true,
        });
    }

    /// The line goes to the bottom terminal, which opens with the focus.
    fn type_command(&mut self, line: &str, status: String, task: bool) {
        self.show_bottom_terminal = true;
        self.focus = Focus::BottomTerminal;
        self.terminal_selection = None;
        self.bottom_terminal
            .current_mut()
            .send_text(&format!("{line}\r"));
        self.status = status;
        if task {
            self.task_running = true;
        }
    }

    /// The command dialog: Enter runs it (and it does not ask again), Esc does not.
    pub(super) fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if let Some(confirm) = self.confirm.take() {
                    if !confirm.task {
                        // Help's check sees what the installation did when it opens again.
                        if let Some(dependencies) = &mut self.dependencies {
                            dependencies.found = None;
                        }
                        self.overlay = super::Overlay::None;
                    }
                    self.confirmed.insert(confirm.line.clone());
                    self.type_command(&confirm.line, confirm.running, confirm.task);
                }
            }
            KeyCode::Esc => {
                self.confirm = None;
                self.status = "Cancelled".into();
            }
            _ => {}
        }
    }

    /// The project choice: arrows, Enter keeps it for the session, Esc goes.
    pub(super) fn handle_choice_key(&mut self, key: KeyEvent) {
        let Some(choice) = &mut self.project_choice else {
            return;
        };
        let count = choice.projects.len();
        match key.code {
            KeyCode::Up => choice.selected = (choice.selected + count - 1) % count,
            KeyCode::Down => choice.selected = (choice.selected + 1) % count,
            KeyCode::Enter => self.choose_project(None),
            KeyCode::Esc => self.project_choice = None,
            _ => {}
        }
    }

    /// The project `index` of the choice (the selected one without it) is kept and its task
    /// starts.
    pub(super) fn choose_project(&mut self, index: Option<usize>) {
        let Some(choice) = self.project_choice.take() else {
            return;
        };
        let project = &choice.projects[index.unwrap_or(choice.selected)];
        self.chosen_project = Some(project.folder.clone());
        self.start_task(project, choice.task);
    }

    /// Shift+F5: stops the debugger, or what a task left running (Ctrl+C to its shell).
    pub(super) fn stop(&mut self) {
        if self.debug.is_some() || self.debug_build.is_some() {
            self.debug_stop();
        } else if std::mem::take(&mut self.task_running) {
            self.bottom_terminal.current_mut().send_text("\u{3}");
            self.status = "Stopped".into();
        } else {
            self.status = "Nothing is running".into();
        }
    }

    /// Whether Shift+F5 has something to stop, wherever the focus is.
    pub(super) fn something_runs(&self) -> bool {
        self.task_running || self.debug.is_some() || self.debug_build.is_some()
    }

    /// Help's Dependencies tab opens: the check starts unless it has a result.
    pub(super) fn open_dependencies(&mut self) {
        self.help_tab = HelpTab::Dependencies;
        let dependencies = self.dependencies.get_or_insert_with(|| Dependencies {
            components: deps::components(
                std::env::consts::OS,
                deps::on_path,
                if cfg!(target_os = "linux") {
                    deps::sudo()
                } else {
                    &[]
                },
            ),
            found: None,
            selected: 0,
            reader: None,
        });
        if dependencies.found.is_none() && dependencies.reader.is_none() {
            dependencies.reader = Some(deps::check(&dependencies.components));
        }
    }

    /// R on the tab: everything is checked again.
    fn check_dependencies(&mut self) {
        if let Some(dependencies) = &mut self.dependencies {
            dependencies.found = None;
            dependencies.reader = None;
        }
        self.open_dependencies();
    }

    /// In the tick: the check's result.
    pub(super) fn tick_dependencies(&mut self) {
        let Some(dependencies) = &mut self.dependencies else {
            return;
        };
        match dependencies.reader.as_ref().map(Receiver::try_recv) {
            Some(Ok(found)) => {
                dependencies.found = Some(found);
                dependencies.reader = None;
            }
            Some(Err(TryRecvError::Disconnected)) => dependencies.reader = None,
            _ => {}
        }
    }

    /// The keys of the Dependencies tab; false for those Help keeps (Esc, the tabs).
    pub(super) fn handle_dependencies_key(&mut self, key: KeyEvent) -> bool {
        let Some(dependencies) = &mut self.dependencies else {
            return false;
        };
        let count = dependencies.components.len();
        match key.code {
            KeyCode::Up => dependencies.selected = dependencies.selected.saturating_sub(1),
            KeyCode::Down => dependencies.selected = (dependencies.selected + 1).min(count - 1),
            KeyCode::Enter => self.ask_to_install(None),
            KeyCode::Char('r' | 'R') => self.check_dependencies(),
            _ => return false,
        }
        true
    }

    /// Enter (or a click) on a component: its installation if it is missing, its update if
    /// it is there, shown in the command dialog; nothing while it is being checked.
    pub(super) fn ask_to_install(&mut self, index: Option<usize>) {
        let Some(dependencies) = &mut self.dependencies else {
            return;
        };
        if let Some(index) = index {
            dependencies.selected = index;
        }
        let Some(found) = dependencies.found.as_ref() else {
            self.status = "Still checking...".into();
            return;
        };
        let component = &dependencies.components[dependencies.selected];
        let (verb, command, installed) = match &found[dependencies.selected] {
            Found::Missing => ("Install", &component.install, "None".to_owned()),
            Found::Installed(version) => (
                "Update",
                &component.update,
                version.clone().unwrap_or_else(|| "Found".into()),
            ),
        };
        let Some(command) = command else {
            self.status = format!(
                "No package manager here can {} {}: do it by hand",
                verb.to_lowercase(),
                component.name
            );
            return;
        };
        let line = shell_line(command, std::env::consts::OS);
        self.confirm = Some(Confirm {
            title: format!("{verb} {}", component.name),
            rows: vec![
                (
                    "Component",
                    format!("{} ({})", component.name, component.kind),
                ),
                ("Installed", installed),
                ("Command", line.clone()),
            ],
            running: format!(
                "{} {}: {line}",
                if verb == "Install" {
                    "Installing"
                } else {
                    "Updating"
                },
                component.name
            ),
            line,
            task: false,
        });
    }

    /// What is missing, said once at a new installation or version instead of installing it.
    pub(super) fn missing_message() -> Option<String> {
        let missing = deps::missing();
        (!missing.is_empty()).then(|| {
            format!(
                "Missing {}: Help, then Dependencies, installs it",
                missing.join(", ")
            )
        })
    }
}
