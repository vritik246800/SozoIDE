use std::process::Command;

#[test]
fn help_and_version_are_available_without_starting_the_ui() {
    let binary = env!("CARGO_BIN_EXE_sozo");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("Usage: sozo [DIRECTORY]"));
    assert!(help.contains("Defaults to the current directory"));

    let version = Command::new(binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("sozo {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rejects_a_missing_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_sozo"))
        .arg("a-directory-that-does-not-exist-sozo")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Could not open directory"));
}

/// A folder of fake commands for `packaging/install-tools.sh`: this is a Linux with `apt-get`
/// as root, and `apt-get` writes what it was asked and "installs" the tools.
#[cfg(unix)]
fn fake_system(directory: &std::path::Path, present: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    let write = |name: &str, body: &str| {
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    write("uname", "echo Linux");
    write("id", "echo 0");
    let log = directory.join("apt.log");
    let bin = directory.display();
    write(
        "apt-get",
        &format!(
            "echo \"$@\" >> '{}'\nshift 2\nfor t in \"$@\"; do printf '#!/bin/sh\\n' > \"{bin}/$t\"; /bin/chmod +x \"{bin}/$t\"; done",
            log.display()
        ),
    );
    for tool in present {
        write(tool, "");
    }
}

#[cfg(unix)]
fn install_tools(directory: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    Command::new("/bin/bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packaging/install-tools.sh"
        ))
        .args(arguments)
        .env("PATH", directory)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn the_tools_script_installs_only_what_is_missing_and_says_it() {
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path();
    let log = bin.join("apt.log");
    fake_system(bin, &["git", "curl"]);

    // --check changes nothing and says it with its exit code.
    let check = install_tools(bin, &["--check"]);
    assert_eq!(check.status.code(), Some(1));
    let text = String::from_utf8_lossy(&check.stdout);
    assert!(
        text.contains("OK       git") && text.contains("Missing  tmux"),
        "{text}"
    );
    assert!(!log.exists());

    // Without --yes and without an answer, nothing is installed.
    let refused = install_tools(bin, &[]);
    assert!(refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stdout).contains("Will run: apt-get install -y"));
    assert!(!log.exists());

    let installed = install_tools(bin, &["--yes"]);
    assert!(installed.status.success(), "{installed:?}");
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "install -y ffmpeg tmux\n"
    );

    // Again: everything is there, nothing runs.
    assert!(install_tools(bin, &["--yes"]).status.success());
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);
    // An unknown tool is refused, not passed to the package manager.
    assert_eq!(install_tools(bin, &["ffmpeg;rm"]).status.code(), Some(2));
}
