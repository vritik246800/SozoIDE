//! What a language server changes or lists beyond a place (F31, TODO 3.1.1–3.1.6): edits of
//! rename, format and code actions read and checked before anything is written, symbols of a
//! file and of the project, the signature at the cursor, and the change of a document sent as
//! only what changed.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{char_column, uri, utf16_column};

/// At most this many edits in one answer, and this much text in each: a server cannot make the
/// app write gigabytes.
const MOST_EDITS: usize = 20_000;
const MOST_TEXT: usize = 1 << 20;

/// One change of a file: rows and UTF-16 columns, as the protocol counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub row: usize,
    pub start: usize,
    pub end_row: usize,
    pub end: usize,
    pub text: String,
}

/// The changes of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEdit {
    pub path: PathBuf,
    pub edits: Vec<TextEdit>,
}

fn text_edit(value: &Value) -> Option<TextEdit> {
    let range = &value["range"];
    let number = |part: &str, key: &str| range[part][key].as_u64().map(|n| n as usize);
    let text = value["newText"].as_str()?;
    (text.len() <= MOST_TEXT).then_some(())?;
    Some(TextEdit {
        row: number("start", "line")?,
        start: number("start", "character")?,
        end_row: number("end", "line")?,
        end: number("end", "character")?,
        text: text.to_owned(),
    })
}

/// The edits of a list of `TextEdit` (the answer of formatting).
pub fn text_edits(list: &Value) -> Result<Vec<TextEdit>, String> {
    let Some(list) = list.as_array() else {
        return Ok(Vec::new());
    };
    if list.len() > MOST_EDITS {
        return Err("too many edits".into());
    }
    list.iter()
        .map(|edit| {
            text_edit(edit).ok_or_else(|| "an edit the server sent cannot be read".to_owned())
        })
        .collect()
}

/// The files a `WorkspaceEdit` changes, each only inside `root` (resolved: no `..`, no links out
/// of it) and only with `file:` addresses; creating, renaming or deleting files is refused.
pub fn workspace_edit(edit: &Value, root: &Path) -> Result<Vec<FileEdit>, String> {
    let inside = root.canonicalize().map_err(|error| error.to_string())?;
    let mut files: Vec<FileEdit> = Vec::new();
    let mut count = 0;
    let mut add = |address: &str, list: &Value| -> Result<(), String> {
        let path = uri::to_path(address)
            .filter(|_| address.starts_with("file:"))
            .ok_or_else(|| format!("not a file of the project: {address}"))?;
        let real = path
            .canonicalize()
            .map_err(|_| format!("{} does not exist", path.display()))?;
        if !real.starts_with(&inside) {
            return Err(format!("{} is outside the project", path.display()));
        }
        let edits = text_edits(list)?;
        count += edits.len();
        if count > MOST_EDITS {
            return Err("too many edits".into());
        }
        match files.iter_mut().find(|file| file.path == real) {
            Some(file) => file.edits.extend(edits),
            None => files.push(FileEdit { path: real, edits }),
        }
        Ok(())
    };
    if let Some(changes) = edit["changes"].as_object() {
        for (address, list) in changes {
            add(address, list)?;
        }
    }
    if let Some(changes) = edit["documentChanges"].as_array() {
        for change in changes {
            if change.get("kind").is_some() {
                return Err("the server asks to create, rename or delete files: not done".into());
            }
            let address = change["textDocument"]["uri"].as_str().unwrap_or_default();
            add(address, &change["edits"])?;
        }
    }
    Ok(files)
}

/// `lines` with `edits` applied, all of them or none: a range past the end of the text, or two
/// that overlap, refuses the whole answer.
pub fn apply_edits(lines: &[String], edits: &[TextEdit]) -> Result<Vec<String>, String> {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|edit| (edit.row, edit.start, edit.end_row, edit.end));
    // Each edit as a range of characters of the whole text.
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    let mut total = 0;
    for line in lines {
        offsets.push(total);
        total += line.chars().count() + 1;
    }
    let at = |row: usize, units: usize| -> Result<usize, String> {
        let line = lines.get(row).ok_or("an edit past the end of the file")?;
        if units > line.encode_utf16().count() {
            return Err("an edit past the end of a line".into());
        }
        Ok(offsets[row] + char_column(line, units))
    };
    let mut ranges = Vec::with_capacity(sorted.len());
    let mut last_end = 0;
    for edit in sorted {
        // The end of the file may be the row just after the last line.
        let start = at(edit.row, edit.start)?;
        let end = if edit.end_row == lines.len() && edit.end == 0 {
            total.saturating_sub(1)
        } else {
            at(edit.end_row, edit.end)?
        };
        if end < start || start < last_end {
            return Err("edits that overlap".into());
        }
        last_end = end;
        ranges.push((start, end, edit.text.as_str()));
    }
    let mut text: Vec<char> = lines.join("\n").chars().collect();
    for (start, end, replacement) in ranges.into_iter().rev() {
        let end = end.min(text.len());
        text.splice(start..end, replacement.replace("\r\n", "\n").chars());
    }
    let text: String = text.into_iter().collect();
    Ok(text.split('\n').map(str::to_owned).collect())
}

/// A symbol of a file or of the project (`documentSymbol`, `workspace/symbol`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub path: PathBuf,
    pub row: usize,
    pub start: usize,
}

fn kind(number: u64) -> &'static str {
    match number {
        2 => "module",
        5 => "class",
        6 => "method",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        22 => "enum member",
        23 => "struct",
        26 => "type parameter",
        _ => "symbol",
    }
}

/// The symbols of an answer: nested (`DocumentSymbol`, children with the parent's name before
/// them) or flat (`SymbolInformation`). At most 5000.
pub fn symbols(result: &Value, path: Option<&Path>) -> Vec<Symbol> {
    let mut found = Vec::new();
    fn walk(list: &Value, path: Option<&Path>, parent: &str, found: &mut Vec<Symbol>) {
        for item in list.as_array().into_iter().flatten() {
            if found.len() >= 5_000 {
                return;
            }
            let name = item["name"].as_str().unwrap_or_default();
            let full = if parent.is_empty() {
                name.to_owned()
            } else {
                format!("{parent}::{name}")
            };
            let (place, file) = match item.get("location") {
                Some(location) => (
                    &location["range"]["start"],
                    location["uri"].as_str().and_then(uri::to_path),
                ),
                None => (
                    &item["selectionRange"]["start"],
                    path.map(Path::to_path_buf),
                ),
            };
            if let Some(file) = file {
                found.push(Symbol {
                    name: full.chars().filter(|c| !c.is_control()).collect(),
                    kind: kind(item["kind"].as_u64().unwrap_or(0)),
                    path: file,
                    row: place["line"].as_u64().unwrap_or(0) as usize,
                    start: place["character"].as_u64().unwrap_or(0) as usize,
                });
            }
            walk(&item["children"], path, &full, found);
        }
    }
    walk(result, path, "", &mut found);
    found
}

/// The signature at the cursor and the columns (characters) of its active parameter.
pub fn signature(result: &Value) -> Option<(String, Option<(usize, usize)>)> {
    let signatures = result["signatures"].as_array()?;
    let active = result["activeSignature"].as_u64().unwrap_or(0) as usize;
    let signature = signatures.get(active).or_else(|| signatures.first())?;
    let label: String = signature["label"]
        .as_str()?
        .chars()
        .filter(|c| !c.is_control())
        .take(300)
        .collect();
    let parameter = signature["activeParameter"]
        .as_u64()
        .or_else(|| result["activeParameter"].as_u64())
        .unwrap_or(0) as usize;
    let range =
        signature["parameters"]
            .get(parameter)
            .and_then(|parameter| match &parameter["label"] {
                Value::String(text) => {
                    let start = label.find(text.as_str())?;
                    Some((
                        label[..start].chars().count(),
                        label[..start].chars().count() + text.chars().count(),
                    ))
                }
                Value::Array(pair) => {
                    let units = |value: &Value| value.as_u64().map(|n| n as usize);
                    Some((
                        char_column(&label, units(&pair[0])?),
                        char_column(&label, units(&pair[1])?),
                    ))
                }
                _ => None,
            });
    Some((label, range))
}

/// A code action or quick fix: its title and what it does (an edit, a command, or both).
#[derive(Clone, Debug, PartialEq)]
pub struct CodeAction {
    pub title: String,
    pub edit: Option<Value>,
    pub command: Option<Value>,
}

pub fn code_actions(result: &Value) -> Vec<CodeAction> {
    result
        .as_array()
        .into_iter()
        .flatten()
        .take(50)
        .filter_map(|item| {
            let title: String = item["title"]
                .as_str()?
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            // A bare `Command` has its command at the top level.
            let command = match &item["command"] {
                Value::String(_) => Some(item.clone()),
                Value::Object(_) => Some(item["command"].clone()),
                _ => None,
            };
            Some(CodeAction {
                title,
                edit: item.get("edit").cloned(),
                command,
            })
        })
        .collect()
}

/// The change from `old` to `new` as one range of `old` and the text that takes its place
/// (`didChange` with only what changed, TODO 3.1.6): rows and UTF-16 columns.
pub fn incremental_change(old: &str, new: &str) -> ((usize, usize), (usize, usize), String) {
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((at, a), _)| at + a.len_utf8());
    let old_rest = &old[prefix..];
    let new_rest = &new[prefix..];
    let suffix = old_rest
        .chars()
        .rev()
        .zip(new_rest.chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>()
        .min(old_rest.len())
        .min(new_rest.len());
    let place = |text: &str, byte: usize| {
        let before = &text[..byte];
        let row = before.matches('\n').count();
        let line_start = before.rfind('\n').map_or(0, |at| at + 1);
        let line = &text[line_start..];
        (
            row,
            utf16_column(line, before[line_start..].chars().count()),
        )
    };
    let start = place(old, prefix);
    let end = place(old, old.len() - suffix);
    (start, end, new[prefix..new.len() - suffix].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edit(row: usize, start: usize, end_row: usize, end: usize, text: &str) -> TextEdit {
        TextEdit {
            row,
            start,
            end_row,
            end,
            text: text.into(),
        }
    }

    #[test]
    fn edits_apply_all_or_none_with_utf16_columns() {
        let lines = vec!["let café = 1;".to_owned(), "café + 2".to_owned()];
        // `café` is 4 UTF-16 units at column 4.
        let renamed =
            apply_edits(&lines, &[edit(0, 4, 0, 8, "tea"), edit(1, 0, 1, 4, "tea")]).unwrap();
        assert_eq!(renamed, ["let tea = 1;", "tea + 2"]);
        assert!(
            apply_edits(&lines, &[edit(5, 0, 5, 1, "x")]).is_err(),
            "past the end"
        );
        assert!(
            apply_edits(&lines, &[edit(0, 0, 0, 99, "x")]).is_err(),
            "past a line"
        );
        assert!(
            apply_edits(&lines, &[edit(0, 0, 0, 5, "a"), edit(0, 3, 0, 6, "b")]).is_err(),
            "overlap"
        );
        assert!(
            apply_edits(&lines, &[edit(0, 5, 0, 2, "a")]).is_err(),
            "end before start"
        );
        // A formatter that replaces everything, to the row after the last.
        let whole = apply_edits(&lines, &[edit(0, 0, 2, 0, "x\r\ny")]).unwrap();
        assert_eq!(whole, ["x", "y"]);
    }

    #[test]
    fn a_workspace_edit_never_reaches_outside_the_project() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("a.rs"), "a").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("x.rs"), "x").unwrap();
        let address = |path: &Path| uri::from_path(path);
        let one = json!({"newText": "b", "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}});
        let ok = workspace_edit(
            &json!({"changes": {address(&root.join("a.rs")): [one.clone()]}}),
            root,
        )
        .unwrap();
        assert_eq!(ok.len(), 1);
        for hostile in [
            json!({"changes": {address(&outside.path().join("x.rs")): [one.clone()]}}),
            json!({"changes": {format!("{}/../{}/x.rs", address(root), outside.path().file_name().unwrap().to_string_lossy()): [one.clone()]}}),
            json!({"changes": {"http://example.com/a.rs": [one.clone()]}}),
            json!({"documentChanges": [{"kind": "delete", "uri": address(&root.join("a.rs"))}]}),
            json!({"changes": {address(&root.join("missing.rs")): [one.clone()]}}),
            json!({"changes": {address(&root.join("a.rs")): [{"newText": 7}]}}),
        ] {
            assert!(workspace_edit(&hostile, root).is_err(), "{hostile}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("x.rs"), root.join("link.rs")).unwrap();
            let through = json!({"changes": {address(&root.join("link.rs")): [one]}});
            assert!(
                workspace_edit(&through, root).is_err(),
                "a link out of the project"
            );
        }
    }

    #[test]
    fn symbols_signatures_and_actions_read_what_servers_send() {
        let nested = json!([{"name": "App", "kind": 23, "selectionRange": {"start": {"line": 3, "character": 4}},
            "children": [{"name": "new", "kind": 6, "selectionRange": {"start": {"line": 5, "character": 8}}}]}]);
        let found = symbols(&nested, Some(Path::new("/p/a.rs")));
        assert_eq!(
            found.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["App", "App::new"]
        );
        assert_eq!((found[1].kind, found[1].row), ("method", 5));
        let flat = json!([{"name": "main\u{1b}", "kind": 12, "location": {"uri": "file:///p/b.rs", "range": {"start": {"line": 1, "character": 0}}}}]);
        let found = symbols(&flat, None);
        assert_eq!(
            (found[0].name.as_str(), found[0].path.clone()),
            ("main", PathBuf::from("/p/b.rs"))
        );
        assert!(symbols(&json!("nonsense"), None).is_empty());

        let help = json!({"signatures": [{"label": "fn add(a: i32, b: i32) -> i32", "parameters": [{"label": "a: i32"}, {"label": [15, 21]}]}], "activeParameter": 1});
        assert_eq!(
            signature(&help),
            Some(("fn add(a: i32, b: i32) -> i32".into(), Some((15, 21))))
        );
        assert_eq!(signature(&json!({})), None);

        let actions = code_actions(
            &json!([{"title": "Import x", "edit": {"changes": {}}}, {"title": "Run", "command": "x.run"}, {"no": "title"}]),
        );
        assert_eq!(actions.len(), 2);
        assert!(actions[0].edit.is_some() && actions[1].command.is_some());
    }

    #[test]
    fn only_what_changed_is_sent() {
        assert_eq!(
            incremental_change("abc\ndef", "abc\ndXef"),
            ((1, 1), (1, 1), "X".into())
        );
        assert_eq!(
            incremental_change("abc\ndef", "abc"),
            ((0, 3), (1, 3), String::new())
        );
        assert_eq!(
            incremental_change("café", "cafés"),
            ((0, 4), (0, 4), "s".into())
        );
        assert_eq!(
            incremental_change("same", "same"),
            ((0, 4), (0, 4), String::new())
        );
        assert_eq!(
            incremental_change("", "new"),
            ((0, 0), (0, 0), "new".into())
        );
        // Repeated characters: the change is placed once, never twice.
        assert_eq!(
            incremental_change("aaaa", "aaa"),
            ((0, 3), (0, 4), String::new())
        );
    }
}
