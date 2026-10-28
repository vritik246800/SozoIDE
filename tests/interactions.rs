use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

struct ChildGuard(Box<dyn Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

#[test]
fn keyboard_mouse_tabs_and_panels_work_in_a_real_pty() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("main.rs"),
        "fn main() { println!(\"FIRST_FILE_MARK\"); }\n",
    )
    .unwrap();
    std::fs::write(project.path().join("other.rs"), "// OTHER_FILE_MARK\n").unwrap();

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 42,
            cols: 150,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let binary =
        std::env::var("SOZO_TEST_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_sozo").to_string());
    let mut command = CommandBuilder::new(binary);
    command.cwd(project.path());
    command.arg(".");
    let child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut child = ChildGuard(child);

    let output = Arc::new(Mutex::new(Vec::new()));
    let reader_output = Arc::clone(&output);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let reader_writer = Arc::clone(&writer);
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let data = &buffer[..count];
            reader_output.lock().unwrap().extend_from_slice(data);
            // ratatui-image keeps a thread reading stdin until the device status reply arrives.
            if data.windows(4).any(|window| window == b"\x1b[5n") {
                let mut writer = reader_writer.lock().unwrap();
                let _ = writer.write_all(b"\x1b[0n");
                let _ = writer.flush();
            }
            if data.windows(4).any(|window| window == b"\x1b[6n") {
                let mut writer = reader_writer.lock().unwrap();
                let _ = writer.write_all(b"\x1b[1;1R");
                let _ = writer.flush();
            }
        }
    });
    let send = |bytes: &[u8]| {
        let mut writer = writer.lock().unwrap();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
        thread::sleep(Duration::from_millis(200));
    };

    wait_for_output(&output, "Starting...");
    send(b" ");
    wait_for_output(&output, "main.rs");

    // Mouse: open two files, select the first tab and close the second tab.
    send(b"\x1b[<0;3;3M\x1b[<0;3;3m");
    wait_for_output(&output, "FIRST_FILE_MARK");
    send(b"\x1b[<0;3;4M\x1b[<0;3;4m");
    wait_for_output(&output, "OTHER_FILE_MARK");
    send(b"\x1b[<0;35;2M\x1b[<0;35;2m");
    send(b"\x1b[<0;59;2M\x1b[<0;59;2m");

    // Edit the active file and wait for the two-second idle autosave.
    send(b"EDITED_MARK ");
    thread::sleep(Duration::from_millis(2300));
    assert!(
        std::fs::read_to_string(project.path().join("main.rs"))
            .unwrap()
            .starts_with("EDITED_MARK fn main()")
    );

    // Global panels and search.
    send(b"\x07");
    wait_for_output(&output, "Source Control");
    send(b"\x04");
    wait_for_output(&output, "Add Connection");
    send(b"\x1b");
    send(b"\x10");
    wait_for_output(&output, "Find in file");
    send(b"FIRST");
    send(b"\x1b");

    send(b"\x1bOP");
    wait_for_output(&output, "Help - all commands");
    send(b"\x1b");
    send(b"\x1bOQ");
    wait_for_output(&output, "Valabdas");
    send(b"\x1b");

    // Mouse drag on the Explorer divider.
    send(b"\x1b[<0;31;5M\x1b[<32;41;5M\x1b[<0;41;5m");

    send(b"\x11");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            panic!("Application did not quit after Ctrl+Q");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_output(output: &Arc<Mutex<Vec<u8>>>, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        if String::from_utf8_lossy(&output.lock().unwrap()).contains(expected)
            || rendered_output(output).contains(expected)
        {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "Output {expected:?} not found:\n{}",
        rendered_output(output)
    );
}

fn rendered_output(output: &Arc<Mutex<Vec<u8>>>) -> String {
    let data = output.lock().unwrap().clone();
    let mut parser = vt100::Parser::new(42, 150, 0);
    parser.process(&data);
    parser.screen().contents()
}
