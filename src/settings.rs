//! `settings.toml` in the configuration folder (F42, TODO 2.1.9): what a user may want
//! different from the defaults. The `SOZO_*` variables still win; a broken file is kept aside
//! and the defaults are used, and SozoIDE says so.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Seconds after the last key before a file is saved; 0 turns autosave off.
    pub autosave_seconds: u64,
    /// The indentation of a file that does not say (by its lines or `.editorconfig`).
    pub tab_width: usize,
    pub insert_spaces: bool,
    /// Long lines wrap on screen; Markdown has its own (Q94: on).
    pub wrap: bool,
    pub wrap_markdown: bool,
    /// Every transition becomes a cut, as with `SOZO_REDUCED_MOTION`.
    pub reduced_motion: bool,
    /// Look for a new version at startup (`SOZO_NO_UPDATE_CHECK` turns it off).
    pub check_updates: bool,
    /// Count an installation or an update once, anonymously (`SOZO_NO_TELEMETRY`).
    pub telemetry: bool,
    /// The language server formats a file each time it is saved (TODO 3.1.2).
    pub format_on_save: bool,
    /// A query of the Database stops after this many seconds; 0 lets it run (TODO 4.1.2).
    pub query_timeout_seconds: u64,
    /// Rows of history each terminal keeps (TODO 5.1.4).
    pub terminal_scrollback: usize,
    /// Attributes only, no colours, as `NO_COLOR` does (TODO 5.2.7).
    pub high_contrast: bool,
    /// `stable`, or `preview` for the pre-releases too (TODO 6.1.2).
    pub update_channel: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autosave_seconds: 2,
            tab_width: 4,
            insert_spaces: true,
            wrap: false,
            wrap_markdown: true,
            reduced_motion: false,
            check_updates: true,
            telemetry: true,
            format_on_save: false,
            query_timeout_seconds: 60,
            terminal_scrollback: 10_000,
            high_contrast: false,
            update_channel: "stable".into(),
        }
    }
}

/// What a new `settings.toml` says: every setting with its default, so it is easy to change.
const TEMPLATE: &str = "\
# SozoIDE settings. Delete a line to go back to its default.
autosave_seconds = 2   # 0 turns autosave off
tab_width = 4          # when a file and its .editorconfig do not say
insert_spaces = true
wrap = false           # long lines wrap on screen
wrap_markdown = true
reduced_motion = false # every transition becomes a cut
check_updates = true   # SOZO_NO_UPDATE_CHECK=1 also turns it off
telemetry = true       # SOZO_NO_TELEMETRY=1 also turns it off
format_on_save = false # the language server formats each file you save
query_timeout_seconds = 60 # a Database query stops after this; 0 lets it run
terminal_scrollback = 10000 # rows of history each terminal keeps
high_contrast = false  # attributes only, no colours (NO_COLOR=1 does the same)
update_channel = \"stable\" # \"preview\" also offers the pre-releases
";

impl Settings {
    pub fn autosave(&self) -> Option<Duration> {
        (self.autosave_seconds > 0).then(|| Duration::from_secs(self.autosave_seconds))
    }

    pub fn query_timeout(&self) -> Option<Duration> {
        (self.query_timeout_seconds > 0).then(|| Duration::from_secs(self.query_timeout_seconds))
    }

    /// Values out of range come back within it: a hostile file never makes a delay of years or
    /// an indentation of a thousand spaces.
    fn clamped(mut self) -> Self {
        self.autosave_seconds = self.autosave_seconds.min(3_600);
        self.tab_width = self.tab_width.clamp(1, 16);
        self.query_timeout_seconds = self.query_timeout_seconds.min(86_400);
        self.terminal_scrollback = self.terminal_scrollback.min(100_000);
        if self.update_channel != "preview" {
            self.update_channel = "stable".into();
        }
        self
    }

    /// The variables of the environment win over the file.
    fn with_environment(mut self, variable: impl Fn(&str) -> bool) -> Self {
        if variable("SOZO_NO_UPDATE_CHECK") {
            self.check_updates = false;
        }
        if variable("SOZO_NO_TELEMETRY") {
            self.telemetry = false;
        }
        if variable("SOZO_REDUCED_MOTION") {
            self.reduced_motion = true;
        }
        self
    }
}

pub fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "SozoIDE")
        .map(|directories| directories.config_dir().join("settings.toml"))
}

/// The settings in `path` and, when the file was broken, what to say; a missing file is
/// written with the defaults, to be found and changed.
pub fn load(path: &Path, variable: impl Fn(&str) -> bool) -> (Settings, Option<String>) {
    let existed = path.exists();
    if !existed {
        let _ = crate::storage::write(path, TEMPLATE);
    }
    let settings: Settings = crate::storage::read(path);
    // `storage::read` keeps a broken file aside as `.broken`: the defaults are in use.
    let warning = (existed && !path.exists()).then(|| {
        format!(
            "{} could not be read: the defaults are in use and it was kept as settings.toml.broken",
            path.display()
        )
    });
    (settings.clamped().with_environment(variable), warning)
}

static SETTINGS: OnceLock<(Settings, Option<String>)> = OnceLock::new();

/// The settings of this run, read once. Debug builds (tests, `cargo run`) neither read nor write
/// the file, like `keys.toml` and the sessions: the defaults and the `SOZO_*` variables.
pub fn get() -> &'static Settings {
    &loaded().0
}

/// What to say about the settings file at startup, if anything.
pub fn warning() -> Option<&'static str> {
    loaded().1.as_deref()
}

fn loaded() -> &'static (Settings, Option<String>) {
    SETTINGS.get_or_init(|| {
        let variable = |name: &str| std::env::var_os(name).is_some();
        match path() {
            Some(path) if !cfg!(test) && !cfg!(debug_assertions) => load(&path, variable),
            _ => (Settings::default().with_environment(variable), None),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_file_has_the_defaults_and_hostile_values_stay_in_range() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        let (settings, warning) = load(&path, |_| false);
        assert_eq!((settings, warning), (Settings::default(), None));
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("autosave_seconds = 2")
        );

        std::fs::write(
            &path,
            "autosave_seconds = 999999999999\ntab_width = 1000\nquery_timeout_seconds = 99999999\nterminal_scrollback = 9999999\n",
        )
        .unwrap();
        let (settings, _) = load(&path, |_| false);
        assert_eq!((settings.autosave_seconds, settings.tab_width), (3_600, 16));
        assert_eq!(settings.query_timeout(), Some(Duration::from_secs(86_400)));
        assert_eq!(settings.terminal_scrollback, 100_000);
        std::fs::write(
            &path,
            "autosave_seconds = 0\ntab_width = 0\nwrap = true\nquery_timeout_seconds = 0\n",
        )
        .unwrap();
        let (settings, _) = load(&path, |_| false);
        assert_eq!(
            (settings.autosave(), settings.query_timeout()),
            (None, None)
        );
        assert_eq!(settings.tab_width, 1);
        assert!(
            settings.wrap && settings.insert_spaces,
            "missing keys keep their default"
        );
    }

    #[test]
    fn a_broken_file_says_so_and_the_environment_wins() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        for broken in [
            "wrap = \"yes\"",
            "autosave_seconds = -1",
            "tab_width = [",
            "\u{0}\u{1}",
        ] {
            std::fs::write(&path, broken).unwrap();
            let (settings, warning) = load(&path, |_| false);
            assert_eq!(settings, Settings::default(), "{broken:?}");
            assert!(
                warning.unwrap().contains("defaults are in use"),
                "{broken:?}"
            );
            assert!(directory.path().join("settings.toml.broken").exists());
        }
        std::fs::write(&path, "check_updates = true\ntelemetry = true\n").unwrap();
        let (settings, _) = load(&path, |name| name != "SOZO_REDUCED_MOTION");
        assert!(!settings.check_updates && !settings.telemetry && !settings.reduced_motion);
    }
}
