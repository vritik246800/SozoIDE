use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use git2::{
    BranchType, DiffFormat, DiffOptions, ObjectType, Oid, Patch, Repository, RepositoryState, Sort,
    Status, StatusOptions,
};

use super::model::*;

const COMMIT_LIMIT: usize = 200;

pub(super) fn read_status(repo: &Repository) -> Result<WorkingTreeStatus> {
    if repo.workdir().is_none() {
        return Ok(WorkingTreeStatus::default());
    }
    let has_head = head_commit(repo)?.is_some();
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(has_head)
        .renames_index_to_workdir(true);
    let statuses = repo.statuses(Some(&mut options))?;
    let mut result = WorkingTreeStatus::default();

    for entry in statuses.iter() {
        let status = entry.status();
        let delta = if status.intersects(
            Status::INDEX_NEW
                | Status::INDEX_MODIFIED
                | Status::INDEX_DELETED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE,
        ) {
            entry.head_to_index()
        } else {
            entry.index_to_workdir()
        };
        let path = delta
            .as_ref()
            .and_then(|delta| delta.new_file().path().or_else(|| delta.old_file().path()))
            .map(Path::to_path_buf)
            .or_else(|| entry.path().map(PathBuf::from));
        let Some(path) = path else {
            continue;
        };
        let old_path = delta.as_ref().and_then(|delta| {
            let old = delta.old_file().path()?;
            (old != path).then(|| old.to_path_buf())
        });

        if status.is_conflicted() {
            result.unstaged.push(ChangeEntry {
                path,
                old_path,
                area: ChangeArea::Unstaged,
                status: FileStatus::Conflict,
            });
            continue;
        }
        if status.intersects(
            Status::INDEX_NEW
                | Status::INDEX_MODIFIED
                | Status::INDEX_DELETED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE,
        ) {
            result.staged.push(ChangeEntry {
                path: path.clone(),
                old_path: old_path.clone(),
                area: ChangeArea::Staged,
                status: FileStatus::from_status(status, true),
            });
        }
        if status.contains(Status::WT_NEW) {
            result.untracked.push(ChangeEntry {
                path,
                old_path,
                area: ChangeArea::Untracked,
                status: FileStatus::Added,
            });
        } else if status.intersects(
            Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE,
        ) {
            result.unstaged.push(ChangeEntry {
                path,
                old_path,
                area: ChangeArea::Unstaged,
                status: FileStatus::from_status(status, false),
            });
        }
    }

    let by_path = |left: &ChangeEntry, right: &ChangeEntry| left.path.cmp(&right.path);
    result.staged.sort_by(by_path);
    result.unstaged.sort_by(by_path);
    result.untracked.sort_by(by_path);
    Ok(result)
}

pub(super) fn read_commits(repo: &Repository) -> Result<Vec<CommitInfo>> {
    if head_commit(repo)?.is_none() {
        return Ok(Vec::new());
    }
    let mut walk = match repo.revwalk() {
        Ok(walk) => walk,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    walk.set_sorting(Sort::TIME | Sort::TOPOLOGICAL)?;
    match walk.push_head() {
        Ok(()) => {}
        Err(error)
            if error.code() == git2::ErrorCode::NotFound
                || error.code() == git2::ErrorCode::UnbornBranch =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.into()),
    }
    let mut commits = Vec::new();
    for id in walk.take(COMMIT_LIMIT) {
        let id = match id {
            Ok(id) => id,
            Err(error)
                if commits.is_empty()
                    && (error.code() == git2::ErrorCode::NotFound
                        || error.code() == git2::ErrorCode::UnbornBranch) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        let commit = repo.find_commit(id)?;
        commits.push(commit_info(&commit));
    }
    Ok(commits)
}

pub(super) fn read_branches(repo: &Repository) -> Result<Vec<BranchInfo>> {
    let mut result = Vec::new();
    let branches = match repo.branches(None) {
        Ok(branches) => branches,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(result),
        Err(error) => return Err(error.into()),
    };
    for branch in branches {
        let (branch, kind) = branch?;
        let name = branch
            .name()?
            .unwrap_or("<invalid branch name>")
            .to_string();
        // `origin/HEAD` only points at another remote branch.
        if kind == BranchType::Remote && name.ends_with("/HEAD") {
            continue;
        }
        let commit_id = branch.get().target().map(|id| id.to_string());
        let commit = branch.get().peel_to_commit().ok();
        let upstream = if kind == BranchType::Local {
            branch
                .upstream()
                .ok()
                .and_then(|upstream| upstream.name().ok().flatten().map(ToString::to_string))
        } else {
            None
        };
        let (ahead, behind) = match (branch.get().target(), upstream.as_deref()) {
            (Some(local), Some(upstream_name)) => {
                let upstream_ref = repo.find_reference(&format!("refs/remotes/{upstream_name}"));
                match upstream_ref.ok().and_then(|reference| reference.target()) {
                    Some(remote) => repo.graph_ahead_behind(local, remote)?,
                    None => (0, 0),
                }
            }
            _ => (0, 0),
        };
        result.push(BranchInfo {
            name,
            commit_id,
            is_head: branch.is_head(),
            is_remote: kind == BranchType::Remote,
            upstream,
            ahead,
            behind,
            summary: commit
                .as_ref()
                .and_then(|commit| commit.summary().map(ToString::to_string))
                .unwrap_or_default(),
            committed_at: commit
                .as_ref()
                .map(|commit| git_time_to_chrono(commit.time())),
        });
    }
    result.sort_by(|left, right| {
        left.is_remote
            .cmp(&right.is_remote)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(result)
}

pub(super) fn read_launchpad(
    repo: &Repository,
    last_commit: Option<CommitInfo>,
) -> Result<LaunchpadSummary> {
    let (current_branch, upstream, ahead, behind) = current_branch_position(repo)?;
    let mut remotes = Vec::new();
    for name in repo.remotes()?.iter().flatten() {
        let url = repo
            .find_remote(name)
            .ok()
            .and_then(|remote| remote.url().map(ToString::to_string));
        remotes.push(RemoteInfo {
            name: name.to_string(),
            url,
        });
    }
    remotes.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(LaunchpadSummary {
        current_branch,
        upstream,
        ahead,
        behind,
        last_commit,
        remotes,
        operation: repository_operation(repo.state()).map(ToString::to_string),
    })
}

pub(super) fn current_branch_position(
    repo: &Repository,
) -> Result<(Option<String>, Option<String>, usize, usize)> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(error)
            if error.code() == git2::ErrorCode::NotFound
                || error.code() == git2::ErrorCode::UnbornBranch =>
        {
            return Ok((None, None, 0, 0));
        }
        Err(error) => return Err(error.into()),
    };
    if !head.is_branch() {
        return Ok((None, None, 0, 0));
    }
    let name = head.shorthand().map(ToString::to_string);
    let local_id = head.target();
    let branch = git2::Branch::wrap(head);
    let upstream = branch
        .upstream()
        .ok()
        .and_then(|upstream| upstream.name().ok().flatten().map(ToString::to_string));
    let upstream_id = branch
        .upstream()
        .ok()
        .and_then(|upstream| upstream.get().target());
    let (ahead, behind) = match (local_id, upstream_id) {
        (Some(local), Some(remote)) => repo.graph_ahead_behind(local, remote)?,
        _ => (0, 0),
    };
    Ok((name, upstream, ahead, behind))
}

pub(super) fn repository_operation(state: RepositoryState) -> Option<&'static str> {
    match state {
        RepositoryState::Clean => None,
        RepositoryState::Merge => Some("Merge in progress"),
        RepositoryState::Revert | RepositoryState::RevertSequence => Some("Revert in progress"),
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => {
            Some("Cherry-pick in progress")
        }
        RepositoryState::Bisect => Some("Bisect in progress"),
        RepositoryState::Rebase
        | RepositoryState::RebaseInteractive
        | RepositoryState::RebaseMerge => Some("Rebase in progress"),
        RepositoryState::ApplyMailbox | RepositoryState::ApplyMailboxOrRebase => {
            Some("Apply mailbox in progress")
        }
    }
}

pub(super) fn read_change_diff(repo: &Repository, path: &Path, kind: DiffKind) -> Result<DiffView> {
    validate_repo_path(path)?;
    if repo.workdir().is_none() {
        bail!("Working-tree diffs require a non-bare Git repository.");
    }
    let mut options = DiffOptions::new();
    options
        .pathspec(path)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
    let diff = match kind {
        DiffKind::Staged => {
            let head = head_commit(repo)?;
            let tree = head.as_ref().map(|commit| commit.tree()).transpose()?;
            repo.diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?
        }
        // The index against the working tree: an empty tree made every line look added (E23).
        DiffKind::Unstaged | DiffKind::Untracked => {
            repo.diff_index_to_workdir(None, Some(&mut options))?
        }
        DiffKind::None | DiffKind::Commit => bail!("Invalid working-tree diff type."),
    };
    let files = diff_files(&diff)?;
    let title = match kind {
        DiffKind::Staged => format!("Staged: {}", path.display()),
        DiffKind::Untracked => format!("Untracked: {}", path.display()),
        _ => format!("Unstaged: {}", path.display()),
    };
    Ok(DiffView {
        title,
        files,
        path: Some(path.to_path_buf()),
        commit_id: None,
        kind,
    })
}

pub(super) fn read_commit_diff(repo: &Repository, commit_id: &str) -> Result<DiffView> {
    let commit = find_commit(repo, commit_id)?;
    let tree = commit.tree()?;
    let parent_tree = if commit.parent_count() == 0 {
        None
    } else {
        Some(commit.parent(0)?.tree()?)
    };
    let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
    diff.find_similar(None)?;
    Ok(DiffView {
        title: format!("Commit {}", short_id(commit.id())),
        files: diff_files(&diff)?,
        path: None,
        commit_id: Some(commit.id().to_string()),
        kind: DiffKind::Commit,
    })
}

/// The files of `diff`, each with its hunks and their lines with the old and new numbers.
pub(super) fn diff_files(diff: &git2::Diff<'_>) -> Result<Vec<DiffFile>> {
    let mut files: Vec<DiffFile> = Vec::new();
    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .unwrap_or_else(|| Path::new("<unknown>"));
        if files.last().is_none_or(|file| file.path != path) {
            files.push(DiffFile {
                path: path.to_path_buf(),
                old_path: delta
                    .old_file()
                    .path()
                    .filter(|old| *old != path)
                    .map(Path::to_path_buf),
                status: FileStatus::from_delta(delta.status()),
                binary: delta.flags().is_binary(),
                hunks: Vec::new(),
            });
        }
        let Some(file) = files.last_mut() else {
            return true;
        };
        let text = String::from_utf8_lossy(line.content())
            .trim_end_matches(['\n', '\r'])
            .to_owned();
        let kind = match line.origin() {
            'H' => {
                file.hunks.push(DiffHunk {
                    header: text,
                    lines: Vec::new(),
                });
                return true;
            }
            'B' => {
                file.binary = true;
                return true;
            }
            '+' => DiffLineKind::Added,
            '-' => DiffLineKind::Removed,
            ' ' => DiffLineKind::Context,
            '=' | '>' | '<' => DiffLineKind::Note,
            _ => return true,
        };
        if let Some(hunk) = file.hunks.last_mut() {
            hunk.lines.push(DiffLine {
                kind,
                old: line.old_lineno(),
                new: line.new_lineno(),
                text: if kind == DiffLineKind::Note {
                    "No newline at end of file".to_owned()
                } else {
                    text
                },
            });
        }
        true
    })?;
    Ok(files)
}

pub(super) fn read_commit_details(repo: &Repository, commit_id: &str) -> Result<CommitDetails> {
    let commit = find_commit(repo, commit_id)?;
    let info = commit_info(&commit);
    let message = commit.message().unwrap_or("").to_string();
    let tree = commit.tree()?;
    let parent_tree = if commit.parent_count() == 0 {
        None
    } else {
        Some(commit.parent(0)?.tree()?)
    };
    let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
    diff.find_similar(None)?;
    let mut files = Vec::new();
    for (index, delta) in diff.deltas().enumerate() {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .unwrap_or_else(|| Path::new("<unknown>"))
            .to_path_buf();
        let old_path = delta
            .old_file()
            .path()
            .and_then(|old| (old != path).then(|| old.to_path_buf()));
        let (additions, deletions) = Patch::from_diff(&diff, index)?
            .and_then(|patch| patch.line_stats().ok())
            .map_or((0, 0), |(_, additions, deletions)| (additions, deletions));
        files.push(CommitFile {
            path,
            old_path,
            status: FileStatus::from_delta(delta.status()),
            additions,
            deletions,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(CommitDetails {
        commit: info,
        message,
        files,
    })
}

pub(super) fn commit_info(commit: &git2::Commit<'_>) -> CommitInfo {
    let signature = commit.author();
    CommitInfo {
        id: commit.id().to_string(),
        short_id: short_id(commit.id()),
        parent_ids: commit.parent_ids().map(|id| id.to_string()).collect(),
        summary: commit.summary().unwrap_or("").to_string(),
        author: signature.name().unwrap_or("Unknown").to_string(),
        author_email: signature.email().unwrap_or("").to_string(),
        committed_at: git_time_to_chrono(commit.time()),
    }
}

pub(super) fn git_time_to_chrono(time: git2::Time) -> DateTime<FixedOffset> {
    let offset = FixedOffset::east_opt(time.offset_minutes() * 60)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset is valid"));
    Utc.timestamp_opt(time.seconds(), 0)
        .single()
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
        .with_timezone(&offset)
}

pub(super) fn short_id(id: Oid) -> String {
    id.to_string().chars().take(7).collect()
}

pub(super) fn find_commit<'repo>(repo: &'repo Repository, id: &str) -> Result<git2::Commit<'repo>> {
    let id = Oid::from_str(id).with_context(|| format!("Invalid commit ID: {id}"))?;
    repo.find_commit(id)
        .with_context(|| format!("Commit not found: {id}"))
}

pub(super) fn head_commit(repo: &Repository) -> Result<Option<git2::Commit<'_>>> {
    match repo.head() {
        Ok(head) => {
            let id = head.peel(ObjectType::Commit)?.id();
            Ok(Some(repo.find_commit(id)?))
        }
        Err(error)
            if error.code() == git2::ErrorCode::NotFound
                || error.code() == git2::ErrorCode::UnbornBranch =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn validate_repo_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("A repository-relative path without '..' is required.");
    }
    Ok(())
}
