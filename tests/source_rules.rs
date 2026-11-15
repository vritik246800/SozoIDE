//! Rules of the project that the source itself can be asked about (`CLAUDE.md` §3.6, §7.2):
//! every external program is in the inventory, no lock or channel can bring the app down,
//! and no file grows past its limit.

use std::path::{Path, PathBuf};

/// The programs each file starts, outside its tests: the table of `SECURITY.md` §10.3. A new
/// `Command::new` goes there first, with where its arguments come from, its time limit and
/// its input, and then here.
const PROGRAMS: [(&str, usize); 13] = [
    ("src/app/marks.rs", 3),
    ("src/app/mod.rs", 5),
    ("src/debug/mod.rs", 6),
    ("src/git/operations.rs", 1),
    ("src/lsp/client.rs", 1),
    ("src/lsp/requests.rs", 1),
    ("src/main.rs", 1),
    ("src/media/captions.rs", 1),
    ("src/media/video.rs", 3),
    ("src/terminal/mod.rs", 4),
    ("src/update/deps.rs", 5),
    ("src/update/install.rs", 12),
    ("src/update/mod.rs", 1),
];

/// Files that were over the limit when it was set: they only get smaller.
const OVER_THE_LIMIT: [(&str, usize); 1] = [("site/src/pages/index.astro", 1257)];

fn files(folder: &Path, extensions: &[&str], found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "node_modules" && name != "dist" && name != ".astro" {
                files(&path, extensions, found);
            }
        } else if path
            .extension()
            .is_some_and(|extension| extensions.iter().any(|known| extension == *known))
        {
            found.push(path);
        }
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn name(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

/// The code of a file without its tests.
fn code(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    if name(path).contains("/tests/") {
        return String::new();
    }
    match text.find("\nmod tests {") {
        Some(tests) => text[..tests].to_string(),
        None => text,
    }
}

#[test]
fn every_program_the_app_starts_is_in_the_inventory() {
    let mut sources = Vec::new();
    files(&root().join("src"), &["rs"], &mut sources);
    let mut found: Vec<(String, usize)> = sources
        .iter()
        .map(|path| {
            let code = code(path);
            let count =
                code.matches("Command::new(").count() + code.matches("CommandBuilder::new").count();
            (name(path), count)
        })
        .filter(|(_, count)| *count > 0)
        .collect();
    found.sort();
    let known: Vec<(String, usize)> = PROGRAMS
        .iter()
        .map(|(file, count)| (file.to_string(), *count))
        .collect();
    assert_eq!(
        found, known,
        "a program is started that SECURITY.md §10.3 does not list"
    );
}

#[test]
fn no_lock_or_channel_is_unwrapped_outside_the_tests() {
    let mut sources = Vec::new();
    files(&root().join("src"), &["rs"], &mut sources);
    for path in sources {
        let code: String = code(&path).split_whitespace().collect();
        for call in ["lock()", "recv()", "read()", "write()"] {
            for end in ["unwrap()", "expect("] {
                assert!(
                    !code.contains(&format!(".{call}.{end}")),
                    "{}: .{call}.{end} would stop the app on a poisoned lock or a closed channel",
                    name(&path)
                );
            }
        }
    }
}

#[test]
fn no_file_grows_past_its_limit() {
    let mut sources = Vec::new();
    files(&root().join("src"), &["rs"], &mut sources);
    files(&root().join("tests"), &["rs"], &mut sources);
    files(&root().join("site/src"), &["astro", "ts"], &mut sources);
    for path in sources {
        let name = name(&path);
        let lines = std::fs::read_to_string(&path).unwrap().lines().count();
        let limit = OVER_THE_LIMIT
            .iter()
            .find(|(file, _)| *file == name)
            .map_or_else(
                || if name.ends_with(".rs") { 2000 } else { 800 },
                |(_, lines)| *lines,
            );
        assert!(
            lines <= limit,
            "{name} has {lines} lines: the limit is {limit}"
        );
    }
}
