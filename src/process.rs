//! Programs SozoIDE asks a question and waits for: they get no input and never run longer than
//! their time (`CLAUDE.md` §7.2). The ones the user talks to (shells, language servers, the
//! debugger, the installers) are started where they are used.

use std::{
    io::Read,
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use wait_timeout::ChildExt;

/// What `command` printed and how it ended; none when it could not start or was stopped for
/// taking longer than `limit`.
pub fn output(command: &mut Command, limit: Duration) -> Option<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Read while it runs: a program that fills a pipe nobody reads would never end.
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = reader(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let stderr = reader(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let status = match child.wait_timeout(limit) {
        Ok(Some(status)) => Some(status),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    };
    let (stdout, stderr) = (stdout.join().ok()?, stderr.join().ok()?);
    Some(Output {
        status: status?,
        stdout,
        stderr,
    })
}

/// Whether `command` ended well within `limit`.
pub fn succeeds(command: &mut Command, limit: Duration) -> bool {
    output(command, limit).is_some_and(|output| output.status.success())
}

/// The time a question to a tool of the system may take.
pub const QUICK: Duration = Duration::from_secs(4);

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// This test binary, which every system the tests run on has: `--list` prints the tests
    /// and ends; `--ignored the_one_that_waits` runs the test below, which never ends.
    fn this_binary() -> Command {
        Command::new(std::env::current_exe().unwrap())
    }

    #[test]
    #[ignore = "started by a_program_gets_no_input_and_is_stopped_after_its_time"]
    fn the_one_that_waits() {
        // Reads its input first: with none (`stdin` is null) it goes on to wait for ever.
        let mut line = String::new();
        let read = std::io::stdin().read_line(&mut line).unwrap();
        println!("read {read} bytes");
        thread::sleep(Duration::from_secs(600));
    }

    #[test]
    fn a_program_gets_no_input_and_is_stopped_after_its_time() {
        let listed = output(this_binary().arg("--list"), Duration::from_secs(30)).unwrap();
        assert!(listed.status.success());
        // More than a pipe holds (64 KiB), read while the program runs.
        assert!(String::from_utf8_lossy(&listed.stdout).contains("the_one_that_waits: test"));
        assert!(succeeds(
            this_binary().arg("--list"),
            Duration::from_secs(30)
        ));

        let started = Instant::now();
        let waiting = output(
            this_binary().args([
                "--ignored",
                "--exact",
                "process::tests::the_one_that_waits",
                "--nocapture",
            ]),
            Duration::from_millis(1500),
        );
        assert!(waiting.is_none(), "it was not stopped");
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(1500), "{took:?}");
        assert!(took < Duration::from_secs(20), "{took:?}");

        assert!(output(&mut Command::new("no-such-program-of-sozo"), QUICK).is_none());
        assert!(!succeeds(
            &mut Command::new("no-such-program-of-sozo"),
            QUICK
        ));
    }
}
