//! CPU load: `cpu.total`, `cpu.user`, `cpu.system`, `cpu.load{core}` and `cpu.loadavg`.
//!
//! Reads `host_processor_info(PROCESSOR_CPU_LOAD_INFO)` directly rather than through
//! sysinfo: sysinfo only exposes a combined usage per core, and the catalog needs the
//! user/system split. Tick counters are `u32` and wrap; deltas use wrapping
//! subtraction.
//!
//! Core labels come from the `hw.perflevel*` sysctls: `P0..` for performance cores and
//! `E0..` for efficiency cores. Logical CPUs are numbered from the lowest perflevel (the
//! most efficient) upward, so the E cores come first. Verified on an M3 Max against
//! `IODeviceTree:/cpus/cpuN` `cluster-type` (cpu0-3 are E, cpu4-15 are P).

use std::ffi::CString;

use kelvo_schema::{CoreKind, Entitlement, Labels, MetricId, Module, SeriesKey};

use super::sysctl;
use crate::{Cadence, CollectError, Collector, CollectorId, Every, Probe, SampleBuf, Tick};

/// `cpu.loadavg` is sampled at most every 5 s (v1-local-monitor.md 6.1).
const LOADAVG_MS: u32 = 5_000;

type Ticks = [u32; 4];

pub struct Cpu {
    total: SeriesKey,
    user: SeriesKey,
    system: SeriesKey,
    cores: Vec<SeriesKey>,
    loadavg: [SeriesKey; 3],
    loadavg_every: Every,
    prev: Vec<Ticks>,
    cur: Vec<Ticks>,
    has_prev: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        let m = MetricId::from_static;
        let la = |w| SeriesKey::new(m("cpu.loadavg"), Labels::single("window", w));
        Self {
            total: SeriesKey::bare(m("cpu.total")),
            user: SeriesKey::bare(m("cpu.user")),
            system: SeriesKey::bare(m("cpu.system")),
            cores: Vec::new(),
            loadavg: [la("1"), la("5"), la("15")],
            loadavg_every: Every::new(LOADAVG_MS),
            prev: Vec::new(),
            cur: Vec::new(),
            has_prev: false,
        }
    }
}

/// Per-core labels and kinds in logical-CPU order, from the `hw.perflevel*` sysctls.
/// Falls back to `0..n` with no kind when the sysctls are missing.
pub fn core_topology() -> Vec<(String, Option<CoreKind>)> {
    let ncpu = sysctl::int(c"hw.ncpu").unwrap_or(0).max(0) as usize;
    let nlevels = sysctl::int(c"hw.nperflevels").unwrap_or(0).max(0);
    let mut levels = Vec::new();
    for i in 0..nlevels {
        let name = CString::new(format!("hw.perflevel{i}.name")).ok();
        let count = CString::new(format!("hw.perflevel{i}.logicalcpu")).ok();
        let (Some(name), Some(count)) = (name, count) else {
            continue;
        };
        let (Some(name), Some(count)) = (sysctl::string(&name), sysctl::int(&count)) else {
            continue;
        };
        levels.push((name, count.max(0) as usize));
    }
    labels_from_perflevels(&levels, ncpu)
}

/// `levels` is in sysctl order (perflevel0 is the fastest). Logical CPUs start with the
/// last level.
fn labels_from_perflevels(
    levels: &[(String, usize)],
    ncpu: usize,
) -> Vec<(String, Option<CoreKind>)> {
    let mut out = Vec::with_capacity(ncpu);
    for (name, count) in levels.iter().rev() {
        let (prefix, kind) = match name.as_str() {
            "Performance" => ("P", Some(CoreKind::Performance)),
            "Efficiency" => ("E", Some(CoreKind::Efficiency)),
            _ => ("C", None),
        };
        for i in 0..*count {
            out.push((format!("{prefix}{i}"), kind));
        }
    }
    if out.len() != ncpu {
        // Unknown layout (Intel, or the sysctls disagree with hw.ncpu): plain indices.
        return (0..ncpu).map(|i| (i.to_string(), None)).collect();
    }
    out
}

/// Reads per-CPU tick counters into `out` (cleared first).
fn read_ticks(out: &mut Vec<Ticks>) -> Result<(), CollectError> {
    let mut ncpu: libc::natural_t = 0;
    let mut info: libc::processor_info_array_t = std::ptr::null_mut();
    let mut count: libc::mach_msg_type_number_t = 0;
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: all out-pointers are valid locals. On success the kernel vm_allocates
    // `count` integers at `info`, which we deallocate below.
    let kr = unsafe {
        libc::host_processor_info(
            mach2::mach_init::mach_host_self(),
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut ncpu,
            &mut info,
            &mut count,
        )
    };
    if kr != libc::KERN_SUCCESS || info.is_null() {
        return Err(CollectError::Os {
            call: "host_processor_info",
            code: i64::from(kr),
        });
    }
    let len = count as usize;
    // SAFETY: the kernel returned `count` contiguous `integer_t`s at `info`.
    let ints = unsafe { std::slice::from_raw_parts(info, len) };
    out.clear();
    let state_max = libc::CPU_STATE_MAX as usize;
    for chunk in ints.chunks_exact(state_max).take(ncpu as usize) {
        let mut t = [0u32; 4];
        for (dst, src) in t.iter_mut().zip(chunk) {
            *dst = *src as u32;
        }
        out.push(t);
    }
    // SAFETY: `info`/`count` describe the region the kernel allocated in our task.
    unsafe {
        libc::vm_deallocate(
            mach2::traps::mach_task_self(),
            info as libc::vm_address_t,
            len * size_of::<libc::integer_t>(),
        );
    }
    Ok(())
}

/// Busy, user (+nice), system and total tick deltas between two readings.
fn delta(prev: &Ticks, cur: &Ticks) -> (u64, u64, u64) {
    let d = |i: usize| u64::from(cur[i].wrapping_sub(prev[i]));
    let user = d(libc::CPU_STATE_USER as usize) + d(libc::CPU_STATE_NICE as usize);
    let system = d(libc::CPU_STATE_SYSTEM as usize);
    let idle = d(libc::CPU_STATE_IDLE as usize);
    (user, system, user + system + idle)
}

fn pct(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (part as f64 * 100.0 / total as f64) as f32
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
        let topo = core_topology();
        self.cores = topo
            .iter()
            .map(|(label, _)| {
                SeriesKey::new(
                    MetricId::from_static("cpu.load"),
                    Labels::single("core", label),
                )
            })
            .collect();
        self.has_prev = false;
        self.loadavg_every.reset();
        let mut series = vec![self.total.clone(), self.user.clone(), self.system.clone()];
        series.extend(self.cores.iter().cloned());
        series.extend(self.loadavg.iter().cloned());
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        if self.loadavg_every.due(tick) {
            let mut avg = [0f64; 3];
            // SAFETY: `avg` has room for the 3 values requested.
            let n = unsafe { libc::getloadavg(avg.as_mut_ptr(), 3) };
            if n == 3 {
                for (key, v) in self.loadavg.iter().zip(avg) {
                    out.push(key, v as f32);
                }
            }
        }

        read_ticks(&mut self.cur)?;
        if self.has_prev && self.cur.len() == self.prev.len() {
            let (mut u, mut s, mut t) = (0, 0, 0);
            for ((key, p), c) in self.cores.iter().zip(&self.prev).zip(&self.cur) {
                let (cu, cs, ct) = delta(p, c);
                out.push(key, pct(cu + cs, ct));
                u += cu;
                s += cs;
                t += ct;
            }
            out.push(&self.total, pct(u + s, t));
            out.push(&self.user, pct(u, t));
            out.push(&self.system, pct(s, t));
        }
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.has_prev = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn efficiency_cores_come_first() {
        let levels = vec![("Performance".to_owned(), 3), ("Efficiency".to_owned(), 2)];
        let labels: Vec<_> = labels_from_perflevels(&levels, 5)
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(labels, ["E0", "E1", "P0", "P1", "P2"]);
    }

    #[test]
    fn mismatched_count_falls_back_to_indices() {
        let levels = vec![("Performance".to_owned(), 3)];
        let topo = labels_from_perflevels(&levels, 4);
        assert_eq!(topo.first(), Some(&("0".to_owned(), None)));
        assert_eq!(topo.len(), 4);
    }

    #[test]
    fn delta_handles_counter_wrap() {
        let prev = [u32::MAX - 9, 0, u32::MAX, 0];
        let cur = [10, 20, 69, 0];
        // user wrapped by 20, system 20, idle wrapped by 70.
        let (u, s, t) = delta(&prev, &cur);
        assert_eq!((u, s, t), (20, 20, 110));
        assert!((pct(u + s, t) - 36.363_636).abs() < 1e-3);
    }

    #[test]
    #[ignore = "reads the live CPU counters; run by hand on a Mac"]
    fn live_smoke() {
        let mut c = Cpu::new();
        let Probe::Supported(series) = c.probe() else {
            panic!("cpu unsupported")
        };
        println!("series: {}", series.len());
        let mut buf = SampleBuf::new();
        for n in 0..2 {
            buf.clear();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: n * u64::from(LOADAVG_MS) * 1_000_000,
                interval_ms: 1_000,
            };
            c.sample(&tick, &mut buf).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        for s in buf.values() {
            println!("{} = {:.1}", s.key, s.value);
        }
        assert!(buf.get(&c.total).is_some());
    }
}
