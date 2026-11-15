//! Editing as programmers expect it (F02, TODO 2.1.1–2.1.6): redo, indentation that follows
//! the file and `.editorconfig`, pairs of brackets and quotes, comments by the language, moving,
//! duplicating and deleting lines, and moving and deleting by words.

use std::path::Path;

use super::{Change, OpenFile, Snapshot, byte_at};

/// How a file indents: with tabs, or with `width` spaces (TODO 2.1.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Indent {
    pub tabs: bool,
    pub width: usize,
}

impl Default for Indent {
    fn default() -> Self {
        Self {
            tabs: false,
            width: 4,
        }
    }
}

impl Indent {
    pub fn unit(self) -> String {
        if self.tabs {
            "\t".into()
        } else {
            " ".repeat(self.width)
        }
    }

    pub fn label(self) -> String {
        if self.tabs {
            format!("Tabs: {}", self.width)
        } else {
            format!("Spaces: {}", self.width)
        }
    }
}

/// What the lines of a file say about its indentation: tabs when more lines start with one;
/// otherwise the smallest step between the indentations of neighbouring lines.
pub fn detect_indent(lines: &[String]) -> Option<Indent> {
    let (mut tabs, mut spaces) = (0, 0);
    let mut steps = [0usize; 9];
    let mut previous = 0usize;
    for line in lines.iter().take(2_000) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let leading = line.len() - line.trim_start_matches(' ').len();
        if leading > 0 {
            spaces += 1;
        }
        let step = leading.abs_diff(previous);
        if (2..=8).contains(&step) {
            steps[step] += 1;
        }
        previous = leading;
    }
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > spaces {
        return Some(Indent {
            tabs: true,
            width: 4,
        });
    }
    let width = (2..=8).max_by_key(|step| (steps[*step], usize::MAX - step))?;
    Some(Indent {
        tabs: false,
        width: if steps[width] == 0 { 4 } else { width },
    })
}

/// The `.editorconfig` settings of a file that SozoIDE follows (TODO 2.1.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorConfig {
    pub indent: Option<Indent>,
    pub crlf: Option<bool>,
    pub final_newline: Option<bool>,
    pub trim_trailing: Option<bool>,
}

/// Reads the `.editorconfig` files from the file's folder up to `root` (or one with
/// `root = true`); the nearest wins. Only `*`, `*.ext`, `*.{a,b}` and exact names are
/// matched — ponytail: no `**` or paths in sections; a real glob crate if projects need them.
pub fn editor_config(path: &Path, root: &Path) -> EditorConfig {
    let mut found = EditorConfig::default();
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for folder in path.ancestors().skip(1) {
        let Ok(text) = std::fs::read_to_string(folder.join(".editorconfig")) else {
            if folder == root {
                break;
            }
            continue;
        };
        // Limit: a hostile file of megabytes is read only up to here.
        let text: String = text.chars().take(64 * 1024).collect();
        let (settings, is_root) = parse_editor_config(&text, &name);
        // Nearer files were read first: they keep what they set.
        found.indent = found.indent.or(settings.indent);
        found.crlf = found.crlf.or(settings.crlf);
        found.final_newline = found.final_newline.or(settings.final_newline);
        found.trim_trailing = found.trim_trailing.or(settings.trim_trailing);
        if is_root || folder == root {
            break;
        }
    }
    found
}

fn parse_editor_config(text: &str, name: &str) -> (EditorConfig, bool) {
    let mut settings = EditorConfig::default();
    let mut is_root = false;
    let mut applies = false;
    let (mut style, mut size, mut tab_width) = (None, None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            applies = section_matches(section, name);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (
            key.trim().to_ascii_lowercase(),
            value.trim().to_ascii_lowercase(),
        );
        if key == "root" && !applies {
            is_root = value == "true";
            continue;
        }
        if !applies {
            continue;
        }
        match key.as_str() {
            "indent_style" => style = Some(value == "tab"),
            "indent_size" => {
                size = value
                    .parse::<usize>()
                    .ok()
                    .filter(|size| (1..=16).contains(size))
            }
            "tab_width" => {
                tab_width = value
                    .parse::<usize>()
                    .ok()
                    .filter(|size| (1..=16).contains(size));
            }
            "end_of_line" => settings.crlf = Some(value == "crlf"),
            "insert_final_newline" => settings.final_newline = Some(value == "true"),
            "trim_trailing_whitespace" => settings.trim_trailing = Some(value == "true"),
            _ => {}
        }
    }
    if style.is_some() || size.is_some() {
        let tabs = style.unwrap_or(false);
        settings.indent = Some(Indent {
            tabs,
            width: size.or(tab_width).unwrap_or(4),
        });
    }
    (settings, is_root)
}

fn section_matches(section: &str, name: &str) -> bool {
    if section == "*" {
        return true;
    }
    if let Some(rest) = section.strip_prefix("*.") {
        let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
        if let Some(list) = rest
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
        {
            return list.split(',').any(|item| item.trim() == extension);
        }
        return rest == extension;
    }
    if let Some(list) = section
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    {
        return list.split(',').any(|item| item.trim() == name);
    }
    section == name
}

/// The comment of a line in the language of a file, and the end of a block comment where the
/// language has no line comment (TODO 2.1.5).
pub fn comment_for(language: &str) -> Option<(&'static str, &'static str)> {
    Some(match language {
        "Rust" | "C" | "C++" | "Java" | "JavaScript" | "TypeScript" | "TypeScriptReact" | "Go"
        | "Swift" | "Kotlin" | "C#" | "Scala" | "Dart" | "PHP" | "Objective-C" | "JSON"
        | "Groovy" | "Zig" | "Astro" => ("// ", ""),
        "Python"
        | "Shell"
        | "Bourne Again Shell (bash)"
        | "Bash"
        | "Ruby"
        | "YAML"
        | "TOML"
        | "Perl"
        | "R"
        | "Makefile"
        | "Dockerfile"
        | "Elixir"
        | "Nix"
        | "PowerShell"
        | "Julia"
        | "CMake"
        | "Git Ignore"
        | "Fish" => ("# ", ""),
        "SQL" | "Lua" | "Haskell" | "Ada" | "Elm" => ("-- ", ""),
        "Lisp" | "Clojure" | "Scheme" | "INI" | "Assembly (x86_64)" => ("; ", ""),
        "LaTeX" | "TeX" | "Erlang" | "MATLAB" => ("% ", ""),
        "VimL" => ("\" ", ""),
        "HTML" | "XML" | "Markdown" | "Vue" | "Svelte" => ("<!-- ", " -->"),
        "CSS" | "SCSS" | "Less" => ("/* ", " */"),
        _ => return None,
    })
}

impl OpenFile {
    /// `Ctrl+Y` / `Ctrl+Shift+Z`: the edit undone last comes back; false when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.future.pop() else {
            return false;
        };
        self.history.push(self.snapshot());
        self.restore(snapshot);
        true
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            cursor: (self.cursor_row, self.cursor_column),
            trailing_newline: self.trailing_newline,
        }
    }

    pub(super) fn restore(&mut self, snapshot: Snapshot) {
        self.lines = snapshot.lines;
        (self.cursor_row, self.cursor_column) = snapshot.cursor;
        self.trailing_newline = snapshot.trailing_newline;
        self.anchor = None;
        self.last_change = None;
        self.changed();
    }

    /// An edit that is one step of undo however much it changes: `apply` gets the lines.
    fn edit(&mut self, apply: impl FnOnce(&mut Self)) {
        if self.read_only {
            return;
        }
        self.last_change = None;
        self.remember(Change::Paste);
        self.last_change = None;
        apply(self);
        self.clamp_cursor();
        self.changed();
    }

    /// The rows the selection covers, or the cursor's row; a selection that ends at the start
    /// of a row leaves that row out.
    fn chosen_rows(&self) -> (usize, usize) {
        match self.selection() {
            Some(((start, _), (end, column))) if end > start && column == 0 => (start, end - 1),
            Some(((start, _), (end, _))) => (start, end),
            None => (self.cursor_row, self.cursor_row),
        }
    }

    /// Enter: a new line indented as this one, one level more after an opening bracket; a
    /// closing bracket right after the cursor goes to a line of its own.
    pub fn new_line(&mut self) {
        if self.read_only {
            return;
        }
        let line = self.lines[self.cursor_row].clone();
        let at = byte_at(&line, self.cursor_column);
        let leading: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let before = line[..at].trim_end().chars().last();
        let after = line[at..].trim_start().chars().next();
        let opens = matches!(before, Some('{' | '[' | '('));
        let closes = opens
            && matches!(
                (before, after),
                (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))
            );
        let unit = self.indent.unit();
        self.remember(Change::Type);
        self.remove_selection();
        let mut text = format!("\n{leading}");
        if opens {
            text.push_str(&unit);
        }
        let column = text.chars().count() - 1;
        if closes {
            text.push('\n');
            text.push_str(&leading);
        }
        for character in text.chars() {
            self.put(character);
        }
        if closes {
            self.cursor_row -= 1;
            self.cursor_column = column;
        }
        self.changed();
    }

    /// Tab: one level in for every chosen row when there is a selection, else the file's unit.
    pub fn indent_lines(&mut self) {
        if self.selection().is_none() {
            if self.read_only {
                return;
            }
            let unit = self.indent.unit();
            self.remember(Change::Type);
            for character in unit.chars() {
                self.put(character);
            }
            return self.changed();
        }
        let (first, last) = self.chosen_rows();
        let unit = self.indent.unit();
        let added = unit.chars().count();
        self.edit(|file| {
            for line in file.lines.iter_mut().take(last + 1).skip(first) {
                if !line.is_empty() {
                    line.insert_str(0, &unit);
                }
            }
            if let Some(anchor) = &mut file.anchor {
                anchor.1 += added;
            }
            file.cursor_column += added;
        });
        self.anchor = Some((first, 0));
        self.cursor_row = last;
        self.cursor_column = self.lines[last].chars().count();
    }

    /// Shift+Tab: one level out for every chosen row.
    pub fn outdent_lines(&mut self) {
        let (first, last) = self.chosen_rows();
        let width = self.indent.width.max(1);
        let selected = self.selection().is_some();
        self.edit(|file| {
            for row in first..=last {
                let line = &mut file.lines[row];
                let removed = if line.starts_with('\t') {
                    1
                } else {
                    line.chars().take(width).take_while(|c| *c == ' ').count()
                };
                line.replace_range(..removed, "");
                if row == file.cursor_row {
                    file.cursor_column = file.cursor_column.saturating_sub(removed);
                }
            }
        });
        if selected {
            self.anchor = Some((first, 0));
            self.cursor_row = last;
            self.cursor_column = self.lines[last].chars().count();
        }
    }

    /// Typing a bracket or a quote: its pair after it where nothing follows; over a selection,
    /// around it. A closing one typed before the same one steps over it (TODO 2.1.4).
    pub fn insert_pair(&mut self, character: char) {
        let close = match character {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '"' | '\'' | '`' => character,
            _ => return self.insert(character),
        };
        if self.read_only {
            return;
        }
        if let Some(text) = self.selected_text() {
            let wrapped = format!("{character}{text}{close}");
            return self.insert_str(&wrapped);
        }
        let line = &self.lines[self.cursor_row];
        let next = line.chars().nth(self.cursor_column);
        let previous = self
            .cursor_column
            .checked_sub(1)
            .and_then(|column| line.chars().nth(column));
        // A quote after a letter is an apostrophe; before a letter it opens nothing.
        let quote = character == close;
        if quote && next == Some(close) {
            self.cursor_column += 1;
            return;
        }
        let room = next.is_none_or(|next| next.is_whitespace() || ")]}>,;:".contains(next));
        if !room || quote && previous.is_some_and(char::is_alphanumeric) {
            return self.insert(character);
        }
        self.insert(character);
        self.put(close);
        self.cursor_column -= 1;
        self.changed();
    }

    /// A closing bracket typed right before the same one steps over it.
    pub fn insert_closing(&mut self, character: char) {
        let next = self.lines[self.cursor_row].chars().nth(self.cursor_column);
        if next == Some(character) && self.selection().is_none() && !self.read_only {
            self.cursor_column += 1;
            self.follow_cursor = true;
        } else {
            self.insert(character);
        }
    }

    /// `Ctrl+/`: the chosen rows commented with the marker of the language, or uncommented
    /// when all of them already are. False when the language has no comment SozoIDE knows.
    pub fn toggle_comment(&mut self) -> bool {
        let Some((open, close)) = comment_for(self.language) else {
            return false;
        };
        let (first, last) = self.chosen_rows();
        let (open_word, close_word) = (open.trim_end(), close.trim_start());
        let rows = first..=last;
        let all = self.lines[rows.clone()]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start().starts_with(open_word));
        let column = self.lines[rows.clone()]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        self.edit(|file| {
            for row in rows {
                let line = &mut file.lines[row];
                if line.trim().is_empty() {
                    continue;
                }
                if all {
                    let start = line.len() - line.trim_start().len();
                    let mut rest = line[start + open_word.len()..].to_owned();
                    if rest.starts_with(' ') && open.ends_with(' ') {
                        rest.remove(0);
                    }
                    if !close_word.is_empty() {
                        let trimmed = rest.trim_end();
                        if let Some(inner) = trimmed.strip_suffix(close_word) {
                            rest = inner.strip_suffix(' ').unwrap_or(inner).to_owned();
                        }
                    }
                    *line = format!("{}{rest}", &line[..start]);
                } else {
                    line.insert_str(column, open);
                    if !close.is_empty() {
                        line.push_str(close);
                    }
                }
            }
        });
        true
    }

    /// `Alt+↑`/`Alt+↓`: the chosen rows change places with the row above or below.
    pub fn move_lines(&mut self, down: bool) {
        let (first, last) = self.chosen_rows();
        if down && last + 1 >= self.lines.len() || !down && first == 0 {
            return;
        }
        let anchor = self.anchor;
        self.edit(|file| {
            if down {
                let row = file.lines.remove(last + 1);
                file.lines.insert(first, row);
                file.cursor_row += 1;
            } else {
                let row = file.lines.remove(first - 1);
                file.lines.insert(last, row);
                file.cursor_row -= 1;
            }
        });
        self.anchor = anchor
            .map(|(row, column)| (if down { row + 1 } else { row.saturating_sub(1) }, column));
    }

    /// `Shift+Alt+↓`: the chosen rows once more below them, with the cursor on the copy.
    pub fn duplicate_lines(&mut self) {
        let (first, last) = self.chosen_rows();
        let count = last - first + 1;
        self.edit(|file| {
            let copy: Vec<String> = file.lines[first..=last].to_vec();
            for (offset, line) in copy.into_iter().enumerate() {
                file.lines.insert(last + 1 + offset, line);
            }
            file.cursor_row += count;
            file.anchor = None;
        });
    }

    /// `Ctrl+Shift+K`: the chosen rows removed.
    pub fn delete_lines(&mut self) {
        let (first, last) = self.chosen_rows();
        self.edit(|file| {
            file.lines.drain(first..=last);
            if file.lines.is_empty() {
                file.lines.push(String::new());
            }
            file.cursor_row = first.min(file.lines.len() - 1);
            file.anchor = None;
        });
    }

    /// Replaces `length` characters at `row`, `column` with `with`, as one step of undo; the
    /// cursor goes after it (TODO 2.1.3).
    pub fn replace_range(&mut self, row: usize, column: usize, length: usize, with: &str) {
        if row >= self.lines.len() {
            return;
        }
        self.edit(|file| {
            let line = &mut file.lines[row];
            let start = byte_at(line, column);
            let end = byte_at(line, column + length);
            line.replace_range(start..end, with);
            file.cursor_row = row;
            file.cursor_column = column + with.chars().count();
            file.anchor = None;
        });
    }

    /// The whole text replaced (edits of a language server), as one step of undo.
    pub fn replace_text(&mut self, lines: Vec<String>) {
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        self.edit(|file| {
            file.lines = lines;
            file.anchor = None;
        });
    }

    /// Every line through `replace`, as one step of undo; how many replacements it made.
    pub fn replace_lines(&mut self, replace: impl Fn(&str) -> (String, usize)) -> usize {
        let results: Vec<(String, usize)> = self.lines.iter().map(|line| replace(line)).collect();
        let total: usize = results.iter().map(|(_, count)| count).sum();
        if total == 0 || self.read_only {
            return 0;
        }
        self.edit(|file| {
            file.lines = results.into_iter().map(|(line, _)| line).collect();
            file.anchor = None;
        });
        total
    }

    /// A click on `LF`/`CRLF` in the status bar: the other line end, saved with the next save.
    pub fn toggle_line_end(&mut self) {
        if self.read_only {
            return;
        }
        self.crlf = !self.crlf;
        self.changed();
    }

    /// A click on the indentation in the status bar: tabs instead of spaces, or spaces.
    pub fn toggle_indent_style(&mut self) {
        self.indent.tabs = !self.indent.tabs;
    }

    /// The bracket at (or just before) the cursor and its pair, looked for at most 5000 lines
    /// away (TODO 2.3.4).
    pub fn matching_bracket(&self) -> Option<((usize, usize), (usize, usize))> {
        let line: Vec<char> = self.lines.get(self.cursor_row)?.chars().collect();
        let at = [self.cursor_column, self.cursor_column.wrapping_sub(1)]
            .into_iter()
            .find(|column| line.get(*column).is_some_and(|c| "()[]{}".contains(*c)))?;
        let bracket = line[at];
        let (pair, forward) = match bracket {
            '(' => (')', true),
            '[' => (']', true),
            '{' => ('}', true),
            ')' => ('(', false),
            ']' => ('[', false),
            _ => ('{', false),
        };
        let mut depth = 0usize;
        let rows: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(self.cursor_row..self.lines.len().min(self.cursor_row + 5_000))
        } else {
            Box::new((self.cursor_row.saturating_sub(5_000)..=self.cursor_row).rev())
        };
        for row in rows {
            let chars: Vec<char> = self.lines[row].chars().collect();
            let columns: Box<dyn Iterator<Item = usize>> = match (forward, row == self.cursor_row) {
                (true, true) => Box::new(at + 1..chars.len()),
                (true, false) => Box::new(0..chars.len()),
                (false, true) => Box::new((0..at).rev()),
                (false, false) => Box::new((0..chars.len()).rev()),
            };
            for column in columns {
                if chars[column] == bracket {
                    depth += 1;
                } else if chars[column] == pair {
                    if depth == 0 {
                        return Some(((self.cursor_row, at), (row, column)));
                    }
                    depth -= 1;
                }
            }
        }
        None
    }

    /// A double click: the word under the cursor selected (TODO 2.2.4).
    pub fn select_word(&mut self) {
        let line: Vec<char> = self.lines[self.cursor_row].chars().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = self.cursor_column.min(line.len());
        let mut end = start;
        while start > 0 && word(line[start - 1]) {
            start -= 1;
        }
        while end < line.len() && word(line[end]) {
            end += 1;
        }
        if start == end {
            return;
        }
        self.anchor = Some((self.cursor_row, start));
        self.cursor_column = end;
    }

    /// A triple click: the whole line, with its end.
    pub fn select_line(&mut self) {
        let row = self.cursor_row;
        self.anchor = Some((row, 0));
        if row + 1 < self.lines.len() {
            self.cursor_row = row + 1;
            self.cursor_column = 0;
        } else {
            self.cursor_column = self.lines[row].chars().count();
        }
    }

    /// `Home`: the first character that is not a space, then the start of the line.
    pub fn smart_home(&mut self) {
        let line = &self.lines[self.cursor_row];
        let first = line.chars().take_while(|c| c.is_whitespace()).count();
        let column = if self.cursor_column == first {
            0
        } else {
            first
        };
        self.move_cursor_to(self.cursor_row, column);
    }

    /// The column where the word before (or after) the cursor starts (or ends).
    pub fn word_column(&self, forward: bool) -> (usize, usize) {
        let line: Vec<char> = self.lines[self.cursor_row].chars().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut column = self.cursor_column.min(line.len());
        if forward {
            if column == line.len() {
                return if self.cursor_row + 1 < self.lines.len() {
                    (self.cursor_row + 1, 0)
                } else {
                    (self.cursor_row, column)
                };
            }
            while column < line.len() && !word(line[column]) {
                column += 1;
            }
            while column < line.len() && word(line[column]) {
                column += 1;
            }
        } else {
            if column == 0 {
                return match self.cursor_row.checked_sub(1) {
                    Some(row) => (row, self.lines[row].chars().count()),
                    None => (0, 0),
                };
            }
            while column > 0 && !word(line[column - 1]) {
                column -= 1;
            }
            while column > 0 && word(line[column - 1]) {
                column -= 1;
            }
        }
        (self.cursor_row, column)
    }

    /// `Ctrl+←`/`Ctrl+→`: to the start of the word before, the end of the word after.
    pub fn move_word(&mut self, forward: bool) {
        let (row, column) = self.word_column(forward);
        self.move_cursor_to(row, column);
    }

    /// `Ctrl+Backspace`/`Ctrl+Delete`: the word before or after the cursor removed.
    pub fn delete_word(&mut self, forward: bool) {
        if self.read_only || self.delete_selection() {
            return;
        }
        let (row, column) = self.word_column(forward);
        self.anchor = Some((row, column));
        self.delete_selection();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_image::picker::Picker;
    use std::time::{Duration, Instant};

    fn file_with(name: &str, text: &str) -> (tempfile::TempDir, OpenFile) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(name);
        std::fs::write(&path, text).unwrap();
        let file = OpenFile::load(path, &mut Picker::halfblocks()).unwrap();
        (directory, file)
    }

    #[test]
    fn redo_brings_back_what_undo_took_and_a_new_edit_forgets_it() {
        let (_directory, mut file) = file_with("a.txt", "one\n");
        file.move_cursor_to(0, 3);
        file.insert('!');
        assert!(file.undo());
        assert_eq!(file.lines[0], "one");
        assert!(file.redo());
        assert_eq!(file.lines[0], "one!");
        assert!(!file.redo(), "nothing more to redo");
        assert!(file.undo());
        file.insert('?');
        assert!(!file.redo(), "a new edit forgets what could be redone");
    }

    #[test]
    fn indentation_follows_the_file_then_its_editorconfig() {
        let lines = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            detect_indent(&lines("fn a() {\n  b();\n  if x {\n    c();\n  }\n}")),
            Some(Indent {
                tabs: false,
                width: 2
            })
        );
        assert!(detect_indent(&lines("a\n\tb\n\tc")).unwrap().tabs);
        assert_eq!(detect_indent(&lines("plain\ntext")), None);
        let (directory, _) = file_with("x.rs", "");
        let root = directory.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(
            root.join(".editorconfig"),
            "root = true\n[*]\nindent_style = space\nindent_size = 3\n[*.{js,ts}]\nindent_style = tab\n[Makefile]\nindent_style = tab\nend_of_line = crlf\n",
        )
        .unwrap();
        std::fs::write(
            root.join("sub/.editorconfig"),
            "[*.rs]\nindent_size = 8\ntrim_trailing_whitespace = true\n",
        )
        .unwrap();
        let config = |name: &str| editor_config(&root.join(name), root);
        assert_eq!(
            config("a.txt").indent,
            Some(Indent {
                tabs: false,
                width: 3
            })
        );
        assert!(config("a.ts").indent.unwrap().tabs);
        assert_eq!(config("Makefile").crlf, Some(true));
        // The nearest file wins and the rest comes from above.
        let nearest = config("sub/m.rs");
        assert_eq!(
            nearest.indent,
            Some(Indent {
                tabs: false,
                width: 8
            })
        );
        assert_eq!(nearest.trim_trailing, Some(true));
        // Hostile values are ignored, never panic.
        std::fs::write(
            root.join(".editorconfig"),
            "[*]\nindent_size = 99999\n[[[\n=\nindent_style\n",
        )
        .unwrap();
        assert_eq!(config("a.txt").indent, None);
    }

    #[test]
    fn enter_indents_tab_moves_lines_and_brackets_close_themselves() {
        let (_directory, mut file) = file_with("a.rs", "fn main() {}\n");
        file.indent = Indent {
            tabs: false,
            width: 4,
        };
        file.move_cursor_to(0, 11);
        file.new_line();
        assert_eq!(file.lines, ["fn main() {", "    ", "}"]);
        assert_eq!((file.cursor_row, file.cursor_column), (1, 4));
        file.insert_pair('(');
        assert_eq!(file.lines[1], "    ()");
        file.insert_closing(')');
        assert_eq!(
            (file.lines[1].as_str(), file.cursor_column),
            ("    ()", 6),
            "steps over"
        );
        // A quote after a letter is an apostrophe.
        file.insert_str(" it");
        file.insert_pair('\'');
        assert!(file.lines[1].ends_with("it'"), "{}", file.lines[1]);
        // Over a selection a bracket goes around it.
        file.anchor = Some((0, 3));
        file.move_cursor_to(0, 7);
        file.insert_pair('[');
        assert_eq!(file.lines[0], "fn [main]() {");
        // Tab and Shift+Tab on a selection of two lines.
        file.anchor = Some((0, 0));
        file.move_cursor_to(1, 2);
        file.indent_lines();
        assert!(file.lines[0].starts_with("    fn") && file.lines[1].starts_with("        ("));
        file.outdent_lines();
        assert!(file.lines[0].starts_with("fn"), "{}", file.lines[0]);
    }

    #[test]
    fn comments_lines_words_and_home_do_what_editors_do() {
        let (_directory, mut file) = file_with("a.rs", "let a = 1;\n  let b = 2;\n\nlast\n");
        file.anchor = Some((0, 0));
        file.move_cursor_to(1, 3);
        assert!(file.toggle_comment());
        // At the smallest indentation of the lines, so they stay lined up.
        assert_eq!(file.lines[..2], ["// let a = 1;", "//   let b = 2;"]);
        assert!(file.toggle_comment());
        assert_eq!(file.lines[..2], ["let a = 1;", "  let b = 2;"]);
        // Undo takes a whole toggle back in one step.
        file.toggle_comment();
        file.undo();
        assert_eq!(file.lines[0], "let a = 1;");
        // Move down, duplicate, delete.
        file.anchor = None;
        file.move_cursor_to(0, 0);
        file.move_lines(true);
        assert_eq!(file.lines[..2], ["  let b = 2;", "let a = 1;"]);
        file.duplicate_lines();
        assert_eq!(file.lines[1..3], ["let a = 1;", "let a = 1;"]);
        file.delete_lines();
        assert_eq!(file.lines.len(), 4);
        // Smart Home and words.
        file.move_cursor_to(0, 8);
        file.smart_home();
        assert_eq!(file.cursor_column, 2);
        file.smart_home();
        assert_eq!(file.cursor_column, 0);
        file.move_word(true);
        assert_eq!(file.cursor_column, 5);
        file.delete_word(true);
        assert_eq!(file.lines[0], "  let = 2;");
        // A language without a known comment says so.
        let (_other, mut plain) = file_with("a.unknownext", "x\n");
        assert!(!plain.toggle_comment());
        let (_html, mut html) = file_with("a.html", "<p>x</p>\n");
        html.toggle_comment();
        assert_eq!(html.lines[0], "<!-- <p>x</p> -->");
        html.toggle_comment();
        assert_eq!(html.lines[0], "<p>x</p>");
    }

    #[test]
    fn line_ends_and_encodings_are_saved_as_they_came_and_bad_text_is_never_saved() {
        let (directory, mut file) = file_with("a.txt", "a\r\nb\r\n");
        assert!(file.crlf);
        file.move_cursor_to(0, 1);
        file.insert('!');
        file.save().unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("a.txt")).unwrap(),
            b"a!\r\nb\r\n"
        );
        let path = directory.path().join("bad.txt");
        std::fs::write(&path, b"ok \xFF\n").unwrap();
        let mut bad = OpenFile::load(path.clone(), &mut Picker::halfblocks()).unwrap();
        assert!(bad.not_text && bad.read_only);
        bad.dirty = true;
        assert!(bad.save().is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"ok \xFF\n");
    }

    #[test]
    fn long_lines_wrap_into_rows_and_the_cursor_follows_them() {
        let (_directory, mut file) = file_with("a.txt", "abcdefghij\nxy\n");
        file.wrap = true;
        file.set_wrap_width(4);
        assert_eq!(file.display_rows(), 4, "3 rows for 10 columns, 1 for 2");
        let (number, spans) = file.display_line(1, 4);
        let text: String = spans.iter().map(|span| span.content.to_string()).collect();
        assert_eq!((number, text.as_str()), (None, "efgh"));
        assert_eq!(file.display_offset(2), 8);
        file.move_cursor_to(0, 9);
        assert_eq!(file.cursor_place(), (2, 1));
        file.toggle_wrap();
        assert_eq!(file.display_rows(), 2);
    }

    #[test]
    fn a_long_file_opens_at_once_and_a_key_stays_within_its_budget() {
        let text = (0..20_000)
            .map(|line| format!("let value_{line} = {line} * 2; // comment\n"))
            .collect::<String>();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("long.rs");
        std::fs::write(&path, text).unwrap();
        let debug = if cfg!(debug_assertions) { 10 } else { 1 };
        let started = Instant::now();
        let mut file = OpenFile::load(path, &mut Picker::halfblocks()).unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(100 * debug),
            "{:?} to open",
            started.elapsed()
        );
        assert!(file.colouring(), "the colours come from a thread");
        // A key while the colours are on their way, and once they arrive.
        let started = Instant::now();
        file.move_cursor_to(10_000, 0);
        file.insert('x');
        assert!(
            started.elapsed() < Duration::from_millis(16 * debug),
            "{:?} for a key",
            started.elapsed()
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        while file.colouring() {
            assert!(Instant::now() < deadline, "the colours never came");
            file.tick_colours();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            file.highlighted[10_000].len() > 1,
            "coloured once they arrive"
        );
        let started = Instant::now();
        file.insert('y');
        assert!(
            started.elapsed() < Duration::from_millis(16 * debug),
            "{:?} for a key",
            started.elapsed()
        );
    }
}
