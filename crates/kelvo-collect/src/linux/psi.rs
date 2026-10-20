//! Pressure stall information from `/proc/pressure/{cpu,memory,io}`: `cpu.psi`,
//! `mem.psi` and `disk.psi`, each `{stall=some|full}`, every 5 s (D-116).
//!
//! Each file has a `some` line and, for memory and io (and cpu since Linux 5.13), a
//! `full` line, each ending in `total=<µs>`: the cumulative time some (or all non-idle)
//! tasks were stalled on the resource. A value is the share of the sample span that
//! counter moved, in percent, so it is a mean over exactly the span it covers. The
//! kernel's `avg10`/`avg60`/`avg300` are decaying averages over fixed windows and are
//! not used. The span is the engine's continuous clock and the counter is read a moment
//! after the tick, so jitter can push a fully stalled span slightly over 100; values are
//! capped there. A counter that went backwards gives no value for that sample.
//!
//! Lines are found at probe: a series exists for each line then present. Without
//! `/proc/pressure` (kernels before 4.20, or `psi=0`, where the files exist but reads
//! fail with `EOPNOTSUPP`) the collector is `NotPresent`.

use std::path::PathBuf;

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::counters::span_secs;
use super::procfs::read_into;
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

const PERIOD_MS: u32 = 5_000;

/// The pressure files and the metric each feeds.
const RESOURCES: [(&str, &str); 3] = [
    ("/proc/pressure/cpu", "cpu.psi"),
    ("/proc/pressure/memory", "mem.psi"),
    ("/proc/pressure/io", "disk.psi"),
];

/// The `total=` counters of the `some` and `full` lines, µs; `None` for a missing line.
fn parse(text: &str) -> [Option<u64>; 2] {
    let mut out = [None; 2];
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        let slot = match it.next() {
            Some("some") => 0,
            Some("full") => 1,
            _ => continue,
        };
        out[slot] = it
            .find_map(|f| f.strip_prefix("total="))
            .and_then(|v| v.parse().ok());
    }
    out
}

/// Percent of `secs` that a µs counter moved from `prev` to `cur`.
fn share(prev: u64, cur: u64, secs: f64) -> Option<f32> {
    let us = cur.checked_sub(prev)?;
    Some((us as f64 / (secs * 1e4)).min(100.0) as f32)
}

struct Resource {
    path: PathBuf,
    /// The `some` and `full` keys; `None` for a line the probe did not find.
    keys: [Option<SeriesKey>; 2],
    prev: [Option<u64>; 2],
}

/// `cpu.psi`, `mem.psi`, `disk.psi`.
pub struct Psi {
    root: SysRoot,
    resources: Vec<Resource>,
    buf: String,
    prev_ns: u64,
}

impl Default for Psi {
    fn default() -> Self {
        Self::new()
    }
}

impl Psi {
    pub fn new() -> Self {
        Self::at(SysRoot::host())
    }

    /// Reads the tree under `root` (tests: a fixture).
    pub fn at(root: SysRoot) -> Self {
        Self {
            root,
            resources: Vec::new(),
            buf: String::new(),
            prev_ns: 0,
        }
    }
}

impl Collector for Psi {
    fn id(&self) -> CollectorId {
        CollectorId("psi")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(PERIOD_MS)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    /// One metric per module; each module's other collectors decide its state, so a
    /// `NotPresent` here never hides CPU, memory or disk.
    fn modules(&self) -> &'static [Module] {
        &[Module::Cpu, Module::Memory, Module::Disk]
    }

    fn probe(&mut self) -> Probe {
        self.prev_ns = 0;
        self.resources.clear();
        for (file, metric) in RESOURCES {
            let path = self.root.path(file);
            if read_into(&path, &mut self.buf).is_err() {
                continue;
            }
            let found = parse(&self.buf);
            let key = |stall| {
                SeriesKey::new(
                    MetricId::from_static(metric),
                    Labels::single("stall", stall),
                )
            };
            let keys = [found[0].map(|_| key("some")), found[1].map(|_| key("full"))];
            if keys.iter().any(Option::is_some) {
                self.resources.push(Resource {
                    path,
                    keys,
                    prev: [None; 2],
                });
            }
        }
        if self.resources.is_empty() {
            return Probe::NotPresent;
        }
        Probe::Supported(
            self.resources
                .iter()
                .flat_map(|r| r.keys.iter().flatten().cloned())
                .collect(),
        )
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        let secs = span_secs(self.prev_ns, now);
        for r in &mut self.resources {
            let cur = match read_into(&r.path, &mut self.buf) {
                Ok(()) => parse(&self.buf),
                Err(_) => [None; 2],
            };
            for ((key, prev), cur) in r.keys.iter().zip(&mut r.prev).zip(cur) {
                if let (Some(key), Some(p), Some(c), Some(secs)) = (key, *prev, cur, secs)
                    && let Some(v) = share(p, c, secs)
                {
                    out.push(key, v);
                }
                *prev = cur;
            }
        }
        self.prev_ns = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    fn tick(at_ms: u64) -> Tick {
        Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: at_ms * 1_000_000,
            interval_ms: 1_000,
        }
    }

    fn psi(metric: &'static str, stall: &str) -> SeriesKey {
        SeriesKey::new(
            MetricId::from_static(metric),
            Labels::single("stall", stall),
        )
    }

    fn close(got: Option<f32>, want: f64) {
        let got = got.expect("a value");
        assert!((f64::from(got) - want).abs() < 1e-6, "{got} vs {want}");
    }

    /// Copies the fixture's pressure files at `t` into `dir`.
    fn pressure_at(dir: &tempfile::TempDir, t: &SysRoot) {
        for f in ["cpu", "memory", "io"] {
            let text = t.read(&format!("/proc/pressure/{f}")).unwrap();
            testdata::write(dir, &format!("proc/pressure/{f}"), &text);
        }
    }

    #[test]
    fn asahi_one_second_of_pressure() {
        let dir = testdata::tree(&[]);
        pressure_at(&dir, &testdata::asahi_t0());
        let mut c = Psi::at(SysRoot::at(dir.path()));
        let Probe::Supported(series) = c.probe() else {
            panic!("not supported")
        };
        assert_eq!(series.len(), 6, "every file has both lines");
        let mut buf = SampleBuf::new();
        c.sample(&tick(10_000), &mut buf).unwrap();
        assert!(buf.values().is_empty(), "the first sample is a baseline");

        pressure_at(&dir, &testdata::asahi_t1());
        buf.clear();
        c.sample(&tick(11_000), &mut buf).unwrap();
        // total deltas over 1 s (1e6 µs): cpu some 16158, cpu full 0; memory 2 and 2;
        // io some 1586, io full 1386.
        close(buf.get(&psi("cpu.psi", "some")), 1.6158);
        close(buf.get(&psi("cpu.psi", "full")), 0.0);
        close(buf.get(&psi("mem.psi", "some")), 0.0002);
        close(buf.get(&psi("mem.psi", "full")), 0.0002);
        close(buf.get(&psi("disk.psi", "some")), 0.1586);
        close(buf.get(&psi("disk.psi", "full")), 0.1386);
        assert_eq!(buf.values().len(), 6);
    }

    #[test]
    fn the_span_comes_from_the_clock() {
        let dir = testdata::tree(&[]);
        pressure_at(&dir, &testdata::asahi_t0());
        let mut c = Psi::at(SysRoot::at(dir.path()));
        c.probe();
        let mut buf = SampleBuf::new();
        c.sample(&tick(10_000), &mut buf).unwrap();
        pressure_at(&dir, &testdata::asahi_t1());
        c.sample(&tick(15_000), &mut buf).unwrap();
        // The same 16158 µs over a 5 s span.
        close(buf.get(&psi("cpu.psi", "some")), 1.6158 / 5.0);
    }

    #[test]
    fn older_kernels_have_no_cpu_full_line() {
        let dir = testdata::tree(&[
            (
                "proc/pressure/cpu",
                "some avg10=0.00 avg60=0.00 avg300=0.00 total=1000\n",
            ),
            (
                "proc/pressure/memory",
                "some avg10=0.00 avg60=0.00 avg300=0.00 total=10\n\
                 full avg10=0.00 avg60=0.00 avg300=0.00 total=5\n",
            ),
        ]);
        let mut c = Psi::at(SysRoot::at(dir.path()));
        let Probe::Supported(series) = c.probe() else {
            panic!("not supported")
        };
        assert_eq!(
            series,
            [
                psi("cpu.psi", "some"),
                psi("mem.psi", "some"),
                psi("mem.psi", "full"),
            ],
            "no cpu full, no io file"
        );
    }

    #[test]
    fn absent_or_disabled_pressure_is_not_present() {
        let dir = testdata::tree(&[("proc/stat", "cpu 1 2 3 4\n")]);
        assert_eq!(Psi::at(SysRoot::at(dir.path())).probe(), Probe::NotPresent);
        // `psi=0`: the files exist but say nothing usable.
        let dir = testdata::tree(&[("proc/pressure/cpu", ""), ("proc/pressure/io", "")]);
        assert_eq!(Psi::at(SysRoot::at(dir.path())).probe(), Probe::NotPresent);
    }

    #[test]
    fn backwards_counters_and_overshoot() {
        assert_eq!(share(100, 99, 1.0), None, "a counter that went backwards");
        assert_eq!(share(0, 1_000_000, 1.0), Some(100.0));
        assert_eq!(share(0, 1_020_000, 1.0), Some(100.0), "jitter is capped");
        assert_eq!(share(0, 500_000, 2.0), Some(25.0));
        assert_eq!(
            parse("some avg10=1.0 total=12\nfull avg10=0 total=x\n"),
            [Some(12), None],
            "a garbled total is a missing line"
        );
        assert_eq!(parse(""), [None, None]);
    }

    #[test]
    fn a_line_that_disappears_gives_no_value_and_no_stale_delta() {
        let dir = testdata::tree(&[("proc/pressure/io", "some total=1000\nfull total=500\n")]);
        let mut c = Psi::at(SysRoot::at(dir.path()));
        c.probe();
        let mut buf = SampleBuf::new();
        c.sample(&tick(1_000), &mut buf).unwrap();
        testdata::write(&dir, "proc/pressure/io", "some total=2000\n");
        c.sample(&tick(2_000), &mut buf).unwrap();
        close(buf.get(&psi("disk.psi", "some")), 0.1);
        assert_eq!(buf.get(&psi("disk.psi", "full")), None);
        // Back again: its baseline was dropped, so still no value on this sample.
        testdata::write(
            &dir,
            "proc/pressure/io",
            "some total=3000\nfull total=9000\n",
        );
        buf.clear();
        c.sample(&tick(3_000), &mut buf).unwrap();
        assert_eq!(buf.get(&psi("disk.psi", "full")), None);
    }

    #[test]
    #[ignore = "reads the live pressure files over 10 s; run by hand on Linux"]
    fn live_smoke() {
        let mut c = Psi::new();
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let start = std::time::Instant::now();
        let at = |s: std::time::Instant| Tick {
            continuous_ns: 1 + u64::try_from(s.elapsed().as_nanos()).unwrap(),
            ..tick(0)
        };
        let mut buf = SampleBuf::new();
        c.sample(&at(start), &mut buf).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(10));
        c.sample(&at(start), &mut buf).unwrap();
        println!("psi window {:.3} s", start.elapsed().as_secs_f64());
        for s in buf.values() {
            println!("{} = {:.4}", s.key, s.value);
        }
        assert!(buf.get(&psi("cpu.psi", "some")).is_some());
    }
}
