//! Memory and swap from `/proc/meminfo` and `/proc/vmstat`.
//!
//! | Metric | Source |
//! |---|---|
//! | `mem.used` | MemTotal - MemAvailable (what `free` derives "used" from on current procps) |
//! | `mem.free` | MemFree |
//! | `mem.cached` | MemAvailable - MemFree: the page cache and reclaimable slab the kernel counts as available |
//! | `mem.swap_used` | SwapTotal - SwapFree |
//! | `mem.swap_in`, `mem.swap_out` | `pswpin`/`pswpout` deltas, pages per second |
//!
//! `mem.cached` is the remainder, so used + cached + free is MemTotal and the Overview
//! composition bar adds up, as on macOS. The macOS-only series (`mem.app`, `mem.wired`,
//! `mem.compressed`, `mem.pressure`, `mem.pressure_level`) have no honest Linux
//! counterpart and are not emitted; memory pressure on Linux is `mem.psi`.

use std::path::PathBuf;

use kelvo_schema::{Entitlement, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::procfs::{kib, read_into, word};
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

struct Keys {
    used: SeriesKey,
    cached: SeriesKey,
    free: SeriesKey,
    swap_used: SeriesKey,
    swap_in: SeriesKey,
    swap_out: SeriesKey,
}

impl Keys {
    fn new() -> Self {
        let k = |id| SeriesKey::bare(MetricId::from_static(id));
        Self {
            used: k("mem.used"),
            cached: k("mem.cached"),
            free: k("mem.free"),
            swap_used: k("mem.swap_used"),
            swap_in: k("mem.swap_in"),
            swap_out: k("mem.swap_out"),
        }
    }

    fn all(&self) -> Vec<SeriesKey> {
        [
            &self.used,
            &self.cached,
            &self.free,
            &self.swap_used,
            &self.swap_in,
            &self.swap_out,
        ]
        .into_iter()
        .cloned()
        .collect()
    }
}

/// The gauges from one `/proc/meminfo`, bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Gauges {
    used: u64,
    cached: u64,
    free: u64,
    swap_used: Option<u64>,
}

fn gauges(meminfo: &str) -> Option<Gauges> {
    let total = kib(meminfo, "MemTotal")?;
    let avail = kib(meminfo, "MemAvailable")?.min(total);
    let free = kib(meminfo, "MemFree")?.min(avail);
    let swap_used = kib(meminfo, "SwapTotal")
        .zip(kib(meminfo, "SwapFree"))
        .map(|(t, f)| t.saturating_sub(f));
    Some(Gauges {
        used: total - avail,
        cached: avail - free,
        free,
        swap_used,
    })
}

pub struct Memory {
    keys: Keys,
    meminfo_path: PathBuf,
    vmstat_path: PathBuf,
    buf: String,
    /// Previous pswpin, pswpout and continuous time.
    prev: Option<(u64, u64, u64)>,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Self::at(&SysRoot::host())
    }

    pub fn at(root: &SysRoot) -> Self {
        Self {
            keys: Keys::new(),
            meminfo_path: root.path("/proc/meminfo"),
            vmstat_path: root.path("/proc/vmstat"),
            buf: String::new(),
            prev: None,
        }
    }

    fn push_swap_rates(&mut self, now: u64, out: &mut SampleBuf) {
        let counters = read_into(&self.vmstat_path, &mut self.buf)
            .ok()
            .and_then(|()| Some((word(&self.buf, "pswpin")?, word(&self.buf, "pswpout")?)));
        let Some((pin, pout)) = counters else {
            self.prev = None;
            return;
        };
        if let Some((ppin, ppout, pt)) = self.prev
            && now > pt
            && let (Some(din), Some(dout)) = (pin.checked_sub(ppin), pout.checked_sub(ppout))
        {
            let secs = (now - pt) as f64 / 1e9;
            out.push(&self.keys.swap_in, (din as f64 / secs) as f32);
            out.push(&self.keys.swap_out, (dout as f64 / secs) as f32);
        }
        self.prev = Some((pin, pout, now));
    }
}

impl Collector for Memory {
    fn id(&self) -> CollectorId {
        CollectorId("memory")
    }

    fn cadence(&self) -> Cadence {
        // Gauges stay on every tick, as on macOS (D-070).
        Cadence::EveryTick
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Memory]
    }

    fn probe(&mut self) -> Probe {
        self.prev = None;
        let ok =
            read_into(&self.meminfo_path, &mut self.buf).is_ok() && gauges(&self.buf).is_some();
        if ok {
            Probe::Supported(self.keys.all())
        } else {
            // No MemAvailable means a kernel older than 3.14.
            Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            }
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        read_into(&self.meminfo_path, &mut self.buf).map_err(|e| CollectError::Os {
            call: "read /proc/meminfo",
            code: i64::from(e.raw_os_error().unwrap_or(0)),
        })?;
        let g = gauges(&self.buf).ok_or(CollectError::UnexpectedShape {
            source_name: "/proc/meminfo",
            detail: "no MemTotal, MemAvailable or MemFree",
        })?;
        let k = &self.keys;
        out.push(&k.used, g.used as f32);
        out.push(&k.cached, g.cached as f32);
        out.push(&k.free, g.free as f32);
        if let Some(swap) = g.swap_used {
            out.push(&k.swap_used, swap as f32);
        }
        self.push_swap_rates(tick.continuous_ns, out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    fn tick(s: u64) -> Tick {
        Tick {
            n: s,
            wall_ms: 0,
            continuous_ns: s * 1_000_000_000,
            interval_ms: 1_000,
        }
    }

    #[test]
    fn asahi_meminfo() {
        let mut m = Memory::at(&testdata::asahi_t0());
        assert!(matches!(m.probe(), Probe::Supported(s) if s.len() == 6));
        let mut buf = SampleBuf::new();
        m.sample(&tick(0), &mut buf).unwrap();
        let kib = |n: u64| (n * 1024) as f32;
        // MemTotal 16065952, MemAvailable 9431952, MemFree 2768624 kB.
        assert_eq!(buf.get(&m.keys.used), Some(kib(16_065_952 - 9_431_952)));
        assert_eq!(buf.get(&m.keys.free), Some(kib(2_768_624)));
        assert_eq!(buf.get(&m.keys.cached), Some(kib(9_431_952 - 2_768_624)));
        // SwapTotal 8388592, SwapFree 5752992 kB.
        assert_eq!(buf.get(&m.keys.swap_used), Some(kib(8_388_592 - 5_752_992)));
        assert_eq!(buf.get(&m.keys.swap_in), None, "first sample: no rate");

        // One second later pswpin/pswpout had not moved.
        m.vmstat_path = testdata::asahi_t1().path("/proc/vmstat");
        buf.clear();
        m.sample(&tick(1), &mut buf).unwrap();
        assert_eq!(buf.get(&m.keys.swap_in), Some(0.0));
        assert_eq!(buf.get(&m.keys.swap_out), Some(0.0));
    }

    #[test]
    fn composition_adds_up_and_clamps() {
        let g = gauges(&testdata::x86().read("/proc/meminfo").unwrap()).unwrap();
        assert_eq!(g.used + g.cached + g.free, 8_000_000 * 1024);
        assert_eq!(g.used, 3_000_000 * 1024);
        assert_eq!(g.swap_used, Some(500_000 * 1024));
        // MemFree above MemAvailable (it can be, by the low watermarks): cached is 0,
        // free is capped so the parts still add up.
        let g = gauges("MemTotal: 100 kB\nMemFree: 60 kB\nMemAvailable: 50 kB\n").unwrap();
        assert_eq!((g.used, g.cached, g.free), (50 * 1024, 0, 50 * 1024));
        assert_eq!(g.swap_used, None, "no swap lines: no value");
        assert_eq!(
            gauges("MemTotal: 100 kB\nMemFree: 60 kB\n"),
            None,
            "pre-3.14"
        );
        assert_eq!(gauges(""), None);
    }

    #[test]
    fn swap_rates_are_pages_per_second_and_skip_regressions() {
        let dir = testdata::tree(&[
            (
                "proc/meminfo",
                &testdata::x86().read("/proc/meminfo").unwrap(),
            ),
            ("proc/vmstat", "pswpin 40\npswpout 60\n"),
        ]);
        let mut m = Memory::at(&SysRoot::at(dir.path()));
        assert!(matches!(m.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        m.sample(&tick(0), &mut buf).unwrap();
        testdata::write(&dir, "proc/vmstat", "pswpin 140\npswpout 70\n");
        buf.clear();
        m.sample(&tick(2), &mut buf).unwrap();
        assert_eq!(buf.get(&m.keys.swap_in), Some(50.0));
        assert_eq!(buf.get(&m.keys.swap_out), Some(5.0));
        // A counter that went backwards: no rate this tick, the next one measures again.
        testdata::write(&dir, "proc/vmstat", "pswpin 10\npswpout 80\n");
        buf.clear();
        m.sample(&tick(3), &mut buf).unwrap();
        assert_eq!(buf.get(&m.keys.swap_in), None);
        assert!(buf.get(&m.keys.used).is_some(), "gauges still reported");
        testdata::write(&dir, "proc/vmstat", "pswpin 12\npswpout 81\n");
        buf.clear();
        m.sample(&tick(4), &mut buf).unwrap();
        assert_eq!(buf.get(&m.keys.swap_in), Some(2.0));
    }

    #[test]
    fn missing_meminfo() {
        let mut m = Memory::at(&SysRoot::at("/nonexistent"));
        assert!(matches!(m.probe(), Probe::Unsupported { .. }));
        assert!(m.sample(&tick(0), &mut SampleBuf::new()).is_err());
    }
}
