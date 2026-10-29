use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::Receiver,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result};
use ratatui_image::picker::Picker;

use crate::{
    database::controller::DatabaseController,
    editor::{OpenFile, SearchState},
    git::controller::GitController,
    terminal::Terminal,
    update::{Notes, UpdateState, Updater, Welcome, notes_since},
};

mod explorer;
mod input;
mod layout;
mod motion;

pub use explorer::{EntryAction, EntryMenu, FileEntry, NamePrompt};
use layout::{DragTarget, TabDrag, inner_size};
pub use layout::{DropZone, Focus, Overlay, Regions, Split, SplitDirection, TabBar};
pub(crate) use motion::AppMotion;

/// How often the open files and the Explorer are compared with the disk.
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// Pages of the first-start tutorial (`ui::overlays` writes them).
pub const TUTORIAL_STEPS: usize = 6;

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
    /// The Explorer's right-click menu and the name it asks for (F01).
    pub entry_menu: Option<EntryMenu>,
    pub name_prompt: Option<NamePrompt>,
    pub splash_visible: bool,
    pub motion: AppMotion,
    pub updater: Updater,
    /// The tutorial or what's new, shown once the splash is gone (F17, F18).
    pub welcome: Welcome,
    pub tutorial_step: usize,
    /// The notes What's new lists: every version since the one that ran before.
    pub notes: Vec<&'static Notes>,
    /// The first row of a dialog taller than the screen (Help, What's new).
    pub overlay_scroll: u16,
    /// Text copied in the editor, for `main` to hand to the terminal (OSC 52).
    pub clipboard: Option<String>,
    /// The message of the installation of git, curl or ffmpeg running in the background (F24).
    dependencies: Option<Receiver<String>>,
    /// The last text copied here, for Ctrl+V when the system clipboard cannot be read.
    copied: Option<String>,
    /// The file or folder Copy or Cut (true) kept in the Explorer, for Paste.
    pub file_clipboard: Option<(PathBuf, bool)>,
    /// When the open files and the folders on screen were last compared with the disk, and the
    /// modification times of those folders.
    watched: Instant,
    folders: Vec<Option<SystemTime>>,
    /// A mouse drag that started in the editor text selects.
    selecting: bool,
    /// Where a drag over a 3D model was last: moving from there turns it.
    turning: Option<(u16, u16)>,
    /// The last time the wheel turned a PDF page: one page per gesture, not per event.
    page_turned: Option<Instant>,
    pub split: Option<Split>,
    pub tab_bars: [TabBar; 2],
    tab_drag: Option<TabDrag>,
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
        let updater = Updater::new(crate::update::enabled());
        // Debug builds (tests, `cargo run`) never read or write the settings, like the updater.
        let welcome = if splash && !cfg!(debug_assertions) {
            updater.welcome()
        } else {
            Welcome::None
        };
        // A new installation or a new version installs the tools it is missing (Q41).
        let dependencies =
            (welcome != Welcome::None).then(crate::update::deps::install_in_background);
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
            entry_menu: None,
            name_prompt: None,
            splash_visible: splash,
            motion: AppMotion::new(now, splash),
            updater,
            welcome,
            tutorial_step: 0,
            notes: Vec::new(),
            overlay_scroll: 0,
            clipboard: None,
            dependencies,
            copied: None,
            file_clipboard: None,
            watched: now,
            folders: Vec::new(),
            selecting: false,
            turning: None,
            page_turned: None,
            split: None,
            tab_bars: [TabBar::default(); 2],
            tab_drag: None,
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
        if let Some(search) = &mut self.search {
            search.poll();
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
        // GIFs and videos move on, also in the pane that is not focused.
        for media in self
            .open_files
            .iter_mut()
            .filter_map(|file| file.media.as_mut())
        {
            media.tick(now);
        }
        if now.duration_since(self.watched) >= WATCH_INTERVAL {
            self.watched = now;
            self.watch_disk();
        }
        if let Some(message) = self
            .dependencies
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.status = message;
            self.dependencies = None;
        }
        self.database.tick_at(now);
        // Copy in the SQL tab: the app owns the clipboard.
        if let Some(text) = self.database.copy_request.take() {
            self.copy(text);
        }
        self.git.tick_at(now);
        // The Diff tab asked for its file in the editor.
        if let Some(path) = self.git.open_request.take() {
            match self.open_file(path) {
                Ok(()) => self.show_git = false,
                Err(error) => self.status = error.to_string(),
            }
        }
        self.tick_update();
    }

    /// Follows what other programs do to the project: open files read again (unless they have
    /// edits not saved yet, which stay, Q40) and the Explorer when a folder on screen changes.
    fn watch_disk(&mut self) {
        for file in &mut self.open_files {
            if !file.changed_on_disk() {
                continue;
            }
            self.status = if file.dirty {
                file.keep_edits();
                format!("{} changed on disk: your edits are kept", file.name)
            } else {
                match file.reload() {
                    Ok(()) => format!("Reloaded {}", file.name),
                    Err(error) => error.to_string(),
                }
            };
        }
        if self.folder_times() != self.folders
            && let Err(error) = self.refresh_entries()
        {
            self.status = error.to_string();
        }
    }

    /// The modification times of the project folder and the folders open in the Explorer:
    /// they change when something in them is created, renamed or deleted.
    fn folder_times(&self) -> Vec<Option<SystemTime>> {
        std::iter::once(&self.root)
            .chain(
                self.entries
                    .iter()
                    .filter(|entry| entry.expanded)
                    .map(|entry| &entry.path),
            )
            .map(|path| {
                std::fs::metadata(path)
                    .and_then(|data| data.modified())
                    .ok()
            })
            .collect()
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
        let free = self.overlay == Overlay::None && !self.splash_visible && self.search.is_none();
        if free && self.welcome != Welcome::None {
            let overlay = match std::mem::replace(&mut self.welcome, Welcome::None) {
                Welcome::WhatsNew(seen) => {
                    self.notes = notes_since(&seen);
                    Overlay::WhatsNew
                }
                _ => Overlay::Tutorial,
            };
            self.tutorial_step = 0;
            self.open_overlay(overlay);
            return;
        }
        match self.updater.state {
            UpdateState::Available if free => {
                self.open_overlay(Overlay::Update);
            }
            UpdateState::Ready(_) => {
                self.flush_all_files();
                self.should_quit = true;
            }
            _ => {}
        }
    }

    /// Opens the repository in the browser, to contribute (About).
    pub fn open_repository(&mut self) {
        self.status = match opener(std::env::consts::OS, crate::branding::REPOSITORY)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => format!("Opened {}", crate::branding::REPOSITORY),
            Err(error) => format!("Could not open the browser: {error}"),
        };
    }

    /// Copies `text` to the system clipboard: the tool of this system now and, from `main`,
    /// the terminal (OSC 52), which also reaches it over SSH.
    pub(super) fn copy(&mut self, text: String) {
        if !cfg!(test) {
            system_clipboard(text.clone());
        }
        self.status = format!("Copied {} characters", text.chars().count());
        self.copied = Some(text.clone());
        self.clipboard = Some(text);
    }

    /// Ctrl+R (F25): saves everything and types the command that runs `file` into the bottom
    /// terminal, which opens and takes the focus.
    pub(super) fn run_file(&mut self, file: PathBuf) {
        self.flush_all_files();
        let scratch = std::env::temp_dir().join("sozo-run");
        let command = crate::run::command(
            &file,
            &self.root,
            std::env::consts::OS,
            crate::update::deps::on_path,
            &scratch,
        );
        match command {
            Ok(line) => {
                self.show_bottom_terminal = true;
                self.focus = Focus::BottomTerminal;
                self.bottom_terminal.send_text(&format!("{line}\r"));
                self.status = format!("Running {}", file.display());
            }
            Err(error) => self.status = error,
        }
    }

    /// Ctrl+V and Paste in the editor: the system clipboard, or the last text copied here.
    pub(super) fn paste_clipboard(&mut self) {
        let text = if cfg!(test) {
            None
        } else {
            system_clipboard_text()
        }
        .or_else(|| self.copied.clone());
        match text {
            Some(text) => self.editor_paste(&text),
            None => self.status = "The clipboard is empty".into(),
        }
    }

    /// The editor's right-click menu: Cut and Copy with a selection, Paste always (F02).
    pub(super) fn open_editor_menu(&mut self, position: (u16, u16)) {
        let Some(file) = self.active_tab.and_then(|index| self.open_files.get(index)) else {
            return;
        };
        let mut actions = Vec::new();
        if file.selection().is_some() {
            actions.extend([EntryAction::Cut, EntryAction::Copy]);
        }
        actions.push(EntryAction::Paste);
        self.entry_menu = Some(EntryMenu {
            target: file.path.clone(),
            actions,
            position,
            selected: 0,
            editor: true,
        });
    }

    /// Cut, Copy or Paste from the editor's menu.
    pub(super) fn editor_action(&mut self, action: EntryAction) {
        let Some(file) = self
            .active_tab
            .and_then(|index| self.open_files.get_mut(index))
        else {
            return;
        };
        let text = match action {
            EntryAction::Copy => file.selected_text(),
            EntryAction::Cut => {
                let text = file.selected_text();
                file.delete_selection();
                text
            }
            EntryAction::Paste => return self.paste_clipboard(),
            _ => None,
        };
        if let Some(text) = text {
            self.copy(text);
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
                self.select_tab(index);
            }
            self.focus = Focus::Editor;
            return Ok(());
        }
        let mut file = OpenFile::load(path, &mut self.picker)?;
        file.pane = self.active_pane();
        self.flush_active_file();
        self.status = format!("Opened {}", file.name);
        self.open_files.push(file);
        self.active_tab = Some(self.open_files.len() - 1);
        self.focus = Focus::Editor;
        Ok(())
    }

    /// The pane with the focus: the one that shows the active tab.
    pub fn active_pane(&self) -> usize {
        self.active_tab
            .and_then(|index| self.open_files.get(index))
            .map_or(0, |file| file.pane)
    }

    /// The tabs of `pane`, in the order they were opened.
    pub fn pane_tabs(&self, pane: usize) -> Vec<usize> {
        (0..self.open_files.len())
            .filter(|index| self.open_files[*index].pane == pane)
            .collect()
    }

    /// The tab on screen in `pane`.
    pub fn shown_tab(&self, pane: usize) -> Option<usize> {
        if pane == self.active_pane() {
            self.active_tab
        } else {
            self.split.map(|split| split.other)
        }
    }

    /// Shows tab `index` in its pane and gives that pane the focus.
    pub fn select_tab(&mut self, index: usize) {
        let pane = |tab: usize| self.open_files.get(tab).map(|file| file.pane);
        if let Some(active) = self.active_tab
            && pane(active) != pane(index)
            && let Some(split) = &mut self.split
        {
            split.other = active;
        }
        self.active_tab = Some(index);
    }

    /// The zone a dragged tab would land in, for the preview.
    pub fn tab_drop_zone(&self) -> Option<DropZone> {
        self.tab_drag.and_then(|drag| drag.zone)
    }

    /// An edge splits the editor: the tab goes to a new pane on that side and the other tabs
    /// stay in the first one. Once split, a pane takes the tab.
    fn drop_tab(&mut self, drag: TabDrag, zone: DropZone) {
        let tab = drag.tab;
        if tab >= self.open_files.len() {
            return;
        }
        let direction = match zone {
            DropZone::Center => return self.select_tab(tab),
            DropZone::Pane(pane) => return self.move_tab(tab, pane, drag.previous),
            DropZone::Left | DropZone::Right => SplitDirection::Columns,
            DropZone::Top | DropZone::Bottom => SplitDirection::Rows,
        };
        if self.split.is_some() || self.open_files.len() < 2 {
            return;
        }
        let pane = usize::from(matches!(zone, DropZone::Right | DropZone::Bottom));
        for (index, file) in self.open_files.iter_mut().enumerate() {
            file.pane = if index == tab { pane } else { 1 - pane };
        }
        let other = drag
            .previous
            .filter(|previous| *previous != tab && *previous < self.open_files.len())
            .unwrap_or(usize::from(tab == 0));
        self.split = Some(Split { direction, other });
        self.active_tab = Some(tab);
        self.focus = Focus::Editor;
    }

    /// Moves `tab` to `pane`; the pane it leaves shows another of its tabs, or the editor
    /// goes back to one pane when it has none left.
    fn move_tab(&mut self, tab: usize, pane: usize, previous: Option<usize>) {
        let from = self.open_files[tab].pane;
        if from == pane {
            return;
        }
        self.open_files[tab].pane = pane;
        let left = self.pane_tabs(from);
        match (left.first(), &mut self.split) {
            (Some(first), Some(split)) => {
                split.other = previous.filter(|tab| left.contains(tab)).unwrap_or(*first);
            }
            _ => self.join_panes(),
        }
        self.active_tab = Some(tab);
        self.focus = Focus::Editor;
    }

    fn join_panes(&mut self) {
        self.split = None;
        for file in &mut self.open_files {
            file.pane = 0;
        }
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.open_files.len() {
            return;
        }
        self.save_file(index);
        let pane = self.open_files[index].pane;
        let was_active = self.active_tab == Some(index);
        self.open_files.remove(index);
        let shift = |tab: usize| if tab > index { tab - 1 } else { tab };
        // The tab that takes the place of the closed one in its pane.
        let tabs = self.pane_tabs(pane);
        let neighbour = tabs
            .iter()
            .copied()
            .find(|tab| *tab >= index)
            .or(tabs.last().copied());
        if let Some(mut split) = self.split.take() {
            // A pane without tabs closes: the other one fills the editor.
            if tabs.is_empty() {
                self.join_panes();
                self.active_tab = if was_active {
                    Some(shift(split.other))
                } else {
                    self.active_tab.map(shift)
                };
                return;
            }
            split.other = if split.other == index {
                neighbour.unwrap_or(0)
            } else {
                shift(split.other)
            };
            self.split = Some(split);
        }
        self.active_tab = match self.active_tab {
            _ if self.open_files.is_empty() => None,
            Some(_) if was_active => neighbour,
            active => active.map(shift),
        };
    }

    pub fn editor_paste(&mut self, text: &str) {
        if self.focus != Focus::Editor
            || self
                .active_tab
                .and_then(|index| self.open_files.get(index))
                .is_none_or(|file| !file.is_text())
        {
            return;
        }
        if let Some(active) = self.active_tab {
            self.open_files[active].insert_str(text);
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

/// The terminal sequence that puts `text` on the clipboard (OSC 52).
pub fn clipboard_sequence(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |bits, (index, byte)| {
            bits | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            encoded.push(if index <= chunk.len() {
                char::from(ALPHABET[(bits >> (18 - 6 * index) & 63) as usize])
            } else {
                '='
            });
        }
    }
    format!("\x1b]52;c;{encoded}\x07")
}

/// The text on the clipboard of this system, from its tool (Wayland, then X11, on Linux).
fn system_clipboard_text() -> Option<String> {
    let tools: &[&[&str]] = match std::env::consts::OS {
        "macos" => &[&["pbpaste"]],
        "windows" => &[&[
            "powershell",
            "-NoProfile",
            "-Command",
            "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-Clipboard -Raw",
        ]],
        _ => &[
            &["wl-paste", "--no-newline"],
            &["xclip", "-selection", "clipboard", "-o"],
            &["xsel", "--clipboard", "--output"],
        ],
    };
    tools.iter().find_map(|tool| {
        let output = Command::new(tool[0])
            .args(&tool[1..])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        // PowerShell ends what it prints with a line break of its own.
        let text = if cfg!(windows) {
            text.strip_suffix("\r\n").map(str::to_owned).unwrap_or(text)
        } else {
            text
        };
        (!text.is_empty()).then_some(text)
    })
}

/// Hands `text` to the clipboard tool of this system, trying the next one when a tool is
/// missing (Wayland, then X11, on Linux).
fn system_clipboard(text: String) {
    let tools: &[&[&str]] = match std::env::consts::OS {
        "macos" => &[&["pbcopy"]],
        "windows" => &[&["clip"]],
        _ => &[
            &["wl-copy"],
            &["xclip", "-selection", "clipboard"],
            &["xsel", "--clipboard", "--input"],
        ],
    };
    // `clip` reads UTF-16 when the text starts with its byte order mark.
    let bytes = if cfg!(windows) {
        [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect()
    } else {
        text.into_bytes()
    };
    std::thread::spawn(move || {
        for tool in tools {
            let Ok(mut child) = Command::new(tool[0])
                .args(&tool[1..])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&bytes);
            }
            if child.wait().is_ok_and(|status| status.success()) {
                break;
            }
        }
    });
}

/// The command that opens a link in the default browser of `os`.
fn opener(os: &str, url: &str) -> Command {
    let mut command = match os {
        "macos" => Command::new("open"),
        "windows" => {
            let mut command = Command::new("rundll32");
            command.arg("url.dll,FileProtocolHandler");
            command
        }
        _ => Command::new("xdg-open"),
    };
    command.arg(url);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_links_with_the_browser_of_each_system() {
        let program = |os| {
            let command = opener(os, "https://example.com");
            let arguments = command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            (
                command.get_program().to_string_lossy().into_owned(),
                arguments,
            )
        };
        assert_eq!(program("macos").0, "open");
        assert_eq!(program("linux").0, "xdg-open");
        let (windows, arguments) = program("windows");
        assert_eq!(windows, "rundll32");
        assert_eq!(arguments.last().unwrap(), "https://example.com");
    }

    #[test]
    fn copied_text_reaches_the_terminal_in_base64() {
        assert_eq!(clipboard_sequence("hello"), "\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(clipboard_sequence("é\n"), "\x1b]52;c;w6kK\x07");
        assert_eq!(clipboard_sequence(""), "\x1b]52;c;\x07");
    }

    #[test]
    fn dragging_a_tab_to_an_edge_splits_and_closing_joins() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(directory.path().join(name), name).unwrap();
        }
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            app.open_file(directory.path().join(name)).unwrap();
        }
        let drag = |tab, previous| TabDrag {
            tab,
            previous,
            zone: None,
        };
        // c.txt was on screen; a.txt goes to a pane of its own on the right.
        app.drop_tab(drag(0, Some(2)), DropZone::Right);
        let split = app.split.unwrap();
        assert_eq!(split.direction, SplitDirection::Columns);
        assert_eq!((app.active_tab, split.other), (Some(0), 2));
        assert_eq!((app.pane_tabs(0), app.pane_tabs(1)), (vec![1, 2], vec![0]));

        // Choosing the tab of the other pane focuses that pane; the panes stay where they are.
        app.select_tab(2);
        assert_eq!((app.active_tab, app.split.unwrap().other), (Some(2), 0));
        assert_eq!(app.active_pane(), 0);
        // A file opened now goes to the pane with the focus.
        std::fs::write(directory.path().join("d.txt"), "d").unwrap();
        app.open_file(directory.path().join("d.txt")).unwrap();
        assert_eq!(app.pane_tabs(0), vec![1, 2, 3]);
        app.close_tab(3);

        // Closing a tab of a pane with more tabs keeps the split, with the indexes moved.
        app.close_tab(1);
        assert_eq!((app.active_tab, app.split.unwrap().other), (Some(1), 0));
        // Closing the last tab of a pane joins the editor again.
        app.close_tab(1);
        assert_eq!((app.active_tab, app.split), (Some(0), None));

        app.open_file(directory.path().join("b.txt")).unwrap();
        app.drop_tab(drag(1, Some(0)), DropZone::Bottom);
        assert_eq!(app.split.unwrap().direction, SplitDirection::Rows);
        assert_eq!((app.pane_tabs(0), app.pane_tabs(1)), (vec![0], vec![1]));
        // Moving a tab to the other pane leaves its pane empty: one editor again.
        app.drop_tab(drag(1, Some(0)), DropZone::Pane(0));
        assert_eq!((app.active_tab, app.split), (Some(1), None));
        assert_eq!(app.pane_tabs(0), vec![0, 1]);
        assert_eq!(
            DropZone::at(ratatui::layout::Rect::new(10, 10, 100, 40), (12, 30)),
            DropZone::Left
        );
    }

    #[test]
    fn a_pane_keeps_the_tabs_it_does_not_give_away() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(directory.path().join(name), name).unwrap();
        }
        let mut app = App::new(directory.path().to_path_buf()).unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            app.open_file(directory.path().join(name)).unwrap();
        }
        let drag = |tab, previous| TabDrag {
            tab,
            previous,
            zone: None,
        };
        app.drop_tab(drag(2, Some(2)), DropZone::Left);
        assert_eq!((app.pane_tabs(0), app.pane_tabs(1)), (vec![2], vec![0, 1]));
        // b.txt joins c.txt on the left; a.txt stays alone on the right.
        app.drop_tab(drag(1, Some(2)), DropZone::Pane(0));
        assert_eq!((app.pane_tabs(0), app.pane_tabs(1)), (vec![1, 2], vec![0]));
        assert_eq!((app.active_tab, app.split.unwrap().other), (Some(1), 0));
        assert_eq!(app.shown_tab(1), Some(0));
    }

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
