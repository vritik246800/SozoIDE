//! Where an update is downloaded, mounted and unpacked: a folder made at that moment inside the
//! user's cache folder, never in the shared temporary folder, so nobody else can put a file or a
//! link where the package goes between its checksum and the installer (`SECURITY.md` §6.1).

use std::{
    collections::hash_map::RandomState,
    fs,
    hash::{BuildHasher, Hasher},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;

/// A folder left by an update that never finished is removed after a day.
const STALE: Duration = Duration::from_secs(24 * 60 * 60);

pub(super) fn base() -> Result<PathBuf> {
    ProjectDirs::from("", "", "SozoIDE")
        .map(|directories| directories.cache_dir().join("updates"))
        .context("Could not find the cache folder of this user")
}

/// A new folder inside `base` that only this user can read: it fails rather than reuse one.
pub(super) fn private_dir(base: &Path) -> Result<PathBuf> {
    fs::create_dir_all(base).with_context(|| format!("Could not create {}", base.display()))?;
    if !fs::symlink_metadata(base)?.is_dir() {
        bail!(
            "{} is not a folder: the update was not downloaded",
            base.display()
        );
    }
    owner_only(base)?;
    clear_stale(base);
    let directory = base.join(format!("update-{:016x}", random()));
    create(&directory).with_context(|| format!("Could not create {}", directory.display()))?;
    Ok(directory)
}

/// A folder inside the private one, for a mount or an unpacked archive.
pub(super) fn inside(file: &Path, name: &str) -> Result<PathBuf> {
    let directory = file
        .parent()
        .context("The update has no folder")?
        .join(name);
    create(&directory).with_context(|| format!("Could not create {}", directory.display()))?;
    Ok(directory)
}

/// Nothing may be where the package is about to be written, not even a link.
pub(super) fn free(file: &Path) -> Result<()> {
    if fs::symlink_metadata(file).is_ok() {
        bail!(
            "{} already exists: the update was not downloaded",
            file.display()
        );
    }
    Ok(())
}

/// Removes the folder of an update, and only that: a file anywhere else is left alone.
pub(super) fn discard(file: &Path) {
    let _ = fs::remove_file(file);
    if let Some(directory) = file.parent()
        && directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("update-"))
    {
        let _ = fs::remove_dir_all(directory);
    }
}

#[cfg(unix)]
fn create(directory: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(directory)
}

/// The cache folder of a Windows user is already closed to the others by its ACL.
#[cfg(not(unix))]
fn create(directory: &Path) -> std::io::Result<()> {
    fs::create_dir(directory)
}

#[cfg(unix)]
fn owner_only(directory: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("Could not close {} to other users", directory.display()))
}

#[cfg(not(unix))]
fn owner_only(_: &Path) -> Result<()> {
    Ok(())
}

fn clear_stale(base: &Path) {
    let old = |entry: &fs::DirEntry| {
        entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > STALE)
    };
    for entry in fs::read_dir(base).into_iter().flatten().flatten() {
        if old(&entry) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// The standard library seeds every `RandomState` from the system: enough for a name nobody
/// can guess, with no crate for it.
fn random() -> u64 {
    RandomState::new().build_hasher().finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_update_gets_a_new_folder_only_its_user_can_open() {
        let cache = tempfile::tempdir().unwrap();
        let base = cache.path().join("updates");
        let first = private_dir(&base).unwrap();
        let second = private_dir(&base).unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with(&base) && first.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for directory in [&base, &first] {
                let mode = fs::metadata(directory).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o700, "{}", directory.display());
            }
        }
        // A folder that is already there is never reused.
        assert!(create(&first).is_err());
        assert!(inside(&first.join("package"), "mount").is_ok());
        assert!(inside(&first.join("package"), "mount").is_err());
    }

    #[test]
    fn a_file_or_a_link_in_the_way_refuses_the_download() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("package");
        assert!(free(&file).is_ok());
        fs::write(&file, "theirs").unwrap();
        assert!(free(&file).is_err());
        #[cfg(unix)]
        {
            let link = directory.path().join("link");
            // Also a link to nowhere, which `exists()` would not see.
            std::os::unix::fs::symlink(directory.path().join("gone"), &link).unwrap();
            assert!(free(&link).is_err());
            // A link in the place of the updates folder is not followed.
            let theirs = directory.path().join("theirs");
            fs::create_dir(&theirs).unwrap();
            let base = directory.path().join("updates");
            std::os::unix::fs::symlink(&theirs, &base).unwrap();
            assert!(private_dir(&base).is_err());
            assert_eq!(fs::read_dir(&theirs).unwrap().count(), 0);
        }
    }

    #[test]
    fn only_the_folder_of_an_update_is_removed_with_it() {
        let cache = tempfile::tempdir().unwrap();
        let directory = private_dir(cache.path()).unwrap();
        let package = directory.join("package");
        fs::write(&package, "abc").unwrap();
        discard(&package);
        assert!(!directory.exists());

        let other = cache.path().join("documents");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("package"), "abc").unwrap();
        fs::write(other.join("letter"), "abc").unwrap();
        discard(&other.join("package"));
        assert!(other.join("letter").exists());
    }
}
