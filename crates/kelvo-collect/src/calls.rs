//! Counts of OS calls per API family, for the per-tick call ceilings (D-062).
//!
//! With the `call-counters` feature, every IOKit, SMC, IOReport, HID, libproc,
//! sysctl/Mach and NetworkStatistics call site in the macOS collectors bumps a per-thread
//! counter. The engine samples collectors on one thread, so a test that drives the engine
//! on its own thread reads exactly that engine's calls. Without the feature, [`count`] compiles to nothing
//! and [`current`] is all zeros. Only tests turn it on (a dev-dependency feature, which
//! resolver 3 keeps out of normal builds).

/// An API family. A count is one call into the OS: a registry property read, an SMC key
/// read, an IOReport sample, a HID event copy, a `proc_*` call, a `sysctl` or Mach host
/// statistics call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Api {
    /// IOKit registry and service calls, and IOPowerSources.
    IoKit,
    /// `IOConnectCallStructMethod` on the SMC user client (one per key read or key info).
    Smc,
    /// `IOReportCreateSamples`.
    IoReport,
    /// HID event-system service reads.
    Hid,
    /// `proc_listallpids`, `proc_pidinfo`, `proc_pid_rusage`.
    Libproc,
    /// `sysctl`, `sysctlbyname`, `host_statistics64`, `host_processor_info`.
    Kernel,
    /// NetworkStatistics manager calls: create, add all, query, destroy (D-081).
    NetStat,
}

impl Api {
    pub const ALL: [Api; 7] = [
        Api::IoKit,
        Api::Smc,
        Api::IoReport,
        Api::Hid,
        Api::Libproc,
        Api::Kernel,
        Api::NetStat,
    ];

    /// The name used in `perf-budget.json`.
    pub fn name(self) -> &'static str {
        match self {
            Api::IoKit => "iokit",
            Api::Smc => "smc",
            Api::IoReport => "ioreport",
            Api::Hid => "hid",
            Api::Libproc => "libproc",
            Api::Kernel => "kernel",
            Api::NetStat => "nstat",
        }
    }
}

/// Call counts, indexed by `Api as usize`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calls(pub [u64; Api::ALL.len()]);

impl Calls {
    pub fn get(&self, api: Api) -> u64 {
        self.0[api as usize]
    }

    /// The calls made between `earlier` and `self`.
    pub fn since(&self, earlier: &Calls) -> Calls {
        let mut out = *self;
        for (o, e) in out.0.iter_mut().zip(earlier.0) {
            *o = o.saturating_sub(e);
        }
        out
    }

    pub fn add(&mut self, other: &Calls) {
        for (a, b) in self.0.iter_mut().zip(other.0) {
            *a += b;
        }
    }
}

/// Whether this build counts calls.
pub const ENABLED: bool = cfg!(feature = "call-counters");

#[cfg(feature = "call-counters")]
thread_local! {
    static COUNTS: std::cell::Cell<[u64; Api::ALL.len()]> =
        const { std::cell::Cell::new([0; Api::ALL.len()]) };
}

/// Records one call on this thread.
#[inline(always)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // only macOS collectors call the OS
pub(crate) fn count(api: Api) {
    #[cfg(feature = "call-counters")]
    COUNTS.with(|c| {
        let mut v = c.get();
        v[api as usize] += 1;
        c.set(v);
    });
    #[cfg(not(feature = "call-counters"))]
    let _ = api;
}

/// This thread's counts so far.
pub fn current() -> Calls {
    #[cfg(feature = "call-counters")]
    return COUNTS.with(|c| Calls(c.get()));
    #[cfg(not(feature = "call-counters"))]
    Calls::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_thread_when_enabled() {
        let before = current();
        count(Api::Smc);
        count(Api::Smc);
        count(Api::Hid);
        let d = current().since(&before);
        let other = std::thread::spawn(current).join().unwrap_or_default();
        assert_eq!(other, Calls::default(), "another thread starts at zero");
        if ENABLED {
            assert_eq!(d.get(Api::Smc), 2);
            assert_eq!(d.get(Api::Hid), 1);
        } else {
            assert_eq!(d, Calls::default());
        }
    }
}
