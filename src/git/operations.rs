use std::{
    io::{self, Read},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use git2::IndexAddOption;
use wait_timeout::ChildExt;

use super::{model::*, panel::GitPanel, repository::head_commit};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

impl GitPanel {
    pub(super) fn stage_change(&mut self, change: ChangeEntry) -> Result<()> {
        if change.area == ChangeArea::Staged {
            self.status_message = "Change is already staged.".to_string();
            return Ok(());
        }
        let repo = self.require_worktree_repository()?;
        let mut index = repo.index()?;
        if change.status == FileStatus::Deleted {
            index.remove_path(&change.path)?;
        } else {
            index.add_path(&change.path)?;
        }
        if let Some(old_path) = change.old_path.as_deref()
            && old_path != change.path
        {
            index.remove_path(old_path)?;
        }
        index.write()?;
        drop(index);
        drop(repo);
        self.refresh()?;
        self.status_message = format!("Staged {}.", change.path.display());
        Ok(())
    }

    pub(super) fn unstage_change(&mut self, change: ChangeEntry) -> Result<()> {
        if change.area != ChangeArea::Staged {
            self.status_message = "Change is not staged.".to_string();
            return Ok(());
        }
        let repo = self.require_worktree_repository()?;
        if let Some(head) = head_commit(&repo)? {
            repo.reset_default(Some(head.as_object()), [change.path.as_path()])?;
        } else {
            let mut index = repo.index()?;
            index.remove_path(&change.path)?;
            index.write()?;
        }
        drop(repo);
        self.refresh()?;
        self.status_message = format!("Unstaged {}.", change.path.display());
        Ok(())
    }

    pub(super) fn stage_all_inner(&mut self) -> Result<()> {
        let repo = self.require_worktree_repository()?;
        let mut index = repo.index()?;
        index.add_all(["."], IndexAddOption::DEFAULT, None)?;
        index.write()?;
        drop(index);
        drop(repo);
        self.refresh()?;
        self.status_message = "Staged all changes.".to_string();
        Ok(())
    }

    pub(super) fn commit_inner(&mut self, message: &str) -> Result<()> {
        if message.trim().is_empty() {
            bail!("Commit message cannot be empty.");
        }
        self.refresh()?;
        if self
            .status
            .unstaged
            .iter()
            .any(|change| change.status == FileStatus::Conflict)
        {
            bail!("Resolve merge conflicts before committing.");
        }
        if self.status.staged.is_empty() {
            bail!("There are no staged changes to commit.");
        }
        let repo = self.require_worktree_repository()?;
        let signature = repo.signature().context(
            "Git author identity is unavailable; configure user.name and user.email for this repository.",
        )?;
        let mut index = repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let parent = head_commit(&repo)?;
        let parents: Vec<_> = parent.iter().collect();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )?;
        drop(tree);
        drop(index);
        drop(parent);
        drop(repo);
        self.refresh()?;
        self.status_message = "Commit created.".to_string();
        Ok(())
    }

    pub(super) fn ensure_clean(&mut self, operation: &str) -> Result<()> {
        self.require_worktree_repository()?;
        self.refresh()?;
        if !self.status.is_clean() {
            bail!(
                "{operation} requires a clean working tree. Commit, stash, or discard changes first."
            );
        }
        if self.launchpad.operation.is_some() {
            bail!("Finish the Git operation already in progress before continuing.");
        }
        Ok(())
    }

    pub(super) fn run_git_operation(&mut self, args: &[&str], success_message: &str) -> Result<()> {
        self.require_worktree_repository()?;
        let output = run_git(&self.root, args)?;
        self.last_output = Some(output.clone());
        if output.timed_out {
            bail!(
                "Git command timed out after {} seconds.",
                COMMAND_TIMEOUT.as_secs()
            );
        }
        if output.exit_code != Some(0) {
            let details = if output.stderr.trim().is_empty() {
                output.stdout.trim()
            } else {
                output.stderr.trim()
            };
            bail!(
                "Git command failed (exit code {}): {}",
                output
                    .exit_code
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
                if details.is_empty() {
                    "no error output"
                } else {
                    details
                }
            );
        }
        self.refresh()?;
        let stdout = output.stdout.trim();
        self.status_message = if stdout.is_empty() {
            success_message.to_string()
        } else {
            stdout.to_string()
        };
        Ok(())
    }
}

fn run_git(root: &Path, args: &[&str]) -> Result<GitCommandOutput> {
    let command_label = format!(
        "git {}",
        args.iter()
            .map(|argument| shell_display(argument))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let mut child = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Could not start {command_label}"))?;

    let mut stdout = child.stdout.take().expect("piped stdout is available");
    let mut stderr = child.stderr.take().expect("piped stderr is available");
    let stdout_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let stderr_reader = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes)?;
        Ok(bytes)
    });

    let timed_out = child.wait_timeout(COMMAND_TIMEOUT)?.is_none();
    let status = if timed_out {
        child.kill()?;
        child.wait()?
    } else {
        child.wait()?
    };
    let stdout = join_output(stdout_reader, "stdout")?;
    let stderr = join_output(stderr_reader, "stderr")?;
    Ok(GitCommandOutput {
        command: command_label,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code: (!timed_out).then(|| status.code()).flatten(),
        timed_out,
    })
}

fn join_output(reader: thread::JoinHandle<io::Result<Vec<u8>>>, stream: &str) -> Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| anyhow!("Git {stream} reader thread panicked."))?
        .with_context(|| format!("Could not read Git {stream}."))
}

fn shell_display(argument: &str) -> String {
    if argument
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._/-".contains(character))
    {
        argument.to_string()
    } else {
        format!("{:?}", argument)
    }
}
