//! What a build said, read back (TODO 5.2.5): the errors and warnings of compilers in the
//! terminal's lines, the test under the cursor as the command that runs it alone, and the named
//! tasks of `.sozo.toml`.

use std::path::{Path, PathBuf};

/// A problem a compiler printed: its file (inside the project), row and column from 0,
/// whether it is an error and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printed {
    pub path: PathBuf,
    pub row: usize,
    pub column: usize,
    pub error: bool,
    pub message: String,
}

/// The problems in `lines`: `file:line:col: error: …` (GCC, Clang, Go, TypeScript with
/// `--pretty false`, ESLint compact), and Rust's `error[E…]: …` with its `--> file:line:col`.
/// Only files inside `root`, at most 2000.
pub fn problems_in(lines: &[String], root: &Path) -> Vec<Printed> {
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(_) => return Vec::new(),
    };
    let inside = |path: &str| -> Option<PathBuf> {
        let path = Path::new(path.trim());
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        let full = full.canonicalize().ok()?;
        (full.starts_with(&root) && full.is_file()).then_some(full)
    };
    let mut found = Vec::new();
    let mut pending: Option<(bool, String)> = None;
    for line in lines {
        let text = line.trim();
        // Rust: the message first, its place on a line of its own.
        if let Some(rest) = text
            .strip_prefix("error")
            .or_else(|| text.strip_prefix("warning"))
            && let Some((_, message)) = rest
                .split_once(": ")
                .filter(|(code, _)| code.is_empty() || code.starts_with('['))
        {
            pending = Some((text.starts_with("error"), message.to_owned()));
            continue;
        }
        if let (Some(place), Some((error, message))) = (text.strip_prefix("--> "), pending.as_ref())
        {
            if let Some((path, row, column)) = place_of(place)
                && let Some(path) = inside(path)
            {
                found.push(Printed {
                    path,
                    row,
                    column,
                    error: *error,
                    message: message.clone(),
                });
            }
            pending = None;
            continue;
        }
        // `file:line:col: error: message` and `file:line: warning: message`.
        for kind in [": error: ", ": warning: ", ": fatal error: "] {
            if let Some((place, message)) = text.split_once(kind)
                && let Some((path, row, column)) = place_of(place)
                && let Some(path) = inside(path)
            {
                found.push(Printed {
                    path,
                    row,
                    column,
                    error: !kind.contains("warning"),
                    message: message.trim().to_owned(),
                });
                break;
            }
        }
        if found.len() >= 2000 {
            break;
        }
    }
    found.dedup();
    found
}

/// `path:line[:column]` with both from 1, as rows and columns from 0.
fn place_of(text: &str) -> Option<(&str, usize, usize)> {
    let mut parts = text.rsplitn(3, ':');
    let last = parts.next()?;
    let middle = parts.next()?;
    match parts.next() {
        Some(path) if !path.is_empty() => Some((
            path,
            middle.trim().parse::<usize>().ok()?.saturating_sub(1),
            last.trim().parse::<usize>().ok()?.saturating_sub(1),
        )),
        _ => Some((
            middle,
            last.trim().parse::<usize>().ok()?.saturating_sub(1),
            0,
        )),
    }
}

/// The command that runs the test the cursor is in, by the file's language, and its name;
/// none outside a test. The words go through `shell_line` like every command.
pub fn test_at(
    path: &Path,
    lines: &[String],
    row: usize,
    root: &Path,
) -> Option<(Vec<String>, String)> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    let above = || {
        lines[..=row.min(lines.len().saturating_sub(1))]
            .iter()
            .rev()
    };
    let word_after = |line: &str, start: &str| -> Option<String> {
        let rest = line.trim_start().strip_prefix(start)?;
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    };
    match extension.as_str() {
        "rs" => {
            let name = above().find_map(|line| {
                let line = line
                    .trim_start()
                    .trim_start_matches("pub ")
                    .trim_start_matches("async ");
                word_after(line, "fn ")
            })?;
            // The name alone matches it (`--exact` would need its module path).
            Some((
                vec![
                    "cargo".into(),
                    "test".into(),
                    name.clone(),
                    "--".into(),
                    "--nocapture".into(),
                ],
                name,
            ))
        }
        "py" => {
            let name = above()
                .find_map(|line| {
                    word_after(line, "def ").or_else(|| word_after(line, "async def "))
                })
                .filter(|name| name.starts_with("test"))?;
            Some((
                vec![
                    "python3".into(),
                    "-m".into(),
                    "pytest".into(),
                    format!("{relative}::{name}"),
                ],
                name,
            ))
        }
        "go" => {
            let name = above()
                .find_map(|line| word_after(line, "func "))
                .filter(|name| name.starts_with("Test"))?;
            let folder = Path::new(&relative).parent().map_or_else(
                || ".".to_owned(),
                |folder| format!("./{}", folder.display()),
            );
            Some((
                vec![
                    "go".into(),
                    "test".into(),
                    "-run".into(),
                    format!("^{name}$"),
                    folder,
                ],
                name,
            ))
        }
        "js" | "ts" | "jsx" | "tsx" | "mjs" | "cjs" => {
            let name = above().find_map(|line| {
                let line = line.trim_start();
                let rest = ["it(", "test(", "describe("]
                    .iter()
                    .find_map(|start| line.strip_prefix(start))?;
                let quote = rest
                    .chars()
                    .next()
                    .filter(|c| matches!(c, '\'' | '"' | '`'))?;
                let name: String = rest[1..].chars().take_while(|c| *c != quote).collect();
                (!name.is_empty() && !name.chars().any(char::is_control)).then_some(name)
            })?;
            Some((
                vec![
                    "npm".into(),
                    "test".into(),
                    "--".into(),
                    "-t".into(),
                    name.clone(),
                ],
                name,
            ))
        }
        _ => None,
    }
}

/// The named tasks of `.sozo.toml` in `folder` (besides build, clean and run): their name and
/// command (program and arguments).
pub fn named_tasks(folder: &Path) -> Vec<(String, Vec<String>)> {
    let Ok(text) = std::fs::read_to_string(folder.join(super::TASKS_FILE)) else {
        return Vec::new();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(tasks) = table.get("tasks").and_then(|tasks| tasks.as_table()) else {
        return Vec::new();
    };
    tasks
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "build" | "clean" | "run"))
        .filter_map(|(name, task)| {
            let mut command = vec![task.get("program")?.as_str()?.to_owned()];
            command.extend(
                task.get("args")
                    .and_then(|args| args.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|arg| arg.as_str().map(str::to_owned)),
            );
            let clean = |word: &String| !word.chars().any(|c| c == '\0' || c == '\n');
            (command.iter().all(clean) && !name.chars().any(char::is_control))
                .then(|| (name.clone(), command))
        })
        .take(50)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_output_becomes_problems_of_the_project_and_nothing_outside_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("main.c"), "int main;\n").unwrap();
        let lines: Vec<String> = [
            "error[E0308]: mismatched types",
            "  --> src/main.rs:3:9",
            "warning: unused variable: `x`",
            " --> src/main.rs:5:1",
            "main.c:2:5: error: expected ';'",
            "main.c:7: warning: implicit int",
            "/etc/passwd:1:1: error: not ours",
            "../outside.c:1:1: error: not ours",
            "plain text: nothing",
        ]
        .map(String::from)
        .to_vec();
        let found = problems_in(&lines, root);
        let summary: Vec<(String, usize, usize, bool)> = found
            .iter()
            .map(|problem| {
                (
                    problem
                        .path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    problem.row,
                    problem.column,
                    problem.error,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("main.rs".into(), 2, 8, true),
                ("main.rs".into(), 4, 0, false),
                ("main.c".into(), 1, 4, true),
                ("main.c".into(), 6, 0, false)
            ]
        );
        assert_eq!(found[0].message, "mismatched types");
        assert!(problems_in(&[], root).is_empty());
    }

    #[test]
    fn the_test_under_the_cursor_and_named_tasks() {
        let root = Path::new("/project");
        let lines = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
        let rust = lines("#[test]\nfn adds_two() {\n    assert!(true);\n}\n");
        assert_eq!(
            test_at(&root.join("src/lib.rs"), &rust, 2, root).unwrap().0,
            ["cargo", "test", "adds_two", "--", "--nocapture"]
        );
        let python = lines("class A:\n    def test_it(self):\n        pass\n");
        assert_eq!(
            test_at(&root.join("tests/a.py"), &python, 2, root)
                .unwrap()
                .0,
            ["python3", "-m", "pytest", "tests/a.py::test_it"]
        );
        assert!(
            test_at(
                &root.join("a.py"),
                &lines("def helper():\n  pass\n"),
                1,
                root
            )
            .is_none()
        );
        let go = lines("func TestSum(t *testing.T) {\n}\n");
        assert_eq!(
            test_at(&root.join("pkg/sum_test.go"), &go, 1, root)
                .unwrap()
                .0,
            ["go", "test", "-run", "^TestSum$", "./pkg"]
        );
        let js = lines("describe('math', () => {\n  it(\"adds; rm -rf /\", () => {\n  });\n});\n");
        let (words, name) = test_at(&root.join("a.test.js"), &js, 2, root).unwrap();
        assert_eq!(name, "adds; rm -rf /");
        assert_eq!(
            words.last().unwrap(),
            "adds; rm -rf /",
            "one word: shell_line quotes it"
        );
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(".sozo.toml"),
            "[tasks.build]\nprogram = \"make\"\n[tasks.lint]\nprogram = \"cargo\"\nargs = [\"clippy\"]\n[tasks.bad]\nprogram = \"x\\ny\"\n",
        )
        .unwrap();
        assert_eq!(
            named_tasks(directory.path()),
            [(
                "lint".to_owned(),
                vec!["cargo".to_owned(), "clippy".to_owned()]
            )]
        );
        std::fs::write(directory.path().join(".sozo.toml"), "[tasks").unwrap();
        assert!(named_tasks(directory.path()).is_empty());
    }
}
