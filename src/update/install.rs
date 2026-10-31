//! Installs a downloaded package the way this copy of SozoIDE was installed and starts the new
//! version in the same terminal and folder. Runs after the TUI has left the alternate screen, so
//! `sudo` can ask for a password.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
};

use anyhow::{Context, Result, bail};

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
    match Package::for_executable(&exe) {
        Package::Dmg => install_dmg(file, &exe),
        Package::Msi => install_msi(file, &exe),
        // The package only recommends git, curl and ffmpeg (F24): Help's Dependencies tab
        // installs what is missing, after Enter (Q65).
        Package::Deb => run(sudo("dpkg").arg("-i").arg(file)),
        Package::Rpm => run(sudo("rpm").arg("-U").arg(file)),
        Package::Archive => install_archive(file, &exe),
    }
    .context("Could not install the update. Download it again from the SozoIDE website")?;
    let _ = fs::remove_file(file);
    println!("Restarting SozoIDE...");
    restart(&exe, root)
}

/// The real file behind `sozo`, so the `/usr/local/bin/sozo` link resolves to the `.app`.
fn executable() -> Result<PathBuf> {
    env::current_exe()
        .and_then(fs::canonicalize)
        .context("Could not find the SozoIDE executable")
}

fn install_dmg(file: &Path, exe: &Path) -> Result<()> {
    let mount = env::temp_dir().join(format!("sozo-update-{}", process::id()));
    fs::create_dir_all(&mount)?;
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
    let old = app.with_file_name(format!("{name}.old"));
    let _ = fs::remove_dir_all(&staged);
    let _ = fs::remove_dir_all(&old);
    run(Command::new("ditto").arg(new_app).arg(&staged))?;
    fs::rename(app, &old).with_context(|| format!("Could not replace {}", app.display()))?;
    if let Err(error) = fs::rename(&staged, app) {
        let _ = fs::rename(&old, app);
        return Err(error).with_context(|| format!("Could not replace {}", app.display()));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

fn replace_file(new: &Path, exe: &Path) -> Result<()> {
    let staged = exe.with_extension("new");
    fs::copy(new, &staged).with_context(|| format!("Could not write {}", staged.display()))?;
    fs::rename(&staged, exe).with_context(|| format!("Could not replace {}", exe.display()))
}

fn install_archive(file: &Path, exe: &Path) -> Result<()> {
    let directory = env::temp_dir().join(format!("sozo-update-{}", process::id()));
    fs::create_dir_all(&directory)?;
    run(Command::new("tar")
        .arg("-xzf")
        .arg(file)
        .arg("-C")
        .arg(&directory))?;
    let result = replace_file(&directory.join("sozo"), exe);
    let _ = fs::remove_dir_all(&directory);
    result
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
    let moved = env::temp_dir().join(format!("sozo-old-{}.exe", process::id()));
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
    let root = Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|output| output.stdout.trim_ascii() == b"0");
    if root {
        Command::new(program)
    } else {
        let mut command = Command::new("sudo");
        command.arg(program);
        command
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
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

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
