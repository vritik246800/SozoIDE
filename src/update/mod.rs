//! New releases: asks GitHub for the latest tag, downloads the package of this system and hands
//! it to `install`. The network goes through the system `curl` (macOS, Windows 10+ and Linux).

use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod deps;
mod install;
mod notes;

use install::Package;
pub use install::install_and_restart;
pub use notes::{NOTES, Notes};

use crate::branding::REPOSITORY;
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Release builds check at startup; debug builds (tests, `cargo run`) and
/// `SOZO_NO_UPDATE_CHECK=1` never touch the network.
pub fn enabled() -> bool {
    !cfg!(debug_assertions) && !cfg!(test) && env::var_os("SOZO_NO_UPDATE_CHECK").is_none()
}

/// Release builds count a new installation or update once (F20); `SOZO_NO_TELEMETRY=1` opts out.
fn counts() -> bool {
    !cfg!(debug_assertions) && !cfg!(test) && env::var_os("SOZO_NO_TELEMETRY").is_none()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Available,
    Downloading { received: u64, total: Option<u64> },
    Ready(PathBuf),
    Failed(String),
}

enum Message {
    Latest(String),
    /// The latest version could not be read: only said after a check from About.
    Unreachable,
    Progress(u64, Option<u64>),
    Downloaded(PathBuf),
    Failed(String),
}

#[derive(Default, Deserialize, Serialize)]
struct Settings {
    skipped: Option<String>,
    /// The last version that started on this machine; none means a new installation.
    seen: Option<String>,
    /// The tools the Setup dialog already offered (F36): each one is offered once.
    #[serde(default)]
    offered: Vec<String>,
    /// The terminals run in tmux (F38).
    #[serde(default)]
    tmux: bool,
}

impl Settings {
    fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|contents| toml::from_str(&contents).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) -> Option<()> {
        path.parent().map_or(Ok(()), fs::create_dir_all).ok()?;
        fs::write(path, toml::to_string(self).ok()?).ok()
    }
}

/// What greets the user after the splash (F17, F18): what's new comes with the version that
/// ran before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Welcome {
    None,
    Tutorial,
    WhatsNew(String),
}

/// The notes of the versions after `seen`, up to this one, newest first.
pub fn notes_since(seen: &str) -> Vec<&'static Notes> {
    NOTES
        .iter()
        .filter(|notes| is_newer(notes.version, seen) && !is_newer(notes.version, CURRENT))
        .collect()
}

pub struct Updater {
    pub state: UpdateState,
    /// The version offered to the user; kept after a dismiss so the closing dialog keeps its text.
    pub version: String,
    settings: Option<PathBuf>,
    receiver: Option<Receiver<Message>>,
    cancel: Arc<AtomicBool>,
    /// The check was asked for in About: it also offers a skipped version and says when there
    /// is nothing newer.
    manual: bool,
    /// What the last check from About found, for its dialog.
    pub checked: Option<String>,
}

impl Updater {
    pub fn new(check: bool) -> Self {
        let mut updater = Self {
            state: UpdateState::Idle,
            version: String::new(),
            settings: ProjectDirs::from("", "", "SozoIDE")
                .filter(|_| !cfg!(test))
                .map(|directories| directories.config_dir().join("update.toml")),
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            manual: false,
            checked: None,
        };
        if check {
            let sender = updater.channel();
            thread::spawn(move || {
                if let Ok(latest) = latest_version() {
                    let _ = sender.send(Message::Latest(latest));
                }
            });
        }
        updater
    }

    #[cfg(test)]
    pub fn offering(version: &str, settings: PathBuf) -> Self {
        Self {
            state: UpdateState::Available,
            version: version.to_string(),
            settings: Some(settings),
            ..Self::new(false)
        }
    }

    pub fn tick(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        let messages = receiver.try_iter().collect::<Vec<_>>();
        for message in messages {
            match message {
                Message::Latest(latest) => self.offer(latest),
                Message::Unreachable => {
                    self.manual = false;
                    self.checked = Some("Could not check for updates".to_string());
                }
                Message::Progress(received, total) => {
                    if matches!(self.state, UpdateState::Downloading { .. }) {
                        self.state = UpdateState::Downloading { received, total };
                    }
                }
                Message::Downloaded(file) => self.state = UpdateState::Ready(file),
                Message::Failed(error) => self.state = UpdateState::Failed(error),
            }
        }
    }

    /// Offers `latest` when it is newer. A skipped version is offered again only from About; a
    /// version newer than the skipped one is always offered.
    fn offer(&mut self, latest: String) {
        let manual = std::mem::take(&mut self.manual);
        let skipped = !manual && self.skipped().as_deref() == Some(latest.as_str());
        if is_newer(&latest, CURRENT) && !skipped {
            self.version = latest;
            self.state = UpdateState::Available;
            self.checked = None;
        } else if manual {
            self.checked = Some(format!("You have the latest version ({CURRENT})"));
        }
    }

    /// About's button: asks GitHub for the latest release now, also when it was skipped
    /// (F16). Nothing happens while an update downloads or waits to install.
    pub fn check_now(&mut self) {
        if matches!(
            self.state,
            UpdateState::Downloading { .. } | UpdateState::Ready(_)
        ) {
            return;
        }
        self.manual = true;
        self.checked = Some("Checking for updates...".to_string());
        // The tests never ask GitHub: they send the answer themselves.
        if cfg!(test) {
            return;
        }
        let sender = self.channel();
        thread::spawn(move || {
            let _ = sender.send(latest_version().map_or(Message::Unreachable, Message::Latest));
        });
    }

    pub fn start_download(&mut self) {
        let package = match Package::detect() {
            Ok(package) => package,
            Err(error) => {
                self.state = UpdateState::Failed(error.to_string());
                return;
            }
        };
        let name = asset_name(env::consts::OS, env::consts::ARCH, package.extension());
        let url = package_url(&name);
        let sums = package_url(SUMS);
        let file = env::temp_dir().join(&name);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let sender = self.channel();
        self.state = UpdateState::Downloading {
            received: 0,
            total: None,
        };
        thread::spawn(move || {
            let verified = download(&url, &file, &cancel, &sender).and_then(|()| {
                let sums = curl(&["-fsSL", "--max-time", "60", &sums])
                    .context("Could not check the update: no SHA256SUMS.txt in the release")?;
                verify(&file, &name, &sums)
            });
            let message = match verified {
                Ok(()) => Message::Downloaded(file),
                Err(error) => {
                    let _ = fs::remove_file(&file);
                    Message::Failed(format!("{error:#}"))
                }
            };
            let _ = sender.send(message);
        });
    }

    /// The tutorial on a new installation, what's new on the first start of a newer version;
    /// remembers this version either way, so each shows once.
    pub fn welcome(&self) -> Welcome {
        let Some(path) = &self.settings else {
            return Welcome::None;
        };
        let mut settings = Settings::load(path);
        let welcome = match settings.seen.as_deref() {
            None => {
                count("installed");
                Welcome::Tutorial
            }
            Some(seen) if is_newer(CURRENT, seen) => {
                count("updated");
                Welcome::WhatsNew(seen.to_owned())
            }
            Some(_) => Welcome::None,
        };
        if settings.seen.as_deref() != Some(CURRENT) {
            settings.seen = Some(CURRENT.to_string());
            settings.save(path);
        }
        welcome
    }

    /// Whether the Setup dialog has something new to say (F36): a tool of `tools` this machine
    /// does not `have` and that was not offered before. Remembers them, so it says it once; a
    /// version that needs a new tool brings it to `tools`.
    pub fn setup_due(&self, tools: &[&str], have: impl Fn(&str) -> bool) -> bool {
        let Some(path) = &self.settings else {
            return false;
        };
        let mut settings = Settings::load(path);
        let due = tools
            .iter()
            .any(|tool| !have(tool) && !settings.offered.iter().any(|known| known == tool));
        for tool in tools {
            if !settings.offered.iter().any(|known| known == tool) {
                settings.offered.push((*tool).to_owned());
            }
        }
        settings.save(path);
        due
    }

    /// Whether the terminals run in tmux (F38).
    pub fn tmux(&self) -> bool {
        self.settings
            .as_ref()
            .is_some_and(|path| Settings::load(path).tmux)
    }

    pub fn set_tmux(&self, on: bool) {
        if let Some(path) = &self.settings {
            let mut settings = Settings::load(path);
            settings.tmux = on;
            settings.save(path);
        }
    }

    /// Never offers this version again.
    pub fn skip(&mut self) {
        if let Some(path) = &self.settings {
            let mut settings = Settings::load(path);
            settings.skipped = Some(self.version.clone());
            if settings.save(path).is_none() {
                self.state = UpdateState::Failed("Could not save the skipped version".to_string());
                return;
            }
        }
        self.state = UpdateState::Idle;
    }

    /// Closes the dialog for this session and stops a running download.
    pub fn dismiss(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.receiver = None;
        self.state = UpdateState::Idle;
    }

    pub fn ready_file(&self) -> Option<&Path> {
        match &self.state {
            UpdateState::Ready(file) => Some(file),
            _ => None,
        }
    }

    fn skipped(&self) -> Option<String> {
        Settings::load(self.settings.as_ref()?).skipped
    }

    fn channel(&mut self) -> Sender<Message> {
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        sender
    }
}

/// Package names carry no version, so `releases/latest/download/<name>` always works (the site
/// links there too).
pub fn asset_name(os: &str, arch: &str, extension: &str) -> String {
    let arch = if arch == "aarch64" { "arm64" } else { arch };
    format!("SozoIDE-{os}-{arch}.{extension}")
}

/// The checksums of a release's packages, one `<sha256>  <name>` per line.
const SUMS: &str = "SHA256SUMS.txt";

/// Whether `file` is the package `name` of the release `sums` came from: nothing installs
/// otherwise (F16).
fn verify(file: &Path, name: &str, sums: &str) -> Result<()> {
    let expected = sums
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, file)| file.trim_start().trim_start_matches('*') == name)
        .map(|(hash, _)| hash.to_ascii_lowercase())
        .with_context(|| format!("Could not check the update: {name} is not in {SUMS}"))?;
    let mut hasher = Sha256::new();
    let mut package = fs::File::open(file).context("Could not read the update")?;
    std::io::copy(&mut package, &mut hasher).context("Could not read the update")?;
    let found = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if found != expected {
        bail!("The update does not match {SUMS}: it was not installed");
    }
    Ok(())
}

/// Always the latest release when the download starts, even if a newer one came out after the
/// startup check: updates never stop on an older version.
fn package_url(name: &str) -> String {
    format!("{REPOSITORY}/releases/latest/download/{name}")
}

/// GitHub counts every download of a release file, so fetching the small
/// `SozoIDE-<os>-<arch>.<kind>.txt` of this version counts one installation (`installed`) or
/// update (`updated`). Nothing else is sent.
fn count(kind: &str) {
    if !counts() {
        return;
    }
    let url = counter_url(kind, env::consts::OS, env::consts::ARCH);
    thread::spawn(move || {
        let _ = curl(&["-fsSL", "--max-time", "10", &url]);
    });
}

fn counter_url(kind: &str, os: &str, arch: &str) -> String {
    format!(
        "{REPOSITORY}/releases/download/v{CURRENT}/{}",
        asset_name(os, arch, &format!("{kind}.txt"))
    )
}

fn latest_version() -> Result<String> {
    let headers = curl(&[
        "-sSI",
        "--max-time",
        "10",
        &format!("{REPOSITORY}/releases/latest"),
    ])?;
    tag_from_headers(&headers).context("No release found")
}

/// `releases/latest` redirects to `releases/tag/vX.Y.Z`.
fn tag_from_headers(headers: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.trim().eq_ignore_ascii_case("location") {
            return None;
        }
        let (_, tag) = value.trim().rsplit_once("/tag/v")?;
        Some(tag.to_string())
    })
}

fn version_numbers(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    let numbers = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(numbers)
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (version_numbers(latest), version_numbers(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// The last `content-length` is the one of the file, after GitHub's redirect.
fn content_length(headers: &str) -> Option<u64> {
    headers
        .lines()
        .rev()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .find_map(|(_, value)| value.trim().parse::<u64>().ok())
        .filter(|length| *length > 0)
}

fn download(url: &str, file: &Path, cancel: &AtomicBool, sender: &Sender<Message>) -> Result<()> {
    let total = curl(&["-sSIL", "--max-time", "30", url])
        .ok()
        .and_then(|headers| content_length(&headers));
    let _ = fs::remove_file(file);
    let mut child = Command::new("curl")
        .args(["-fsSL", "--max-time", "900", "-o"])
        .arg(file)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not run curl")?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(file);
            bail!("Download cancelled");
        }
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let mut error = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                let _ = stderr.read_to_string(&mut error);
            }
            bail!("Could not download the update: {}", error.trim());
        }
        let received = fs::metadata(file).map_or(0, |metadata| metadata.len());
        let _ = sender.send(Message::Progress(received, total));
        thread::sleep(Duration::from_millis(100));
    }
}

fn curl(args: &[&str]) -> Result<String> {
    let output = Command::new("curl")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("Could not run curl")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn updater_in(directory: &Path) -> Updater {
        Updater {
            settings: Some(directory.join("update.toml")),
            ..Updater::new(false)
        }
    }

    #[test]
    fn greets_new_installations_and_updates_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("update.toml");
        let updater = updater_in(directory.path());
        assert_eq!(updater.welcome(), Welcome::Tutorial);
        assert_eq!(updater.welcome(), Welcome::None);

        fs::write(&path, "skipped = \"9.9.9\"\nseen = \"0.0.1\"\n").unwrap();
        assert_eq!(updater.welcome(), Welcome::WhatsNew("0.0.1".to_owned()));
        assert_eq!(updater.welcome(), Welcome::None);
        let mut skipping = updater_in(directory.path());
        skipping.version = "9.9.8".to_string();
        skipping.skip();
        let settings = Settings::load(&path);
        assert_eq!(settings.skipped.as_deref(), Some("9.9.8"));
        assert_eq!(
            settings.seen.as_deref(),
            Some(CURRENT),
            "skipping keeps the seen version"
        );
    }

    #[test]
    fn installations_and_updates_are_counted_on_the_files_of_this_version() {
        assert!(!counts(), "debug builds never touch the network");
        assert_eq!(
            counter_url("installed", "macos", "aarch64"),
            format!("{REPOSITORY}/releases/download/v{CURRENT}/SozoIDE-macos-arm64.installed.txt")
        );
        assert!(
            counter_url("updated", "windows", "x86_64")
                .ends_with(&format!("v{CURRENT}/SozoIDE-windows-x86_64.updated.txt"))
        );
    }

    #[test]
    fn the_setup_dialog_comes_once_and_again_only_for_a_tool_it_never_offered() {
        let directory = tempfile::tempdir().unwrap();
        let updater = updater_in(directory.path());
        let nothing = |_: &str| false;
        // Everything installed: no dialog, and nothing to say later either.
        assert!(!updater.setup_due(&["git"], |_| true));
        assert!(!updater.setup_due(&["git"], nothing));
        // A new installation without ffmpeg, once.
        assert!(updater.setup_due(&["git", "ffmpeg"], nothing));
        assert!(!updater.setup_due(&["git", "ffmpeg"], nothing));
        // A version that brings tmux asks about tmux, whatever the user skipped before.
        assert!(updater.setup_due(&["git", "ffmpeg", "tmux"], nothing));
        assert!(!updater.setup_due(&["git", "ffmpeg", "tmux"], nothing));
        // The other settings stay, and an old file reads.
        let path = directory.path().join("update.toml");
        fs::write(&path, "seen = \"0.8.1\"\n").unwrap();
        assert!(updater.setup_due(&["tmux"], nothing));
        assert_eq!(Settings::load(&path).seen.as_deref(), Some("0.8.1"));
    }

    #[test]
    fn tmux_is_off_until_it_is_turned_on() {
        let directory = tempfile::tempdir().unwrap();
        let updater = updater_in(directory.path());
        assert!(!updater.tmux());
        updater.set_tmux(true);
        assert!(updater.tmux());
        updater.set_tmux(false);
        assert!(!updater.tmux());
    }

    #[test]
    fn only_the_package_the_release_lists_passes() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("package");
        fs::write(&file, "abc").unwrap();
        // SHA-256 of "abc", from FIPS 180.
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let name = "SozoIDE-macos-arm64.dmg";
        let sums = format!("0000  SozoIDE-windows-x86_64.msi\n{hash}  {name}\n");
        assert!(verify(&file, name, &sums).is_ok());
        // As `sha256sum -b` writes it, and in capitals.
        let binary = format!("{}  *{name}\n", hash.to_uppercase());
        assert!(verify(&file, name, &binary).is_ok());

        fs::write(&file, "abd").unwrap();
        let error = verify(&file, name, &sums).unwrap_err().to_string();
        assert!(error.contains("does not match SHA256SUMS.txt"), "{error}");

        let error = verify(&file, "SozoIDE-linux-x86_64.deb", &sums)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not in SHA256SUMS.txt"), "{error}");
        assert!(verify(&directory.path().join("gone"), name, &sums).is_err());
    }

    #[test]
    fn release_notes_belong_to_this_version() {
        assert_eq!(NOTES[0].version, CURRENT);
        assert!(!NOTES[0].new.is_empty());
        assert!(
            NOTES
                .windows(2)
                .all(|pair| is_newer(pair[0].version, pair[1].version)),
            "newest first"
        );
    }

    #[test]
    fn an_update_shows_the_notes_of_every_version_since_the_one_that_ran() {
        let versions = |seen| {
            notes_since(seen)
                .iter()
                .map(|notes| notes.version)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            versions("0.3.1"),
            [
                CURRENT, "0.8.2", "0.8.1", "0.8.0", "0.7.9", "0.7.3", "0.7.2", "0.7.1", "0.7.0",
                "0.6.0", "0.5.0", "0.4.0", "0.3.3", "0.3.2"
            ]
        );
        assert_eq!(versions("0.1.4").len(), NOTES.len());
        assert!(versions(CURRENT).is_empty());
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.1", "0.1.2"));
        assert!(!is_newer("0.1", "0.0.1"));
        assert!(!is_newer("0.2.0-beta", "0.1.0"));
    }

    #[test]
    fn reads_the_tag_and_size_from_github_headers() {
        let latest = "HTTP/2 302 \r\nLocation: https://github.com/o/r/releases/tag/v0.1.2\r\n\r\n";
        assert_eq!(tag_from_headers(latest).as_deref(), Some("0.1.2"));
        assert_eq!(tag_from_headers("HTTP/2 404 \r\n\r\n"), None);
        let asset =
            "HTTP/2 302 \r\ncontent-length: 0\r\n\r\nHTTP/2 200 \r\ncontent-length: 845\r\n";
        assert_eq!(content_length(asset), Some(845));
        assert_eq!(content_length("HTTP/2 302 \r\ncontent-length: 0\r\n"), None);
    }

    #[test]
    fn package_names_match_the_release_files() {
        assert_eq!(
            asset_name("macos", "aarch64", "dmg"),
            "SozoIDE-macos-arm64.dmg"
        );
        assert_eq!(
            asset_name("windows", "x86_64", "msi"),
            "SozoIDE-windows-x86_64.msi"
        );
        assert_eq!(
            asset_name("linux", "aarch64", "tar.gz"),
            "SozoIDE-linux-arm64.tar.gz"
        );
    }

    #[test]
    fn updates_download_the_latest_release() {
        assert_eq!(
            package_url("SozoIDE-macos-arm64.dmg"),
            format!("{REPOSITORY}/releases/latest/download/SozoIDE-macos-arm64.dmg")
        );
    }

    #[test]
    fn offers_newer_versions_until_one_is_skipped() {
        let directory = tempfile::tempdir().unwrap();
        let mut updater = updater_in(directory.path());
        updater.offer("0.0.1".to_string());
        assert_eq!(updater.state, UpdateState::Idle);

        updater.offer("99.0.0".to_string());
        assert_eq!(updater.state, UpdateState::Available);
        updater.skip();
        assert_eq!(updater.state, UpdateState::Idle);

        let mut restarted = updater_in(directory.path());
        restarted.offer("99.0.0".to_string());
        assert_eq!(restarted.state, UpdateState::Idle);
        restarted.offer("99.0.1".to_string());
        assert_eq!(restarted.state, UpdateState::Available);
        assert_eq!(restarted.version, "99.0.1");
    }

    #[test]
    fn a_check_from_about_offers_the_skipped_version_and_says_when_there_is_none() {
        let directory = tempfile::tempdir().unwrap();
        let mut updater = updater_in(directory.path());
        updater.offer("99.0.0".to_string());
        updater.skip();

        // The startup check keeps quiet about the skipped version; About's offers it.
        updater.offer("99.0.0".to_string());
        assert_eq!(updater.state, UpdateState::Idle);
        assert!(updater.checked.is_none());
        updater.manual = true;
        updater.offer("99.0.0".to_string());
        assert_eq!(updater.state, UpdateState::Available);
        assert_eq!(updater.version, "99.0.0");
        assert!(!updater.manual, "only that check");
        updater.dismiss();

        // Nothing newer: About says so; the startup check says nothing.
        updater.offer(CURRENT.to_string());
        assert!(updater.checked.is_none());
        updater.manual = true;
        updater.offer(CURRENT.to_string());
        assert_eq!(updater.state, UpdateState::Idle);
        assert_eq!(
            updater.checked,
            Some(format!("You have the latest version ({CURRENT})"))
        );

        // No answer from GitHub, after a check from About.
        updater.manual = true;
        let sender = updater.channel();
        sender.send(Message::Unreachable).unwrap();
        updater.tick();
        assert_eq!(
            updater.checked.as_deref(),
            Some("Could not check for updates")
        );
        assert!(!updater.manual);

        // A download in progress is not checked again.
        updater.checked = None;
        updater.state = UpdateState::Downloading {
            received: 0,
            total: None,
        };
        updater.check_now();
        assert!(updater.checked.is_none() && !updater.manual);
    }

    #[test]
    fn progress_and_results_arrive_from_the_worker_threads() {
        let directory = tempfile::tempdir().unwrap();
        let mut updater = updater_in(directory.path());
        let sender = updater.channel();
        sender.send(Message::Latest("99.0.0".to_string())).unwrap();
        updater.tick();
        assert_eq!(updater.state, UpdateState::Available);

        updater.state = UpdateState::Downloading {
            received: 0,
            total: None,
        };
        sender.send(Message::Progress(5, Some(10))).unwrap();
        updater.tick();
        assert_eq!(
            updater.state,
            UpdateState::Downloading {
                received: 5,
                total: Some(10)
            }
        );
        let file = directory.path().join("SozoIDE-macos-arm64.dmg");
        sender.send(Message::Downloaded(file.clone())).unwrap();
        updater.tick();
        assert_eq!(updater.ready_file(), Some(file.as_path()));

        updater.dismiss();
        assert!(updater.cancel.load(Ordering::Relaxed));
        assert_eq!(updater.state, UpdateState::Idle);
        assert_eq!(updater.version, "99.0.0");
    }

    #[test]
    fn cancelling_stops_the_download_and_removes_the_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("partial");
        let (sender, _receiver) = mpsc::channel();
        let cancel = AtomicBool::new(true);
        let error = download("http://127.0.0.1:9/none", &file, &cancel, &sender).unwrap_err();
        assert_eq!(error.to_string(), "Download cancelled");
        assert!(!file.exists());
    }
}
