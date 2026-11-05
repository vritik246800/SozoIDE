use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex, RwLock},
    thread,
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use vt100::{MouseProtocolEncoding, MouseProtocolMode, Parser, Screen};

/// A PTY and the shell running in it.
type Shell = (Box<dyn MasterPty + Send>, Box<dyn Child>);

/// Rows of history a shell keeps (F37).
const SCROLLBACK: usize = 10_000;
/// Rows a turn of the wheel moves where it scrolls.
const WHEEL_ROWS: usize = 3;

/// What the program in a terminal asked of it and the parser cannot answer alone (F37).
#[derive(Default)]
pub struct Replies {
    /// Bytes owed to the program: read and written after each chunk of its output.
    pending: Vec<u8>,
    /// Text it put on the clipboard (OSC 52), in base64.
    copied: Vec<Vec<u8>>,
}

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut Screen,
        first: Option<u8>,
        _second: Option<u8>,
        params: &[&[u16]],
        character: char,
    ) {
        let parameter = params
            .first()
            .and_then(|values| values.first())
            .copied()
            .unwrap_or(0);
        let (row, column) = screen.cursor_position();
        let (rows, columns) = screen.size();
        let reply = match (first, character, parameter) {
            // Who is there: a VT220 with colours, as xterm says.
            (None, 'c', 0) => "\x1b[?62;22c".to_owned(),
            (Some(b'>'), 'c', 0) => "\x1b[>0;10;0c".to_owned(),
            (None, 'n', 5) => "\x1b[0n".to_owned(),
            (None, 'n', 6) => format!("\x1b[{};{}R", row + 1, column + 1),
            (Some(b'?'), 'n', 6) => format!("\x1b[?{};{}R", row + 1, column + 1),
            (Some(b'>'), 'q', 0) => {
                format!("\x1bP>|SozoIDE {}\x1b\\", env!("CARGO_PKG_VERSION"))
            }
            (None, 't', 18) => format!("\x1b[8;{rows};{columns}t"),
            _ => return,
        };
        self.pending.extend_from_slice(reply.as_bytes());
    }

    fn copy_to_clipboard(&mut self, _: &mut Screen, _kind: &[u8], data: &[u8]) {
        self.copied.push(data.to_vec());
    }
}

/// What the mouse did in a terminal, for the program running there. Buttons: 0 left, 1 middle,
/// 2 right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press(u8),
    Release(u8),
    /// Moved with a button held, or with none.
    Motion(Option<u8>),
    Wheel {
        up: bool,
    },
}

pub struct Terminal {
    pub parser: Arc<RwLock<Parser<Replies>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// The PTY and its shell, or why this system could not open them (E36).
    shell: std::result::Result<Shell, String>,
    size: (u16, u16),
    /// The tmux session it attached to (F38), which outlives this terminal.
    session: Option<String>,
}

/// The shells of a terminal panel, one per tab (F04): `active` is the one on screen.
pub struct Terminals {
    pub tabs: Vec<Terminal>,
    pub active: usize,
    /// What the tmux sessions of this panel start with, when tmux is on (F38).
    tmux: Option<String>,
}

impl Terminals {
    pub fn new(cwd: &Path, count: usize, tmux: Option<String>) -> Self {
        let mut panel = Self {
            tabs: Vec::new(),
            active: 0,
            tmux,
        };
        for _ in 0..count.max(1) {
            panel.add(cwd);
        }
        panel.active = 0;
        panel
    }

    /// Tmux is on or off for the shells to come (F38).
    pub fn set_tmux(&mut self, tmux: Option<String>) {
        self.tmux = tmux;
    }

    pub fn current(&self) -> &Terminal {
        &self.tabs[self.active]
    }

    pub fn current_mut(&mut self) -> &mut Terminal {
        &mut self.tabs[self.active]
    }

    /// A new shell in a tab of its own, which comes on screen.
    pub fn add(&mut self, cwd: &Path) {
        // The lowest number free: a tab opens on the session that tab had before.
        let session = self.tmux.as_ref().map(|prefix| {
            (1..)
                .map(|number| format!("{prefix}-{number}"))
                .find(|name| {
                    self.tabs
                        .iter()
                        .all(|tab| tab.session.as_ref() != Some(name))
                })
                .unwrap_or_default()
        });
        self.tabs.push(Terminal::new(cwd, session));
        self.active = self.tabs.len() - 1;
    }

    /// Closes tab `index` with its shell; the tab after it, or the last one, comes on screen.
    /// A panel keeps its last shell.
    pub fn close(&mut self, index: usize) {
        if self.tabs.len() < 2 || index >= self.tabs.len() {
            return;
        }
        // Closed for good: only leaving the IDE keeps a session (F38).
        if let Some(session) = &self.tabs.remove(index).session {
            kill_session(session);
        }
        if index < self.active || self.active == self.tabs.len() {
            self.active -= 1;
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    /// The next tab, or the previous one, round the ends.
    pub fn step(&mut self, forward: bool) {
        let count = self.tabs.len();
        self.active = (self.active + if forward { 1 } else { count - 1 }) % count;
    }

    /// The text the programs of this panel put on the clipboard (OSC 52).
    pub fn take_copied(&mut self) -> Vec<String> {
        self.tabs
            .iter_mut()
            .flat_map(Terminal::take_copied)
            .collect()
    }

    /// Every shell of the panel takes the size of its area.
    pub fn resize(&mut self, size: (u16, u16)) {
        for terminal in &mut self.tabs {
            terminal.resize(size);
        }
    }
}

impl Terminal {
    /// A shell in `cwd`, or the tmux session `session` when there is one (F38); without them,
    /// a terminal that says why (a Windows without ConPTY, E36).
    pub fn new(cwd: &Path, session: Option<String>) -> Self {
        Self::spawn(cwd, session).unwrap_or_else(|error| Self::unavailable(format!("{error:#}")))
    }

    fn spawn(cwd: &Path, session: Option<String>) -> Result<Self> {
        // portable-pty panics on a Windows without ConPTY instead of failing.
        #[cfg(windows)]
        if !conpty_available() {
            anyhow::bail!("Terminals need Windows 10 version 1809 (October 2018) or newer");
        }
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system().openpty(size)?;
        let mut command = match &session {
            Some(session) => {
                let mut command = CommandBuilder::new("tmux");
                command.args(tmux_arguments(session, cwd));
                // Inside another tmux, this one would refuse to start.
                command.env_remove("TMUX");
                command
            }
            None => CommandBuilder::new_default_prog(),
        };
        command.cwd(cwd);
        // What this terminal speaks, whatever the terminal around SozoIDE is (F37).
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let parser = Arc::new(RwLock::new(Parser::new_with_callbacks(
            size.rows,
            size.cols,
            SCROLLBACK,
            Replies::default(),
        )));
        let reader_parser = Arc::clone(&parser);
        let reader_writer = Arc::clone(&writer);
        thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let replies = reader_parser.write().map(|mut parser| {
                            parser.process(&buffer[..count]);
                            std::mem::take(&mut parser.callbacks_mut().pending)
                        });
                        if let (Ok(replies), Ok(mut writer)) = (replies, reader_writer.lock())
                            && !replies.is_empty()
                        {
                            let _ = writer.write_all(&replies);
                            let _ = writer.flush();
                        }
                    }
                }
            }
        });

        Ok(Self {
            parser,
            writer,
            shell: Ok((pair.master, child)),
            size: (size.cols, size.rows),
            session,
        })
    }

    /// A terminal without a shell: it shows `message` and takes no keys.
    pub fn unavailable(message: String) -> Self {
        let size = (80, 24);
        let mut parser = Parser::new_with_callbacks(size.1, size.0, 0, Replies::default());
        parser.process(message.as_bytes());
        Self {
            parser: Arc::new(RwLock::new(parser)),
            writer: Arc::new(Mutex::new(Box::new(std::io::sink()))),
            shell: Err(message),
            size,
            session: None,
        }
    }

    /// The text from cell `start` to cell `end` (row and column on screen, both included, in
    /// either order), with the rows that wrap joined.
    pub fn text_between(&self, start: (u16, u16), end: (u16, u16)) -> String {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        self.parser
            .read()
            .map(|parser| {
                parser
                    .screen()
                    .contents_between(start.0, start.1, end.0, end.1.saturating_add(1))
            })
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    pub fn resize(&mut self, size: (u16, u16)) {
        if size == self.size || size.0 == 0 || size.1 == 0 {
            return;
        }
        self.size = size;
        let master = match &self.shell {
            Ok((master, _)) => master,
            Err(message) => {
                // The message wraps again at the new width.
                if let Ok(mut parser) = self.parser.write() {
                    *parser = Parser::new_with_callbacks(size.1, size.0, 0, Replies::default());
                    parser.process(message.as_bytes());
                }
                return;
            }
        };
        let pty_size = PtySize {
            rows: size.1,
            cols: size.0,
            pixel_width: 0,
            pixel_height: 0,
        };
        let _ = master.resize(pty_size);
        if let Ok(mut parser) = self.parser.write() {
            parser.screen_mut().set_size(size.1, size.0);
        }
    }

    pub fn send_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::PageUp if self.keeps_history() => self.scroll(true, self.size.1 as usize),
            KeyCode::PageDown if self.keeps_history() => self.scroll(false, self.size.1 as usize),
            _ => {
                self.scroll_to_live();
                if let Some(bytes) = key_to_bytes(key, self.reading(Screen::application_cursor)) {
                    self.write(&bytes);
                }
            }
        }
    }

    pub fn send_text(&mut self, text: &str) {
        self.scroll_to_live();
        self.write(text.as_bytes());
    }

    /// Pasted text: a program that asked for it can tell it from typing (bracketed paste).
    pub fn paste(&mut self, text: &str) {
        if self.reading(Screen::bracketed_paste) {
            self.send_text(&format!("\x1b[200~{text}\x1b[201~"));
        } else {
            self.send_text(text);
        }
    }

    /// The history of this shell is what PageUp, PageDown and the wheel move; a full-screen
    /// program has none and takes them itself (F37).
    fn keeps_history(&self) -> bool {
        !self.reading(Screen::alternate_screen) && !self.wants_mouse()
    }

    fn reading(&self, read: impl Fn(&Screen) -> bool) -> bool {
        self.parser.read().is_ok_and(|parser| read(parser.screen()))
    }

    /// Whether the program here asked for the mouse (F37).
    pub fn wants_mouse(&self) -> bool {
        self.parser
            .read()
            .is_ok_and(|parser| parser.screen().mouse_protocol_mode() != MouseProtocolMode::None)
    }

    /// Tells the program here what the mouse did on cell `cell` (row, column); false when it
    /// did not ask for this.
    pub fn send_mouse(
        &mut self,
        action: MouseAction,
        cell: (u16, u16),
        modifiers: KeyModifiers,
    ) -> bool {
        let Ok(parser) = self.parser.read() else {
            return false;
        };
        let screen = parser.screen();
        let bytes = mouse_bytes(
            action,
            cell,
            modifiers,
            screen.mouse_protocol_mode(),
            screen.mouse_protocol_encoding(),
        );
        drop(parser);
        match bytes {
            Some(bytes) => {
                self.write(&bytes);
                true
            }
            None => false,
        }
    }

    /// A turn of the wheel on cell `cell`, as a terminal does it: to the program that asked for
    /// the mouse, as arrows to a full-screen one that did not (the programs that page), and
    /// through the history in a shell.
    pub fn wheel(&mut self, up: bool, cell: (u16, u16), modifiers: KeyModifiers) {
        if self.send_mouse(MouseAction::Wheel { up }, cell, modifiers) {
            return;
        }
        if self.keeps_history() {
            self.scroll(up, WHEEL_ROWS);
            return;
        }
        let code = if up { KeyCode::Up } else { KeyCode::Down };
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        for _ in 0..WHEEL_ROWS {
            self.send_key(key);
        }
    }

    /// The text programs here put on the clipboard since the last time (OSC 52).
    fn take_copied(&mut self) -> Vec<String> {
        self.parser.write().map_or_else(
            |_| Vec::new(),
            |mut parser| {
                std::mem::take(&mut parser.callbacks_mut().copied)
                    .iter()
                    .filter_map(|data| base64_text(data))
                    .collect()
            },
        )
    }

    pub fn scroll(&self, up: bool, amount: usize) {
        let current = self
            .parser
            .read()
            .map(|parser| parser.screen().scrollback())
            .unwrap_or(0);
        let offset = if up {
            current.saturating_add(amount)
        } else {
            current.saturating_sub(amount)
        };
        if let Ok(mut parser) = self.parser.write() {
            parser.screen_mut().set_scrollback(offset);
        }
    }

    pub fn scroll_to_live(&self) {
        if let Ok(mut parser) = self.parser.write() {
            parser.screen_mut().set_scrollback(0);
        }
    }

    pub fn scrollback(&self) -> usize {
        self.parser
            .read()
            .map(|parser| parser.screen().scrollback())
            .unwrap_or(0)
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Ok((_, child)) = &mut self.shell {
            let _ = child.kill();
        }
    }
}

/// Whether this Windows has ConPTY: `CreatePseudoConsole` in `kernel32`, which portable-pty
/// loads with a panic when it is missing (Windows 10 before 1809, E36).
#[cfg(windows)]
fn conpty_available() -> bool {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }
    let kernel32 = "kernel32.dll\0".encode_utf16().collect::<Vec<_>>();
    // SAFETY: both names end with a NUL, and kernel32 is loaded in every Windows process.
    unsafe {
        let module = GetModuleHandleW(kernel32.as_ptr());
        !module.is_null()
            && !GetProcAddress(module, c"CreatePseudoConsole".as_ptr().cast()).is_null()
    }
}

/// The arguments of `tmux` for the session `session` in `cwd`: attaches to it, or starts it,
/// without its status bar and with the wheel reaching the programs in it. The user's own
/// configuration stays as it is.
fn tmux_arguments(session: &str, cwd: &Path) -> Vec<String> {
    [
        "new-session",
        "-A",
        "-s",
        session,
        "-c",
        &cwd.to_string_lossy(),
        ";",
        "set-option",
        "-q",
        "status",
        "off",
        ";",
        "set-option",
        "-q",
        "mouse",
        "on",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn kill_session(session: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", session])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The first part of the tmux sessions of a folder's panel (F38): the same folder finds its
/// sessions again, and two folders of the same name do not share them.
pub fn tmux_prefix(root: &Path, panel: &str) -> String {
    let path = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // FNV-1a: stable between versions, which the hasher of the standard library is not.
    let hash = path
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("sozo-{hash:016x}-{panel}")
}

/// What a program that asked for the mouse reads; `None` when it did not ask for this event
/// or the cell does not fit the encoding.
fn mouse_bytes(
    action: MouseAction,
    (row, column): (u16, u16),
    modifiers: KeyModifiers,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    let wanted = match action {
        MouseAction::Press(_) | MouseAction::Wheel { .. } => mode != MouseProtocolMode::None,
        MouseAction::Release(_) => matches!(
            mode,
            MouseProtocolMode::PressRelease
                | MouseProtocolMode::ButtonMotion
                | MouseProtocolMode::AnyMotion
        ),
        MouseAction::Motion(Some(_)) => matches!(
            mode,
            MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
        ),
        MouseAction::Motion(None) => mode == MouseProtocolMode::AnyMotion,
    };
    if !wanted {
        return None;
    }
    let button = match action {
        MouseAction::Press(button) => u32::from(button),
        MouseAction::Release(button) if encoding == MouseProtocolEncoding::Sgr => u32::from(button),
        // The older encodings do not say which one was let go.
        MouseAction::Release(_) => 3,
        MouseAction::Motion(button) => 32 + button.map_or(3, u32::from),
        MouseAction::Wheel { up } => 64 + u32::from(!up),
    };
    let held = |modifier, value| {
        if modifiers.contains(modifier) {
            value
        } else {
            0
        }
    };
    let code = button
        + held(KeyModifiers::SHIFT, 4)
        + held(KeyModifiers::ALT, 8)
        + held(KeyModifiers::CONTROL, 16);
    let (column, row) = (u32::from(column), u32::from(row));
    match encoding {
        MouseProtocolEncoding::Sgr => {
            let end = if matches!(action, MouseAction::Release(_)) {
                'm'
            } else {
                'M'
            };
            Some(format!("\x1b[<{code};{};{}{end}", column + 1, row + 1).into_bytes())
        }
        MouseProtocolEncoding::Default => {
            let byte = |value: u32| u8::try_from(value + 32).ok();
            let mut bytes = b"\x1b[M".to_vec();
            bytes.extend([byte(code)?, byte(column + 1)?, byte(row + 1)?]);
            Some(bytes)
        }
        MouseProtocolEncoding::Utf8 => {
            let character = |value: u32| char::from_u32(value + 32);
            let text = [
                character(code)?,
                character(column + 1)?,
                character(row + 1)?,
            ];
            Some(format!("\x1b[M{}", text.iter().collect::<String>()).into_bytes())
        }
    }
}

/// The text inside an OSC 52.
fn base64_text(data: &[u8]) -> Option<String> {
    let mut bytes = Vec::with_capacity(data.len() / 4 * 3);
    let (mut bits, mut pending) = (0_u32, 0_u32);
    for byte in data.iter().filter(|byte| **byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        pending += 6;
        if pending >= 8 {
            pending -= 8;
            bytes.push((bits >> pending) as u8);
            bits &= (1 << pending) - 1;
        }
    }
    String::from_utf8(bytes).ok()
}

/// The bytes of `key`; arrows follow the mode the program chose (`application`, DECCKM) and
/// keys with Shift, Alt or Ctrl say so as xterm does.
fn key_to_bytes(key: KeyEvent, application: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let letter = match key.code {
        KeyCode::Up => Some(('A', true)),
        KeyCode::Down => Some(('B', true)),
        KeyCode::Right => Some(('C', true)),
        KeyCode::Left => Some(('D', true)),
        KeyCode::Home => Some(('H', true)),
        KeyCode::End => Some(('F', true)),
        KeyCode::F(1) => Some(('P', false)),
        KeyCode::F(2) => Some(('Q', false)),
        KeyCode::F(3) => Some(('R', false)),
        KeyCode::F(4) => Some(('S', false)),
        _ => None,
    };
    if let Some((letter, cursor)) = letter {
        return Some(
            if modifier > 1 {
                format!("\x1b[1;{modifier}{letter}")
            } else if cursor && !application {
                format!("\x1b[{letter}")
            } else {
                format!("\x1bO{letter}")
            }
            .into_bytes(),
        );
    }
    let code = match key.code {
        KeyCode::Insert => Some(2),
        KeyCode::Delete => Some(3),
        KeyCode::PageUp => Some(5),
        KeyCode::PageDown => Some(6),
        KeyCode::F(5) => Some(15),
        KeyCode::F(6) => Some(17),
        KeyCode::F(7) => Some(18),
        KeyCode::F(8) => Some(19),
        KeyCode::F(9) => Some(20),
        KeyCode::F(10) => Some(21),
        KeyCode::F(11) => Some(23),
        KeyCode::F(12) => Some(24),
        _ => None,
    };
    if let Some(code) = code {
        return Some(
            if modifier > 1 {
                format!("\x1b[{code};{modifier}~")
            } else {
                format!("\x1b[{code}~")
            }
            .into_bytes(),
        );
    }
    let mut bytes = match key.code {
        KeyCode::Char(character) if ctrl => {
            let value = (character as u32)
                .checked_sub('a' as u32)
                .and_then(|value| u8::try_from(value).ok());
            match character {
                ' ' | '@' => vec![0],
                '[' => vec![27],
                '\\' => vec![28],
                ']' => vec![29],
                '^' | '_' => vec![30],
                '?' => vec![127],
                _ if value.is_some() && character.is_ascii_lowercase() => vec![value.unwrap() + 1],
                _ if character.is_ascii_uppercase() => vec![(character as u8) & 0x1f],
                _ => return None,
            }
        }
        KeyCode::Char(character) => {
            let mut buffer = [0; 4];
            character.encode_utf8(&mut buffer).as_bytes().to_vec()
        }
        // Alt+Enter: a new line that does not run, in zsh, fish and command line tools (Q58).
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => b"\x1b\r".to_vec(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
        _ => return None,
    };
    if alt {
        bytes.insert(0, 27);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn converts_terminal_control_keys() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(key, false), Some(vec![3]));
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(enter, false), Some(b"\r".to_vec()));
    }

    #[test]
    fn shift_enter_writes_a_new_line_without_running_it() {
        let shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(key_to_bytes(shift_enter, false), Some(b"\x1b\r".to_vec()));
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(enter, false), Some(b"\r".to_vec()));
    }

    #[test]
    fn a_selection_reads_the_text_between_two_cells() {
        let terminal = Terminal::unavailable(String::new());
        terminal
            .parser
            .write()
            .unwrap()
            .process(b"\x1b[2J\x1b[Hfirst line\r\nsecond line\r\n");
        // Backwards too, from the end of the first row into the second one.
        assert_eq!(terminal.text_between((1, 5), (0, 6)), "line\nsecond");
        assert_eq!(terminal.text_between((0, 0), (0, 4)), "first");
        // A row that wraps comes back as one line.
        let terminal = Terminal::unavailable(String::new());
        {
            let mut parser = terminal.parser.write().unwrap();
            parser.screen_mut().set_size(4, 6);
            parser.process(b"\x1b[2J\x1b[Habcdefghij\r\nk");
        }
        assert_eq!(terminal.text_between((0, 2), (2, 0)), "cdefghij\nk");
    }

    #[test]
    fn a_terminal_without_a_shell_says_why_and_takes_no_keys() {
        let message = "Terminals need Windows 10 version 1809 (October 2018) or newer";
        let mut terminal = Terminal::unavailable(message.to_string());
        terminal.resize((30, 5));
        let shown = screen_contents(&terminal);
        // Wrapped to the width of the panel, whole.
        assert!(
            shown.starts_with("Terminals need Windows 10 ver"),
            "{shown}"
        );
        assert_eq!(shown.replace('\n', ""), message);
        terminal.send_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        terminal.send_text("ls\r");
        assert_eq!(screen_contents(&terminal).replace('\n', ""), message);
    }

    #[test]
    fn a_panel_adds_closes_and_switches_its_shells() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut panel = Terminals::new(&cwd, 2, None);
        assert_eq!((panel.tabs.len(), panel.active), (2, 0));
        panel.add(&cwd);
        assert_eq!((panel.tabs.len(), panel.active), (3, 2));
        panel.step(true);
        assert_eq!(panel.active, 0);
        panel.step(false);
        assert_eq!(panel.active, 2);
        // Closing a tab before the one on screen keeps it on screen.
        panel.select(1);
        panel.close(0);
        assert_eq!((panel.tabs.len(), panel.active), (2, 0));
        // Closing the last tab shows the one before it; the last shell stays.
        panel.select(1);
        panel.close(1);
        assert_eq!((panel.tabs.len(), panel.active), (1, 0));
        panel.close(0);
        assert_eq!(panel.tabs.len(), 1);
        panel.resize((50, 10));
        assert_eq!(panel.current().size(), (50, 10));
    }

    #[cfg(windows)]
    #[test]
    fn this_windows_has_conpty() {
        assert!(conpty_available());
    }

    #[test]
    fn both_shells_execute_independent_commands_and_resize() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut bottom = Terminal::new(&cwd, None);
        let mut right = Terminal::new(&cwd, None);
        bottom.resize((81, 24));
        right.resize((81, 24));

        for character in "echo SOZO_BOTTOM_741".chars() {
            bottom.send_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        bottom.send_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        right.send_text("echo SOZO_RIGHT_852\r");

        wait_for_output(&bottom, "SOZO_BOTTOM_741");
        wait_for_output(&right, "SOZO_RIGHT_852");
        assert!(!screen_contents(&bottom).contains("SOZO_RIGHT_852"));
        assert!(!screen_contents(&right).contains("SOZO_BOTTOM_741"));

        bottom.resize((55, 12));
        right.resize((65, 18));
        assert_eq!(bottom.parser.read().unwrap().screen().size(), (12, 55));
        assert_eq!(right.parser.read().unwrap().screen().size(), (18, 65));
    }

    #[test]
    fn scrollback_supports_mouse_keyboard_and_live_return() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut terminal = Terminal::new(&cwd, None);
        terminal.resize((40, 4));
        {
            let mut parser = terminal.parser.write().unwrap();
            for index in 0..30 {
                parser.process(format!("history {index}\r\n").as_bytes());
            }
        }
        terminal.scroll(true, 3);
        assert_eq!(terminal.scrollback(), 3);
        terminal.scroll(false, 1);
        assert_eq!(terminal.scrollback(), 2);
        terminal.send_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(terminal.scrollback(), 6);
        terminal.send_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(terminal.scrollback(), 2);
        terminal.scroll_to_live();
        assert_eq!(terminal.scrollback(), 0);
    }

    /// A terminal whose writes can be read, as the program in it would.
    fn listening() -> (Terminal, Arc<Mutex<Vec<u8>>>) {
        #[derive(Clone)]
        struct Sent(Arc<Mutex<Vec<u8>>>);
        impl Write for Sent {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut terminal = Terminal::unavailable(String::new());
        terminal.writer = Arc::new(Mutex::new(Box::new(Sent(sent.clone()))));
        terminal.size = (80, 24);
        *terminal.parser.write().unwrap() =
            Parser::new_with_callbacks(24, 80, SCROLLBACK, Replies::default());
        (terminal, sent)
    }

    fn program_prints(terminal: &Terminal, output: &[u8]) {
        terminal.parser.write().unwrap().process(output);
    }

    fn received(sent: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
        std::mem::take(&mut *sent.lock().unwrap())
    }

    #[test]
    fn the_wheel_reaches_a_full_screen_program_as_a_terminal_sends_it() {
        let (mut terminal, sent) = listening();
        for index in 0..40 {
            program_prints(&terminal, format!("history {index}\r\n").as_bytes());
        }
        // A shell: the wheel goes through the history and the program reads nothing.
        terminal.wheel(true, (0, 0), KeyModifiers::NONE);
        assert_eq!(terminal.scrollback(), WHEEL_ROWS);
        assert_eq!(received(&sent), b"");
        terminal.scroll_to_live();

        // Full screen without the mouse (`less`, `man`): arrows, as many as rows.
        program_prints(&terminal, b"\x1b[?1049h");
        terminal.wheel(false, (0, 0), KeyModifiers::NONE);
        assert_eq!(received(&sent), b"\x1b[B\x1b[B\x1b[B");
        assert_eq!(terminal.scrollback(), 0);
        program_prints(&terminal, b"\x1b[?1h");
        terminal.wheel(true, (0, 0), KeyModifiers::NONE);
        assert_eq!(received(&sent), b"\x1bOA\x1bOA\x1bOA");

        // With the mouse (Claude Code, `vim`, `htop`): the wheel itself, on the cell it was on.
        program_prints(&terminal, b"\x1b[?1000h\x1b[?1006h");
        assert!(terminal.wants_mouse());
        terminal.wheel(true, (4, 9), KeyModifiers::NONE);
        assert_eq!(received(&sent), b"\x1b[<64;10;5M");
        terminal.wheel(false, (4, 9), KeyModifiers::NONE);
        assert_eq!(received(&sent), b"\x1b[<65;10;5M");
        // The main screen with the mouse is the program's too.
        program_prints(&terminal, b"\x1b[?1049l");
        terminal.wheel(true, (0, 0), KeyModifiers::NONE);
        assert_eq!(received(&sent), b"\x1b[<64;1;1M");
        assert_eq!(terminal.scrollback(), 0);
    }

    #[test]
    fn page_keys_belong_to_the_program_on_a_full_screen() {
        let (mut terminal, sent) = listening();
        for index in 0..40 {
            program_prints(&terminal, format!("history {index}\r\n").as_bytes());
        }
        terminal.send_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert!(terminal.scrollback() > 0);
        assert_eq!(received(&sent), b"");
        terminal.scroll_to_live();
        program_prints(&terminal, b"\x1b[?1049h");
        terminal.send_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(terminal.scrollback(), 0);
        assert_eq!(received(&sent), b"\x1b[5~");
    }

    #[test]
    fn mouse_events_follow_the_mode_and_the_encoding_the_program_chose() {
        use MouseProtocolEncoding::{Default, Sgr, Utf8};
        use MouseProtocolMode::{AnyMotion, ButtonMotion, None as Off, Press, PressRelease};
        let none = KeyModifiers::NONE;
        let cell = (1, 2);
        let bytes = |action, mode, encoding| mouse_bytes(action, cell, none, mode, encoding);
        let press = MouseAction::Press(0);
        let release = MouseAction::Release(2);
        let drag = MouseAction::Motion(Some(0));
        let moved = MouseAction::Motion(None);
        assert_eq!(bytes(press, Off, Sgr), None);
        assert_eq!(bytes(press, Press, Sgr), Some(b"\x1b[<0;3;2M".to_vec()));
        assert_eq!(bytes(release, Press, Sgr), None);
        assert_eq!(
            bytes(release, PressRelease, Sgr),
            Some(b"\x1b[<2;3;2m".to_vec())
        );
        assert_eq!(bytes(drag, PressRelease, Sgr), None);
        assert_eq!(
            bytes(drag, ButtonMotion, Sgr),
            Some(b"\x1b[<32;3;2M".to_vec())
        );
        assert_eq!(bytes(moved, ButtonMotion, Sgr), None);
        assert_eq!(
            bytes(moved, AnyMotion, Sgr),
            Some(b"\x1b[<35;3;2M".to_vec())
        );
        // The first encoding: three bytes, 32 added, and no way to say which button was let go.
        assert_eq!(
            bytes(press, Press, Default),
            Some(vec![0x1b, b'[', b'M', 32, 35, 34])
        );
        assert_eq!(
            bytes(release, PressRelease, Default),
            Some(vec![0x1b, b'[', b'M', 35, 35, 34])
        );
        // Past its last column it cannot say where.
        let far = mouse_bytes(press, (0, 230), none, Press, Default);
        assert_eq!(far, None);
        let far = mouse_bytes(press, (0, 230), none, Press, Utf8).unwrap();
        assert_eq!(String::from_utf8(far).unwrap(), "\x1b[M \u{107}!");
        // Modifiers add to the button.
        let held = KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL;
        assert_eq!(
            mouse_bytes(press, (0, 0), held, Press, Sgr),
            Some(b"\x1b[<28;1;1M".to_vec())
        );
    }

    #[test]
    fn keys_say_their_modifiers_and_follow_the_cursor_mode() {
        let key = |code, modifiers| key_to_bytes(KeyEvent::new(code, modifiers), false);
        let none = KeyModifiers::NONE;
        assert_eq!(key(KeyCode::Up, none), Some(b"\x1b[A".to_vec()));
        assert_eq!(
            key_to_bytes(KeyEvent::new(KeyCode::Up, none), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key(KeyCode::Left, KeyModifiers::CONTROL),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            key(KeyCode::Right, KeyModifiers::ALT),
            Some(b"\x1b[1;3C".to_vec())
        );
        assert_eq!(
            key(KeyCode::Delete, KeyModifiers::SHIFT),
            Some(b"\x1b[3;2~".to_vec())
        );
        assert_eq!(key(KeyCode::F(1), none), Some(b"\x1bOP".to_vec()));
        assert_eq!(key(KeyCode::F(5), none), Some(b"\x1b[15~".to_vec()));
        assert_eq!(
            key(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(vec![4])
        );
        assert_eq!(
            key(KeyCode::Char('x'), KeyModifiers::ALT),
            Some(vec![27, b'x'])
        );
    }

    #[test]
    fn a_paste_is_marked_only_for_a_program_that_asked() {
        let (mut terminal, sent) = listening();
        terminal.paste("ls -la");
        assert_eq!(received(&sent), b"ls -la");
        program_prints(&terminal, b"\x1b[?2004h");
        terminal.paste("ls -la");
        assert_eq!(received(&sent), b"\x1b[200~ls -la\x1b[201~");
        // Commands the IDE types are not pastes.
        terminal.send_text("cargo run\r");
        assert_eq!(received(&sent), b"cargo run\r");
    }

    #[test]
    fn questions_to_the_terminal_are_answered_also_when_they_arrive_in_two_pieces() {
        let replies = |output: &[&[u8]]| {
            let (terminal, _) = listening();
            let mut parser = terminal.parser.write().unwrap();
            for piece in output {
                parser.process(piece);
            }
            std::mem::take(&mut parser.callbacks_mut().pending)
        };
        assert_eq!(replies(&[b"\x1b[3;5H\x1b[6n"]), b"\x1b[3;5R");
        assert_eq!(replies(&[b"\x1b[3;5H\x1b[", b"6n"]), b"\x1b[3;5R");
        assert_eq!(replies(&[b"\x1b[?6n"]), b"\x1b[?1;1R");
        assert_eq!(replies(&[b"\x1b[5n"]), b"\x1b[0n");
        assert_eq!(replies(&[b"\x1b[c"]), b"\x1b[?62;22c");
        assert_eq!(replies(&[b"\x1b[0c"]), b"\x1b[?62;22c");
        assert_eq!(replies(&[b"\x1b[>c"]), b"\x1b[>0;10;0c");
        assert_eq!(replies(&[b"\x1b[18t"]), b"\x1b[8;24;80t");
        assert!(replies(&[b"\x1b[>q"]).starts_with(b"\x1bP>|SozoIDE "));
        assert_eq!(replies(&[b"\x1b[2J"]), b"");
    }

    #[test]
    fn text_a_program_copies_reaches_the_clipboard() {
        let (mut terminal, _) = listening();
        program_prints(&terminal, b"\x1b]52;c;aMOpIHRoZXJl\x07");
        program_prints(&terminal, b"\x1b]52;c;bw==\x07");
        assert_eq!(terminal.take_copied(), ["h\u{e9} there", "o"]);
        assert!(terminal.take_copied().is_empty());
        assert_eq!(base64_text(b"not base64!"), None);
    }

    #[test]
    fn tmux_sessions_belong_to_the_folder_panel_and_tab() {
        let arguments = tmux_arguments("sozo-1", Path::new("/work"));
        assert_eq!(
            arguments[..6],
            ["new-session", "-A", "-s", "sozo-1", "-c", "/work"]
        );
        assert!(
            arguments
                .windows(3)
                .any(|words| words == ["-q", "status", "off"])
        );
        assert!(
            arguments
                .windows(3)
                .any(|words| words == ["-q", "mouse", "on"])
        );

        let first = tmux_prefix(Path::new("/nowhere/a"), "bottom");
        assert_eq!(first, tmux_prefix(Path::new("/nowhere/a"), "bottom"));
        assert_ne!(first, tmux_prefix(Path::new("/nowhere/b"), "bottom"));
        assert_ne!(first, tmux_prefix(Path::new("/nowhere/a"), "right"));
        assert!(first.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    fn screen_contents(terminal: &Terminal) -> String {
        terminal.parser.read().unwrap().screen().contents()
    }

    fn wait_for_output(terminal: &Terminal, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let contents = screen_contents(terminal);
            if contents
                .lines()
                .filter(|line| !line.contains("echo "))
                .any(|line| line.contains(expected))
            {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "saida '{expected}' nao encontrada; conteudo: {}",
            screen_contents(terminal)
        );
    }
}
