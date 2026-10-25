mod animation;
mod app;
mod branding;
mod database;
mod editor;
mod git;
mod terminal;
mod ui;

use std::{env, io, path::PathBuf, time::Duration};

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
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let mut app = App::with_picker(root, picker, true)?;

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
    result
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
