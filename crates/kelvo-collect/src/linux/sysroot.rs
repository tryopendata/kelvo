//! Where the Linux collectors read `/proc` and `/sys`. Every reader takes a [`SysRoot`] so
//! tests point it at a captured fixture tree (`linux/fixtures/`) instead of the live host.

use std::io;
use std::path::{Path, PathBuf};

/// The roots of procfs and sysfs, plus the filesystem root for `/etc` and `/run`.
#[derive(Clone, Debug)]
pub struct SysRoot {
    root: PathBuf,
}

impl SysRoot {
    /// The live host.
    pub fn host() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }

    /// A fixture tree laid out like `/` (`<dir>/proc/stat`, `<dir>/sys/class/...`).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { root: dir.into() }
    }

    /// `rel` resolved under the root; `rel` is written absolute (`"/proc/stat"`).
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_start_matches('/'))
    }

    /// `rel`'s contents. procfs reports zero sizes, so this reads to the end rather than
    /// trusting metadata.
    pub fn read(&self, rel: &str) -> io::Result<String> {
        std::fs::read_to_string(self.path(rel))
    }

    /// `rel`'s contents, trimmed, or `None` when unreadable or empty.
    pub fn read_trimmed(&self, rel: &str) -> Option<String> {
        let s = self.read(rel).ok()?;
        let s = s.trim();
        (!s.is_empty()).then(|| s.to_owned())
    }

    /// `rel` parsed as an integer after trimming.
    pub fn read_u64(&self, rel: &str) -> Option<u64> {
        self.read_trimmed(rel)?.parse().ok()
    }

    /// The root itself, for directory walks.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// System page size in bytes. Asahi kernels use 16 KiB pages, so this is never assumed.
pub fn page_size() -> u64 {
    // SAFETY: sysconf has no preconditions; a negative result means unsupported.
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    u64::try_from(n).ok().filter(|&n| n > 0).unwrap_or(4096)
}

/// Clock ticks per second for `/proc` jiffies (`USER_HZ`).
pub fn clk_tck() -> u64 {
    // SAFETY: sysconf has no preconditions; a negative result means unsupported.
    let n = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    u64::try_from(n).ok().filter(|&n| n > 0).unwrap_or(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_resolve_under_the_root() {
        let r = SysRoot::at("/tmp/fx");
        assert_eq!(r.path("/proc/stat"), PathBuf::from("/tmp/fx/proc/stat"));
        assert_eq!(
            SysRoot::host().path("/proc/stat"),
            PathBuf::from("/proc/stat")
        );
    }

    #[test]
    fn sysconf_values_are_positive() {
        assert!(page_size() >= 4096);
        assert!(clk_tck() > 0);
    }
}
