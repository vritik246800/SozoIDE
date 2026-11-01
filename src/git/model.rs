use std::{collections::HashSet, path::PathBuf};

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
}

impl GitTab {
    pub const ALL: [Self; 6] = [
        Self::Changes,
        Self::CommitGraph,
        Self::Branches,
        Self::Diff,
        Self::Launchpad,
        Self::Inspect,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Changes => "Changes",
            Self::CommitGraph => "Commit Graph",
            Self::Branches => "Branches",
            Self::Diff => "Diff",
            Self::Launchpad => "Launchpad",
            Self::Inspect => "Inspect",
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
    /// Branches and tags that point here (timeline labels).
    pub refs: Vec<String>,
}

/// One row of the timeline graph (F06): two cells per lane, each a symbol and the colour
/// number of its lane (`None` for a blank cell).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimelineRow {
    pub cells: Vec<(char, Option<usize>)>,
}

impl TimelineRow {
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.cells
            .iter()
            .map(|(symbol, _)| symbol)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }
}

/// The graph of `commits` (newest first, each after its children): one lane per line of work
/// with its own colour; `●` a commit, `│` a lane passing by, `╮`/`╭` a merge bringing a parent
/// in from a lane, `╯`/`╰` a lane that started at this commit ending in it.
pub fn timeline(commits: &[CommitInfo]) -> Vec<TimelineRow> {
    // Each lane waits for a commit and keeps its colour.
    let mut lanes: Vec<Option<(String, usize)>> = Vec::new();
    let mut colours = 0;
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        let before = lanes
            .iter()
            .map(|lane| lane.as_ref().map(|(_, colour)| *colour))
            .collect::<Vec<_>>();
        let waiting = lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| lane.as_ref().is_some_and(|(id, _)| *id == commit.id))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let (lane, colour) = match waiting.first() {
            Some(&index) => (index, before[index].unwrap_or(0)),
            None => {
                colours += 1;
                (free_lane(&mut lanes), colours - 1)
            }
        };
        // Lanes that waited for this commit too end in it.
        let closing = waiting[waiting.len().min(1)..]
            .iter()
            .map(|&index| (index, before[index].unwrap_or(0)))
            .collect::<Vec<_>>();
        for &(index, _) in &closing {
            lanes[index] = None;
        }
        lanes[lane] = commit
            .parent_ids
            .first()
            .map(|parent| (parent.clone(), colour));
        let mut opening = Vec::new();
        for parent in commit.parent_ids.iter().skip(1) {
            let existing = lanes
                .iter()
                .position(|other| other.as_ref().is_some_and(|(id, _)| id == parent));
            match existing {
                Some(index) => opening.push((index, lanes[index].as_ref().map_or(0, |(_, c)| *c))),
                None => {
                    let index = free_lane(&mut lanes);
                    lanes[index] = Some((parent.clone(), colours));
                    opening.push((index, colours));
                    colours += 1;
                }
            }
        }

        let width = lanes.len().max(before.len());
        let mut cells = vec![(' ', None); width * 2];
        for (index, colour) in before.iter().enumerate() {
            if let Some(colour) = colour
                && index != lane
                && !closing.iter().any(|(closed, _)| *closed == index)
            {
                cells[index * 2] = ('│', Some(*colour));
            }
        }
        let mut connect = |to: usize, colour: usize, symbol: char| {
            let (from, to_cell) = (lane * 2, to * 2);
            for cell in &mut cells[from.min(to_cell) + 1..from.max(to_cell)] {
                // Crossing a lane, or passing a nearer lane that ends or starts here.
                *cell = match *cell {
                    ('│', passing) => ('┼', passing),
                    ('╯' | '╰', own) => ('┴', own),
                    ('╮' | '╭', own) => ('┬', own),
                    _ => ('─', Some(colour)),
                };
            }
            cells[to_cell] = (symbol, Some(colour));
        };
        for &(index, colour) in &closing {
            connect(index, colour, if index > lane { '╯' } else { '╰' });
        }
        for &(index, colour) in &opening {
            let passing = before.get(index).copied().flatten().is_some();
            let symbol = match (index > lane, passing) {
                (true, true) => '┤',
                (true, false) => '╮',
                (false, true) => '├',
                (false, false) => '╭',
            };
            connect(index, colour, symbol);
        }
        cells[lane * 2] = ('●', Some(colour));
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(TimelineRow { cells });
    }
    rows
}

fn free_lane(lanes: &mut Vec<Option<(String, usize)>>) -> usize {
    lanes.iter().position(Option::is_none).unwrap_or_else(|| {
        lanes.push(None);
        lanes.len() - 1
    })
}

/// How a line of the editor differs from the file in the last commit (F34), for its margin:
/// added, modified, or the line under which (above which, at the top) lines were deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineMark {
    Added,
    Modified,
    DeletedBelow,
    DeletedAbove,
}

/// How a diff line changed, for the timeline's colours (Q38): a run of removed lines followed
/// by added ones is a modification, line by line; the rest were added or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineChange {
    Context,
    Added,
    Removed,
    Modified,
}

pub fn line_changes(lines: &[DiffLine]) -> Vec<LineChange> {
    let mut changes = lines
        .iter()
        .map(|line| match line.kind {
            DiffLineKind::Added => LineChange::Added,
            DiffLineKind::Removed => LineChange::Removed,
            DiffLineKind::Context | DiffLineKind::Note => LineChange::Context,
        })
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        let removed = lines[index..]
            .iter()
            .take_while(|line| line.kind == DiffLineKind::Removed)
            .count();
        let added = lines[index + removed..]
            .iter()
            .take_while(|line| line.kind == DiffLineKind::Added)
            .count();
        let paired = removed.min(added);
        for offset in (0..paired).chain(removed..removed + paired) {
            changes[index + offset] = LineChange::Modified;
        }
        index += (removed + added).max(1);
    }
    changes
}

#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    pub commit_id: Option<String>,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    /// The summary and date of the commit the branch points to.
    pub summary: String,
    pub committed_at: Option<DateTime<FixedOffset>>,
}

/// A row of the Branches tree: a group (local branches, a remote, a `/` folder) or a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchRow {
    /// A group at the top: the local branches or a remote.
    pub group: bool,
    /// For each level above this row, whether a line goes on down there (`│`).
    pub guides: Vec<bool>,
    /// The last row of its level (`└─`, otherwise `├─`); groups at the top have neither.
    pub last: bool,
    pub label: String,
    /// The branch (its index in the branches), none for a group.
    pub branch: Option<usize>,
}

/// The branches as a tree like the GitLens one: «Local» and one group per remote, then a
/// folder for each `/` in the names (`feature/a` and `feature/b` under `feature`). The
/// branches come sorted, local first, so the rows keep their order.
pub fn branch_tree(branches: &[BranchInfo]) -> Vec<BranchRow> {
    #[derive(Default)]
    struct Node {
        label: String,
        branch: Option<usize>,
        children: Vec<Node>,
    }
    fn insert(node: &mut Node, path: &[&str], branch: usize) {
        let Some((first, rest)) = path.split_first() else {
            return;
        };
        let position = node
            .children
            .iter()
            .position(|child| child.label == *first && child.branch.is_none() && !rest.is_empty());
        let index = match position {
            Some(index) => index,
            None => {
                node.children.push(Node {
                    label: (*first).to_owned(),
                    ..Node::default()
                });
                node.children.len() - 1
            }
        };
        if rest.is_empty() {
            node.children[index].branch = Some(branch);
        } else {
            insert(&mut node.children[index], rest, branch);
        }
    }
    fn rows(node: &Node, guides: &mut Vec<bool>, out: &mut Vec<BranchRow>) {
        for (index, child) in node.children.iter().enumerate() {
            let last = index + 1 == node.children.len();
            out.push(BranchRow {
                group: false,
                guides: guides.clone(),
                last,
                label: child.label.clone(),
                branch: child.branch,
            });
            guides.push(!last);
            rows(child, guides, out);
            guides.pop();
        }
    }
    let mut root = Node::default();
    for (index, branch) in branches.iter().enumerate() {
        let mut path = branch.name.split('/').collect::<Vec<_>>();
        if !branch.is_remote {
            path.insert(0, "Local");
        }
        insert(&mut root, &path, index);
    }
    let mut out = Vec::new();
    for group in &root.children {
        out.push(BranchRow {
            group: true,
            guides: Vec::new(),
            last: false,
            label: group.label.clone(),
            branch: None,
        });
        rows(group, &mut Vec::new(), &mut out);
    }
    out
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
    pub last_commit: Option<CommitInfo>,
    pub remotes: Vec<RemoteInfo>,
    /// A merge, rebase, cherry-pick… left in progress.
    pub operation: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
    /// «No newline at end of file».
    Note,
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    /// The line number before and after the change.
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug)]
pub struct DiffFile {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub status: FileStatus,
    pub binary: bool,
    pub hunks: Vec<DiffHunk>,
}

impl DiffFile {
    /// Lines added and removed.
    pub fn counts(&self) -> (usize, usize) {
        let lines = self.hunks.iter().flat_map(|hunk| &hunk.lines);
        lines.fold((0, 0), |(added, removed), line| match line.kind {
            DiffLineKind::Added => (added + 1, removed),
            DiffLineKind::Removed => (added, removed + 1),
            _ => (added, removed),
        })
    }
}

#[derive(Clone, Debug)]
pub struct DiffView {
    pub title: String,
    pub files: Vec<DiffFile>,
    pub path: Option<PathBuf>,
    pub commit_id: Option<String>,
    pub kind: DiffKind,
}

impl Default for DiffView {
    fn default() -> Self {
        Self {
            title: "Diff".to_string(),
            files: Vec::new(),
            path: None,
            commit_id: None,
            kind: DiffKind::None,
        }
    }
}

/// A row of the Diff tab: a file, a hunk, a line, or (side by side) the line before and the
/// line after the change, by their index in the hunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffRow {
    File(usize),
    Hunk(usize, usize),
    Line(usize, usize, usize),
    Pair(usize, usize, Option<usize>, Option<usize>),
}

impl DiffRow {
    pub fn file(self) -> usize {
        match self {
            Self::File(file)
            | Self::Hunk(file, _)
            | Self::Line(file, ..)
            | Self::Pair(file, ..) => file,
        }
    }

    pub fn hunk(self) -> Option<usize> {
        match self {
            Self::File(_) => None,
            Self::Hunk(_, hunk) | Self::Line(_, hunk, _) | Self::Pair(_, hunk, ..) => Some(hunk),
        }
    }
}

/// What is folded in the Diff tab: a whole file (`None`) or one of its hunks.
pub type DiffFolds = HashSet<(usize, Option<usize>)>;

impl DiffView {
    pub fn counts(&self) -> (usize, usize) {
        self.files
            .iter()
            .map(DiffFile::counts)
            .fold((0, 0), |total, counts| {
                (total.0 + counts.0, total.1 + counts.1)
            })
    }

    /// The rows on screen: a folded file or hunk hides what is under it; `split` pairs the
    /// removed lines of a hunk with the added lines after them.
    pub fn rows(&self, folds: &DiffFolds, split: bool) -> Vec<DiffRow> {
        let mut rows = Vec::new();
        for (file_index, file) in self.files.iter().enumerate() {
            rows.push(DiffRow::File(file_index));
            if folds.contains(&(file_index, None)) {
                continue;
            }
            for (hunk_index, hunk) in file.hunks.iter().enumerate() {
                rows.push(DiffRow::Hunk(file_index, hunk_index));
                if folds.contains(&(file_index, Some(hunk_index))) {
                    continue;
                }
                if !split {
                    rows.extend(
                        (0..hunk.lines.len())
                            .map(|line| DiffRow::Line(file_index, hunk_index, line)),
                    );
                    continue;
                }
                let mut index = 0;
                while index < hunk.lines.len() {
                    let removed = (index..hunk.lines.len())
                        .take_while(|line| hunk.lines[*line].kind == DiffLineKind::Removed)
                        .collect::<Vec<_>>();
                    let after = index + removed.len();
                    let added = (after..hunk.lines.len())
                        .take_while(|line| hunk.lines[*line].kind == DiffLineKind::Added)
                        .collect::<Vec<_>>();
                    if removed.is_empty() && added.is_empty() {
                        rows.push(DiffRow::Pair(
                            file_index,
                            hunk_index,
                            Some(index),
                            Some(index),
                        ));
                        index += 1;
                        continue;
                    }
                    for pair in 0..removed.len().max(added.len()) {
                        rows.push(DiffRow::Pair(
                            file_index,
                            hunk_index,
                            removed.get(pair).copied(),
                            added.get(pair).copied(),
                        ));
                    }
                    index = after + added.len();
                }
            }
        }
        rows
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
    /// Stage or unstage the file of the Diff tab.
    StageDiff,
    UnstageDiff,
    /// The diff of the next (`true`) or previous change.
    DiffNeighbour(bool),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str, is_remote: bool) -> BranchInfo {
        BranchInfo {
            name: name.to_owned(),
            commit_id: None,
            is_head: false,
            is_remote,
            upstream: None,
            ahead: 0,
            behind: 0,
            summary: String::new(),
            committed_at: None,
        }
    }

    #[test]
    fn branches_are_grouped_by_remote_and_by_folder() {
        let branches = [
            branch("feature/login", false),
            branch("feature/ui/menu", false),
            branch("main", false),
            branch("origin/main", true),
            branch("origin/release/1.0", true),
        ];
        let rows = branch_tree(&branches)
            .into_iter()
            .map(|row| {
                let mut prefix = row
                    .guides
                    .iter()
                    .map(|line| if *line { "│ " } else { "  " })
                    .collect::<String>();
                if !row.group {
                    prefix.push_str(if row.last { "└ " } else { "├ " });
                }
                format!(
                    "{prefix}{}{}",
                    row.label,
                    row.branch.map_or(String::new(), |b| format!(" #{b}"))
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                "Local",
                "├ feature",
                "│ ├ login #0",
                "│ └ ui",
                "│   └ menu #1",
                "└ main #2",
                "origin",
                "├ main #3",
                "└ release",
                "  └ 1.0 #4",
            ]
        );
    }

    fn commit(id: &str, parents: &[&str]) -> CommitInfo {
        CommitInfo {
            id: id.to_owned(),
            short_id: id.to_owned(),
            parent_ids: parents.iter().map(|parent| (*parent).to_owned()).collect(),
            summary: String::new(),
            author: String::new(),
            author_email: String::new(),
            committed_at: DateTime::parse_from_rfc3339("2026-10-28T10:00:00+00:00").unwrap(),
            refs: Vec::new(),
        }
    }

    #[test]
    fn the_timeline_draws_lanes_merges_and_branch_colours() {
        // main: A - B - C - M, feature: B - F - G merged into M; hotfix: H from C, not merged.
        let commits = [
            commit("H", &["C"]),
            commit("M", &["C", "G"]),
            commit("G", &["F"]),
            commit("C", &["B"]),
            commit("F", &["B"]),
            commit("B", &["A"]),
            commit("A", &[]),
        ];
        let rows = timeline(&commits);
        let text = rows.iter().map(TimelineRow::text).collect::<Vec<_>>();
        assert_eq!(
            text,
            ["●", "│ ●─╮", "│ │ ●", "●─╯ │", "│   ●", "●───╯", "●"]
        );
        let colours = |row: usize| {
            rows[row]
                .cells
                .iter()
                .filter(|(symbol, _)| *symbol != ' ')
                .map(|(_, colour)| colour.unwrap())
                .collect::<Vec<_>>()
        };
        // Each line of work has its colour: hotfix 0, main 1, feature 2; a commit two lanes
        // wait for takes the first one's.
        assert_eq!(colours(1), [0, 1, 2, 2]);
        assert_eq!(colours(3), [0, 1, 1, 2]);
        assert_eq!(colours(5), [0, 2, 2, 2, 2]);
        // A merge into a lane already passing by joins it.
        let rows = timeline(&[
            commit("M", &["X", "Y"]),
            commit("N", &["Y"]),
            commit("X", &["Y"]),
            commit("Y", &[]),
        ]);
        let text = rows.iter().map(TimelineRow::text).collect::<Vec<_>>();
        assert_eq!(text, ["●─╮", "│ │ ●", "● │ │", "●─┴─╯"]);
    }

    #[test]
    fn removed_lines_followed_by_added_ones_are_modified() {
        let line = |kind| DiffLine {
            kind,
            old: None,
            new: None,
            text: String::new(),
        };
        use DiffLineKind::{Added, Context, Removed};
        use LineChange as C;
        let lines = [
            Context, Removed, Removed, Added, Context, Added, Removed, Removed, Added,
        ]
        .map(line);
        assert_eq!(
            line_changes(&lines),
            [
                C::Context,
                C::Modified,
                C::Removed,
                C::Modified,
                C::Context,
                C::Added,
                C::Modified,
                C::Removed,
                C::Modified,
            ]
        );
    }

    #[test]
    fn diff_rows_fold_and_pair_lines() {
        let line = |kind, text: &str| DiffLine {
            kind,
            old: None,
            new: None,
            text: text.to_owned(),
        };
        let diff = DiffView {
            files: vec![DiffFile {
                path: PathBuf::from("a.txt"),
                old_path: None,
                status: FileStatus::Modified,
                binary: false,
                hunks: vec![
                    DiffHunk {
                        header: "@@ -1,3 +1,2 @@".to_owned(),
                        lines: vec![
                            line(DiffLineKind::Context, "same"),
                            line(DiffLineKind::Removed, "old one"),
                            line(DiffLineKind::Removed, "old two"),
                            line(DiffLineKind::Added, "new"),
                        ],
                    },
                    DiffHunk {
                        header: "@@ -9 +8 @@".to_owned(),
                        lines: vec![line(DiffLineKind::Added, "end")],
                    },
                ],
            }],
            ..DiffView::default()
        };
        assert_eq!(diff.counts(), (2, 2));
        let mut folds = DiffFolds::new();
        assert_eq!(diff.rows(&folds, false).len(), 1 + 1 + 4 + 1 + 1);
        assert_eq!(
            diff.rows(&folds, true)[2..5],
            [
                DiffRow::Pair(0, 0, Some(0), Some(0)),
                DiffRow::Pair(0, 0, Some(1), Some(3)),
                DiffRow::Pair(0, 0, Some(2), None),
            ]
        );
        folds.insert((0, Some(0)));
        assert_eq!(
            diff.rows(&folds, false),
            [
                DiffRow::File(0),
                DiffRow::Hunk(0, 0),
                DiffRow::Hunk(0, 1),
                DiffRow::Line(0, 1, 0)
            ]
        );
        folds.insert((0, None));
        assert_eq!(diff.rows(&folds, false), [DiffRow::File(0)]);
    }
}
