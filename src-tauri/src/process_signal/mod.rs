//! Quit and Force Quit for the Processes page (D-029, v1-local-monitor.md 4.14).
//!
//! [`signal_process`] holds the guardrails and is OS-agnostic: it reads the target through
//! a [`ProcessOs`], so tests drive it with a fake. The real one, [`SystemProcessOs`],
//! lives in `kelvo-collect` beside the processes collector, so the start time it reads is
//! the collector's own formula (D-065).
//!
//! The sandboxed `appstore` edition cannot signal other processes:
//! [`signals_available`] is false there, the command answers `unavailable`, and
//! `get_edition` tells the UI to hide the actions. In order, it refuses PIDs at or below 1
//! and Kelvo's own PID, re-reads the process and fails with `PidReused` when its start
//! time differs from the one the user picked, refuses `kernel_task`, `launchd`,
//! `WindowServer`, `loginwindow` and Kelvo's own processes (its WebKit helpers), and only
//! then signals. `EPERM` becomes `PermissionDenied`; Kelvo never escalates.
//!
//! The refuse list is [`SignalRefusal::of`]. Live process rows carry its answer
//! (`LiveProcess::refusal`, D-092), so the UI disables the actions with the same rule.
//!
//! The start-time check and the signal are two calls, so a PID could in principle be
//! reused between them. That window is microseconds against PID reuse that takes a full
//! wrap of the PID space; the check exists for the seconds between a click and a confirm.

pub use kelvo_engine::process_control::{
    OsError, ProcessInfo, ProcessOs, StopKind, SystemProcessOs, signals_available,
};
use kelvo_schema::{HostId, JsSafeInt};
use serde::{Deserialize, Serialize};

/// What to ask of the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// `NSRunningApplication.terminate` for a regular app, `SIGTERM` otherwise.
    Quit,
    /// `NSRunningApplication.forceTerminate` for a regular app, `SIGKILL` otherwise.
    ForceQuit,
}

impl From<SignalKind> for StopKind {
    fn from(k: SignalKind) -> Self {
        match k {
            SignalKind::Quit => StopKind::Quit,
            SignalKind::ForceQuit => StopKind::ForceQuit,
        }
    }
}

/// A process start time in microseconds since the Unix epoch, as `LiveProcess` carries
/// it, passed as a plain JS `number` (D-039).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct Micros(#[specta(type = JsSafeInt)] pub i64);

/// Why `process_signal` did not signal. Serialized with a `kind` tag like `CommandError`;
/// the host variants share `CommandError`'s names and shapes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProcessSignalError {
    /// On the refuse list.
    #[error("refused: {refusal:?}")]
    Refused { refusal: SignalRefusal },
    /// The process exited before the signal was sent.
    #[error("no such process")]
    NotFound,
    /// The PID now belongs to a process with a different start time. Nothing was sent.
    #[error("the pid belongs to a different process now")]
    PidReused,
    /// `EPERM`: another user's process. Kelvo never escalates.
    #[error("permission denied")]
    PermissionDenied,
    /// No host with this id is registered.
    #[error("unknown host {host}")]
    UnknownHost { host: HostId },
    /// The host is not this Mac. Signals do not cross the wire in v1 (D-029).
    #[error("host {host} is not the local machine")]
    RemoteHost { host: HostId },
    /// This edition cannot signal processes (the sandboxed App Store build). The UI
    /// learns it up front from `get_edition` and should not offer the actions.
    #[error("not available in this edition")]
    Unavailable,
    /// The OS failed in a way none of the above covers.
    #[error("{message}")]
    Failed { message: String },
}

/// Why Kelvo will not signal a process. The UI words it (`process-signal.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SignalRefusal {
    /// The macOS kernel (pid 0), or a pid kill(2) would treat as a process group.
    KernelTask,
    Launchd,
    WindowServer,
    LoginWindow,
    /// Kelvo itself or one of its helpers.
    Kelvo,
}

impl SignalRefusal {
    /// Refused by pid alone, before the process is read: kill(2) treats 0 and negative
    /// pids as process groups, 1 is launchd, and `me` is Kelvo.
    fn by_pid(pid: i32, me: i32) -> Option<Self> {
        match pid {
            ..=0 => Some(Self::KernelTask),
            1 => Some(Self::Launchd),
            _ if pid == me => Some(Self::Kelvo),
            _ => None,
        }
    }

    /// Refused once the process is read: by name, or because it is one of Kelvo's own.
    fn by_process(name: &str, own: bool) -> Option<Self> {
        match name {
            "kernel_task" => Some(Self::KernelTask),
            "launchd" => Some(Self::Launchd),
            "WindowServer" => Some(Self::WindowServer),
            "loginwindow" => Some(Self::LoginWindow),
            _ if own => Some(Self::Kelvo),
            _ => None,
        }
    }

    /// Whether, and why, [`signal_process`] refuses `pid` named `name`, with `me` the
    /// calling process and `own` whether `pid` is one of Kelvo's own processes.
    pub fn of(pid: i32, name: &str, me: i32, own: bool) -> Option<Self> {
        Self::by_pid(pid, me).or_else(|| Self::by_process(name, own))
    }
}

/// [`SignalRefusal::of`] for rows of one batch, taking `(pid, start_time_us, name)`: this
/// process's pid and Kelvo's own processes are read once, here.
pub fn refusals() -> impl Fn(i32, i64, &str) -> Option<SignalRefusal> {
    let me = SystemProcessOs.self_pid();
    let own = kelvo_engine::process_control::own_processes();
    move |pid, start_time_us, name| {
        SignalRefusal::of(pid, name, me, own.contains(pid, start_time_us))
    }
}

fn map_os(e: OsError) -> ProcessSignalError {
    match e {
        OsError::NoSuchProcess => ProcessSignalError::NotFound,
        OsError::PermissionDenied => ProcessSignalError::PermissionDenied,
        OsError::Unavailable => ProcessSignalError::Unavailable,
        OsError::Other(message) => ProcessSignalError::Failed { message },
    }
}

/// Signals `pid` if it is still the process that started at `start_time_us` and is not on
/// the refuse list. See the module docs for the order of checks.
pub fn signal_process(
    os: &impl ProcessOs,
    pid: i32,
    start_time_us: i64,
    kind: SignalKind,
) -> Result<(), ProcessSignalError> {
    if !signals_available() {
        return Err(ProcessSignalError::Unavailable);
    }
    let me = os.self_pid();
    // Never let a process group (pid 0 or below) through to kill(2).
    if let Some(refusal) = SignalRefusal::by_pid(pid, me) {
        return Err(ProcessSignalError::Refused { refusal });
    }
    let info = os.info(pid).map_err(map_os)?;
    if info.start_time_us != start_time_us {
        return Err(ProcessSignalError::PidReused);
    }
    let own = info.responsible_pid == Some(me) || os.is_own(pid, info.start_time_us);
    if let Some(refusal) = SignalRefusal::by_process(&info.name, own) {
        return Err(ProcessSignalError::Refused { refusal });
    }
    os.send(pid, kind.into()).map_err(map_os)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    const ME: i32 = 500;

    #[derive(Default)]
    struct FakeOs {
        procs: HashMap<i32, Result<ProcessInfo, OsError>>,
        send_result: Option<OsError>,
        sent: RefCell<Vec<(i32, StopKind)>>,
        /// Kelvo's own processes, as the self-CPU coalition lists them.
        own: Vec<i32>,
    }

    impl FakeOs {
        fn with(mut self, pid: i32, name: &str, start: i64, responsible: Option<i32>) -> Self {
            self.procs.insert(
                pid,
                Ok(ProcessInfo {
                    name: name.to_owned(),
                    start_time_us: start,
                    responsible_pid: responsible,
                }),
            );
            self
        }

        fn sent(&self) -> Vec<(i32, StopKind)> {
            self.sent.borrow().clone()
        }
    }

    impl ProcessOs for FakeOs {
        fn self_pid(&self) -> i32 {
            ME
        }

        fn is_own(&self, pid: i32, _start_time_us: i64) -> bool {
            self.own.contains(&pid)
        }

        fn info(&self, pid: i32) -> Result<ProcessInfo, OsError> {
            self.procs
                .get(&pid)
                .cloned()
                .unwrap_or(Err(OsError::NoSuchProcess))
        }

        fn send(&self, pid: i32, kind: StopKind) -> Result<(), OsError> {
            if let Some(e) = &self.send_result {
                return Err(e.clone());
            }
            self.sent.borrow_mut().push((pid, kind));
            Ok(())
        }
    }

    fn is_refused(r: Result<(), ProcessSignalError>) -> bool {
        matches!(r, Err(ProcessSignalError::Refused { .. }))
    }

    #[test]
    fn signals_a_matching_process() {
        let os = FakeOs::default().with(4242, "Safari", 77, Some(4242));
        assert_eq!(signal_process(&os, 4242, 77, SignalKind::Quit), Ok(()));
        assert_eq!(signal_process(&os, 4242, 77, SignalKind::ForceQuit), Ok(()));
        assert_eq!(
            os.sent(),
            vec![(4242, StopKind::Quit), (4242, StopKind::ForceQuit)]
        );
    }

    #[test]
    fn refuses_every_listed_target_without_sending() {
        let os = FakeOs::default()
            .with(0, "kernel_task", 1, None)
            .with(1, "launchd", 1, None)
            .with(ME, "kelvo", 1, None)
            .with(300, "WindowServer", 1, None)
            .with(301, "loginwindow", 1, None)
            .with(302, "kernel_task", 1, None)
            .with(303, "launchd", 1, None)
            // A WebKit helper: Kelvo is responsible for it.
            .with(400, "com.apple.WebKit.WebContent", 1, Some(ME));
        for pid in [-1, 0, 1, ME, 300, 301, 302, 303, 400] {
            for kind in [SignalKind::Quit, SignalKind::ForceQuit] {
                assert!(
                    is_refused(signal_process(&os, pid, 1, kind)),
                    "pid {pid} was not refused"
                );
            }
        }
        assert!(os.sent().is_empty());
    }

    #[test]
    fn refusal_names_the_process() {
        let os = FakeOs::default().with(300, "WindowServer", 1, None);
        assert_eq!(
            signal_process(&os, 300, 1, SignalKind::Quit),
            Err(ProcessSignalError::Refused {
                refusal: SignalRefusal::WindowServer
            })
        );
    }

    /// A live row's refusal (`SignalRefusal::of`, from the row and the own-process list)
    /// is what `signal_process` answers for the same process.
    #[test]
    fn row_refusal_matches_signal_process() {
        let mut os = FakeOs::default()
            .with(0, "kernel_task", 1, None)
            .with(1, "launchd", 1, None)
            .with(ME, "kelvo", 1, None)
            .with(300, "WindowServer", 1, None)
            .with(301, "loginwindow", 1, None)
            .with(302, "kernel_task", 1, None)
            .with(303, "launchd", 1, Some(1))
            // A WebKit helper macOS holds Kelvo responsible for.
            .with(400, "com.apple.WebKit.WebContent", 1, Some(ME))
            // `tauri dev`: the terminal is responsible; the coalition still lists it.
            .with(401, "com.apple.WebKit.Networking", 1, Some(77))
            .with(4242, "Safari", 1, Some(4242))
            .with(4243, "Kelvo Helper", 1, Some(4243));
        os.own = vec![ME, 400, 401];
        for pid in [-1, 0, 1, ME, 300, 301, 302, 303, 400, 401, 4242, 4243] {
            let name = os.info(pid).map(|i| i.name).unwrap_or_default();
            let row = SignalRefusal::of(pid, &name, ME, os.own.contains(&pid));
            let signal = match signal_process(&os, pid, 1, SignalKind::Quit) {
                Err(ProcessSignalError::Refused { refusal }) => Some(refusal),
                Ok(()) => None,
                Err(e) => panic!("pid {pid}: {e}"),
            };
            assert_eq!(row, signal, "pid {pid} {name}");
        }
        assert_eq!(
            os.sent(),
            vec![(4242, StopKind::Quit), (4243, StopKind::Quit)]
        );
    }

    #[test]
    fn stale_start_time_is_pid_reused() {
        let os = FakeOs::default().with(4242, "Safari", 78, None);
        assert_eq!(
            signal_process(&os, 4242, 77, SignalKind::ForceQuit),
            Err(ProcessSignalError::PidReused)
        );
        assert!(os.sent().is_empty());
    }

    #[test]
    fn pid_reuse_is_reported_before_the_refuse_list() {
        // The row the user picked is gone; that is the more useful answer.
        let os = FakeOs::default().with(300, "WindowServer", 2, None);
        assert_eq!(
            signal_process(&os, 300, 1, SignalKind::Quit),
            Err(ProcessSignalError::PidReused)
        );
    }

    #[test]
    fn exited_process_is_not_found() {
        let os = FakeOs::default();
        assert_eq!(
            signal_process(&os, 4242, 77, SignalKind::Quit),
            Err(ProcessSignalError::NotFound)
        );
    }

    #[test]
    fn unreadable_process_of_another_user_is_permission_denied() {
        let mut os = FakeOs::default();
        os.procs.insert(88, Err(OsError::PermissionDenied));
        assert_eq!(
            signal_process(&os, 88, 1, SignalKind::Quit),
            Err(ProcessSignalError::PermissionDenied)
        );
        assert!(os.sent().is_empty());
    }

    #[test]
    fn eperm_from_the_signal_is_permission_denied() {
        let mut os = FakeOs::default().with(88, "syspolicyd", 5, None);
        os.send_result = Some(OsError::PermissionDenied);
        assert_eq!(
            signal_process(&os, 88, 5, SignalKind::ForceQuit),
            Err(ProcessSignalError::PermissionDenied)
        );
    }

    #[test]
    fn esrch_from_the_signal_is_not_found_and_other_errors_carry_a_message() {
        let mut os = FakeOs::default().with(88, "sleep", 5, None);
        os.send_result = Some(OsError::NoSuchProcess);
        assert_eq!(
            signal_process(&os, 88, 5, SignalKind::Quit),
            Err(ProcessSignalError::NotFound)
        );
        os.send_result = Some(OsError::Other("EINVAL".into()));
        assert_eq!(
            signal_process(&os, 88, 5, SignalKind::Quit),
            Err(ProcessSignalError::Failed {
                message: "EINVAL".into()
            })
        );
    }

    #[test]
    fn an_os_that_cannot_signal_is_unavailable() {
        let mut os = FakeOs::default();
        os.procs.insert(88, Err(OsError::Unavailable));
        assert_eq!(
            signal_process(&os, 88, 1, SignalKind::Quit),
            Err(ProcessSignalError::Unavailable)
        );
        let mut os = FakeOs::default().with(88, "sleep", 5, None);
        os.send_result = Some(OsError::Unavailable);
        assert_eq!(
            signal_process(&os, 88, 5, SignalKind::Quit),
            Err(ProcessSignalError::Unavailable)
        );
    }

    #[test]
    fn errors_serialize_with_the_kind_tag_the_ui_switches_on() {
        let json = |e: ProcessSignalError| serde_json::to_value(e).unwrap();
        assert_eq!(
            json(ProcessSignalError::PidReused),
            serde_json::json!({ "kind": "pid_reused" })
        );
        assert_eq!(
            json(ProcessSignalError::PermissionDenied),
            serde_json::json!({ "kind": "permission_denied" })
        );
        assert_eq!(
            json(ProcessSignalError::NotFound),
            serde_json::json!({ "kind": "not_found" })
        );
        assert_eq!(
            json(ProcessSignalError::Unavailable),
            serde_json::json!({ "kind": "unavailable" })
        );
        assert_eq!(
            json(ProcessSignalError::Refused {
                refusal: SignalRefusal::LoginWindow
            }),
            serde_json::json!({ "kind": "refused", "refusal": "login_window" })
        );
        assert_eq!(
            serde_json::from_str::<SignalKind>("\"force_quit\"").unwrap(),
            SignalKind::ForceQuit
        );
    }

    /// The guard against the real OS: a stale start time sends nothing and the right one
    /// stops the child.
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "spawns and signals real processes; run by hand"]
    fn live_guard_quits_and_force_quits_a_child() {
        use std::process::{Command, Stdio};

        let os = SystemProcessOs;
        for kind in [SignalKind::Quit, SignalKind::ForceQuit] {
            let mut child = Command::new("sleep")
                .arg("600")
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            let pid = child.id() as i32;
            let start = os.info(pid).unwrap().start_time_us;
            assert_eq!(
                signal_process(&os, pid, start - 1, kind),
                Err(ProcessSignalError::PidReused)
            );
            assert!(child.try_wait().unwrap().is_none(), "{kind:?}: child died");
            assert_eq!(signal_process(&os, pid, start, kind), Ok(()));
            assert!(!child.wait().unwrap().success(), "{kind:?}");
        }
    }
}
