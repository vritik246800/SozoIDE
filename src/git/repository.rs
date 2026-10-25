use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use git2::{
    BranchType, DiffFormat, DiffOptions, ObjectType, Oid, Patch, Repository, RepositoryState, Sort,
    Status, StatusOptions,
};

use super::model::*;

const COMMIT_LIMIT: usize = 200;
const FILE_HISTORY_LIMIT: usize = 100;

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
        let full_name = branch
            .get()
            .name()
            .unwrap_or("<invalid reference name>")
            .to_string();
        let commit_id = branch.get().target().map(|id| id.to_string());
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
            full_name,
            commit_id,
            is_head: branch.is_head(),
            is_remote: kind == BranchType::Remote,
            upstream,
            ahead,
            behind,
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
    status: &WorkingTreeStatus,
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
        staged_count: status.staged.len(),
        unstaged_count: status.unstaged.len(),
        untracked_count: status.untracked.len(),
        last_commit,
        remotes,
    })
}

pub(super) fn read_health(
    repo: &Repository,
    status: &WorkingTreeStatus,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
) -> Result<HealthInfo> {
    let mut conflicts: Vec<_> = status
        .unstaged
        .iter()
        .filter(|change| change.status == FileStatus::Conflict)
        .map(|change| change.path.clone())
        .collect();
    conflicts.sort();
    conflicts.dedup();
    Ok(HealthInfo {
        is_clean: status.is_clean(),
        has_upstream: upstream.is_some(),
        ahead,
        behind,
        conflicts,
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
        DiffKind::Unstaged | DiffKind::Untracked => {
            repo.diff_tree_to_workdir_with_index(None, Some(&mut options))?
        }
        DiffKind::None | DiffKind::Commit => bail!("Invalid working-tree diff type."),
    };
    let text = diff_text(&diff)?;
    let title = match kind {
        DiffKind::Staged => format!("Staged: {}", path.display()),
        DiffKind::Untracked => format!("Untracked: {}", path.display()),
        _ => format!("Unstaged: {}", path.display()),
    };
    Ok(DiffView {
        title,
        text,
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
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
    Ok(DiffView {
        title: format!("Commit: {}", commit.id()),
        text: diff_text(&diff)?,
        path: None,
        commit_id: Some(commit.id().to_string()),
        kind: DiffKind::Commit,
    })
}

pub(super) fn diff_text(diff: &git2::Diff<'_>) -> Result<String> {
    let mut bytes = Vec::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        if matches!(line.origin(), '+' | '-' | ' ' | '\\') {
            bytes.push(line.origin() as u8);
        }
        bytes.extend_from_slice(line.content());
        true
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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

pub(super) fn read_line_history(repo: &Repository, path: &Path) -> Result<Vec<BlameLine>> {
    validate_repo_path(path)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow!("Line history requires a non-bare Git repository."))?;
    let content = fs::read(workdir.join(path))
        .with_context(|| format!("Could not read {} for line history.", path.display()))?;
    let content = String::from_utf8_lossy(&content);
    let lines: Vec<_> = content.lines().collect();
    let blame = repo.blame_file(path, None)?;
    let mut result = Vec::new();
    for hunk in blame.iter() {
        let id = hunk.final_commit_id();
        let commit = repo.find_commit(id).ok();
        let info = commit.as_ref().map(commit_info);
        let signature = hunk.final_signature();
        for offset in 0..hunk.lines_in_hunk() {
            let line_number = hunk.final_start_line() + offset;
            result.push(BlameLine {
                line_number,
                original_line_number: hunk.orig_start_line() + offset,
                commit_id: id.to_string(),
                short_id: short_id(id),
                author: info
                    .as_ref()
                    .map(|info| info.author.clone())
                    .or_else(|| signature.name().map(ToString::to_string))
                    .unwrap_or_else(|| "Unknown".to_string()),
                summary: info
                    .as_ref()
                    .map(|info| info.summary.clone())
                    .unwrap_or_default(),
                committed_at: info.as_ref().map_or_else(
                    || git_time_to_chrono(signature.when()),
                    |info| info.committed_at,
                ),
                content: lines
                    .get(line_number - 1)
                    .copied()
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    result.sort_by_key(|line| line.line_number);
    Ok(result)
}

pub(super) fn read_file_history(
    repo: &Repository,
    path: &Path,
) -> Result<(Vec<FileHistoryEntry>, Vec<VisualHistoryEntry>)> {
    validate_repo_path(path)?;
    if head_commit(repo)?.is_none() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TIME | Sort::TOPOLOGICAL)?;
    match walk.push_head() {
        Ok(()) => {}
        Err(error)
            if error.code() == git2::ErrorCode::NotFound
                || error.code() == git2::ErrorCode::UnbornBranch =>
        {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(error) => return Err(error.into()),
    }
    let mut history = Vec::new();
    for id in walk {
        let id = match id {
            Ok(id) => id,
            Err(error)
                if history.is_empty()
                    && (error.code() == git2::ErrorCode::NotFound
                        || error.code() == git2::ErrorCode::UnbornBranch) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        let commit = repo.find_commit(id)?;
        let tree = commit.tree()?;
        let parent_tree = if commit.parent_count() == 0 {
            None
        } else {
            Some(commit.parent(0)?.tree()?)
        };
        let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        diff.find_similar(None)?;
        for delta in diff.deltas() {
            let new_path = delta.new_file().path();
            let old_path = delta.old_file().path();
            if new_path != Some(path) && old_path != Some(path) {
                continue;
            }
            let entry_path = new_path.or(old_path).unwrap_or(path).to_path_buf();
            let previous_path =
                old_path.and_then(|old| (old != entry_path).then(|| old.to_path_buf()));
            let info = commit_info(&commit);
            history.push(FileHistoryEntry {
                commit: info,
                path: entry_path,
                old_path: previous_path,
                status: FileStatus::from_delta(delta.status()),
            });
            break;
        }
        if history.len() >= FILE_HISTORY_LIMIT {
            break;
        }
    }
    let visual = history
        .iter()
        .map(|entry| VisualHistoryEntry {
            commit_id: entry.commit.id.clone(),
            short_id: entry.commit.short_id.clone(),
            summary: entry.commit.summary.clone(),
            committed_at: entry.commit.committed_at,
            lane: 0,
            parent_ids: entry.commit.parent_ids.clone(),
            status: entry.status,
        })
        .collect();
    Ok((history, visual))
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
