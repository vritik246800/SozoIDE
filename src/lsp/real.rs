//! The language servers of the machine, really started (TODO 3.4.1): only with
//! `cargo test --features servers`, which the CI job `servers` runs on Ubuntu with every server
//! installed. Each test is skipped, and says so, when its server is not on this machine.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::{Event, Goal, Lsp, server_for};

/// Waits up to a minute for an event that `wanted` takes, asking again every few seconds
/// while the server still loads the project.
fn wait_for(
    lsp: &mut Lsp,
    mut ask: impl FnMut(&mut Lsp),
    wanted: impl Fn(&Event) -> bool,
) -> Option<Event> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut asked = Instant::now() - Duration::from_secs(10);
    while Instant::now() < deadline {
        if asked.elapsed() > Duration::from_secs(5) {
            ask(lsp);
            asked = Instant::now();
        }
        for event in lsp.poll() {
            if wanted(&event) {
                return Some(event);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// A project with `files`, the server of `main` started on it; none when the server is not
/// installed here.
fn project(
    files: &[(&str, &str)],
    main: &str,
) -> Option<(tempfile::TempDir, Lsp, PathBuf, String)> {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let path = root.join(main);
    let (name, arguments, _) = server_for(&path)?;
    if crate::update::deps::server_command(name, arguments).is_none() {
        eprintln!("skipped: {name} is not installed here");
        return None;
    }
    let mut lsp = Lsp::new(root.clone(), true);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(lsp.open(&path, &text, 1), None);
    Some((directory, lsp, path, text))
}

/// The definition of the symbol at `row`, `column` of `main`, from the real server.
fn definition(
    files: &[(&str, &str)],
    main: &str,
    row: usize,
    column: usize,
) -> Option<(PathBuf, usize)> {
    let (_directory, mut lsp, path, text) = project(files, main)?;
    let line = text.lines().nth(row).unwrap().to_owned();
    let event = wait_for(
        &mut lsp,
        |lsp| {
            let _ = lsp.places(Goal::Definition, &path, 1, row, column, &line);
        },
        |event| matches!(event, Event::Places { places, .. } if !places.is_empty()),
    )
    .expect("the server never answered with a place");
    let Event::Places { places, .. } = event else {
        unreachable!()
    };
    Some((places[0].path.clone(), places[0].row))
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn rust_analyzer_finds_a_definition_and_renames_in_two_files() {
    let files = [
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "src/main.rs",
            "mod util;\nfn main() {\n    util::helper();\n}\n",
        ),
        ("src/util.rs", "pub fn helper() {}\n"),
    ];
    let Some((path, row)) = definition(&files, "src/main.rs", 2, 12) else {
        return;
    };
    assert_eq!((name(&path), row), ("util.rs".to_owned(), 0));
    let (_directory, mut lsp, main, text) = project(&files, "src/main.rs").unwrap();
    let line = text.lines().nth(2).unwrap().to_owned();
    let event = wait_for(
        &mut lsp,
        |lsp| {
            let _ = lsp.rename(&main, 1, 2, 12, &line, "assist");
        },
        |event| matches!(event, Event::Edits { result: Ok(files), .. } if files.len() == 2),
    )
    .expect("no rename in two files");
    let Event::Edits {
        result: Ok(edits), ..
    } = event
    else {
        unreachable!()
    };
    assert!(
        edits
            .iter()
            .all(|file| file.path.starts_with(main.parent().unwrap()))
    );
}

#[test]
fn clangd_finds_a_definition_in_a_header() {
    let files = [
        ("util.h", "int add(int a, int b);\n"),
        (
            "main.c",
            "#include \"util.h\"\nint main(void) { return add(1, 2); }\n",
        ),
    ];
    let Some((path, row)) = definition(&files, "main.c", 1, 25) else {
        return;
    };
    assert_eq!((name(&path), row), ("util.h".to_owned(), 0));
}

#[test]
fn pyright_finds_a_definition_in_another_module() {
    let files = [
        ("util.py", "def helper():\n    return 1\n"),
        ("main.py", "from util import helper\nhelper()\n"),
    ];
    let Some((path, row)) = definition(&files, "main.py", 1, 2) else {
        return;
    };
    assert_eq!((name(&path), row), ("util.py".to_owned(), 0));
}

#[test]
fn gopls_finds_a_definition_in_another_file() {
    let files = [
        ("go.mod", "module demo\n\ngo 1.21\n"),
        (
            "util.go",
            "package main\n\nfunc helper() int { return 1 }\n",
        ),
        ("main.go", "package main\n\nfunc main() { helper() }\n"),
    ];
    let Some((path, row)) = definition(&files, "main.go", 2, 15) else {
        return;
    };
    assert_eq!((name(&path), row), ("util.go".to_owned(), 2));
}

#[test]
fn typescript_language_server_finds_a_definition_in_another_module() {
    let files = [
        (
            "util.ts",
            "export function helper(): number { return 1; }\n",
        ),
        ("main.ts", "import { helper } from \"./util\";\nhelper();\n"),
    ];
    let Some((path, row)) = definition(&files, "main.ts", 1, 2) else {
        return;
    };
    assert_eq!((name(&path), row), ("util.ts".to_owned(), 0));
}
