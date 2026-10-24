use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Instant,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::{
    animation::InterfaceMotion,
    git::{GitAction, GitPanel, GitTab},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitMode {
    None,
    Commit(String),
    Rebase(String),
    ConfirmRebase(String),
    ConfirmCheckout(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitUiAction {
    Tab(GitTab),
    Refresh,
    Select(usize),
    Stage,
    Unstage,
    StageAll,
    OpenCommit,
    OpenRebase,
    Fetch,
    Pull,
    Push,
    ShowDiff,
    Inspect,
    LineHistory,
    FileHistory,
    VisualFileHistory,
    Checkout,
    Submit,
    Cancel,
}

#[derive(Default)]
pub struct GitRegions {
    pub area: Rect,
    pub content: Rect,
    pub actions: Vec<(GitUiAction, Rect)>,
}

pub struct GitController {
    pub panel: Arc<Mutex<GitPanel>>,
    pub regions: GitRegions,
    pub mode: GitMode,
    pub motion: InterfaceMotion<GitTab>,
    pub operation: bool,
    pub current_file: Option<PathBuf>,
    task: Option<JoinHandle<()>>,
}

impl GitController {
    pub fn new(root: PathBuf) -> Self {
        Self {
            panel: Arc::new(Mutex::new(GitPanel::new(root))),
            regions: GitRegions::default(),
            mode: GitMode::None,
            motion: InterfaceMotion::new(Instant::now(), GitTab::Changes),
            operation: false,
            current_file: None,
            task: None,
        }
    }

    pub fn open(&mut self, current_file: Option<PathBuf>) {
        self.current_file = current_file;
        if !self.operation {
            self.spawn(GitAction::Refresh, None);
        }
    }

    pub fn tick_at(&mut self, now: Instant) {
        if self.task.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(task) = self.task.take() {
                let _ = task.join();
            }
            self.operation = false;
        }
        self.motion.tick(now, self.operation);
        if let Ok(panel) = self.panel.try_lock() {
            self.motion.sync_tab(panel.active_tab);
        }
        if self.mode != GitMode::None && self.motion.dialog_closed() {
            self.mode = GitMode::None;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.mode != GitMode::None {
            if self.motion.dialog_closing() {
                return true;
            }
            match key.code {
                KeyCode::Esc => self.close_mode(),
                KeyCode::Enter => self.action(GitUiAction::Submit),
                KeyCode::Backspace => match &mut self.mode {
                    GitMode::Commit(input) | GitMode::Rebase(input) => {
                        input.pop();
                    }
                    _ => {}
                },
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match &mut self.mode {
                        GitMode::Commit(input) | GitMode::Rebase(input) => input.push(character),
                        _ => {}
                    }
                }
                _ => {}
            }
            return true;
        }
        if key.code == KeyCode::Esc {
            return false;
        }
        let action = {
            let Ok(mut panel) = self.panel.try_lock() else {
                return true;
            };
            match key.code {
                KeyCode::Tab => {
                    let index = GitTab::ALL
                        .iter()
                        .position(|tab| *tab == panel.active_tab)
                        .unwrap_or(0);
                    panel.change_tab_by_index((index + 1) % GitTab::ALL.len());
                    None
                }
                KeyCode::Up => {
                    panel.select_previous();
                    None
                }
                KeyCode::Down => {
                    panel.select_next();
                    None
                }
                KeyCode::Enter => match panel.active_tab {
                    GitTab::Changes => Some(GitUiAction::ShowDiff),
                    GitTab::CommitGraph => Some(GitUiAction::Inspect),
                    GitTab::Branches => Some(GitUiAction::Checkout),
                    _ => None,
                },
                KeyCode::Char('s') if panel.active_tab == GitTab::Changes => {
                    Some(GitUiAction::Stage)
                }
                KeyCode::Char('u') if panel.active_tab == GitTab::Changes => {
                    Some(GitUiAction::Unstage)
                }
                _ => None,
            }
        };
        if let Some(action) = action {
            self.action(action);
        }
        true
    }

    pub fn paste(&mut self, text: &str) {
        if self.motion.dialog_closing() {
            return;
        }
        match &mut self.mode {
            GitMode::Commit(input) | GitMode::Rebase(input) => input.push_str(text),
            _ => {}
        }
    }

    pub fn click(&mut self, point: (u16, u16)) {
        if self.mode != GitMode::None && self.motion.dialog_closing() {
            return;
        }
        if let Some((action, _)) = self
            .regions
            .actions
            .iter()
            .find(|(_, area)| contains(*area, point))
            .cloned()
        {
            self.action(action);
        }
    }

    pub fn scroll(&mut self, up: bool) {
        if let Ok(mut panel) = self.panel.try_lock() {
            if up {
                panel.select_previous();
            } else {
                panel.select_next();
            }
        }
    }

    pub fn action(&mut self, action: GitUiAction) {
        match action {
            GitUiAction::Tab(tab) => {
                if let Ok(mut panel) = self.panel.try_lock() {
                    panel.change_tab(tab);
                }
            }
            GitUiAction::Select(index) => {
                if let Ok(mut panel) = self.panel.try_lock() {
                    panel.select(index);
                }
            }
            GitUiAction::Refresh => self.spawn(GitAction::Refresh, None),
            GitUiAction::Stage => self.spawn(GitAction::StageSelected, None),
            GitUiAction::Unstage => self.spawn(GitAction::UnstageSelected, None),
            GitUiAction::StageAll => self.spawn(GitAction::StageAll, None),
            GitUiAction::OpenCommit => self.open_mode(GitMode::Commit(String::new())),
            GitUiAction::OpenRebase => self.open_mode(GitMode::Rebase(String::new())),
            GitUiAction::Fetch => self.spawn(GitAction::Fetch, None),
            GitUiAction::Pull => self.spawn(GitAction::Pull, None),
            GitUiAction::Push => self.spawn(GitAction::Push, None),
            GitUiAction::ShowDiff => self.spawn(GitAction::ShowDiff, Some(GitTab::Diff)),
            GitUiAction::Inspect => self.spawn(GitAction::InspectSelected, Some(GitTab::Inspect)),
            GitUiAction::LineHistory => self.load_file(true, GitTab::LineHistory),
            GitUiAction::FileHistory => self.load_file(false, GitTab::FileHistory),
            GitUiAction::VisualFileHistory => self.load_file(false, GitTab::VisualFileHistory),
            GitUiAction::Checkout => {
                let branch =
                    self.panel.try_lock().ok().and_then(|panel| {
                        panel.selected_branch().map(|branch| branch.name.clone())
                    });
                if let Some(branch) = branch {
                    self.open_mode(GitMode::ConfirmCheckout(branch));
                }
            }
            GitUiAction::Submit => match self.mode.clone() {
                GitMode::Commit(message) => {
                    self.close_mode();
                    if !message.trim().is_empty() {
                        self.spawn(GitAction::Commit(message), Some(GitTab::Changes));
                    }
                }
                GitMode::Rebase(target) if !target.trim().is_empty() => {
                    self.mode = GitMode::ConfirmRebase(target);
                    self.motion.restart_dialog(Instant::now());
                }
                GitMode::Rebase(_) => self.close_mode(),
                GitMode::ConfirmRebase(target) => {
                    self.close_mode();
                    self.spawn(GitAction::Rebase(target), Some(GitTab::Health));
                }
                GitMode::ConfirmCheckout(branch) => {
                    self.close_mode();
                    self.spawn(GitAction::Checkout(branch), Some(GitTab::Branches));
                }
                GitMode::None => {}
            },
            GitUiAction::Cancel => self.close_mode(),
        }
    }

    fn open_mode(&mut self, mode: GitMode) {
        self.mode = mode;
        self.motion.open_dialog(Instant::now());
    }

    fn close_mode(&mut self) {
        self.motion.close_dialog(Instant::now());
    }

    fn load_file(&mut self, line: bool, tab: GitTab) {
        let selected_change = self
            .panel
            .try_lock()
            .ok()
            .and_then(|panel| panel.selected_change().map(|change| change.path.clone()));
        let path = self.current_file.clone().or(selected_change);
        if let Some(path) = path {
            let action = if line {
                GitAction::LineHistorySelected
            } else {
                GitAction::FileHistorySelected
            };
            self.spawn_with_path(action, path, tab);
        }
    }

    fn spawn(&mut self, action: GitAction, tab: Option<GitTab>) {
        if self.operation {
            return;
        }
        let panel = Arc::clone(&self.panel);
        self.operation = true;
        self.task = Some(thread::spawn(move || {
            let Ok(mut panel) = panel.lock() else {
                return;
            };
            let inspect = matches!(action, GitAction::InspectSelected);
            let result = panel.execute(action);
            if inspect
                && result.is_ok()
                && let Some(commit) = panel.selected_commit().cloned()
            {
                let _ = panel.diff_commit(&commit.id);
            }
            if let Some(tab) = tab {
                panel.change_tab(tab);
            }
        }));
    }

    fn spawn_with_path(&mut self, action: GitAction, path: PathBuf, tab: GitTab) {
        if self.operation {
            return;
        }
        let panel = Arc::clone(&self.panel);
        self.operation = true;
        self.task = Some(thread::spawn(move || {
            let Ok(mut panel) = panel.lock() else {
                return;
            };
            let result = match action {
                GitAction::LineHistorySelected => panel.load_line_history(path),
                GitAction::FileHistorySelected => panel.load_file_history(path),
                _ => Ok(()),
            };
            if let Err(error) = result {
                panel.last_error = Some(error.to_string());
            } else {
                panel.change_tab(tab);
            }
        }));
    }
}

fn contains(area: Rect, point: (u16, u16)) -> bool {
    point.0 >= area.x
        && point.0 < area.x.saturating_add(area.width)
        && point.1 >= area.y
        && point.1 < area.y.saturating_add(area.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{CONTENT_DURATION, DIALOG_DURATION};

    #[test]
    fn tab_and_mode_transitions_are_synchronized() {
        let directory = tempfile::tempdir().unwrap();
        let mut controller = GitController::new(directory.path().to_path_buf());
        controller
            .panel
            .lock()
            .unwrap()
            .change_tab(GitTab::Branches);
        let start = Instant::now();
        controller.tick_at(start);
        assert_eq!(controller.motion.content(), 0.0);
        controller.tick_at(start + CONTENT_DURATION / 2);
        assert!(controller.motion.content() > 0.0 && controller.motion.content() < 1.0);
        controller.tick_at(start + CONTENT_DURATION);
        assert_eq!(controller.motion.content(), 1.0);

        controller.action(GitUiAction::OpenCommit);
        controller.action(GitUiAction::Cancel);
        let closed = Instant::now();
        controller.tick_at(closed);
        assert!(matches!(controller.mode, GitMode::Commit(_)));
        controller.tick_at(closed + DIALOG_DURATION);
        assert_eq!(controller.mode, GitMode::None);
    }
}
