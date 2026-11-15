//! The command line (TODO 6.1.1, F10): `sozo [DIRECTORY | FILE[:LINE[:COLUMN]]]`, `--help`,
//! `--version`, `--diagnose` (what a bug report needs, without personal paths) and
//! `--completions <shell>` / `--man` (written by the program itself, always in step with it).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Open the folder, and a file in it at a line and column (from 1) when one was given.
    Open {
        root: PathBuf,
        file: Option<(PathBuf, usize, usize)>,
    },
    /// Text to print and leave: help, version, diagnosis, completions, manual.
    Print(String),
}

pub const USAGE: &str = "Usage: sozo [DIRECTORY | FILE[:LINE[:COLUMN]]]";

/// The words after `sozo`, read; `current` is the folder it was started in.
pub fn parse(arguments: &[String], current: &Path) -> Result<Request> {
    match arguments {
        [] => Ok(Request::Open {
            root: current.to_path_buf(),
            file: None,
        }),
        [flag] if flag == "--help" || flag == "-h" => Ok(Request::Print(help())),
        [flag] if flag == "--version" || flag == "-V" => Ok(Request::Print(format!(
            "sozo {}",
            env!("CARGO_PKG_VERSION")
        ))),
        [flag] if flag == "--diagnose" => Ok(Request::Print(diagnose(
            &|name| std::env::var(name).ok(),
            &crate::update::deps::on_path,
        ))),
        [flag] if flag == "--man" => Ok(Request::Print(manual())),
        [flag] if flag == "--notes" => Ok(Request::Print(notes())),
        [flag, shell] if flag == "--completions" => completions(shell).map(Request::Print),
        [argument] => open(argument, current),
        _ => bail!("{USAGE}"),
    }
}

fn help() -> String {
    format!(
        "SozoIDE {}\n\n{USAGE}\n\nOpen DIRECTORY in SozoIDE. Defaults to the current directory.\nA FILE opens in the folder it is in, at LINE and COLUMN when given (src/main.rs:12:5).\n\nOptions:\n  -h, --help              This help\n  -V, --version           The version\n      --diagnose          What a bug report needs (no personal paths)\n      --completions SHELL Completions for bash, zsh, fish or powershell\n      --man               The manual page (roff)",
        env!("CARGO_PKG_VERSION")
    )
}

/// A folder, or a file with `:line[:column]` after it (the place a compiler prints).
fn open(argument: &str, current: &Path) -> Result<Request> {
    let (path, line, column) = split_place(argument);
    let full = current.join(path);
    let full = full
        .canonicalize()
        .with_context(|| format!("Could not open directory {argument}"))?;
    if full.is_dir() {
        return Ok(Request::Open {
            root: full,
            file: None,
        });
    }
    if !full.is_file() {
        bail!("{} is not a directory or a file", full.display());
    }
    let root = full
        .parent()
        .context("A file without a folder")?
        .to_path_buf();
    Ok(Request::Open {
        root,
        file: Some((full, line.max(1), column.max(1))),
    })
}

/// `path:12:5` → (`path`, 12, 5); a path that is not followed by numbers stays whole (a
/// Windows `C:\…` too).
fn split_place(argument: &str) -> (&str, usize, usize) {
    if let Some((rest, last)) = argument.rsplit_once(':')
        && let Ok(last) = last.parse::<usize>()
    {
        if let Some((path, line)) = rest.rsplit_once(':')
            && let Ok(line) = line.parse::<usize>()
            && !path.is_empty()
        {
            return (path, line, last);
        }
        if !rest.is_empty() {
            return (rest, last, 1);
        }
    }
    (argument, 1, 1)
}

/// Version, system, terminal, `TERM`, the image protocol it could ask for and the tools found.
/// No path, user or host name: it is pasted into a public issue.
pub fn diagnose(
    variable: &dyn Fn(&str) -> Option<String>,
    on_path: &dyn Fn(&str) -> bool,
) -> String {
    let value = |name: &str| {
        variable(name)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(60)
                    .collect::<String>()
            })
    };
    let shown = |name: &str| value(name).unwrap_or_else(|| "(not set)".into());
    let tools = [
        "git",
        "curl",
        "ffmpeg",
        "ffplay",
        "ffprobe",
        "tmux",
        "rust-analyzer",
        "clangd",
        "gopls",
        "pyright-langserver",
        "typescript-language-server",
        "lldb-dap",
        "dlv",
        "gdb",
    ]
    .iter()
    .map(|tool| {
        format!(
            "  {tool}: {}",
            if on_path(tool) { "found" } else { "missing" }
        )
    })
    .collect::<Vec<_>>()
    .join("\n");
    let pictures = match (
        value("TERM_PROGRAM").as_deref(),
        value("KITTY_WINDOW_ID").is_some(),
        shown("TERM").as_str(),
    ) {
        (_, true, _) | (_, _, "xterm-kitty") => "kitty graphics",
        (Some("iTerm.app" | "WezTerm"), _, _) => "iTerm2 images",
        (_, _, term) if term.starts_with("tmux") || term.starts_with("screen") => {
            "half blocks (through tmux)"
        }
        _ => "asked at startup; half blocks when nothing answers",
    };
    format!(
        "SozoIDE {}\nSystem: {} {}\nTerminal: {} {}\nTERM: {}\nCOLORTERM: {}\nInside tmux: {}\nNO_COLOR: {}\nPictures: {pictures}\nTools:\n{tools}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        shown("TERM_PROGRAM"),
        value("TERM_PROGRAM_VERSION").unwrap_or_default(),
        shown("TERM"),
        shown("COLORTERM"),
        if value("TMUX").is_some() { "yes" } else { "no" },
        if value("NO_COLOR").is_some() {
            "set"
        } else {
            "not set"
        },
    )
}

const FLAGS: [&str; 6] = [
    "--help",
    "--version",
    "--diagnose",
    "--completions",
    "--man",
    "-h",
];

/// Completions for `shell`: the flags, the shells after `--completions`, then files and folders.
pub fn completions(shell: &str) -> Result<String> {
    let flags = FLAGS.join(" ");
    Ok(match shell {
        "bash" => format!(
            "_sozo() {{\n  local current=${{COMP_WORDS[COMP_CWORD]}}\n  if [[ ${{COMP_WORDS[COMP_CWORD-1]}} == --completions ]]; then\n    COMPREPLY=($(compgen -W \"bash zsh fish powershell\" -- \"$current\"))\n  elif [[ $current == -* ]]; then\n    COMPREPLY=($(compgen -W \"{flags}\" -- \"$current\"))\n  else\n    COMPREPLY=($(compgen -f -- \"$current\"))\n  fi\n}}\ncomplete -o filenames -F _sozo sozo\n"
        ),
        "zsh" => "#compdef sozo\n_arguments \\\n  '(-h --help)'{-h,--help}'[help]' \\\n  '(-V --version)'{-V,--version}'[version]' \\\n  '--diagnose[what a bug report needs]' \\\n  '--completions[completions for a shell]:shell:(bash zsh fish powershell)' \\\n  '--man[the manual page]' \\\n  '1:directory or file:_files'\n".to_owned(),
        "fish" => "complete -c sozo -s h -l help -d 'Help'\ncomplete -c sozo -s V -l version -d 'Version'\ncomplete -c sozo -l diagnose -d 'What a bug report needs'\ncomplete -c sozo -l completions -x -a 'bash zsh fish powershell' -d 'Completions for a shell'\ncomplete -c sozo -l man -d 'The manual page'\n".to_owned(),
        "powershell" => format!(
            "Register-ArgumentCompleter -Native -CommandName sozo -ScriptBlock {{\n  param($word)\n  @({}) | Where-Object {{ $_ -like \"$word*\" }} | ForEach-Object {{ [System.Management.Automation.CompletionResult]::new($_) }}\n}}\n",
            FLAGS.iter().map(|flag| format!("'{flag}'")).collect::<Vec<_>>().join(", ")
        ),
        other => bail!("No completions for {other}: bash, zsh, fish or powershell"),
    })
}

/// The notes of this version, one per line (the release's `NOTES.txt`, TODO 6.1.2).
pub fn notes() -> String {
    crate::update::NOTES
        .iter()
        .find(|notes| notes.version == env!("CARGO_PKG_VERSION"))
        .map(|notes| {
            notes
                .new
                .iter()
                .chain(notes.fixed)
                .copied()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// The manual page, in roff, for `man sozo`.
pub fn manual() -> String {
    format!(
        ".TH SOZO 1 \"\" \"SozoIDE {version}\" \"User Commands\"\n.SH NAME\nsozo \\- a terminal IDE: explorer, editor, terminals, Git and SQL\n.SH SYNOPSIS\n.B sozo\n[\\fIDIRECTORY\\fR | \\fIFILE\\fR[:\\fILINE\\fR[:\\fICOLUMN\\fR]]]\n.SH DESCRIPTION\nOpens DIRECTORY (the current one by default) in SozoIDE. A FILE opens in its folder at LINE and COLUMN. F1 inside lists every key.\n.SH OPTIONS\n.TP\n.BR \\-h \", \" \\-\\-help\nThe help.\n.TP\n.BR \\-V \", \" \\-\\-version\nThe version.\n.TP\n.B \\-\\-diagnose\nWhat a bug report needs, without personal paths.\n.TP\n.BI \\-\\-completions \" SHELL\"\nCompletions for bash, zsh, fish or powershell.\n.TP\n.B \\-\\-man\nThis page.\n.SH ENVIRONMENT\n.TP\n.B NO_COLOR\nDraw with attributes only.\n.TP\n.B SOZO_REDUCED_MOTION\nEvery transition becomes a cut.\n.SH SEE ALSO\n{repository}\n",
        version = env!("CARGO_PKG_VERSION"),
        repository = crate::branding::REPOSITORY,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_files_at_a_line_and_the_options_are_read_and_hostile_ones_refused() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let words = |text: &str| {
            text.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            parse(&[], &root).unwrap(),
            Request::Open {
                root: root.clone(),
                file: None
            }
        );
        assert_eq!(
            parse(&words("src"), &root).unwrap(),
            Request::Open {
                root: root.join("src"),
                file: None
            }
        );
        assert_eq!(
            parse(&words("src/main.rs:12:5"), &root).unwrap(),
            Request::Open {
                root: root.join("src"),
                file: Some((root.join("src/main.rs"), 12, 5))
            }
        );
        assert_eq!(
            parse(&words("src/main.rs:7"), &root).unwrap(),
            Request::Open {
                root: root.join("src"),
                file: Some((root.join("src/main.rs"), 7, 1))
            }
        );
        assert_eq!(
            parse(&words("src/main.rs"), &root).unwrap(),
            Request::Open {
                root: root.join("src"),
                file: Some((root.join("src/main.rs"), 1, 1))
            }
        );
        assert!(parse(&words("missing.rs:3"), &root).is_err());
        assert!(parse(&words("a b"), &root).is_err());
        assert!(
            matches!(parse(&words("--version"), &root).unwrap(), Request::Print(text) if text.starts_with("sozo "))
        );
        assert!(
            matches!(parse(&words("--help"), &root).unwrap(), Request::Print(text) if text.contains(USAGE))
        );
        for shell in ["bash", "zsh", "fish", "powershell"] {
            let text = completions(shell).unwrap();
            assert!(
                text.contains("sozo") && text.contains("diagnose"),
                "{shell}"
            );
        }
        assert!(completions("cmd; rm -rf /").is_err());
        assert!(manual().starts_with(".TH SOZO 1"));
        // The notes of this version, as many lines as notes.
        let count = crate::update::NOTES[0].new.len() + crate::update::NOTES[0].fixed.len();
        assert_eq!(notes().lines().count(), count);
    }

    #[test]
    fn the_diagnosis_says_the_system_without_paths_or_names() {
        let variables = |name: &str| match name {
            "TERM" => Some("xterm-256color".to_owned()),
            "TERM_PROGRAM" => Some("WezTerm".to_owned()),
            "HOME" => Some("/home/dev/someone".to_owned()),
            "TMUX" => Some("/private/tmp/tmux-501/default,123,0".to_owned()),
            "COLORTERM" => Some("truecolor\u{1b}[31m".to_owned()),
            _ => None,
        };
        let text = diagnose(&variables, &|tool| tool == "git");
        assert!(text.contains("TERM: xterm-256color") && text.contains("Pictures: iTerm2 images"));
        assert!(
            text.contains("Inside tmux: yes")
                && text.contains("git: found")
                && text.contains("ffmpeg: missing")
        );
        assert!(
            !text.contains("someone") && !text.contains("/private") && !text.contains("tmux-501"),
            "{text}"
        );
        assert!(!text.contains('\u{1b}'), "no control characters");
    }
}
