use std::{
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex, RwLock},
    thread,
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use vt100::Parser;

pub struct Terminal {
    pub parser: Arc<RwLock<Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child>,
    size: (u16, u16),
}

impl Terminal {
    pub fn new(cwd: &Path) -> Result<Self> {
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system().openpty(size)?;
        let mut command = CommandBuilder::new_default_prog();
        command.cwd(cwd);
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let parser = Arc::new(RwLock::new(Parser::new(size.rows, size.cols, 1_000)));
        let reader_parser = Arc::clone(&parser);
        let reader_writer = Arc::clone(&writer);
        thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let data = &buffer[..count];
                        if let Ok(mut parser) = reader_parser.write() {
                            parser.process(data);
                        }
                        if data.windows(4).any(|window| window == b"\x1b[6n") {
                            let (row, column) = reader_parser
                                .read()
                                .map(|parser| parser.screen().cursor_position())
                                .unwrap_or((0, 0));
                            if let Ok(mut writer) = reader_writer.lock() {
                                let response = format!("\x1b[{};{}R", row + 1, column + 1);
                                let _ = writer.write_all(response.as_bytes());
                                let _ = writer.flush();
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            size: (size.cols, size.rows),
        })
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
        let pty_size = PtySize {
            rows: size.1,
            cols: size.0,
            pixel_width: 0,
            pixel_height: 0,
        };
        let _ = self.master.resize(pty_size);
        if let Ok(mut parser) = self.parser.write() {
            parser.screen_mut().set_size(size.1, size.0);
        }
    }

    pub fn send_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::PageUp => self.scroll(true, self.size.1 as usize),
            KeyCode::PageDown => self.scroll(false, self.size.1 as usize),
            _ => {
                self.scroll_to_live();
                if let Some(bytes) = key_to_bytes(key) {
                    self.write(&bytes);
                }
            }
        }
    }

    pub fn send_text(&mut self, text: &str) {
        self.scroll_to_live();
        self.write(text.as_bytes());
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
        let _ = self.child.kill();
    }
}

fn key_to_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
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
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(1) => b"\x1bOP".to_vec(),
        KeyCode::F(2) => b"\x1bOQ".to_vec(),
        KeyCode::F(3) => b"\x1bOR".to_vec(),
        KeyCode::F(4) => b"\x1bOS".to_vec(),
        KeyCode::F(number @ 5..=12) => {
            let code = match number {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => unreachable!(),
            };
            format!("\x1b[{code}~").into_bytes()
        }
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
        assert_eq!(key_to_bytes(key), Some(vec![3]));
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(enter), Some(b"\r".to_vec()));
    }

    #[test]
    fn both_shells_execute_independent_commands_and_resize() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut bottom = Terminal::new(&cwd).unwrap();
        let mut right = Terminal::new(&cwd).unwrap();
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
        let mut terminal = Terminal::new(&cwd).unwrap();
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
