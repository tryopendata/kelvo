//! CPU load from `/proc/stat` jiffies: `cpu.total`, `cpu.user`, `cpu.system`,
//! `cpu.iowait`, `cpu.steal`, `cpu.load{core}`, and `cpu.loadavg{window}` from
//! `/proc/loadavg` every 5 s.
//!
//! Shares of each interval's jiffies (D-115):
//!
//! | Series | Fields |
//! |---|---|
//! | `cpu.user` | user + nice (`guest` is already inside `user`, so it is not added) |
//! | `cpu.system` | system + irq + softirq |
//! | `cpu.steal` | steal |
//! | `cpu.total` | user + system + steal: everything but idle and iowait, the macOS meaning |
//! | `cpu.iowait` | iowait, idle time with I/O outstanding |
//!
//! The denominator is all eight fields. Per-core loads use the same busy set and their
//! labels come from [`super::topology`], so they match `HostInfo::cpu_topology`.
//! Counters are `u64` and should only grow; a field that went backwards (a kernel
//! accounting quirk, CPU hotplug) leaves that line without a value for the tick.

use std::path::PathBuf;

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::procfs::read_into;
use super::topology::core_labels;
use crate::{Cadence, CollectError, Collector, CollectorId, Every, Probe, SampleBuf, Tick};

/// `cpu.loadavg` is sampled at most every 5 s, as on macOS.
const LOADAVG_MS: u32 = 5_000;

/// One `/proc/stat` cpu line: user, nice, system, idle, iowait, irq, softirq, steal.
type Jiffies = [u64; 8];

const USER: usize = 0;
const NICE: usize = 1;
const SYSTEM: usize = 2;
const IOWAIT: usize = 4;
const IRQ: usize = 5;
const SOFTIRQ: usize = 6;
const STEAL: usize = 7;

/// Parses the counters after a `cpu`/`cpuN` token. Kernels before 2.6.11 print fewer
/// fields; the missing ones read 0.
fn parse_jiffies<'a>(fields: impl Iterator<Item = &'a str>) -> Option<Jiffies> {
    let mut j = [0u64; 8];
    let mut n = 0;
    for (slot, f) in j.iter_mut().zip(fields) {
        *slot = f.parse().ok()?;
        n += 1;
    }
    (n >= 4).then_some(j)
}

/// Parses `/proc/stat`: the aggregate line, and each `cpuN` line into `per_cpu[N]` (left
/// `None` for CPUs without a line). Returns the aggregate.
fn parse_stat(text: &str, per_cpu: &mut [Option<Jiffies>]) -> Option<Jiffies> {
    per_cpu.fill(None);
    let mut all = None;
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        let Some(rest) = it.next().and_then(|t| t.strip_prefix("cpu")) else {
            continue;
        };
        if rest.is_empty() {
            all = parse_jiffies(it);
        } else if let Some(slot) = rest.parse::<usize>().ok().and_then(|i| per_cpu.get_mut(i)) {
            *slot = parse_jiffies(it);
        }
    }
    all
}

/// Shares of one interval, percent.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shares {
    total: f32,
    user: f32,
    system: f32,
    iowait: f32,
    steal: f32,
}

/// The shares between two readings; `None` when a counter went backwards or no time
/// passed.
fn shares(prev: &Jiffies, cur: &Jiffies) -> Option<Shares> {
    let mut d = [0u64; 8];
    for ((d, p), c) in d.iter_mut().zip(prev).zip(cur) {
        *d = c.checked_sub(*p)?;
    }
    let all: u64 = d.iter().sum();
    if all == 0 {
        return None;
    }
    let pct = |n: u64| (n as f64 * 100.0 / all as f64) as f32;
    let user = d[USER] + d[NICE];
    let system = d[SYSTEM] + d[IRQ] + d[SOFTIRQ];
    Some(Shares {
        total: pct(user + system + d[STEAL]),
        user: pct(user),
        system: pct(system),
        iowait: pct(d[IOWAIT]),
        steal: pct(d[STEAL]),
    })
}

/// The three averages from `/proc/loadavg`.
fn parse_loadavg(text: &str) -> Option<[f32; 3]> {
    let mut it = text.split_ascii_whitespace().map(|f| f.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

pub struct Cpu {
    root: SysRoot,
    stat_path: PathBuf,
    loadavg_path: PathBuf,
    total: SeriesKey,
    user: SeriesKey,
    system: SeriesKey,
    iowait: SeriesKey,
    steal: SeriesKey,
    /// `(logical CPU, cpu.load key)`, from the last probe.
    cores: Vec<(usize, SeriesKey)>,
    loadavg: [SeriesKey; 3],
    loadavg_every: Every,
    buf: String,
    prev: Vec<Option<Jiffies>>,
    cur: Vec<Option<Jiffies>>,
    prev_all: Option<Jiffies>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self::at(SysRoot::host())
    }

    /// Reads the tree under `root` (tests: a fixture).
    pub fn at(root: SysRoot) -> Self {
        let m = MetricId::from_static;
        let la = |w| SeriesKey::new(m("cpu.loadavg"), Labels::single("window", w));
        Self {
            stat_path: root.path("/proc/stat"),
            loadavg_path: root.path("/proc/loadavg"),
            root,
            total: SeriesKey::bare(m("cpu.total")),
            user: SeriesKey::bare(m("cpu.user")),
            system: SeriesKey::bare(m("cpu.system")),
            iowait: SeriesKey::bare(m("cpu.iowait")),
            steal: SeriesKey::bare(m("cpu.steal")),
            cores: Vec::new(),
            loadavg: [la("1"), la("5"), la("15")],
            loadavg_every: Every::new(LOADAVG_MS),
            buf: String::new(),
            prev: Vec::new(),
            cur: Vec::new(),
            prev_all: None,
        }
    }

    fn push_loadavg(&mut self, out: &mut SampleBuf) {
        if read_into(&self.loadavg_path, &mut self.buf).is_ok()
            && let Some(avg) = parse_loadavg(&self.buf)
        {
            for (key, v) in self.loadavg.iter().zip(avg) {
                out.push(key, v);
            }
        }
    }
}

impl Collector for Cpu {
    fn id(&self) -> CollectorId {
        CollectorId("cpu")
    }

    fn cadence(&self) -> Cadence {
        // D-067: every tick while a window or the menu bar shows it, else every 10 s.
        crate::LIVE_OR_IDLE
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Cpu]
    }

    fn probe(&mut self) -> Probe {
        self.prev_all = None;
        self.loadavg_every.reset();
        if read_into(&self.stat_path, &mut self.buf).is_err()
            || parse_stat(&self.buf, &mut []).is_none()
        {
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        }
        self.cores = core_labels(&self.root)
            .into_iter()
            .map(|(cpu, label)| {
                let key = SeriesKey::new(
                    MetricId::from_static("cpu.load"),
                    Labels::single("core", &label),
                );
                (cpu, key)
            })
            .collect();
        let slots = self.cores.iter().map(|(c, _)| c + 1).max().unwrap_or(0);
        self.prev = vec![None; slots];
        self.cur = vec![None; slots];
        let mut series = vec![
            self.total.clone(),
            self.user.clone(),
            self.system.clone(),
            self.iowait.clone(),
            self.steal.clone(),
        ];
        series.extend(self.cores.iter().map(|(_, k)| k.clone()));
        series.extend(self.loadavg.iter().cloned());
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        if self.loadavg_every.due(tick) {
            self.push_loadavg(out);
        }
        read_into(&self.stat_path, &mut self.buf).map_err(|e| CollectError::Os {
            call: "read /proc/stat",
            code: i64::from(e.raw_os_error().unwrap_or(0)),
        })?;
        let Some(all) = parse_stat(&self.buf, &mut self.cur) else {
            return Err(CollectError::UnexpectedShape {
                source_name: "/proc/stat",
                detail: "no aggregate cpu line",
            });
        };
        if let Some(s) = self.prev_all.as_ref().and_then(|p| shares(p, &all)) {
            out.push(&self.total, s.total);
            out.push(&self.user, s.user);
            out.push(&self.system, s.system);
            out.push(&self.iowait, s.iowait);
            out.push(&self.steal, s.steal);
        }
        if self.prev_all.is_some() {
            for (cpu, key) in &self.cores {
                let p = self.prev.get(*cpu).copied().flatten();
                let c = self.cur.get(*cpu).copied().flatten();
                if let (Some(p), Some(c)) = (p, c)
                    && let Some(s) = shares(&p, &c)
                {
                    out.push(key, s.total);
                }
            }
        }
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.prev_all = Some(all);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    fn tick(n: u64) -> Tick {
        Tick {
            n,
            wall_ms: 0,
            continuous_ns: n * 1_000_000_000,
            interval_ms: 1_000,
        }
    }

    fn core(label: &str) -> SeriesKey {
        SeriesKey::new(
            MetricId::from_static("cpu.load"),
            Labels::single("core", label),
        )
    }

    fn close(got: Option<f32>, want: f64) {
        let got = got.expect("a value");
        assert!((f64::from(got) - want).abs() < 1e-4, "{got} vs {want}");
    }

    #[test]
    fn asahi_one_second_of_jiffies() {
        let mut c = Cpu::at(testdata::asahi_t0());
        let Probe::Supported(series) = c.probe() else {
            panic!("unsupported")
        };
        // 5 shares, 8 cores, 3 load averages.
        assert_eq!(series.len(), 16);
        assert!(series.contains(&core("E3")) && series.contains(&core("P3")));

        let mut buf = SampleBuf::new();
        c.sample(&tick(0), &mut buf).unwrap();
        // The first sample has no rates, only the load averages.
        assert_eq!(buf.values().len(), 3);
        assert_eq!(buf.get(&c.loadavg[0]), Some(0.34));
        assert_eq!(buf.get(&c.loadavg[2]), Some(0.47));

        c.stat_path = testdata::asahi_t1().path("/proc/stat");
        buf.clear();
        c.sample(&tick(1), &mut buf).unwrap();
        // Aggregate deltas: user 24, nice 0, system 56, idle 1207, iowait 1, irq 3,
        // softirq 2, steal 0; 1293 jiffies in all.
        close(buf.get(&c.total), 85.0 * 100.0 / 1293.0);
        close(buf.get(&c.user), 24.0 * 100.0 / 1293.0);
        close(buf.get(&c.system), 61.0 * 100.0 / 1293.0);
        close(buf.get(&c.iowait), 100.0 / 1293.0);
        close(buf.get(&c.steal), 0.0);
        // cpu0: user 2, system 5, idle 153, iowait 1, irq 1, softirq 1.
        close(buf.get(&core("E0")), 9.0 * 100.0 / 163.0);
        // cpu4 only idled; cpu7: user 11, system 29, idle 122.
        close(buf.get(&core("P0")), 0.0);
        close(buf.get(&core("P3")), 40.0 * 100.0 / 162.0);
        assert_eq!(buf.get(&c.loadavg[0]), None, "load averages every 5 s");
    }

    #[test]
    fn x86_cores_are_uniform_and_steal_counts_as_busy() {
        let dir = testdata::tree(&[]);
        let x86 = testdata::x86();
        for f in ["proc/stat", "sys/devices/system/cpu/online", "proc/loadavg"] {
            testdata::write(&dir, f, &x86.read(&format!("/{f}")).unwrap());
        }
        let mut c = Cpu::at(SysRoot::at(dir.path()));
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        c.sample(&tick(0), &mut buf).unwrap();
        // +100 user, +100 steal, +700 idle, +100 iowait on the aggregate and cpu2.
        testdata::write(
            &dir,
            "proc/stat",
            "cpu  4100 100 2000 80700 400 50 50 600 0 0\n\
             cpu0 1000 25 500 20000 75 12 13 125 0 0\n\
             cpu1 1000 25 500 20000 75 13 12 125 0 0\n\
             cpu2 1100 25 500 20700 175 12 13 225 0 0\n\
             cpu3 1000 25 500 20000 75 13 12 125 0 0\n",
        );
        buf.clear();
        c.sample(&tick(1), &mut buf).unwrap();
        close(buf.get(&c.total), 20.0);
        close(buf.get(&c.steal), 10.0);
        close(buf.get(&c.iowait), 10.0);
        close(buf.get(&core("C2")), 20.0);
        // No jiffies passed on cpu0: no value rather than 0%.
        assert_eq!(buf.get(&core("C0")), None);
    }

    #[test]
    fn a_counter_that_went_backwards_gives_no_value() {
        let prev = [100, 0, 100, 1000, 50, 0, 0, 0];
        let mut cur = [110, 0, 110, 1080, 50, 0, 0, 0];
        assert!(shares(&prev, &cur).is_some());
        cur[IOWAIT] = 49;
        assert_eq!(shares(&prev, &cur), None);
        // Wrapped to a small number: also a regression, not a huge delta.
        cur[IOWAIT] = 50;
        cur[USER] = 3;
        assert_eq!(shares(&prev, &cur), None);
        assert_eq!(shares(&prev, &prev), None, "no time passed");
    }

    #[test]
    fn garbled_and_short_lines() {
        let mut per = [None; 3];
        let text =
            "cpu  1 2 3 4\ncpu0 1 2 x 4 5\ncpu1 1 2 3 4 5 6 7 8 9 10\ncpu9 1 1 1 1\n\nintr\n";
        assert_eq!(parse_stat(text, &mut per), Some([1, 2, 3, 4, 0, 0, 0, 0]));
        assert_eq!(per[0], None, "garbled field");
        assert_eq!(
            per[1],
            Some([1, 2, 3, 4, 5, 6, 7, 8]),
            "guest fields ignored"
        );
        assert_eq!(per[2], None, "no line");
        assert_eq!(parse_stat("cpu 1 2\n", &mut per), None, "too few fields");
        assert_eq!(parse_stat("", &mut per), None);
        assert_eq!(
            parse_loadavg("0.34 0.41 0.47 1/999 2394632\n"),
            Some([0.34, 0.41, 0.47])
        );
        assert_eq!(parse_loadavg("0.34 x 0.47"), None);
        assert_eq!(parse_loadavg(""), None);
    }

    #[test]
    fn missing_proc_stat_is_unsupported() {
        let mut c = Cpu::at(SysRoot::at("/nonexistent"));
        assert!(matches!(c.probe(), Probe::Unsupported { .. }));
    }

    #[test]
    #[ignore = "reads the live CPU counters; run by hand on Linux"]
    fn live_smoke() {
        let mut c = Cpu::new();
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        for n in 0..2 {
            buf.clear();
            c.sample(&tick(n * 5), &mut buf).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        for s in buf.values() {
            println!("{} = {:.2}", s.key, s.value);
        }
        assert!(buf.get(&c.total).is_some());
    }
}
