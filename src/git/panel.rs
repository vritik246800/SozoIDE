use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use git2::Repository;

use super::{model::*, repository::*};

#[derive(Debug)]
pub struct GitPanel {
    pub root: PathBuf,
    pub repository_root: Option<PathBuf>,
    pub active_tab: GitTab,
    pub selected: usize,
    pub status: WorkingTreeStatus,
    pub commits: Vec<CommitInfo>,
    pub branches: Vec<BranchInfo>,
    pub launchpad: LaunchpadSummary,
    pub health: HealthInfo,
    pub diff: DiffView,
    pub inspected_commit: Option<CommitDetails>,
    pub line_history: Vec<BlameLine>,
    pub file_history: Vec<FileHistoryEntry>,
    pub visual_file_history: Vec<VisualHistoryEntry>,
    pub status_message: String,
    pub last_error: Option<String>,
    pub last_output: Option<GitCommandOutput>,
    line_history_path: Option<PathBuf>,
    file_history_path: Option<PathBuf>,
}

impl GitPanel {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let mut panel = Self {
            root: root.into(),
            repository_root: None,
            active_tab: GitTab::Changes,
            selected: 0,
            status: WorkingTreeStatus::default(),
            commits: Vec::new(),
            branches: Vec::new(),
            launchpad: LaunchpadSummary::default(),
            health: HealthInfo::default(),
            diff: DiffView::default(),
            inspected_commit: None,
            line_history: Vec::new(),
            file_history: Vec::new(),
            visual_file_history: Vec::new(),
            status_message: String::new(),
            last_error: None,
            last_output: None,
            line_history_path: None,
            file_history_path: None,
        };
        if let Err(error) = panel.refresh() {
            panel.last_error = Some(format!("{error:#}"));
        }
        panel
    }

    pub fn in_repository(&self) -> bool {
        self.repository_root.is_some()
    }

    pub fn change_tab(&mut self, tab: GitTab) {
        self.active_tab = tab;
        self.selected = 0;
    }

    pub fn change_tab_by_index(&mut self, index: usize) {
        if let Some(tab) = GitTab::ALL.get(index) {
            self.change_tab(*tab);
        }
    }

    pub fn entry_count(&self) -> usize {
        match self.active_tab {
            GitTab::Changes => {
                self.status.staged.len() + self.status.unstaged.len() + self.status.untracked.len()
            }
            GitTab::CommitGraph => self.commits.len(),
            GitTab::Branches => self.branches.len(),
            GitTab::Diff => self.diff.text.lines().count(),
            GitTab::Launchpad => 0,
            GitTab::Inspect => self
                .inspected_commit
                .as_ref()
                .map_or(0, |details| details.files.len()),
            GitTab::LineHistory => self.line_history.len(),
            GitTab::FileHistory => self.file_history.len(),
            GitTab::VisualFileHistory => self.visual_file_history.len(),
            GitTab::Health => self.health.conflicts.len(),
        }
    }

    pub fn select(&mut self, index: usize) {
        let count = self.entry_count();
        self.selected = if count == 0 { 0 } else { index.min(count - 1) };
    }

    pub fn select_next(&mut self) {
        self.select(self.selected.saturating_add(1));
    }

    pub fn select_previous(&mut self) {
        self.select(self.selected.saturating_sub(1));
    }

    pub fn selected_change(&self) -> Option<&ChangeEntry> {
        self.status
            .staged
            .iter()
            .chain(&self.status.unstaged)
            .chain(&self.status.untracked)
            .nth(self.selected)
    }

    pub fn selected_commit(&self) -> Option<&CommitInfo> {
        self.commits.get(self.selected)
    }

    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.branches.get(self.selected)
    }

    pub fn selected_file_history(&self) -> Option<&FileHistoryEntry> {
        self.file_history.get(self.selected)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let previous_diff = self.diff.clone();
        let inspected_id = self
            .inspected_commit
            .as_ref()
            .map(|details| details.commit.id.clone());
        let line_history_path = self.line_history_path.clone();
        let file_history_path = self.file_history_path.clone();

        let Some(repo) = self.open_repository()? else {
            self.clear_repository_data();
            self.status_message = "Not a Git repository.".to_string();
            return Ok(());
        };

        let repository_root = repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf();
        let status = read_status(&repo).context("Could not read working-tree status.")?;
        let commits = read_commits(&repo).context("Could not read commits.")?;
        let branches = read_branches(&repo).context("Could not read branches.")?;
        let launchpad = read_launchpad(&repo, &status, commits.first().cloned())
            .context("Could not read launchpad summary.")?;
        let health = read_health(
            &repo,
            &status,
            launchpad.upstream.clone(),
            launchpad.ahead,
            launchpad.behind,
        )
        .context("Could not read repository health.")?;

        self.repository_root = Some(repository_root);
        self.status = status;
        self.commits = commits;
        self.branches = branches;
        self.launchpad = launchpad;
        self.health = health;
        self.select(self.selected);

        if let Some(id) = inspected_id {
            self.inspected_commit = Some(read_commit_details(&repo, &id)?);
        }
        if let Some(path) = line_history_path {
            self.line_history = read_line_history(&repo, &path)?;
        }
        if let Some(path) = file_history_path {
            let (history, visual) = read_file_history(&repo, &path)?;
            self.file_history = history;
            self.visual_file_history = visual;
        }
        match (
            previous_diff.kind,
            previous_diff.path,
            previous_diff.commit_id,
        ) {
            (DiffKind::Commit, _, Some(id)) => self.diff = read_commit_diff(&repo, &id)?,
            (kind, Some(path), _) if kind != DiffKind::None => {
                self.diff = read_change_diff(&repo, &path, kind)?;
            }
            _ => self.diff = DiffView::default(),
        }
        Ok(())
    }

    pub fn diff_selected(&mut self) -> Result<()> {
        let Some(change) = self.selected_change().cloned() else {
            self.diff = DiffView::default();
            self.status_message = "No change selected.".to_string();
            return Ok(());
        };
        let kind = match change.area {
            ChangeArea::Staged => DiffKind::Staged,
            ChangeArea::Unstaged => DiffKind::Unstaged,
            ChangeArea::Untracked => DiffKind::Untracked,
        };
        let repo = self.require_repository()?;
        self.diff = read_change_diff(&repo, &change.path, kind)?;
        Ok(())
    }

    pub fn diff_commit(&mut self, commit_id: &str) -> Result<()> {
        let repo = self.require_repository()?;
        self.diff = read_commit_diff(&repo, commit_id)?;
        Ok(())
    }

    pub fn inspect_commit(&mut self, commit_id: &str) -> Result<()> {
        let repo = self.require_repository()?;
        self.inspected_commit = Some(read_commit_details(&repo, commit_id)?);
        Ok(())
    }

    pub fn load_line_history(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        validate_repo_path(path)?;
        let repo = self.require_repository()?;
        self.line_history = read_line_history(&repo, path)?;
        self.line_history_path = Some(path.to_path_buf());
        self.selected = 0;
        Ok(())
    }

    pub fn load_file_history(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        validate_repo_path(path)?;
        let repo = self.require_repository()?;
        let (history, visual) = read_file_history(&repo, path)?;
        self.file_history = history;
        self.visual_file_history = visual;
        self.file_history_path = Some(path.to_path_buf());
        self.selected = 0;
        Ok(())
    }

    pub fn execute(&mut self, action: GitAction) -> Result<()> {
        self.last_error = None;
        let result = self.execute_inner(action);
        if let Err(error) = &result {
            self.last_error = Some(format!("{error:#}"));
        }
        result
    }

    #[cfg(test)]
    pub fn stage_selected(&mut self) -> Result<()> {
        self.execute(GitAction::StageSelected)
    }

    #[cfg(test)]
    pub fn unstage_selected(&mut self) -> Result<()> {
        self.execute(GitAction::UnstageSelected)
    }

    #[cfg(test)]
    pub fn stage_all(&mut self) -> Result<()> {
        self.execute(GitAction::StageAll)
    }

    #[cfg(test)]
    pub fn commit(&mut self, message: impl Into<String>) -> Result<()> {
        self.execute(GitAction::Commit(message.into()))
    }

    #[cfg(test)]
    pub fn fetch(&mut self) -> Result<()> {
        self.execute(GitAction::Fetch)
    }

    #[cfg(test)]
    pub fn pull(&mut self) -> Result<()> {
        self.execute(GitAction::Pull)
    }

    #[cfg(test)]
    pub fn push(&mut self) -> Result<()> {
        self.execute(GitAction::Push)
    }

    #[cfg(test)]
    pub fn rebase(&mut self, target: impl Into<String>) -> Result<()> {
        self.execute(GitAction::Rebase(target.into()))
    }

    #[cfg(test)]
    pub fn checkout(&mut self, branch: impl Into<String>) -> Result<()> {
        self.execute(GitAction::Checkout(branch.into()))
    }

    fn execute_inner(&mut self, action: GitAction) -> Result<()> {
        match action {
            GitAction::Refresh => {
                self.refresh()?;
                if self.in_repository() {
                    self.status_message = "Repository refreshed.".to_string();
                }
            }
            GitAction::StageSelected => self.stage_selected_inner()?,
            GitAction::UnstageSelected => self.unstage_selected_inner()?,
            GitAction::StageAll => self.stage_all_inner()?,
            GitAction::Commit(message) => self.commit_inner(&message)?,
            GitAction::Fetch => {
                self.run_git_operation(&["fetch", "--prune"], "Fetch completed.")?
            }
            GitAction::Pull => self.run_git_operation(&["pull", "--ff-only"], "Pull completed.")?,
            GitAction::Push => self.run_git_operation(&["push"], "Push completed.")?,
            GitAction::Rebase(target) => {
                self.ensure_clean("Rebase")?;
                if target.trim().is_empty() {
                    bail!("A rebase target is required.");
                }
                self.run_git_operation(&["rebase", "--", &target], "Rebase completed.")?;
            }
            GitAction::Checkout(branch) => {
                self.ensure_clean("Checkout")?;
                if branch.trim().is_empty() {
                    bail!("A branch name is required.");
                }
                self.run_git_operation(&["checkout", &branch, "--"], "Checkout completed.")?;
            }
            GitAction::ShowDiff => self.diff_selected()?,
            GitAction::InspectSelected => {
                let Some(commit) = self.selected_commit().cloned() else {
                    bail!("No commit selected.");
                };
                self.inspect_commit(&commit.id)?;
            }
            GitAction::LineHistorySelected | GitAction::FileHistorySelected => {
                let is_line_history = matches!(action, GitAction::LineHistorySelected);
                let Some(change) = self.selected_change().cloned() else {
                    bail!("No file selected.");
                };
                if is_line_history {
                    self.load_line_history(&change.path)?;
                } else {
                    self.load_file_history(&change.path)?;
                }
            }
        }
        Ok(())
    }

    fn open_repository(&self) -> Result<Option<Repository>> {
        match Repository::discover(&self.root) {
            Ok(repo) => Ok(Some(repo)),
            Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn require_repository(&self) -> Result<Repository> {
        self.open_repository()?
            .ok_or_else(|| anyhow!("Not a Git repository: {}", self.root.display()))
    }

    pub(super) fn require_worktree_repository(&self) -> Result<Repository> {
        let repo = self.require_repository()?;
        if repo.workdir().is_none() {
            bail!("This operation requires a non-bare Git repository.");
        }
        Ok(repo)
    }

    fn clear_repository_data(&mut self) {
        self.repository_root = None;
        self.status = WorkingTreeStatus::default();
        self.commits.clear();
        self.branches.clear();
        self.launchpad = LaunchpadSummary::default();
        self.health = HealthInfo::default();
        self.diff = DiffView::default();
        self.inspected_commit = None;
        self.line_history.clear();
        self.file_history.clear();
        self.visual_file_history.clear();
        self.line_history_path = None;
        self.file_history_path = None;
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use super::*;
    use git2::{IndexAddOption, ObjectType, Oid, Signature};
    use tempfile::TempDir;

    fn write(root: &Path, path: &str, content: &str) {
        let destination = root.join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(destination, content).unwrap();
    }

    fn init_repo() -> (TempDir, Repository) {
        let directory = TempDir::new().unwrap();
        let repo = Repository::init(directory.path()).unwrap();
        repo.set_head("refs/heads/main").unwrap();
        repo.config()
            .unwrap()
            .set_str("user.name", "Test User")
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "test@example.com")
            .unwrap();
        (directory, repo)
    }

    fn commit_all(repo: &Repository, message: &str) -> Oid {
        let mut index = repo.index().unwrap();
        index.add_all(["."], IndexAddOption::DEFAULT, None).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = Signature::now("Test User", "test@example.com").unwrap();
        let parent = head_commit(repo).unwrap();
        let parents: Vec<_> = parent.iter().collect();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )
        .unwrap()
    }

    fn checkout(repo: &Repository, branch: &str) {
        let reference = format!("refs/heads/{branch}");
        let commit = repo
            .find_reference(&reference)
            .unwrap()
            .peel(ObjectType::Commit)
            .unwrap();
        repo.checkout_tree(&commit, None).unwrap();
        repo.set_head(&reference).unwrap();
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn refresh_outside_repository_is_safe() {
        let directory = TempDir::new().unwrap();
        let mut panel = GitPanel::new(directory.path());
        assert!(!panel.in_repository());
        assert!(panel.status.is_clean());
        assert_eq!(panel.status_message, "Not a Git repository.");
        panel.refresh().unwrap();
        panel.select_next();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn status_diff_stage_unstage_and_commit_work() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "first\n");
        let mut panel = GitPanel::new(directory.path());
        assert!(panel.last_error.is_none(), "{:?}", panel.last_error);
        assert_eq!(panel.status.untracked.len(), 1);
        panel.diff_selected().unwrap();
        assert!(panel.diff.text.contains("+first"));

        panel.stage_selected().unwrap();
        assert_eq!(panel.status.staged.len(), 1);
        assert!(panel.status.untracked.is_empty());
        panel.select(0);
        panel.diff_selected().unwrap();
        assert_eq!(panel.diff.kind, DiffKind::Staged);
        panel.unstage_selected().unwrap();
        assert_eq!(panel.status.untracked.len(), 1);
        assert!(panel.status.unstaged.is_empty());

        panel.stage_all().unwrap();
        panel.commit("Initial commit").unwrap();
        assert!(panel.status.is_clean());
        assert_eq!(panel.commits.len(), 1);
        assert_eq!(
            panel.launchpad.last_commit.unwrap().summary,
            "Initial commit"
        );
        drop(repo);
    }

    #[test]
    fn staged_and_unstaged_versions_of_one_file_are_separate() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "base\n");
        commit_all(&repo, "Base");
        write(directory.path(), "file.txt", "staged\n");
        let mut panel = GitPanel::new(directory.path());
        panel.stage_all().unwrap();
        write(directory.path(), "file.txt", "working\n");
        panel.refresh().unwrap();
        assert_eq!(panel.status.staged.len(), 1);
        assert_eq!(panel.status.unstaged.len(), 1);
        assert_eq!(panel.status.staged[0].path, panel.status.unstaged[0].path);
    }

    #[test]
    fn selected_deletion_can_be_staged() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "base\n");
        commit_all(&repo, "Base");
        fs::remove_file(directory.path().join("file.txt")).unwrap();
        let mut panel = GitPanel::new(directory.path());
        assert_eq!(panel.status.unstaged[0].status, FileStatus::Deleted);
        panel.stage_selected().unwrap();
        assert_eq!(panel.status.staged[0].status, FileStatus::Deleted);
        assert!(panel.status.unstaged.is_empty());
        panel.commit("Delete file").unwrap();
        assert!(panel.status.is_clean());
    }

    #[test]
    fn commit_graph_and_inspect_include_parents_and_files() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "one\n");
        let first = commit_all(&repo, "First");
        write(directory.path(), "file.txt", "one\ntwo\n");
        let second = commit_all(&repo, "Second");
        let mut panel = GitPanel::new(directory.path());
        assert_eq!(panel.commits[0].id, second.to_string());
        assert_eq!(panel.commits[0].parent_ids, vec![first.to_string()]);
        panel.inspect_commit(&second.to_string()).unwrap();
        let details = panel.inspected_commit.as_ref().unwrap();
        assert_eq!(details.message, "Second");
        assert_eq!(details.files.len(), 1);
        assert_eq!(details.files[0].additions, 1);
        panel.diff_commit(&second.to_string()).unwrap();
        assert!(panel.diff.text.contains("+two"), "{:?}", panel.diff.text);
    }

    #[test]
    fn line_file_and_visual_history_are_loaded() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "one\n");
        commit_all(&repo, "First");
        write(directory.path(), "file.txt", "one\ntwo\n");
        commit_all(&repo, "Second");
        let mut panel = GitPanel::new(directory.path());
        panel.load_line_history("file.txt").unwrap();
        assert_eq!(panel.line_history.len(), 2);
        assert_eq!(panel.line_history[0].summary, "First");
        assert_eq!(panel.line_history[1].summary, "Second");
        panel.load_file_history("file.txt").unwrap();
        assert_eq!(panel.file_history.len(), 2);
        assert_eq!(panel.visual_file_history.len(), 2);
        assert_eq!(panel.visual_file_history[0].lane, 0);
        panel.refresh().unwrap();
        assert_eq!(panel.line_history.len(), 2);
        assert_eq!(panel.file_history.len(), 2);
    }

    #[test]
    fn branches_and_launchpad_describe_local_and_remote_references() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "base\n");
        let id = commit_all(&repo, "Base");
        let commit = repo.find_commit(id).unwrap();
        repo.branch("feature", &commit, false).unwrap();
        repo.remote("origin", "https://example.invalid/repo.git")
            .unwrap();
        let panel = GitPanel::new(directory.path());
        assert!(
            panel
                .branches
                .iter()
                .any(|branch| branch.name == "main" && branch.is_head)
        );
        assert!(panel.branches.iter().any(|branch| branch.name == "feature"));
        assert_eq!(panel.launchpad.current_branch.as_deref(), Some("main"));
        assert_eq!(panel.launchpad.remotes[0].name, "origin");
        assert!(!panel.health.has_upstream);
    }

    #[test]
    fn checkout_and_rebase_reject_a_dirty_worktree() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "base\n");
        let id = commit_all(&repo, "Base");
        repo.branch("feature", &repo.find_commit(id).unwrap(), false)
            .unwrap();
        let mut panel = GitPanel::new(directory.path());
        write(directory.path(), "file.txt", "dirty\n");
        assert!(panel.checkout("feature").is_err());
        assert!(panel.rebase("feature").is_err());
        assert_eq!(repo.head().unwrap().shorthand(), Some("main"));
        assert_eq!(
            fs::read_to_string(directory.path().join("file.txt")).unwrap(),
            "dirty\n"
        );
    }

    #[test]
    fn checkout_switches_clean_worktrees() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "base\n");
        let id = commit_all(&repo, "Base");
        repo.branch("feature", &repo.find_commit(id).unwrap(), false)
            .unwrap();
        let mut panel = GitPanel::new(directory.path());
        panel.checkout("feature").unwrap();
        assert_eq!(repo.head().unwrap().shorthand(), Some("feature"));
        assert_eq!(panel.launchpad.current_branch.as_deref(), Some("feature"));
        checkout(&repo, "main");
    }

    #[test]
    fn explicit_rebase_replays_the_current_branch() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "base\n");
        let id = commit_all(&repo, "Base");
        repo.branch("feature", &repo.find_commit(id).unwrap(), false)
            .unwrap();
        git(directory.path(), &["checkout", "feature"]);
        write(directory.path(), "feature.txt", "feature\n");
        commit_all(&repo, "Feature change");
        git(directory.path(), &["checkout", "main"]);
        write(directory.path(), "main.txt", "main\n");
        let main_id = commit_all(&repo, "Main change");
        git(directory.path(), &["checkout", "feature"]);

        let mut panel = GitPanel::new(directory.path());
        panel.rebase("main").unwrap();
        assert_eq!(repo.head().unwrap().shorthand(), Some("feature"));
        assert_eq!(panel.commits[0].summary, "Feature change");
        assert_eq!(panel.commits[0].parent_ids, [main_id.to_string()]);
        assert!(directory.path().join("main.txt").exists());
        assert!(directory.path().join("feature.txt").exists());
    }

    #[test]
    fn fetch_pull_push_and_ahead_behind_use_only_temp_repositories() {
        let directory = TempDir::new().unwrap();
        let bare_path = directory.path().join("remote.git");
        let bare = Repository::init_bare(&bare_path).unwrap();
        bare.set_head("refs/heads/main").unwrap();

        let seed_path = directory.path().join("seed");
        fs::create_dir(&seed_path).unwrap();
        let seed = Repository::init(&seed_path).unwrap();
        seed.set_head("refs/heads/main").unwrap();
        seed.config()
            .unwrap()
            .set_str("user.name", "Test User")
            .unwrap();
        seed.config()
            .unwrap()
            .set_str("user.email", "test@example.com")
            .unwrap();
        write(&seed_path, "file.txt", "base\n");
        commit_all(&seed, "Base");
        seed.remote("origin", bare_path.to_str().unwrap()).unwrap();
        git(&seed_path, &["push", "-u", "origin", "main"]);

        let clone_path = directory.path().join("clone");
        git(
            directory.path(),
            &[
                "clone",
                bare_path.to_str().unwrap(),
                clone_path.to_str().unwrap(),
            ],
        );
        let clone = Repository::open(&clone_path).unwrap();
        clone
            .config()
            .unwrap()
            .set_str("user.name", "Test User")
            .unwrap();
        clone
            .config()
            .unwrap()
            .set_str("user.email", "test@example.com")
            .unwrap();
        write(&clone_path, "file.txt", "remote\n");
        commit_all(&clone, "Remote change");
        git(&clone_path, &["push"]);

        let mut panel = GitPanel::new(&seed_path);
        panel.fetch().unwrap();
        assert_eq!(panel.health.behind, 1);
        panel.pull().unwrap();
        assert_eq!(panel.health.behind, 0);
        assert_eq!(
            fs::read_to_string(seed_path.join("file.txt"))
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["remote"]
        );

        write(&seed_path, "file.txt", "local\n");
        panel.stage_all().unwrap();
        panel.commit("Local change").unwrap();
        assert_eq!(panel.health.ahead, 1);
        panel.push().unwrap();
        assert_eq!(panel.health.ahead, 0);
        git(&clone_path, &["pull", "--ff-only"]);
        assert_eq!(
            fs::read_to_string(clone_path.join("file.txt"))
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["local"]
        );
        assert!(panel.last_output.is_some());
    }

    #[test]
    fn action_errors_are_visible_and_paths_cannot_escape_repository() {
        let (directory, _repo) = init_repo();
        let mut panel = GitPanel::new(directory.path());
        assert!(panel.commit("").is_err());
        assert!(panel.last_error.is_some());
        assert!(panel.load_file_history("../outside").is_err());
        assert!(panel.diff_commit("not-an-oid").is_err());
    }

    #[test]
    fn tab_and_selection_helpers_support_renderer_input() {
        let (directory, _repo) = init_repo();
        write(directory.path(), "a.txt", "a\n");
        write(directory.path(), "b.txt", "b\n");
        let mut panel = GitPanel::new(directory.path());
        assert_eq!(panel.entry_count(), 2);
        panel.select_next();
        assert_eq!(panel.selected_change().unwrap().path, Path::new("b.txt"));
        panel.select_previous();
        assert_eq!(panel.selected_change().unwrap().path, Path::new("a.txt"));
        panel.change_tab_by_index(9);
        assert_eq!(panel.active_tab, GitTab::Health);
        assert_eq!(GitTab::ALL.map(GitTab::label)[8], "Visual File History");
    }
}
