//! Owner-only modes for what Kelvo keeps on disk (D-074): usage history, the host id and
//! settings are nobody else's business. Directories are created 0700 and files 0600;
//! existing ones with any group or other bits are tightened to owner-only, never loosened.
//!
//! The store applies this to its own files (the database, its `-wal` and `-shm`, the
//! lock). SQLite creates the `-wal` and `-shm` files with the database file's mode, so
//! creating the database 0600 before SQLite opens it covers them too.

use std::fs::{DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub const PRIVATE_DIR_MODE: u32 = 0o700;
pub const PRIVATE_FILE_MODE: u32 = 0o600;

/// Group and other permission bits.
const SHARED_BITS: u32 = 0o077;

/// Creates `dir` and any missing parents 0700, then tightens `dir` itself if it already
/// existed with a looser mode. Parents that already exist are left alone.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE_DIR_MODE)
        .create(dir)?;
    restrict(dir)
}

/// Removes group and other permissions from `path` if it has any. A missing path is not
/// an error.
pub fn restrict(path: &Path) -> io::Result<()> {
    match std::fs::metadata(path) {
        Ok(m) => tighten(m.permissions(), |p| std::fs::set_permissions(path, p)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Opens `path` for writing without truncating it, creating it 0600, and tightens it if
/// it already existed with a looser mode.
pub fn open_private(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(PRIVATE_FILE_MODE)
        .open(path)?;
    restrict_open(&file)?;
    Ok(file)
}

/// Writes `bytes` to `path`, creating it 0600 (tightening an existing looser file).
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(PRIVATE_FILE_MODE)
        .open(path)?;
    restrict_open(&file)?;
    file.write_all(bytes)
}

fn restrict_open(file: &File) -> io::Result<()> {
    tighten(file.metadata()?.permissions(), |p| file.set_permissions(p))
}

fn tighten(perms: Permissions, set: impl FnOnce(Permissions) -> io::Result<()>) -> io::Result<()> {
    let mode = perms.mode() & 0o7777;
    if mode & SHARED_BITS == 0 {
        return Ok(());
    }
    set(Permissions::from_mode(mode & !SHARED_BITS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn new_dirs_and_files_are_owner_only() {
        let base = crate::test_dir("perms-new");
        let dir = base.join("a").join("b");
        create_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&base.join("a")), 0o700, "created parents too");

        write_private(&dir.join("w"), b"x").unwrap();
        assert_eq!(mode(&dir.join("w")), 0o600);
        drop(open_private(&dir.join("o")).unwrap());
        assert_eq!(mode(&dir.join("o")), 0o600);
    }

    #[test]
    fn looser_existing_ones_are_tightened_and_tighter_ones_kept() {
        let base = crate::test_dir("perms-existing");
        let dir = base.join("d");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
        create_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);

        for name in ["w", "o", "r"] {
            std::fs::write(dir.join(name), b"old").unwrap();
            std::fs::set_permissions(dir.join(name), Permissions::from_mode(0o666)).unwrap();
        }
        write_private(&dir.join("w"), b"new").unwrap();
        drop(open_private(&dir.join("o")).unwrap());
        restrict(&dir.join("r")).unwrap();
        for name in ["w", "o", "r"] {
            assert_eq!(mode(&dir.join(name)), 0o600, "{name}");
        }
        assert_eq!(std::fs::read(dir.join("w")).unwrap(), b"new");
        assert_eq!(
            std::fs::read(dir.join("o")).unwrap(),
            b"old",
            "not truncated"
        );

        std::fs::set_permissions(dir.join("r"), Permissions::from_mode(0o400)).unwrap();
        restrict(&dir.join("r")).unwrap();
        assert_eq!(mode(&dir.join("r")), 0o400, "never loosened");
        restrict(&dir.join("missing")).unwrap();
    }

    fn store_files(db: &Path) -> Vec<std::path::PathBuf> {
        ["", "-wal", "-shm", ".lock"]
            .iter()
            .map(|s| {
                let mut p = db.as_os_str().to_owned();
                p.push(s);
                p.into()
            })
            .collect()
    }

    #[test]
    fn a_new_store_is_owner_only() {
        let db = crate::test_dir("perms-store").join("history.sqlite");
        let store = crate::Store::open(crate::StoreConfig::new(&db)).unwrap();
        store.writer().flush().unwrap();
        for f in store_files(&db) {
            assert!(f.exists(), "{}", f.display());
            assert_eq!(mode(&f), 0o600, "{}", f.display());
        }
        store.close().unwrap();
    }

    #[test]
    fn an_existing_store_is_tightened_on_open() {
        let db = crate::test_dir("perms-store-old").join("history.sqlite");
        let store = crate::Store::open(crate::StoreConfig::new(&db)).unwrap();
        store.writer().flush().unwrap();
        // As files from before D-074 were: 0644, with the WAL and shm still there.
        for f in store_files(&db) {
            std::fs::set_permissions(&f, Permissions::from_mode(0o644)).unwrap();
        }
        let copy = db.with_file_name("copy.sqlite");
        for (from, to) in store_files(&db).into_iter().zip(store_files(&copy)) {
            std::fs::copy(from, to).unwrap();
        }
        store.close().unwrap();

        let store = crate::Store::open(crate::StoreConfig::new(&copy)).unwrap();
        for f in store_files(&copy) {
            assert_eq!(mode(&f), 0o600, "{}", f.display());
        }
        store.close().unwrap();
    }
}
