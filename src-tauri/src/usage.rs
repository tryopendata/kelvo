//! `query_usage_by_app` and `query_energy_by_app` (D-093, D-099): the engine's per-app use
//! over a range, with what the Quit actions need and what the host measured beyond
//! Kelvo's processes. Which process an app row's Quit acts on is decided here, so every
//! view that offers it agrees.

use kelvo_engine::{UsageApp, UsageProc};

use crate::history::{MetricStats, RangeStats};
use crate::ipc::{
    AppEnergy, AppUsage, EnergyByApp, MetricStat, ProcessEnergy, ProcessUsage, SeriesStats,
    UsageByApp, UsageKey, UsageOther, UsageTotal,
};
use crate::process_signal::SignalRefusal;

/// The host series a remainder is taken from.
pub const CPU_TOTAL: &str = "cpu.total";
pub const GPU_UTIL: &str = "gpu.util";
pub const DISK_READ: &str = "disk.read_total";
pub const DISK_WRITE: &str = "disk.write_total";

/// The engine's key for the IPC one.
pub fn engine_key(by: crate::ipc::UsageKey) -> kelvo_engine::UsageKey {
    use crate::ipc::UsageKey as K;
    match by {
        K::Cpu => kelvo_engine::UsageKey::Cpu,
        K::Gpu => kelvo_engine::UsageKey::Gpu,
        K::Memory => kelvo_engine::UsageKey::Memory,
        K::Disk => kelvo_engine::UsageKey::Disk,
        K::Energy => kelvo_engine::UsageKey::Energy,
    }
}

/// The process an app row's Quit acts on: the running main executable (quitting it
/// quits the app and its helpers), else the only running process.
fn quit_pid(processes: &[UsageProc]) -> Option<i32> {
    let mut running = processes.iter().filter(|p| p.running);
    if let Some(main) = processes.iter().find(|p| p.running && p.app_main) {
        return Some(main.pid);
    }
    match (running.next(), running.next()) {
        (Some(only), None) => Some(only.pid),
        _ => None,
    }
}

/// A running process's refusal, `None` for an exited one.
fn refusal_of(
    p: &UsageProc,
    refusal: &impl Fn(i32, i64, &str) -> Option<SignalRefusal>,
) -> Option<SignalRefusal> {
    if p.running {
        refusal(p.pid, p.start_time_us, &p.name)
    } else {
        None
    }
}

/// The `query_energy_by_app` answer from a usage read sorted by energy. `refusal` is the
/// rule `process_signal` checks, for a running process `(pid, start_time_us, name)`.
pub fn energy_by_app(
    e: kelvo_engine::UsageByApp,
    refusal: impl Fn(i32, i64, &str) -> Option<SignalRefusal>,
) -> EnergyByApp {
    let app = |a: UsageApp| AppEnergy {
        name: a.name.to_string(),
        energy_j: a.energy_j,
        avg_w: a.avg_w,
        quit_pid: quit_pid(&a.processes),
        processes: a
            .processes
            .iter()
            .filter(|p| p.energy_j > 0.0)
            .map(|p| ProcessEnergy {
                pid: p.pid,
                start_time_us: p.start_time_us,
                name: p.name.to_string(),
                energy_j: p.energy_j,
                avg_w: p.avg_w,
                running: p.running,
                refusal: refusal_of(p, &refusal),
            })
            .collect(),
    };
    EnergyByApp {
        from_ms: e.from_ms,
        to_ms: e.to_ms,
        since_ms: e.since_ms,
        measured_ms: e.covered_ms,
        total_j: e.total.energy_j,
        apps: e
            .apps
            .into_iter()
            .filter(|a| a.energy_j > 0.0)
            .map(app)
            .collect(),
    }
}

/// The `query_usage_by_app` answer. `stats` holds the host series named above over the
/// covered part of the range (`None` with history unreadable), `cores` the host's CPU
/// cores.
pub fn usage_by_app(
    e: kelvo_engine::UsageByApp,
    stats: Option<&RangeStats>,
    cores: usize,
    refusal: impl Fn(i32, i64, &str) -> Option<SignalRefusal>,
) -> UsageByApp {
    let other = other(&e, stats, cores);
    let app = |a: UsageApp| AppUsage {
        name: a.name.to_string(),
        cpu_avg_pct: a.cpu_avg_pct,
        gpu_avg_pct: a.gpu_avg_pct,
        mem_peak_bytes: a.mem_peak_b,
        mem_avg_bytes: a.mem_avg_b,
        read_bytes: a.read_b,
        write_bytes: a.write_b,
        energy_j: a.energy_j,
        avg_w: a.avg_w,
        quit_pid: quit_pid(&a.processes),
        processes: a
            .processes
            .iter()
            .map(|p| ProcessUsage {
                pid: p.pid,
                start_time_us: p.start_time_us,
                name: p.name.to_string(),
                cpu_avg_pct: p.cpu_avg_pct,
                gpu_avg_pct: p.gpu_avg_pct,
                mem_peak_bytes: p.mem_peak_b,
                read_bytes: p.read_b,
                write_bytes: p.write_b,
                energy_j: p.energy_j,
                avg_w: p.avg_w,
                running: p.running,
                refusal: refusal_of(p, &refusal),
            })
            .collect(),
    };
    UsageByApp {
        from_ms: e.from_ms,
        to_ms: e.to_ms,
        since_ms: e.since_ms,
        complete_to_ms: e.complete_to_ms,
        covered_ms: e.covered_ms,
        gpu_covered_ms: e.gpu_covered_ms,
        total: UsageTotal {
            cpu_avg_pct: e.total.cpu_avg_pct,
            gpu_avg_pct: e.total.gpu_avg_pct,
            read_bytes: e.total.read_b,
            write_bytes: e.total.write_b,
            energy_j: e.total.energy_j,
            avg_w: e.total.avg_w,
        },
        other,
        apps: e.apps.into_iter().map(app).collect(),
    }
}

/// Where the remainder's host series are read: the covered part of the usage range.
pub fn remainder_range(e: &kelvo_engine::UsageByApp) -> Option<(i64, i64)> {
    let from = e.since_ms?.max(e.from_ms);
    (e.covered_ms > 0 && from < e.to_ms).then_some((from, e.to_ms))
}

/// The host's series less the apps' totals, clamped at 0. Averages are compared with
/// averages, each over its own measured time; bytes with bytes over the same range.
fn other(e: &kelvo_engine::UsageByApp, stats: Option<&RangeStats>, cores: usize) -> UsageOther {
    let stat = |m: &str| -> Option<&MetricStats> {
        stats?
            .metrics
            .iter()
            .find(|s| s.metric == m && s.measured_ms > 0)
    };
    let mut clamped: Vec<UsageKey> = Vec::new();
    // A remainder below `-slack` means the two disagreed beyond rounding.
    let mut less = |key: UsageKey, host: f64, apps: f64, slack: f64| {
        let d = host - apps;
        if d < -slack && !clamped.contains(&key) {
            clamped.push(key);
        }
        d.max(0.0)
    };
    let measured = e.covered_ms > 0;
    let cpu = stat(CPU_TOTAL)
        .and_then(|s| s.avg)
        .filter(|_| measured && cores > 0)
        .map(|host| less(UsageKey::Cpu, host * cores as f64, e.total.cpu_avg_pct, 1.0));
    let gpu = stat(GPU_UTIL)
        .and_then(|s| s.avg)
        .zip(e.total.gpu_avg_pct)
        .map(|(host, apps)| less(UsageKey::Gpu, host, apps, 1.0));
    let read = stat(DISK_READ).filter(|_| measured).map(|s| {
        less(
            UsageKey::Disk,
            s.integral,
            e.total.read_b,
            0.01 * s.integral,
        )
    });
    let write = stat(DISK_WRITE).filter(|_| measured).map(|s| {
        less(
            UsageKey::Disk,
            s.integral,
            e.total.write_b,
            0.01 * s.integral,
        )
    });
    UsageOther {
        cpu_avg_pct: cpu,
        gpu_avg_pct: gpu,
        read_bytes: read,
        write_bytes: write,
        clamped,
    }
}

/// The `query_series_stats` answer.
pub fn series_stats(s: RangeStats) -> SeriesStats {
    SeriesStats {
        from_ms: s.from_ms,
        to_ms: s.to_ms,
        metrics: s
            .metrics
            .into_iter()
            .map(|m| MetricStat {
                metric: m.metric.to_owned(),
                measured_ms: u64::try_from(m.measured_ms).unwrap_or(0),
                avg: m.avg,
                max: m.max,
                integral: m.integral,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use kelvo_engine::UsageTotal as EngineTotal;

    use super::*;

    fn p(pid: i32, running: bool, main: bool) -> UsageProc {
        UsageProc {
            pid,
            start_time_us: 1,
            name: format!("p{pid}").into(),
            cpu_avg_pct: 0.0,
            gpu_avg_pct: None,
            mem_peak_b: 0,
            read_b: 0.0,
            write_b: 0.0,
            energy_j: 1.0,
            avg_w: 0.1,
            running,
            app_main: main,
        }
    }

    fn app(name: &str, processes: Vec<UsageProc>) -> UsageApp {
        UsageApp {
            name: name.into(),
            cpu_avg_pct: 0.0,
            gpu_avg_pct: None,
            mem_peak_b: 0,
            mem_avg_b: 0,
            read_b: 0.0,
            write_b: 0.0,
            energy_j: 2.0,
            avg_w: 0.2,
            processes,
        }
    }

    fn stat(metric: &'static str, avg: f64, measured_ms: i64) -> MetricStats {
        MetricStats {
            metric,
            measured_ms,
            avg: Some(avg),
            max: Some(avg),
            integral: avg * measured_ms as f64 / 1_000.0,
        }
    }

    #[test]
    fn quit_targets_the_main_executable_then_a_sole_process() {
        // Chrome: helpers and the main process; Quit goes to the main one.
        assert_eq!(quit_pid(&[p(2, true, false), p(1, true, true)]), Some(1));
        // The main process exited: no single target among the helpers left.
        assert_eq!(
            quit_pid(&[p(2, true, false), p(3, true, false), p(1, false, true)]),
            None
        );
        // A CLI with one running process.
        assert_eq!(quit_pid(&[p(5, false, false), p(6, true, false)]), Some(6));
        assert_eq!(quit_pid(&[p(5, false, false)]), None);
    }

    #[test]
    fn exited_processes_carry_no_refusal() {
        let e = kelvo_engine::UsageByApp {
            from_ms: 0,
            to_ms: 10_000,
            since_ms: Some(0),
            covered_ms: 10_000,
            total: EngineTotal {
                energy_j: 2.0,
                ..EngineTotal::default()
            },
            apps: vec![app(
                "WindowServer",
                vec![p(1, true, false), p(2, false, false)],
            )],
            ..kelvo_engine::UsageByApp::default()
        };
        let out = energy_by_app(e, |_, _, _| Some(SignalRefusal::WindowServer));
        let procs = &out.apps[0].processes;
        assert_eq!(procs[0].refusal, Some(SignalRefusal::WindowServer));
        assert_eq!(procs[1].refusal, None);
        assert_eq!(out.apps[0].quit_pid, Some(1));
        assert_eq!(out.measured_ms, 10_000);
    }

    #[test]
    fn the_remainder_is_the_host_less_the_apps_clamped() {
        let e = kelvo_engine::UsageByApp {
            from_ms: 0,
            to_ms: 60_000,
            since_ms: Some(0),
            covered_ms: 60_000,
            gpu_covered_ms: 60_000,
            total: EngineTotal {
                cpu_avg_pct: 150.0,
                gpu_avg_pct: Some(30.0),
                read_b: 1_000.0,
                write_b: 9_000.0,
                ..EngineTotal::default()
            },
            ..kelvo_engine::UsageByApp::default()
        };
        let stats = RangeStats {
            from_ms: 0,
            to_ms: 60_000,
            metrics: vec![
                // 25% of 8 cores is 200% of one core.
                stat(CPU_TOTAL, 25.0, 60_000),
                stat(GPU_UTIL, 40.0, 60_000),
                stat(DISK_READ, 100.0, 60_000),
                // 6,000 bytes against the apps' 9,000: the apps were sampled differently.
                stat(DISK_WRITE, 100.0, 60_000),
            ],
        };
        let o = other(&e, Some(&stats), 8);
        assert_eq!(o.cpu_avg_pct, Some(50.0));
        assert_eq!(o.gpu_avg_pct, Some(10.0));
        assert_eq!(o.read_bytes, Some(5_000.0));
        assert_eq!(o.write_bytes, Some(0.0));
        // Only the key whose remainder was cut says so.
        assert_eq!(o.clamped, [UsageKey::Disk]);

        // Nothing covered, or no host series: no remainder rather than a wrong one.
        let none = other(
            &kelvo_engine::UsageByApp {
                covered_ms: 0,
                ..e.clone()
            },
            Some(&stats),
            8,
        );
        assert_eq!((none.cpu_avg_pct, none.read_bytes), (None, None));
        assert_eq!(other(&e, None, 8).cpu_avg_pct, None);
    }

    #[test]
    fn the_remainder_reads_only_the_covered_part() {
        let e = kelvo_engine::UsageByApp {
            from_ms: 0,
            to_ms: 60_000,
            since_ms: Some(25_000),
            covered_ms: 35_000,
            ..kelvo_engine::UsageByApp::default()
        };
        assert_eq!(remainder_range(&e), Some((25_000, 60_000)));
        let never = kelvo_engine::UsageByApp { covered_ms: 0, ..e };
        assert_eq!(remainder_range(&never), None);
    }
}
