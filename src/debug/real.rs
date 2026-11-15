//! The debug adapters of the machine, really started (TODO 3.4.1): only with
//! `cargo test --features servers`, and only on Linux — on macOS Delve and LLDB ask for
//! Developer Tools access in a dialog of the system, which a test cannot answer (that proof is
//! made by a person: `TESTING.md` §5.1). Each test is skipped, and says so, when its adapter or
//! compiler is not on this machine.

use std::{
    collections::{BTreeSet, HashMap},
    time::{Duration, Instant},
};

use super::{Event, Session, State, locate, plan};

/// Builds and debugs `name` with `text`, a breakpoint at `row`: where it stops and the
/// variables there.
fn stops_at(name: &str, text: &str, row: usize) -> Option<(usize, Vec<(String, String)>)> {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join(name);
    std::fs::write(&file, text).unwrap();
    if name.ends_with(".go") {
        std::fs::write(root.join("go.mod"), "module demo\n\ngo 1.21\n").unwrap();
    }
    let scratch = tempfile::tempdir().unwrap();
    let (plan, build) = match plan(&file, &root, scratch.path(), locate) {
        Ok(found) => found,
        Err(why) => {
            eprintln!("skipped: {why}");
            return None;
        }
    };
    let program = match build {
        Some(build) => build.run(scratch.path()).unwrap(),
        None => None,
    };
    let breakpoints = HashMap::from([(file.clone(), BTreeSet::from([row]))]);
    let mut session = Session::start(&plan, program.as_deref(), &breakpoints).unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut stopped = None;
    while Instant::now() < deadline {
        for event in session.poll() {
            if let Event::Stopped { row, .. } = event {
                stopped = Some(row);
            }
        }
        if stopped.is_some() && !session.variables.is_empty() {
            break;
        }
        assert!(
            !matches!(session.state, State::Ended(_)),
            "the program ended before stopping"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let row = stopped.expect("never stopped at the breakpoint");
    let variables = session.variables.clone();
    session.stop();
    Some((row, variables))
}

#[test]
fn delve_stops_at_a_breakpoint_in_go_and_shows_the_variables() {
    let text = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tcount := 41\n\tcount++\n\tfmt.Println(count)\n}\n";
    let Some((row, variables)) = stops_at("main.go", text, 6) else {
        return;
    };
    assert_eq!(row, 6);
    assert!(
        variables.iter().any(|(name, _)| name == "count"),
        "{variables:?}"
    );
}

#[test]
fn debugpy_stops_at_a_breakpoint_in_python() {
    let text = "count = 41\ncount += 1\nprint(count)\n";
    let Some((row, variables)) = stops_at("main.py", text, 1) else {
        return;
    };
    assert_eq!(row, 1);
    assert!(
        variables.iter().any(|(name, _)| name == "count"),
        "{variables:?}"
    );
}

#[test]
fn gdb_or_lldb_stops_at_a_breakpoint_in_c() {
    let text = "#include <stdio.h>\nint main(void) {\n    int count = 41;\n    count++;\n    printf(\"%d\\n\", count);\n    return 0;\n}\n";
    let Some((row, _)) = stops_at("main.c", text, 3) else {
        return;
    };
    assert_eq!(row, 3);
}
