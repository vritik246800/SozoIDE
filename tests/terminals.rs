//! The terminals of SozoIDE from the outside (F37, F38): a program in a real shell in a real
//! PTY asks for what full-screen programs ask for and writes down what it receives.
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};

use common::{Home, Sozo, contains, wait_for_file};

/// A program that switches `modes` on, says it is ready, and writes everything it reads to a
/// file until it is killed.
fn recorder(home: &Home, modes: &str) -> (PathBuf, String) {
    let log = home.path().join("received");
    let script = home.path().join("program.sh");
    std::fs::write(
        &script,
        format!(
            "stty raw -echo\nprintf '{modes}\\033[2J\\033[HREADY_MARK'\nexec cat > '{}'\n",
            log.display()
        ),
    )
    .unwrap();
    (log, format!("sh '{}'\r", script.display()))
}

/// SozoIDE with the bottom terminal on screen and its shell waiting.
fn with_terminal(home: &Home) -> Sozo {
    let sozo = home.start();
    sozo.past_splash();
    sozo.send(b"\n");
    sozo.wait_for("Bottom terminal");
    std::thread::sleep(std::time::Duration::from_millis(700));
    sozo
}

fn run(sozo: &Sozo, line: &str) {
    sozo.send(line);
}

const FULL_SCREEN_WITH_MOUSE: &str = r"\033[?1049h\033[?1000h\033[?1002h\033[?1006h";

#[test]
fn a_full_screen_program_gets_the_wheel_and_the_mouse_on_its_own_cells() {
    let home = Home::new();
    let (log, line) = recorder(&home, FULL_SCREEN_WITH_MOUSE);
    let sozo = with_terminal(&home);
    run(&sozo, &line);
    sozo.wait_for("READY_MARK");
    let (top, left) = sozo.find("READY_MARK");
    // Cell 3 of row 2 of the program, wherever the panel is.
    let cell = (top + 2, left + 3);
    sozo.wheel(true, cell);
    sozo.wheel(false, cell);
    sozo.click(cell);
    sozo.drag(cell, (cell.0, cell.1 + 2));
    let received = wait_for_file(&log, |bytes| contains(bytes, b"\x1b[<0;6;3m"));
    let text = String::from_utf8_lossy(&received);
    assert!(
        text.starts_with("\x1b[<64;4;3M\x1b[<65;4;3M\x1b[<0;4;3M\x1b[<0;4;3m"),
        "{text:?}"
    );
    assert!(text.contains("\x1b[<32;6;3M"), "the drag: {text:?}");
    assert!(
        !sozo.screen().contains("[history"),
        "the wheel left the history alone"
    );
}

#[test]
fn a_full_screen_program_without_the_mouse_gets_arrows() {
    let home = Home::new();
    let (log, line) = recorder(&home, r"\033[?1049h");
    let sozo = with_terminal(&home);
    run(&sozo, &line);
    sozo.wait_for("READY_MARK");
    let (top, left) = sozo.find("READY_MARK");
    sozo.wheel(false, (top + 1, left));
    let received = wait_for_file(&log, |bytes| bytes.len() >= 9);
    assert_eq!(received, b"\x1b[B\x1b[B\x1b[B");
}

#[test]
fn a_paste_is_marked_for_a_program_that_asked_and_plain_for_one_that_did_not() {
    let home = Home::new();
    let (log, line) = recorder(&home, r"\033[?2004h");
    let sozo = with_terminal(&home);
    assert!(
        sozo.asked_for_paste(),
        "the terminal around has to mark pastes"
    );
    run(&sozo, &line);
    sozo.wait_for("READY_MARK");
    sozo.send(b"\x1b[200~one\ntwo\x1b[201~");
    let received = wait_for_file(&log, |bytes| contains(bytes, b"\x1b[201~"));
    assert_eq!(received, b"\x1b[200~one\ntwo\x1b[201~");

    let home = Home::new();
    let (log, line) = recorder(&home, "");
    let sozo = with_terminal(&home);
    run(&sozo, &line);
    sozo.wait_for("READY_MARK");
    sozo.send(b"\x1b[200~plain\x1b[201~");
    assert_eq!(wait_for_file(&log, |bytes| !bytes.is_empty()), b"plain");
}

#[test]
fn every_ctrl_key_reaches_the_program_and_the_panels_stay() {
    let home = Home::new();
    let (log, line) = recorder(&home, r"\033[?1049h");
    let sozo = with_terminal(&home);
    run(&sozo, &line);
    sozo.wait_for("READY_MARK");
    // Ctrl+B, J (a new line byte), G, D, P, then an arrow, Ctrl+Left and PageUp.
    sozo.send(b"\x02\x0a\x07\x04\x10\x1b[A\x1b[1;5D\x1b[5~");
    let received = wait_for_file(&log, |bytes| contains(bytes, b"\x1b[5~"));
    assert_eq!(received, b"\x02\x0a\x07\x04\x10\x1b[A\x1b[1;5D\x1b[5~");
    let screen = sozo.screen();
    assert!(
        screen.contains("Bottom terminal") && screen.contains("Explorer"),
        "{screen}"
    );
    assert!(
        !screen.contains("Add Connection") && !screen.contains("Find in file"),
        "{screen}"
    );
}

#[test]
fn f6_leaves_the_terminal_and_the_ctrl_keys_are_commands_again() {
    let home = Home::new();
    home.write("visible.txt", "text\n");
    let sozo = with_terminal(&home);
    sozo.wait_for("visible.txt");
    sozo.send(b"\x1b[17~");
    sozo.send(b"\x02");
    sozo.wait_until("the Explorer hidden", |screen| {
        !screen.contains("visible.txt")
    });
    sozo.send(b"\x02");
    sozo.wait_for("visible.txt");
    sozo.quit();
}

#[test]
fn ctrl_d_ends_what_reads_the_terminal_and_ctrl_c_stops_what_runs() {
    let home = Home::new();
    let sozo = with_terminal(&home);
    let output = home.path().join("typed");
    run(&sozo, &format!("cat > '{}'\r", output.display()));
    std::thread::sleep(std::time::Duration::from_millis(400));
    sozo.send("from the keyboard\r");
    sozo.send(b"\x04");
    wait_for_file(&output, |bytes| contains(bytes, b"from the keyboard"));
    run(&sozo, "echo AFTER_EOF\r");
    sozo.wait_for("AFTER_EOF");
    run(&sozo, "sleep 60\r");
    std::thread::sleep(std::time::Duration::from_millis(300));
    sozo.send(b"\x03");
    run(&sozo, "echo AFTER_INTERRUPT\r");
    sozo.wait_for("AFTER_INTERRUPT");
    sozo.quit();
}

#[test]
fn a_shell_scrolls_its_history_with_the_wheel_and_typing_comes_back() {
    let home = Home::new();
    let sozo = with_terminal(&home);
    run(&sozo, "seq 1 500\r");
    sozo.wait_for("500");
    let (row, column) = sozo.find("500");
    sozo.wheel(true, (row, column));
    sozo.wait_for("[history: 3]");
    sozo.wheel(true, (row, column));
    sozo.wait_for("[history: 6]");
    sozo.wheel(false, (row, column));
    sozo.wait_for("[history: 3]");
    run(&sozo, "e");
    sozo.wait_until("the live screen", |screen| !screen.contains("[history"));
}

#[test]
fn programs_are_answered_what_they_ask_the_terminal() {
    let home = Home::new();
    let log = home.path().join("received");
    let script = home.path().join("asks.sh");
    std::fs::write(
        &script,
        format!(
            "stty raw -echo\nprintf '\\033[2J\\033[4;9H\\033[6n\\033[c\\033[5n\\033[>q'\nexec cat > '{}'\n",
            log.display()
        ),
    )
    .unwrap();
    let sozo = with_terminal(&home);
    run(&sozo, &format!("sh '{}'\r", script.display()));
    let received = wait_for_file(&log, |bytes| contains(bytes, b"SozoIDE"));
    let text = String::from_utf8_lossy(&received);
    assert!(
        text.starts_with("\x1b[4;9R\x1b[?62;22c\x1b[0n\x1bP>|SozoIDE "),
        "{text:?}"
    );
}

#[test]
fn shells_are_told_what_this_terminal_speaks_whatever_is_around_it() {
    let home = Home::new();
    let sozo = home.start_with(&[("TERM", "tmux-256color".to_string())]);
    sozo.past_splash();
    sozo.send(b"\n");
    sozo.wait_for("Bottom terminal");
    std::thread::sleep(std::time::Duration::from_millis(700));
    run(&sozo, "echo \"T=$TERM C=$COLORTERM\"\r");
    sozo.wait_for("T=xterm-256color C=truecolor");
}

#[test]
fn text_a_program_copies_reaches_the_status_bar_and_the_terminal_around() {
    let home = Home::new();
    let sozo = with_terminal(&home);
    // "hello" in base64, split so that the shell line does not contain the sequence itself.
    run(&sozo, "printf '\\033]52;c;aGVsbG8=\\007'\r");
    sozo.wait_for("Copied 5 characters");
}

#[test]
fn a_program_that_ends_gives_the_wheel_back_to_the_shell() {
    let home = Home::new();
    let script = home.path().join("leaves.sh");
    std::fs::write(
        &script,
        format!("printf '{FULL_SCREEN_WITH_MOUSE}'\nprintf '\\033[?1000l\\033[?1002l\\033[?1006l\\033[?1049l'\necho LEFT_MARK\n"),
    )
    .unwrap();
    let sozo = with_terminal(&home);
    run(&sozo, "seq 1 200\r");
    sozo.wait_for("200");
    run(&sozo, &format!("sh '{}'\r", script.display()));
    sozo.wait_for("LEFT_MARK");
    let (row, column) = sozo.find("LEFT_MARK");
    sozo.wheel(true, (row, column));
    sozo.wait_for("[history: 3]");
}

fn tmux_here() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn sessions(home: &Path) -> String {
    let output = std::process::Command::new("tmux")
        .env("TMUX_TMPDIR", home)
        .args(["list-sessions", "-F", "#S"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Debug builds never read the settings (like the tests of the app): `cargo test --release`.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "needs a release build, which reads the settings"
)]
fn with_tmux_on_a_shell_outlives_the_app_and_comes_back() {
    if !tmux_here() {
        eprintln!("no tmux here");
        return;
    }
    let home = Home::new();
    home.settings(
        "seen = \"99.0.0\"\noffered = [\"git\", \"curl\", \"ffmpeg\", \"tmux\"]\ntmux = true\n",
    );
    let sozo = home.start();
    sozo.past_splash();
    sozo.send(b"\n");
    sozo.wait_for("Bottom terminal");
    std::thread::sleep(std::time::Duration::from_secs(1));
    run(&sozo, "export KEEP_MARK=alive\r");
    let names = sessions(home.path());
    assert_eq!(names.lines().count(), 3, "{names}");
    assert!(
        names.lines().all(|name| name.starts_with("sozo-")),
        "{names}"
    );
    sozo.quit();
    assert_eq!(
        sessions(home.path()).lines().count(),
        3,
        "they stay after quitting"
    );

    let sozo = home.start();
    sozo.past_splash();
    sozo.wait_for("Bottom terminal");
    let (row, column) = sozo.find("Bottom terminal");
    sozo.click((row + 2, column + 2));
    std::thread::sleep(std::time::Duration::from_millis(600));
    run(&sozo, "echo BACK_$KEEP_MARK\r");
    sozo.wait_for("BACK_alive");
    sozo.quit();
    let _ = std::process::Command::new("tmux")
        .env("TMUX_TMPDIR", home.path())
        .arg("kill-server")
        .status();
}
