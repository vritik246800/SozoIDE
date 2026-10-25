use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

#[test]
fn application_starts_and_restores_host_terminal_on_quit() {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 35,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_sozo"));
    command.cwd(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);

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
                writer.write_all(b"\x1b[0n").unwrap();
                writer.flush().unwrap();
            }
            if data.windows(4).any(|window| window == b"\x1b[6n") {
                let mut writer = reader_writer.lock().unwrap();
                writer.write_all(b"\x1b[1;1R").unwrap();
                writer.flush().unwrap();
            }
        }
    });

    wait_for_output(&output, "SozoIDE");
    writer.lock().unwrap().write_all(b" ").unwrap();
    writer.lock().unwrap().flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    writer.lock().unwrap().write_all(b"\x11").unwrap();
    writer.lock().unwrap().flush().unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("a aplicacao nao terminou depois de Ctrl+Q");
        }
        thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success());

    let output = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
    assert!(output.contains("\x1b[?1049h"));
    assert!(output.contains("\x1b[?1049l"));
    // ConPTY merges the mouse modes into one sequence; Unix PTYs receive them separately.
    for state in ['h', 'l'] {
        assert!(
            output.contains(&format!("\x1b[?1003;1006{state}"))
                || (output.contains(&format!("\x1b[?1003{state}"))
                    && output.contains(&format!("\x1b[?1006{state}"))),
            "{output:?}"
        );
    }
}

fn wait_for_output(output: &Arc<Mutex<Vec<u8>>>, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if String::from_utf8_lossy(&output.lock().unwrap()).contains(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "saida '{expected}' nao encontrada: {}",
        String::from_utf8_lossy(&output.lock().unwrap())
    );
}
