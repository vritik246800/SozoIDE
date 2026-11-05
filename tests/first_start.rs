//! What only a release build does, as it reads and writes the settings (F36, F16): run with
//! `cargo test --release --test first_start`.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;

use common::Home;

/// Commands of the machine a run sees: `name` is a script with `body`.
fn command(home: &Home, name: &str, body: &str) {
    let path = home.path().join("bin").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `/usr/bin` without what a test pretends is missing cannot be had: only this folder and `sh`.
fn bare_path(home: &Home) -> String {
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["sh", "stty", "cat", "id", "uname"] {
        for folder in ["/bin", "/usr/bin"] {
            let source = std::path::Path::new(folder).join(tool);
            if source.exists() {
                let _ = std::os::unix::fs::symlink(&source, bin.join(tool));
                break;
            }
        }
    }
    bin.display().to_string()
}

/// A package manager this machine seems to have. Nothing runs it: shells are login shells and
/// put the real `PATH` back, so the tests stop at the dialog that shows the command.
fn manager(home: &Home) {
    let name = if cfg!(target_os = "macos") {
        "brew"
    } else {
        "apt-get"
    };
    command(home, name, "exit 1");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "needs a release build")]
fn the_panel_comes_first_installs_what_is_chosen_and_does_not_come_back() {
    let home = Home::fresh();
    let path = bare_path(&home);
    manager(&home);
    command(&home, "git", "echo git version 2.0");
    let environment = [("PATH", path)];

    let sozo = home.start_with(&environment);
    sozo.wait_for("Starting...");
    sozo.send(" ");
    sozo.wait_for("Best experience");
    sozo.wait_for("Installed");
    sozo.wait_for("[x]");
    let screen = sozo.screen();
    for text in ["Git", "curl", "FFmpeg", "tmux", "Dependencies", "Skip"] {
        assert!(screen.contains(text), "{text}: {screen}");
    }
    assert!(
        !screen.contains("Tutorial 1/"),
        "the tutorial waits: {screen}"
    );

    // Leave curl out: down to its row and Space.
    sozo.send(b"\x1b[B");
    sozo.send(" ");
    sozo.send(b"\r");
    sozo.wait_for("Install the tools");
    let dialog = sozo.screen();
    assert!(dialog.contains("FFmpeg, tmux"), "{dialog}");
    assert!(
        dialog.contains("install ffmpeg") || dialog.contains("ffmpeg"),
        "{dialog}"
    );
    assert!(!dialog.contains("install curl"), "{dialog}");
    // Esc there runs nothing and goes back to the panel.
    sozo.send(b"\x1b");
    sozo.wait_until("the dialog gone", |screen| {
        !screen.contains("Install the tools")
    });
    assert!(sozo.screen().contains("Best experience"));
    sozo.send(b"\x1b");
    sozo.wait_for("Tutorial 1/");
    sozo.quit();

    let again = home.start_with(&environment);
    again.wait_for("Starting...");
    again.send(" ");
    again.wait_for("Explorer");
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(
        !again.screen().contains("Best experience"),
        "{}",
        again.screen()
    );
    again.quit();
}

#[test]
#[cfg_attr(debug_assertions, ignore = "needs a release build")]
fn skipping_keeps_everything_working_and_a_new_tool_asks_once_more() {
    let home = Home::fresh();
    let path = bare_path(&home);
    let environment = [("PATH", path)];
    // As a version from before tmux left it: the rest was offered already.
    home.settings("seen = \"99.0.0\"\noffered = [\"git\", \"curl\", \"ffmpeg\"]\n");
    home.write("visible.txt", "text\n");
    let sozo = home.start_with(&environment);
    sozo.wait_for("Starting...");
    sozo.send(" ");
    sozo.wait_for("Best experience");
    sozo.send(b"\x1b");
    sozo.wait_until("the panel gone", |screen| {
        !screen.contains("Best experience")
    });
    sozo.wait_for("Dependencies");
    // Everything answers: the Explorer hides and comes back.
    sozo.send(b"\x02");
    sozo.wait_until("hidden", |screen| !screen.contains("visible.txt"));
    sozo.quit();
    let text = std::fs::read_to_string(home.config().join("update.toml")).unwrap();
    assert!(text.contains("tmux"), "{text}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "needs a release build")]
fn nothing_missing_means_no_panel() {
    let home = Home::fresh();
    let bin = home.path().join("bin");
    let path = bare_path(&home);
    for tool in ["git", "curl", "ffmpeg", "tmux"] {
        command(&home, tool, "echo 1.0");
    }
    assert!(bin.join("tmux").exists());
    let sozo = home.start_with(&[("PATH", path)]);
    sozo.wait_for("Starting...");
    sozo.send(" ");
    sozo.wait_for("Tutorial 1/");
    assert!(!sozo.screen().contains("Best experience"));
}

/// A `curl` that serves a release of `9.9.9` with this package and these checksums.
fn release(home: &Home, sums: &str) {
    std::fs::write(home.path().join("sums"), sums).unwrap();
    command(
        home,
        "curl",
        &format!(
            r#"home='{}'
case "$*" in
*-sSIL*) printf 'content-length: 3\r\n' ;;
*-sSI*) printf 'location: https://github.com/x/y/releases/tag/v9.9.9\r\n' ;;
*SHA256SUMS*) [ -s "$home/sums" ] && exec /bin/cat "$home/sums"; exit 22 ;;
*-o*) for last; do :; done
    while [ $# -gt 0 ]; do [ "$1" = -o ] && file=$2; shift; done
    printf abc > "$file" ;;
esac"#,
            home.path().display()
        ),
    );
}

fn update_is_refused(sums: &str, message: &str) {
    let home = Home::fresh();
    let path = bare_path(&home);
    release(&home, sums);
    home.settings("seen = \"99.0.0\"\noffered = [\"git\", \"curl\", \"ffmpeg\", \"tmux\"]\n");
    let sozo = home.start_with(&[("PATH", path), ("SOZO_NO_UPDATE_CHECK", String::new())]);
    sozo.wait_for("Starting...");
    sozo.send(" ");
    sozo.wait_for("9.9.9");
    sozo.send(b"\r");
    sozo.wait_for(message);
    // Nothing is left to install, and the app goes on.
    let left = std::fs::read_dir(home.path())
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("SozoIDE-"));
    assert!(!left, "the package was left behind");
    sozo.send(b"\x1b");
    sozo.wait_until("the dialog gone", |screen| !screen.contains(message));
    sozo.quit();
}

#[test]
#[cfg_attr(debug_assertions, ignore = "needs a release build")]
fn an_update_that_does_not_match_its_checksum_is_not_installed() {
    let names = [
        "macos-arm64.dmg",
        "linux-x86_64.tar.gz",
        "linux-arm64.tar.gz",
    ];
    let sums = names
        .iter()
        .map(|name| format!("{}  SozoIDE-{name}\n", "0".repeat(64)))
        .collect::<String>();
    update_is_refused(&sums, "does not match");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "needs a release build")]
fn an_update_without_checksums_is_not_installed() {
    update_is_refused("", "SHA256SUMS");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "needs a release build")]
fn each_folder_opens_with_the_layout_it_was_left_with() {
    let home = Home::new();
    home.write("visible.txt", "text\n");
    let sozo = home.start();
    sozo.past_splash();
    sozo.send(b"\n");
    sozo.wait_for("Bottom terminal");
    let bottom = sozo.find("Bottom terminal");
    sozo.drag(bottom, (1, 70));
    sozo.wait_until("the terminal on top", |_| {
        sozo.locate("Bottom terminal")
            .is_some_and(|(row, _)| row <= 2)
    });
    let place = sozo.find("Bottom terminal");
    sozo.quit();

    let sozo = home.start();
    sozo.past_splash();
    sozo.wait_for("Bottom terminal");
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!(sozo.find("Bottom terminal"), place);
    sozo.quit();

    // A file from another version with a broken layout opens with the first one.
    let sessions = home.config().join("sessions.toml");
    let text = std::fs::read_to_string(&sessions).unwrap();
    assert!(text.contains("BottomTerminal"), "{text}");
    let broken = text.replace("BottomTerminal", "Editor");
    std::fs::write(&sessions, broken).unwrap();
    let sozo = home.start();
    sozo.past_splash();
    sozo.wait_for("Bottom terminal");
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(sozo.find("Bottom terminal").0 > 10, "{}", sozo.screen());
    sozo.quit();
}
