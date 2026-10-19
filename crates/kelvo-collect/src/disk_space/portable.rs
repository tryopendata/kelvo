//! A `Lister` on `std::fs`: Linux, and tests on any Unix. macOS uses `getattrlistbulk`
//! (`macos::space`), several times faster on large trees.
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::walk::{ListError, ListedFile, Lister, Listing};

#[derive(Default)]
pub struct StdLister;

impl Lister for StdLister {
    fn list(&mut self, dir: &Path, dev: u64, out: &mut Listing) -> Result<(), ListError> {
        let meta = std::fs::symlink_metadata(dir).map_err(|_| ListError::Unreadable)?;
        if meta.dev() != dev {
            return Err(ListError::OtherDevice);
        }
        let entries = std::fs::read_dir(dir).map_err(|_| ListError::Unreadable)?;
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue; // Removed since the listing.
            };
            let name: Box<str> = entry.file_name().to_string_lossy().into();
            if meta.is_dir() {
                out.dirs.push(name);
            } else {
                out.files.push(ListedFile {
                    name,
                    bytes: meta.blocks().saturating_mul(512),
                    linked: (meta.nlink() > 1).then(|| meta.ino()),
                });
            }
        }
        Ok(())
    }
}
