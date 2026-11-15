//! Installs a downloaded package the way this copy of SozoIDE was installed and starts the new
//! version in the same terminal and folder. Runs after the TUI has left the alternate screen, so
//! `sudo` can ask for a password.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
};

use anyhow::{Context, Result, bail};

use super::staging;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Package {
    Dmg,
    Msi,
    Deb,
    Rpm,
    Archive,
}

impl Package {
    pub(super) fn detect() -> Result<Self> {
        Ok(Self::for_executable(&executable()?))
    }

    fn for_executable(exe: &Path) -> Self {
        let package_manager = exe == Path::new("/usr/bin/sozo");
        match env::consts::OS {
            "macos" => Self::Dmg,
            "windows" => Self::Msi,
            _ if package_manager && succeeds(Command::new("dpkg").args(["-S"]).arg(exe)) => {
                Self::Deb
            }
            _ if package_manager && succeeds(Command::new("rpm").args(["-qf"]).arg(exe)) => {
                Self::Rpm
            }
            _ => Self::Archive,
        }
    }

    pub(super) fn extension(self) -> &'static str {
        match self {
            Self::Dmg => "dmg",
            Self::Msi => "msi",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Archive => "tar.gz",
        }
    }
}

pub fn install_and_restart(file: &Path, root: &Path) -> Result<()> {
    let exe = executable()?;
    println!("Installing the SozoIDE update...");
    let installed = match Package::for_executable(&exe) {
        Package::Dmg => install_dmg(file, &exe),
        Package::Msi => install_msi(file, &exe),
        // The package only recommends git, curl and ffmpeg (F24): Help's Dependencies tab
        // installs what is missing, after Enter (Q65).
        Package::Deb => run(sudo("dpkg").arg("-i").arg(file)),
        Package::Rpm => run(sudo("rpm").arg("-U").arg(file)),
        Package::Archive => install_archive(file, &exe),
    }
    .context("Could not install the update. Download it again from the SozoIDE website");
    staging::discard(file);
    installed?;
    // A new version that does not even say its version goes back to the one kept (TODO 6.1.2).
    if !starts(&exe) && restore_previous(&exe) {
        println!("The new version did not start: the previous one is back.");
    }
    println!("Restarting SozoIDE...");
    restart(&exe, root)
}

/// Whether `exe` runs and says its version, in a few seconds.
fn starts(exe: &Path) -> bool {
    crate::process::succeeds(
        Command::new(exe).arg("--version"),
        std::time::Duration::from_secs(10),
    )
}

/// Where the version before an update waits until the new one has started: beside the app
/// (`SozoIDE.app.previous`) or the binary (`sozo.previous`).
fn previous_of(exe: &Path) -> PathBuf {
    match app_bundle(exe) {
        Some(app) => app.with_file_name(format!(
            "{}.previous",
            app.file_name().unwrap_or_default().to_string_lossy()
        )),
        None => exe.with_extension("previous"),
    }
}

/// The version kept goes back in place; false when there is none.
fn restore_previous(exe: &Path) -> bool {
    let previous = previous_of(exe);
    if !previous.exists() {
        return false;
    }
    let current: PathBuf = app_bundle(exe).map_or_else(|| exe.to_path_buf(), Path::to_path_buf);
    let aside = current.with_extension("failed");
    let _ = fs::remove_dir_all(&aside);
    let _ = fs::remove_file(&aside);
    fs::rename(&current, &aside).is_ok() && fs::rename(&previous, &current).is_ok()
}

/// The new version started its interface: the one before it is no longer needed. Called by
/// `main` once the app is up.
pub fn confirm_started() {
    let Ok(exe) = executable() else {
        return;
    };
    let previous = previous_of(&exe);
    if fs::symlink_metadata(&previous).is_ok_and(|metadata| metadata.is_dir()) {
        let _ = fs::remove_dir_all(&previous);
    } else {
        let _ = fs::remove_file(&previous);
    }
}

/// The real file behind `sozo`, so the `/usr/local/bin/sozo` link resolves to the `.app`.
fn executable() -> Result<PathBuf> {
    env::current_exe()
        .and_then(fs::canonicalize)
        .context("Could not find the SozoIDE executable")
}

fn install_dmg(file: &Path, exe: &Path) -> Result<()> {
    // Mounted inside the private folder of the download (R2).
    let mount = staging::inside(file, "mount")?;
    run(Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-quiet", "-mountpoint"])
        .arg(&mount)
        .arg(file))?;
    let new_app = mount.join("SozoIDE.app");
    let result = match app_bundle(exe) {
        Some(app) => replace_app(&new_app, app),
        None => replace_file(&new_app.join("Contents/MacOS/sozo"), exe),
    };
    let _ = run(Command::new("hdiutil")
        .args(["detach", "-quiet"])
        .arg(&mount));
    result
}

fn app_bundle(exe: &Path) -> Option<&Path> {
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then_some(app)
}

/// Copies next to the old app and swaps by renaming: the running binary is never overwritten.
fn replace_app(new_app: &Path, app: &Path) -> Result<()> {
    let name = app
        .file_name()
        .context("Invalid app path")?
        .to_string_lossy();
    let staged = app.with_file_name(format!("{name}.new"));
    // Kept until the new version starts (`confirm_started`), to go back to (TODO 6.1.2).
    let old = app.with_file_name(format!("{name}.previous"));
    let _ = fs::remove_dir_all(&staged);
    let _ = fs::remove_dir_all(&old);
    run(Command::new("ditto").arg(new_app).arg(&staged))?;
    fs::rename(app, &old).with_context(|| format!("Could not replace {}", app.display()))?;
    if let Err(error) = fs::rename(&staged, app) {
        let _ = fs::rename(&old, app);
        return Err(error).with_context(|| format!("Could not replace {}", app.display()));
    }
    Ok(())
}

fn replace_file(new: &Path, exe: &Path) -> Result<()> {
    let staged = exe.with_extension("new");
    let previous = exe.with_extension("previous");
    fs::copy(new, &staged).with_context(|| format!("Could not write {}", staged.display()))?;
    let _ = fs::remove_file(&previous);
    // The running binary moves aside (it keeps running) and stays until the new one starts.
    fs::rename(exe, &previous).with_context(|| format!("Could not replace {}", exe.display()))?;
    if let Err(error) = fs::rename(&staged, exe) {
        let _ = fs::rename(&previous, exe);
        return Err(error).with_context(|| format!("Could not replace {}", exe.display()));
    }
    Ok(())
}

fn install_archive(file: &Path, exe: &Path) -> Result<()> {
    let directory = staging::inside(file, "unpacked")?;
    let result = unpack(file, &directory).and_then(|new| replace_file(&new, exe));
    let _ = fs::remove_dir_all(&directory);
    result
}

/// Takes `sozo` out of the archive and nothing else, as this user (R2): another member, a path
/// that leaves the folder or a link in the place of the binary install nothing.
fn unpack(file: &Path, directory: &Path) -> Result<PathBuf> {
    run(Command::new("tar")
        .arg("-xzf")
        .arg(file)
        .arg("-C")
        .arg(directory)
        .args(["--no-same-owner", "sozo"])
        .stdin(Stdio::null()))?;
    let new = directory.join("sozo");
    if !fs::symlink_metadata(&new).is_ok_and(|metadata| metadata.is_file()) {
        bail!("The update has no sozo binary");
    }
    Ok(new)
}

/// Windows keeps the running `sozo.exe` locked but lets it move, so it leaves `bin` before the
/// installer writes the new one into the same folder.
fn install_msi(file: &Path, exe: &Path) -> Result<()> {
    let bin = exe.parent().context("Invalid executable path")?;
    let folder = bin
        .parent()
        .filter(|_| {
            bin.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("bin"))
        })
        .context("This copy of SozoIDE was not installed with the MSI installer")?;
    let moved = file.with_file_name(format!("sozo-old-{}.exe", process::id()));
    let moved = if fs::rename(exe, &moved).is_ok() {
        moved
    } else {
        let beside = exe.with_extension("exe.old");
        fs::rename(exe, &beside).context("Could not move the running sozo.exe")?;
        beside
    };
    let mut command = Command::new("msiexec");
    command.arg("/i").arg(file).args(["/passive", "/norestart"]);
    application_folder(&mut command, folder);
    let status = command.status().context("Could not run msiexec")?;
    // 3010: installed, a reboot would finish removing the moved executable.
    if !matches!(status.code(), Some(0 | 3010)) {
        let _ = fs::rename(&moved, exe);
        bail!("msiexec failed ({status})");
    }
    Ok(())
}

/// msiexec reads `NAME="value"`, not the `"NAME=value"` that normal quoting would produce.
#[cfg(windows)]
fn application_folder(command: &mut Command, folder: &Path) {
    use std::os::windows::process::CommandExt;
    command.raw_arg(format!("APPLICATIONFOLDER=\"{}\"", folder.display()));
}

#[cfg(not(windows))]
fn application_folder(command: &mut Command, folder: &Path) {
    command.arg(format!("APPLICATIONFOLDER={}", folder.display()));
}

fn sudo(program: &str) -> Command {
    match super::deps::sudo().first() {
        Some(sudo) => {
            let mut command = Command::new(sudo);
            command.arg(program);
            command
        }
        None => Command::new(program),
    }
}

#[cfg(unix)]
fn restart(exe: &Path, root: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    Err(Command::new(exe).arg(root).exec()).context("Could not restart SozoIDE")
}

/// Windows has no `exec`: the new version runs as a child in this console and its exit code
/// becomes ours.
#[cfg(not(unix))]
fn restart(exe: &Path, root: &Path) -> Result<()> {
    let status = Command::new(exe)
        .arg(root)
        .status()
        .context("Could not restart SozoIDE")?;
    process::exit(status.code().unwrap_or(1));
}

fn run(command: &mut Command) -> Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .with_context(|| format!("Could not run {program}"))?;
    if !status.success() {
        bail!("{program} failed ({status})");
    }
    Ok(())
}

fn succeeds(command: &mut Command) -> bool {
    crate::process::succeeds(command, crate::process::QUICK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn the_version_before_an_update_is_kept_and_comes_back_when_the_new_one_fails() {
        let directory = tempfile::tempdir().unwrap();
        let exe = directory.path().join("sozo");
        std::fs::write(&exe, "old").unwrap();
        let new = directory.path().join("new");
        std::fs::write(&new, "new").unwrap();
        replace_file(&new, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(previous_of(&exe)).unwrap(),
            "old",
            "kept until it starts"
        );
        // A new binary that does not run goes back to the old one.
        assert!(!starts(&exe));
        assert!(restore_previous(&exe));
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "old");
        assert!(!restore_previous(&exe), "nothing left to go back to");
        // In a .app, the whole bundle is kept beside it.
        let app = directory.path().join("SozoIDE.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        assert_eq!(
            previous_of(&app.join("Contents/MacOS/sozo")),
            directory.path().join("SozoIDE.app.previous")
        );
    }

    #[test]
    fn finds_the_app_bundle_around_the_executable() {
        let exe = Path::new("/Applications/SozoIDE.app/Contents/MacOS/sozo");
        assert_eq!(
            app_bundle(exe),
            Some(Path::new("/Applications/SozoIDE.app"))
        );
        assert_eq!(app_bundle(Path::new("/usr/local/bin/sozo")), None);
    }

    #[test]
    fn swaps_the_app_and_the_binary_without_leaving_copies() {
        let directory = tempfile::tempdir().unwrap();
        let app = directory.path().join("SozoIDE.app");
        let new_app = directory.path().join("mount/SozoIDE.app");
        for (root, version) in [(&app, "old"), (&new_app, "new")] {
            fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
            fs::write(root.join("Contents/MacOS/sozo"), version).unwrap();
        }
        if cfg!(target_os = "macos") {
            replace_app(&new_app, &app).unwrap();
            assert_eq!(
                fs::read_to_string(app.join("Contents/MacOS/sozo")).unwrap(),
                "new"
            );
            assert!(!directory.path().join("SozoIDE.app.old").exists());
            assert!(!directory.path().join("SozoIDE.app.new").exists());
        }

        let exe = directory.path().join("sozo");
        fs::write(&exe, "old").unwrap();
        replace_file(&new_app.join("Contents/MacOS/sozo"), &exe).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
        assert!(!exe.with_extension("new").exists());
    }

    /// One member of a `ustar` archive, written by hand so that it can hold what `tar` itself
    /// refuses to pack.
    #[cfg(unix)]
    fn member(name: &str, kind: u8, link: &str, data: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000755\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].copy_from_slice(b"        ");
        header[156] = kind;
        header[157..157 + link.len()].copy_from_slice(link.as_bytes());
        header[257..265].copy_from_slice(b"ustar\x0000");
        let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().div_ceil(512) * 512, 0);
        bytes
    }

    #[cfg(unix)]
    fn archive(directory: &Path, members: &[Vec<u8>]) -> PathBuf {
        let tar = directory.join("package.tar");
        let mut bytes = members.concat();
        bytes.extend_from_slice(&[0; 1024]);
        fs::write(&tar, bytes).unwrap();
        assert!(succeeds(Command::new("gzip").arg(&tar)));
        directory.join("package.tar.gz")
    }

    #[test]
    #[cfg(unix)]
    fn only_the_binary_leaves_an_archive_and_never_as_a_link() {
        let directory = tempfile::tempdir().unwrap();
        let download = directory.path().join("download");
        fs::create_dir(&download).unwrap();
        let file = archive(
            &download,
            &[
                member("sozo", b'0', "", b"new"),
                member("extra", b'0', "", b"more"),
                member("../outside", b'0', "", b"out"),
            ],
        );
        let unpacked = staging::inside(&file, "unpacked").unwrap();
        let new = unpack(&file, &unpacked).unwrap();
        assert_eq!(fs::read_to_string(new).unwrap(), "new");
        assert!(!unpacked.join("extra").exists());
        assert!(!download.join("outside").exists());
        assert!(!directory.path().join("outside").exists());

        // A link where the binary should be would copy whatever it points to.
        let secret = directory.path().join("secret");
        fs::write(&secret, "private").unwrap();
        let other = directory.path().join("other");
        fs::create_dir(&other).unwrap();
        let file = archive(
            &other,
            &[member("sozo", b'2', secret.to_str().unwrap(), b"")],
        );
        let unpacked = staging::inside(&file, "unpacked").unwrap();
        let error = unpack(&file, &unpacked).unwrap_err();
        assert_eq!(error.to_string(), "The update has no sozo binary");

        // And an archive without it installs nothing.
        let empty = directory.path().join("empty");
        fs::create_dir(&empty).unwrap();
        let file = archive(&empty, &[member("extra", b'0', "", b"more")]);
        let unpacked = staging::inside(&file, "unpacked").unwrap();
        assert!(unpack(&file, &unpacked).is_err());
    }

    #[test]
    fn this_system_has_a_package_for_updates() {
        let package = Package::detect().unwrap();
        let expected = match env::consts::OS {
            "macos" => Package::Dmg,
            "windows" => Package::Msi,
            _ => Package::Archive,
        };
        assert_eq!(package, expected);
    }
}
