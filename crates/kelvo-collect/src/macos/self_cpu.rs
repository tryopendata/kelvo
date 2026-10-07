//! `self.cpu`: CPU used by Kelvo itself, the app process plus its WebKit helper
//! processes (WebContent, Networking, GPU), as percent of one core over the interval.
//! This is the number behind the idle-CPU budget (architecture.md, under 0.5%).
//!
//! Helpers are found with `responsibility_get_pid_responsible_for_pid`: macOS holds the
//! app responsible for the XPC helpers WebKit spawns for it. Verified on macOS 27 without
//! any entitlement: every `com.apple.WebKit.*` helper of a running WKWebView app resolved
//! to that app's pid. Coalitions were not needed (and `PROC_PIDCOALITIONINFO` is not in
//! the public headers).
//!
//! When the app is launched from a terminal (`tauri dev`), macOS holds the terminal
//! responsible for the app too, so the helpers resolve to the terminal's pid rather than
//! ours. In that case WebKit helpers sharing our responsible pid are counted as well. That
//! can over-count if another app launched from the same terminal also runs WebKit, which
//! only matters in development.
//!
//! Asking every pid who is responsible for it is most of this collector's cost (about
//! 1,100 calls on the dev machine), so [`Coalition`] remembers each pid's answer: a pid
//! is asked once when it first appears, a member's start time is checked on every sample
//! (a reused pid is asked again), and every [`MEMBERSHIP_RECHECK_NS`] every pid is asked
//! again, which bounds how late a helper that took over a non-member's pid is counted.
//! `scripts/bench-coalition.sh` measures the packaged app with the same [`Coalition`].

use std::collections::HashMap;

use kelvo_schema::{Entitlement, MetricId, Module, SeriesKey};

use super::libproc;
use crate::process_control::OwnProcessList;
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

/// How long a pid's membership answer is trusted before every pid is asked again. macOS
/// hands out pids in increasing order, so a new helper taking over a non-member's pid
/// within five minutes needs the pid space to wrap in that time.
pub const MEMBERSHIP_RECHECK_NS: u64 = 300_000_000_000;

/// One process of a coalition at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub pid: i32,
    /// `ri_proc_start_abstime`: tells a reused pid from the process seen before.
    pub start_abstime: u64,
    /// User plus system CPU time since the process started, in ns.
    pub cpu_ns: u64,
    /// `ri_phys_footprint`, the number Activity Monitor's Memory column shows.
    pub footprint_bytes: u64,
}

/// What a pid was found to be, and for a member, which process that was.
#[derive(Clone, Copy)]
enum Known {
    Member { start_abstime: u64 },
    Other,
}

/// An app process and the helpers macOS holds it responsible for, with each pid's
/// membership remembered between listings (module docs).
pub struct Coalition {
    root: i32,
    ticks_to_ns: f64,
    pids: Vec<i32>,
    known: HashMap<i32, Known>,
    next: HashMap<i32, Known>,
    checked_at: Option<u64>,
    /// Membership lookups made, for the test of the cache.
    asked: u64,
}

impl Coalition {
    pub fn new(root: i32) -> Self {
        Self {
            root,
            ticks_to_ns: super::sysctl::mach_ticks_to_ns(),
            pids: Vec::new(),
            known: HashMap::new(),
            next: HashMap::new(),
            checked_at: None,
            asked: 0,
        }
    }

    pub fn root(&self) -> i32 {
        self.root
    }

    /// Responsible-pid lookups made so far (one per pid not answered from the cache).
    pub fn lookups(&self) -> u64 {
        self.asked
    }

    /// Forgets every answer, so the next listing asks every pid.
    pub fn reset(&mut self) {
        self.known.clear();
        self.checked_at = None;
    }

    /// The members now, into `out` (cleared first). `now_ns` is the continuous clock.
    /// Returns false if the pid listing failed.
    pub fn members(&mut self, now_ns: u64, out: &mut Vec<Member>) -> bool {
        out.clear();
        if !libproc::list_pids(&mut self.pids) {
            return false;
        }
        if self
            .checked_at
            .is_none_or(|at| now_ns.saturating_sub(at) >= MEMBERSHIP_RECHECK_NS)
        {
            self.known.clear();
            self.checked_at = Some(now_ns);
        }
        let mut root_responsible = None;
        self.next.clear();
        for i in 0..self.pids.len() {
            let Some(&pid) = self.pids.get(i) else {
                continue;
            };
            let known = self.known.get(&pid).copied();
            if let Some(Known::Other) = known {
                self.next.insert(pid, Known::Other);
                continue;
            }
            let ri = match known {
                Some(Known::Member { start_abstime }) => match libproc::rusage(pid) {
                    Some(ri) if ri.ri_proc_start_abstime == start_abstime => Some(ri),
                    // Reused or gone: ask again below.
                    _ => None,
                },
                _ => None,
            };
            let ri = match ri {
                Some(ri) => ri,
                None => {
                    let sr = *root_responsible
                        .get_or_insert_with(|| libproc::responsible_pid(self.root));
                    self.asked += 1;
                    if !is_member(self.root, pid, sr) {
                        self.next.insert(pid, Known::Other);
                        continue;
                    }
                    let Some(ri) = libproc::rusage(pid) else {
                        continue;
                    };
                    ri
                }
            };
            self.next.insert(
                pid,
                Known::Member {
                    start_abstime: ri.ri_proc_start_abstime,
                },
            );
            out.push(Member {
                pid,
                start_abstime: ri.ri_proc_start_abstime,
                cpu_ns: (ri.ri_user_time.saturating_add(ri.ri_system_time) as f64
                    * self.ticks_to_ns) as u64,
                footprint_bytes: ri.ri_phys_footprint,
            });
        }
        // Pids no longer listed are forgotten.
        std::mem::swap(&mut self.known, &mut self.next);
        true
    }
}

/// Whether macOS holds `root` responsible for `pid` (module docs). `root_responsible` is
/// the pid responsible for `root` itself.
fn is_member(root: i32, pid: i32, root_responsible: Option<i32>) -> bool {
    if pid == root {
        return true;
    }
    let Some(r) = libproc::responsible_pid(pid) else {
        return false;
    };
    if r == root {
        return true;
    }
    // Launched from a terminal: helpers resolve to the terminal, as we do.
    root_responsible.is_some_and(|sr| sr != root && sr == r)
        && libproc::bsd_info(pid)
            .is_some_and(|b| libproc::name_starts_with(&b, b"com.apple.WebKit."))
}

/// The process name of `pid` (tools: the coalition benchmark's per-process lines).
pub fn process_name(pid: i32) -> Option<String> {
    libproc::bsd_info(pid).map(|b| libproc::name_of(&b).into_owned())
}

pub struct SelfCpu {
    key: SeriesKey,
    coalition: Coalition,
    now: Vec<Member>,
    /// Member pid -> (start abstime, cumulative CPU ns) at the previous sample.
    prev: HashMap<i32, (u64, u64)>,
    cur: HashMap<i32, (u64, u64)>,
    prev_ns: u64,
    /// Where the app's own processes are published, when measuring this process.
    own: Option<OwnProcessList>,
    /// The members' pids, reused for publishing.
    own_pids: Vec<i32>,
}

impl Default for SelfCpu {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfCpu {
    /// Measures the current process and its helpers.
    /// Publishes them as the app's own processes ([`OwnProcessList::global`]).
    pub fn new() -> Self {
        Self {
            own: Some(OwnProcessList::global().clone()),
            ..Self::for_pid(std::process::id() as i32)
        }
    }

    /// Measures `pid` and its helpers (tests and tools). Publishes nothing.
    pub fn for_pid(pid: i32) -> Self {
        Self {
            key: SeriesKey::bare(MetricId::from_static("self.cpu")),
            coalition: Coalition::new(pid),
            now: Vec::new(),
            prev: HashMap::new(),
            cur: HashMap::new(),
            prev_ns: 0,
            own: None,
            own_pids: Vec::new(),
        }
    }

    /// How many processes the last sample counted (the app plus helpers).
    pub fn members(&self) -> usize {
        self.prev.len()
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
        self.prev.clear();
        self.prev_ns = 0;
        self.coalition.reset();
        if libproc::rusage(self.coalition.root()).is_some() {
            Probe::Supported(vec![self.key.clone()])
        } else {
            Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            }
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        if !self.coalition.members(now, &mut self.now) {
            return Err(CollectError::Os {
                call: "proc_listallpids",
                code: 0,
            });
        }
        if let Some(own) = &self.own {
            self.own_pids.clear();
            self.own_pids.extend(self.now.iter().map(|m| m.pid));
            // The listing began no later than the tick.
            own.record(tick.wall_ms.saturating_mul(1_000), &mut self.own_pids);
        }
        self.cur.clear();
        let mut delta_ns = 0u64;
        for m in &self.now {
            if let Some(&(pstart, pcpu)) = self.prev.get(&m.pid)
                && pstart == m.start_abstime
            {
                delta_ns += m.cpu_ns.saturating_sub(pcpu);
            }
            self.cur.insert(m.pid, (m.start_abstime, m.cpu_ns));
        }
        std::mem::swap(&mut self.prev, &mut self.cur);

        if self.prev_ns > 0 && now > self.prev_ns {
            let pct = delta_ns as f64 / (now - self.prev_ns) as f64 * 100.0;
            out.push(&self.key, pct as f32);
        }
        self.prev_ns = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick() -> Tick {
        Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: super::super::sysctl::continuous_ns(),
            interval_ms: 1_000,
        }
    }

    #[test]
    fn measures_own_busy_loop() {
        let mut c = SelfCpu::new();
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        c.sample(&tick(), &mut buf).unwrap();
        assert!(buf.values().is_empty(), "first sample has no rate");
        let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
        let mut x = 0u64;
        while std::time::Instant::now() < until {
            x = std::hint::black_box(x.wrapping_add(1));
        }
        c.sample(&tick(), &mut buf).unwrap();
        let pct = buf.get(&c.key).unwrap();
        // One thread spinning for the whole interval: close to 100% of a core, but the
        // test harness runs other tests in parallel, so only bound it loosely.
        assert!(pct > 30.0, "{pct}");
        assert!(c.members() >= 1);
    }

    #[test]
    fn membership_is_asked_once_per_pid_until_the_recheck() {
        let me = std::process::id() as i32;
        let mut c = Coalition::new(me);
        let mut out = Vec::new();
        let s = 1_000_000_000;
        let asked = |c: &mut Coalition, out: &mut Vec<Member>, at: u64| {
            let before = c.lookups();
            assert!(c.members(at, out));
            assert!(out.iter().any(|m| m.pid == me), "the root is a member");
            c.lookups() - before
        };
        let first = asked(&mut c, &mut out, 0);
        assert!(first > 50, "every listed pid is asked once: {first}");
        // Only pids that appeared since (other tests, the machine) are asked.
        let cached = asked(&mut c, &mut out, 10 * s);
        assert!(cached * 10 < first, "first {first}, cached {cached}");
        let rechecked = asked(&mut c, &mut out, 10 * s + MEMBERSHIP_RECHECK_NS);
        assert!(
            rechecked * 10 > first * 9,
            "first {first}, rechecked {rechecked}"
        );
    }

    #[test]
    #[ignore = "needs a running app with WebKit helpers; run by hand on a Mac"]
    fn live_smoke_finds_webkit_helpers() {
        // Find any app that owns a WebKit helper and measure it.
        let mut pids = Vec::new();
        assert!(libproc::list_pids(&mut pids));
        let host = pids.iter().find_map(|&pid| {
            let b = libproc::bsd_info(pid)?;
            libproc::name_of(&b)
                .starts_with("com.apple.WebKit.")
                .then(|| libproc::responsible_pid(pid))
                .flatten()
        });
        let Some(host) = host else {
            println!("no WebKit helper running");
            return;
        };
        let name = process_name(host);
        let mut c = SelfCpu::for_pid(host);
        c.probe();
        let mut buf = SampleBuf::new();
        c.sample(&tick(), &mut buf).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        c.sample(&tick(), &mut buf).unwrap();
        println!(
            "host {host} {name:?}: members {}, self.cpu = {:?}",
            c.members(),
            buf.get(&c.key)
        );
        assert!(c.members() >= 2, "host plus at least one helper");
    }
}
