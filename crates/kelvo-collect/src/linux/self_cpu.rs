//! `self.cpu`: CPU used by this process (the agent), as percent of one core over the
//! interval, from utime + stime in `/proc/self/stat`. The Linux agent has no helper
//! processes, so unlike macOS there is no coalition to walk.

use std::path::PathBuf;

use kelvo_schema::{Entitlement, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::procfs::read_into;
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

/// utime + stime from a `stat` line, after the last `)` (comm may hold one).
fn cpu_ticks(stat: &str) -> Option<u64> {
    let rest = stat.get(stat.rfind(')')? + 1..)?;
    let mut f = rest.split_ascii_whitespace();
    let utime: u64 = f.nth(11)?.parse().ok()?;
    let stime: u64 = f.next()?.parse().ok()?;
    Some(utime.saturating_add(stime))
}

pub struct SelfCpu {
    key: SeriesKey,
    stat_path: PathBuf,
    clk_tck: u64,
    buf: String,
    /// CPU ticks and continuous time at the previous sample.
    prev: Option<(u64, u64)>,
}

impl Default for SelfCpu {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfCpu {
    pub fn new() -> Self {
        Self::at(&SysRoot::host(), "/proc/self/stat", super::clk_tck())
    }

    /// Reads `stat` (a path under `root`) with the given `USER_HZ` (tests).
    pub fn at(root: &SysRoot, stat: &str, clk_tck: u64) -> Self {
        Self {
            key: SeriesKey::bare(MetricId::from_static("self.cpu")),
            stat_path: root.path(stat),
            clk_tck: clk_tck.max(1),
            buf: String::new(),
            prev: None,
        }
    }

    fn read(&mut self) -> Option<u64> {
        read_into(&self.stat_path, &mut self.buf).ok()?;
        cpu_ticks(&self.buf)
    }
}

impl Collector for SelfCpu {
    fn id(&self) -> CollectorId {
        CollectorId("self_cpu")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(10_000)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Cpu]
    }

    fn probe(&mut self) -> Probe {
        self.prev = None;
        if self.read().is_some() {
            Probe::Supported(vec![self.key.clone()])
        } else {
            Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            }
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        let ticks = self.read().ok_or(CollectError::UnexpectedShape {
            source_name: "/proc/self/stat",
            detail: "unreadable or no utime/stime",
        })?;
        if let Some((pticks, pt)) = self.prev
            && now > pt
            && let Some(d) = ticks.checked_sub(pticks)
        {
            let cpu_s = d as f64 / self.clk_tck as f64;
            out.push(
                &self.key,
                (cpu_s / ((now - pt) as f64 / 1e9) * 100.0) as f32,
            );
        }
        self.prev = Some((ticks, now));
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
    fn fixture_ticks() {
        let stat = testdata::asahi_t0().read("/proc/1/stat").unwrap();
        assert_eq!(cpu_ticks(&stat), Some(496_368 + 373_557));
        assert_eq!(cpu_ticks("1 (a b) S 1 2"), None);
        assert_eq!(cpu_ticks(""), None);
    }

    #[test]
    fn percent_of_one_core_over_the_interval() {
        let stat = testdata::asahi_t0().read("/proc/1/stat").unwrap();
        let dir = testdata::tree(&[("proc/self/stat", &stat)]);
        let mut c = SelfCpu::at(&SysRoot::at(dir.path()), "/proc/self/stat", 100);
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        c.sample(&tick(0), &mut buf).unwrap();
        assert!(buf.values().is_empty(), "first sample has no rate");
        // +25 stime ticks (0.25 s) over 10 s: 2.5%.
        let later = stat.replacen(" 373557 ", " 373582 ", 1);
        testdata::write(&dir, "proc/self/stat", &later);
        c.sample(&tick(10), &mut buf).unwrap();
        let pct = buf.get(&c.key).unwrap();
        assert!((pct - 2.5).abs() < 1e-5, "{pct}");
        // Went backwards (cannot for one process; a garbled read): no value.
        testdata::write(&dir, "proc/self/stat", &stat);
        buf.clear();
        c.sample(&tick(20), &mut buf).unwrap();
        assert!(buf.values().is_empty());
    }

    #[test]
    fn measures_own_busy_loop() {
        let mut c = SelfCpu::new();
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        let start = std::time::Instant::now();
        c.sample(&tick(0), &mut buf).unwrap();
        let mut x = 0u64;
        while start.elapsed() < std::time::Duration::from_millis(300) {
            x = std::hint::black_box(x.wrapping_add(1));
        }
        let ns = start.elapsed().as_nanos() as u64;
        let t = Tick {
            continuous_ns: ns,
            ..tick(0)
        };
        c.sample(&t, &mut buf).unwrap();
        let pct = buf.get(&c.key).unwrap();
        // One spinning thread: close to 100%, loosely bounded because tests run in
        // parallel and USER_HZ granularity is 10 ms.
        assert!(pct > 30.0, "{pct}");
    }
}
