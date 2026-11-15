//! The files of the user's settings folder (`sessions.toml`, `keys.toml`, `update.toml`,
//! `connections.toml`): written whole or not at all, closed to other users, and kept aside
//! when they cannot be read (`SECURITY.md` §2).

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::de::DeserializeOwned;

/// Writes to a file beside `path` and renames it over: a crash or a full disk leaves the old
/// file, never half of the new one.
pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let temporary = beside(path, "tmp");
    let _ = fs::remove_file(&temporary);
    let written = create(&temporary)
        .and_then(|mut file| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// What the file holds, or the default when there is none. A file that cannot be understood
/// moves to `<name>.broken`, so the next save does not write over the only copy.
pub fn read<T: DeserializeOwned + Default>(path: &Path) -> T {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return T::default(),
        Err(_) => String::from("\0"),
    };
    toml::from_str(&text).unwrap_or_else(|_| {
        let _ = fs::rename(path, beside(path, "broken"));
        T::default()
    })
}

/// The folder of this user where SozoIDE puts what it builds and then runs (`run`, `debug`):
/// inside the cache folder and closed to other users, never the shared temporary folder,
/// where someone else could put a program in the place of the one just compiled (R27).
pub fn private_folder(name: &str) -> PathBuf {
    let base = directories::ProjectDirs::from("", "", "SozoIDE")
        .map_or_else(std::env::temp_dir, |directories| {
            directories.cache_dir().to_path_buf()
        });
    private_folder_in(&base, name)
}

fn private_folder_in(base: &Path, name: &str) -> PathBuf {
    let folder = base.join(name);
    if fs::create_dir_all(&folder).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&folder, fs::Permissions::from_mode(0o700));
        }
    }
    folder
}

fn beside(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.{suffix}"))
}

/// A new file that only its user reads; never one that is already there, nor a link.
fn create(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    type Settings = BTreeMap<String, u16>;

    #[test]
    fn settings_are_written_whole_and_closed_to_other_users() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("deep/settings.toml");
        write(&path, "width = 30\n").unwrap();
        assert_eq!(read::<Settings>(&path)["width"], 30);

        // A file left open to everyone by an older version is closed by the next save.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            write(&path, "width = 31\n").unwrap();
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let names = |folder: &Path| {
            let mut names: Vec<_> = fs::read_dir(folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        };
        assert_eq!(names(path.parent().unwrap()), ["settings.toml"]);

        // A write that cannot finish leaves the file as it was and nothing beside it.
        let before = fs::read_to_string(&path).unwrap();
        fs::create_dir(beside(&path, "tmp")).unwrap();
        assert!(write(&path, "width = 99\n").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::remove_dir(beside(&path, "tmp")).unwrap();
        let folder = directory.path().join("folder.toml");
        fs::create_dir_all(folder.join("inside")).unwrap();
        assert!(write(&folder, "width = 1\n").is_err());
        assert_eq!(names(directory.path()), ["deep", "folder.toml"]);
    }

    #[test]
    fn what_is_built_to_run_goes_to_a_folder_of_this_user_alone() {
        let cache = tempfile::tempdir().unwrap();
        let folder = private_folder_in(cache.path(), "run");
        assert_eq!(folder, cache.path().join("run"));
        assert!(folder.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Also one that was there, open to everyone.
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o777)).unwrap();
            private_folder_in(cache.path(), "run");
            let mode = fs::metadata(&folder).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    #[test]
    fn a_broken_file_is_kept_aside_and_read_as_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        assert!(read::<Settings>(&path).is_empty());
        assert!(!beside(&path, "broken").exists());

        for broken in [
            &b"width = [unfinished"[..],
            b"width = \"wide\"\n",
            b"width = 99999999999999999999\n",
            b"\xff\xfe\x00not text",
        ] {
            fs::write(&path, broken).unwrap();
            assert!(read::<Settings>(&path).is_empty());
            assert_eq!(fs::read(beside(&path, "broken")).unwrap(), broken);
            assert!(!path.exists());
        }
        // The next save starts a new file; the broken one stays to be looked at.
        write(&path, "width = 30\n").unwrap();
        assert_eq!(read::<Settings>(&path)["width"], 30);
        assert!(beside(&path, "broken").exists());
    }
}
