//! Detectors and alert rules over fixture series: scripted episodes (true positives,
//! near misses) and a recorded quiet hour from a real Mac (`tests/fixtures`), which must
//! produce nothing.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use kelvo_collect::ProcessSample;
use kelvo_schema::{
    AlertCause, AlertSettings, DetectorThresholds, Event, EventDetail, PowerComponent, SeriesKey,
    ThermalState,
};

use super::Detectors;

const T0: i64 = 1_791_158_400_000;
const S: i64 = 1000;
const MIN: i64 = 60 * S;

const FAN0: usize = 0;
const FAN1: usize = 1;
const THERMAL: usize = 2;
const PACKAGE: usize = 3;
const ANE: usize = 4;

fn layout() -> Vec<SeriesKey> {
    [
        "fan.rpm{fan=0}",
        "fan.rpm{fan=1}",
        "thermal.state",
        "power.package",
        "power.ane",
    ]
    .iter()
    .map(|k| SeriesKey::parse(k).unwrap())
    .collect()
}

fn proc(name: &str, pid: i32, cpu: f32, energy: f32) -> ProcessSample {
    ProcessSample {
        pid,
        start_time_us: 1,
        name: Arc::from(name),
        cpu_pct: cpu,
        mem_bytes: 0,
        compressed_bytes: None,
        threads: 1,
        idle_wakeups_per_s: 0.0,
        energy,
        energy_j: 0.0,
        app: None,
        app_main: false,
        disk_read_bps: 0.0,
        disk_write_bps: 0.0,
        net_rx_bps: None,
        net_tx_bps: None,
        gpu_pct: None,
        user: "u".into(),
    }
}

/// A scripted run: one tick per second from `T0`, each a value per series (`NaN` when
/// not sampled) and, every 10 s like the idle processes collector, a batch.
struct Script {
    d: Detectors,
    ts: i64,
    values: [f32; 5],
    /// Each series' sampling period, as the engine passes it (1 s by default).
    periods: [u32; 5],
    events: Vec<Event>,
}

impl Script {
    fn new(alerts: AlertSettings) -> Self {
        let mut d = Detectors::new(DetectorThresholds::DEFAULT, alerts);
        d.bind(&layout());
        Self {
            d,
            ts: T0,
            values: [f32::NAN; 5],
            periods: [1000; 5],
            events: Vec::new(),
        }
    }

    fn quiet() -> Self {
        Self::new(AlertSettings::default())
    }

    /// One tick with `set` applied to a fresh all-`NaN` frame and an optional batch.
    fn tick(&mut self, set: &[(usize, f32)], batch: Option<&[ProcessSample]>) {
        self.values = [f32::NAN; 5];
        for &(i, v) in set {
            self.values[i] = v;
        }
        self.d.on_tick(
            self.ts,
            &self.values,
            &self.periods,
            batch,
            &mut self.events,
        );
        self.ts += S;
    }

    /// `secs` ticks; `f(second)` gives each tick's values, `procs(second)` the batch on
    /// every tenth.
    fn run(
        &mut self,
        secs: i64,
        f: impl Fn(i64) -> Vec<(usize, f32)>,
        procs: impl Fn(i64) -> Vec<ProcessSample>,
    ) {
        for s in 0..secs {
            let batch = (self.ts / S % 10 == 0).then(|| procs(s));
            self.tick(&f(s), batch.as_deref());
        }
    }

    fn kinds(&self) -> Vec<&'static str> {
        self.events.iter().map(|e| e.detail.kind()).collect()
    }
}

fn none(_: i64) -> Vec<ProcessSample> {
    Vec::new()
}

/// Deterministic jitter in [-1, 1): a linear congruential generator over the second.
fn jitter(s: i64, salt: u64) -> f32 {
    let x = (s as u64 ^ salt)
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((x >> 33) as f32 / (1u64 << 31) as f32) - 1.0
}

// ---- fans_ramped -------------------------------------------------------------------

/// A "Fans ramped up · kernel_task + Xcode" episode: idle fans, a build, a
/// ramp from ~1,300 to ~3,500 rpm over 20 s, the fans read every 2 s. One event,
/// attributed to the two busiest processes of the minute before, busiest first.
#[test]
fn fans_ramp_fires_once_with_attribution() {
    let mut s = Script::quiet();
    let rpm = |sec: i64| match sec {
        ..300 => 1300.0 + 40.0 * jitter(sec, 1),
        300..320 => 1300.0 + 110.0 * (sec - 300) as f32,
        _ => 3500.0 + 60.0 * jitter(sec, 2),
    };
    let fans = |sec: i64| {
        if sec % 2 == 0 {
            vec![(FAN0, rpm(sec)), (FAN1, rpm(sec) - 150.0)]
        } else {
            vec![]
        }
    };
    let procs = |sec: i64| {
        if sec < 250 {
            vec![proc("Safari", 3, 12.0, 2.0)]
        } else {
            vec![
                proc("kernel_task", 0, 180.0, 5.0),
                proc("Xcode", 1, 420.0, 40.0),
                proc("Safari", 3, 12.0, 2.0),
            ]
        }
    };
    s.run(600, fans, procs);
    // The build itself is also two sustained processes.
    s.events.retain(|e| e.detail.kind() == "fans_ramped");
    assert_eq!(s.events.len(), 1, "{:?}", s.events);
    let e = &s.events[0];
    assert_eq!(
        e.processes,
        ["Xcode", "kernel_task"],
        "Safari is under the floor"
    );
    assert!(
        (T0 + 300 * S..T0 + 320 * S).contains(&e.ts_ms),
        "fires during the ramp, at {}",
        (e.ts_ms - T0) / S
    );
    assert!(e.start_ms < e.ts_ms && e.start_ms >= e.ts_ms - MIN);
    let EventDetail::FansRamped { from_rpm, to_rpm } = e.detail else {
        panic!("{e:?}")
    };
    assert!(to_rpm - from_rpm >= 1000.0 && from_rpm < 1400.0);
}

/// Attribution leaves out processes that were barely running, even when fewer than two
/// were busy.
#[test]
fn fans_attribution_skips_idle_processes() {
    let mut s = Script::quiet();
    s.run(
        200,
        |sec| vec![(FAN0, if sec < 150 { 1300.0 } else { 3600.0 })],
        |_| {
            vec![
                proc("kernel_task", 0, 240.0, 1.0),
                proc("Dock", 2, 4.0, 0.1),
            ]
        },
    );
    s.events.retain(|e| e.detail.kind() == "fans_ramped");
    assert_eq!(s.events.len(), 1);
    assert_eq!(s.events[0].processes, ["kernel_task"]);
}

/// A slow climb (1,000 rpm over five minutes) and a fan that hunts by a few hundred
/// rpm are not ramps.
#[test]
fn fans_slow_climb_and_hunting_are_quiet() {
    let mut s = Script::quiet();
    s.run(
        600,
        |sec| {
            let v = if sec < 300 {
                1500.0 + 3.4 * sec as f32
            } else {
                2500.0 + 450.0 * jitter(sec, 3)
            };
            vec![(FAN0, v)]
        },
        none,
    );
    assert!(s.events.is_empty(), "{:?}", s.events);
}

/// After a ramp the next one counts only once the fans have settled and the refractory
/// period is over, and it is measured from then: fans that come down and go straight
/// back up inside the period are not a second event, and are not reported late when
/// the period ends either.
#[test]
fn fans_rearm_after_settling() {
    let mut s = Script::quiet();
    let up_down = |sec: i64| {
        // Up at 120 s, down at 180 s, up again at 200 s (inside two minutes) until
        // 260 s, then up again at 300 s.
        let high = matches!(sec, 120..180 | 200..260 | 300..360);
        vec![(FAN0, if high { 4000.0 } else { 1200.0 })]
    };
    s.run(400, up_down, none);
    let at: Vec<(i64, i64)> = s
        .events
        .iter()
        .map(|e| ((e.ts_ms - T0) / S, (e.start_ms - T0) / S))
        .collect();
    assert_eq!(at, [(120, 119), (300, 299)]);
}

/// Fans that stay up are one event however long they stay there, even swinging by more
/// than a rise while up, and a fan that stops and starts again at its minimum speed is
/// not a ramp.
#[test]
fn fans_staying_up_or_restarting_are_one_event() {
    let mut s = Script::quiet();
    s.run(
        1200,
        |sec| {
            let v = match sec {
                ..100 => 1300.0,
                100..300 => 3800.0 + 200.0 * jitter(sec, 4),
                300..700 if (sec / 40) % 2 == 0 => 2600.0,
                300..700 => 3800.0,
                700..800 => 1300.0,
                800..900 => 0.0,
                900..910 => 300.0,
                _ => 1300.0,
            };
            vec![(FAN0, v)]
        },
        none,
    );
    let at: Vec<i64> = s.events.iter().map(|e| (e.ts_ms - T0) / S).collect();
    assert_eq!(at, [100]);
}

/// Fans that settle high after a ramp and never come back within half a rise still
/// re-arm after `fan_rearm_after_ms`, measured from where they are then: a second build
/// pushing them another 1,000 rpm up is reported. Staying high with nothing new is
/// still one event.
#[test]
fn fans_settled_high_rearm_after_ten_minutes() {
    let rpm = |sec: i64| match sec {
        ..100 => 1300.0,
        100..1000 => 3000.0 + 100.0 * jitter(sec, 8),
        _ => 4300.0,
    };
    let mut s = Script::quiet();
    s.run(1200, |sec| vec![(FAN0, rpm(sec))], none);
    let at: Vec<i64> = s.events.iter().map(|e| (e.ts_ms - T0) / S).collect();
    assert_eq!(at, [100, 1000], "{:?}", s.events);
    let EventDetail::FansRamped { from_rpm, to_rpm } = s.events[1].detail else {
        panic!()
    };
    assert!(from_rpm > 2800.0 && to_rpm == 4300.0, "{from_rpm} {to_rpm}");

    let mut s = Script::quiet();
    s.run(2400, |sec| vec![(FAN0, rpm(sec.min(999)))], none);
    assert_eq!(s.events.len(), 1, "{:?}", s.events);
}

// ---- thermal_state -----------------------------------------------------------------

#[test]
fn thermal_change_holds_before_reporting() {
    let mut s = Script::quiet();
    let level = |sec: i64| match sec {
        ..60 => 0.0,
        60..64 => 1.0, // a 4 s flicker: not an event
        64..120 => 0.0,
        120..300 => 1.0,
        300..400 => 2.0,
        _ => 0.0,
    };
    s.run(500, |sec| vec![(THERMAL, level(sec))], none);
    let got: Vec<_> = s
        .events
        .iter()
        .map(|e| match e.detail {
            EventDetail::ThermalState { from, to } => ((e.ts_ms - T0) / S, from, to),
            _ => panic!("{e:?}"),
        })
        .collect();
    use ThermalState::*;
    assert_eq!(
        got,
        [
            (130, Some(Nominal), Fair),
            (310, Some(Fair), Serious),
            (410, Some(Serious), Nominal)
        ]
    );
    assert_eq!(s.events[0].start_ms, T0 + 120 * S);
}

/// The first reading only sets the level, and a reset (sleep) forgets it.
#[test]
fn thermal_first_reading_and_reset_are_silent() {
    let mut s = Script::quiet();
    s.run(30, |_| vec![(THERMAL, 2.0)], none);
    s.d.reset();
    s.run(30, |_| vec![(THERMAL, 0.0)], none);
    assert!(s.events.is_empty(), "{:?}", s.events);
}

// ---- sustained_process -------------------------------------------------------------

#[test]
fn sustained_process_fires_once_per_run() {
    let mut s = Script::quiet();
    let procs = |sec: i64| {
        let builder = match sec {
            ..60 => 20.0,
            // 150% for 4 minutes, one batch at 80% (under the threshold, above the
            // release), 150% again, then hovering at 60 to 140%.
            60..300 => 150.0,
            300..310 => 80.0,
            310..450 => 150.0,
            450..600 => 100.0 + 40.0 * jitter(sec, 4),
            _ => 10.0,
        };
        vec![proc("clang", 9, builder, 1.0), proc("Finder", 4, 2.0, 0.1)]
    };
    s.run(700, |_| vec![], procs);
    assert_eq!(s.kinds(), ["sustained_process"], "{:?}", s.events);
    let e = &s.events[0];
    assert_eq!(e.processes, ["clang"]);
    assert_eq!(e.start_ms, T0 + 60 * S);
    assert_eq!(e.ts_ms, T0 + 180 * S);
    let EventDetail::SustainedProcess { cpu_pct, secs, .. } = &e.detail else {
        panic!()
    };
    assert_eq!(*secs, 120);
    assert!((cpu_pct - 150.0).abs() < 0.01);
}

/// Bursts shorter than the duration, separated by a batch below the threshold, never
/// add up to a run; a process that exits ends its run.
#[test]
fn sustained_process_needs_an_unbroken_run() {
    let mut s = Script::quiet();
    let procs = |sec: i64| {
        let burst = if sec % 100 < 90 { 130.0 } else { 40.0 };
        let mut v = vec![proc("node", 5, burst, 1.0)];
        // Exits for one batch every 100 s.
        if sec % 100 >= 10 {
            v.push(proc("python3", 6, 300.0, 1.0));
        }
        v
    };
    s.run(1000, |_| vec![], procs);
    assert!(s.events.is_empty(), "{:?}", s.events);
}

/// A cargo build: a dozen `rustc` processes over 100% for minutes each, several at
/// once and one after another for half an hour, with `cargo` itself idle. One pill per
/// name per 10 minutes, not one per `rustc`; another name still gets its own.
#[test]
fn sustained_process_cargo_build_is_bounded() {
    let mut s = Script::quiet();
    // rustc pid 1000 + k runs from 60k s for 150 + 30 * (k % 4) s at 120 to 380%.
    let procs = |sec: i64| {
        let mut v = vec![proc("cargo", 900, 3.0, 0.5)];
        for k in 0..30i64 {
            let start = 60 * k;
            let len = 150 + 30 * (k % 4);
            if (start..start + len).contains(&sec) {
                let cpu = 250.0 + 130.0 * jitter(sec, k as u64);
                v.push(proc("rustc", 1000 + k as i32, cpu, 5.0));
            }
        }
        if (900..1100).contains(&sec) {
            v.push(proc("clang", 50, 140.0, 3.0));
        }
        v
    };
    s.run(1800, |_| vec![], procs);
    let got: Vec<(&str, i64)> = s
        .events
        .iter()
        .map(|e| match &e.detail {
            EventDetail::SustainedProcess { process, .. } => (process.as_str(), (e.ts_ms - T0) / S),
            _ => panic!("{e:?}"),
        })
        .collect();
    let rustc: Vec<i64> = got
        .iter()
        .filter(|(n, _)| *n == "rustc")
        .map(|(_, t)| *t)
        .collect();
    assert!(
        (2..=3).contains(&rustc.len()),
        "about one rustc pill per 10 minutes over 30: {got:?}"
    );
    assert!(rustc.windows(2).all(|w| w[1] - w[0] >= 600), "{rustc:?}");
    assert_eq!(
        got.iter().filter(|(n, _)| *n == "clang").count(),
        1,
        "{got:?}"
    );
}

/// Two processes reaching the duration in one batch: two events, a millisecond apart
/// (the store keys events on host, ts and kind).
#[test]
fn same_kind_on_one_tick_gets_distinct_timestamps() {
    let mut s = Script::quiet();
    s.run(
        200,
        |_| vec![],
        |_| vec![proc("a", 1, 200.0, 1.0), proc("b", 2, 200.0, 1.0)],
    );
    let ts: Vec<i64> = s.events.iter().map(|e| e.ts_ms).collect();
    assert_eq!(ts.len(), 2, "{:?}", s.events);
    assert_eq!(ts[1], ts[0] + 1);
}

// ---- power_spike -------------------------------------------------------------------

fn package(base: f32) -> impl Fn(i64) -> f32 {
    move |sec| base + 0.8 * jitter(sec, 5)
}

#[test]
fn power_spike_fires_with_top_energy_process() {
    let mut s = Script::quiet();
    let idle = package(4.0);
    let w = |sec: i64| match sec {
        // A 3 s burst to 25 W (a page load) is too short.
        200..203 => 25.0,
        400..460 => 22.0 + jitter(sec, 6),
        _ => idle(sec),
    };
    let procs = |sec: i64| {
        if (395..470).contains(&sec) {
            vec![
                proc("WindowServer", 1, 30.0, 8.0),
                proc("Photos", 2, 90.0, 60.0),
            ]
        } else {
            vec![proc("WindowServer", 1, 10.0, 3.0)]
        }
    };
    s.run(700, |sec| vec![(PACKAGE, w(sec))], procs);
    assert_eq!(s.kinds(), ["power_spike"], "{:?}", s.events);
    let e = &s.events[0];
    assert_eq!(e.processes, ["Photos"]);
    assert_eq!(e.start_ms, T0 + 400 * S);
    assert_eq!(e.ts_ms, T0 + 410 * S);
    let EventDetail::PowerSpike {
        component,
        watts,
        baseline_watts,
    } = e.detail
    else {
        panic!()
    };
    assert_eq!(component, PowerComponent::Package);
    assert!((3.0..5.0).contains(&baseline_watts), "{baseline_watts}");
    assert!(watts >= 21.0);
}

/// No spike during warm-up; a 20-minute plateau is one spike, and a return after the
/// refractory period is another.
#[test]
fn power_spike_warmup_and_one_event_per_plateau() {
    let mut s = Script::quiet();
    let idle = package(4.0);
    let w = |sec: i64| match sec {
        30..60 => 20.0, // during warm-up
        300..1500 => 20.0,
        // Back up within the refractory period: not reported.
        1600..1660 => 20.0,
        // Back up after it: a second, separate spike.
        1900..1960 => 20.0,
        _ => idle(sec),
    };
    s.run(2000, |sec| vec![(PACKAGE, w(sec))], none);
    let at: Vec<i64> = s.events.iter().map(|e| (e.ts_ms - T0) / S).collect();
    assert_eq!(at, [310, 1910], "{:?}", s.events);
}

/// One reading is not a baseline: after a single idle reading and minutes of gaps (the
/// usual `power.package` on macOS 27, D-043), a burst is not compared with it.
#[test]
fn power_spike_ignores_a_stale_baseline() {
    let mut s = Script::quiet();
    let w = |sec: i64| match sec {
        0 => Some(4.0),
        180..240 => Some(20.0),
        _ => None,
    };
    s.run(
        300,
        |sec| w(sec).map(|v| vec![(PACKAGE, v)]).unwrap_or_default(),
        none,
    );
    assert!(s.events.is_empty(), "{:?}", s.events);
}

/// At a slow base tick the warm-up time passes in a handful of readings; the baseline
/// needs `power_warmup_readings` of them before a spike counts.
#[test]
fn power_spike_needs_enough_readings_to_warm_up() {
    let mut s = Script::quiet();
    s.periods = [30_000; 5];
    let w = |sec: i64| match sec {
        _ if sec % 30 != 0 => None,
        180 | 210 => Some(25.0),
        _ => Some(4.0),
    };
    s.run(
        600,
        |sec| w(sec).map(|v| vec![(PACKAGE, v)]).unwrap_or_default(),
        none,
    );
    assert!(s.events.is_empty(), "{:?}", s.events);
}

/// Tray-only, IOReport reads power every 10 s on a 1 s base tick: readings 10 s apart
/// are the normal spacing, not holes, so a plateau is one spike. Opening a window
/// (readings every second from then) does not restart the warm-up.
#[test]
fn power_spike_at_the_idle_cadence_and_across_a_cadence_switch() {
    let idle = package(4.0);
    let w = |sec: i64| match sec {
        600..900 => 20.0,
        _ => idle(sec),
    };
    let mut s = Script::quiet();
    s.periods = [10_000; 5];
    s.run(
        1200,
        |sec| {
            if sec % 10 == 0 {
                vec![(PACKAGE, w(sec))]
            } else {
                vec![]
            }
        },
        none,
    );
    let at: Vec<i64> = s.events.iter().map(|e| (e.ts_ms - T0) / S).collect();
    assert_eq!(at, [610], "{:?}", s.events);

    // Warm on the 10 s cadence by 150 s; a window opens at 155 s and the spike starts
    // at 160 s, read every second: reported 10 s later, no new warm-up.
    let mut s = Script::quiet();
    s.periods = [10_000; 5];
    s.run(
        155,
        |sec| {
            if sec % 10 == 0 {
                vec![(PACKAGE, idle(sec))]
            } else {
                vec![]
            }
        },
        none,
    );
    s.periods = [1000; 5];
    s.run(
        60,
        |sec| vec![(PACKAGE, if sec >= 5 { 20.0 } else { idle(sec) })],
        none,
    );
    let at: Vec<i64> = s.events.iter().map(|e| (e.ts_ms - T0) / S).collect();
    assert_eq!(at, [170], "{:?}", s.events);
}

/// Two readings minutes apart do not make a 10 s hold; a spike seen reading by reading,
/// after a fresh warm-up, still does.
#[test]
fn power_spike_hold_is_not_bridged_across_gaps() {
    let mut s = Script::quiet();
    let w = |sec: i64| match sec {
        ..300 => Some(4.0 + 0.5 * jitter(sec, 9)),
        400 | 700 => Some(25.0),
        800..1100 => Some(4.0 + 0.5 * jitter(sec, 10)),
        1100..1160 => Some(25.0),
        _ => None,
    };
    s.run(
        1200,
        |sec| w(sec).map(|v| vec![(PACKAGE, v)]).unwrap_or_default(),
        none,
    );
    let at: Vec<i64> = s.events.iter().map(|e| (e.ts_ms - T0) / S).collect();
    assert_eq!(at, [1110], "{:?}", s.events);
}

/// Under sustained load a rise of the minimum watts is not a spike unless it is also
/// well above the baseline in ratio: 20 W to 29 W is load, not a spike.
#[test]
fn power_spike_needs_the_ratio_too() {
    let mut s = Script::quiet();
    let w = |sec: i64| match sec {
        ..900 => 20.0 + jitter(sec, 7),
        900..1000 => 29.0,
        _ => 20.0,
    };
    s.run(1100, |sec| vec![(PACKAGE, w(sec))], none);
    assert!(s.events.is_empty(), "{:?}", s.events);
}

/// The ANE idles at zero: a watt and a half for a while is a spike; a few tenths are
/// not.
#[test]
fn ane_spike_from_zero() {
    let mut s = Script::quiet();
    let w = |sec: i64| match sec {
        200..260 => 0.4,
        400..460 => 1.5,
        _ => 0.0,
    };
    s.run(600, |sec| vec![(ANE, w(sec))], none);
    assert_eq!(s.kinds(), ["power_spike"], "{:?}", s.events);
    assert!(matches!(
        s.events[0].detail,
        EventDetail::PowerSpike {
            component: PowerComponent::Ane,
            ..
        }
    ));
}

// ---- alerts ------------------------------------------------------------------------

fn alerts(hot_process: bool, thermal_serious: bool) -> AlertSettings {
    AlertSettings {
        hot_process,
        thermal_serious,
    }
}

#[test]
fn hot_process_alert_after_five_minutes_with_cooldown() {
    let mut s = Script::new(alerts(true, false));
    // 250% for 6 minutes, rest 2 minutes, again for 6 (inside the 30-minute cooldown),
    // then after the cooldown once more.
    let procs = |sec: i64| {
        let hot = matches!(sec, 0..360 | 480..840 | 2400..2760);
        vec![proc("ffmpeg", 7, if hot { 250.0 } else { 5.0 }, 1.0)]
    };
    s.run(3000, |_| vec![], procs);
    let alerts: Vec<i64> = s
        .events
        .iter()
        .filter(|e| e.detail.kind() == "alert")
        .map(|e| (e.ts_ms - T0) / S)
        .collect();
    assert_eq!(alerts, [300, 2700], "{:?}", s.events);
    let e = s
        .events
        .iter()
        .find(|e| e.detail.kind() == "alert")
        .unwrap();
    assert_eq!(e.processes, ["ffmpeg"]);
    let EventDetail::Alert { cause, rule_id, .. } = &e.detail else {
        panic!()
    };
    assert_eq!(*rule_id, kelvo_schema::AlertRule::hot_process().id);
    assert!(matches!(cause, AlertCause::ProcessCpu { process, .. } if process == "ffmpeg"));
}

#[test]
fn thermal_alert_on_the_edge_with_cooldown() {
    let mut s = Script::new(alerts(false, true));
    let level = |sec: i64| match sec {
        ..100 => 1.0,
        100..400 => 2.0,  // Serious: fires at once, once
        400..500 => 1.0,  // clears
        500..600 => 3.0,  // Critical inside the cooldown: no alert
        600..2100 => 0.0, // clears
        _ => 2.0,         // after the cooldown: fires, and only once however long
    };
    s.run(4200, |sec| vec![(THERMAL, level(sec))], none);
    let alerts: Vec<i64> = s
        .events
        .iter()
        .filter(|e| e.detail.kind() == "alert")
        .map(|e| (e.ts_ms - T0) / S)
        .collect();
    assert_eq!(alerts, [100, 2100]);
}

#[test]
fn alerts_off_by_default_and_switchable() {
    let hot = |_: i64| vec![proc("ffmpeg", 7, 400.0, 1.0)];
    let mut s = Script::quiet();
    s.run(400, |_| vec![(THERMAL, 3.0)], hot);
    assert!(!s.kinds().contains(&"alert"), "{:?}", s.kinds());

    s.d.set_alerts(alerts(true, true));
    s.run(400, |_| vec![(THERMAL, 3.0)], hot);
    let n = s.kinds().iter().filter(|k| **k == "alert").count();
    assert_eq!(n, 2, "{:?}", s.events);

    // Off means off: another hot process, after the cooldown, gets no alert.
    s.d.set_alerts(alerts(false, true));
    let other = |_: i64| vec![proc("x264", 8, 400.0, 1.0)];
    s.run(1800, |_| vec![(THERMAL, 3.0)], none);
    s.run(400, |_| vec![(THERMAL, 3.0)], other);
    assert_eq!(s.kinds().iter().filter(|k| **k == "alert").count(), 2);

    // Turning one on again keeps the other's state: no second thermal alert.
    s.d.set_alerts(alerts(true, true));
    s.run(100, |_| vec![(THERMAL, 3.0)], none);
    assert_eq!(s.kinds().iter().filter(|k| **k == "alert").count(), 2);
}

/// Two processes whose hot runs complete on the same batch: one alert naming both,
/// hottest first, with the hottest as the cause. Before, the second was marked fired
/// and never reported.
#[test]
fn hot_process_alert_names_every_process_completing_together() {
    let mut s = Script::new(alerts(true, false));
    s.run(
        400,
        |_| vec![],
        |_| {
            vec![
                proc("x264", 8, 230.0, 1.0),
                proc("ffmpeg", 7, 390.0, 1.0),
                proc("Finder", 4, 2.0, 0.1),
            ]
        },
    );
    let alerts: Vec<&Event> = s
        .events
        .iter()
        .filter(|e| e.detail.kind() == "alert")
        .collect();
    assert_eq!(alerts.len(), 1, "{:?}", s.events);
    let e = alerts[0];
    assert_eq!(e.processes, ["ffmpeg", "x264"]);
    let EventDetail::Alert { cause, .. } = &e.detail else {
        panic!()
    };
    assert!(
        matches!(cause, AlertCause::ProcessCpu { process, cpu_pct } if process == "ffmpeg" && (*cpu_pct - 390.0).abs() < 0.01),
        "{cause:?}"
    );
}

fn thermal_alerts(s: &Script) -> Vec<i64> {
    s.events
        .iter()
        .filter(|e| e.detail.kind() == "alert")
        .map(|e| (e.ts_ms - T0) / S)
        .collect()
}

/// A restart reads the last alerts from the store: a rule that fired five minutes
/// before the restart is still cooling down, and a condition still met after the
/// restart is the episode it already reported, so it does not fire when the cooldown
/// ends either. Once the condition clears and returns after the cooldown, it fires. An
/// alert older than the cooldown holds nothing.
#[test]
fn alert_cooldown_survives_a_restart() {
    let fired = |ago: i64| Event {
        ts_ms: T0 - ago,
        start_ms: T0 - ago,
        processes: vec![],
        detail: EventDetail::Alert {
            rule_id: kelvo_schema::AlertRule::thermal_serious().id,
            rule_name: "Thermal state Serious or worse".into(),
            cause: AlertCause::ThermalState {
                state: ThermalState::Serious,
            },
        },
    };
    let mut s = Script::new(alerts(false, true));
    // An older alert first: the newest one counts, whatever the order.
    s.d.seed_alert_history(&[fired(5 * MIN), fired(40 * MIN)]);
    s.run(1800, |_| vec![(THERMAL, 2.0)], none);
    assert_eq!(thermal_alerts(&s), [] as [i64; 0], "never cleared");
    s.run(60, |_| vec![(THERMAL, 1.0)], none);
    s.run(10, |_| vec![(THERMAL, 2.0)], none);
    assert_eq!(thermal_alerts(&s), [1860], "cleared, then Serious again");

    // Seeded within the cooldown, but the condition clears first: an episode after the
    // cooldown fires as usual.
    let mut s = Script::new(alerts(false, true));
    s.d.seed_alert_history(&[fired(5 * MIN)]);
    s.run(60, |_| vec![(THERMAL, 0.0)], none);
    s.run(
        1800,
        |sec| vec![(THERMAL, if sec < 1500 { 0.0 } else { 2.0 })],
        none,
    );
    assert_eq!(thermal_alerts(&s), [60 + 1500]);

    let mut s = Script::new(alerts(false, true));
    s.d.seed_alert_history(&[fired(31 * MIN)]);
    s.run(10, |_| vec![(THERMAL, 2.0)], none);
    assert_eq!(thermal_alerts(&s), [0]);
}

/// Switching a rule off and on again keeps its cooldown, and a condition met all along
/// is still the episode it reported: no second alert when the cooldown ends, only after
/// the condition clears and returns.
#[test]
fn alert_cooldown_survives_switching_off_and_on() {
    let mut s = Script::new(alerts(false, true));
    s.run(60, |_| vec![(THERMAL, 2.0)], none);
    s.d.set_alerts(alerts(false, false));
    s.run(60, |_| vec![(THERMAL, 2.0)], none);
    s.d.set_alerts(alerts(false, true));
    s.run(1800, |_| vec![(THERMAL, 2.0)], none);
    assert_eq!(thermal_alerts(&s), [0]);
    s.run(30, |_| vec![(THERMAL, 1.0)], none);
    s.run(10, |_| vec![(THERMAL, 2.0)], none);
    assert_eq!(thermal_alerts(&s), [0, 1950]);
}

/// The process rule after a sleep, within the cooldown: a process hot before and after
/// is the one already reported, and is not reported when the cooldown ends.
#[test]
fn hot_process_still_hot_after_a_reset_is_not_reported_again() {
    let mut s = Script::new(alerts(true, false));
    let hot = |_: i64| vec![proc("ffmpeg", 7, 300.0, 1.0)];
    // Fires at 300 s; the reset near the end of the cooldown (2,100 s) would otherwise
    // let the restarted run complete just after it.
    s.run(1900, |_| vec![], hot);
    s.d.reset();
    s.run(1000, |_| vec![], hot);
    let n = s.kinds().iter().filter(|k| **k == "alert").count();
    assert_eq!(n, 1, "{:?}", s.events);
}

/// A reset outside a reported episode (here a relayout after the reported one cleared
/// and the condition came back, unreported, within the cooldown) starts over: still
/// Serious when the cooldown ends, it fires then.
#[test]
fn reset_after_the_episode_cleared_does_not_hold_the_next_one() {
    let mut s = Script::new(alerts(false, true));
    let level = |sec: i64| match sec {
        ..60 => 2.0,
        60..600 => 0.0,
        _ => 2.0,
    };
    s.run(900, |sec| vec![(THERMAL, level(sec))], none);
    s.d.bind(&layout());
    s.run(1100, |sec| vec![(THERMAL, level(sec + 900))], none);
    assert_eq!(thermal_alerts(&s), [0, 1800]);
}

/// After a reset within the cooldown only the processes the last alert named are held
/// off; another process going hot is new, and fires once the cooldown is over.
#[test]
fn reset_holds_off_only_the_processes_already_reported() {
    let mut s = Script::new(alerts(true, false));
    let procs = |sec: i64| {
        let mut v = vec![proc("ffmpeg", 7, 300.0, 1.0)];
        // Hot from shortly before the reset, not yet reported.
        if sec >= 1750 {
            v.push(proc("node", 9, 300.0, 1.0));
        }
        v
    };
    s.run(1800, |_| vec![], procs);
    s.d.reset();
    s.run(1000, |_| vec![], move |sec| procs(sec + 1800));
    let alerts: Vec<(i64, Vec<String>)> = s
        .events
        .iter()
        .filter(|e| e.detail.kind() == "alert")
        .map(|e| ((e.ts_ms - T0) / S, e.processes.clone()))
        .collect();
    assert_eq!(
        alerts,
        [
            (300, vec!["ffmpeg".to_owned()]),
            (2100, vec!["node".to_owned()])
        ],
        "{:?}",
        s.events
    );
}

/// A restart seeded with a process alert holds off the processes it named, not others.
#[test]
fn seeded_process_alert_holds_off_only_its_processes() {
    let mut s = Script::new(alerts(true, false));
    s.d.seed_alert_history(&[Event {
        ts_ms: T0 - 25 * MIN,
        start_ms: T0 - 30 * MIN,
        processes: vec!["ffmpeg".into()],
        detail: EventDetail::Alert {
            rule_id: kelvo_schema::AlertRule::hot_process().id,
            rule_name: "Process above 200% CPU for 5 minutes".into(),
            cause: AlertCause::ProcessCpu {
                process: "ffmpeg".into(),
                cpu_pct: 300.0,
            },
        },
    }]);
    s.run(
        900,
        |_| vec![],
        |_| vec![proc("ffmpeg", 7, 300.0, 1.0), proc("node", 9, 300.0, 1.0)],
    );
    let alerts: Vec<(i64, Vec<String>)> = s
        .events
        .iter()
        .filter(|e| e.detail.kind() == "alert")
        .map(|e| ((e.ts_ms - T0) / S, e.processes.clone()))
        .collect();
    // The cooldown ends at 300 s; node's run, begun at 0 s, completes then.
    assert_eq!(alerts, [(300, vec!["node".to_owned()])], "{:?}", s.events);
}

// ---- recorded fixtures -------------------------------------------------------------

/// Replays a recorded fixture: `{"interval_ms": n, "ticks": [[fan0, fan1, thermal,
/// package, ane], ...]}` from a real Mac, `null` where a series was not sampled. See
/// `tests/fixtures/README.md` for how they were made. Returns the events and the span.
fn replay(json: &str) -> (Vec<Event>, i64) {
    let r: serde_json::Value = serde_json::from_str(json).unwrap();
    let interval = r["interval_ms"].as_i64().unwrap();
    let ticks = r["ticks"].as_array().unwrap();
    let mut d = Detectors::new(DetectorThresholds::DEFAULT, alerts(true, true));
    d.bind(&layout());
    let mut out = Vec::new();
    for (i, t) in ticks.iter().enumerate() {
        let mut v = [f32::NAN; 5];
        for (slot, x) in v.iter_mut().zip(t.as_array().unwrap()) {
            if let Some(x) = x.as_f64() {
                *slot = x as f32;
            }
        }
        let periods = [u32::try_from(interval).unwrap(); 5];
        d.on_tick(T0 + i as i64 * interval, &v, &periods, None, &mut out);
    }
    (out, ticks.len() as i64 * interval)
}

/// 25 minutes of a real M-series Mac under repeated builds (other agents' cargo
/// runs): the fans ramp from about 1450 to 3750 rpm five times and stop altogether
/// for over a minute, thermal state stays Nominal, and package and ANE power are gaps
/// but for three readings (D-043). Each ramp is reported once, while it happens, and
/// nothing else fires. Recorded with `examples/dump` (`tests/fixtures/README.md`).
#[test]
fn recorded_build_ramps() {
    let (events, span) = replay(include_str!("../../tests/fixtures/busy-25min.json"));
    assert_eq!(span, 1499 * S);
    let at: Vec<(&str, i64)> = events
        .iter()
        .map(|e| (e.detail.kind(), (e.ts_ms - T0) / S))
        .collect();
    // The ramps start at about 10 s, 283 s (from a stopped fan), 470 s, 905 s and
    // 1200 s of the recording.
    assert_eq!(
        at,
        [
            ("fans_ramped", 36),
            ("fans_ramped", 296),
            ("fans_ramped", 494),
            ("fans_ramped", 917),
            ("fans_ramped", 1211),
        ]
    );
    for e in &events {
        let EventDetail::FansRamped { from_rpm, to_rpm } = e.detail else {
            unreachable!()
        };
        assert!(from_rpm >= 1000.0 && to_rpm - from_rpm >= 1000.0, "{e:?}");
        assert!(e.ts_ms - e.start_ms <= MIN, "{e:?}");
    }
}
