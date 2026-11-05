//! SozoIDE in a real PTY: what the tests send is what a terminal sends, and what they read is
//! the screen a terminal would show. Each one has its own home, so nothing touches the machine.
#![allow(dead_code)]

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub const ROWS: u16 = 42;
pub const COLUMNS: u16 = 150;
const WAIT: Duration = Duration::from_secs(15);

pub struct Sozo {
    child: Box<dyn Child>,
    /// Kept open: dropping it hangs up on the app.
    _master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    screen: Arc<Mutex<vt100::Parser>>,
}

impl Drop for Sozo {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Where a run lives: the project and the folders of the user.
pub struct Home {
    pub directory: tempfile::TempDir,
}

impl Home {
    /// A machine that has run this version before: a release build opens no dialog.
    pub fn new() -> Self {
        let home = Self::fresh();
        home.settings(&format!(
            "seen = \"{}\"\noffered = [\"git\", \"curl\", \"ffmpeg\", \"tmux\"]\n",
            env!("CARGO_PKG_VERSION")
        ));
        home
    }

    /// A machine that never ran SozoIDE.
    pub fn fresh() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("project")).unwrap();
        std::fs::write(directory.path().join(".zshrc"), "PS1='P> '\n").unwrap();
        Self { directory }
    }

    pub fn project(&self) -> PathBuf {
        self.directory.path().join("project")
    }

    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    pub fn write(&self, name: &str, text: impl AsRef<[u8]>) -> PathBuf {
        let path = self.project().join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    /// The settings file of this home, with what a previous run would have left.
    pub fn settings(&self, text: &str) {
        let path = self.config().join("update.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub fn config(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.path().join("Library/Application Support/SozoIDE")
        } else {
            self.path().join(".config/sozoide")
        }
    }

    pub fn start(&self) -> Sozo {
        self.start_with(&[])
    }

    pub fn start_with(&self, environment: &[(&str, String)]) -> Sozo {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLUMNS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let binary = std::env::var("SOZO_TEST_BIN")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_sozo").to_string());
        let mut command = CommandBuilder::new(binary);
        command.cwd(self.project());
        command.arg(".");
        for (name, value) in [
            ("HOME", self.path().display().to_string()),
            ("USERPROFILE", self.path().display().to_string()),
            (
                "XDG_CONFIG_HOME",
                self.path().join(".config").display().to_string(),
            ),
            ("ZDOTDIR", self.path().display().to_string()),
            ("TMUX_TMPDIR", self.path().display().to_string()),
            ("TMPDIR", self.path().display().to_string()),
            ("SHELL", "/bin/sh".to_string()),
            ("PS1", "P> ".to_string()),
            ("ENV", String::new()),
            ("TERM", "xterm-256color".to_string()),
            ("SOZO_NO_UPDATE_CHECK", "1".to_string()),
            ("SOZO_NO_TELEMETRY", "1".to_string()),
        ] {
            if !environment
                .iter()
                .any(|(name_given, _)| *name_given == name)
            {
                command.env(name, value);
            }
        }
        for (name, value) in environment {
            if *name == "SOZO_NO_UPDATE_CHECK" && value.is_empty() {
                command.env_remove(name);
            } else {
                command.env(name, value);
            }
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLUMNS, 0)));
        let (reader_writer, reader_screen) = (Arc::clone(&writer), Arc::clone(&screen));
        thread::spawn(move || {
            let mut buffer = [0_u8; 65_536];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let data = &buffer[..count];
                reader_screen.lock().unwrap().process(data);
                // What crossterm and ratatui-image wait for at startup.
                for (question, answer) in [
                    (&b"\x1b[5n"[..], &b"\x1b[0n"[..]),
                    (b"\x1b[6n", b"\x1b[1;1R"),
                ] {
                    if data.windows(4).any(|window| window == question) {
                        let mut writer = reader_writer.lock().unwrap();
                        let _ = writer.write_all(answer);
                        let _ = writer.flush();
                    }
                }
            }
        });
        Sozo {
            child,
            _master: pair.master,
            writer,
            screen,
        }
    }
}

impl Sozo {
    pub fn send(&self, bytes: impl AsRef<[u8]>) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes.as_ref()).unwrap();
        writer.flush().unwrap();
        drop(writer);
        thread::sleep(Duration::from_millis(120));
    }

    pub fn screen(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }

    pub fn rows(&self) -> Vec<String> {
        self.screen().lines().map(str::to_owned).collect()
    }

    pub fn wait_for(&self, text: &str) {
        self.wait_until(text, |screen| screen.contains(text));
    }

    pub fn wait_until(&self, what: &str, done: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if done(&self.screen()) {
                return;
            }
            thread::sleep(Duration::from_millis(40));
        }
        panic!("{what} not reached:\n{}", self.screen());
    }

    /// The first start: the splash goes with a key, and the IDE is on screen.
    pub fn past_splash(&self) {
        self.wait_for("Starting...");
        self.send(" ");
        self.wait_for("Explorer");
        thread::sleep(Duration::from_millis(400));
    }

    /// Row and column, from 0, of `text` on screen.
    pub fn find(&self, text: &str) -> (u16, u16) {
        self.locate(text)
            .unwrap_or_else(|| panic!("{text:?} is not on screen:\n{}", self.screen()))
    }

    pub fn locate(&self, text: &str) -> Option<(u16, u16)> {
        let screen = self.screen.lock().unwrap();
        let screen = screen.screen();
        let (rows, columns) = screen.size();
        (0..rows).find_map(|row| {
            let line = (0..columns)
                .map(|column| {
                    let cell = screen.cell(row, column);
                    match cell.map(vt100::Cell::contents) {
                        Some(contents) if !contents.is_empty() => {
                            contents.chars().next().unwrap_or(' ')
                        }
                        _ => ' ',
                    }
                })
                .collect::<Vec<_>>();
            let wanted = text.chars().collect::<Vec<_>>();
            line.windows(wanted.len())
                .position(|window| window == wanted)
                .map(|column| (row, column as u16))
        })
    }

    /// Whether the app asked the terminal around it to mark pastes.
    pub fn asked_for_paste(&self) -> bool {
        self.screen.lock().unwrap().screen().bracketed_paste()
    }

    /// The colour of the cell at `row`, `column`.
    pub fn colour(&self, row: u16, column: u16) -> vt100::Color {
        let screen = self.screen.lock().unwrap();
        screen
            .screen()
            .cell(row, column)
            .map_or(vt100::Color::Default, vt100::Cell::fgcolor)
    }

    /// A left press, moves and release from cell `from` (row, column) to `to`.
    pub fn drag(&self, from: (u16, u16), to: (u16, u16)) {
        self.send(press(from));
        self.send(format!("\x1b[<32;{};{}M", to.1 + 1, to.0 + 1));
        self.send(format!("\x1b[<0;{};{}m", to.1 + 1, to.0 + 1));
    }

    pub fn click(&self, cell: (u16, u16)) {
        self.send(press(cell));
        self.send(format!("\x1b[<0;{};{}m", cell.1 + 1, cell.0 + 1));
    }

    pub fn wheel(&self, up: bool, cell: (u16, u16)) {
        let code = if up { 64 } else { 65 };
        self.send(format!("\x1b[<{code};{};{}M", cell.1 + 1, cell.0 + 1));
    }

    /// Ctrl+Q and the exit code of the app, with the terminal it leaves behind.
    pub fn quit(mut self) {
        self.send(b"\x11");
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "{status:?}");
                return;
            }
            thread::sleep(Duration::from_millis(40));
        }
        panic!("the app did not quit:\n{}", self.screen());
    }
}

fn press(cell: (u16, u16)) -> String {
    format!("\x1b[<0;{};{}M", cell.1 + 1, cell.0 + 1)
}

/// Waits for `path` to hold what `done` looks for.
pub fn wait_for_file(path: &Path, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    loop {
        let bytes = std::fs::read(path).unwrap_or_default();
        if done(&bytes) {
            return bytes;
        }
        assert!(
            Instant::now() < deadline,
            "{}: {:?}",
            path.display(),
            String::from_utf8_lossy(&bytes)
        );
        thread::sleep(Duration::from_millis(40));
    }
}

pub fn contains(bytes: &[u8], wanted: &[u8]) -> bool {
    bytes.windows(wanted.len()).any(|window| window == wanted)
}
