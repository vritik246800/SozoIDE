//! Source Control beyond stage and commit (F06, TODO 4.1.1, 4.1.3–4.1.7): discard (the file
//! kept first, to bring it back), stage, unstage and discard one hunk or one line, amend and
//! signed commits, stash, tags and branches, publish a branch, blame and the history of a file,
//! and merge conflicts block by block. Everything that changes the repository goes through the
//! `git` command with its arguments apart, after `--` where a path or a name could look like an
//! option; names of branches and tags are checked first.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::{model::*, operations::run_git, panel::GitPanel};

/// What a hunk or a line does to the repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkAction {
    Stage,
    Unstage,
    Discard,
}

/// A name of a branch or a tag as Git takes it, refused before `git` sees it otherwise: never
/// empty, never starting with `-` (it would be an option), no spaces, no `..`, `~^:?*[\`, `@{`,
/// no control characters, not ending in `.lock`, `/` or `.`.
pub fn checked_name(name: &str) -> Result<&str> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.starts_with(['-', '/', '.'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("@{")
        || name.contains("//")
        || name.len() > 200
        || name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c));
    if bad {
        bail!("{name:?} is not a name Git takes for a branch or a tag");
    }
    Ok(name)
}

/// The patch of one hunk of `file`, or of the lines of it at `lines` (indexes in the hunk):
/// the other added lines are left out and the other removed lines stay as context, so the
/// patch applies alone.
pub fn hunk_patch(file: &DiffFile, hunk: usize, lines: Option<&[usize]>) -> Option<String> {
    let chunk = file.hunks.get(hunk)?;
    let path = file.path.to_string_lossy();
    let old = file
        .old_path
        .as_deref()
        .map_or(path.clone(), Path::to_string_lossy);
    let mut body = String::new();
    let (mut old_count, mut new_count) = (0, 0);
    let old_start = chunk.lines.iter().find_map(|line| line.old).unwrap_or(0);
    let new_start = chunk.lines.iter().find_map(|line| line.new).unwrap_or(0);
    let mut chosen = 0;
    for (index, line) in chunk.lines.iter().enumerate() {
        let wanted = lines.is_none_or(|lines| lines.contains(&index));
        match line.kind {
            DiffLineKind::Context => {
                body.push_str(&format!(" {}\n", line.text));
                old_count += 1;
                new_count += 1;
            }
            DiffLineKind::Added if wanted => {
                body.push_str(&format!("+{}\n", line.text));
                new_count += 1;
                chosen += 1;
            }
            DiffLineKind::Added => {}
            DiffLineKind::Removed if wanted => {
                body.push_str(&format!("-{}\n", line.text));
                old_count += 1;
                chosen += 1;
            }
            DiffLineKind::Removed => {
                body.push_str(&format!(" {}\n", line.text));
                old_count += 1;
                new_count += 1;
            }
            DiffLineKind::Note => body.push_str("\\ No newline at end of file\n"),
        }
    }
    if chosen == 0 {
        return None;
    }
    let start = |start: u32, count: u32| {
        if count == 0 {
            start.saturating_sub(1)
        } else {
            start.max(1)
        }
    };
    Some(format!(
        "diff --git a/{old} b/{path}\n--- a/{old}\n+++ b/{path}\n@@ -{},{old_count} +{},{new_count} @@\n{body}",
        start(old_start, old_count),
        start(new_start, new_count),
    ))
}

/// A merge conflict in a file: the lines of `<<<<<<<`, `=======` and `>>>>>>>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub start: usize,
    pub middle: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Current,
    Incoming,
    Both,
}

pub fn conflicts(lines: &[String]) -> Vec<Conflict> {
    let mut found = Vec::new();
    let mut start = None;
    let mut middle = None;
    for (row, line) in lines.iter().enumerate() {
        if line.starts_with("<<<<<<<") {
            start = Some(row);
            middle = None;
        } else if line.starts_with("=======") && start.is_some() {
            middle = Some(row);
        } else if line.starts_with(">>>>>>>")
            && let (Some(start), Some(middle)) = (start.take(), middle.take())
        {
            found.push(Conflict {
                start,
                middle,
                end: row,
            });
        }
    }
    found
}

/// `lines` with conflict `conflict` resolved as `resolution`.
pub fn resolve(lines: &[String], conflict: &Conflict, resolution: Resolution) -> Vec<String> {
    let current = &lines[conflict.start + 1..conflict.middle];
    // A `|||||||` (diff3) part belongs to neither side.
    let current_end = current
        .iter()
        .position(|line| line.starts_with("|||||||"))
        .unwrap_or(current.len());
    let current = &current[..current_end];
    let incoming = &lines[conflict.middle + 1..conflict.end];
    let chosen: Vec<String> = match resolution {
        Resolution::Current => current.to_vec(),
        Resolution::Incoming => incoming.to_vec(),
        Resolution::Both => current.iter().chain(incoming).cloned().collect(),
    };
    lines[..conflict.start]
        .iter()
        .cloned()
        .chain(chosen)
        .chain(lines[conflict.end + 1..].iter().cloned())
        .collect()
}

/// What `git` said when it failed, as a person can act on it.
pub fn readable_error(stderr: &str) -> String {
    let text = stderr.to_ascii_lowercase();
    let reason = if text.contains("has no upstream branch") || text.contains("no upstream") {
        "This branch is not on the remote yet: Publish puts it there"
    } else if text.contains("non-fast-forward")
        || text.contains("rejected")
        || text.contains("fetch first")
    {
        "The remote has commits you do not have: pull first (SozoIDE never force-pushes)"
    } else if text.contains("authentication failed")
        || text.contains("could not read username")
        || text.contains("permission denied")
        || text.contains("terminal prompts disabled")
    {
        "Git has no credentials for this remote: set up a credential helper or an SSH key"
    } else if text.contains("not possible to fast-forward") || text.contains("diverging") {
        "The branches have diverged: pull only fast-forwards; merge or rebase by hand"
    } else if text.contains("could not resolve host") || text.contains("unable to access") {
        "The remote cannot be reached: check the network"
    } else {
        return stderr
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Git failed")
            .trim()
            .to_owned();
    };
    reason.to_owned()
}

/// One line of blame: the short commit, the author, the date and the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blame {
    pub commit: String,
    pub author: String,
    pub date: String,
    pub summary: String,
}

/// `git blame --line-porcelain`: one `Blame` per line of the file.
pub fn parse_blame(text: &str) -> Vec<Blame> {
    let mut found = Vec::new();
    let mut current = Blame {
        commit: String::new(),
        author: String::new(),
        date: String::new(),
        summary: String::new(),
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("author ") {
            current.author = rest.to_owned();
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            current.date = rest
                .parse::<i64>()
                .ok()
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|time| time.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
        } else if let Some(rest) = line.strip_prefix("summary ") {
            current.summary = rest.to_owned();
        } else if line.starts_with('\t') {
            found.push(current.clone());
        } else if let Some(hash) = line
            .split_whitespace()
            .next()
            .filter(|word| word.len() == 40 && word.chars().all(|c| c.is_ascii_hexdigit()))
        {
            current.commit = hash[..8].to_owned();
        }
    }
    found
}

impl GitPanel {
    fn git(&mut self, args: &[&str], done: &str) -> Result<()> {
        self.require_worktree_repository()?;
        let output = run_git(&self.root, args)?;
        self.last_output = Some(output.clone());
        if output.timed_out || output.exit_code != Some(0) {
            bail!("{}", readable_error(&output.stderr));
        }
        self.refresh()?;
        self.status_message = done.to_owned();
        Ok(())
    }

    fn git_text(&mut self, args: &[&str]) -> Result<String> {
        self.require_worktree_repository()?;
        let output = run_git(&self.root, args)?;
        if output.timed_out || output.exit_code != Some(0) {
            bail!("{}", readable_error(&output.stderr));
        }
        Ok(output.stdout)
    }

    /// Discards the changes of a file; it is copied first to `keep`, to bring it back.
    pub fn discard_file(&mut self, change: &ChangeEntry, keep: &Path) -> Result<PathBuf> {
        let path = self.root.join(&change.path);
        let outside = change
            .path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)));
        if outside || !path.starts_with(&self.root) {
            bail!("Only files of the repository can be discarded");
        }
        let kept = keep.join(change.path.file_name().unwrap_or_default());
        if path.is_file() {
            std::fs::copy(&path, &kept)?;
        }
        let relative = change.path.to_string_lossy().into_owned();
        if change.area == ChangeArea::Untracked {
            std::fs::remove_file(&path)?;
            self.refresh()?;
        } else {
            // From the index: what is staged stays staged.
            self.git(&["checkout", "--", &relative], "Discarded")?;
        }
        self.status_message = format!("Discarded {relative}: a copy is in {}", kept.display());
        Ok(kept)
    }

    /// Stages, unstages or discards one hunk, or some lines of it, by `git apply`.
    pub fn apply_hunk(
        &mut self,
        file: &DiffFile,
        hunk: usize,
        lines: Option<&[usize]>,
        action: HunkAction,
    ) -> Result<()> {
        let Some(patch) = hunk_patch(file, hunk, lines) else {
            bail!("Nothing to change there");
        };
        // A name of its own each time, made with `create_new`: two at once never share it.
        static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = crate::storage::private_folder("patches");
        let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = directory.join(format!("hunk-{}-{count}.patch", std::process::id()));
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(patch.as_bytes())?;
        }
        let path = path.to_string_lossy().into_owned();
        let args: Vec<&str> = match action {
            HunkAction::Stage => vec!["apply", "--cached", "--", &path],
            HunkAction::Unstage => vec!["apply", "--cached", "-R", "--", &path],
            HunkAction::Discard => vec!["apply", "-R", "--", &path],
        };
        let done = match action {
            HunkAction::Stage => "Staged the hunk",
            HunkAction::Unstage => "Unstaged the hunk",
            HunkAction::Discard => "Discarded the hunk",
        };
        let result = self.git(&args, done);
        let _ = std::fs::remove_file(&path);
        result
    }

    /// Amends the last commit with what is staged and, if given, a new message.
    pub fn amend(&mut self, message: Option<&str>) -> Result<()> {
        match message.filter(|message| !message.trim().is_empty()) {
            Some(message) => self.git(
                &["commit", "--amend", "-m", message],
                "Amended the last commit",
            ),
            None => self.git(
                &["commit", "--amend", "--no-edit"],
                "Amended the last commit",
            ),
        }
    }

    /// Whether Git signs commits here (`commit.gpgsign`): they go through `git commit`, which
    /// fails and says why instead of asking for a passphrase where nobody sees it.
    pub fn signs(&mut self) -> bool {
        self.git_text(&["config", "--bool", "commit.gpgsign"])
            .is_ok_and(|text| text.trim() == "true")
    }

    pub fn commit_signed(&mut self, message: &str) -> Result<()> {
        self.git(
            &["commit", "-S", "-m", message],
            "Commit created and signed",
        )
    }

    pub fn stash_push(&mut self, message: &str) -> Result<()> {
        let message = message.trim();
        if message.is_empty() {
            self.git(&["stash", "push", "--include-untracked"], "Stashed")
        } else {
            self.git(
                &["stash", "push", "--include-untracked", "-m", message],
                "Stashed",
            )
        }
    }

    pub fn stash_apply(&mut self, index: usize) -> Result<()> {
        self.git(
            &["stash", "apply", &format!("stash@{{{index}}}")],
            "Stash applied",
        )
    }

    pub fn stash_drop(&mut self, index: usize) -> Result<()> {
        self.git(
            &["stash", "drop", &format!("stash@{{{index}}}")],
            "Stash dropped",
        )
    }

    pub fn create_tag(&mut self, name: &str, message: &str) -> Result<()> {
        let name = checked_name(name)?;
        if message.trim().is_empty() {
            self.git(&["tag", "--", name], "Tag created")
        } else {
            self.git(&["tag", "-a", "-m", message, "--", name], "Tag created")
        }
    }

    pub fn create_branch(&mut self, name: &str) -> Result<()> {
        let name = checked_name(name)?;
        self.git(&["branch", "--", name], "Branch created")
    }

    pub fn rename_branch(&mut self, old: &str, new: &str) -> Result<()> {
        let (old, new) = (checked_name(old)?, checked_name(new)?);
        self.git(&["branch", "-m", "--", old, new], "Branch renamed")
    }

    /// Deletes a branch; only a merged one unless `force` (Shift).
    pub fn delete_branch(&mut self, name: &str, force: bool) -> Result<()> {
        let name = checked_name(name)?;
        self.git(
            &["branch", if force { "-D" } else { "-d" }, "--", name],
            "Branch deleted",
        )
    }

    /// Pushes a branch that has no upstream and sets it: never with force.
    pub fn publish(&mut self) -> Result<()> {
        self.git(
            &["push", "--set-upstream", "origin", "HEAD"],
            "Branch published",
        )
    }

    /// The blame of `path`: one entry per line.
    pub fn blame(&mut self, path: &Path) -> Result<Vec<Blame>> {
        let relative = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let text = self.git_text(&["blame", "--line-porcelain", "--", &relative])?;
        Ok(parse_blame(&text))
    }

    /// The commits that changed `path`, newest first: short id, date, author and summary.
    pub fn file_history(&mut self, path: &Path) -> Result<Vec<Blame>> {
        let relative = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let text = self.git_text(&[
            "log",
            "--follow",
            "-n",
            "200",
            "--format=%h%x09%ad%x09%an%x09%s",
            "--date=short",
            "--",
            &relative,
        ])?;
        Ok(text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(4, '\t');
                Some(Blame {
                    commit: parts.next()?.to_owned(),
                    date: parts.next()?.to_owned(),
                    author: parts.next()?.to_owned(),
                    summary: parts.next()?.chars().filter(|c| !c.is_control()).collect(),
                })
            })
            .collect())
    }

    /// Resolves conflict `index` of `path` as `resolution`; with none left the file is staged.
    pub fn resolve_conflict(
        &mut self,
        path: &Path,
        index: usize,
        resolution: Resolution,
    ) -> Result<usize> {
        let full = self.root.join(path);
        let text = std::fs::read_to_string(&full)?;
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let found = conflicts(&lines);
        let Some(conflict) = found.get(index) else {
            bail!("No such conflict");
        };
        let resolved = resolve(&lines, conflict, resolution);
        let mut out = resolved.join("\n");
        if text.ends_with('\n') {
            out.push('\n');
        }
        std::fs::write(&full, out)?;
        let left = found.len() - 1;
        if left == 0 {
            let relative = path.to_string_lossy().into_owned();
            self.git(&["add", "--", &relative], "Conflicts resolved and staged")?;
        } else {
            self.refresh()?;
        }
        Ok(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::repository::read_change_diff;
    use std::process::Command;

    /// A repository in a temporary folder (never the real one) with `a.txt` committed.
    fn repository(text: &str) -> (tempfile::TempDir, GitPanel) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .current_dir(&root)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "{args:?}"
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("a.txt"), text).unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "first"]);
        let mut panel = GitPanel::new(&root);
        panel.refresh().unwrap();
        (directory, panel)
    }

    #[test]
    #[cfg(unix)]
    fn an_ext_remote_never_runs_and_the_hooks_that_run_are_named() {
        let (_directory, mut panel) = repository("one\n");
        let marker = panel.root.join("pwned");
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .current_dir(&panel.root)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "{args:?}"
            );
        };
        // Even with the repository allowing `ext::`, SozoIDE's Git does not.
        git(&["config", "protocol.ext.allow", "always"]);
        git(&[
            "remote",
            "add",
            "origin",
            &format!("ext::sh -c touch% {}", marker.display()),
        ]);
        assert!(panel.execute(crate::git::GitAction::Fetch).is_err());
        assert!(panel.execute(crate::git::GitAction::Push).is_err());
        assert!(!marker.exists(), "the remote ran a command");
        // A hook of the repository is named; the samples are not.
        let hooks = panel.root.join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(hooks.join("pre-push.sample"), "#!/bin/sh\n").unwrap();
        panel.refresh().unwrap();
        assert_eq!(panel.hooks, ["pre-commit"]);
        // A path that leaves the repository is never discarded.
        let outside = panel.root.parent().unwrap().join("kept.txt");
        std::fs::write(&outside, "mine").unwrap();
        for hostile in ["../kept.txt", "/etc/hosts", "./../kept.txt"] {
            let change = ChangeEntry {
                path: std::path::PathBuf::from(hostile),
                ..panel.status.unstaged.first().cloned().unwrap_or_else(|| {
                    std::fs::write(panel.root.join("a.txt"), "changed\n").unwrap();
                    panel.refresh().unwrap();
                    panel.status.unstaged[0].clone()
                })
            };
            let keep = panel.root.clone();
            let error = panel.discard_file(&change, &keep).unwrap_err().to_string();
            assert!(
                error.starts_with("Only files of the repository"),
                "{hostile}: {error}"
            );
        }
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "mine");
        let _ = std::fs::remove_file(outside);
    }

    fn staged_text(panel: &GitPanel) -> String {
        let output = Command::new("git")
            .current_dir(&panel.root)
            .args(["show", ":a.txt"])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn one_line_of_a_hunk_is_staged_unstaged_and_another_discarded() {
        let (_directory, mut panel) = repository("one\ntwo\nthree\n");
        std::fs::write(panel.root.join("a.txt"), "ONE\ntwo\nTHREE\n").unwrap();
        let repo = panel.require_worktree_repository().unwrap();
        let diff = read_change_diff(&repo, Path::new("a.txt"), DiffKind::Unstaged).unwrap();
        drop(repo);
        let file = &diff.files[0];
        // The lines of the first change only: `-one` and `+ONE`.
        let lines: Vec<usize> = file.hunks[0]
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                line.text.eq_ignore_ascii_case("one") && line.kind != DiffLineKind::Context
            })
            .map(|(index, _)| index)
            .collect();
        panel
            .apply_hunk(file, 0, Some(&lines), HunkAction::Stage)
            .unwrap();
        assert_eq!(staged_text(&panel), "ONE\ntwo\nthree\n");
        panel
            .apply_hunk(file, 0, Some(&lines), HunkAction::Unstage)
            .unwrap();
        assert_eq!(staged_text(&panel), "one\ntwo\nthree\n");
        // The whole hunk discarded: the file as committed.
        panel
            .apply_hunk(file, 0, None, HunkAction::Discard)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(panel.root.join("a.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
        assert!(hunk_patch(file, 9, None).is_none());
    }

    #[test]
    fn a_discarded_file_is_kept_first_and_names_that_look_like_options_are_refused() {
        let (_directory, mut panel) = repository("one\n");
        std::fs::write(panel.root.join("a.txt"), "mine\n").unwrap();
        panel.refresh().unwrap();
        let change = panel.status.unstaged[0].clone();
        let keep = tempfile::tempdir().unwrap();
        let kept = panel.discard_file(&change, keep.path()).unwrap();
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "mine\n");
        assert_eq!(
            std::fs::read_to_string(panel.root.join("a.txt")).unwrap(),
            "one\n"
        );
        for hostile in [
            "--upload-pack=touch x",
            "-D",
            "a b",
            "a..b",
            "x.lock",
            "",
            "a~1",
            "a\u{1b}b",
            "@{1}",
            "/x",
            "x/",
        ] {
            assert!(checked_name(hostile).is_err(), "{hostile:?}");
            assert!(panel.create_branch(hostile).is_err(), "{hostile:?}");
        }
        assert!(!panel.root.join("x").exists());
        // Branches, tags and the stash.
        panel.create_branch("feature/x").unwrap();
        panel.rename_branch("feature/x", "feature/y").unwrap();
        assert!(panel.delete_branch("feature/y", false).is_ok());
        panel.create_tag("v1.0.0", "First").unwrap();
        assert_eq!(panel.tags, ["v1.0.0"]);
        std::fs::write(panel.root.join("a.txt"), "stashed\n").unwrap();
        panel.stash_push("work").unwrap();
        assert_eq!(
            std::fs::read_to_string(panel.root.join("a.txt")).unwrap(),
            "one\n"
        );
        assert_eq!(panel.stashes, [(0, "On main: work".to_owned())]);
        panel.stash_apply(0).unwrap();
        assert_eq!(
            std::fs::read_to_string(panel.root.join("a.txt")).unwrap(),
            "stashed\n"
        );
        panel.stash_drop(0).unwrap();
        assert!(panel.stashes.is_empty());
        // Amend keeps one commit with the new message; publishing with no remote says why.
        panel.amend(Some("first, amended")).unwrap();
        let log = Command::new("git")
            .current_dir(&panel.root)
            .args(["log", "--format=%s"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(log.stdout).unwrap(), "first, amended\n");
        assert!(panel.publish().is_err());
        assert!(!panel.signs());
    }

    #[test]
    fn conflicts_resolve_block_by_block_and_blame_and_history_read_git() {
        let lines: Vec<String> = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> other\nz"
            .lines()
            .map(str::to_owned)
            .collect();
        let found = conflicts(&lines);
        assert_eq!(
            found,
            [Conflict {
                start: 1,
                middle: 3,
                end: 5
            }]
        );
        assert_eq!(
            resolve(&lines, &found[0], Resolution::Current),
            ["a", "mine", "z"]
        );
        assert_eq!(
            resolve(&lines, &found[0], Resolution::Incoming),
            ["a", "theirs", "z"]
        );
        assert_eq!(
            resolve(&lines, &found[0], Resolution::Both),
            ["a", "mine", "theirs", "z"]
        );
        assert!(conflicts(&["<<<<<<< unfinished".to_owned()]).is_empty());
        let (_directory, mut panel) = repository("one\ntwo\n");
        let blame = panel.blame(&panel.root.join("a.txt")).unwrap();
        assert_eq!(blame.len(), 2);
        assert_eq!(
            (blame[0].author.as_str(), blame[0].summary.as_str()),
            ("Test", "first")
        );
        let history = panel.file_history(&panel.root.join("a.txt")).unwrap();
        assert_eq!(history[0].summary, "first");
        // A conflict in the work tree: resolved, then staged.
        std::fs::write(panel.root.join("a.txt"), lines.join("\n") + "\n").unwrap();
        assert_eq!(
            panel
                .resolve_conflict(Path::new("a.txt"), 0, Resolution::Both)
                .unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(panel.root.join("a.txt")).unwrap(),
            "a\nmine\ntheirs\nz\n"
        );
        assert_eq!(staged_text(&panel), "a\nmine\ntheirs\nz\n");
    }

    /// TODO 4.1.8: a repository of 50 000 commits reads a page of the timeline and its status
    /// within the budget (2 s in a debug build, 10 times less in release), and the next page
    /// adds 1000.
    #[test]
    fn a_huge_repository_reads_a_page_of_its_timeline_within_its_budget() {
        let (_directory, mut panel) = repository("start\n");
        let mut stream = String::from("");
        for index in 0..50_000u32 {
            let parent = if index == 0 {
                "from refs/heads/main^0\n".to_owned()
            } else {
                String::new()
            };
            stream.push_str(&format!(
                "commit refs/heads/main\ncommitter T <t@e> {} +0000\ndata 4\nc{:03}\n{parent}M 644 inline n.txt\ndata 6\n{:05}\n\n",
                1_700_000_000 + index,
                index % 1000,
                index,
            ));
        }
        let mut child = Command::new("git")
            .current_dir(&panel.root)
            .args(["fast-import", "--quiet"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), stream.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        Command::new("git")
            .current_dir(&panel.root)
            .args(["reset", "-q", "--hard"])
            .status()
            .unwrap();
        let budget = if cfg!(debug_assertions) { 2_000 } else { 200 };
        let started = std::time::Instant::now();
        panel.refresh().unwrap();
        let took = started.elapsed();
        assert!(took.as_millis() < budget, "{took:?}");
        assert_eq!(panel.commits.len(), 1000);
        panel.more_commits().unwrap();
        assert_eq!(panel.commits.len(), 2000);
    }

    #[test]
    fn errors_of_git_are_said_as_what_to_do() {
        assert!(
            readable_error("fatal: The current branch x has no upstream branch.")
                .contains("Publish")
        );
        assert!(
            readable_error(" ! [rejected] main -> main (non-fast-forward)").contains("pull first")
        );
        assert!(
            readable_error(
                "fatal: could not read Username for 'https://x': terminal prompts disabled"
            )
            .contains("credential")
        );
        assert!(
            readable_error("fatal: Not possible to fast-forward, aborting.").contains("diverged")
        );
        assert_eq!(
            readable_error("\nfatal: something else\n"),
            "fatal: something else"
        );
    }
}
