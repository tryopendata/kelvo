//! Low-disk guard (D-057): when the volume holding the store runs low on free space,
//! stop persisting the 10 s tier and resume once space recovers.
//!
//! The guard is checked on the shell's schedule (every few minutes and after each
//! prune), never per tick. Free space comes through [`FreeSpace`] so tests can drive it.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::writer::Writer;

/// Free and total bytes of one volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeSpace {
    /// Bytes an unprivileged process can still write.
    pub available_bytes: u64,
    pub total_bytes: u64,
}

/// Where the guard reads free space from.
pub trait FreeSpace: Send {
    fn volume_space(&self, path: &Path) -> io::Result<VolumeSpace>;
}

/// The real volume, through `statvfs`. On APFS `f_bavail` leaves out purgeable space,
/// so this errs towards "less free" than Finder shows.
#[derive(Clone, Copy, Debug, Default)]
pub struct StatVfs;

impl FreeSpace for StatVfs {
    fn volume_space(&self, path: &Path) -> io::Result<VolumeSpace> {
        let st = rustix::fs::statvfs(path)?;
        Ok(VolumeSpace {
            available_bytes: st.f_bavail.saturating_mul(st.f_frsize),
            total_bytes: st.f_blocks.saturating_mul(st.f_frsize),
        })
    }
}

/// Absolute part of the low-disk threshold.
pub const LOW_DISK_BYTES: u64 = 2_000_000_000;
/// Relative part of the low-disk threshold, in percent of the volume.
pub const LOW_DISK_PERCENT: u64 = 5;

/// S10 pauses when available space drops under the smaller of 2 GB and 5% of the volume.
pub fn low_disk_threshold(total_bytes: u64) -> u64 {
    LOW_DISK_BYTES.min(total_bytes / 100 * LOW_DISK_PERCENT)
}

/// S10 resumes only above 1.5 times the threshold, so a volume hovering at the line does
/// not flip the mode on every check.
pub fn resume_threshold(total_bytes: u64) -> u64 {
    low_disk_threshold(total_bytes) / 2 * 3
}

/// Pauses and resumes S10 persistence on `writer` from the free space of the volume
/// holding `path`.
pub struct LowDiskGuard<F: FreeSpace> {
    space: F,
    path: PathBuf,
    writer: Writer,
}

impl<F: FreeSpace> LowDiskGuard<F> {
    pub fn new(space: F, path: impl Into<PathBuf>, writer: Writer) -> Self {
        Self {
            space,
            path: path.into(),
            writer,
        }
    }

    /// Whether S10 is paused right now.
    pub fn paused(&self) -> bool {
        self.writer.s10_paused()
    }

    /// Reads free space and pauses or resumes S10. Returns whether it is paused after the
    /// check. If free space cannot be read, the mode stays as it was and the error is
    /// returned for the caller to log.
    pub fn check(&mut self, now_ms: i64) -> Result<bool> {
        let space = self.space.volume_space(&self.path)?;
        let paused = self.writer.s10_paused();
        if !paused && space.available_bytes < low_disk_threshold(space.total_bytes) {
            tracing::warn!(
                available = space.available_bytes,
                total = space.total_bytes,
                "disk almost full: pausing 10 s history; minute history and live values continue"
            );
            self.writer.set_s10_paused(true, now_ms)?;
        } else if paused && space.available_bytes >= resume_threshold(space.total_bytes) {
            tracing::info!(
                available = space.available_bytes,
                total = space.total_bytes,
                "disk space recovered: resuming 10 s history"
            );
            self.writer.set_s10_paused(false, now_ms)?;
        }
        Ok(self.writer.s10_paused())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_the_smaller_of_two_gigabytes_and_five_percent() {
        // 1 TB volume: 5% is 50 GB, so 2 GB wins.
        assert_eq!(low_disk_threshold(1_000_000_000_000), 2_000_000_000);
        // 20 GB volume: 5% is 1 GB.
        assert_eq!(low_disk_threshold(20_000_000_000), 1_000_000_000);
        assert_eq!(resume_threshold(1_000_000_000_000), 3_000_000_000);
    }

    #[test]
    fn statvfs_reads_the_temp_volume() {
        let space = StatVfs
            .volume_space(&std::env::temp_dir())
            .expect("statvfs on the temp dir");
        assert!(space.total_bytes > 0);
        assert!(space.available_bytes <= space.total_bytes);
    }
}
