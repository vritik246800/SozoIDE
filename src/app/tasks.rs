//! Build, clean, rebuild and run the project (F32) and Help's Dependencies tab (F33): what is
//! run is shown first and typed into the bottom terminal only after Enter.

use std::sync::mpsc::{Receiver, TryRecvError};

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Focus};
use crate::{
    run::{Project, Task, detect, shell_line, task_line},
    terminal::tmux_prefix,
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
    /// A language server the editor needed (Q74): watched until it is there, then the request
    /// runs again.
    pub server: Option<(&'static str, super::intellisense::Request)>,
    /// The row of the Dependencies tab being installed or updated: watched until it changes
    /// (F35).
    pub component: Option<usize>,
}

/// The projects of a monorepo to choose from, for `task`.
pub struct ProjectChoice {
    pub projects: Vec<Project>,
    pub selected: usize,
    pub task: Task,
}

/// The Setup dialog (F36): the tools that make SozoIDE better, what was found of each, the
/// ones chosen to install and the row with the cursor.
pub struct Setup {
    pub components: Vec<Component>,
    pub found: Option<Vec<Found>>,
    pub chosen: Vec<bool>,
    pub selected: usize,
    reader: Option<Receiver<Vec<Found>>>,
}

impl Setup {
    /// Whether row `index` can be chosen: it is missing and this machine can install it.
    pub fn can_install(&self, index: usize) -> bool {
        self.found
            .as_ref()
            .is_some_and(|found| found[index] == Found::Missing)
            && self.components[index].install.is_some()
    }

    fn found(&mut self, found: Vec<Found>) {
        self.found = Some(found);
        self.chosen = (0..self.components.len())
            .map(|index| self.can_install(index))
            .collect();
    }

    #[cfg(test)]
    pub fn set_reader(&mut self, reader: Receiver<Vec<Found>>) {
        self.reader = Some(reader);
    }

    /// Space or a click on a row that can be chosen.
    fn toggle(&mut self, index: usize) {
        if self.can_install(index) {
            self.chosen[index] = !self.chosen[index];
        }
    }
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
    pub(super) reader: Option<Receiver<Vec<Found>>>,
    /// The row being installed or updated and what will say it changed (F35).
    pub watching: Option<(usize, Receiver<Found>)>,
}

impl Dependencies {
    /// A tab already checked, for the tests. Without a result it is still checking: it has
    /// a reader with nobody to answer, so opening the tab starts no real check (E53).
    #[cfg(test)]
    pub fn checked(components: Vec<Component>, found: Option<Vec<Found>>) -> Self {
        Self {
            reader: found.is_none().then(|| std::sync::mpsc::channel().1),
            components,
            found,
            selected: 0,
            watching: None,
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
            server: None,
            component: None,
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
                        // The row changes by itself once the installation is done (F35); the
                        // terminal takes the focus, for a password.
                        if let Some(index) = confirm.component
                            && let Some(dependencies) = &mut self.dependencies
                        {
                            let component = &dependencies.components[index];
                            let before = dependencies
                                .found
                                .as_ref()
                                .and_then(|found| found.get(index).cloned())
                                .unwrap_or(Found::Missing);
                            dependencies.watching = Some((
                                index,
                                (self.component_watcher)(
                                    component.program,
                                    component.probe.clone(),
                                    before,
                                ),
                            ));
                        }
                        self.overlay = super::Overlay::None;
                    }
                    self.confirmed.insert(confirm.line.clone());
                    self.type_command(&confirm.line, confirm.running, confirm.task);
                    if let Some((name, request)) = confirm.server {
                        self.watch_server(name, request);
                    }
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
    /// Every component with the commands of this machine.
    pub(super) fn all_components() -> Vec<Component> {
        deps::components(
            std::env::consts::OS,
            deps::on_path,
            if cfg!(target_os = "linux") {
                deps::sudo()
            } else {
                &[]
            },
        )
    }

    /// Opens the Setup dialog on `components`, checked on a thread (F36).
    pub(super) fn open_setup(&mut self, components: Vec<Component>) {
        let reader = (self.setup_checker)(&components);
        self.setup = Some(Setup {
            chosen: vec![false; components.len()],
            components,
            found: None,
            selected: 0,
            reader: Some(reader),
        });
        self.open_overlay(super::Overlay::Setup);
    }

    /// The keys of the Setup dialog: it installs what is chosen, after the same dialog that
    /// says the command as everything installed from here (Q65), or it goes for good.
    pub(super) fn handle_setup_key(&mut self, key: KeyEvent) {
        let Some(setup) = &mut self.setup else {
            return;
        };
        match key.code {
            KeyCode::Up => setup.selected = setup.selected.saturating_sub(1),
            KeyCode::Down => {
                setup.selected = (setup.selected + 1).min(setup.components.len().saturating_sub(1));
            }
            KeyCode::Char(' ') => setup.toggle(setup.selected),
            KeyCode::Enter => self.install_chosen(),
            KeyCode::Esc | KeyCode::Char('s' | 'S') => {
                self.status = "Help, then Dependencies, installs them when you want".into();
                self.close_overlay();
            }
            _ => {}
        }
    }

    /// A click on a row: the cursor goes there and it is chosen or left out.
    pub(super) fn click_setup_row(&mut self, index: usize) {
        if let Some(setup) = &mut self.setup {
            setup.selected = index;
            setup.toggle(index);
        }
    }

    fn install_chosen(&mut self) {
        let Some(setup) = &self.setup else {
            return;
        };
        if setup.found.is_none() {
            self.status = "Still checking...".into();
            return;
        }
        let os = std::env::consts::OS;
        let chosen = setup
            .components
            .iter()
            .zip(&setup.chosen)
            .filter(|(_, chosen)| **chosen)
            .filter_map(|(component, _)| Some((component.name, component.install.as_ref()?)))
            .collect::<Vec<_>>();
        if chosen.is_empty() {
            self.close_overlay();
            return;
        }
        let names = chosen
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        let line = chosen
            .iter()
            .map(|(_, command)| shell_line(command, os))
            .collect::<Vec<_>>()
            .join(" && ");
        self.confirm = Some(Confirm {
            title: "Install the tools".to_owned(),
            rows: vec![("Tools", names.clone()), ("Command", line.clone())],
            running: format!("Installing {names}"),
            line,
            task: false,
            server: None,
            component: None,
        });
    }

    pub(super) fn open_dependencies(&mut self) {
        self.help_tab = HelpTab::Dependencies;
        let dependencies = self.dependencies.get_or_insert_with(|| Dependencies {
            components: Self::all_components(),
            found: None,
            selected: 0,
            reader: None,
            watching: None,
        });
        // Every time the tab opens, from what was found before: something may have been
        // installed or removed since (F35).
        if dependencies.reader.is_none() {
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

    /// In the tick: the check's result, and the row being installed or updated once it
    /// changes; a language server now there starts at once (F35).
    pub(super) fn tick_dependencies(&mut self) {
        if let Some(setup) = &mut self.setup
            && let Some(found) = setup
                .reader
                .as_ref()
                .and_then(|reader| reader.try_recv().ok())
        {
            setup.reader = None;
            setup.found(found);
        }
        let Some(dependencies) = &mut self.dependencies else {
            return;
        };
        let mut changed = false;
        match dependencies.reader.as_ref().map(Receiver::try_recv) {
            Some(Ok(found)) => {
                dependencies.found = Some(found);
                dependencies.reader = None;
                changed = true;
            }
            Some(Err(TryRecvError::Disconnected)) => dependencies.reader = None,
            _ => {}
        }
        if let Some((index, watched)) = &dependencies.watching {
            match watched.try_recv() {
                Ok(now) => {
                    if let Some(found) = dependencies
                        .found
                        .as_mut()
                        .and_then(|found| found.get_mut(*index))
                    {
                        *found = now;
                    }
                    dependencies.watching = None;
                    changed = true;
                }
                Err(TryRecvError::Disconnected) => dependencies.watching = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        // Only when something new was found: a server that fails again waits for the next.
        if !changed {
            return;
        }
        let arrived = dependencies
            .found
            .iter()
            .flat_map(|found| dependencies.components.iter().zip(found))
            .filter(|(component, found)| {
                component.kind == "Language server" && matches!(found, Found::Installed(_))
            })
            .map(|(component, _)| component.name)
            .collect::<Vec<_>>();
        for name in arrived {
            // CSS's and JSON's come with HTML's.
            let names: &[&str] = if name == "vscode-html-language-server" {
                &[
                    name,
                    "vscode-css-language-server",
                    "vscode-json-language-server",
                ]
            } else {
                &[name]
            };
            for name in names {
                if self.lsp.missing(name) {
                    self.start_again(name);
                    self.status = format!("{name} is installed");
                }
            }
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
            KeyCode::Char('t' | 'T') => self.toggle_tmux(),
            _ => return false,
        }
        true
    }

    /// T on the tab: the shells to come run in tmux, or stop, and it stays so (F38). The ones
    /// on screen keep what they are.
    pub(super) fn toggle_tmux(&mut self) {
        let on = !self.updater.tmux();
        if on && !(self.tmux_usable)() {
            self.status = "tmux 2.1 or newer is needed: install it first".into();
            return;
        }
        self.updater.set_tmux(on);
        let sessions = |panel| on.then(|| tmux_prefix(&self.root, panel));
        self.bottom_terminal.set_tmux(sessions("bottom"));
        self.right_terminal.set_tmux(sessions("right"));
        self.status = if on {
            "tmux on: new shells, and every shell after restarting SozoIDE, keep running".into()
        } else {
            "tmux off: new shells are plain ones".into()
        };
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
            server: None,
            component: Some(dependencies.selected),
        });
    }

    /// What is missing, said once at a new installation or version instead of installing it.
    /// The Setup dialog says it when it comes.
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
