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

mod install;
mod notes;

use install::Package;
pub use install::install_and_restart;
pub use notes::NOTES;

use crate::branding::REPOSITORY;
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Release builds check at startup; debug builds (tests, `cargo run`) and
/// `SOZO_NO_UPDATE_CHECK=1` never touch the network.
pub fn enabled() -> bool {
    !cfg!(debug_assertions) && env::var_os("SOZO_NO_UPDATE_CHECK").is_none()
}

/// Release builds count a new installation once (F20); `SOZO_NO_TELEMETRY=1` opts out.
fn counts_installs() -> bool {
    !cfg!(debug_assertions) && env::var_os("SOZO_NO_TELEMETRY").is_none()
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
    Progress(u64, Option<u64>),
    Downloaded(PathBuf),
    Failed(String),
}

#[derive(Default, Deserialize, Serialize)]
struct Settings {
    skipped: Option<String>,
    /// The last version that started on this machine; none means a new installation.
    seen: Option<String>,
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

/// What greets the user after the splash (F17, F18).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Welcome {
    None,
    Tutorial,
    WhatsNew,
}

pub struct Updater {
    pub state: UpdateState,
    /// The version offered to the user; kept after a dismiss so the closing dialog keeps its text.
    pub version: String,
    settings: Option<PathBuf>,
    receiver: Option<Receiver<Message>>,
    cancel: Arc<AtomicBool>,
}

impl Updater {
    pub fn new(check: bool) -> Self {
        let mut updater = Self {
            state: UpdateState::Idle,
            version: String::new(),
            settings: ProjectDirs::from("", "", "SozoIDE")
                .map(|directories| directories.config_dir().join("update.toml")),
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
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

    fn offer(&mut self, latest: String) {
        if is_newer(&latest, CURRENT) && self.skipped().as_deref() != Some(latest.as_str()) {
            self.version = latest;
            self.state = UpdateState::Available;
        }
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
        let url = format!("{REPOSITORY}/releases/download/v{}/{name}", self.version);
        let file = env::temp_dir().join(name);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let sender = self.channel();
        self.state = UpdateState::Downloading {
            received: 0,
            total: None,
        };
        thread::spawn(move || {
            let message = match download(&url, &file, &cancel, &sender) {
                Ok(()) => Message::Downloaded(file),
                Err(error) => Message::Failed(error.to_string()),
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
                count_install();
                Welcome::Tutorial
            }
            Some(seen) if is_newer(CURRENT, seen) => Welcome::WhatsNew,
            Some(_) => Welcome::None,
        };
        if settings.seen.as_deref() != Some(CURRENT) {
            settings.seen = Some(CURRENT.to_string());
            settings.save(path);
        }
        welcome
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

/// GitHub counts every download of a release file, so fetching the small
/// `SozoIDE-<os>-<arch>.installed.txt` of this version counts one installation. Nothing else is sent.
fn count_install() {
    if !counts_installs() {
        return;
    }
    let url = install_counter_url(env::consts::OS, env::consts::ARCH);
    thread::spawn(move || {
        let _ = curl(&["-fsSL", "--max-time", "10", &url]);
    });
}

fn install_counter_url(os: &str, arch: &str) -> String {
    format!(
        "{REPOSITORY}/releases/download/v{CURRENT}/{}",
        asset_name(os, arch, "installed.txt")
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
        assert_eq!(updater.welcome(), Welcome::WhatsNew);
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
    fn installations_are_counted_on_the_file_of_this_version() {
        assert!(!counts_installs(), "debug builds never touch the network");
        assert_eq!(
            install_counter_url("macos", "aarch64"),
            format!("{REPOSITORY}/releases/download/v{CURRENT}/SozoIDE-macos-arm64.installed.txt")
        );
        assert!(
            install_counter_url("windows", "x86_64")
                .ends_with("SozoIDE-windows-x86_64.installed.txt")
        );
    }

    #[test]
    fn release_notes_belong_to_this_version() {
        assert_eq!(NOTES.version, CURRENT);
        assert!(!NOTES.new.is_empty());
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
