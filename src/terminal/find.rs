//! The history of a terminal searched, cleared and opened where it points (TODO 5.1.2–5.1.4):
//! every line of the scrollback, the matches of a search, the title and the folder a program
//! says (OSC 0/2, OSC 7), and the links of its output (OSC 8, URLs, `file:line`).

use std::path::{Path, PathBuf};

use vt100::Parser;

use super::{Replies, Terminal};

/// Where a click in a terminal points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// An `http` or `https` address, for the browser.
    Url(String),
    /// A file of the project, its line and column from 1.
    File(PathBuf, usize, usize),
}

/// The text a program gives a terminal (a title, a link) without control characters, at most
/// `limit` characters.
pub(super) fn clean(bytes: &[u8], limit: usize) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

/// An address the browser may open: `http` or `https`, nothing else (no `javascript:`,
/// `file:`, `data:`), without spaces or control characters.
pub fn web_address(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let ok = (lower.starts_with("https://") || lower.starts_with("http://"))
        && text.len() > "http://".len()
        && text.len() <= 2048
        && !text.chars().any(|c| c.is_control() || c.is_whitespace());
    ok.then(|| text.to_owned())
}

/// The folder of an OSC 7 `file://host/path` with its `%` escapes undone.
pub(super) fn osc7_folder(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let mut bytes = Vec::new();
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = (input.next()? as char).to_digit(16)?;
            let low = (input.next()? as char).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    let text = String::from_utf8(bytes).ok()?;
    (!text.chars().any(char::is_control)).then(|| PathBuf::from(text))
}

/// What is under `column` of `line`: a link a program gave (OSC 8, `links` as its text and
/// address), an address, or `path:line[:column]` of a file inside `root`.
pub fn link_in(line: &str, column: usize, links: &[(String, String)], root: &Path) -> Option<Link> {
    let characters: Vec<char> = line.chars().collect();
    for (text, address) in links.iter().rev() {
        let needle: Vec<char> = text.chars().collect();
        if needle.is_empty() {
            continue;
        }
        let hit = (0..characters.len().saturating_sub(needle.len() - 1))
            .filter(|start| characters[*start..].starts_with(&needle))
            .any(|start| (start..start + needle.len()).contains(&column));
        if hit {
            if let Some(url) = web_address(address) {
                return Some(Link::Url(url));
            }
            if let Some(path) = address
                .strip_prefix("file://")
                .and_then(|_| osc7_folder(address))
            {
                return inside(root, &path).map(|path| Link::File(path, 1, 1));
            }
            return None;
        }
    }
    // The word under the column, without the brackets and quotes around it.
    if column >= characters.len() || characters[column].is_whitespace() {
        return None;
    }
    let start = (0..column)
        .rev()
        .find(|at| characters[*at].is_whitespace())
        .map_or(0, |at| at + 1);
    let end = (column..characters.len())
        .find(|at| characters[*at].is_whitespace())
        .unwrap_or(characters.len());
    let word: String = characters[start..end].iter().collect();
    let word = word
        .trim_matches(|c: char| "()[]{}<>\"'`,;".contains(c))
        .trim_end_matches(['.', ':']);
    if let Some(url) = web_address(word) {
        return Some(Link::Url(url));
    }
    // `src/main.rs:10:5`, `src/main.rs:10`, or `--> src/main.rs:10:5` of a compiler.
    let mut parts = word.split(':');
    let path = parts.next()?;
    let row = parts.next().and_then(|row| row.parse::<usize>().ok());
    let col = parts
        .next()
        .and_then(|col| col.parse::<usize>().ok())
        .unwrap_or(1);
    let row = row?;
    inside(root, Path::new(path)).map(|path| Link::File(path, row.max(1), col.max(1)))
}

/// `path` (relative to `root` or absolute) when it is a file inside `root`, links resolved.
fn inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let full = full.canonicalize().ok()?;
    (full.starts_with(&root) && full.is_file()).then_some(full)
}

/// Every line of `parser`: the history first, then the screen, without trailing spaces.
pub(super) fn all_lines(parser: &mut Parser<Replies>) -> Vec<String> {
    let screen = parser.screen_mut();
    let kept = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let back = screen.scrollback();
    let (rows, columns) = screen.size();
    let mut lines: Vec<String> = Vec::new();
    let mut offset = back;
    loop {
        screen.set_scrollback(offset);
        let top = back - offset;
        for (index, row) in screen.rows(0, columns).enumerate() {
            if top + index == lines.len() {
                lines.push(row.trim_end().to_owned());
            }
        }
        if offset == 0 {
            break;
        }
        offset = offset.saturating_sub(usize::from(rows.max(1)));
    }
    screen.set_scrollback(kept);
    lines
}

/// Each occurrence of `query` (any case) in `lines`: line, column and length in characters.
pub fn find_in(lines: &[String], query: &str) -> Vec<(usize, usize, usize)> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (number, line) in lines.iter().enumerate() {
        let text: Vec<char> = line.to_lowercase().chars().collect();
        let mut at = 0;
        while at + query.len() <= text.len() {
            if text[at..at + query.len()] == query[..] {
                found.push((number, at, query.len()));
                at += query.len();
            } else {
                at += 1;
            }
            if found.len() >= 10_000 {
                return found;
            }
        }
    }
    found
}

/// A search of the history of a terminal: what is typed, what it found and which one is shown.
#[derive(Clone, Debug, Default)]
pub struct TerminalSearch {
    pub query: String,
    pub matches: Vec<(usize, usize, usize)>,
    pub index: usize,
}

impl Terminal {
    /// Every line of the history and the screen.
    pub fn lines(&self) -> Vec<String> {
        self.parser
            .write()
            .map(|mut parser| all_lines(&mut parser))
            .unwrap_or_default()
    }

    /// Scrolls so that line `line` (of `lines`) is on screen.
    pub fn show_line(&self, line: usize) {
        if let Ok(mut parser) = self.parser.write() {
            let screen = parser.screen_mut();
            screen.set_scrollback(usize::MAX);
            let back = screen.scrollback();
            screen.set_scrollback(back.saturating_sub(line));
        }
    }

    /// The history goes; the screen stays as it is (also when a program sends `ESC [3J`).
    pub fn clear_history(&self) {
        if let Ok(mut parser) = self.parser.write() {
            clear(&mut parser);
        }
    }

    /// The title the program set (OSC 0/2), cleaned.
    pub fn title(&self) -> Option<String> {
        self.parser.read().ok()?.callbacks().title.clone()
    }

    /// The folder the shell said it is in (OSC 7), for a new tab to start there.
    pub fn folder(&self) -> Option<PathBuf> {
        self.parser.read().ok()?.callbacks().folder.clone()
    }

    /// Where the cell `(row, column)` on screen points, within `root`.
    pub fn link_at(&self, cell: (u16, u16), root: &Path) -> Option<Link> {
        let parser = self.parser.read().ok()?;
        let screen = parser.screen();
        let (_, columns) = screen.size();
        let line = screen.rows(0, columns).nth(usize::from(cell.0))?;
        link_in(&line, usize::from(cell.1), &parser.callbacks().links, root)
    }
}

/// A parser with the same screen, size and callbacks and no history.
pub(super) fn clear(parser: &mut Parser<Replies>) {
    let (rows, columns) = parser.screen().size();
    let screen = parser.screen().contents_formatted();
    let callbacks = std::mem::take(parser.callbacks_mut());
    let mut fresh = Parser::new_with_callbacks(rows, columns, super::scrollback_rows(), callbacks);
    fresh.process(&screen);
    fresh.callbacks_mut().clear = false;
    *parser = fresh;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_web_addresses_or_files_of_the_project_and_nothing_else() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let line = "  --> src/main.rs:10:5 see https://example.com/a?b=1.";
        assert_eq!(
            link_in(line, 8, &[], root),
            Some(Link::File(
                root.canonicalize().unwrap().join("src/main.rs"),
                10,
                5
            ))
        );
        assert_eq!(
            link_in(line, 30, &[], root),
            Some(Link::Url("https://example.com/a?b=1".into()))
        );
        assert_eq!(link_in(line, 1, &[], root), None, "a space points nowhere");
        // Hostile: other schemes, files outside the project, a link given by OSC 8.
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "../outside.rs:1",
            "/etc/hosts:1",
        ] {
            assert_eq!(link_in(bad, 2, &[], root), None, "{bad}");
        }
        let osc = vec![
            ("click me".to_owned(), "javascript:alert(1)".to_owned()),
            ("docs".to_owned(), "https://example.com/docs".to_owned()),
        ];
        assert_eq!(
            link_in("open docs now", 6, &osc, root),
            Some(Link::Url("https://example.com/docs".into()))
        );
        assert_eq!(link_in("please click me", 10, &osc, root), None);
        let outside = vec![("passwd".to_owned(), "file:///etc/passwd".to_owned())];
        assert_eq!(link_in("passwd", 1, &outside, root), None);
        assert_eq!(web_address("https://a.b/\u{7}x"), None);
        assert_eq!(
            osc7_folder("file://host/home/dev/my%20dir"),
            Some(PathBuf::from("/home/dev/my dir"))
        );
        assert_eq!(
            osc7_folder("file://host/a%0ab"),
            None,
            "a control character is refused"
        );
        assert_eq!(osc7_folder("file://host/a%zz"), None);
        assert_eq!(
            clean(b"build \x1b]0;evil\x07 done\n", 100),
            "build ]0;evil done"
        );
    }

    #[test]
    fn the_history_is_searched_shown_and_cleared() {
        let mut parser = Parser::new_with_callbacks(4, 20, 100, Replies::default());
        for number in 0..30 {
            parser.process(format!("line {number}\r\n").as_bytes());
        }
        parser.process(b"Needle here");
        let lines = all_lines(&mut parser);
        assert_eq!(lines[0], "line 0");
        assert_eq!(lines[29], "line 29");
        assert!(lines.iter().any(|line| line == "Needle here"));
        assert_eq!(parser.screen().scrollback(), 0, "the view is where it was");
        assert_eq!(
            find_in(&lines, "LINE 2"),
            [
                (2, 0, 6),
                (20, 0, 6),
                (21, 0, 6),
                (22, 0, 6),
                (23, 0, 6),
                (24, 0, 6),
                (25, 0, 6),
                (26, 0, 6),
                (27, 0, 6),
                (28, 0, 6),
                (29, 0, 6)
            ]
        );
        assert!(find_in(&lines, "").is_empty());
        // A program's `clear` (`ESC [3J`) drops the history, the screen stays.
        parser.process(b"\x1b[3J");
        assert!(parser.callbacks().clear);
        clear(&mut parser);
        let lines = all_lines(&mut parser);
        assert!(lines.len() <= 4, "{lines:?}");
        assert!(lines.iter().any(|line| line == "Needle here"));
    }

    #[test]
    fn hostile_output_never_writes_to_the_shell_nor_opens_what_it_should_not() {
        let mut parser = Parser::new_with_callbacks(10, 60, 100, Replies::default());
        // A title with control characters and a sequence inside, links to `javascript:` and to a
        // file outside, a folder with a new line, and a request to read the clipboard.
        parser.process(b"\x1b]2;rm -rf ~\x0d\x0a\x1b[31mevil\x07");
        parser.process(b"\x1b]8;;javascript:alert(1)\x1b\\click\x1b]8;;\x1b\\ ");
        parser.process(b"\x1b]8;;file:///etc/passwd\x1b\\passwd\x1b]8;;\x1b\\ ");
        parser.process(b"\x1b]7;file://host/tmp/a%0Ab\x07");
        parser.process(b"\x1b]52;c;?\x07");
        let replies = &parser.callbacks();
        assert!(
            replies.pending.is_empty(),
            "nothing is typed back: {:?}",
            String::from_utf8_lossy(&replies.pending)
        );
        assert!(
            replies.copied.is_empty(),
            "the clipboard is never read for a program"
        );
        let title = replies.title.clone().unwrap();
        assert!(!title.chars().any(char::is_control), "{title:?}");
        assert_eq!(replies.folder, None);
        let root = tempfile::tempdir().unwrap();
        let line: String = parser.screen().rows(0, 60).next().unwrap();
        for column in [0, 7] {
            assert_eq!(
                link_in(&line, column, &replies.links, root.path()),
                None,
                "{line} @ {column}"
            );
        }
    }
}
