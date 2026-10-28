use std::path::PathBuf;

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
    pub diff: DiffView,
    /// Folded files and hunks of the diff, and whether it is side by side.
    pub diff_folds: DiffFolds,
    pub diff_split: bool,
    pub inspected_commit: Option<CommitDetails>,
    pub status_message: String,
    pub last_error: Option<String>,
    pub last_output: Option<GitCommandOutput>,
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
            diff: DiffView::default(),
            diff_folds: DiffFolds::new(),
            diff_split: false,
            inspected_commit: None,
            status_message: String::new(),
            last_error: None,
            last_output: None,
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
            GitTab::Changes | GitTab::Launchpad => self.changes().count(),
            GitTab::CommitGraph => self.commits.len(),
            GitTab::Branches => self.branches.len(),
            GitTab::Diff => self.diff_rows().len(),
            GitTab::Inspect => self
                .inspected_commit
                .as_ref()
                .map_or(0, |details| details.files.len()),
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

    /// Staged, then unstaged, then untracked changes: the order of Changes and Launchpad.
    pub fn changes(&self) -> impl Iterator<Item = &ChangeEntry> {
        self.status
            .staged
            .iter()
            .chain(&self.status.unstaged)
            .chain(&self.status.untracked)
    }

    pub fn selected_change(&self) -> Option<&ChangeEntry> {
        self.changes().nth(self.selected)
    }

    pub fn selected_commit(&self) -> Option<&CommitInfo> {
        self.commits.get(self.selected)
    }

    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.branches.get(self.selected)
    }

    /// The rows of the Diff tab as they are folded and laid out now.
    pub fn diff_rows(&self) -> Vec<DiffRow> {
        self.diff.rows(&self.diff_folds, self.diff_split)
    }

    /// The change the diff shows, by its place in `changes`.
    pub fn diff_change(&self) -> Option<usize> {
        let path = self.diff.path.as_ref()?;
        let area = match self.diff.kind {
            DiffKind::Staged => ChangeArea::Staged,
            DiffKind::Unstaged => ChangeArea::Unstaged,
            DiffKind::Untracked => ChangeArea::Untracked,
            DiffKind::None | DiffKind::Commit => return None,
        };
        self.changes()
            .position(|change| change.path == *path && change.area == area)
    }

    /// Folds or unfolds the file or hunk of diff row `index`.
    pub fn toggle_fold(&mut self, index: usize) {
        let Some(row) = self.diff_rows().get(index).copied() else {
            return;
        };
        let key = (row.file(), row.hunk());
        if !self.diff_folds.remove(&key) {
            self.diff_folds.insert(key);
        }
        // The cursor stays on the row that was folded.
        let rows = self.diff_rows();
        self.selected = rows
            .iter()
            .position(|other| {
                (other.file(), other.hunk()) == key
                    && matches!(other, DiffRow::File(_) | DiffRow::Hunk(..))
            })
            .unwrap_or(0);
    }

    /// The next (`forward`) or previous file or hunk header in the diff.
    pub fn jump_hunk(&mut self, forward: bool) {
        let rows = self.diff_rows();
        let header = |index: &usize| matches!(rows[*index], DiffRow::Hunk(..) | DiffRow::File(_));
        let target = if forward {
            (self.selected + 1..rows.len()).find(header)
        } else {
            (0..self.selected).rev().find(header)
        };
        if let Some(target) = target {
            self.selected = target;
        }
    }

    /// Unified or side by side; the cursor keeps its hunk.
    pub fn toggle_split(&mut self) {
        let row = self.diff_rows().get(self.selected).copied();
        self.diff_split = !self.diff_split;
        if let Some(row) = row {
            let key = (row.file(), row.hunk());
            self.selected = self
                .diff_rows()
                .iter()
                .position(|other| (other.file(), other.hunk()) == key)
                .unwrap_or(0);
        }
    }

    fn show_diff(&mut self, diff: DiffView) {
        self.diff = diff;
        self.diff_folds.clear();
        self.selected = 0;
    }

    pub fn refresh(&mut self) -> Result<()> {
        let previous_diff = self.diff.clone();
        let inspected_id = self
            .inspected_commit
            .as_ref()
            .map(|details| details.commit.id.clone());

        let Some(repo) = self.open_repository()? else {
            self.clear_repository_data();
            self.status_message = "Not a Git repository.".to_string();
            return Ok(());
        };

        let repository_root = repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf();
        let status = read_status(&repo).context("Could not read working-tree status.")?;
        let commits = read_commits(&repo).context("Could not read commits.")?;
        let branches = read_branches(&repo).context("Could not read branches.")?;
        let launchpad = read_launchpad(&repo, commits.first().cloned())
            .context("Could not read launchpad summary.")?;

        self.repository_root = Some(repository_root);
        self.status = status;
        self.commits = commits;
        self.branches = branches;
        self.launchpad = launchpad;

        if let Some(id) = inspected_id {
            self.inspected_commit = Some(read_commit_details(&repo, &id)?);
        }
        let diff = match (
            previous_diff.kind,
            previous_diff.path,
            previous_diff.commit_id,
        ) {
            (DiffKind::Commit, _, Some(id)) => read_commit_diff(&repo, &id)?,
            (kind, Some(path), _) if kind != DiffKind::None => {
                read_change_diff(&repo, &path, kind)?
            }
            _ => DiffView::default(),
        };
        // The same diff keeps its folds; another one starts unfolded.
        if diff.files.len() != self.diff.files.len() {
            self.diff_folds.clear();
        }
        self.diff = diff;
        self.select(self.selected);
        Ok(())
    }

    pub fn diff_selected(&mut self) -> Result<()> {
        let Some(change) = self.selected_change().cloned() else {
            self.diff = DiffView::default();
            self.status_message = "No change selected.".to_string();
            return Ok(());
        };
        self.diff_change_entry(&change)
    }

    fn diff_change_entry(&mut self, change: &ChangeEntry) -> Result<()> {
        let kind = match change.area {
            ChangeArea::Staged => DiffKind::Staged,
            ChangeArea::Unstaged => DiffKind::Unstaged,
            ChangeArea::Untracked => DiffKind::Untracked,
        };
        let repo = self.require_repository()?;
        let diff = read_change_diff(&repo, &change.path, kind)?;
        self.show_diff(diff);
        Ok(())
    }

    pub fn diff_commit(&mut self, commit_id: &str) -> Result<()> {
        let repo = self.require_repository()?;
        let diff = read_commit_diff(&repo, commit_id)?;
        self.show_diff(diff);
        Ok(())
    }

    pub fn inspect_commit(&mut self, commit_id: &str) -> Result<()> {
        let repo = self.require_repository()?;
        self.inspected_commit = Some(read_commit_details(&repo, commit_id)?);
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
            GitAction::StageSelected => {
                let change = self.selected_change().cloned();
                self.stage_change(change.ok_or_else(|| anyhow!("No change selected."))?)?;
            }
            GitAction::UnstageSelected => {
                let change = self.selected_change().cloned();
                self.unstage_change(change.ok_or_else(|| anyhow!("No change selected."))?)?;
            }
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
                // A remote branch is checked out as the local branch that follows it.
                let remote = self
                    .branches
                    .iter()
                    .find(|other| other.is_remote && other.name == branch)
                    .and_then(|remote| remote.name.split_once('/'))
                    .map(|(_, local)| local.to_owned());
                match remote {
                    Some(local) if !self.branches.iter().any(|other| other.name == local) => self
                        .run_git_operation(
                        &["checkout", "--track", &branch, "--"],
                        "Checkout completed.",
                    )?,
                    Some(local) => {
                        self.run_git_operation(&["checkout", &local, "--"], "Checkout completed.")?
                    }
                    None => {
                        self.run_git_operation(&["checkout", &branch, "--"], "Checkout completed.")?
                    }
                }
            }
            GitAction::ShowDiff => self.diff_selected()?,
            GitAction::InspectSelected => {
                let Some(commit) = self.selected_commit().cloned() else {
                    bail!("No commit selected.");
                };
                self.inspect_commit(&commit.id)?;
            }
            GitAction::StageDiff | GitAction::UnstageDiff => {
                let stage = matches!(action, GitAction::StageDiff);
                let change = self
                    .diff_change()
                    .and_then(|index| self.changes().nth(index).cloned())
                    .ok_or_else(|| {
                        anyhow!("The diff does not show a change of the working tree.")
                    })?;
                let path = change.path.clone();
                if stage {
                    self.stage_change(change)?;
                } else {
                    self.unstage_change(change)?;
                }
                // The diff follows the file to where it went.
                let areas = if stage {
                    [ChangeArea::Staged; 2]
                } else {
                    [ChangeArea::Unstaged, ChangeArea::Untracked]
                };
                let moved = self
                    .changes()
                    .find(|other| other.path == path && areas.contains(&other.area))
                    .cloned();
                if let Some(moved) = moved {
                    self.diff_change_entry(&moved)?;
                }
            }
            GitAction::DiffNeighbour(forward) => {
                let count = self.changes().count();
                if count == 0 {
                    bail!("There are no changes.");
                }
                let next = match self.diff_change() {
                    Some(index) if forward => (index + 1) % count,
                    Some(index) => (index + count - 1) % count,
                    None => 0,
                };
                let change = self.changes().nth(next).cloned();
                if let Some(change) = change {
                    self.diff_change_entry(&change)?;
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
        self.diff = DiffView::default();
        self.diff_folds.clear();
        self.inspected_commit = None;
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use std::path::Path;

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
        assert!(added(&panel).contains(&"first".to_owned()));

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
        assert_eq!(added(&panel), ["two"]);
    }

    #[test]
    fn the_timeline_reads_every_branch_with_its_labels() {
        let (directory, repo) = init_repo();
        write(directory.path(), "file.txt", "base\n");
        let base = commit_all(&repo, "Base");
        repo.branch("feature/login", &repo.find_commit(base).unwrap(), false)
            .unwrap();
        repo.tag_lightweight("v1.0", &repo.find_object(base, None).unwrap(), false)
            .unwrap();
        write(directory.path(), "file.txt", "main\n");
        let main = commit_all(&repo, "On main");
        checkout(&repo, "feature/login");
        write(directory.path(), "login.txt", "login\n");
        let feature = commit_all(&repo, "Login");
        checkout(&repo, "main");
        let panel = GitPanel::new(directory.path());
        // The branch not checked out is in the timeline too, newest first.
        let ids = panel
            .commits
            .iter()
            .map(|commit| commit.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&feature.to_string()) && ids.contains(&main.to_string()));
        assert_eq!(ids[2], base.to_string());
        let labels = |id: Oid| {
            panel
                .commits
                .iter()
                .find(|commit| commit.id == id.to_string())
                .unwrap()
                .refs
                .clone()
        };
        assert_eq!(labels(main), ["HEAD -> main"]);
        assert_eq!(labels(feature), ["feature/login"]);
        assert_eq!(labels(base), ["tag: v1.0"]);
        assert_eq!(timeline(&panel.commits).len(), 3);
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
        assert!(panel.launchpad.upstream.is_none());
        assert!(panel.branches.iter().all(|branch| branch.summary == "Base"));
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
        assert_eq!(panel.launchpad.behind, 1);
        panel.pull().unwrap();
        assert_eq!(panel.launchpad.behind, 0);
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
        assert_eq!(panel.launchpad.ahead, 1);
        panel.push().unwrap();
        assert_eq!(panel.launchpad.ahead, 0);
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
        assert!(validate_repo_path(Path::new("../outside")).is_err());
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
        panel.change_tab_by_index(5);
        assert_eq!(panel.active_tab, GitTab::Inspect);
        assert_eq!(
            GitTab::ALL.map(GitTab::label),
            [
                "Changes",
                "Commit Graph",
                "Branches",
                "Diff",
                "Launchpad",
                "Inspect"
            ]
        );
    }

    fn added(panel: &GitPanel) -> Vec<String> {
        panel
            .diff
            .files
            .iter()
            .flat_map(|file| &file.hunks)
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind == DiffLineKind::Added)
            .map(|line| line.text.clone())
            .collect()
    }

    #[test]
    fn the_diff_numbers_its_lines_folds_and_stages_its_file() {
        let (directory, repo) = init_repo();
        write(directory.path(), "a.txt", "one\ntwo\nthree\n");
        write(directory.path(), "b.txt", "bee\n");
        commit_all(&repo, "Base");
        write(directory.path(), "a.txt", "one\n2\nthree\nfour\n");
        write(directory.path(), "b.txt", "BEE\n");
        let mut panel = GitPanel::new(directory.path());
        panel.execute(GitAction::ShowDiff).unwrap();
        let file = &panel.diff.files[0];
        assert_eq!(file.path, Path::new("a.txt"));
        assert_eq!(file.counts(), (2, 1));
        let numbers = file.hunks[0]
            .lines
            .iter()
            .map(|line| (line.kind, line.old, line.new))
            .collect::<Vec<_>>();
        assert_eq!(
            numbers,
            [
                (DiffLineKind::Context, Some(1), Some(1)),
                (DiffLineKind::Removed, Some(2), None),
                (DiffLineKind::Added, None, Some(2)),
                (DiffLineKind::Context, Some(3), Some(3)),
                (DiffLineKind::Added, None, Some(4)),
            ]
        );

        panel.active_tab = GitTab::Diff;
        assert_eq!(panel.entry_count(), 2 + 5);
        panel.jump_hunk(true);
        assert_eq!(panel.selected, 1, "the hunk header");
        panel.toggle_fold(1);
        assert_eq!((panel.entry_count(), panel.selected), (2, 1));
        panel.toggle_fold(1);
        panel.toggle_split();
        assert_eq!(
            panel.entry_count(),
            2 + 4,
            "the removed and the added line side by side"
        );

        // The next change, then staging from the diff: the diff follows the file.
        panel.execute(GitAction::DiffNeighbour(true)).unwrap();
        assert_eq!(panel.diff.path.as_deref(), Some(Path::new("b.txt")));
        panel.execute(GitAction::StageDiff).unwrap();
        assert_eq!(panel.status.staged[0].path, Path::new("b.txt"));
        assert_eq!(panel.diff.kind, DiffKind::Staged);
        assert_eq!(added(&panel), ["BEE"]);
        panel.execute(GitAction::UnstageDiff).unwrap();
        assert!(panel.status.staged.is_empty());
        assert_eq!(panel.diff.kind, DiffKind::Unstaged);
    }

    #[test]
    fn a_remote_branch_is_checked_out_as_a_local_one_that_follows_it() {
        let directory = TempDir::new().unwrap();
        let remote_path = directory.path().join("remote.git");
        Repository::init_bare(&remote_path).unwrap();
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
        seed.remote("origin", remote_path.to_str().unwrap())
            .unwrap();
        git(
            &seed_path,
            &["push", "origin", "main", "main:feature/remote-only"],
        );
        git(&seed_path, &["fetch", "origin"]);

        let mut panel = GitPanel::new(&seed_path);
        assert!(
            panel
                .branches
                .iter()
                .any(|branch| branch.name == "origin/feature/remote-only")
        );
        assert!(
            !panel
                .branches
                .iter()
                .any(|branch| branch.name.ends_with("/HEAD"))
        );
        panel.checkout("origin/feature/remote-only").unwrap();
        assert_eq!(
            panel.launchpad.current_branch.as_deref(),
            Some("feature/remote-only")
        );
        assert_eq!(
            panel.launchpad.upstream.as_deref(),
            Some("origin/feature/remote-only")
        );
    }
}
