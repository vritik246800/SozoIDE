//! The commands SozoIDE runs instead of linking (Q41): `git` (Source Control), `curl` (updates)
//! and `ffmpeg` (videos). What is missing is installed in the background with the system's own
//! package manager, after a new installation or an update (F24).

use std::{
    env,
    ffi::OsStr,
    process::{Command, Stdio},
    sync::mpsc::{Receiver, channel},
    thread,
};

pub const TOOLS: [&str; 3] = ["git", "curl", "ffmpeg"];

/// The tools of `TOOLS` that are not on the `PATH`.
pub fn missing() -> Vec<&'static str> {
    TOOLS.into_iter().filter(|tool| !on_path(tool)).collect()
}

fn on_path(tool: &str) -> bool {
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

/// Installs what is missing in a thread; the message for the status bar arrives when it is
/// done (none when nothing was missing).
pub fn install_in_background() -> Receiver<String> {
    let (sender, receiver) = channel();
    thread::spawn(move || {
        let missing = missing();
        if missing.is_empty() {
            return;
        }
        let sudo = if cfg!(target_os = "linux") {
            sudo(true)
        } else {
            &[]
        };
        let commands = plan(env::consts::OS, &missing, on_path, sudo);
        let installed = !commands.is_empty()
            && commands.iter().all(|command| {
                Command::new(&command[0])
                    .args(&command[1..])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success())
            });
        let list = missing.join(", ");
        let _ = sender.send(if installed {
            format!("Installed {list}: restart SozoIDE if a video does not play")
        } else {
            format!("Could not install {list}: install it with your package manager")
        });
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
    use std::path::Path;

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
    }
}
