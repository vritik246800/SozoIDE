use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use ratatui_image::picker::Picker;

use crate::{
    database::controller::DatabaseController,
    editor::{OpenFile, SearchState},
    git::controller::GitController,
    terminal::Terminal,
    update::{UpdateState, Updater},
};

mod explorer;
mod input;
mod layout;
mod motion;

pub use explorer::FileEntry;
use layout::{DragTarget, inner_size};
pub use layout::{Focus, Overlay, Regions};
pub(crate) use motion::AppMotion;

/// Nerd Fonts glyphs by their `nerd_fonts` name. Copied instead of `NerdFonts::load()`, which
/// reads its table at runtime from the build machine's cargo registry and panics on every other
/// machine (ERRORS.md E10); a test keeps them equal to the crate's table.
const ICONS: [(&str, char); 3] = [
    ("custom-folder", '\u{e5ff}'),
    ("custom-folder_open", '\u{e5fe}'),
    ("dev-terminal", '\u{e795}'),
];

pub struct App {
    pub root: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected_entry: usize,
    pub explorer_scroll: usize,
    pub open_files: Vec<OpenFile>,
    pub active_tab: Option<usize>,
    pub focus: Focus,
    pub show_explorer: bool,
    pub show_bottom_terminal: bool,
    pub show_right_terminal: bool,
    pub show_database: bool,
    pub database: DatabaseController,
    pub show_git: bool,
    pub git: GitController,
    pub explorer_width: u16,
    pub right_terminal_width: u16,
    pub bottom_terminal_height: u16,
    pub bottom_terminal: Terminal,
    pub right_terminal: Terminal,
    pub regions: Regions,
    pub status: String,
    pub should_quit: bool,
    pub overlay: Overlay,
    pub search: Option<SearchState>,
    pub splash_visible: bool,
    pub motion: AppMotion,
    pub updater: Updater,
    splash_until: Instant,
    picker: Picker,
    drag: Option<DragTarget>,
}

impl App {
    #[cfg(test)]
    pub fn new(root: PathBuf) -> Result<Self> {
        Self::with_picker(root, Picker::halfblocks(), false)
    }

    pub fn with_picker(root: PathBuf, picker: Picker, splash: bool) -> Result<Self> {
        let bottom_terminal = Terminal::new(&root).context("Could not open the bottom terminal")?;
        let right_terminal = Terminal::new(&root).context("Could not open the right terminal")?;
        let git = GitController::new(root.clone());
        let now = Instant::now();
        let mut app = Self {
            root,
            entries: Vec::new(),
            selected_entry: 0,
            explorer_scroll: 0,
            open_files: Vec::new(),
            active_tab: None,
            focus: Focus::Explorer,
            show_explorer: true,
            show_bottom_terminal: false,
            show_right_terminal: false,
            show_database: false,
            database: DatabaseController::new()?,
            show_git: false,
            git,
            explorer_width: 30,
            right_terminal_width: 42,
            bottom_terminal_height: 12,
            bottom_terminal,
            right_terminal,
            regions: Regions::default(),
            status: "Ready".to_string(),
            should_quit: false,
            overlay: Overlay::None,
            search: None,
            splash_visible: splash,
            motion: AppMotion::new(now, splash),
            updater: Updater::new(crate::update::enabled()),
            splash_until: now + Duration::from_millis(1400),
            picker,
            drag: None,
        };
        app.refresh_entries()?;
        Ok(app)
    }

    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    pub fn tick_at(&mut self, now: Instant) {
        self.motion.tick(now);
        self.motion.sync_panels(
            self.show_explorer,
            self.show_bottom_terminal,
            self.show_right_terminal,
        );
        let tab = self.active_tab.and_then(|index| {
            self.open_files
                .get(index)
                .map(|file| (index, file.path.as_path()))
        });
        self.motion
            .sync_content(self.show_git, self.show_database, tab);
        if self.splash_visible && now >= self.splash_until && !self.motion.splash_closing() {
            self.motion.close_splash(now);
        }
        if self.splash_visible && self.motion.splash_closed() {
            self.splash_visible = false;
        }
        if self.overlay != Overlay::None && self.motion.overlay_closed() {
            self.overlay = Overlay::None;
        }
        if self.search.is_some() && self.motion.search_closed() {
            self.search = None;
        }
        let due = self
            .open_files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.autosave_due(now))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for index in due {
            self.save_file(index);
        }
        self.database.tick_at(now);
        self.git.tick_at(now);
        self.tick_update();
    }

    /// Shows the update notice once nothing else is on screen; quits to install when the
    /// download is done (`main` installs and restarts).
    fn tick_update(&mut self) {
        self.updater.tick();
        if let UpdateState::Downloading {
            received,
            total: Some(total),
        } = self.updater.state
        {
            self.motion
                .sync_download((received as f64 / total.max(1) as f64) as f32);
        }
        match self.updater.state {
            UpdateState::Available
                if self.overlay == Overlay::None
                    && !self.splash_visible
                    && self.search.is_none() =>
            {
                self.open_overlay(Overlay::Update);
            }
            UpdateState::Ready(_) => {
                self.flush_all_files();
                self.should_quit = true;
            }
            _ => {}
        }
    }

    pub fn icon(&self, name: &str) -> char {
        ICONS
            .iter()
            .find(|(icon, _)| *icon == name)
            .map_or(' ', |(_, glyph)| *glyph)
    }

    pub fn open_file(&mut self, path: PathBuf) -> Result<()> {
        if let Some(index) = self.open_files.iter().position(|file| file.path == path) {
            if self.active_tab != Some(index) {
                self.flush_active_file();
                self.active_tab = Some(index);
            }
            self.focus = Focus::Editor;
            return Ok(());
        }
        let file = OpenFile::load(path, &mut self.picker)?;
        self.flush_active_file();
        self.status = format!("Opened {}", file.name);
        self.open_files.push(file);
        self.active_tab = Some(self.open_files.len() - 1);
        self.focus = Focus::Editor;
        Ok(())
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.open_files.len() {
            return;
        }
        self.save_file(index);
        self.open_files.remove(index);
        self.active_tab = match (self.open_files.is_empty(), self.active_tab) {
            (true, _) => None,
            (false, Some(active)) if active > index => Some(active - 1),
            (false, Some(active)) if active == index => Some(index.min(self.open_files.len() - 1)),
            (false, active) => active,
        };
    }

    pub fn editor_paste(&mut self, text: &str) {
        if self.focus != Focus::Editor
            || self
                .active_tab
                .and_then(|index| self.open_files.get(index))
                .is_none_or(|file| file.image.is_some())
        {
            return;
        }
        for character in text.chars() {
            match character {
                '\r' => {}
                '\n' | '\t' => self.insert_editor_character(character),
                character if !character.is_control() => self.insert_editor_character(character),
                _ => {}
            }
        }
    }

    fn insert_editor_character(&mut self, character: char) {
        if let Some(active) = self.active_tab {
            self.open_files[active].insert(character);
        }
    }

    pub(super) fn flush_active_file(&mut self) {
        if let Some(active) = self.active_tab {
            self.save_file(active);
        }
    }

    pub(super) fn flush_all_files(&mut self) {
        for index in 0..self.open_files.len() {
            self.save_file(index);
        }
    }

    fn save_file(&mut self, index: usize) {
        let Some(file) = self.open_files.get_mut(index) else {
            return;
        };
        if !file.dirty {
            return;
        }
        let name = file.name.clone();
        self.status = match file.save() {
            Ok(()) => format!("Saved {name}"),
            Err(error) => error.to_string(),
        };
    }

    #[cfg(test)]
    pub fn active_file(&self) -> Option<&OpenFile> {
        self.active_tab.and_then(|index| self.open_files.get(index))
    }

    pub fn resize_terminals(&mut self) {
        if self.show_bottom_terminal && self.motion.bottom_terminal_stable() {
            self.bottom_terminal
                .resize(inner_size(self.regions.bottom_terminal));
        }
        if self.show_right_terminal && self.motion.right_terminal_stable() {
            self.right_terminal
                .resize(inner_size(self.regions.right_terminal));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        App::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn icons_match_the_nerd_fonts_table() {
        let fonts = nerd_fonts::NerdFonts {
            nf: nerd_fonts::NerdFonts::load(),
        };
        for (name, glyph) in ICONS {
            assert_eq!(fonts.get(name), Some(glyph), "{name}");
        }
        let app = test_app();
        assert_eq!(app.icon("dev-terminal"), '\u{e795}');
        assert_eq!(app.icon("custom-file"), ' ');
    }

    #[test]
    fn opens_files_in_tabs_without_duplicates() {
        let mut app = test_app();
        let cargo = app.root.join("Cargo.toml");
        let main = app.root.join("src/main.rs");
        app.open_file(cargo.clone()).unwrap();
        app.open_file(main).unwrap();
        app.open_file(cargo).unwrap();
        assert_eq!(app.open_files.len(), 2);
        assert_eq!(app.active_tab, Some(0));
        app.close_tab(0);
        assert_eq!(app.open_files.len(), 1);
        assert_eq!(app.active_tab, Some(0));
    }

    #[test]
    fn expanding_nested_directories_keeps_parent_open() {
        let mut app = test_app();
        let src = app.root.join("src");
        app.selected_entry = app
            .entries
            .iter()
            .position(|entry| entry.path == src)
            .unwrap();
        app.activate_selected();
        assert!(
            app.entries
                .iter()
                .any(|entry| entry.path == app.root.join("src/main.rs"))
        );
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(
            !app.entries
                .iter()
                .find(|entry| entry.path == src)
                .unwrap()
                .expanded
        );
    }

    #[test]
    fn edits_text_and_autosaves_after_the_idle_delay() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.rs");
        std::fs::write(&path, "abc\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(path.clone()).unwrap();
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::End,
            crossterm::event::KeyModifiers::NONE,
        ));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        ));
        app.editor_paste("!\nnext");
        assert_eq!(app.active_file().unwrap().lines, ["ab!", "next"]);
        assert!(app.active_file().unwrap().dirty);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "abc\n");

        app.open_files[0].make_autosave_due();
        app.tick();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ab!\nnext");
        assert!(!app.active_file().unwrap().dirty);
    }

    #[test]
    fn saves_before_switching_tabs_changing_focus_and_closing() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.txt");
        let second = directory.path().join("second.txt");
        std::fs::write(&first, "first\n").unwrap();
        std::fs::write(&second, "second\n").unwrap();
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        app.open_file(first.clone()).unwrap();
        app.editor_paste("one ");
        app.open_file(second.clone()).unwrap();
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "one first\n");

        app.editor_paste("two ");
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('j'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "two second\n");

        app.focus = Focus::Editor;
        app.editor_paste("three ");
        app.close_tab(1);
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            "two three second\n"
        );
    }
}
