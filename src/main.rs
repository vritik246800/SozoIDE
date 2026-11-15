mod animation;
mod app;
mod branding;
#[cfg(test)]
mod budgets;
mod cli;
mod database;
mod debug;
mod editor;
mod git;
mod lsp;
mod media;
mod process;
#[cfg(test)]
mod properties;
mod run;
mod settings;
mod storage;
mod terminal;
mod ui;
mod update;

use std::{
    env,
    io::{self, Write},
    path::PathBuf,
    process::Command,
    time::Duration,
};

use anyhow::Result;
use app::App;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::picker::Picker;

fn main() -> Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let (root, file) = match cli::parse(&arguments, &env::current_dir()?)? {
        cli::Request::Print(text) => {
            println!("{text}");
            return Ok(());
        }
        cli::Request::Open { root, file } => (root, file),
    };
    let mut app = App::with_picker(root, image_picker(), true)?;
    // The interface came up: the version before an update is no longer kept (TODO 6.1.2).
    update::confirm_started();
    // `sozo src/main.rs:12:5`: the file at its place (TODO 6.1.1).
    if let Some((path, line, column)) = file {
        app.open_file_at(path, line, column);
    }

    stop_cleanly_on_panic();
    enable_raw_mode()?;
    // Whatever fails from here on, the terminal goes back as it was.
    let result = (|| -> Result<()> {
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        // Without it a paste arrives as typing: each new line would run in a terminal (E58).
        let _ = execute!(stdout, EnableBracketedPaste);
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        );
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        terminal.clear()?;
        run(&mut terminal, &mut app)
    })();
    app.database.shutdown();
    restore_terminal();
    result?;
    if let Some(file) = app.updater.ready_file().map(PathBuf::from) {
        let root = app.root.clone();
        // Closes the shells of both terminals before the new version takes over.
        drop(app);
        update::install_and_restart(&file, &root)?;
    }
    Ok(())
}

/// Puts the terminal back as SozoIDE found it: safe to call twice and from any thread.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, PopKeyboardEnhancementFlags, DisableBracketedPaste);
    let _ = execute!(
        stdout,
        LeaveAlternateScreen,
        DisableMouseCapture,
        crossterm::cursor::Show
    );
}

/// A panic in any thread gives the terminal back and says where it happened (R12), then
/// SozoIDE stops: what is in memory can no longer be trusted, and the autosave has already
/// written all but the last two seconds. `crash.log` keeps the version, the system, the place
/// in the code and the message; never the text of a file.
fn stop_cleanly_on_panic() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default(info);
        let place = info.location().map_or_else(
            || "an unknown place".to_string(),
            |location| format!("{}:{}", location.file(), location.line()),
        );
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let report = format!(
            "SozoIDE {} stopped unexpectedly on {} {}\nat {place}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            env::consts::OS,
            env::consts::ARCH,
            message.chars().take(300).collect::<String>(),
        );
        let mut stderr = io::stderr();
        let _ = write!(
            stderr,
            "\n{report}Please report it: {}/issues\n",
            branding::REPOSITORY
        );
        if let Some(log) = directories::ProjectDirs::from("", "", "SozoIDE")
            .map(|directories| directories.cache_dir().join("crash.log"))
            && storage::write(&log, &report).is_ok()
        {
            let _ = writeln!(stderr, "Kept in {}", log.display());
        }
        std::process::exit(101);
    }));
}

/// Asks the terminal which image protocol it speaks. Inside tmux the question travels through
/// passthrough and only an attached client answers it; when nobody does, the reader thread of
/// ratatui-image stays alive and steals the cursor position reply that ratatui needs at startup.
fn image_picker() -> Picker {
    if tmux_terminal() && !tmux_passthrough_reaches_a_client() {
        return Picker::halfblocks();
    }
    // A Windows console only reads the question once its VT processing is on; one that cannot
    // turn it on would print the question as text (E37).
    #[cfg(windows)]
    if !crossterm::ansi_support::supports_ansi() {
        return Picker::halfblocks();
    }
    Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
}

/// The same test ratatui-image uses to wrap its queries in tmux passthrough.
fn tmux_terminal() -> bool {
    env::var("TERM").is_ok_and(|term| term.starts_with("tmux"))
        || env::var("TERM_PROGRAM").is_ok_and(|program| program == "tmux")
}

fn tmux_passthrough_reaches_a_client() -> bool {
    let Some(output) = process::output(
        Command::new("tmux")
            .args(["set", "-p", "allow-passthrough", "on", ";"])
            .args(["show", "-pv", "allow-passthrough", ";"])
            .args(["display", "-p", "#{session_attached}"]),
        process::QUICK,
    ) else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    output.status.success()
        && matches!(lines.next(), Some("on" | "all"))
        && lines
            .next()
            .and_then(|clients| clients.parse::<u32>().ok())
            .is_some_and(|clients| clients > 0)
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        app.tick();
        terminal.draw(|frame| {
            ui::draw(frame, app);
            // `NO_COLOR` or `high_contrast`: attributes only (TODO 5.2.7).
            if ui::colourless() {
                ui::without_colour(frame.buffer_mut());
            }
        })?;
        // Debug builds only: the test of the panic hook asks for a panic here or in a thread.
        #[cfg(debug_assertions)]
        match env::var("SOZO_TEST_PANIC").as_deref() {
            Ok("main") => panic!("asked for by SOZO_TEST_PANIC"),
            Ok("thread") => {
                let _ = std::thread::spawn(|| panic!("asked for by SOZO_TEST_PANIC")).join();
            }
            _ => {}
        }
        // Copied text also goes to the terminal's clipboard, which works over SSH.
        if let Some(text) = app.clipboard.take() {
            let backend = terminal.backend_mut();
            backend.write_all(app::clipboard_sequence(&text).as_bytes())?;
            backend.flush()?;
        }
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    app.handle_key(key);
                }
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                Event::Paste(text) if app.name_prompt.is_some() => app.name_paste(&text),
                Event::Paste(text) if app.search.is_some() && !app.motion.search_closing() => {
                    app.search_paste(&text);
                }
                Event::Paste(text) if app.focus == app::Focus::Database => {
                    app.database.paste(&text);
                }
                Event::Paste(text) if app.focus == app::Focus::Git => {
                    app.git.paste(&text);
                }
                Event::Paste(text) if app.focus == app::Focus::Editor => {
                    app.editor_paste(&text);
                }
                Event::Paste(text) => {
                    if matches!(
                        app.focus,
                        app::Focus::BottomTerminal | app::Focus::RightTerminal
                    ) {
                        app.terminals(app.focus).current_mut().paste(&text);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
