//! Reading the two clocks a tick carries: wall time for timestamps and bucket alignment,
//! and a monotonic clock that keeps counting through sleep for spans.

use std::time::{SystemTime, UNIX_EPOCH};

/// One reading of both clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockReading {
    /// Millisecond Unix epoch.
    pub wall_ms: i64,
    /// Monotonic nanoseconds that advance during sleep (`CLOCK_MONOTONIC_RAW` on macOS,
    /// which is `mach_continuous_time`; `CLOCK_BOOTTIME` on Linux).
    pub continuous_ns: u64,
}

impl ClockReading {
    /// Milliseconds of continuous time from `earlier` to `self` (0 if it went backwards).
    pub fn continuous_ms_since(&self, earlier: &ClockReading) -> i64 {
        let ns = self.continuous_ns.saturating_sub(earlier.continuous_ns);
        i64::try_from(ns / 1_000_000).unwrap_or(i64::MAX)
    }
}

/// The current wall time in milliseconds.
pub fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
const CONTINUOUS_CLOCK: libc::clockid_t = libc::CLOCK_MONOTONIC_RAW;
#[cfg(not(target_os = "macos"))]
const CONTINUOUS_CLOCK: libc::clockid_t = libc::CLOCK_BOOTTIME;

/// Monotonic nanoseconds that keep advancing while the machine sleeps.
pub fn continuous_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid out-pointer for the duration of the call; the clock id is a
    // constant the platform defines.
    let rc = unsafe { libc::clock_gettime(CONTINUOUS_CLOCK, &mut ts) };
    if rc != 0 {
        return 0;
    }
    let secs = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    secs.saturating_mul(1_000_000_000).saturating_add(nanos)
}

/// Both clocks, now.
pub fn now() -> ClockReading {
    ClockReading {
        wall_ms: wall_ms(),
        continuous_ns: continuous_ns(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_clock_advances() {
        let a = now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = now();
        assert!(b.continuous_ns > a.continuous_ns);
        assert!(b.continuous_ms_since(&a) >= 4);
        assert_eq!(a.continuous_ms_since(&b), 0);
    }
}
