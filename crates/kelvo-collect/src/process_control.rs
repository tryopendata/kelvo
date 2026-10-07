//! The OS side of Quit and Force Quit (D-029): reading a process's identity and sending
//! it a stop request. The guardrails (refuse list, start-time check) live in the app
//! shell's `process_signal`, which reads and signals through [`ProcessOs`].
//!
//! The macOS implementation reads through the same libproc helpers as the processes
//! collector, so the `start_time_us` a `ProcessSample` carries and the one
//! [`ProcessOs::info`] returns come from one formula. The UI sends back the start time of
//! the row the user picked; two formulas that drifted apart would turn every Quit into a
//! "PID reused" refusal.
//!
//! An `appstore` build is sandboxed and may not signal processes outside its sandbox, so
//! [`signals_available`] is false there and every call answers [`OsError::Unavailable`].

use std::sync::{Arc, Mutex, OnceLock};

/// What to ask of the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopKind {
    /// `NSRunningApplication.terminate` for a regular app, `SIGTERM` otherwise.
    Quit,
    /// `NSRunningApplication.forceTerminate` for a regular app, `SIGKILL` otherwise.
    ForceQuit,
}

/// What the shell's guard needs to know about a live process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub name: String,
    /// Microseconds since the Unix epoch, the same value the processes collector reports.
    pub start_time_us: i64,
    /// The process macOS holds responsible for this one (an app for its XPC helpers), when
    /// known.
    pub responsible_pid: Option<i32>,
}

/// How an OS call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsError {
    /// `ESRCH`.
    NoSuchProcess,
    /// `EPERM`.
    PermissionDenied,
    /// This build cannot signal processes (the sandboxed `appstore` edition, or a platform
    /// without an implementation).
    Unavailable,
    Other(String),
}

/// The OS calls behind Quit and Force Quit. [`SystemProcessOs`] is the real one; the
/// shell's tests use a fake that records what was sent.
pub trait ProcessOs {
    /// The calling process's PID.
    fn self_pid(&self) -> i32;
    /// Name, start time and responsible PID of `pid`.
    fn info(&self, pid: i32) -> Result<ProcessInfo, OsError>;
    /// Delivers `kind` to `pid`. The caller guarantees `pid > 1`.
    fn send(&self, pid: i32, kind: StopKind) -> Result<(), OsError>;
    /// Whether `pid`, started at `start_time_us`, is one of Kelvo's own processes
    /// ([`own_processes`]).
    fn is_own(&self, pid: i32, start_time_us: i64) -> bool {
        own_processes().contains(pid, start_time_us)
    }
}

/// Kelvo's own processes as one listing found them: the app and the helpers macOS holds
/// it responsible for. The process table marks them refused without a syscall per row
/// (D-092). Cheap to clone: readers take a copy and never hold a lock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwnProcesses {
    /// Sorted.
    pids: Arc<[i32]>,
    /// When the listing began, µs since the Unix epoch, the unit of
    /// `ProcessSample::start_time_us`.
    listed_at_us: i64,
}

impl OwnProcesses {
    /// Whether `pid`, started at `start_time_us`, was on the listing. A process that
    /// started after the listing began cannot have been on it: its pid was reused after
    /// one of Kelvo's helpers exited.
    pub fn contains(&self, pid: i32, start_time_us: i64) -> bool {
        start_time_us <= self.listed_at_us && self.pids.binary_search(&pid).is_ok()
    }
}

/// Where the `self.cpu` collector publishes [`OwnProcesses`] (every 10 s, from its cached
/// coalition) and the shell reads them. The app uses [`OwnProcessList::global`]; a test
/// makes its own, so no test sees another's list.
#[derive(Clone, Debug, Default)]
pub struct OwnProcessList(Arc<Mutex<OwnProcesses>>);

impl OwnProcessList {
    /// The app's list: what [`own_processes`] reads.
    pub fn global() -> &'static OwnProcessList {
        static GLOBAL: OnceLock<OwnProcessList> = OnceLock::new();
        GLOBAL.get_or_init(OwnProcessList::default)
    }

    /// Records a listing that began at `listed_at_us`. Sorts `pids` in place; the list
    /// allocates only when its members changed.
    pub fn record(&self, listed_at_us: i64, pids: &mut [i32]) {
        pids.sort_unstable();
        let mut own = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if *own.pids != *pids {
            own.pids = Arc::from(&*pids);
        }
        own.listed_at_us = listed_at_us;
    }

    /// The list as last recorded; empty before the first record.
    pub fn get(&self) -> OwnProcesses {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The app's own-process list as last recorded; empty before the first `self.cpu` sample
/// and where that collector does not run.
pub fn own_processes() -> OwnProcesses {
    OwnProcessList::global().get()
}

/// Whether this build can signal other processes: false in the sandboxed `appstore`
/// edition and on platforms without an implementation. The shell reports it to the UI,
/// which hides Quit and Force Quit when it is false.
pub const fn signals_available() -> bool {
    cfg!(all(target_os = "macos", not(feature = "appstore")))
}

#[cfg(not(target_os = "macos"))]
pub use self::unsupported::UnsupportedProcessOs as SystemProcessOs;
#[cfg(target_os = "macos")]
pub use crate::macos::process_control::MacProcessOs as SystemProcessOs;

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use super::{OsError, ProcessInfo, ProcessOs, StopKind};

    /// Process signals are macOS-only until the v4 agent needs them.
    pub struct UnsupportedProcessOs;

    impl ProcessOs for UnsupportedProcessOs {
        fn self_pid(&self) -> i32 {
            std::process::id().try_into().unwrap_or(i32::MAX)
        }

        fn info(&self, _pid: i32) -> Result<ProcessInfo, OsError> {
            Err(OsError::Unavailable)
        }

        fn send(&self, _pid: i32, _kind: StopKind) -> Result<(), OsError> {
            Err(OsError::Unavailable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_started_after_the_listing_is_not_on_it() {
        let list = OwnProcessList::default();
        assert!(!list.get().contains(7, 0), "empty before the first listing");
        list.record(1_000, &mut [9, 7]);
        let own = list.get();
        assert!(own.contains(7, 1_000));
        assert!(own.contains(9, 10));
        assert!(!own.contains(7, 1_001), "pid 7 reused after the listing");
        assert!(!own.contains(8, 10));
    }

    #[test]
    fn readers_keep_the_listing_they_took() {
        let list = OwnProcessList::default();
        list.record(1_000, &mut [3, 1]);
        let before = list.get();
        list.record(2_000, &mut [1, 3]);
        let same = list.get();
        assert!(
            Arc::ptr_eq(&before.pids, &same.pids),
            "unchanged members reuse it"
        );
        list.record(3_000, &mut [1]);
        assert!(
            before.contains(3, 500),
            "a copy taken earlier is not changed"
        );
        assert!(!list.get().contains(3, 500));
    }
}
