//! The shortcuts that can be changed in Help (F29, Q56): the key of each command, the keys each
//! system starts with and `keys.toml` in the settings folder, which keeps the changed ones.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Overlay};

/// A command whose key can be changed, in the order Help lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Explorer,
    BottomTerminal,
    RightTerminal,
    SourceControl,
    Database,
    NextTerminal,
    PreviousTerminal,
    CloseTab,
    Run,
    Build,
    Clean,
    Rebuild,
    RunProject,
    Suggestions,
    Definition,
    Peek,
    References,
    Hover,
    Back,
    FindInFile,
    FindInWorkspace,
    Breakpoint,
    Debug,
    StepOver,
    StepInto,
    StopDebug,
    Help,
    About,
    Quit,
}

pub const ACTIONS: [Action; 29] = [
    Action::Explorer,
    Action::BottomTerminal,
    Action::RightTerminal,
    Action::SourceControl,
    Action::Database,
    Action::NextTerminal,
    Action::PreviousTerminal,
    Action::CloseTab,
    Action::Run,
    Action::Build,
    Action::Clean,
    Action::Rebuild,
    Action::RunProject,
    Action::Suggestions,
    Action::Definition,
    Action::Peek,
    Action::References,
    Action::Hover,
    Action::Back,
    Action::FindInFile,
    Action::FindInWorkspace,
    Action::Breakpoint,
    Action::Debug,
    Action::StepOver,
    Action::StepInto,
    Action::StopDebug,
    Action::Help,
    Action::About,
    Action::Quit,
];

impl Action {
    /// Its name in `keys.toml`.
    fn name(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::BottomTerminal => "bottom_terminal",
            Self::RightTerminal => "right_terminal",
            Self::SourceControl => "source_control",
            Self::Database => "database",
            Self::NextTerminal => "next_terminal",
            Self::PreviousTerminal => "previous_terminal",
            Self::CloseTab => "close_tab",
            Self::Run => "run",
            Self::Build => "build",
            Self::Clean => "clean",
            Self::Rebuild => "rebuild",
            Self::RunProject => "run_project",
            Self::Suggestions => "suggestions",
            Self::Definition => "definition",
            Self::Peek => "peek_definition",
            Self::References => "references",
            Self::Hover => "hover",
            Self::Back => "go_back",
            Self::FindInFile => "find_in_file",
            Self::FindInWorkspace => "find_in_workspace",
            Self::Breakpoint => "breakpoint",
            Self::Debug => "debug",
            Self::StepOver => "step_over",
            Self::StepInto => "step_into",
            Self::StopDebug => "stop_debugging",
            Self::Help => "help",
            Self::About => "about",
            Self::Quit => "quit",
        }
    }

    /// What it does, as Help says it.
    pub fn title(self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::BottomTerminal => "Bottom terminal",
            Self::RightTerminal => "Right terminal",
            Self::SourceControl => "Source Control",
            Self::Database => "Database",
            Self::NextTerminal => "Next terminal tab",
            Self::PreviousTerminal => "Previous terminal tab",
            Self::CloseTab => "Close the tab",
            Self::Run => "Run the file or its project",
            Self::Build => "Build the project",
            Self::Clean => "Clean the project",
            Self::Rebuild => "Clean and build the project",
            Self::RunProject => "Run the project",
            Self::Suggestions => "Suggestions (language server)",
            Self::Definition => "Go to the definition",
            Self::Peek => "Peek the definition",
            Self::References => "Find the references",
            Self::Hover => "Information on the symbol",
            Self::Back => "Back to where you were",
            Self::FindInFile => "Find in the file",
            Self::FindInWorkspace => "Find in the workspace",
            Self::Breakpoint => "Breakpoint on the line",
            Self::Debug => "Build and debug, or continue",
            Self::StepOver => "Step over",
            Self::StepInto => "Step into",
            Self::StopDebug => "Stop debugging or what runs",
            Self::Help => "Help",
            Self::About => "About",
            Self::Quit => "Quit",
        }
    }

    /// The key it has before anyone changes it (`CLAUDE.md` §5): macOS and Linux terminals
    /// send Ctrl+Shift+B as Ctrl+B, so the right terminal is Ctrl+Option+B and Ctrl+Alt+B
    /// there (E11, E18).
    fn default_binding(self, os: &str) -> Binding {
        let ctrl = |character| Binding::new(KeyCode::Char(character), KeyModifiers::CONTROL);
        let both = KeyModifiers::CONTROL
            | if matches!(os, "macos" | "linux") {
                KeyModifiers::ALT
            } else {
                KeyModifiers::SHIFT
            };
        match self {
            Self::Explorer => ctrl('b'),
            Self::BottomTerminal => ctrl('j'),
            Self::RightTerminal => Binding::new(KeyCode::Char('b'), both),
            Self::SourceControl => ctrl('g'),
            Self::Database => ctrl('d'),
            Self::NextTerminal => Binding::new(KeyCode::PageDown, KeyModifiers::CONTROL),
            Self::PreviousTerminal => Binding::new(KeyCode::PageUp, KeyModifiers::CONTROL),
            Self::CloseTab => ctrl('w'),
            Self::Run => ctrl('r'),
            Self::Build => Binding::new(KeyCode::F(7), KeyModifiers::NONE),
            Self::Clean => Binding::new(KeyCode::F(7), KeyModifiers::SHIFT),
            Self::Rebuild => Binding::new(KeyCode::F(7), KeyModifiers::CONTROL),
            Self::RunProject => Binding::new(KeyCode::F(5), KeyModifiers::CONTROL),
            Self::Suggestions => ctrl(' '),
            Self::Definition => Binding::new(KeyCode::F(12), KeyModifiers::NONE),
            Self::Peek => Binding::new(KeyCode::F(12), KeyModifiers::ALT),
            Self::References => Binding::new(KeyCode::F(12), KeyModifiers::SHIFT),
            Self::Hover => ctrl('k'),
            Self::Back => Binding::new(KeyCode::Left, KeyModifiers::ALT),
            Self::FindInFile => ctrl('p'),
            Self::FindInWorkspace => Binding::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            Self::Breakpoint => Binding::new(KeyCode::F(9), KeyModifiers::NONE),
            Self::Debug => Binding::new(KeyCode::F(5), KeyModifiers::NONE),
            Self::StepOver => Binding::new(KeyCode::F(10), KeyModifiers::NONE),
            Self::StepInto => Binding::new(KeyCode::F(11), KeyModifiers::NONE),
            Self::StopDebug => Binding::new(KeyCode::F(5), KeyModifiers::SHIFT),
            Self::Help => Binding::new(KeyCode::F(1), KeyModifiers::NONE),
            Self::About => Binding::new(KeyCode::F(2), KeyModifiers::NONE),
            Self::Quit => ctrl('q'),
        }
    }
}

/// A key with its modifiers, the same whatever the terminal sent (`from_key`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

const CHOSEN: KeyModifiers = KeyModifiers::CONTROL
    .union(KeyModifiers::ALT)
    .union(KeyModifiers::SHIFT)
    .union(KeyModifiers::SUPER);

impl Binding {
    const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    /// The key as terminals send it in different ways: a letter in upper case means Shift,
    /// another character already says its Shift, BackTab is Shift+Tab and F13 to F24 are what
    /// some terminals send for Shift+F1 to Shift+F12. Option+Left reaches the app from the
    /// Terminal.app as `ESC b` (Alt+B, a word back in a shell): it is Alt+Left (Q67).
    pub fn from_key(key: KeyEvent) -> Self {
        let mut modifiers = key.modifiers & CHOSEN;
        let code = match key.code {
            KeyCode::Char('b') if modifiers == KeyModifiers::ALT => KeyCode::Left,
            KeyCode::Char(character) if character.is_alphabetic() => {
                if character.is_uppercase() {
                    modifiers |= KeyModifiers::SHIFT;
                }
                KeyCode::Char(character.to_lowercase().next().unwrap_or(character))
            }
            KeyCode::Char(character) => {
                modifiers.remove(KeyModifiers::SHIFT);
                KeyCode::Char(character)
            }
            KeyCode::BackTab => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            KeyCode::F(number @ 13..=24) => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::F(number - 12)
            }
            code => code,
        };
        Self { code, modifiers }
    }

    /// Reads what `text` writes: "Ctrl+Alt+B", "Shift+F5", "Ctrl+Space", "Ctrl++".
    pub fn parse(text: &str) -> Option<Self> {
        let (names, key) = match text.strip_suffix('+') {
            Some("") => ("", "+"),
            Some(rest) => (rest.strip_suffix('+')?, "+"),
            None => text.rsplit_once('+').unwrap_or(("", text)),
        };
        let mut modifiers = KeyModifiers::NONE;
        for name in names.split('+').filter(|name| !name.is_empty()) {
            modifiers |= match name {
                "Ctrl" => KeyModifiers::CONTROL,
                "Alt" | "Option" => KeyModifiers::ALT,
                "Shift" => KeyModifiers::SHIFT,
                "Super" | "Cmd" => KeyModifiers::SUPER,
                _ => return None,
            };
        }
        let code = match key {
            "Space" => KeyCode::Char(' '),
            "Enter" => KeyCode::Enter,
            "Tab" => KeyCode::Tab,
            "Esc" => KeyCode::Esc,
            "Backspace" => KeyCode::Backspace,
            "Delete" => KeyCode::Delete,
            "Insert" => KeyCode::Insert,
            "Home" => KeyCode::Home,
            "End" => KeyCode::End,
            "PageUp" => KeyCode::PageUp,
            "PageDown" => KeyCode::PageDown,
            "Up" => KeyCode::Up,
            "Down" => KeyCode::Down,
            "Left" => KeyCode::Left,
            "Right" => KeyCode::Right,
            _ => match key.strip_prefix('F').and_then(|number| number.parse().ok()) {
                Some(number @ 1..=24) => KeyCode::F(number),
                _ => {
                    let mut characters = key.chars();
                    match (characters.next(), characters.next()) {
                        // Letters are written in upper case; Shift is written out.
                        (Some(character), None) => {
                            KeyCode::Char(character.to_lowercase().next().unwrap_or(character))
                        }
                        _ => return None,
                    }
                }
            },
        };
        Some(Self { code, modifiers })
    }

    /// As `keys.toml` keeps it, the same on every system: "Ctrl+Alt+B", "Shift+F5".
    pub fn text(self) -> String {
        self.label("")
    }

    /// As `os` writes it (`UX.md` §1.4): Option, Cmd and `fn` for the function and page keys on
    /// macOS.
    pub fn label(self, os: &str) -> String {
        let mac = os == "macos";
        let mut parts = Vec::new();
        for (modifier, name, mac_name) in [
            (KeyModifiers::CONTROL, "Ctrl", "Ctrl"),
            (KeyModifiers::ALT, "Alt", "Option"),
            (KeyModifiers::SHIFT, "Shift", "Shift"),
            (KeyModifiers::SUPER, "Super", "Cmd"),
        ] {
            if self.modifiers.contains(modifier) {
                parts.push(if mac { mac_name } else { name }.to_string());
            }
        }
        let fn_key = |name: &str| {
            if mac {
                format!("fn+{name}")
            } else {
                name.to_string()
            }
        };
        parts.push(match self.code {
            KeyCode::Char(' ') => "Space".into(),
            KeyCode::Char(character) => character.to_uppercase().to_string(),
            KeyCode::F(number) => fn_key(&format!("F{number}")),
            KeyCode::PageUp if mac => "fn+Up".into(),
            KeyCode::PageDown if mac => "fn+Down".into(),
            KeyCode::Home if mac => "fn+Left".into(),
            KeyCode::End if mac => "fn+Right".into(),
            KeyCode::PageUp => "PageUp".into(),
            KeyCode::PageDown => "PageDown".into(),
            KeyCode::Home => "Home".into(),
            KeyCode::End => "End".into(),
            KeyCode::Enter => "Enter".into(),
            KeyCode::Tab => "Tab".into(),
            KeyCode::Esc => "Esc".into(),
            KeyCode::Backspace => "Backspace".into(),
            KeyCode::Delete => "Delete".into(),
            KeyCode::Insert => "Insert".into(),
            KeyCode::Up => "Up".into(),
            KeyCode::Down => "Down".into(),
            KeyCode::Left => "Left".into(),
            KeyCode::Right => "Right".into(),
            other => format!("{other:?}"),
        });
        parts.join("+")
    }

    /// Why this key cannot be a shortcut on `os`: keys that type, or that edit text, stay
    /// theirs (Q56).
    pub fn refusal(self, os: &str) -> Option<String> {
        let label = self.label(os);
        let command = self
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        if !command && !matches!(self.code, KeyCode::F(_)) {
            let alt = if os == "macos" { "Option" } else { "Alt" };
            return Some(format!(
                "{label} types or moves: hold Ctrl or {alt} with a key, or use a function key"
            ));
        }
        let edit = match self.code {
            _ if self.modifiers != KeyModifiers::CONTROL => None,
            KeyCode::Char('a') => Some("select all"),
            KeyCode::Char('c') => Some("copy"),
            KeyCode::Char('v') => Some("paste"),
            KeyCode::Char('x') => Some("cut"),
            KeyCode::Char('z') => Some("undo"),
            _ => None,
        };
        edit.map(|what| format!("{label} stays {what}"))
    }
}

/// The key of every command in `ACTIONS`, on one system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    os: &'static str,
    bindings: [Binding; ACTIONS.len()],
}

impl Keymap {
    pub fn defaults(os: &'static str) -> Self {
        Self {
            os,
            bindings: ACTIONS.map(|action| action.default_binding(os)),
        }
    }

    /// The keys `os` starts with and, over them, the ones changed in `path`; a command or a
    /// key the file does not know keeps its default.
    pub fn load(path: Option<&Path>, os: &'static str) -> Self {
        let mut keymap = Self::defaults(os);
        let changed = path
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str::<BTreeMap<String, String>>(&text).ok())
            .unwrap_or_default();
        for (index, action) in ACTIONS.iter().enumerate() {
            if let Some(binding) = changed
                .get(action.name())
                .and_then(|text| Binding::parse(text))
            {
                keymap.bindings[index] = binding;
            }
        }
        keymap
    }

    /// Writes the keys that differ from the defaults; with none, the file goes.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let changed = ACTIONS
            .iter()
            .filter(|action| self.binding(**action) != action.default_binding(self.os))
            .map(|action| (action.name(), self.binding(*action).text()))
            .collect::<BTreeMap<_, _>>();
        if changed.is_empty() {
            return match fs::remove_file(path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let text = toml::to_string(&changed).map_err(std::io::Error::other)?;
        fs::write(path, text)
    }

    fn index(action: Action) -> usize {
        ACTIONS
            .iter()
            .position(|known| *known == action)
            .unwrap_or_default()
    }

    pub fn binding(&self, action: Action) -> Binding {
        self.bindings[Self::index(action)]
    }

    /// The key of `action` as this system writes it.
    pub fn label(&self, action: Action) -> String {
        self.binding(action).label(self.os)
    }

    /// The command `key` runs, if any.
    pub fn action(&self, key: KeyEvent) -> Option<Action> {
        let pressed = Binding::from_key(key);
        ACTIONS
            .iter()
            .zip(self.bindings)
            .find(|(_, binding)| *binding == pressed)
            .map(|(action, _)| *action)
    }

    /// Whether `key` runs `action`.
    pub fn is(&self, action: Action, key: KeyEvent) -> bool {
        self.binding(action) == Binding::from_key(key)
    }

    /// Gives `binding` to `action`. The command that had it takes the key `action` had, so
    /// that every command keeps a key; that command and its new key come back.
    pub fn set(&mut self, action: Action, binding: Binding) -> Option<(Action, Binding)> {
        let index = Self::index(action);
        let old = self.bindings[index];
        let other = self
            .bindings
            .iter()
            .position(|taken| *taken == binding)
            .filter(|other| *other != index);
        self.bindings[index] = binding;
        other.map(|other| {
            self.bindings[other] = old;
            (ACTIONS[other], old)
        })
    }

    pub fn reset(&mut self) {
        *self = Self::defaults(self.os);
    }

    pub fn os(&self) -> &'static str {
        self.os
    }
}

/// Where the changed keys are kept. Debug builds (tests, `cargo run`) neither read nor write
/// them, like the sessions; the tests set their own file.
pub(super) fn keys_path() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    directories::ProjectDirs::from("", "", "SozoIDE")
        .map(|directories| directories.config_dir().join("keys.toml"))
}

/// What Help is doing with the shortcuts (F29): the command chosen with Tab or the mouse,
/// whether it waits for its new keys or for Enter to reset them all, and what happened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyEditor {
    pub selected: Option<Action>,
    pub capturing: bool,
    pub confirm_reset: bool,
    pub message: Option<String>,
    /// The next frame of Help scrolls to `selected`.
    pub follow: bool,
}

impl App {
    /// Help's own keys: Tab and BackTab choose a command, Enter waits for its new keys and R
    /// asks to reset them all. False for the keys Help leaves to scrolling and closing.
    pub(super) fn handle_shortcut_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let forward =
                    key.code == KeyCode::Tab && !key.modifiers.contains(KeyModifiers::SHIFT);
                let position = self
                    .key_editor
                    .selected
                    .and_then(|action| ACTIONS.iter().position(|known| *known == action));
                let count = ACTIONS.len();
                let next = match position {
                    Some(position) if forward => (position + 1) % count,
                    Some(position) => (position + count - 1) % count,
                    None if forward => 0,
                    None => count - 1,
                };
                self.key_editor.selected = Some(ACTIONS[next]);
                self.key_editor.follow = true;
                self.key_editor.message = None;
            }
            KeyCode::Enter => {
                let action = self.key_editor.selected.unwrap_or(ACTIONS[0]);
                self.choose_shortcut(action);
            }
            KeyCode::Char('r' | 'R')
                if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() =>
            {
                self.ask_to_reset_shortcuts();
            }
            _ => return false,
        }
        true
    }

    /// A click on a command's key, or Enter on it: Help waits for its new keys.
    pub(super) fn choose_shortcut(&mut self, action: Action) {
        self.key_editor = KeyEditor {
            selected: Some(action),
            capturing: true,
            follow: true,
            ..KeyEditor::default()
        };
    }

    pub(super) fn ask_to_reset_shortcuts(&mut self) {
        self.key_editor.capturing = false;
        self.key_editor.confirm_reset = true;
        self.key_editor.message = None;
    }

    /// The key that comes while Help waits: the new keys of the command, or Enter to reset
    /// them all; Esc gives up.
    pub(super) fn edit_shortcut(&mut self, key: KeyEvent) {
        let editor = &mut self.key_editor;
        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            editor.capturing = false;
            editor.confirm_reset = false;
            editor.message = None;
            return;
        }
        if editor.confirm_reset {
            editor.confirm_reset = false;
            if key.code == KeyCode::Enter {
                self.keys.reset();
                self.key_editor.message =
                    Some(self.save_keys("Every shortcut is back to the default"));
            }
            return;
        }
        let Some(action) = editor.selected.filter(|_| editor.capturing) else {
            return;
        };
        let binding = Binding::from_key(key);
        let os = self.keys.os();
        if let Some(refusal) = binding.refusal(os) {
            editor.message = Some(refusal);
            return;
        }
        editor.capturing = false;
        let label = binding.label(os);
        let done = if self.keys.binding(action) == binding {
            format!("{} is already {label}", action.title())
        } else {
            match self.keys.set(action, binding) {
                Some((other, old)) => format!(
                    "{}: {label}; {} took {}",
                    action.title(),
                    other.title(),
                    old.label(os)
                ),
                None => format!("{}: {label}", action.title()),
            }
        };
        self.key_editor.message = Some(self.save_keys(&done));
    }

    /// Keeps the keys in `keys.toml` and says `done`, or why they could not be kept.
    fn save_keys(&self, done: &str) -> String {
        match self.keys_path.as_deref().map(|path| self.keys.save(path)) {
            Some(Err(error)) => format!("{done}, but it could not be kept: {error}"),
            _ => done.to_string(),
        }
    }

    /// Help opens with nothing chosen.
    pub(super) fn reset_key_editor(&mut self, overlay: Overlay) {
        if overlay == Overlay::Help {
            self.key_editor = KeyEditor::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn keys_are_written_and_read_the_way_each_system_types_them() {
        let control = KeyModifiers::CONTROL;
        let right = Action::RightTerminal;
        assert_eq!(Keymap::defaults("macos").label(right), "Ctrl+Option+B");
        assert_eq!(Keymap::defaults("linux").label(right), "Ctrl+Alt+B");
        assert_eq!(Keymap::defaults("windows").label(right), "Ctrl+Shift+B");
        let mac = Keymap::defaults("macos");
        assert_eq!(mac.label(Action::Help), "fn+F1");
        assert_eq!(mac.label(Action::StopDebug), "Shift+fn+F5");
        assert_eq!(mac.label(Action::NextTerminal), "Ctrl+fn+Down");
        assert_eq!(mac.label(Action::Suggestions), "Ctrl+Space");
        let windows = Keymap::defaults("windows");
        assert_eq!(windows.label(Action::PreviousTerminal), "Ctrl+PageUp");
        assert_eq!(windows.label(Action::FindInWorkspace), "Ctrl+Shift+P");
        // keys.toml reads what it writes, whatever the system.
        for text in [
            "Ctrl+Alt+B",
            "Shift+F5",
            "Ctrl+Space",
            "Ctrl++",
            "+",
            "Ctrl+Shift+Super+K",
            "Alt+PageDown",
            "F24",
        ] {
            assert_eq!(Binding::parse(text).unwrap().text(), text);
        }
        assert_eq!(
            Binding::parse("Option+Cmd+K"),
            Some(Binding::new(
                KeyCode::Char('k'),
                KeyModifiers::ALT | KeyModifiers::SUPER
            ))
        );
        assert_eq!(Binding::parse("Ctrl+Hyper+K"), None);
        assert_eq!(Binding::parse("Ctrl+Nothing"), None);
        assert_eq!(Binding::parse("Ctrl+F99"), None);
        assert_eq!(
            Binding::parse("Ctrl+B"),
            Some(Binding::new(KeyCode::Char('b'), control))
        );
    }

    #[test]
    fn keys_are_known_whatever_the_terminal_sends() {
        let keys = Keymap::defaults("windows");
        let control = KeyModifiers::CONTROL;
        let shift = KeyModifiers::SHIFT;
        // Ctrl+Shift+B comes as an upper case B, with or without Shift.
        for pressed in [
            key(KeyCode::Char('B'), control | shift),
            key(KeyCode::Char('b'), control | shift),
            key(KeyCode::Char('B'), control),
        ] {
            assert_eq!(keys.action(pressed), Some(Action::RightTerminal));
        }
        assert_eq!(
            keys.action(key(KeyCode::Char('b'), control)),
            Some(Action::Explorer)
        );
        // Some terminals send Shift+F5 as F17.
        assert!(keys.is(Action::StopDebug, key(KeyCode::F(17), KeyModifiers::NONE)));
        assert!(keys.is(Action::StopDebug, key(KeyCode::F(5), shift)));
        assert!(!keys.is(Action::Debug, key(KeyCode::F(5), shift)));
        // Ctrl+Space arrives as a space with Ctrl; a symbol says its own Shift.
        assert_eq!(
            keys.action(key(KeyCode::Char(' '), control)),
            Some(Action::Suggestions)
        );
        assert_eq!(
            Binding::from_key(key(KeyCode::Char('+'), control | shift)),
            Binding::new(KeyCode::Char('+'), control)
        );
        assert_eq!(
            Binding::from_key(key(KeyCode::BackTab, shift)),
            Binding::new(KeyCode::Tab, shift)
        );
        // A key with Alt is not the key without it.
        assert_eq!(
            keys.action(key(KeyCode::Char('b'), control | KeyModifiers::ALT)),
            None
        );
        let mac = Keymap::defaults("macos");
        assert_eq!(
            mac.action(key(KeyCode::Char('b'), control | KeyModifiers::ALT)),
            Some(Action::RightTerminal)
        );
    }

    #[test]
    fn a_key_of_another_command_swaps_and_only_changed_keys_are_kept() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings/keys.toml");
        let mut keys = Keymap::defaults("linux");
        let ctrl = |character| Binding::new(KeyCode::Char(character), KeyModifiers::CONTROL);
        assert_eq!(keys.set(Action::Explorer, ctrl('e')), None);
        // Ctrl+G was Source Control: it takes Ctrl+E, the key the Explorer had.
        assert_eq!(
            keys.set(Action::Explorer, ctrl('g')),
            Some((Action::SourceControl, ctrl('e')))
        );
        assert_eq!(keys.label(Action::Explorer), "Ctrl+G");
        assert_eq!(keys.label(Action::SourceControl), "Ctrl+E");
        keys.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("explorer = \"Ctrl+G\""), "{text}");
        assert!(text.contains("source_control = \"Ctrl+E\""), "{text}");
        assert!(!text.contains("database"), "{text}");
        assert_eq!(Keymap::load(Some(&path), "linux"), keys);
        // A file written by hand: unknown names and keys keep the defaults.
        fs::write(
            &path,
            "explorer = \"Ctrl+Nope\"\nmystery = \"Ctrl+K\"\nquit = \"Alt+Q\"\n",
        )
        .unwrap();
        let read = Keymap::load(Some(&path), "linux");
        assert_eq!(read.label(Action::Explorer), "Ctrl+B");
        assert_eq!(read.label(Action::Quit), "Alt+Q");
        assert_eq!(Keymap::load(None, "linux"), Keymap::defaults("linux"));
        // Back to the defaults: nothing left to keep, so the file goes.
        keys.reset();
        assert_eq!(keys, Keymap::defaults("linux"));
        keys.save(&path).unwrap();
        assert!(!path.exists());
        keys.save(&path).unwrap();
    }

    #[test]
    fn keys_that_type_or_edit_text_are_refused() {
        let refused = |code, modifiers| Binding::new(code, modifiers).refusal("linux");
        assert!(refused(KeyCode::Char('e'), KeyModifiers::NONE).is_some());
        assert!(refused(KeyCode::Char('e'), KeyModifiers::SHIFT).is_some());
        assert!(refused(KeyCode::Enter, KeyModifiers::NONE).is_some());
        assert!(refused(KeyCode::Down, KeyModifiers::SHIFT).is_some());
        assert_eq!(
            refused(KeyCode::Char('c'), KeyModifiers::CONTROL).as_deref(),
            Some("Ctrl+C stays copy")
        );
        assert!(refused(KeyCode::Char('z'), KeyModifiers::CONTROL).is_some());
        // With another modifier they are new keys.
        assert_eq!(
            refused(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            None
        );
        assert_eq!(refused(KeyCode::Char('e'), KeyModifiers::ALT), None);
        assert_eq!(refused(KeyCode::F(7), KeyModifiers::NONE), None);
        assert_eq!(refused(KeyCode::Char('k'), KeyModifiers::SUPER), None);
        assert!(
            Binding::new(KeyCode::Char('e'), KeyModifiers::NONE)
                .refusal("macos")
                .unwrap()
                .contains("Option")
        );
    }
}
