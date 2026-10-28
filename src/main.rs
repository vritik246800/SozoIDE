mod animation;
mod app;
mod branding;
mod database;
mod editor;
mod git;
mod terminal;
mod ui;
mod update;

use std::{
    env,
    io::{self, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use app::App;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::picker::Picker;

fn main() -> Result<()> {
    let Some(root) = command_line_root()? else {
        return Ok(());
    };
    let mut app = App::with_picker(root, image_picker(), true)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        )
    );
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run(&mut terminal, &mut app);
    app.database.shutdown();
    disable_raw_mode()?;
    let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result?;
    if let Some(file) = app.updater.ready_file().map(PathBuf::from) {
        let root = app.root.clone();
        // Closes the shells of both terminals before the new version takes over.
        drop(app);
        update::install_and_restart(&file, &root)?;
    }
    Ok(())
}

/// Asks the terminal which image protocol it speaks. Inside tmux the question travels through
/// passthrough and only an attached client answers it; when nobody does, the reader thread of
/// ratatui-image stays alive and steals the cursor position reply that ratatui needs at startup.
fn image_picker() -> Picker {
    if tmux_terminal() && !tmux_passthrough_reaches_a_client() {
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
    let Ok(output) = Command::new("tmux")
        .args(["set", "-p", "allow-passthrough", "on", ";"])
        .args(["show", "-pv", "allow-passthrough", ";"])
        .args(["display", "-p", "#{session_attached}"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
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

fn command_line_root() -> Result<Option<PathBuf>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => Ok(Some(env::current_dir()?)),
        [argument] if argument == "--help" || argument == "-h" => {
            println!(
                "SozoIDE {}\n\nUsage: sozo [DIRECTORY]\n\nOpen DIRECTORY in SozoIDE. Defaults to the current directory.",
                env!("CARGO_PKG_VERSION")
            );
            Ok(None)
        }
        [argument] if argument == "--version" || argument == "-V" => {
            println!("sozo {}", env!("CARGO_PKG_VERSION"));
            Ok(None)
        }
        [argument] => {
            let root = argument
                .parse::<PathBuf>()?
                .canonicalize()
                .with_context(|| format!("Could not open directory {argument}"))?;
            if !root.is_dir() {
                bail!("{} is not a directory", root.display());
            }
            Ok(Some(root))
        }
        _ => bail!("Usage: sozo [DIRECTORY]"),
    }
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;
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
                Event::Paste(text) => match app.focus {
                    app::Focus::BottomTerminal => app.bottom_terminal.send_text(&text),
                    app::Focus::RightTerminal => app.right_terminal.send_text(&text),
                    _ => {}
                },
                _ => {}
            }
        }
    }
    Ok(())
}
