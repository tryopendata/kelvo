//! The macOS [`ProcessOs`]: the processes collector's libproc helpers for name, start time
//! and responsible PID, `NSRunningApplication` for regular apps and `kill(2)` for
//! everything else (D-029).

use std::ffi::c_int;

use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication};

use super::libproc;
use crate::process_control::{OsError, ProcessInfo, ProcessOs, StopKind, signals_available};

/// The real OS.
pub struct MacProcessOs;

impl ProcessOs for MacProcessOs {
    fn self_pid(&self) -> i32 {
        // SAFETY: getpid has no preconditions and cannot fail.
        unsafe { libc::getpid() }
    }

    fn info(&self, pid: i32) -> Result<ProcessInfo, OsError> {
        if !signals_available() {
            return Err(OsError::Unavailable);
        }
        let Some(bsd) = libproc::bsd_info(pid) else {
            // libproc refuses other users' processes without saying why in a way callers
            // can rely on; `kill(pid, 0)` answers exactly "exists?" and "may I signal?".
            return Err(match kill(pid, 0) {
                Ok(()) => OsError::Other(format!("cannot read process {pid}")),
                Err(e) => e,
            });
        };
        Ok(ProcessInfo {
            name: libproc::name_of(&bsd).into_owned(),
            start_time_us: libproc::start_time_us(&bsd),
            responsible_pid: libproc::responsible_pid(pid),
        })
    }

    fn send(&self, pid: i32, kind: StopKind) -> Result<(), OsError> {
        if !signals_available() {
            return Err(OsError::Unavailable);
        }
        // A regular app gets the AppKit request (Quit runs its own quit path, so it can
        // ask to save). Agents and daemons with an NSRunningApplication do not reliably
        // handle that, so they get the signal like any other process.
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular);
        if let Some(app) = app {
            let sent = match kind {
                StopKind::Quit => app.terminate(),
                StopKind::ForceQuit => app.forceTerminate(),
            };
            if sent {
                return Ok(());
            }
            // NO means it could not be sent (exited, or still launching): fall through,
            // so kill(2) reports ESRCH or delivers the signal.
        }
        let sig = match kind {
            StopKind::Quit => libc::SIGTERM,
            StopKind::ForceQuit => libc::SIGKILL,
        };
        kill(pid, sig)
    }
}

fn kill(pid: i32, sig: c_int) -> Result<(), OsError> {
    // SAFETY: plain syscall; the caller guarantees pid > 1, so it never targets a group.
    if unsafe { libc::kill(pid, sig) } == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    Err(match err.raw_os_error() {
        Some(libc::ESRCH) => OsError::NoSuchProcess,
        Some(libc::EPERM) => OsError::PermissionDenied,
        _ => OsError::Other(err.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::macos::processes::Processes;
    use crate::{Collector, Probe, SampleBuf, Tick};

    #[test]
    fn reads_own_process() {
        let os = MacProcessOs;
        let me = os.self_pid();
        let info = os.info(me).unwrap();
        assert!(!info.name.is_empty());
        assert!(info.start_time_us > 1_600_000_000_000_000);
    }

    /// The UI sends back the `start_time_us` of the row the user picked, and Quit refuses
    /// with "PID reused" unless `info` reads the same value. Both must come from one
    /// formula.
    #[test]
    fn info_start_time_matches_the_processes_collector() {
        let me = MacProcessOs.self_pid();
        let mut c = Processes::new();
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        // A row needs a previous sample to compute its rates, so sample twice.
        for n in 0..2 {
            buf.clear();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: (n + 1) * 1_000_000_000,
                interval_ms: 1_000,
            };
            c.sample(&tick, &mut buf).unwrap();
        }
        let row = buf
            .processes()
            .iter()
            .find(|p| p.pid == me)
            .expect("the collector lists the test process");
        let info = MacProcessOs.info(me).unwrap();
        assert_eq!(row.start_time_us, info.start_time_us);
        assert_eq!(&*row.name, info.name);
    }

    fn gone(pid: i32) -> bool {
        // A killed child stays a zombie until reaped; libproc still lists it, so wait on it.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(kill(pid, 0), Err(OsError::NoSuchProcess)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    #[ignore = "spawns and signals real processes; run by hand"]
    fn live_quit_and_force_quit_a_child() {
        let os = MacProcessOs;
        for kind in [StopKind::Quit, StopKind::ForceQuit] {
            let mut child = Command::new("sleep")
                .arg("600")
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            let pid = child.id() as i32;
            assert!(os.info(pid).is_ok());
            assert_eq!(os.send(pid, kind), Ok(()));
            let status = child.wait().unwrap();
            assert!(!status.success(), "{kind:?}: {status:?}");
            assert!(gone(pid), "{kind:?}: pid {pid} still exists");
            assert_eq!(os.send(pid, kind), Err(OsError::NoSuchProcess));
        }
    }

    #[test]
    #[ignore = "needs a root-owned process on a real Mac; run by hand, never as root"]
    fn live_another_users_process_is_permission_denied() {
        // SAFETY: getuid has no preconditions.
        assert_ne!(unsafe { libc::getuid() }, 0, "run as a normal user");
        // syslogd always runs as root.
        let out = Command::new("pgrep")
            .args(["-x", "-U", "root", "syslogd"])
            .output()
            .unwrap();
        let pid: i32 = String::from_utf8(out.stdout)
            .unwrap()
            .trim()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(os_info_error(pid), OsError::PermissionDenied);
    }

    fn os_info_error(pid: i32) -> OsError {
        MacProcessOs.info(pid).unwrap_err()
    }
}
