//! Ctrl+R (F25): the command that compiles and runs a file, or its project, typed into the
//! bottom terminal. The terminal's shell runs it, so input, colours and Ctrl+C work as they do
//! there, and SozoIDE keeps nothing running of its own. F7, Shift+F7, Ctrl+F7 and Ctrl+F5
//! (F32): the project found by its manifest, built, cleaned or run the same way.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// What F7, Shift+F7, Ctrl+F7 and Ctrl+F5 ask of a project (F32).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Build,
    Clean,
    Rebuild,
    Run,
}

impl Task {
    pub fn label(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Clean => "Clean",
            Self::Rebuild => "Rebuild",
            Self::Run => "Run",
        }
    }
}

/// The commands of one task (each a program and its arguments, one after the other) and the
/// folder they run in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub folder: PathBuf,
    pub commands: Vec<Vec<String>>,
}

/// A project found by its manifest: what it is, its folder, the file that says so and the
/// commands it has. A command nobody can know safely (CMake's program, Maven's main class)
/// is none: `.sozo.toml` gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub kind: &'static str,
    pub folder: PathBuf,
    pub source: String,
    pub build: Option<Step>,
    pub clean: Option<Step>,
    pub run: Option<Step>,
}

/// The file of a project's own commands (Q68).
pub const TASKS_FILE: &str = ".sozo.toml";

/// Folders a monorepo's projects are not in.
const SKIPPED: [&str; 6] = ["node_modules", "target", "build", "dist", "vendor", "venv"];

/// The projects of `root`: its own, or, when it has none, those of its folders one level
/// down (a monorepo's frontend and backend). Nothing is run to find them.
pub fn detect(root: &Path, os: &str) -> Vec<Project> {
    if let Some(project) = project_in(root, os) {
        return vec![project];
    }
    let mut folders = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(".");
            path.is_dir() && !name.starts_with('.') && !SKIPPED.contains(&name)
        })
        .collect::<Vec<_>>();
    folders.sort();
    folders
        .iter()
        .filter_map(|folder| project_in(folder, os))
        .collect()
}

/// The project of `folder`, by the first manifest found, `.sozo.toml` first.
fn project_in(folder: &Path, os: &str) -> Option<Project> {
    let has = |name: &str| folder.join(name).is_file();
    let read = |name: &str| fs::read_to_string(folder.join(name)).unwrap_or_default();
    let words = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let step = |commands: &[&[&str]]| {
        Some(Step {
            folder: folder.to_path_buf(),
            commands: commands.iter().map(|command| words(command)).collect(),
        })
    };
    let python = if os == "windows" { "python" } else { "python3" };
    let project = |kind, source: &str, build, clean, run| {
        Some(Project {
            kind,
            folder: folder.to_path_buf(),
            source: source.to_owned(),
            build,
            clean,
            run,
        })
    };
    if has(TASKS_FILE) {
        let table = read(TASKS_FILE).parse::<toml::Table>().ok()?;
        let task = |name: &str| {
            let task = table.get("tasks")?.get(name)?;
            let mut command = vec![task.get("program")?.as_str()?.to_owned()];
            command.extend(
                task.get("args")
                    .and_then(|args| args.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|arg| arg.as_str().map(str::to_owned)),
            );
            let cwd = task.get("cwd").and_then(|cwd| cwd.as_str()).unwrap_or(".");
            Some(Step {
                folder: folder.join(cwd),
                commands: vec![command],
            })
        };
        return project(
            "Custom",
            TASKS_FILE,
            task("build"),
            task("clean"),
            task("run"),
        );
    }
    if has("Cargo.toml") {
        return project(
            "Cargo",
            "Cargo.toml",
            step(&[&["cargo", "build"]]),
            step(&[&["cargo", "clean"]]),
            step(&[&["cargo", "run"]]),
        );
    }
    if has("go.mod") {
        return project(
            "Go",
            "go.mod",
            step(&[&["go", "build", "./..."]]),
            step(&[&["go", "clean"]]),
            step(&[&["go", "run", "."]]),
        );
    }
    if has("package.json") {
        let manager = [
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("bun.lockb", "bun"),
            ("bun.lock", "bun"),
        ]
        .iter()
        .find(|(lock, _)| has(lock))
        .map_or("npm", |(_, manager)| manager);
        let json =
            serde_json::from_str::<serde_json::Value>(&read("package.json")).unwrap_or_default();
        // Only the scripts the project has: no `npm run build` where there is none.
        let script = |names: &[&str]| {
            let name = names
                .iter()
                .find(|name| json["scripts"][**name].is_string())?;
            step(&[&[manager, "run", name]])
        };
        return project(
            "Node.js",
            "package.json",
            script(&["build"]),
            script(&["clean"]),
            script(&["dev", "start"]),
        );
    }
    if has("CMakeLists.txt") {
        return project(
            "CMake",
            "CMakeLists.txt",
            step(&[
                &["cmake", "-S", ".", "-B", "build"],
                &["cmake", "--build", "build"],
            ]),
            step(&[&["cmake", "--build", "build", "--target", "clean"]]),
            None,
        );
    }
    if let Some(name) = ["Makefile", "makefile", "GNUmakefile"]
        .into_iter()
        .find(|name| has(name))
    {
        let text = read(name);
        let target = |target: &str| {
            text.lines()
                .any(|line| line.starts_with(&format!("{target}:")))
                .then(|| step(&[&["make", target]]))
                .flatten()
        };
        return project(
            "Make",
            name,
            step(&[&["make"]]),
            target("clean"),
            target("run"),
        );
    }
    if has("pom.xml") {
        return project(
            "Maven",
            "pom.xml",
            step(&[&["mvn", "package"]]),
            step(&[&["mvn", "clean"]]),
            None,
        );
    }
    if let Some(name) = ["build.gradle.kts", "build.gradle"]
        .into_iter()
        .find(|name| has(name))
    {
        let wrapper = if os == "windows" {
            "gradlew.bat"
        } else {
            "./gradlew"
        };
        let gradle = if has(wrapper.trim_start_matches("./")) {
            wrapper
        } else {
            "gradle"
        };
        let run = read(name)
            .contains("application")
            .then(|| step(&[&[gradle, "run"]]))
            .flatten();
        return project(
            "Gradle",
            name,
            step(&[&[gradle, "build"]]),
            step(&[&[gradle, "clean"]]),
            run,
        );
    }
    if let Some(name) = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".sln") || name.ends_with(".csproj"))
        .min()
    {
        return project(
            ".NET",
            &name,
            step(&[&["dotnet", "build"]]),
            step(&[&["dotnet", "clean"]]),
            step(&[&["dotnet", "run"]]),
        );
    }
    if has("Package.swift") {
        return project(
            "Swift",
            "Package.swift",
            step(&[&["swift", "build"]]),
            step(&[&["swift", "package", "clean"]]),
            step(&[&["swift", "run"]]),
        );
    }
    if has("pubspec.yaml") {
        if read("pubspec.yaml").contains("sdk: flutter") {
            return project(
                "Flutter",
                "pubspec.yaml",
                None,
                step(&[&["flutter", "clean"]]),
                step(&[&["flutter", "run"]]),
            );
        }
        return project(
            "Dart",
            "pubspec.yaml",
            None,
            None,
            step(&[&["dart", "run"]]),
        );
    }
    if has("manage.py") {
        let run = step(&[&[python, "manage.py", "runserver"]]);
        return project("Django", "manage.py", None, None, run);
    }
    if has("artisan") {
        let run = step(&[&["php", "artisan", "serve"]]);
        return project("Laravel", "artisan", None, None, run);
    }
    if has("Gemfile") {
        let run = if folder.join("bin").join("rails").is_file() {
            step(&[&["bin/rails", "server"]])
        } else if has("config.ru") {
            step(&[&["bundle", "exec", "rackup"]])
        } else {
            None
        };
        return project("Ruby", "Gemfile", None, None, run);
    }
    if let Some(name) = [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ]
    .into_iter()
    .find(|name| has(name))
    {
        return project(
            "Docker Compose",
            name,
            step(&[&["docker", "compose", "build"]]),
            step(&[&["docker", "compose", "down"]]),
            step(&[&["docker", "compose", "up", "--build"]]),
        );
    }
    if has("Dockerfile") {
        let image = folder
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase().replace(' ', "-"))
            .unwrap_or_else(|| "app".into());
        return project(
            "Docker",
            "Dockerfile",
            step(&[&["docker", "build", "-t", &image, "."]]),
            None,
            step(&[&["docker", "run", "--rm", "-it", &image]]),
        );
    }
    if let Some(source) = ["pyproject.toml", "requirements.txt"]
        .into_iter()
        .find(|name| has(name))
    {
        let run = ["main.py", "app.py"]
            .into_iter()
            .find(|name| has(name))
            .and_then(|main| step(&[&[python, main]]));
        return project("Python", source, None, None, run);
    }
    None
}

/// The shell line of `task` for `project` (`cd` to its folder, then its commands), with the
/// tools this machine `has`; the error says why there is none.
pub fn task_line(
    project: &Project,
    task: Task,
    os: &str,
    has: impl Fn(&str) -> bool,
) -> Result<String, String> {
    let windows = os == "windows";
    let wanted = |step: &Option<Step>, task: Task| {
        step.clone().ok_or_else(|| {
            format!(
                "This {} project has no {} command: add [tasks.{}] to {TASKS_FILE}",
                project.kind,
                task.label().to_lowercase(),
                task.label().to_lowercase()
            )
        })
    };
    let steps = match task {
        Task::Build => vec![wanted(&project.build, task)?],
        Task::Clean => vec![wanted(&project.clean, task)?],
        Task::Run => vec![wanted(&project.run, task)?],
        // Without a clean command, Rebuild builds.
        Task::Rebuild => project
            .clean
            .iter()
            .cloned()
            .chain([wanted(&project.build, Task::Build)?])
            .collect(),
    };
    let mut parts = Vec::new();
    let mut folder = None;
    for step in &steps {
        for command in &step.commands {
            let program = &command[0];
            // Programs of the project itself (`./gradlew`, `bin/rails`) are not on the PATH.
            if !program.contains('/') && !program.ends_with(".bat") && !has(program) {
                return Err(format!("{program} is missing: Help, then Dependencies"));
            }
        }
        if folder.as_ref() != Some(&step.folder) {
            let cd = if windows { "cd /d" } else { "cd" };
            parts.push(format!(
                "{cd} {}",
                quote(&step.folder.display().to_string(), windows)
            ));
            folder = Some(step.folder.clone());
        }
        for command in &step.commands {
            parts.push(shell_line(command, os));
        }
    }
    Ok(parts.join(" && "))
}

/// `words` as one line of the shell of `os`.
pub fn shell_line(words: &[String], os: &str) -> String {
    let windows = os == "windows";
    words
        .iter()
        .map(|word| shell_word(word, windows))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A word as it is when the shell takes it so, quoted otherwise.
fn shell_word(word: &str, windows: bool) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@+,".contains(c))
    {
        word.to_owned()
    } else {
        quote(word, windows)
    }
}

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
        // Paths joined as the code joins them: `\\` between the parts on Windows.
        let app = root.join("app");
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::write(app.join("Cargo.toml"), "").unwrap();
        let scratch = PathBuf::from("/tmp/sozo run");
        let all = |_: &str| true;
        let run =
            |file: &Path, has: &dyn Fn(&str) -> bool| command(file, root, "linux", has, &scratch);
        let cargo = format!(
            "cargo run --manifest-path '{}'",
            app.join("Cargo.toml").display()
        );
        assert_eq!(run(&app.join("src").join("main.rs"), &all).unwrap(), cargo);
        let single = run(&root.join("tool.rs"), &all).unwrap();
        assert!(
            single.starts_with("mkdir -p '/tmp/sozo run' && rustc '"),
            "{single}"
        );
        let program = scratch.join("tool").display().to_string();
        assert!(
            single.ends_with(&format!("-o '{program}' && '{program}'")),
            "{single}"
        );
        let run = |file: &str, has: &dyn Fn(&str) -> bool| run(&root.join(file), has);
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

    #[test]
    fn projects_are_found_by_their_manifests_with_their_commands() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        // Nothing yet; then a monorepo: one project per folder, hidden and build folders out.
        assert!(detect(root, "linux").is_empty());
        write(
            "web/package.json",
            r#"{"scripts": {"dev": "vite", "build": "vite build"}}"#,
        );
        write("web/pnpm-lock.yaml", "");
        write("api/go.mod", "module api");
        write("node_modules/x/package.json", "{}");
        write(".hidden/Cargo.toml", "");
        let found = detect(root, "linux");
        let kinds = found.iter().map(|project| project.kind).collect::<Vec<_>>();
        assert_eq!(kinds, ["Go", "Node.js"]);
        let web = &found[1];
        assert_eq!(web.source, "package.json");
        let commands = |step: &Option<Step>| step.as_ref().map(|step| step.commands.clone());
        let words = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            commands(&web.run),
            Some(vec![words(&["pnpm", "run", "dev"])])
        );
        assert_eq!(
            commands(&web.build),
            Some(vec![words(&["pnpm", "run", "build"])])
        );
        assert_eq!(web.clean, None, "no clean script, no clean command");

        // A project at the root wins over its folders; `.sozo.toml` over every manifest.
        write("Makefile", "all:\n\tcc main.c\nclean:\n\trm -f a.out\n");
        let make = &detect(root, "linux")[0];
        assert_eq!(make.kind, "Make");
        assert_eq!(commands(&make.clean), Some(vec![words(&["make", "clean"])]));
        assert_eq!(make.run, None, "no run target");
        write(
            ".sozo.toml",
            "[tasks.run]\nprogram = \"python3\"\nargs = [\"serve.py\", \"--port\", \"8000\"]\ncwd = \"api\"\n",
        );
        let custom = &detect(root, "linux")[0];
        assert_eq!(
            (custom.kind, custom.source.as_str()),
            ("Custom", ".sozo.toml")
        );
        let run = custom.run.as_ref().unwrap();
        assert_eq!(run.folder, root.join("api"));
        assert_eq!(
            run.commands,
            [words(&["python3", "serve.py", "--port", "8000"])]
        );
        assert_eq!(custom.build, None);

        // Other manifests, one folder each.
        let other = tempfile::tempdir().unwrap();
        let one = |files: &[(&str, &str)], os| {
            let folder = other.path().join(files[0].0.replace(['/', '.'], "_"));
            for (name, text) in files {
                let path = folder.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            project_in(&folder, os).unwrap()
        };
        let cargo = one(&[("Cargo.toml", "")], "linux");
        assert_eq!(commands(&cargo.run), Some(vec![words(&["cargo", "run"])]));
        let cmake = one(&[("CMakeLists.txt", "")], "linux");
        assert_eq!(
            cmake.build.unwrap().commands.len(),
            2,
            "configure, then build"
        );
        assert_eq!(cmake.run, None);
        let gradle = one(
            &[
                ("build.gradle.kts", "plugins { application }"),
                ("gradlew", ""),
            ],
            "linux",
        );
        assert_eq!(
            commands(&gradle.run),
            Some(vec![words(&["./gradlew", "run"])])
        );
        let django = one(&[("manage.py", "")], "windows");
        assert_eq!(
            commands(&django.run),
            Some(vec![words(&["python", "manage.py", "runserver"])])
        );
        let dotnet = one(&[("App.csproj", "")], "linux");
        assert_eq!(
            (dotnet.kind, dotnet.source.as_str()),
            (".NET", "App.csproj")
        );
        let compose = one(&[("compose.yaml", "")], "linux");
        assert_eq!(
            commands(&compose.run),
            Some(vec![words(&["docker", "compose", "up", "--build"])])
        );
        let flutter = one(
            &[(
                "pubspec.yaml",
                "dependencies:\n  flutter:\n    sdk: flutter",
            )],
            "linux",
        );
        assert_eq!(flutter.kind, "Flutter");
        let python = one(&[("requirements.txt", ""), ("app.py", "")], "linux");
        assert_eq!(
            commands(&python.run),
            Some(vec![words(&["python3", "app.py"])])
        );
    }

    #[test]
    fn tasks_become_shell_lines_or_say_why() {
        let folder = PathBuf::from("/work/my app");
        let step = |commands: &[&[&str]]| {
            Some(Step {
                folder: folder.clone(),
                commands: commands
                    .iter()
                    .map(|command| command.iter().map(|w| w.to_string()).collect())
                    .collect(),
            })
        };
        let project = Project {
            kind: "CMake",
            folder: folder.clone(),
            source: "CMakeLists.txt".into(),
            build: step(&[
                &["cmake", "-S", ".", "-B", "build"],
                &["cmake", "--build", "build"],
            ]),
            clean: step(&[&["cmake", "--build", "build", "--target", "clean"]]),
            run: None,
        };
        let all = |_: &str| true;
        assert_eq!(
            task_line(&project, Task::Build, "linux", all).unwrap(),
            "cd '/work/my app' && cmake -S . -B build && cmake --build build"
        );
        assert_eq!(
            task_line(&project, Task::Rebuild, "linux", all).unwrap(),
            "cd '/work/my app' && cmake --build build --target clean && cmake -S . -B build && cmake --build build",
            "clean, then build, one cd"
        );
        assert_eq!(
            task_line(&project, Task::Build, "windows", all).unwrap(),
            "cd /d \"/work/my app\" && cmake -S . -B build && cmake --build build"
        );
        assert_eq!(
            task_line(&project, Task::Run, "linux", all).unwrap_err(),
            "This CMake project has no run command: add [tasks.run] to .sozo.toml"
        );
        assert_eq!(
            task_line(&project, Task::Build, "linux", |tool| tool != "cmake").unwrap_err(),
            "cmake is missing: Help, then Dependencies"
        );
        // Without a clean command Rebuild builds; the project's own programs need no PATH.
        let gradle = Project {
            kind: "Gradle",
            build: step(&[&["./gradlew", "build"]]),
            clean: None,
            ..project
        };
        assert_eq!(
            task_line(&gradle, Task::Rebuild, "linux", |_| false).unwrap(),
            "cd '/work/my app' && ./gradlew build"
        );
        assert_eq!(shell_word("it's", false), r"'it'\''s'");
        assert_eq!(shell_word("--port=8000", false), "--port=8000");
    }
}
