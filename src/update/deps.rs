//! The commands SozoIDE runs instead of linking (Q41): `git` (Source Control), `curl` (updates)
//! and `ffmpeg` (videos). What is missing is installed in the background with the system's own
//! package manager, after a new installation or an update (F24).

use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{Receiver, channel},
    thread,
};

pub const TOOLS: [&str; 3] = ["git", "curl", "ffmpeg"];

/// The tools of `TOOLS` that are not on the `PATH`.
pub fn missing() -> Vec<&'static str> {
    TOOLS.into_iter().filter(|tool| !on_path(tool)).collect()
}

pub fn on_path(tool: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| found_in(&paths, tool))
}

/// Whether one of the folders of `paths` (a `PATH`) holds `tool`.
fn found_in(paths: &OsStr, tool: &str) -> bool {
    env::split_paths(paths).any(|directory| {
        directory.join(tool).is_file() || directory.join(format!("{tool}.exe")).is_file()
    })
}

/// The commands that install `missing` on `os` with the package manager this machine `has`:
/// Homebrew on macOS, `winget` on Windows, `apt-get` or `dnf` on Linux behind `sudo` (the
/// words before the command: none as root, `sudo -n` where no one can type a password).
pub fn plan(
    os: &str,
    missing: &[&str],
    has: impl Fn(&str) -> bool,
    sudo: &[&str],
) -> Vec<Vec<String>> {
    if missing.is_empty() {
        return Vec::new();
    }
    let words = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
    let with = |manager: &[&str], names: &dyn Fn(&str) -> &'static str| {
        let mut command: Vec<String> = words(sudo);
        command.extend(words(manager));
        command.extend(missing.iter().map(|tool| names(tool).to_owned()));
        vec![command]
    };
    match os {
        "macos" if has("brew") => with(&["brew", "install"], &|tool| match tool {
            "git" => "git",
            "curl" => "curl",
            _ => "ffmpeg",
        }),
        "windows" if has("winget") => missing
            .iter()
            .map(|tool| {
                let id = match *tool {
                    "git" => "Git.Git",
                    "curl" => "cURL.cURL",
                    _ => "Gyan.FFmpeg",
                };
                words(&[
                    "winget",
                    "install",
                    "--id",
                    id,
                    "--exact",
                    "--silent",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                    "--disable-interactivity",
                ])
            })
            .collect(),
        "linux" if has("apt-get") => with(
            &[
                "env",
                "DEBIAN_FRONTEND=noninteractive",
                "apt-get",
                "install",
                "-y",
            ],
            &|tool| match tool {
                "git" => "git",
                "curl" => "curl",
                _ => "ffmpeg",
            },
        ),
        // Fedora ships FFmpeg as `ffmpeg-free`.
        "linux" if has("dnf") => with(&["dnf", "install", "-y"], &|tool| match tool {
            "git" => "git",
            "curl" => "curl",
            _ => "ffmpeg-free",
        }),
        _ => Vec::new(),
    }
}

/// What to put before a package manager on Linux: nothing as root; `sudo`, or `sudo -n` when
/// `quiet` (in the background no one can type a password, so it only works without one).
pub fn sudo(quiet: bool) -> &'static [&'static str] {
    let root = Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|output| output.stdout.trim_ascii() == b"0");
    match (root, quiet) {
        (true, _) => &[],
        (false, true) => &["sudo", "-n"],
        (false, false) => &["sudo"],
    }
}

/// Where a tool is: the `PATH`, then the folders where `go install`, `cargo install` and
/// `pip install --user` put them without adding them to it.
pub fn locate(tool: &str) -> Option<PathBuf> {
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"));
    let extra = home.iter().flat_map(|home| {
        let home = Path::new(home);
        [".cargo/bin", "go/bin", ".local/bin"].map(|folder| home.join(folder))
    });
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(extra)
        .flat_map(|folder| {
            ["", ".exe", ".cmd"].map(|suffix| folder.join(format!("{tool}{suffix}")))
        })
        .find(|path| path.is_file())
}

/// The command line of a language server, found by `locate`; on macOS `clangd` also comes
/// with the Xcode command line tools, behind `xcrun`.
pub fn server_command(name: &str, arguments: &[&str]) -> Option<Vec<String>> {
    let program = locate(name)
        .map(|path| vec![path.display().to_string()])
        .or_else(|| {
            let xcrun = cfg!(target_os = "macos")
                && name == "clangd"
                && Command::new("xcrun")
                    .args(["--find", "clangd"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success());
            xcrun.then(|| vec!["xcrun".to_owned(), name.to_owned()])
        })?;
    Some(
        program
            .into_iter()
            .chain(arguments.iter().map(|argument| (*argument).to_owned()))
            .collect(),
    )
}

/// The language servers of F26 (Q46) that are not `present`, each with the command that
/// installs it with what this machine `has`; one without a way to install it is left out.
pub fn language_plan(
    os: &str,
    present: impl Fn(&str) -> bool,
    has: impl Fn(&str) -> bool,
    sudo: &[&str],
) -> Vec<(&'static str, Vec<String>)> {
    let words = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let system = |manager: &[&str]| {
        let mut command = words(sudo);
        command.extend(words(manager));
        command
    };
    let python = ["python3", "python"].into_iter().find(|python| has(python));
    let mut plan = Vec::new();
    let mut add = |tool: &'static str, command: Option<Vec<String>>| {
        if !present(tool)
            && let Some(command) = command
        {
            plan.push((tool, command));
        }
    };
    add(
        "rust-analyzer",
        if has("rustup") {
            Some(words(&["rustup", "component", "add", "rust-analyzer"]))
        } else {
            (os == "macos" && has("brew")).then(|| words(&["brew", "install", "rust-analyzer"]))
        },
    );
    add(
        "clangd",
        match os {
            "linux" if has("apt-get") => Some(system(&["apt-get", "install", "-y", "clangd"])),
            "linux" if has("dnf") => Some(system(&["dnf", "install", "-y", "clang-tools-extra"])),
            "windows" if has("winget") => Some(words(&[
                "winget",
                "install",
                "--id",
                "LLVM.LLVM",
                "--exact",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity",
            ])),
            _ => None,
        },
    );
    add(
        "pyright-langserver",
        if has("npm") {
            Some(words(&["npm", "install", "--global", "pyright"]))
        } else {
            python.map(|python| words(&[python, "-m", "pip", "install", "--user", "pyright"]))
        },
    );
    add(
        "typescript-language-server",
        has("npm").then(|| {
            words(&[
                "npm",
                "install",
                "--global",
                "typescript-language-server",
                "typescript",
            ])
        }),
    );
    add(
        "gopls",
        has("go").then(|| words(&["go", "install", "golang.org/x/tools/gopls@latest"])),
    );
    add(
        "jdtls",
        (os != "windows" && has("brew")).then(|| words(&["brew", "install", "jdtls"])),
    );
    // Debuggers (F27): GDB speaks DAP since 14 (lldb-dap comes with Xcode and with LLVM, the
    // clangd of Windows); debugpy for Python; Delve for Go.
    let gdb = if os != "linux" || present("lldb-dap") {
        None
    } else if has("apt-get") {
        Some(system(&["apt-get", "install", "-y", "gdb"]))
    } else {
        has("dnf").then(|| system(&["dnf", "install", "-y", "gdb"]))
    };
    add("gdb", gdb);
    add(
        "debugpy",
        python.map(|python| words(&[python, "-m", "pip", "install", "--user", "debugpy"])),
    );
    add(
        "dlv",
        if has("go") {
            Some(words(&[
                "go",
                "install",
                "github.com/go-delve/delve/cmd/dlv@latest",
            ]))
        } else {
            (os != "windows" && has("brew")).then(|| words(&["brew", "install", "delve"]))
        },
    );
    plan
}

/// Installs what is missing in a thread: `git`, `curl` and `ffmpeg`, then the language
/// servers. The message for the status bar arrives when it is done (none when nothing was
/// missing).
pub fn install_in_background() -> Receiver<String> {
    let (sender, receiver) = channel();
    thread::spawn(move || {
        let sudo = if cfg!(target_os = "linux") {
            sudo(true)
        } else {
            &[]
        };
        let os = env::consts::OS;
        let missing = missing();
        let mut steps = plan(os, &missing, on_path, sudo)
            .into_iter()
            .map(|command| (missing.join(", "), command))
            .collect::<Vec<_>>();
        if !missing.is_empty() && steps.is_empty() {
            let _ = sender.send(format!(
                "Install {}: no package manager found",
                missing.join(", ")
            ));
        }
        let present = |tool: &str| match tool {
            "lldb-dap" | "debugpy" => crate::debug::locate(tool).is_some(),
            _ => locate(tool).is_some(),
        };
        let servers = language_plan(os, present, on_path, sudo);
        steps.extend(
            servers
                .into_iter()
                .map(|(tool, command)| (tool.to_owned(), command)),
        );
        if steps.is_empty() {
            return;
        }
        let (mut installed, mut failed) = (Vec::new(), Vec::new());
        for (tools, command) in steps {
            let done = Command::new(&command[0])
                .args(&command[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if done { &mut installed } else { &mut failed }.push(tools);
        }
        let mut message = Vec::new();
        if !installed.is_empty() {
            message.push(format!("Installed {}", installed.join(", ")));
        }
        if !failed.is_empty() {
            message.push(format!("could not install {}", failed.join(", ")));
        }
        let _ = sender.send(message.join("; "));
    });
    receiver
}

/// Runs the plan in this terminal, where `sudo` can ask for a password (the Linux update).
pub fn install_here() {
    let missing = missing();
    for command in plan(env::consts::OS, &missing, on_path, sudo(false)) {
        println!("Installing {}...", missing.join(", "));
        let _ = Command::new(&command[0]).args(&command[1..]).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tools_are_installed_by_each_systems_package_manager() {
        let text = |commands: Vec<Vec<String>>| {
            commands
                .iter()
                .map(|command| command.join(" "))
                .collect::<Vec<_>>()
        };
        let only = |managers: &'static [&'static str]| move |tool: &str| managers.contains(&tool);
        assert_eq!(
            text(plan("macos", &["git", "ffmpeg"], only(&["brew"]), &[])),
            ["brew install git ffmpeg"]
        );
        assert!(
            plan("macos", &["ffmpeg"], only(&[]), &[]).is_empty(),
            "no Homebrew"
        );
        assert_eq!(
            text(plan("windows", &["curl", "ffmpeg"], only(&["winget"]), &[])),
            [
                "winget install --id cURL.cURL --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity",
                "winget install --id Gyan.FFmpeg --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity"
            ]
        );
        assert_eq!(
            text(plan(
                "linux",
                &["ffmpeg"],
                only(&["apt-get", "dnf"]),
                &["sudo", "-n"]
            )),
            ["sudo -n env DEBIAN_FRONTEND=noninteractive apt-get install -y ffmpeg"]
        );
        assert_eq!(
            text(plan("linux", &["git", "ffmpeg"], only(&["dnf"]), &[])),
            ["dnf install -y git ffmpeg-free"]
        );
        assert!(plan("linux", &[], only(&["apt-get"]), &[]).is_empty());

        // A tool is found in the folders of the PATH.
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("ffmpeg"), "").unwrap();
        let paths = env::join_paths([Path::new("/nowhere"), directory.path()]).unwrap();
        assert!(found_in(&paths, "ffmpeg") && !found_in(&paths, "git"));

        // Language servers: each with the tool its ecosystem installs it with, or none.
        let servers = |os, has: &'static [&'static str]| {
            language_plan(
                os,
                |tool| tool == "gopls" || tool == "dlv",
                only(has),
                &["sudo", "-n"],
            )
            .into_iter()
            .map(|(tool, command)| format!("{tool}: {}", command.join(" ")))
            .collect::<Vec<_>>()
        };
        assert_eq!(
            servers("linux", &["rustup", "apt-get", "python3", "go"]),
            [
                "rust-analyzer: rustup component add rust-analyzer",
                "clangd: sudo -n apt-get install -y clangd",
                "pyright-langserver: python3 -m pip install --user pyright",
                "gdb: sudo -n apt-get install -y gdb",
                "debugpy: python3 -m pip install --user debugpy",
            ],
            "gopls and dlv are there; no npm for TypeScript, no Homebrew for jdtls"
        );
        assert_eq!(
            servers("macos", &["brew", "npm"]),
            [
                "rust-analyzer: brew install rust-analyzer",
                "pyright-langserver: npm install --global pyright",
                "typescript-language-server: npm install --global typescript-language-server typescript",
                "jdtls: brew install jdtls",
            ],
            "no gdb on macOS (lldb-dap comes with Xcode), no Python for debugpy, dlv is there"
        );
    }
}
