use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Instant,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::{
    animation::{Edge, InterfaceMotion},
    git::{DiffRow, GitAction, GitPanel, GitTab},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitMode {
    None,
    Commit(String),
    Rebase(String),
    ConfirmRebase(String),
    ConfirmCheckout(String),
}

/// Where the timeline is (F06): the graph, the files of a commit in the panel that slides in
/// from the right, or the diff of one of those files over the whole tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineStage {
    Graph,
    Files,
    Diff,
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
    Checkout,
    /// A branch clicked in the Branches tree: switch to it, after the confirmation.
    Branch(usize),
    /// A file clicked in the Launchpad: its diff.
    OpenChange(usize),
    /// A row clicked in the Diff tab: a file or hunk header folds and unfolds.
    DiffRow(usize),
    NextHunk,
    PreviousHunk,
    NextFile,
    PreviousFile,
    ToggleSplit,
    StageDiff,
    UnstageDiff,
    /// Opens the file of the diff in the editor.
    OpenFile,
    /// A commit clicked in the timeline: its files slide in.
    OpenCommitFiles(usize),
    /// A file of that commit: its diff.
    TimelineFile(usize),
    /// Back from the diff to the files, or from the files to the graph.
    TimelineBack,
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
    /// A file the Diff tab asked to open in the editor; the app opens it.
    pub open_request: Option<PathBuf>,
    pub timeline: TimelineStage,
    /// The file chosen among the commit's and the first row of its diff.
    pub timeline_file: usize,
    pub timeline_scroll: usize,
    /// The commit whose files were asked for while Git was busy; loaded once it is free.
    pub pending_commit: Option<usize>,
    task: Option<JoinHandle<()>>,
    /// A refresh that follows changes made elsewhere (F35), read without holding the panel.
    following: Option<JoinHandle<()>>,
}

impl GitController {
    pub fn new(root: PathBuf) -> Self {
        Self {
            panel: Arc::new(Mutex::new(GitPanel::new(root))),
            regions: GitRegions::default(),
            mode: GitMode::None,
            motion: InterfaceMotion::new(Instant::now(), GitTab::Changes),
            operation: false,
            open_request: None,
            timeline: TimelineStage::Graph,
            timeline_file: 0,
            timeline_scroll: 0,
            pending_commit: None,
            task: None,
            following: None,
        }
    }

    /// The repository changed elsewhere: it is read in the background and the panel takes
    /// the result in one go, without the spinner of an operation or a blank panel (F35).
    pub fn follow(&mut self) {
        if self.operation
            || self
                .following
                .as_ref()
                .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        let panel = Arc::clone(&self.panel);
        self.following = Some(thread::spawn(move || {
            let Some(source) = panel.lock().ok().map(|panel| panel.source()) else {
                return;
            };
            let read = GitPanel::read(source);
            if let Ok(mut panel) = panel.lock() {
                match read {
                    Ok(snapshot) => panel.apply(snapshot),
                    Err(error) => panel.last_error = Some(format!("{error:#}")),
                }
            }
        }));
    }

    /// Whether a refresh that follows changes is still reading.
    #[cfg(test)]
    pub fn following(&self) -> bool {
        self.following
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    pub fn open(&mut self) {
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
            if panel.active_tab != GitTab::CommitGraph {
                self.timeline = TimelineStage::Graph;
            }
        }
        if self.timeline == TimelineStage::Files && self.motion.side_closed() {
            self.timeline = TimelineStage::Graph;
        }
        if !self.operation
            && let Some(index) = self.pending_commit
        {
            let selected = self
                .panel
                .try_lock()
                .map(|mut panel| panel.select(index))
                .is_ok();
            if selected {
                self.pending_commit = None;
                self.spawn(GitAction::InspectSelected, None);
            }
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
        if self.timeline != TimelineStage::Graph {
            self.timeline_key(key.code);
            return true;
        }
        if key.code == KeyCode::Esc {
            return false;
        }
        let action = {
            let Ok(mut panel) = self.panel.try_lock() else {
                return true;
            };
            let tab = panel.active_tab;
            let changes = matches!(tab, GitTab::Changes | GitTab::Launchpad);
            match key.code {
                KeyCode::Tab => {
                    let index = GitTab::ALL
                        .iter()
                        .position(|other| *other == tab)
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
                KeyCode::PageUp => {
                    let target = panel.selected.saturating_sub(10);
                    panel.select(target);
                    None
                }
                KeyCode::PageDown => {
                    let target = panel.selected + 10;
                    panel.select(target);
                    None
                }
                KeyCode::Home => {
                    panel.select(0);
                    None
                }
                KeyCode::End => {
                    panel.select(usize::MAX);
                    None
                }
                KeyCode::Enter => match tab {
                    GitTab::Changes | GitTab::Launchpad => Some(GitUiAction::ShowDiff),
                    GitTab::CommitGraph => Some(GitUiAction::OpenCommitFiles(panel.selected)),
                    GitTab::Branches => Some(GitUiAction::Checkout),
                    GitTab::Diff => Some(GitUiAction::DiffRow(panel.selected)),
                    GitTab::Inspect => None,
                },
                KeyCode::Char('s') if changes => Some(GitUiAction::Stage),
                KeyCode::Char('u') if changes => Some(GitUiAction::Unstage),
                KeyCode::Char('c') if changes => Some(GitUiAction::OpenCommit),
                KeyCode::Char(character) if tab == GitTab::Diff => match character {
                    'n' => Some(GitUiAction::NextHunk),
                    'p' => Some(GitUiAction::PreviousHunk),
                    ']' => Some(GitUiAction::NextFile),
                    '[' => Some(GitUiAction::PreviousFile),
                    'v' => Some(GitUiAction::ToggleSplit),
                    's' => Some(GitUiAction::StageDiff),
                    'u' => Some(GitUiAction::UnstageDiff),
                    'o' => Some(GitUiAction::OpenFile),
                    _ => None,
                },
                _ => None,
            }
        };
        if let Some(action) = action {
            self.action(action);
        }
        true
    }

    /// The files panel: arrows choose, Enter shows the diff, Esc closes. The diff: arrows and
    /// pages scroll, `[`/`]` change file, Esc or Left go back to the files.
    fn timeline_key(&mut self, code: KeyCode) {
        let files = self.commit_files();
        match (self.timeline, code) {
            (TimelineStage::Files, KeyCode::Up) => {
                self.timeline_file = self.timeline_file.saturating_sub(1);
            }
            (TimelineStage::Files, KeyCode::Down) => {
                self.timeline_file = (self.timeline_file + 1).min(files.saturating_sub(1));
            }
            (TimelineStage::Files, KeyCode::Enter | KeyCode::Right) if files > 0 => {
                self.action(GitUiAction::TimelineFile(self.timeline_file));
            }
            (_, KeyCode::Esc) | (TimelineStage::Diff, KeyCode::Left | KeyCode::Backspace) => {
                self.action(GitUiAction::TimelineBack);
            }
            (TimelineStage::Diff, KeyCode::Up) => self.scroll_diff(-1),
            (TimelineStage::Diff, KeyCode::Down) => self.scroll_diff(1),
            (TimelineStage::Diff, KeyCode::PageUp) => self.scroll_diff(-20),
            (TimelineStage::Diff, KeyCode::PageDown) => self.scroll_diff(20),
            (TimelineStage::Diff, KeyCode::Home) => self.timeline_scroll = 0,
            (TimelineStage::Diff, KeyCode::Char(side @ ('[' | ']'))) if files > 0 => {
                let file = if side == ']' {
                    (self.timeline_file + 1) % files
                } else {
                    (self.timeline_file + files - 1) % files
                };
                self.action(GitUiAction::TimelineFile(file));
            }
            _ => {}
        }
    }

    /// The files of the commit loaded for the timeline, 0 while it loads.
    fn commit_files(&self) -> usize {
        self.panel
            .try_lock()
            .ok()
            .and_then(|panel| {
                panel
                    .inspected_commit
                    .as_ref()
                    .map(|details| details.files.len())
            })
            .unwrap_or(0)
    }

    fn scroll_diff(&mut self, rows: isize) {
        self.timeline_scroll = self.timeline_scroll.saturating_add_signed(rows);
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
        match self.timeline {
            TimelineStage::Files => {
                self.timeline_key(if up { KeyCode::Up } else { KeyCode::Down });
                return;
            }
            TimelineStage::Diff => {
                self.scroll_diff(if up { -3 } else { 3 });
                return;
            }
            TimelineStage::Graph => {}
        }
        if let Ok(mut panel) = self.panel.try_lock() {
            if up {
                panel.select_previous();
            } else {
                panel.select_next();
            }
        }
    }

    pub fn action(&mut self, action: GitUiAction) {
        // Choices that only move the view, made at once on the panel.
        let local = |controller: &mut Self, change: &dyn Fn(&mut GitPanel)| {
            if let Ok(mut panel) = controller.panel.try_lock() {
                change(&mut panel);
            }
        };
        match action {
            GitUiAction::Tab(tab) => local(self, &|panel| panel.change_tab(tab)),
            GitUiAction::Select(index) => local(self, &|panel| panel.select(index)),
            GitUiAction::DiffRow(index) => local(self, &|panel| {
                let header = matches!(
                    panel.diff_rows().get(index),
                    Some(DiffRow::File(_) | DiffRow::Hunk(..))
                );
                panel.select(index);
                if header {
                    panel.toggle_fold(index);
                }
            }),
            GitUiAction::NextHunk => local(self, &|panel| panel.jump_hunk(true)),
            GitUiAction::PreviousHunk => local(self, &|panel| panel.jump_hunk(false)),
            GitUiAction::ToggleSplit => local(self, &GitPanel::toggle_split),
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
            GitUiAction::OpenChange(index) => {
                local(self, &|panel| panel.select(index));
                self.spawn(GitAction::ShowDiff, Some(GitTab::Diff));
            }
            GitUiAction::Inspect => self.spawn(GitAction::InspectSelected, Some(GitTab::Inspect)),
            GitUiAction::NextFile => self.spawn(GitAction::DiffNeighbour(true), None),
            GitUiAction::PreviousFile => self.spawn(GitAction::DiffNeighbour(false), None),
            GitUiAction::StageDiff => self.spawn(GitAction::StageDiff, None),
            GitUiAction::UnstageDiff => self.spawn(GitAction::UnstageDiff, None),
            GitUiAction::OpenFile => {
                self.open_request = self.panel.try_lock().ok().and_then(|panel| {
                    let row = panel.diff_rows().get(panel.selected).copied();
                    let file = panel.diff.files.get(row.map_or(0, DiffRow::file))?;
                    Some(panel.repository_root.as_ref()?.join(&file.path))
                });
            }
            GitUiAction::OpenCommitFiles(index) => {
                local(self, &|panel| panel.select(index));
                self.timeline = TimelineStage::Files;
                self.timeline_file = 0;
                self.motion.open_side(Instant::now());
                // Loaded by the next tick, after any operation that holds the panel.
                self.pending_commit = Some(index);
                self.tick_at(Instant::now());
            }
            GitUiAction::TimelineFile(index) => {
                self.timeline_file = index;
                self.timeline_scroll = 0;
                if self.timeline != TimelineStage::Diff {
                    self.timeline = TimelineStage::Diff;
                    self.motion.replay(Edge::Right);
                }
            }
            GitUiAction::TimelineBack => match self.timeline {
                TimelineStage::Diff => {
                    self.timeline = TimelineStage::Files;
                    self.motion.replay(Edge::Left);
                }
                TimelineStage::Files => self.motion.close_side(Instant::now()),
                TimelineStage::Graph => {}
            },
            GitUiAction::Branch(index) => {
                local(self, &|panel| panel.select(index));
                self.action(GitUiAction::Checkout);
            }
            GitUiAction::Checkout => {
                let branch = self.panel.try_lock().ok().and_then(|panel| {
                    panel
                        .selected_branch()
                        .filter(|branch| !branch.is_head)
                        .map(|branch| branch.name.clone())
                });
                if let Some(branch) = branch {
                    self.open_mode(GitMode::ConfirmCheckout(branch));
                }
            }
            GitUiAction::Submit => match self.mode.clone() {
                GitMode::Commit(message) => {
                    self.close_mode();
                    if !message.trim().is_empty() {
                        self.spawn(GitAction::Commit(message), None);
                    }
                }
                GitMode::Rebase(target) if !target.trim().is_empty() => {
                    self.mode = GitMode::ConfirmRebase(target);
                    self.motion.restart_dialog(Instant::now());
                }
                GitMode::Rebase(_) => self.close_mode(),
                GitMode::ConfirmRebase(target) => {
                    self.close_mode();
                    self.spawn(GitAction::Rebase(target), Some(GitTab::Launchpad));
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
                let selected = panel.selected;
                let _ = panel.diff_commit(&commit.id);
                // The timeline stays on its commit (another tab starts at its top anyway).
                if tab.is_none() {
                    panel.selected = selected;
                }
            }
            if let Some(tab) = tab {
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
    use crate::animation::{DIALOG, Edge, TAB};

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
        assert_eq!(controller.motion.edge(), Edge::Right);
        controller.tick_at(start + TAB.open / 2);
        assert!(controller.motion.content() > 0.0 && controller.motion.content() < 1.0);
        controller.tick_at(start + TAB.open);
        assert_eq!(controller.motion.content(), 1.0);

        controller.action(GitUiAction::OpenCommit);
        controller.action(GitUiAction::Cancel);
        let closed = Instant::now();
        controller.tick_at(closed);
        assert!(matches!(controller.mode, GitMode::Commit(_)));
        controller.tick_at(closed + DIALOG.close);
        assert_eq!(controller.mode, GitMode::None);
    }

    /// Waits for the operation and lets a closing dialog finish, which swallows keys meanwhile.
    fn finish(controller: &mut GitController) {
        while controller.operation || controller.mode != GitMode::None {
            controller.tick_at(Instant::now() + DIALOG.close);
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn the_timeline_opens_a_commit_then_a_file_and_goes_back_with_esc() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.name", "Test User"]);
        git(root, &["config", "user.email", "test@example.com"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "Base"]);
        std::fs::write(root.join("a.txt"), "uno\n").unwrap();
        std::fs::write(root.join("b.txt"), "two\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "Second"]);
        let mut controller = GitController::new(root.to_path_buf());
        controller.action(GitUiAction::Tab(GitTab::CommitGraph));
        let press = |controller: &mut GitController, code| {
            controller.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        };
        press(&mut controller, KeyCode::Enter);
        assert_eq!(controller.timeline, TimelineStage::Files);
        finish(&mut controller);
        let files = controller
            .panel
            .lock()
            .unwrap()
            .inspected_commit
            .clone()
            .unwrap();
        assert_eq!(files.message, "Second\n");
        assert_eq!(files.files.len(), 2);
        press(&mut controller, KeyCode::Down);
        press(&mut controller, KeyCode::Down);
        assert_eq!(controller.timeline_file, 1);
        press(&mut controller, KeyCode::Enter);
        assert_eq!(controller.timeline, TimelineStage::Diff);
        assert_eq!(controller.motion.edge(), Edge::Right);
        press(&mut controller, KeyCode::Char('['));
        assert_eq!(controller.timeline_file, 0);
        press(&mut controller, KeyCode::PageDown);
        assert_eq!(controller.timeline_scroll, 20);
        // Esc goes back a step at a time and keeps the panel open until the graph.
        assert!(press(&mut controller, KeyCode::Esc));
        assert_eq!(controller.timeline, TimelineStage::Files);
        assert_eq!(controller.motion.edge(), Edge::Left);
        assert!(press(&mut controller, KeyCode::Esc));
        assert!(controller.motion.side_closing());
        controller.tick_at(Instant::now() + DIALOG.close);
        assert_eq!(controller.timeline, TimelineStage::Graph);
        assert!(!press(&mut controller, KeyCode::Esc));
    }

    #[test]
    fn clicking_a_branch_asks_to_switch_and_the_diff_answers_its_keys() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.name", "Test User"]);
        git(root, &["config", "user.email", "test@example.com"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "Base"]);
        git(root, &["branch", "feature/login"]);
        let mut controller = GitController::new(root.to_path_buf());
        let feature = {
            let panel = controller.panel.lock().unwrap();
            panel
                .branches
                .iter()
                .position(|branch| branch.name == "feature/login")
                .unwrap()
        };
        let main = 1 - feature;
        controller.action(GitUiAction::Tab(GitTab::Branches));
        // The branch already checked out asks nothing.
        controller.action(GitUiAction::Branch(main));
        assert_eq!(controller.mode, GitMode::None);
        controller.action(GitUiAction::Branch(feature));
        assert_eq!(
            controller.mode,
            GitMode::ConfirmCheckout("feature/login".to_owned())
        );
        controller.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        finish(&mut controller);
        assert_eq!(
            controller
                .panel
                .lock()
                .unwrap()
                .launchpad
                .current_branch
                .as_deref(),
            Some("feature/login")
        );

        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        controller.action(GitUiAction::Refresh);
        finish(&mut controller);
        controller.action(GitUiAction::Tab(GitTab::Changes));
        controller.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        finish(&mut controller);
        assert_eq!(controller.panel.lock().unwrap().active_tab, GitTab::Diff);
        let key = |controller: &mut GitController, character| {
            controller.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        };
        key(&mut controller, 'n');
        assert_eq!(controller.panel.lock().unwrap().selected, 1);
        controller.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            controller.panel.lock().unwrap().entry_count(),
            2,
            "the hunk folded"
        );
        key(&mut controller, 'v');
        assert!(controller.panel.lock().unwrap().diff_split);
        key(&mut controller, 'o');
        assert_eq!(
            controller
                .open_request
                .as_deref()
                .map(|path| path.ends_with("a.txt")),
            Some(true)
        );
        key(&mut controller, 's');
        finish(&mut controller);
        assert_eq!(controller.panel.lock().unwrap().status.staged.len(), 1);
    }
}
