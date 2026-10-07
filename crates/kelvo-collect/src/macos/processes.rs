//! Per-process rows (v1-local-monitor.md 6.2) from libproc.
//!
//! Each sample lists every pid (`proc_listallpids`), reads `PROC_PIDTBSDINFO` for name,
//! uid and start time, and `proc_pid_rusage(RUSAGE_INFO_V6)` for CPU time, footprint,
//! idle wakeups, disk bytes and energy. A process is keyed by `(pid, start_time)`, so a
//! reused pid starts fresh. Rates (CPU, wakeups, disk, energy) need a previous sample of
//! the same key and the first sample of a process has none, so it is skipped until the
//! next sample.
//!
//! Wakeups: `idle_wakeups_per_s` uses `ri_interrupt_wkups`, not `ri_pkg_idle_wkups`. The
//! package-idle counter is what `top`'s IDLEW column shows, but on Apple Silicon it barely
//! moves (a terminal app on the dev machine: 3,928 package-idle wakeups over several days
//! against 9.4 million interrupt wakeups), so its per-second rate is always 0.
//!
//! Visibility without root (measured on macOS 27): `PROC_PIDTBSDINFO`,
//! `PROC_PIDTASKALLINFO` and `proc_pid_rusage` succeed only for the user's own processes;
//! for other users' processes (root daemons, `WindowServer`, `_`-accounts, about a third
//! of all pids on the dev machine) they fail, and only `PROC_PIDT_SHORTBSDINFO` (name,
//! uid, no start time, no usage) works. `top` and Activity Monitor see them because `top`
//! is setuid root and Activity Monitor asks the root `sysmond`. Rows with made-up zeros
//! would be wrong, so those processes are not emitted; [`Processes::visibility`] counts
//! them so the UI can say how many are hidden. Showing them needs a privileged helper.
//!
//! Calls per sample are kept down for the processes nobody can read (D-066): a pid whose
//! `PROC_PIDTBSDINFO` or `proc_pid_rusage` failed is skipped without a call until it
//! leaves the listing or [`UNREADABLE_RECHECK_NS`] passes. The start time that would
//! complete its key is exactly what cannot be read, so the cache is by pid; a pid reused
//! between two listings by one of the user's own processes shows up at the next recheck.
//! Thread counts (`PROC_PIDTASKALLINFO`, the most expensive call) are read every
//! [`THREADS_EVERY_NS`] per process, not every sample.
//!
//! `compressed_bytes` is `None`: the only per-process source is `task_info(TASK_VM_INFO)`,
//! which needs another process's task port, which `task_for_pid` refuses without a
//! debugger entitlement.
//!
//! # Energy approximation
//!
//! `energy` is the average power in watts over the interval, from `ri_energy_nj`, the
//! kernel's per-task energy estimate (Apple Silicon attributes cluster energy to tasks by
//! cycles). It covers CPU only, not GPU, ANE, disk or network, so it is an approximation
//! of Activity Monitor's "Energy Impact", which is a proprietary unitless score. When the
//! kernel reports no energy for a task (zero delta while the task used CPU), the
//! collector falls back to a CPU-time model: `cpu_pct / 100 * FALLBACK_WATTS_PER_CORE`
//! plus `idle_wakeups_per_s * FALLBACK_JOULES_PER_WAKEUP`. Both constants are rough and
//! are only there to rank processes sensibly.

use std::collections::HashMap;
use std::sync::Arc;

use kelvo_schema::Entitlement;

use super::libproc;
use crate::{
    Cadence, CollectError, Collector, CollectorId, Interest, Probe, ProcessSample, SampleBuf, Tick,
};

/// Fallback model: watts per fully busy core. Between an E-core (~0.5 W) and a P-core
/// (~4 W) at typical frequencies; deliberately rough.
const FALLBACK_WATTS_PER_CORE: f64 = 2.0;
/// Fallback model: energy per package idle wakeup, in joules (~200 microjoules).
const FALLBACK_JOULES_PER_WAKEUP: f64 = 0.0002;
/// How long an unreadable pid is skipped before it is tried again.
const UNREADABLE_RECHECK_NS: u64 = 60_000_000_000;
/// How often a process's thread count is read.
const THREADS_EVERY_NS: u64 = 5_000_000_000;

#[derive(Clone, Copy)]
struct Prev {
    cpu_ticks: u64,
    wakeups: u64,
    read: u64,
    written: u64,
    energy_nj: u64,
    at_ns: u64,
}

struct Known {
    name: Arc<str>,
    user: Arc<str>,
    /// [`libproc::process_app`], read once when the process is first seen.
    app: Option<Arc<str>>,
    app_main: bool,
    prev: Option<Prev>,
    /// The last thread count read and when, on the continuous clock.
    threads: Option<(u32, u64)>,
}

impl Known {
    /// The thread count, read through `read` when none is cached or it is older than
    /// [`THREADS_EVERY_NS`].
    fn threads(&mut self, now: u64, read: impl FnOnce() -> u32) -> u32 {
        match self.threads {
            Some((n, at)) if now.saturating_sub(at) < THREADS_EVERY_NS => n,
            _ => {
                let n = read();
                self.threads = Some((n, now));
                n
            }
        }
    }
}

/// Pids that could not be read, with when they were last tried.
#[derive(Default)]
struct Unreadable {
    at: HashMap<i32, u64>,
    next: HashMap<i32, u64>,
}

impl Unreadable {
    /// Starts a sample.
    fn begin(&mut self) {
        self.next.clear();
    }

    /// Whether to skip `pid` without a call: it failed less than
    /// [`UNREADABLE_RECHECK_NS`] ago.
    fn skip(&mut self, pid: i32, now: u64) -> bool {
        match self.at.remove(&pid) {
            Some(at) if now.saturating_sub(at) < UNREADABLE_RECHECK_NS => {
                self.next.insert(pid, at);
                true
            }
            _ => false,
        }
    }

    fn failed(&mut self, pid: i32, now: u64) {
        self.next.insert(pid, now);
    }

    /// Ends a sample; pids not listed this time are forgotten.
    fn end(&mut self) {
        std::mem::swap(&mut self.at, &mut self.next);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.at.len()
    }
}

/// How many processes the last sample saw and could read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    /// Every pid `proc_listallpids` returned.
    pub listed: u32,
    /// Processes with full usage data (the user's own).
    pub readable: u32,
}

pub struct Processes {
    ticks_to_ns: f64,
    pids: Vec<i32>,
    known: HashMap<(i32, i64), Known>,
    seen: HashMap<(i32, i64), Known>,
    unreadable: Unreadable,
    users: HashMap<u32, Arc<str>>,
    visibility: Visibility,
}

impl Default for Processes {
    fn default() -> Self {
        Self::new()
    }
}

impl Processes {
    pub fn new() -> Self {
        Self {
            ticks_to_ns: super::sysctl::mach_ticks_to_ns(),
            pids: Vec::new(),
            known: HashMap::new(),
            seen: HashMap::new(),
            unreadable: Unreadable::default(),
            users: HashMap::new(),
            visibility: Visibility::default(),
        }
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }
}

/// Rates for one process over `secs`.
struct Rates {
    cpu_pct: f64,
    wakeups_per_s: f64,
    read_bps: f64,
    write_bps: f64,
    watts: f64,
    /// `watts` times the interval: what the process used over it.
    joules: f64,
}

fn rates(prev: &Prev, cur: &Prev, ticks_to_ns: f64) -> Option<Rates> {
    if cur.at_ns <= prev.at_ns {
        return None;
    }
    let secs = (cur.at_ns - prev.at_ns) as f64 / 1e9;
    let d = |a: u64, b: u64| a.saturating_sub(b) as f64;
    let cpu_ns = d(cur.cpu_ticks, prev.cpu_ticks) * ticks_to_ns;
    let cpu_pct = cpu_ns / (secs * 1e9) * 100.0;
    let wakeups_per_s = d(cur.wakeups, prev.wakeups) / secs;
    let energy_j = d(cur.energy_nj, prev.energy_nj) / 1e9;
    let watts = if energy_j > 0.0 || cpu_ns == 0.0 {
        energy_j / secs
    } else {
        cpu_pct / 100.0 * FALLBACK_WATTS_PER_CORE + wakeups_per_s * FALLBACK_JOULES_PER_WAKEUP
    };
    Some(Rates {
        cpu_pct,
        wakeups_per_s,
        read_bps: d(cur.read, prev.read) / secs,
        write_bps: d(cur.written, prev.written) / secs,
        watts,
        joules: watts * secs,
    })
}

impl Collector for Processes {
    fn id(&self) -> CollectorId {
        CollectorId("processes")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Adaptive {
            idle_ms: crate::IDLE_MS,
            interest: Interest::Processes,
        }
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn probe(&mut self) -> Probe {
        self.known.clear();
        self.unreadable = Unreadable::default();
        if libproc::list_pids(&mut self.pids) {
            // Process rows are not series.
            Probe::Supported(Vec::new())
        } else {
            Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            }
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        if !libproc::list_pids(&mut self.pids) {
            return Err(CollectError::Os {
                call: "proc_listallpids",
                code: i64::from(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)),
            });
        }
        let now = tick.continuous_ns;
        let mut vis = Visibility {
            listed: self.pids.len() as u32,
            readable: 0,
        };
        self.seen.clear();
        self.unreadable.begin();
        for &pid in &self.pids {
            if self.unreadable.skip(pid, now) {
                continue;
            }
            // Both fail for other users' processes and for pids that exited since the
            // listing; either way there is nothing to report.
            let Some(bsd) = libproc::bsd_info(pid) else {
                self.unreadable.failed(pid, now);
                continue;
            };
            let Some(ri) = libproc::rusage(pid) else {
                self.unreadable.failed(pid, now);
                continue;
            };
            vis.readable += 1;
            let key = (pid, libproc::start_time_us(&bsd));
            let mut known = self.known.remove(&key).unwrap_or_else(|| {
                let uid = bsd.pbi_uid;
                let user = self
                    .users
                    .entry(uid)
                    .or_insert_with(|| libproc::user_name(uid).into())
                    .clone();
                let name = libproc::name_of(&bsd);
                let (app, app_main) = libproc::process_app(pid, &name)
                    .map_or((None, false), |(a, main)| (Some(Arc::from(a)), main));
                Known {
                    name: Arc::from(name.as_ref()),
                    user,
                    app,
                    app_main,
                    prev: None,
                    threads: None,
                }
            });
            let cur = Prev {
                cpu_ticks: ri.ri_user_time.saturating_add(ri.ri_system_time),
                wakeups: ri.ri_interrupt_wkups,
                read: ri.ri_diskio_bytesread,
                written: ri.ri_diskio_byteswritten,
                energy_nj: ri.ri_energy_nj,
                at_ns: now,
            };
            if let Some(r) = known
                .prev
                .as_ref()
                .and_then(|p| rates(p, &cur, self.ticks_to_ns))
            {
                let threads = known.threads(now, || {
                    libproc::task_all_info(pid)
                        .map(|t| t.ptinfo.pti_threadnum.max(0) as u32)
                        .unwrap_or(0)
                });
                out.push_process(ProcessSample {
                    pid,
                    start_time_us: key.1,
                    name: known.name.clone(),
                    cpu_pct: r.cpu_pct as f32,
                    mem_bytes: ri.ri_phys_footprint,
                    compressed_bytes: None,
                    threads,
                    idle_wakeups_per_s: r.wakeups_per_s as f32,
                    energy: r.watts as f32,
                    energy_j: r.joules as f32,
                    app: known.app.clone(),
                    app_main: known.app_main,
                    disk_read_bps: r.read_bps as f32,
                    disk_write_bps: r.write_bps as f32,
                    net_rx_bps: None,
                    net_tx_bps: None,
                    gpu_pct: None,
                    user: known.user.clone(),
                });
            }
            known.prev = Some(cur);
            self.seen.insert(key, known);
        }
        // Processes not seen this time have exited; dropping `known` forgets them.
        std::mem::swap(&mut self.known, &mut self.seen);
        self.unreadable.end();
        self.visibility = vis;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(cpu_ticks: u64, wakeups: u64, energy_nj: u64, at_ns: u64) -> Prev {
        Prev {
            cpu_ticks,
            wakeups,
            read: 0,
            written: 0,
            energy_nj,
            at_ns,
        }
    }

    #[test]
    fn cpu_percent_uses_the_timebase() {
        // Apple Silicon timebase: 125/3 ns per tick. 24M ticks = 1 s of CPU in 2 s.
        let r = rates(
            &p(0, 0, 0, 0),
            &p(24_000_000, 0, 0, 2_000_000_000),
            125.0 / 3.0,
        )
        .unwrap();
        assert!((r.cpu_pct - 50.0).abs() < 1e-9, "{}", r.cpu_pct);
    }

    #[test]
    fn energy_prefers_kernel_estimate_then_falls_back() {
        let r = rates(
            &p(0, 0, 0, 0),
            &p(1_000_000_000, 10, 3_000_000_000, 1_000_000_000),
            1.0,
        )
        .unwrap();
        assert!((r.watts - 3.0).abs() < 1e-9);
        // CPU used but no energy reported: model kicks in.
        let r = rates(
            &p(0, 0, 0, 0),
            &p(1_000_000_000, 100, 0, 1_000_000_000),
            1.0,
        )
        .unwrap();
        assert!(
            (r.watts - (2.0 + 100.0 * 0.0002)).abs() < 1e-9,
            "{}",
            r.watts
        );
        // Idle process: zero.
        let r = rates(&p(5, 0, 7, 0), &p(5, 0, 7, 1_000_000_000), 1.0).unwrap();
        assert_eq!(r.watts, 0.0);
    }

    #[test]
    fn joules_are_watts_over_the_interval() {
        // 6 J over 2 s: 3 W, and the 6 J the interval used.
        let r = rates(
            &p(0, 0, 0, 0),
            &p(1_000_000_000, 0, 6_000_000_000, 2_000_000_000),
            1.0,
        )
        .unwrap();
        assert!((r.watts - 3.0).abs() < 1e-9);
        assert!((r.joules - 6.0).abs() < 1e-9);
    }

    #[test]
    fn no_rate_without_elapsed_time() {
        assert!(rates(&p(0, 0, 0, 5), &p(9, 0, 0, 5), 1.0).is_none());
    }

    #[test]
    fn an_unreadable_pid_is_skipped_until_the_recheck_or_it_exits() {
        let mut u = Unreadable::default();
        let s = 1_000_000_000;
        u.begin();
        assert!(!u.skip(7, 0), "never tried");
        u.failed(7, 0);
        u.failed(8, 0);
        u.end();

        u.begin();
        assert!(u.skip(7, 30 * s));
        // 8 left the listing: not asked about, so forgotten.
        u.end();
        assert_eq!(u.len(), 1);

        u.begin();
        assert!(!u.skip(8, 31 * s), "a new process with an old pid is read");
        assert!(u.skip(7, 59 * s));
        u.end();
        u.begin();
        assert!(!u.skip(7, 60 * s), "tried again after the recheck interval");
        u.end();
        assert_eq!(u.len(), 0, "and forgotten unless it fails again");
    }

    #[test]
    fn thread_counts_are_read_every_five_seconds() {
        let mut k = Known {
            name: "p".into(),
            user: "me".into(),
            app: None,
            app_main: false,
            prev: None,
            threads: None,
        };
        let s = 1_000_000_000;
        let mut reads = 0;
        let mut at = |now: u64, n: u32| {
            k.threads(now, || {
                reads += 1;
                n
            })
        };
        assert_eq!(at(0, 3), 3);
        assert_eq!(at(4 * s, 9), 3, "cached");
        assert_eq!(at(5 * s, 9), 9, "read again");
        assert_eq!(at(9 * s, 1), 9);
        drop(at);
        assert_eq!(reads, 2);
    }

    #[test]
    #[ignore = "reads every live process; run by hand on a Mac"]
    fn live_smoke() {
        let mut c = Processes::new();
        assert_eq!(c.probe(), Probe::Supported(Vec::new()));
        let mut buf = SampleBuf::new();
        for n in 0..2 {
            buf.clear();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: super::super::sysctl::continuous_ns(),
                interval_ms: 1_000,
            };
            c.sample(&tick, &mut buf).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        let mut rows = buf.processes().to_vec();
        rows.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
        println!("visibility: {:?}, rows: {}", c.visibility(), rows.len());
        let with_energy = rows.iter().filter(|r| r.energy > 0.0).count();
        println!("rows with energy > 0: {with_energy}");
        let wakeups: f32 = rows.iter().map(|r| r.idle_wakeups_per_s).sum();
        println!("total idle wakeups/s: {wakeups:.0}");
        for r in rows.iter().take(12) {
            println!(
                "{:>6} {:<28} {:>6.1}% {:>8.1} MB thr={:<4} wk/s={:<6.1} {:.3} W {:.3} J r={:.0} w={:.0} {} app={:?} main={}",
                r.pid,
                r.name,
                r.cpu_pct,
                r.mem_bytes as f64 / 1e6,
                r.threads,
                r.idle_wakeups_per_s,
                r.energy,
                r.energy_j,
                r.disk_read_bps,
                r.disk_write_bps,
                r.user,
                r.app,
                r.app_main
            );
        }
        let with_app = rows.iter().filter(|r| r.app.is_some()).count();
        println!("rows with an app: {with_app}");
        assert!(c.visibility().readable > 0);
    }
}
