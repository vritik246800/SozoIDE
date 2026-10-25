use std::path::PathBuf;

use chrono::{DateTime, FixedOffset};
use git2::{Delta, Status};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd)]
pub enum GitTab {
    #[default]
    Changes,
    CommitGraph,
    Branches,
    Diff,
    Launchpad,
    Inspect,
    LineHistory,
    FileHistory,
    VisualFileHistory,
    Health,
}

impl GitTab {
    pub const ALL: [Self; 10] = [
        Self::Changes,
        Self::CommitGraph,
        Self::Branches,
        Self::Diff,
        Self::Launchpad,
        Self::Inspect,
        Self::LineHistory,
        Self::FileHistory,
        Self::VisualFileHistory,
        Self::Health,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Changes => "Changes",
            Self::CommitGraph => "Commit Graph",
            Self::Branches => "Branches",
            Self::Diff => "Diff",
            Self::Launchpad => "Launchpad",
            Self::Inspect => "Inspect",
            Self::LineHistory => "Line History",
            Self::FileHistory => "File History",
            Self::VisualFileHistory => "Visual File History",
            Self::Health => "Health",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeArea {
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
    Conflict,
    Unknown,
}

impl FileStatus {
    pub(super) fn from_delta(delta: Delta) -> Self {
        match delta {
            Delta::Added | Delta::Untracked => Self::Added,
            Delta::Modified => Self::Modified,
            Delta::Deleted => Self::Deleted,
            Delta::Renamed => Self::Renamed,
            Delta::Typechange => Self::TypeChanged,
            Delta::Conflicted => Self::Conflict,
            _ => Self::Unknown,
        }
    }

    pub(super) fn from_status(status: Status, staged: bool) -> Self {
        if status.is_conflicted() {
            return Self::Conflict;
        }
        let flags = if staged {
            [
                Status::INDEX_RENAMED,
                Status::INDEX_NEW,
                Status::INDEX_DELETED,
                Status::INDEX_TYPECHANGE,
                Status::INDEX_MODIFIED,
            ]
            .into_iter()
            .find(|flag| status.contains(*flag))
        } else {
            [
                Status::WT_RENAMED,
                Status::WT_NEW,
                Status::WT_DELETED,
                Status::WT_TYPECHANGE,
                Status::WT_MODIFIED,
            ]
            .into_iter()
            .find(|flag| status.contains(*flag))
        };
        match flags {
            Some(Status::INDEX_RENAMED | Status::WT_RENAMED) => Self::Renamed,
            Some(Status::INDEX_NEW | Status::WT_NEW) => Self::Added,
            Some(Status::INDEX_DELETED | Status::WT_DELETED) => Self::Deleted,
            Some(Status::INDEX_TYPECHANGE | Status::WT_TYPECHANGE) => Self::TypeChanged,
            Some(Status::INDEX_MODIFIED | Status::WT_MODIFIED) => Self::Modified,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChangeEntry {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub area: ChangeArea,
    pub status: FileStatus,
}

#[derive(Clone, Debug, Default)]
pub struct WorkingTreeStatus {
    pub staged: Vec<ChangeEntry>,
    pub unstaged: Vec<ChangeEntry>,
    pub untracked: Vec<ChangeEntry>,
}

impl WorkingTreeStatus {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.untracked.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub parent_ids: Vec<String>,
    pub summary: String,
    pub author: String,
    pub author_email: String,
    pub committed_at: DateTime<FixedOffset>,
}

#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    pub full_name: String,
    pub commit_id: Option<String>,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Clone, Debug)]
pub struct RemoteInfo {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct LaunchpadSummary {
    pub current_branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub last_commit: Option<CommitInfo>,
    pub remotes: Vec<RemoteInfo>,
}

#[derive(Clone, Debug)]
pub struct HealthInfo {
    pub is_clean: bool,
    pub has_upstream: bool,
    pub ahead: usize,
    pub behind: usize,
    pub conflicts: Vec<PathBuf>,
    pub operation: Option<String>,
}

impl Default for HealthInfo {
    fn default() -> Self {
        Self {
            is_clean: true,
            has_upstream: false,
            ahead: 0,
            behind: 0,
            conflicts: Vec::new(),
            operation: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffKind {
    #[default]
    None,
    Staged,
    Unstaged,
    Untracked,
    Commit,
}

#[derive(Clone, Debug)]
pub struct DiffView {
    pub title: String,
    pub text: String,
    pub path: Option<PathBuf>,
    pub commit_id: Option<String>,
    pub kind: DiffKind,
}

impl Default for DiffView {
    fn default() -> Self {
        Self {
            title: "Diff".to_string(),
            text: String::new(),
            path: None,
            commit_id: None,
            kind: DiffKind::None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommitFile {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub status: FileStatus,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone, Debug)]
pub struct CommitDetails {
    pub commit: CommitInfo,
    pub message: String,
    pub files: Vec<CommitFile>,
}

#[derive(Clone, Debug)]
pub struct BlameLine {
    pub line_number: usize,
    pub original_line_number: usize,
    pub commit_id: String,
    pub short_id: String,
    pub author: String,
    pub summary: String,
    pub committed_at: DateTime<FixedOffset>,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct FileHistoryEntry {
    pub commit: CommitInfo,
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub status: FileStatus,
}

#[derive(Clone, Debug)]
pub struct VisualHistoryEntry {
    pub commit_id: String,
    pub short_id: String,
    pub summary: String,
    pub committed_at: DateTime<FixedOffset>,
    pub lane: usize,
    pub parent_ids: Vec<String>,
    pub status: FileStatus,
}

#[derive(Clone, Debug)]
pub struct GitCommandOutput {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

#[derive(Clone, Debug)]
pub enum GitAction {
    Refresh,
    StageSelected,
    UnstageSelected,
    StageAll,
    Commit(String),
    Fetch,
    Pull,
    Push,
    Rebase(String),
    Checkout(String),
    ShowDiff,
    InspectSelected,
    LineHistorySelected,
    FileHistorySelected,
}
