//! Ctrl+R (F25): the command that compiles and runs a file, or its project, typed into the
//! bottom terminal. The terminal's shell runs it, so input, colours and Ctrl+C work as they do
//! there, and SozoIDE keeps nothing running of its own.

use std::path::Path;

/// The command line that runs `file`, for the shell of `os` (`sh`-like, or `cmd.exe` on
/// Windows), with the tools this machine `has`; compiled programs go to `scratch`. The error
/// says what is missing.
pub fn command(
    file: &Path,
    root: &Path,
    os: &str,
    has: impl Fn(&str) -> bool,
    scratch: &Path,
) -> Result<String, String> {
    let windows = os == "windows";
    let quote = |path: &Path| quote(&path.display().to_string(), windows);
    let and = " && ";
    let extension = file
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let program = scratch.join(if windows { format!("{stem}.exe") } else { stem });
    let first = |tools: &[&'static str]| tools.iter().copied().find(|tool| has(tool));
    let need = |tools: &[&'static str]| {
        first(tools).ok_or_else(|| format!("Install {} to run .{extension} files", tools[0]))
    };
    // A file inside a Cargo project or a Go module runs its project.
    let project = |marker: &str| {
        file.ancestors()
            .skip(1)
            .take_while(|folder| folder.starts_with(root))
            .find(|folder| folder.join(marker).is_file())
    };
    let python = if windows {
        ["python", "py"]
    } else {
        ["python3", "python"]
    };
    let line = match extension.as_str() {
        "rs" => match project("Cargo.toml") {
            Some(folder) => format!(
                "{} run --manifest-path {}",
                need(&["cargo"])?,
                quote(&folder.join("Cargo.toml"))
            ),
            None => format!(
                "{} {} -o {}{and}{}",
                need(&["rustc"])?,
                quote(file),
                quote(&program),
                quote(&program)
            ),
        },
        "c" => format!(
            "{} {} -o {}{and}{}",
            need(&["gcc", "clang", "cc"])?,
            quote(file),
            quote(&program),
            quote(&program)
        ),
        "cpp" | "cc" | "cxx" | "c++" => format!(
            "{} {} -o {}{and}{}",
            need(&["g++", "clang++", "c++"])?,
            quote(file),
            quote(&program),
            quote(&program)
        ),
        "go" => match project("go.mod") {
            Some(folder) => format!("cd {}{and}{} run .", quote(folder), need(&["go"])?),
            None => format!("{} run {}", need(&["go"])?, quote(file)),
        },
        "java" => format!("{} {}", need(&["java"])?, quote(file)),
        "kt" => {
            let jar = scratch.join("program.jar");
            format!(
                "{} {} -include-runtime -d {}{and}{} -jar {}",
                need(&["kotlinc"])?,
                quote(file),
                quote(&jar),
                need(&["java"])?,
                quote(&jar)
            )
        }
        "py" => format!("{} {}", need(&python)?, quote(file)),
        "js" | "mjs" | "cjs" => format!("{} {}", need(&["node"])?, quote(file)),
        "ts" | "mts" | "cts" => match first(&["deno", "bun"]) {
            Some(runtime) => format!("{runtime} run {}", quote(file)),
            None => format!("{} --yes tsx {}", need(&["npx"])?, quote(file)),
        },
        "rb" => format!("{} {}", need(&["ruby"])?, quote(file)),
        "php" => format!("{} {}", need(&["php"])?, quote(file)),
        "lua" => format!("{} {}", need(&["lua"])?, quote(file)),
        "pl" => format!("{} {}", need(&["perl"])?, quote(file)),
        "r" => format!("{} {}", need(&["Rscript"])?, quote(file)),
        "swift" => format!("{} {}", need(&["swift"])?, quote(file)),
        "dart" => format!("{} run {}", need(&["dart"])?, quote(file)),
        "zig" => format!("{} run {}", need(&["zig"])?, quote(file)),
        "hs" => format!("{} {}", need(&["runghc"])?, quote(file)),
        "ex" | "exs" => format!("{} {}", need(&["elixir"])?, quote(file)),
        "jl" => format!("{} {}", need(&["julia"])?, quote(file)),
        "sh" | "bash" => format!("{} {}", need(&["bash", "sh"])?, quote(file)),
        "zsh" => format!("{} {}", need(&["zsh"])?, quote(file)),
        "ps1" => format!(
            "{} -NoProfile -File {}",
            need(&["pwsh", "powershell"])?,
            quote(file)
        ),
        "bat" | "cmd" if windows => quote(file),
        _ => {
            return Err(format!(
                "SozoIDE does not know how to run .{extension} files"
            ));
        }
    };
    // Compiled programs need their folder; `cmd.exe` and the POSIX shells both create it.
    if line.contains(&scratch.display().to_string()) {
        let make = if windows {
            format!("if not exist {0} mkdir {0}", quote(scratch))
        } else {
            format!("mkdir -p {}", quote(scratch))
        };
        return Ok(format!("{make}{and}{line}"));
    }
    Ok(line)
}

/// A word the shell takes as it is: single quotes in POSIX shells, double quotes in `cmd.exe`.
fn quote(word: &str, windows: bool) -> String {
    if windows {
        format!("\"{word}\"")
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn each_language_runs_with_its_tool_and_projects_run_whole() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("app/src")).unwrap();
        std::fs::write(root.join("app/Cargo.toml"), "").unwrap();
        let scratch = PathBuf::from("/tmp/sozo run");
        let all = |_: &str| true;
        let run = |file: &str, has: &dyn Fn(&str) -> bool| {
            command(&root.join(file), root, "linux", has, &scratch)
        };
        let cargo = format!(
            "cargo run --manifest-path '{}'",
            root.join("app/Cargo.toml").display()
        );
        assert_eq!(run("app/src/main.rs", &all).unwrap(), cargo);
        let single = run("tool.rs", &all).unwrap();
        assert!(
            single.starts_with("mkdir -p '/tmp/sozo run' && rustc '"),
            "{single}"
        );
        assert!(single.ends_with("-o '/tmp/sozo run/tool' && '/tmp/sozo run/tool'"));
        assert!(run("main.py", &all).unwrap().starts_with("python3 '"));
        assert!(
            run("main.c", &|tool| tool == "clang")
                .unwrap()
                .contains("clang '")
        );
        assert!(run("Main.java", &all).unwrap().starts_with("java '"));
        assert!(run("main.go", &all).unwrap().starts_with("go run '"));
        assert!(
            run("index.ts", &|tool| tool == "npx")
                .unwrap()
                .contains("npx --yes tsx")
        );
        assert_eq!(
            run("main.py", &|_| false).unwrap_err(),
            "Install python3 to run .py files"
        );
        assert!(run("notes.md", &all).is_err());

        // Paths with quotes stay one word; Windows quotes for cmd.exe and runs Python as python.
        assert_eq!(quote("it's", false), r"'it'\''s'");
        let windows = command(
            Path::new("C:/x/a b.py"),
            Path::new("C:/x"),
            "windows",
            all,
            &scratch,
        );
        assert_eq!(windows.unwrap(), "python \"C:/x/a b.py\"");
    }
}
