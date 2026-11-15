use std::process::Command;

#[test]
fn help_and_version_are_available_without_starting_the_ui() {
    let binary = env!("CARGO_BIN_EXE_sozo");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("Usage: sozo [DIRECTORY | FILE[:LINE[:COLUMN]]]"));
    assert!(help.contains("Defaults to the current directory"));

    let version = Command::new(binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("sozo {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn diagnose_completions_and_the_manual_print_and_leave() {
    let binary = env!("CARGO_BIN_EXE_sozo");
    // A home folder with a name that must never show up in the report.
    let home = std::env::temp_dir().join("sozo-diagnose-marker");
    let run = |args: &[&str]| {
        Command::new(binary)
            .args(args)
            .env("HOME", &home)
            .output()
            .unwrap()
    };
    let diagnosis = run(&["--diagnose"]);
    assert!(diagnosis.status.success());
    let text = String::from_utf8(diagnosis.stdout).unwrap();
    assert!(
        text.contains(&format!("SozoIDE {}", env!("CARGO_PKG_VERSION"))) && text.contains("Tools:")
    );
    assert!(!text.contains("sozo-diagnose-marker"), "no personal paths");
    let bash = run(&["--completions", "bash"]);
    assert!(
        bash.status.success()
            && String::from_utf8_lossy(&bash.stdout)
                .contains("complete -o filenames -F _sozo sozo")
    );
    assert!(!run(&["--completions", "tcsh"]).status.success());
    assert!(String::from_utf8_lossy(&run(&["--man"]).stdout).starts_with(".TH SOZO 1"));
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
/// as root, and `apt-get` writes what it was asked and "installs" the tools (`update` alone
/// has nothing to shift: dash, the `sh` of Ubuntu, stops there, E85).
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
            "echo \"$@\" >> '{}'\n[ \"$1\" = install ] || exit 0\nshift 2\nfor t in \"$@\"; do printf '#!/bin/sh\\n' > \"{bin}/$t\"; /bin/chmod +x \"{bin}/$t\"; done",
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
        // The package lists first: a system just installed has none (TODO 6.2.2).
        "update\ninstall -y ffmpeg tmux\n"
    );

    // Again: everything is there, nothing runs.
    assert!(install_tools(bin, &["--yes"]).status.success());
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().lines().count(),
        2,
        "nothing new ran"
    );
    // An unknown tool is refused, not passed to the package manager.
    assert_eq!(install_tools(bin, &["ffmpeg;rm"]).status.code(), Some(2));
}

/// The launcher of the macOS app, with its `osascript` and its `/usr/local/bin` swapped for
/// ones of the test. The fake `osascript` writes each argument on a line of `log` and answers
/// Install.
#[cfg(target_os = "macos")]
fn launcher(folder: &std::path::Path, bin: &std::path::Path) -> (std::path::PathBuf, String) {
    use std::os::unix::fs::PermissionsExt;
    let macos = folder.join("SozoIDE.app/Contents/MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    std::fs::write(macos.join("sozo"), "").unwrap();
    let log = folder.join("osascript.log");
    let fake = folder.join("osascript");
    std::fs::write(
        &fake,
        "#!/bin/sh\nfor argument; do printf '%s\\n' \"$argument\" >> \"$(dirname \"$0\")/osascript.log\"; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/macos/launcher.sh"),
    )
    .unwrap();
    assert!(
        script.contains("BIN=/usr/local/bin\n")
            && script.contains("OSASCRIPT=/usr/bin/osascript\n")
    );
    // The paths go in single quotes; the ones of the test have a quote of their own.
    let quoted =
        |path: &std::path::Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
    let script = script
        .replace("BIN=/usr/local/bin\n", &format!("BIN={}\n", quoted(bin)))
        .replace(
            "OSASCRIPT=/usr/bin/osascript\n",
            &format!("OSASCRIPT={}\n", quoted(&fake)),
        );
    let launcher = macos.join("SozoIDE");
    std::fs::write(&launcher, script).unwrap();
    let status = Command::new("/bin/bash").arg(&launcher).status().unwrap();
    assert!(status.success());
    (macos.join("sozo"), std::fs::read_to_string(log).unwrap())
}

#[cfg(target_os = "macos")]
#[test]
fn the_launcher_never_puts_the_path_of_the_app_inside_a_command() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    // A folder that would end a quoted string, run a command and split in two.
    let folder = directory.path().join("It's a \"te$t\" `dir`; touch pwned");
    let bin = folder.join("bin");
    std::fs::create_dir_all(&bin).unwrap();

    // The folder of the command can be written: the link is made without asking for more.
    let (sozo, log) = launcher(&folder, &bin);
    assert_eq!(std::fs::read_link(bin.join("sozo")).unwrap(), sozo);
    let path = folder.display().to_string();
    assert!(!log.contains(&path), "the path went into osascript: {log}");
    assert!(
        log.contains("do script (quoted form of item 1 of argv) & \" .\""),
        "{log}"
    );
    assert_eq!(log.lines().last(), Some("sozo"));

    // It cannot: the paths reach osascript as arguments of their own, after `--`, and the
    // script that runs as administrator only names them as items of `argv`.
    std::fs::remove_file(bin.join("sozo")).unwrap();
    std::fs::remove_file(folder.join("osascript.log")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (sozo, log) = launcher(&folder, &bin);
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    let app = folder.join("SozoIDE.app").display().to_string();
    let arguments = lines.iter().position(|line| *line == "--").unwrap();
    assert_eq!(
        lines[arguments + 1..arguments + 5],
        [
            app.as_str(),
            &sozo.display().to_string(),
            &bin.display().to_string(),
            &bin.join("sozo").display().to_string()
        ]
    );
    let privileged = lines
        .iter()
        .find(|line| line.contains("with administrator privileges"))
        .unwrap();
    assert!(!privileged.contains(&path), "{privileged}");
    assert!(privileged.contains("& theApp &") && privileged.contains("& theLink with"));
    // Without the command installed, Terminal gets the whole path, again as an argument.
    assert_eq!(lines.last(), Some(&sozo.display().to_string().as_str()));
    assert!(!directory.path().join("pwned").exists() && !folder.join("pwned").exists());
}
