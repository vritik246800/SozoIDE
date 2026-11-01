//! The commands SozoIDE runs instead of linking (Q41): `git` (Source Control), `curl` (updates)
//! and `ffmpeg` (videos), with the runtimes, compilers, language servers and debuggers the
//! projects need. Help's Dependencies tab (F33) shows each one with its version and installs or
//! updates it with the system's own package manager, only after Enter (Q65).

use std::{
    env,
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{Receiver, channel},
    thread,
    time::{Duration, Instant},
};

use rayon::prelude::*;

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

/// A system package by package manager: Homebrew (`--cask` included), `winget` ID, `apt-get`
/// and `dnf` names.
struct Package {
    brew: &'static [&'static str],
    winget: &'static str,
    apt: &'static [&'static str],
    dnf: &'static [&'static str],
}

/// The command that installs `package` on `os` with the package manager this machine `has`;
/// on Linux behind `sudo` (the words before it: none as root).
fn system_install(
    os: &str,
    package: &Package,
    has: &dyn Fn(&str) -> bool,
    sudo: &[&str],
) -> Option<Vec<String>> {
    let words = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let with = |manager: &[&str], names: &[&str]| {
        let mut command = words(sudo);
        command.extend(words(manager));
        command.extend(words(names));
        command
    };
    match os {
        "macos" if has("brew") => Some([words(&["brew", "install"]), words(package.brew)].concat()),
        "windows" if has("winget") => Some(words(&[
            "winget",
            "install",
            "--id",
            package.winget,
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])),
        "linux" if has("apt-get") => Some(with(
            &[
                "env",
                "DEBIAN_FRONTEND=noninteractive",
                "apt-get",
                "install",
                "-y",
            ],
            package.apt,
        )),
        "linux" if has("dnf") => Some(with(&["dnf", "install", "-y"], package.dnf)),
        _ => None,
    }
}

/// The command that updates what `install` installed: the same package manager's upgrade;
/// `npm` and `go install …@latest` update by installing again.
pub fn upgrade(install: &[String]) -> Vec<String> {
    let mut command = install.to_vec();
    let position = |word: &str| command.iter().position(|known| known == word);
    if let Some(index) = position("brew").or_else(|| position("winget")) {
        command[index + 1] = "upgrade".into();
    } else if let Some(index) = position("apt-get") {
        command.insert(index + 2, "--only-upgrade".into());
    } else if let Some(index) = position("dnf") {
        command[index + 1] = "upgrade".into();
    } else if let Some(index) = position("pip") {
        command.insert(index + 2, "--upgrade".into());
    } else if command.first().is_some_and(|program| program == "rustup") {
        command = vec!["rustup".into(), "update".into()];
    }
    command
}

/// What to put before a package manager on Linux: nothing as root, `sudo` otherwise (the
/// terminal of the Dependencies tab can ask for the password).
pub fn sudo() -> &'static [&'static str] {
    let root = Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|output| output.stdout.trim_ascii() == b"0");
    if root { &[] } else { &["sudo"] }
}

/// A line of the Dependencies tab (F33): what it is, the program found on the machine, the
/// command that prints its version and the commands that install and update it here (none
/// where this machine has no way).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub name: &'static str,
    pub kind: &'static str,
    pub program: &'static str,
    pub probe: Vec<String>,
    pub install: Option<Vec<String>>,
    pub update: Option<Vec<String>>,
}

/// Every component on `os`, SozoIDE's own tools first, with the commands of the package
/// managers this machine `has`.
pub fn components(os: &str, has: impl Fn(&str) -> bool, sudo: &[&str]) -> Vec<Component> {
    let python = if os == "windows" { "python" } else { "python3" };
    let system: [(&str, &str, &str, &[&str], Package); 10] = [
        (
            "Git",
            "Tool",
            "git",
            &["--version"],
            Package {
                brew: &["git"],
                winget: "Git.Git",
                apt: &["git"],
                dnf: &["git"],
            },
        ),
        (
            "curl",
            "Tool",
            "curl",
            &["--version"],
            Package {
                brew: &["curl"],
                winget: "cURL.cURL",
                apt: &["curl"],
                dnf: &["curl"],
            },
        ),
        // Fedora ships FFmpeg as `ffmpeg-free`.
        (
            "FFmpeg",
            "Tool",
            "ffmpeg",
            &["-version"],
            Package {
                brew: &["ffmpeg"],
                winget: "Gyan.FFmpeg",
                apt: &["ffmpeg"],
                dnf: &["ffmpeg-free"],
            },
        ),
        (
            "Rust",
            "Compiler",
            "rustc",
            &["--version"],
            Package {
                brew: &["rust"],
                winget: "Rustlang.Rustup",
                apt: &["rustc", "cargo"],
                dnf: &["rust", "cargo"],
            },
        ),
        (
            "Node.js",
            "Runtime",
            "node",
            &["--version"],
            Package {
                brew: &["node"],
                winget: "OpenJS.NodeJS.LTS",
                apt: &["nodejs", "npm"],
                dnf: &["nodejs", "npm"],
            },
        ),
        (
            "Python",
            "Runtime",
            python,
            &["--version"],
            Package {
                brew: &["python"],
                winget: "Python.Python.3.13",
                apt: &["python3", "python3-pip"],
                dnf: &["python3", "python3-pip"],
            },
        ),
        (
            "Java",
            "Runtime",
            "java",
            &["-version"],
            Package {
                brew: &["openjdk"],
                winget: "EclipseAdoptium.Temurin.21.JDK",
                apt: &["default-jdk"],
                dnf: &["java-21-openjdk-devel"],
            },
        ),
        (
            "Go",
            "Compiler",
            "go",
            &["version"],
            Package {
                brew: &["go"],
                winget: "GoLang.Go",
                apt: &["golang-go"],
                dnf: &["golang"],
            },
        ),
        (
            "Docker",
            "Server",
            "docker",
            &["--version"],
            Package {
                brew: &["--cask", "docker"],
                winget: "Docker.DockerDesktop",
                apt: &["docker.io"],
                dnf: &["moby-engine"],
            },
        ),
        (
            "CMake",
            "Build tool",
            "cmake",
            &["--version"],
            Package {
                brew: &["cmake"],
                winget: "Kitware.CMake",
                apt: &["cmake"],
                dnf: &["cmake"],
            },
        ),
    ];
    let mut list = system
        .into_iter()
        .map(|(name, kind, program, version, package)| {
            let install = system_install(os, &package, &has, sudo);
            // rustup updates the Rust it installed.
            let update = if program == "rustc" && has("rustup") {
                Some(vec!["rustup".into(), "update".into()])
            } else {
                install.as_deref().map(upgrade)
            };
            Component {
                name,
                kind,
                program,
                probe: [program]
                    .iter()
                    .chain(version)
                    .map(|w| (*w).to_owned())
                    .collect(),
                install,
                update,
            }
        })
        .collect::<Vec<_>>();
    // Language servers and debuggers, as `language_plan` installs them.
    let plan = language_plan(os, |_| false, &has, sudo);
    let mut tools = plan.iter().map(|(tool, _)| *tool).collect::<Vec<_>>();
    for tool in [
        "rust-analyzer",
        "clangd",
        "gopls",
        "jdtls",
        "gdb",
        "dlv",
        "lua-language-server",
    ] {
        // GDB debugs on Linux only: lldb-dap comes with Xcode and with LLVM on Windows.
        if !tools.contains(&tool) && (tool != "gdb" || os == "linux") {
            tools.push(tool);
        }
    }
    for tool in tools {
        let install = plan
            .iter()
            .find(|(known, _)| *known == tool)
            .map(|(_, command)| command.clone());
        let kind = if matches!(tool, "gdb" | "dlv" | "debugpy") {
            "Debugger"
        } else {
            "Language server"
        };
        list.push(Component {
            name: tool,
            kind,
            program: tool,
            probe: tool_probe(tool, python),
            update: install.as_deref().map(upgrade),
            install,
        });
    }
    list
}

/// The command that says the version of language server or debugger `tool`.
fn tool_probe(tool: &str, python: &str) -> Vec<String> {
    match tool {
        "dlv" | "gopls" => vec![tool.to_owned(), "version".to_owned()],
        "debugpy" => [python, "-m", "debugpy", "--version"]
            .map(str::to_owned)
            .to_vec(),
        _ => vec![tool.to_owned(), "--version".to_owned()],
    }
}

/// What the Dependencies tab found of a component: missing, or there with its version (none
/// when the program does not say it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    Missing,
    Installed(Option<String>),
}

/// What the component of `program` is, by its `probe`: there with the version it says (none
/// when it says nothing or waits for input), or missing — also a rustup proxy whose
/// component is not installed, which fails saying its toolchain lacks it (`rust-analyzer`
/// before `rustup component add rust-analyzer`, E45).
pub fn found(program: &str, probe: &[String]) -> Found {
    match run(probe) {
        Some((true, version)) => Found::Installed(version),
        Some((false, Some(line))) if line.contains("toolchain") => Found::Missing,
        _ if locate(program).is_some() => Found::Installed(None),
        _ => Found::Missing,
    }
}

/// Whether `probe` succeeded within a few seconds and the first line it printed (on its
/// output or its errors, as `java -version` does); none when it cannot start or waits for
/// input (it is stopped).
fn run(probe: &[String]) -> Option<(bool, Option<String>)> {
    let program = locate(&probe[0]).unwrap_or_else(|| PathBuf::from(&probe[0]));
    let mut child = Command::new(program)
        .args(&probe[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < Duration::from_secs(4) => {
                thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut text = String::new();
    child.stdout.take()?.read_to_string(&mut text).ok()?;
    if text.trim().is_empty() {
        child.stderr.take()?.read_to_string(&mut text).ok()?;
    }
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(60).collect());
    Some((status.success(), line))
}

/// What is found of each component, read in parallel on threads; all of it arrives at once.
pub fn check(components: &[Component]) -> Receiver<Vec<Found>> {
    let (sender, receiver) = channel();
    let probes = components
        .iter()
        .map(|component| (component.program, component.probe.clone()))
        .collect::<Vec<_>>();
    thread::spawn(move || {
        // Its own pool, one thread per program: a probe waits on its program for seconds and
        // must not hold the pool that draws pictures and videos.
        let check_all = || {
            probes
                .par_iter()
                .map(|(program, probe)| found(program, probe))
                .collect::<Vec<_>>()
        };
        let found = match rayon::ThreadPoolBuilder::new()
            .num_threads(probes.len().max(1))
            .build()
        {
            Ok(pool) => pool.install(check_all),
            Err(_) => check_all(),
        };
        let _ = sender.send(found);
    });
    receiver
}

/// Says from a thread what is found of the component of `program` once it is no longer
/// `before` — installed, updated to another version or removed in the bottom terminal (F35):
/// checked every 2 seconds for 10 minutes.
pub fn watch_component(
    program: &'static str,
    probe: Vec<String>,
    before: Found,
) -> Receiver<Found> {
    watch_component_every(program, probe, before, Duration::from_secs(2), 300)
}

fn watch_component_every(
    program: &'static str,
    probe: Vec<String>,
    before: Found,
    every: Duration,
    times: u32,
) -> Receiver<Found> {
    let (sender, receiver) = channel();
    thread::spawn(move || {
        for _ in 0..times {
            thread::sleep(every);
            let now = found(program, &probe);
            if now != before {
                let _ = sender.send(now);
                return;
            }
        }
    });
    receiver
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

/// The command that installs language server `name` here, as the Dependencies tab would;
/// CSS's and JSON's come with HTML's (`vscode-langservers-extracted`).
pub fn server_install(name: &str) -> Option<Vec<String>> {
    let sudo = if cfg!(target_os = "linux") {
        sudo()
    } else {
        &[]
    };
    install_in(
        language_plan(env::consts::OS, |_| false, on_path, sudo),
        name,
    )
}

fn install_in(plan: Vec<(&str, Vec<String>)>, name: &str) -> Option<Vec<String>> {
    let name = if name.starts_with("vscode-") {
        "vscode-html-language-server"
    } else {
        name
    };
    plan.into_iter()
        .find(|(tool, _)| *tool == name)
        .map(|(_, command)| command)
}

/// Says once, from a thread, when language server `name` is there (installed in the bottom
/// terminal): checked every 2 seconds for 10 minutes.
pub fn watch(name: &str) -> Receiver<()> {
    watch_every(name, Duration::from_secs(2), 300)
}

fn watch_every(name: &str, every: Duration, times: u32) -> Receiver<()> {
    let (sender, receiver) = channel();
    let name = name.to_owned();
    let probe = tool_probe(&name, "python3");
    thread::spawn(move || {
        for _ in 0..times {
            thread::sleep(every);
            if found(&name, &probe) != Found::Missing {
                let _ = sender.send(());
                return;
            }
        }
    });
    receiver
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
    // Code Intelligence (F31): the servers of other languages with highlight, from npm, and
    // Lua's from Homebrew. `vscode-langservers-extracted` brings HTML, CSS and JSON together.
    for (tool, package) in [
        ("bash-language-server", "bash-language-server"),
        ("yaml-language-server", "yaml-language-server"),
        ("intelephense", "intelephense"),
        (
            "vscode-html-language-server",
            "vscode-langservers-extracted",
        ),
    ] {
        add(
            tool,
            has("npm").then(|| words(&["npm", "install", "--global", package])),
        );
    }
    add(
        "lua-language-server",
        (os != "windows" && has("brew"))
            .then(|| words(&["brew", "install", "lua-language-server"])),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tools_are_installed_by_each_systems_package_manager() {
        let only = |managers: &'static [&'static str]| move |tool: &str| managers.contains(&tool);

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
                "bash-language-server: npm install --global bash-language-server",
                "yaml-language-server: npm install --global yaml-language-server",
                "intelephense: npm install --global intelephense",
                "vscode-html-language-server: npm install --global vscode-langservers-extracted",
                "lua-language-server: brew install lua-language-server",
            ],
            "no gdb on macOS (lldb-dap comes with Xcode), no Python for debugpy, dlv is there"
        );
    }

    #[test]
    fn every_component_has_its_install_and_update_commands_on_each_system() {
        let only = |managers: &'static [&'static str]| move |tool: &str| managers.contains(&tool);
        let line = |command: &Option<Vec<String>>| command.as_ref().map(|c| c.join(" "));
        let find = |list: &[Component], name: &str| {
            list.iter()
                .find(|component| component.name == name)
                .unwrap()
                .clone()
        };
        let mac = components("macos", only(&["brew", "npm"]), &[]);
        let names = mac
            .iter()
            .map(|component| component.name)
            .take(10)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "Git", "curl", "FFmpeg", "Rust", "Node.js", "Python", "Java", "Go", "Docker",
                "CMake"
            ],
            "SozoIDE's own tools first"
        );
        let ffmpeg = find(&mac, "FFmpeg");
        assert_eq!(ffmpeg.probe, ["ffmpeg", "-version"]);
        assert_eq!(
            line(&ffmpeg.install).as_deref(),
            Some("brew install ffmpeg")
        );
        assert_eq!(line(&ffmpeg.update).as_deref(), Some("brew upgrade ffmpeg"));
        let docker = find(&mac, "Docker");
        assert_eq!(
            line(&docker.install).as_deref(),
            Some("brew install --cask docker")
        );
        assert_eq!(
            line(&docker.update).as_deref(),
            Some("brew upgrade --cask docker")
        );
        let pyright = find(&mac, "pyright-langserver");
        assert_eq!(pyright.kind, "Language server");
        assert_eq!(
            line(&pyright.install).as_deref(),
            Some("npm install --global pyright"),
            "npm updates by installing again"
        );
        assert_eq!(pyright.update, pyright.install);
        assert_eq!(find(&mac, "gopls").install, None, "no Go here");
        assert!(
            mac.iter().all(|component| component.name != "gdb"),
            "Linux only"
        );
        assert_eq!(find(&mac, "dlv").probe, ["dlv", "version"]);

        let windows = components("windows", only(&["winget"]), &[]);
        let java = find(&windows, "Java");
        assert_eq!(java.probe, ["java", "-version"]);
        assert!(
            line(&java.install)
                .unwrap()
                .starts_with("winget install --id EclipseAdoptium.Temurin.21.JDK --exact --silent")
        );
        assert!(
            line(&java.update)
                .unwrap()
                .starts_with("winget upgrade --id")
        );
        assert_eq!(find(&windows, "Python").probe[0], "python");

        let apt = components("linux", only(&["apt-get", "rustup", "python3"]), &["sudo"]);
        let node = find(&apt, "Node.js");
        assert_eq!(
            line(&node.install).as_deref(),
            Some("sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y nodejs npm")
        );
        assert_eq!(
            line(&node.update).as_deref(),
            Some(
                "sudo env DEBIAN_FRONTEND=noninteractive apt-get install --only-upgrade -y nodejs npm"
            )
        );
        assert_eq!(
            line(&find(&apt, "Rust").update).as_deref(),
            Some("rustup update")
        );
        let debugpy = find(&apt, "debugpy");
        assert_eq!(debugpy.probe, ["python3", "-m", "debugpy", "--version"]);
        assert_eq!(
            line(&debugpy.update).as_deref(),
            Some("python3 -m pip install --upgrade --user debugpy")
        );
        let dnf = components("linux", only(&["dnf"]), &[]);
        assert_eq!(
            line(&find(&dnf, "FFmpeg").install).as_deref(),
            Some("dnf install -y ffmpeg-free")
        );
        assert_eq!(
            line(&find(&dnf, "CMake").update).as_deref(),
            Some("dnf upgrade -y cmake")
        );
        // Without a package manager nothing can be installed, and nothing is said to be.
        let none = components("macos", only(&[]), &[]);
        assert!(none.iter().all(|component| component.install.is_none()));
    }

    #[test]
    fn versions_are_read_from_the_first_line_and_a_missing_tool_has_none() {
        let words = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        // `cargo` is there wherever these tests build.
        let Found::Installed(Some(cargo)) = found("cargo", &words(&["cargo", "--version"])) else {
            panic!("cargo has a version");
        };
        assert!(cargo.starts_with("cargo "), "{cargo}");
        assert!(!cargo.contains('\n'));
        assert_eq!(run(&words(&["sozo-no-such-tool", "--version"])), None);
        assert_eq!(
            found(
                "sozo-no-such-tool",
                &words(&["sozo-no-such-tool", "--version"])
            ),
            Found::Missing
        );
        // A program that is there but fails says no version.
        assert_eq!(
            found("cargo", &words(&["cargo", "--no-such-flag"])),
            Found::Installed(None)
        );
        // A rustup proxy without its component fails saying so (E45): the test binary, which
        // fails with «error: Unrecognized option: 'toolchain'» on every system, stands for it.
        let test = env::current_exe().unwrap().display().to_string();
        assert_eq!(
            found("cargo", &[test.clone(), "--toolchain".into()]),
            Found::Missing
        );
        assert_eq!(
            found("cargo", &[test, "--no-such-flag".into()]),
            Found::Installed(None)
        );
        let list = vec![Component {
            name: "Nothing",
            kind: "Tool",
            program: "sozo-no-such-tool",
            probe: words(&["sozo-no-such-tool", "--version"]),
            install: None,
            update: None,
        }];
        let found = check(&list).recv().unwrap();
        assert_eq!(found, [Found::Missing]);
    }

    #[test]
    fn a_missing_server_has_its_install_command_and_is_watched_until_it_is_there() {
        let only = |managers: &'static [&'static str]| move |tool: &str| managers.contains(&tool);
        let plan = || language_plan("macos", |_| false, only(&["rustup", "npm"]), &[]);
        let line = |command: Option<Vec<String>>| command.map(|command| command.join(" "));
        assert_eq!(
            line(install_in(plan(), "rust-analyzer")).as_deref(),
            Some("rustup component add rust-analyzer")
        );
        // CSS and JSON come in the package of the HTML server.
        assert_eq!(
            line(install_in(plan(), "vscode-json-language-server")).as_deref(),
            Some("npm install --global vscode-langservers-extracted")
        );
        assert_eq!(install_in(plan(), "gopls"), None, "no go here");

        // `cargo` is there wherever the tests build; the other never comes.
        let there = watch_every("cargo", Duration::from_millis(5), 100);
        assert_eq!(there.recv_timeout(Duration::from_secs(10)), Ok(()));
        // A component is said again once it is not what it was: `cargo` is there.
        let words = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        let changed = watch_component_every(
            "cargo",
            words(&["cargo", "--version"]),
            Found::Missing,
            Duration::from_millis(5),
            100,
        );
        assert!(matches!(
            changed.recv_timeout(Duration::from_secs(10)),
            Ok(Found::Installed(Some(version))) if version.starts_with("cargo ")
        ));
        let same = found("cargo", &words(&["cargo", "--version"]));
        let unchanged = watch_component_every(
            "cargo",
            words(&["cargo", "--version"]),
            same,
            Duration::from_millis(1),
            3,
        );
        assert_eq!(
            unchanged.recv_timeout(Duration::from_secs(10)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
        let never = watch_every("sozo-no-such-tool", Duration::from_millis(1), 3);
        assert_eq!(
            never.recv_timeout(Duration::from_secs(10)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
    }
}
