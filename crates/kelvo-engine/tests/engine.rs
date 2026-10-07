//! The engine driven by a fake ticker, fake power signals and fake collectors, writing to
//! a real SQLite store.

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use kelvo_collect::{Cadence, Interest};
use kelvo_engine::{Bus, BusMsg, LiveHub, Source, SourceSink};
use kelvo_schema::settings::MenuBarMode;
use kelvo_schema::{
    Catalog, GapReason, Module, ModuleCap, PowerSource, SyncKinds, Tier, UnsupportedReason,
};
use kelvo_store::{CursorRead, PageRow};

fn s10_rows(h: &Harness) -> (Vec<PageRow>, Vec<Vec<String>>) {
    let mut r = h.reader();
    let CursorRead::Page(page) = r
        .read_after(h.host, Tier::S10, None, 1_000, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("truncated");
    };
    let mut layouts = vec![Vec::new(); page.layouts.len() + 1];
    for l in &page.layouts {
        let i = l.layout_no as usize;
        if layouts.len() <= i {
            layouts.resize(i + 1, Vec::new());
        }
        layouts[i] = l.series.iter().map(|k| k.to_string()).collect();
    }
    let mut rows = page.rows;
    rows.sort_by_key(|r| (r.bucket_ts, r.layout_no));
    (rows, layouts)
}

fn gaps(h: &Harness) -> Vec<kelvo_schema::Gap> {
    h.reader().gaps(h.host, 0, i64::MAX).unwrap()
}

fn cpu_and_loadavg() -> (
    Vec<Box<dyn kelvo_collect::Collector>>,
    FakeHandle,
    FakeHandle,
) {
    let (fast, fh) = fake("fast", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let (slow, sh) = fake(
        "slow",
        Cadence::Every(5_000),
        &[Module::Cpu],
        &["cpu.loadavg{window=1}"],
    );
    (vec![fast, slow], fh, sh)
}

#[test]
fn layout_comes_before_frames_and_unsampled_series_are_nan() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("layout-first", collectors, &settings_all_on());
    h.visible();
    h.ticks(7);
    let msgs = h.drain();
    let first_layout = msgs
        .iter()
        .position(|m| matches!(m, BusMsg::Layout(_)))
        .unwrap();
    let first_frame = msgs
        .iter()
        .position(|m| matches!(m, BusMsg::Frame(_)))
        .unwrap();
    assert!(first_layout < first_frame, "Layout is published first");
    let frames: Vec<_> = msgs
        .into_iter()
        .filter_map(|m| match m {
            BusMsg::Frame(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(frames.len(), 7);
    for (n, f) in frames.iter().enumerate() {
        assert_eq!(f.layout.layout_no, 1);
        assert_eq!(value(f, "cpu.total", false), n as f32);
        let raw = value(f, "cpu.loadavg{window=1}", false);
        if n % 5 == 0 {
            assert_eq!(raw, n as f32, "sampled on its cadence");
        } else {
            assert!(
                raw.is_nan(),
                "tick {n}: unsampled series is NaN in the frame"
            );
        }
        // The held value covers the ticks in between.
        let held = value(f, "cpu.loadavg{window=1}", true);
        assert_eq!(held, (n - n % 5) as f32, "tick {n}: held value");
    }
    // The Snapshot is built from held values: load average present on an off tick.
    let snap = frames[3].snapshot(h.host, &Catalog::builtin()).unwrap();
    let cpu = snap.cpu.unwrap();
    assert_eq!(cpu.total, Some(3.0));
    assert_eq!(cpu.load_avg.len(), 1);
    assert_eq!(cpu.load_avg[0].value, Some(0.0));
}

#[test]
fn held_value_goes_stale_after_two_missed_samples() {
    let (collectors, fast, _) = cpu_and_loadavg();
    let mut h = Harness::new("stale", collectors, &settings_all_on());
    h.ticks(2);
    fast.set_emit(false);
    h.ticks(3);
    let frames = h.frames();
    let held: Vec<f32> = frames.iter().map(|f| value(f, "cpu.total", true)).collect();
    assert_eq!(held[..4], [0.0, 1.0, 1.0, 1.0], "held for 2.5 intervals");
    assert!(
        held[4].is_nan(),
        "third missed sample: a gap, not a frozen value"
    );
    let snap = frames[4].snapshot(h.host, &Catalog::builtin()).unwrap();
    assert_eq!(snap.cpu.unwrap().total, None);
}

fn hold(f: &kelvo_engine::LiveFrame, key: &str) -> u32 {
    let i = f
        .layout
        .series
        .iter()
        .position(|k| k.to_string() == key)
        .unwrap();
    f.holds[i]
}

#[test]
fn holds_follow_each_series_period_and_are_shared_between_frames() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("holds", collectors, &settings_all_on());
    h.visible();
    h.ticks(3);
    let frames = h.frames();
    assert_eq!(hold(&frames[0], "cpu.total"), 2_500, "2.5 x the 1 s tick");
    assert_eq!(
        hold(&frames[0], "cpu.loadavg{window=1}"),
        12_500,
        "2.5 x 5 s"
    );
    assert!(
        Arc::ptr_eq(&frames[1].holds, &frames[2].holds),
        "unchanged periods share one Arc"
    );
}

#[test]
fn holds_follow_an_adaptive_collector_to_its_idle_period() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["cpu.total"],
    );
    ph.0.lock().unwrap().processes = 40;
    let mut h = Harness::new("holds-adaptive", vec![procs], &settings_all_on());
    h.ticks(2);
    let idle = h.frames();
    assert_eq!(
        hold(idle.last().unwrap(), "cpu.total"),
        75_000,
        "nobody looks: in the background, 2.5 x 30 s (D-094)"
    );
    h.visible();
    h.ctl.set_process_interest(Some(0));
    h.ticks(2);
    let live = h.frames();
    assert_eq!(
        hold(live.last().unwrap(), "cpu.total"),
        2_500,
        "a window looks"
    );
}

#[test]
fn buckets_close_on_wall_clock_boundaries_and_ignore_held_values() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("buckets", collectors, &settings_all_on());
    h.visible();
    // Tick n lands at T0 + (n + 1) s. Bucket [T0, T0+10s) holds n = 0..=8, the next
    // n = 9..=18; n = 19 at T0 + 20 s closes the second.
    h.ticks(20);
    let (rows, layouts) = s10_rows(&h);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].bucket_ts, T0);
    assert_eq!(rows[1].bucket_ts, T0 + 10_000);
    let series = &layouts[rows[0].layout_no as usize];
    let total = series.iter().position(|s| s == "cpu.total").unwrap();
    let load = series
        .iter()
        .position(|s| s == "cpu.loadavg{window=1}")
        .unwrap();
    let stats = |row: &PageRow, i: usize| row.stats[i * 3..i * 3 + 3].to_vec();
    assert_eq!(stats(&rows[0], total), vec![0.0, 8.0, 4.0]);
    assert_eq!(stats(&rows[1], total), vec![9.0, 18.0, 13.5]);
    // Load average was measured at n = 0, 5 and n = 10, 15 only. Held values would pull
    // the average to (0*5 + 5*4) / 9 instead.
    assert_eq!(stats(&rows[0], load), vec![0.0, 5.0, 2.5]);
    assert_eq!(stats(&rows[1], load), vec![10.0, 15.0, 12.5]);

    // The M1 row closes at the minute boundary: n = 59 lands at T0 + 60 s.
    h.ticks(40);
    let mut r = h.reader();
    let CursorRead::Page(m1) = r
        .read_after(h.host, Tier::M1, None, 100, SyncKinds::ALL)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(m1.rows.len(), 1);
    assert_eq!(m1.rows[0].bucket_ts, T0);
}

#[test]
fn skipped_ticks_thin_a_bucket_without_a_gap() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("skipped", collectors, &settings_all_on());
    h.visible();
    h.ticks(3); // n 0..=2 at T0+1..3 s
    h.clock.skip();
    h.clock.skip(); // T0+4, T0+5 never delivered
    h.ticks(7); // n 3..=9 at T0+6..12 s
    let (rows, _) = s10_rows(&h);
    assert_eq!(rows.len(), 1);
    assert!(gaps(&h).is_empty(), "two skipped ticks are not a gap");
}

#[test]
fn layout_change_mid_bucket_writes_one_row_per_layout() {
    let (disk, dh) = fake(
        "disk",
        Cadence::EveryTick,
        &[Module::Disk],
        &["disk.read{dev=disk0}"],
    );
    let mut h = Harness::new("layout-change", vec![disk], &settings_all_on());
    h.ticks(3); // T0+1..3 s, layout 1
    dh.set_keys(&["disk.read{dev=disk0}", "disk.read{dev=disk4}"]);
    h.ctl.reprobe(&[Module::Disk]);
    h.pump();
    h.ticks(4); // T0+4..7 s, layout 2
    let msgs = h.drain();
    let l2 = msgs
        .iter()
        .position(|m| matches!(m, BusMsg::Layout(l) if l.layout_no == 2))
        .expect("layout 2 published");
    let first_l2_frame = msgs
        .iter()
        .position(|m| matches!(m, BusMsg::Frame(f) if f.layout.layout_no == 2))
        .unwrap();
    assert!(l2 < first_l2_frame);
    assert!(
        msgs.iter()
            .any(|m| matches!(m, BusMsg::Caps(c) if c.revision == 2)),
        "series count changed: new capabilities revision"
    );

    h.ticks(4); // crosses T0 + 10 s
    let (rows, layouts) = s10_rows(&h);
    let first: Vec<_> = rows.iter().filter(|r| r.bucket_ts == T0).collect();
    assert_eq!(first.len(), 2, "one row per layout in the same bucket");
    let lens: Vec<usize> = first
        .iter()
        .map(|r| layouts[r.layout_no as usize].len())
        .collect();
    assert_eq!(lens, vec![1, 2]);
    // The old layout's row holds only its own ticks (n 0..=2).
    assert_eq!(first[0].stats, vec![0.0, 2.0, 1.0]);

    // Backfill splits at the layout change.
    let segs = h.live.backfill(0);
    let shape: Vec<_> = segs
        .iter()
        .map(|s| (s.layout.layout_no, s.rows.len()))
        .collect();
    assert_eq!(shape, vec![(1, 3), (2, 8)]);
}

#[test]
fn sleep_gap_spans_the_continuous_clock() {
    let (collectors, fast, _) = cpu_and_loadavg();
    let mut h = Harness::new("sleep", collectors, &settings_all_on());
    h.ticks(4); // last tick at T0 + 4 s
    let slept_at = h.clock.now();
    let ack = h.power.will_sleep(slept_at);
    h.pump();
    assert!(
        ack.recv_timeout(Duration::from_secs(1)).is_ok(),
        "engine acks after flushing"
    );
    assert!(!h.clock.is_running(), "ticker stopped for sleep");
    assert!(h.ctl.status().asleep);
    // The partial bucket was flushed before sleeping.
    let (rows, _) = s10_rows(&h);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].bucket_ts, T0);
    let open = gaps(&h);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].reason, GapReason::Sleep);
    assert_eq!(open[0].end_ms, None);

    // Five minutes asleep; NTP steps the wall clock 30 s forward on wake.
    h.clock.advance(Duration::from_secs(300));
    h.clock.jump_wall(30_000);
    let probes_before = fast.probes();
    h.power.did_wake(h.clock.now());
    h.pump();
    assert!(h.clock.is_running(), "ticker restarted");
    let g = gaps(&h);
    assert_eq!(g[0].reason, GapReason::Sleep);
    assert_eq!(g[0].start_ms, slept_at.wall_ms);
    assert_eq!(
        g[0].end_ms,
        Some(slept_at.wall_ms + 300_000),
        "span from the continuous clock, not the stepped wall clock"
    );
    h.ticks(2);
    assert!(fast.probes() > probes_before, "re-probed after wake");
    // The 30 s the wall clock jumped is a `clock_changed` gap right after the sleep gap
    // (D-064), so the timeline has no unexplained hole; no stall gap after a real wake.
    let g = gaps(&h);
    assert_eq!(g.len(), 2, "{g:?}");
    assert_eq!(g[1].reason, GapReason::ClockChanged);
    assert_eq!(g[1].start_ms, slept_at.wall_ms + 300_000);
    assert_eq!(g[1].end_ms, Some(slept_at.wall_ms + 330_000));
}

#[test]
fn sleep_inside_a_bucket_does_not_lose_the_first_half() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("sleep-short", collectors, &settings_all_on());
    h.visible();
    h.ticks(3); // n 0..=2 at T0 + 1..3 s
    let at = h.clock.now();
    let _ack = h.power.will_sleep(at);
    h.pump();
    h.clock.advance(Duration::from_secs(2));
    h.power.did_wake(h.clock.now());
    h.pump();
    // Wake at T0 + 5 s; ticks resume at T0 + 6..11 s: n 3..=6 complete the first
    // bucket, n 7 at T0 + 10 s closes it.
    h.ticks(6);
    let (rows, layouts) = s10_rows(&h);
    let total = layouts[rows[0].layout_no as usize]
        .iter()
        .position(|s| s == "cpu.total")
        .unwrap();
    assert_eq!(rows[0].bucket_ts, T0);
    assert_eq!(
        rows[0].stats[total * 3..total * 3 + 3],
        [0.0, 6.0, 3.0],
        "the flushed partial row was replaced by the full bucket"
    );
}

#[test]
fn a_missed_sleep_event_becomes_a_stall_gap() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("stall", collectors, &settings_all_on());
    h.visible();
    h.ticks(2); // last at T0 + 2 s
    h.clock.advance(Duration::from_secs(60));
    h.ticks(1); // T0 + 63 s
    let g = gaps(&h);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].reason, GapReason::Sleep);
    assert_eq!(g[0].start_ms, T0 + 3_000);
    assert_eq!(g[0].end_ms, Some(T0 + 63_000));
}

#[test]
fn pause_writes_a_paused_gap_and_keeps_the_bus() {
    let (collectors, fast, _) = cpu_and_loadavg();
    let mut h = Harness::new("pause", collectors, &settings_all_on());
    h.ticks(2);
    h.drain();
    h.ctl.set_paused(true);
    h.pump();
    let paused_at = h.clock.now().wall_ms;
    assert!(!h.clock.is_running(), "collectors stopped");
    assert!(!h.tick(), "no ticks while paused");
    let msgs = h.drain();
    assert!(
        msgs.iter()
            .any(|m| matches!(m, BusMsg::Status(s) if s.paused)),
        "status still published"
    );
    let samples = fast.samples();
    h.clock.advance(Duration::from_secs(30));
    h.ctl.set_paused(false);
    h.pump();
    let g = gaps(&h);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].reason, GapReason::Paused);
    assert_eq!(g[0].start_ms, paused_at);
    assert_eq!(g[0].end_ms, Some(paused_at + 30_000));
    assert_eq!(fast.samples(), samples, "nothing sampled while paused");
    h.ticks(1);
    assert_eq!(fast.samples(), samples + 1);
}

type GapRow = (GapReason, Option<Module>, i64, Option<i64>);

fn gap_rows(mut r: kelvo_store::Reader, host: kelvo_schema::HostId) -> Vec<GapRow> {
    let mut g: Vec<_> = r
        .gaps(host, 0, i64::MAX)
        .unwrap()
        .into_iter()
        .map(|g| (g.reason, g.module, g.start_ms, g.end_ms))
        .collect();
    g.sort_by_key(|g| (g.2, g.1));
    g
}

/// Gaps as a reader sees them without anyone flushing the writer: only what the engine
/// itself committed.
fn committed_gaps(h: &Harness) -> Vec<GapRow> {
    gap_rows(h.store.as_ref().unwrap().reader().unwrap(), h.host)
}

/// With 5-minute commits (D-070), a pause or resume left in the open batch would show a
/// reader no pause, then an open pause drawn over the samples after it.
#[test]
fn pause_and_resume_reach_readers_without_a_flush() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("pause-commit", collectors, &settings_all_on());
    h.ticks(2);
    h.ctl.set_paused(true);
    h.pump();
    let paused_at = h.clock.now().wall_ms;
    assert_eq!(
        committed_gaps(&h),
        [(GapReason::Paused, None, paused_at, None)]
    );
    h.clock.advance(Duration::from_secs(30));
    h.ctl.set_paused(false);
    h.pump();
    assert_eq!(
        committed_gaps(&h),
        [(GapReason::Paused, None, paused_at, Some(paused_at + 30_000))]
    );

    // Paused while asleep: the wake closes the sleep gap and opens the pause, both
    // committed with the wake.
    h.ticks(2);
    let slept_at = h.clock.now();
    let _ack = h.power.will_sleep(slept_at);
    h.pump();
    h.ctl.set_paused(true);
    h.pump();
    h.clock.advance(Duration::from_secs(60));
    h.power.did_wake(h.clock.now());
    h.pump();
    let woke_at = slept_at.wall_ms + 60_000;
    assert_eq!(
        committed_gaps(&h),
        [
            (GapReason::Paused, None, paused_at, Some(paused_at + 30_000)),
            (GapReason::Sleep, None, slept_at.wall_ms, Some(woke_at)),
            (GapReason::Paused, None, woke_at, None),
        ]
    );
}

/// The same for a module switched off and on again.
#[test]
fn module_switches_reach_readers_without_a_flush() {
    let (cpu, _) = fake("cpu", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let (pwr, _) = fake(
        "pwr",
        Cadence::EveryTick,
        &[Module::Power],
        &["power.system"],
    );
    let mut h = Harness::new("disable-commit", vec![cpu, pwr], &settings_all_on());
    h.ticks(2);
    let mut off = settings_all_on();
    off.modules.get_mut(&Module::Power).unwrap().enabled = false;
    h.ctl.apply_settings(&off);
    h.pump();
    let at = h.clock.now().wall_ms;
    assert_eq!(
        committed_gaps(&h),
        [
            (GapReason::ModuleDisabled, Some(Module::Power), at, None),
            (GapReason::ModuleDisabled, Some(Module::Sensors), at, None),
        ]
    );
    h.ticks(5);
    let back_at = h.clock.now().wall_ms;
    h.ctl.apply_settings(&settings_all_on());
    h.pump();
    assert_eq!(
        committed_gaps(&h),
        [
            (
                GapReason::ModuleDisabled,
                Some(Module::Power),
                at,
                Some(back_at)
            ),
            (
                GapReason::ModuleDisabled,
                Some(Module::Sensors),
                at,
                Some(back_at)
            ),
        ]
    );
}

#[test]
fn disabling_power_gaps_power_and_sensors_and_stops_their_collectors() {
    let (cpu, _) = fake("cpu", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let (pwr, ph) = fake(
        "pwr",
        Cadence::EveryTick,
        &[Module::Power],
        &["power.system"],
    );
    let (sens, sh) = fake(
        "sens",
        Cadence::Every(2_000),
        &[Module::Sensors],
        &["thermal.cpu"],
    );
    let mut h = Harness::new("disable", vec![cpu, pwr, sens], &settings_all_on());
    h.ticks(2);
    let mut off = settings_all_on();
    off.modules.get_mut(&Module::Power).unwrap().enabled = false;
    h.ctl.apply_settings(&off);
    h.pump();
    let at = h.clock.now().wall_ms;
    let (ps, ss) = (ph.samples(), sh.samples());
    h.ticks(4);
    assert_eq!(ph.samples(), ps, "power collector not sampled");
    assert_eq!(sh.samples(), ss, "sensors share the Power switch");
    let f = h.frames().pop().unwrap();
    let keys: Vec<String> = f.layout.series.iter().map(|k| k.to_string()).collect();
    assert_eq!(keys, vec!["cpu.total"]);

    let g = gaps(&h);
    let mut mods: Vec<_> = g
        .iter()
        .map(|g| (g.reason, g.module, g.start_ms, g.end_ms))
        .collect();
    mods.sort_by_key(|m| m.1);
    assert_eq!(
        mods,
        vec![
            (GapReason::ModuleDisabled, Some(Module::Power), at, None),
            (GapReason::ModuleDisabled, Some(Module::Sensors), at, None),
        ]
    );

    h.ticks(5);
    let back_at = h.clock.now().wall_ms;
    h.ctl.apply_settings(&settings_all_on());
    h.pump();
    h.ticks(2);
    assert!(ph.samples() > ps, "re-enabled");
    let g = gaps(&h);
    assert!(
        g.iter().all(|g| g.end_ms == Some(back_at)),
        "{g:?} {back_at}"
    );
    let f = h.frames().pop().unwrap();
    assert_eq!(f.layout.series.len(), 3);
}

#[test]
fn status_carries_the_power_source_and_primary_interface() {
    let (net, nh) = fake(
        "network",
        Cadence::EveryTick,
        &[Module::Network],
        &["net.rx{iface=en0}", "net.rx_total"],
    );
    let mut h = Harness::new("status-facts", vec![net], &settings_all_on());
    h.ticks(1);
    let st = h.ctl.status();
    assert_eq!(st.power_source, PowerSource::Adapter);
    assert_eq!(st.primary_iface, None);

    nh.set_primary_iface(Some("en0"));
    h.power.set(|p| p.charging = true);
    h.ticks(1);
    let st = h.ctl.status();
    assert_eq!(st.power_source, PowerSource::Charging);
    assert_eq!(st.primary_iface.as_deref(), Some("en0"));
    let published = h
        .drain()
        .into_iter()
        .filter_map(|m| match m {
            BusMsg::Status(s) => s.primary_iface,
            _ => None,
        })
        .count();
    assert!(published > 0, "the change is published, not only stored");

    // A tick without a reading keeps the last one; a VPN route clears it.
    h.ticks(1);
    assert_eq!(h.ctl.status().primary_iface.as_deref(), Some("en0"));
    nh.set_primary_iface(None);
    h.power.set(|p| p.on_battery = true);
    h.ticks(1);
    let st = h.ctl.status();
    assert_eq!(st.primary_iface, None);
    assert_eq!(st.power_source, PowerSource::Battery);

    // Switching the network module off clears it: nothing keeps it current.
    nh.set_primary_iface(Some("en0"));
    h.ticks(1);
    assert_eq!(h.ctl.status().primary_iface.as_deref(), Some("en0"));
    h.drain();
    let mut off = settings_all_on();
    off.modules.get_mut(&Module::Network).unwrap().enabled = false;
    h.ctl.apply_settings(&off);
    h.pump();
    assert_eq!(h.ctl.status().primary_iface, None);
    assert!(
        h.drain()
            .iter()
            .any(|m| matches!(m, BusMsg::Status(s) if s.primary_iface.is_none())),
        "the cleared interface is published"
    );
}

#[test]
fn backs_off_on_battery_and_low_power_mode() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("backoff", collectors, &settings_all_on());
    h.visible();
    h.ticks(1);
    assert_eq!(h.clock.period(), Some(Duration::from_secs(1)));
    h.power.set(|p| p.on_battery = true);
    h.ticks(1);
    assert_eq!(h.clock.period(), Some(Duration::from_secs(2)));
    let st = h.ctl.status();
    assert!(st.backed_off && st.on_battery);
    assert_eq!(st.interval_ms, 2_000);

    // Battery slowdown off: back to the user's interval; Low Power Mode still backs off.
    let mut s = settings_all_on();
    s.sampling.slow_on_battery = false;
    h.ctl.apply_settings(&s);
    h.pump();
    assert_eq!(h.clock.period(), Some(Duration::from_secs(1)));
    h.power.set(|p| p.low_power_mode = true);
    h.ticks(1);
    assert_eq!(h.clock.period(), Some(Duration::from_secs(2)));

    // The back-off doubles the user's interval, up to the longest one offered.
    for (user, backed_off) in [(5_000, 10_000), (30_000, 60_000), (60_000, 60_000)] {
        s.sampling.interval_ms = user;
        h.ctl.apply_settings(&s);
        h.pump();
        assert_eq!(h.clock.period(), Some(Duration::from_millis(backed_off)));
        assert_eq!(u64::from(h.ctl.status().interval_ms), backed_off);
    }

    // Display sleep is a flag, not a back-off.
    h.power.set(|p| {
        p.low_power_mode = false;
        p.display_asleep = true;
    });
    h.ticks(1);
    let st = h.ctl.status();
    assert!(st.display_idle && !st.backed_off);
}

/// The tick the engine runs at is `effective_interval_ms` for every setting and power
/// state, the function the generated sampling plans are built from (D-092).
#[test]
fn engine_ticks_at_effective_interval_ms() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("plans", collectors, &settings_all_on());
    h.visible();
    h.ticks(1);
    for interval in kelvo_schema::settings::SamplingSettings::INTERVALS_MS {
        for bits in 0..16u8 {
            let [slow, perf, battery, lpm] = [0, 1, 2, 3].map(|b| bits & (1 << b) != 0);
            let mut s = settings_all_on();
            s.sampling.interval_ms = interval;
            s.sampling.slow_on_battery = slow;
            s.sampling.performance_mode = perf;
            h.ctl.apply_settings(&s);
            h.power.set(|p| {
                p.on_battery = battery;
                p.low_power_mode = lpm;
            });
            h.ticks(1);
            let power = kelvo_engine::PowerState {
                on_battery: battery,
                low_power_mode: lpm,
                ..Default::default()
            };
            let want = kelvo_engine::effective_interval_ms(interval, slow, perf, power);
            assert_eq!(
                h.ctl.status().interval_ms,
                want,
                "{interval} slow {slow} perf {perf} battery {battery} lpm {lpm}"
            );
            assert_eq!(
                h.clock.period(),
                Some(Duration::from_millis(u64::from(want)))
            );
        }
    }
}

#[test]
fn lagging_subscriber_drops_frames_and_never_blocks_the_engine() {
    let (collectors, _, _) = cpu_and_loadavg();
    let bus = Bus::new(8);
    let mut slow = bus.subscribe();
    let mut h = Harness::with_bus("lag", collectors, &settings_all_on(), bus);
    h.ticks(100);
    // The engine kept going.
    let latest = h.live.latest_frame().unwrap();
    assert_eq!(value(&latest, "cpu.total", false), 99.0);
    let mut got = Vec::new();
    while let Some(m) = slow.try_recv() {
        got.push(m);
    }
    assert_eq!(got.len(), 8, "keeps the newest messages");
    assert!(slow.dropped() > 0);
    // A late frame is self-describing even though its Layout message was dropped.
    let BusMsg::Frame(f) = got.last().unwrap() else {
        panic!()
    };
    assert_eq!(value(f, "cpu.total", false), 99.0);
}

#[test]
fn process_interest_switches_adaptive_cadence() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    ph.0.lock().unwrap().processes = 40;
    let mut h = Harness::new("interest", vec![procs], &settings_all_on());
    h.visible();
    h.ticks(20);
    assert_eq!(ph.samples(), 2, "every 10 ticks when nobody looks");
    h.ctl.set_process_interest(Some(0));
    assert_eq!(h.ctl.process_interest(), Some(0));
    h.drain();
    h.ticks(5);
    assert_eq!(
        ph.samples(),
        7,
        "every tick while a window wants every sample"
    );
    let batches = h
        .drain()
        .into_iter()
        .filter(|m| matches!(m, BusMsg::Processes(_)))
        .count();
    assert_eq!(batches, 5);
    // The Overview asks for a batch every 3 s: the collector runs at that period.
    h.ctl.set_process_interest(Some(3_000));
    h.ticks(9);
    assert_eq!(ph.samples(), 10, "every third tick");
    h.ctl.set_process_interest(None);
    assert_eq!(h.ctl.process_interest(), None);

    // Store snapshots stay at one per 10 s with the top 30.
    let mut r = h.reader();
    let at = r.processes_at(h.host, T0 + 21_000).unwrap().unwrap();
    assert_eq!(at.rows.len(), 30);
    assert_eq!(at.rows[0].name, "proc39", "sorted by CPU");
}

/// Performance mode (D-088) with a window showing detail: processes run every 30 s
/// while no window wants them, temperatures keep their 5 s period (the window shows
/// them), a window gets process rows at most every 2 s, the battery back-off applies
/// even with the setting off, and Low Power Mode turns it all on without the setting,
/// and the status says why. With nothing on screen the background slows the same
/// cadences without the mode (D-094).
#[test]
fn performance_mode_slows_cadences_and_follows_low_power_mode() {
    use kelvo_schema::PerformanceReason;
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    let (temps, th) = fake(
        "hid.thermal",
        Cadence::Every(5_000),
        &[Module::Sensors],
        &["thermal.cpu"],
    );
    let mut s = settings_all_on();
    s.sampling.slow_on_battery = false;
    // The menu bar shows watts, not a temperature.
    s.modules.get_mut(&Module::Power).unwrap().menu_bar = MenuBarMode::WattsValue;
    let mut h = Harness::new("performance", vec![procs, temps], &s);
    h.visible();
    h.ticks(60);
    assert_eq!(
        (ph.samples(), th.samples()),
        (6, 12),
        "10 s and 5 s when off"
    );
    assert_eq!(h.ctl.status().performance, PerformanceReason::Off);

    s.sampling.performance_mode = true;
    h.ctl.apply_settings(&s);
    h.pump();
    assert_eq!(h.ctl.status().performance, PerformanceReason::Setting);
    let (p0, t0) = (ph.samples(), th.samples());
    h.ticks(60);
    assert_eq!(ph.samples() - p0, 2, "every 30 s");
    assert_eq!(
        th.samples() - t0,
        12,
        "a window shows temperatures: every 5 s"
    );

    // A window asking for every tick gets every other one.
    h.ctl.set_process_interest(Some(0));
    let p1 = ph.samples();
    h.ticks(10);
    assert_eq!(ph.samples() - p1, 5, "every 2 s");
    h.ctl.set_process_interest(None);

    // The battery back-off applies although `slow_on_battery` is off.
    h.power.set(|p| p.on_battery = true);
    h.ticks(1);
    assert_eq!(h.clock.period(), Some(Duration::from_secs(2)));

    // Low Power Mode alone: on, and the status says why.
    s.sampling.performance_mode = false;
    h.ctl.apply_settings(&s);
    h.pump();
    assert_eq!(h.clock.period(), Some(Duration::from_secs(1)));
    assert_eq!(h.ctl.status().performance, PerformanceReason::Off);
    h.power.set(|p| p.low_power_mode = true);
    h.ticks(1);
    assert_eq!(h.ctl.status().performance, PerformanceReason::LowPowerMode);
    h.power.set(|p| p.low_power_mode = false);
    h.ticks(1);
    assert_eq!(h.ctl.status().performance, PerformanceReason::Off);
}

/// The background (D-094): with no window showing detail the base tick is 2 s,
/// processes run every 30 s and the temperature collectors every 10 s, a temperature in
/// the menu bar included, without Performance mode. The battery back-off does not stack
/// on it, the status says backgrounded and not backed off, and a window gets the 1 s
/// tick and the normal periods as soon as its detail interest is acknowledged.
#[test]
fn the_background_slows_the_tick_processes_and_temperatures() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    let (temps, th) = fake(
        "hid.thermal",
        Cadence::Every(5_000),
        &[Module::Sensors],
        &["thermal.cpu"],
    );
    let mut s = settings_all_on();
    s.modules.get_mut(&Module::Power).unwrap().menu_bar = MenuBarMode::TempInCombined;
    let mut h = Harness::new("background", vec![procs, temps], &s);
    assert_eq!(h.clock.period(), Some(Duration::from_secs(2)));
    let st = h.ctl.status();
    assert_eq!(st.interval_ms, 2_000);
    assert!(st.backgrounded && !st.backed_off, "{st:?}");
    h.ticks(30);
    assert_eq!(
        (ph.samples(), th.samples()),
        (2, 6),
        "60 s: processes every 30 s, temperatures every 10 s"
    );

    // On battery the back-off's 2 s is the background tick: they do not stack.
    h.power.set(|p| p.on_battery = true);
    h.ticks(1);
    assert_eq!(h.clock.period(), Some(Duration::from_secs(2)));
    assert!(h.ctl.status().backed_off);
    h.power.set(|p| p.on_battery = false);
    h.ticks(1);
    assert!(!h.ctl.status().backed_off);

    // A window opens: the status has the 1 s tick before the ack.
    let ack = h.ctl.set_detail_interest(true).expect("crossed from zero");
    h.pump();
    assert!(ack.try_recv().is_ok(), "acknowledged once applied");
    let st = h.ctl.status();
    assert_eq!(st.interval_ms, 1_000);
    assert!(!st.backgrounded, "{st:?}");
    assert_eq!(h.clock.period(), Some(Duration::from_secs(1)));
    assert!(
        h.ctl.set_detail_interest(true).is_none(),
        "a second window does not cross"
    );
    let (p0, t0) = (ph.samples(), th.samples());
    h.ticks(20);
    assert_eq!(
        (ph.samples() - p0, th.samples() - t0),
        (2, 4),
        "processes every 10 s, temperatures every 5 s"
    );

    // Both windows close: back to the background.
    assert!(h.ctl.set_detail_interest(false).is_none());
    assert!(
        h.ctl.set_detail_interest(false).is_some(),
        "crossed to zero"
    );
    h.pump();
    assert!(h.ctl.status().backgrounded);
    assert_eq!(h.clock.period(), Some(Duration::from_secs(2)));
}

/// A window opening while paused re-chooses the tick without starting the ticker;
/// resuming starts it at the visible tick.
#[test]
fn a_window_opening_while_paused_resumes_on_the_visible_tick() {
    let (c, _) = fake("c", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let mut h = Harness::new("detail-paused", vec![c], &settings_all_on());
    h.ticks(2);
    h.ctl.set_paused(true);
    h.pump();
    assert_eq!(h.clock.period(), None, "paused");
    let ack = h.ctl.set_detail_interest(true).expect("crossed from zero");
    h.pump();
    assert!(ack.try_recv().is_ok());
    assert_eq!(h.ctl.status().interval_ms, 1_000);
    assert_eq!(h.clock.period(), None, "still paused");
    h.ctl.set_paused(false);
    h.pump();
    assert_eq!(h.clock.period(), Some(Duration::from_secs(1)));
}

/// At an interval the background does not slow, a window opening changes only
/// `backgrounded`; the status still goes out so the tray paces for a window.
#[test]
fn a_window_opening_publishes_the_status_when_the_tick_stays() {
    let (c, _) = fake("c", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let mut s = settings_all_on();
    s.sampling.interval_ms = 5_000;
    let mut h = Harness::new("detail-same-tick", vec![c], &s);
    h.ticks(2);
    h.drain();
    let _ack = h.ctl.set_detail_interest(true).expect("crossed from zero");
    h.pump();
    let statuses: Vec<_> = h
        .drain()
        .into_iter()
        .filter_map(|m| match m {
            BusMsg::Status(st) => Some((st.interval_ms, st.backgrounded)),
            _ => None,
        })
        .collect();
    assert_eq!(statuses.last(), Some(&(5_000, false)), "{statuses:?}");
    assert_eq!(h.clock.period(), Some(Duration::from_secs(5)));
}

/// A window that opens more than a second after the last background tick, with the
/// next 1 s boundary more than half a second away, gets a sample at once. Right after
/// a tick, close to the next boundary, or with a tick already queued, it waits.
#[test]
fn a_window_opening_on_a_stale_frame_samples_at_once() {
    let (cpu, ch) = fake("cpu", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let mut h = Harness::new("open-sample", vec![cpu], &settings_all_on());
    let open = |h: &mut Harness| {
        let before = ch.samples();
        drop(h.ctl.set_detail_interest(true));
        h.pump();
        let sampled = ch.samples() - before;
        drop(h.ctl.set_detail_interest(false));
        h.pump();
        sampled
    };
    h.ticks(2);
    assert_eq!(open(&mut h), 0, "the last sample is fresh");
    h.clock.advance(Duration::from_millis(1_200));
    assert_eq!(open(&mut h), 1, "1.2 s old, the next boundary 0.8 s away");
    h.ticks(1);
    h.clock.advance(Duration::from_millis(1_700));
    assert_eq!(open(&mut h), 0, "the next boundary is 0.3 s away");
    h.ticks(1);
    h.clock.advance(Duration::from_millis(1_200));
    // The ticker's tick is queued behind the command: it is the fresh sample.
    drop(h.ctl.set_detail_interest(true));
    let before = ch.samples();
    assert!(h.clock.tick());
    h.pump();
    assert_eq!(ch.samples() - before, 1, "one sample, the queued tick");
    let frames = h.frames();
    assert!(
        frames.windows(2).all(|w| w[0].ts_ms < w[1].ts_ms),
        "no frame out of order"
    );
}

/// History holds (D-092) cover the slowest base tick the series was sampled at: the
/// background's 2 s, or the battery back-off when that is slower (D-094).
#[test]
fn history_holds_cover_the_background_tick() {
    let (collectors, _) = three_periods();
    let mut h = Harness::new("history-holds-bg", collectors, &at_interval(500));
    h.ticks(2);
    let now = h.clock.now().wall_ms;
    let hold = |h: &Harness, bucket: i64| {
        h.live
            .history_hold_ms(&key("cpu.total"), bucket, now - 3_600_000)
    };
    assert_eq!(hold(&h, 1_000), 5_000, "0.5 s, but 2 s in the background");
    h.ctl.apply_settings(&at_interval(5_000));
    h.pump();
    h.ticks(2);
    let now = h.clock.now().wall_ms;
    let hold = |bucket: i64| {
        h.live
            .history_hold_ms(&key("cpu.total"), bucket, now - 1_000)
    };
    assert_eq!(hold(1_000), 25_000, "5 s backed off to 10 s");
}

/// Network rates per process (D-081): the on-demand collector runs only on process
/// ticks while a view asks for network, its rates join the rows by pid (a pid without
/// traffic gets 0), a baseline sample leaves the rows without rates, and it is released
/// as soon as nobody asks.
#[test]
fn network_rates_join_process_rows_while_a_view_asks_for_them() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    ph.0.lock().unwrap().processes = 3;
    let (net, nh) = fake_net();
    let mut h = Harness::new("net-procs", vec![procs, net], &settings_all_on());
    h.visible();
    // D-082's on-demand path, which is what remains with network history off.
    h.ctl.set_network_history(false);
    assert!(h.ctl.capabilities().process_network);
    // Per batch: (pid, rx, tx) of each row.
    type NetRows = Vec<Vec<(i32, Option<f32>, Option<f32>)>>;
    let net_of = |h: &mut Harness| -> NetRows {
        h.drain()
            .into_iter()
            .filter_map(|m| match m {
                BusMsg::Processes(b) => Some(
                    b.rows
                        .iter()
                        .map(|p| (p.pid, p.net_rx_bps, p.net_tx_bps))
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    };
    let samples = || nh.lock().unwrap().samples;
    let releases = || nh.lock().unwrap().releases;

    h.ticks(20);
    h.ctl.set_network_process_interest(true);
    h.ticks(5);
    assert_eq!(samples(), 0, "network without process interest is nothing");

    h.drain();
    h.ctl.set_process_interest(Some(3_000));
    h.ticks(9);
    assert_eq!(samples(), 3, "on the process ticks only");
    let batches = net_of(&mut h);
    assert_eq!(batches.len(), 3);
    assert!(
        batches[0].iter().all(|r| r.1.is_none() && r.2.is_none()),
        "the baseline has no rates: {:?}",
        batches[0]
    );
    assert_eq!(
        batches[1],
        vec![
            (100, Some(1000.0), Some(100.0)),
            (101, Some(2000.0), Some(200.0)),
            (102, Some(0.0), Some(0.0)),
        ]
    );

    h.ctl.set_network_process_interest(false);
    h.ticks(6);
    assert_eq!((samples(), releases()), (3, 1), "released once, then idle");
    assert!(net_of(&mut h).iter().flatten().all(|r| r.1.is_none()));

    h.ctl.set_network_process_interest(true);
    h.ticks(3);
    assert_eq!(samples(), 4);
    assert!(
        net_of(&mut h).iter().flatten().all(|r| r.1.is_none()),
        "a new baseline after the release"
    );
    h.ctl.set_process_interest(None);
    h.ticks(1);
    assert_eq!(releases(), 2, "process interest gone: released");
}

/// Pause and sleep release the on-demand collectors (D-082) even though the window that
/// wants them stays open: no tick comes while paused or asleep, so the tick loop's own
/// release never runs. After resuming the first sample is a fresh baseline.
#[test]
fn pause_and_sleep_release_on_demand_collectors() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    ph.0.lock().unwrap().processes = 3;
    let (net, nh) = fake_net();
    let (gpu, gh) = fake_gpu();
    let mut h = Harness::new("on-demand-pause", vec![procs, net, gpu], &settings_all_on());
    let state = |s: &Arc<std::sync::Mutex<FakeNetState>>| {
        let s = s.lock().unwrap();
        (s.samples, s.releases, s.open)
    };
    h.ctl.set_process_interest(Some(0));
    h.ctl.set_network_process_interest(true);
    h.ctl.set_gpu_process_interest(true);
    h.ticks(3);
    assert_eq!(state(&nh), (3, 0, true));
    assert_eq!(state(&gh), (3, 0, true));

    h.ctl.set_paused(true);
    h.pump();
    assert_eq!(state(&nh), (3, 1, false), "paused: released");
    assert_eq!(state(&gh), (3, 1, false), "paused: released");

    h.ctl.set_paused(false);
    h.pump();
    h.ticks(2);
    assert_eq!(state(&nh), (5, 1, true), "sampling again, from a baseline");

    let ack = h.power.will_sleep(h.clock.now());
    h.pump();
    assert!(ack.recv_timeout(Duration::from_secs(1)).is_ok());
    assert_eq!(state(&nh), (5, 2, false), "asleep: released");
    assert_eq!(state(&gh), (5, 2, false), "asleep: released");

    h.clock.advance(Duration::from_secs(60));
    h.power.did_wake(h.clock.now());
    h.pump();
    h.ticks(1);
    assert_eq!(state(&nh), (6, 2, true));
}

#[test]
fn process_network_capability_follows_the_collectors_probe() {
    let (iface, _) = fake(
        "network",
        kelvo_collect::LIVE_OR_IDLE,
        &[Module::Network],
        &["net.rx{iface=en0}"],
    );
    let (net, nh) = fake_net();
    let mut h = Harness::new("net-caps", vec![iface, net], &settings_all_on());
    let before = h.ctl.capabilities();
    assert!(before.process_network);
    // The framework goes away on a re-probe; the interface series stay, so only the
    // process capability changes, and that alone is a new revision.
    nh.lock().unwrap().probe = Some(unsupported());
    h.ctl.reprobe(&[Module::Network]);
    h.ticks(1);
    let after = h.ctl.capabilities();
    assert!(!after.process_network);
    assert_eq!(after.modules, before.modules);
    assert!(after.revision > before.revision);
}

/// Outside Performance mode the GPU collector is held with nothing asking, and GPU time
/// joins the process samples for per-app GPU over a range (D-099).
#[test]
fn gpu_shares_join_every_process_sample_outside_performance_mode() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    ph.0.lock().unwrap().processes = 3;
    let (gpu, gh) = fake_gpu();
    let mut h = Harness::new("gpu-always", vec![procs, gpu], &settings_all_on());
    h.ctl.set_network_history(false);
    // Tray-only, no view asks for rows or GPU: the processes collector runs at its
    // background idle period, and GPU time joins each of its samples for per-app GPU
    // over a range (D-099).
    h.ticks(120);
    let gpu_of: Vec<Vec<Option<f32>>> = h
        .drain()
        .into_iter()
        .filter_map(|m| match m {
            BusMsg::Processes(b) => Some(b.rows.iter().map(|p| p.gpu_pct).collect()),
            _ => None,
        })
        .collect();
    let (samples, releases) = {
        let g = gh.lock().unwrap();
        (g.samples, g.releases)
    };
    assert_eq!(samples, ph.samples(), "on every process sample");
    assert!(samples >= 3);
    assert_eq!(releases, 0, "held between samples");
    assert!(gpu_of.iter().skip(1).all(|b| b.iter().all(Option::is_some)));
}

/// With 1 s process rows (the Processes page) GPU still joins at most once per 10 s usage
/// bucket, and each GPU batch says the wall time its shares are of (D-099).
#[test]
fn gpu_joins_fast_process_rows_once_per_usage_bucket() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    ph.0.lock().unwrap().processes = 3;
    let (gpu, gh) = fake_gpu();
    let mut h = Harness::new("gpu-paced", vec![procs, gpu], &settings_all_on());
    h.visible();
    h.ctl.set_network_history(false);
    h.ctl.set_process_interest(Some(1_000));
    h.ticks(60);
    let spans: Vec<Option<i64>> = h
        .drain()
        .into_iter()
        .filter_map(|m| match m {
            BusMsg::Processes(b) => Some(b.gpu_span_ms),
            _ => None,
        })
        .collect();
    let (samples, releases) = {
        let g = gh.lock().unwrap();
        (g.samples, g.releases)
    };
    assert!(ph.samples() >= 50, "1 s rows: {}", ph.samples());
    assert!((5..=7).contains(&samples), "about once per 10 s: {samples}");
    assert_eq!(releases, 0, "held between samples");
    let measured: Vec<i64> = spans.into_iter().flatten().collect();
    assert!(!measured.is_empty());
    assert!(
        measured.iter().all(|&ms| (9_000..=11_000).contains(&ms)),
        "each GPU batch covers its own ~10 s: {measured:?}"
    );
}

/// In Performance mode GPU time works like network rates and independently of them:
/// sampled on the process ticks only while a view asks, joined by pid (0 for a process
/// without GPU time), no value on the baseline, released when the interest ends.
#[test]
fn in_performance_mode_gpu_shares_join_process_rows_only_while_a_view_asks() {
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[Module::Cpu],
        &["self.cpu"],
    );
    ph.0.lock().unwrap().processes = 3;
    let (net, nh) = fake_net();
    let (gpu, gh) = fake_gpu();
    // Performance mode gives up per-app GPU over a range: GPU is on demand (D-085, D-099).
    let mut s = settings_all_on();
    s.sampling.performance_mode = true;
    let mut h = Harness::new("gpu-procs", vec![procs, net, gpu], &s);
    h.visible();
    h.ctl.set_network_history(false);
    assert!(h.ctl.capabilities().process_gpu);
    // Per batch: (pid, gpu, net rx) of each row.
    type GpuRows = Vec<Vec<(i32, Option<f32>, Option<f32>)>>;
    let rows_of = |h: &mut Harness| -> GpuRows {
        h.drain()
            .into_iter()
            .filter_map(|m| match m {
                BusMsg::Processes(b) => Some(
                    b.rows
                        .iter()
                        .map(|p| (p.pid, p.gpu_pct, p.net_rx_bps))
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    };
    let samples = || gh.lock().unwrap().samples;
    let releases = || gh.lock().unwrap().releases;

    h.ctl.set_gpu_process_interest(true);
    h.ticks(5);
    assert_eq!(samples(), 0, "GPU without process interest is nothing");

    h.drain();
    h.ctl.set_process_interest(Some(2_000));
    h.ticks(6);
    assert_eq!(samples(), 3, "on the process ticks only");
    assert_eq!(nh.lock().unwrap().samples, 0, "network was not asked for");
    let batches = rows_of(&mut h);
    assert_eq!(batches.len(), 3);
    assert!(
        batches[0].iter().all(|r| r.1.is_none()),
        "the baseline has no shares: {:?}",
        batches[0]
    );
    assert_eq!(
        batches[1],
        vec![
            (100, Some(30.0), None),
            (101, Some(0.0), None),
            (102, Some(12.5), None),
        ]
    );

    h.ctl.set_gpu_process_interest(false);
    h.ticks(4);
    assert_eq!((samples(), releases()), (3, 1), "released once, then idle");
    assert!(rows_of(&mut h).iter().flatten().all(|r| r.1.is_none()));

    h.ctl.set_gpu_process_interest(true);
    h.ticks(2);
    assert_eq!(samples(), 4);
    assert!(
        rows_of(&mut h).iter().flatten().all(|r| r.1.is_none()),
        "a new baseline after the release"
    );
    h.ctl.set_process_interest(None);
    h.ticks(1);
    assert_eq!(releases(), 2, "process interest gone: released");
}

#[test]
fn process_gpu_capability_follows_the_collectors_probe() {
    let (util, _) = fake("gpu", Cadence::EveryTick, &[Module::Gpu], &["gpu.util"]);
    let (gpu, gh) = fake_gpu();
    let mut h = Harness::new("gpu-caps", vec![util, gpu], &settings_all_on());
    let before = h.ctl.capabilities();
    assert!(before.process_gpu);
    assert!(!before.process_network);
    gh.lock().unwrap().probe = Some(unsupported());
    h.ctl.reprobe(&[Module::Gpu]);
    h.ticks(1);
    let after = h.ctl.capabilities();
    assert!(!after.process_gpu);
    assert_eq!(after.modules, before.modules);
    assert!(after.revision > before.revision);
}

#[test]
fn capabilities_follow_probes() {
    let (cpu, _) = fake("cpu", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let (smc, _) = fake(
        "smc",
        Cadence::Every(2_000),
        &[Module::Sensors],
        &["thermal.cpu"],
    );
    let (bat, bh) = fake("battery", Cadence::Every(10_000), &[Module::Battery], &[]);
    bh.set_probe(Some(kelvo_collect::Probe::NotPresent));
    let (ior, ih) = fake("ioreport", Cadence::EveryTick, &[Module::Gpu], &[]);
    ih.set_probe(Some(unsupported()));
    let (net, nh) = fake(
        "net",
        Cadence::EveryTick,
        &[Module::Network],
        &["net.rx{iface=en0}"],
    );
    let mut h = Harness::new("caps", vec![cpu, smc, bat, ior, net], &settings_all_on());
    let caps = h.ctl.capabilities();
    assert_eq!(caps.revision, 1);
    assert_eq!(
        caps.module(Module::Cpu),
        &ModuleCap::Available { series: 1 }
    );
    assert_eq!(caps.module(Module::Battery), &ModuleCap::NotPresent);
    assert_eq!(
        caps.module(Module::Gpu),
        &ModuleCap::Unsupported(UnsupportedReason::UnknownChip)
    );
    assert_eq!(
        caps.module(Module::Network),
        &ModuleCap::Available { series: 1 }
    );

    // The interface goes away: re-probe, new revision, network still "available" with
    // zero series, and a new layout without it.
    nh.set_keys(&[]);
    h.ctl.reprobe(&[Module::Network]);
    h.pump();
    h.ticks(1);
    let caps = h.ctl.capabilities();
    assert_eq!(caps.revision, 2);
    assert_eq!(
        caps.module(Module::Network),
        &ModuleCap::Available { series: 0 }
    );
    assert_eq!(h.live.layout().unwrap().layout_no, 2);

    // A hint that changes nothing keeps the revision and the layout.
    h.ctl.reprobe(&[Module::Network]);
    h.pump();
    h.ticks(1);
    assert_eq!(h.ctl.capabilities().revision, 2);
    assert_eq!(h.live.layout().unwrap().layout_no, 2);
}

#[test]
fn shutdown_flushes_and_closes_open_gaps() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut off = settings_all_on();
    off.modules.get_mut(&Module::Disk).unwrap().enabled = false;
    let mut h = Harness::new("shutdown", collectors, &off);
    h.ticks(3);
    h.ctl.set_paused(true);
    h.pump();
    h.clock.advance(Duration::from_secs(4));
    h.ctl.shutdown();
    assert!(!h.engine.pump(), "stopped");
    let end = h.clock.now().wall_ms;
    let g = gaps(&h);
    assert_eq!(g.len(), 2, "{g:?}");
    assert!(g.iter().all(|g| g.end_ms == Some(end)));
    let (rows, _) = s10_rows(&h);
    assert_eq!(rows.len(), 1, "partial bucket flushed");
}

/// The "Network history" setting (D-089) reaches the engine before its first tick, so
/// a user who turned it off never has a NetworkStatistics session opened at launch.
#[test]
fn local_source_starts_with_the_network_history_setting() {
    for on in [true, false] {
        let mut settings = settings_all_on();
        settings.history.network_history = on;
        let (ticker, _clock) = kelvo_engine::FakeTicker::new();
        let (power, _) = kelvo_engine::FakePowerSignals::new();
        let source = Arc::new(kelvo_engine::LocalSource::new(
            host_record(),
            kelvo_engine::EngineParts {
                collectors: Vec::new(),
                ticker: Box::new(ticker),
                power: Box::new(power),
                hints: None,
            },
            settings.clone(),
        ));
        let mut handle = Arc::clone(&source)
            .start(SourceSink {
                live: LiveHub::new(Bus::default()),
                store: None,
            })
            .unwrap();
        assert_eq!(
            source.engine().unwrap().network_history(),
            kelvo_engine::network_history_enabled(&settings)
        );
        assert_eq!(
            kelvo_engine::network_history_enabled(&settings),
            on && !cfg!(feature = "appstore")
        );
        handle.stop();
    }
}

#[test]
fn local_source_runs_the_engine_on_its_own_thread() {
    let dir = TempDir::new("local-source");
    let store =
        kelvo_store::Store::open(kelvo_store::StoreConfig::new(dir.0.join("history.sqlite")))
            .unwrap();
    // The app shell registers the host; the source only writes history (D-064).
    store.writer().upsert_host(host_record()).unwrap();
    let (ticker, clock) = kelvo_engine::FakeTicker::new();
    let (power, _) = kelvo_engine::FakePowerSignals::new();
    let (cpu, _) = fake("cpu", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let source = Arc::new(kelvo_engine::LocalSource::new(
        host_record(),
        kelvo_engine::EngineParts {
            collectors: vec![cpu],
            ticker: Box::new(ticker),
            power: Box::new(power),
            hints: None,
        },
        settings_all_on(),
    ));
    assert_eq!(source.capabilities().revision, 0, "empty before start");
    let bus = Bus::default();
    let mut sub = bus.subscribe();
    let mut handle = Arc::clone(&source)
        .start(SourceSink {
            live: LiveHub::new(bus.clone()),
            store: Some(store.writer()),
        })
        .unwrap();
    assert!(
        Arc::clone(&source)
            .start(SourceSink {
                live: LiveHub::new(bus.clone()),
                store: None
            })
            .is_err(),
        "starts once"
    );
    // Wait for the engine thread to start the ticker, then tick it.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !clock.is_running() {
        assert!(std::time::Instant::now() < deadline, "engine never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    clock.tick();
    let frame = loop {
        match sub.blocking_recv() {
            Some(BusMsg::Frame(f)) => break f,
            Some(_) => {}
            None => panic!("bus closed"),
        }
    };
    assert_eq!(value(&frame, "cpu.total", false), 0.0);
    assert_eq!(source.capabilities().revision, 1);
    assert!(!handle.control().status().paused);
    handle.stop();
    store.writer().flush().unwrap();
    let CursorRead::Page(page) = store
        .reader()
        .unwrap()
        .read_after(host_record().id, Tier::S10, None, 10, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("truncated");
    };
    assert_eq!(page.rows.len(), 1, "the engine wrote through the sink");
}

/// Settings at `interval_ms`, every module on.
fn at_interval(interval_ms: u32) -> kelvo_schema::Settings {
    let mut s = settings_all_on();
    s.sampling.interval_ms = interval_ms;
    s
}

/// A fast collector, a 5 s collector and a 60 s collector (disk capacity).
fn three_periods() -> (Vec<Box<dyn kelvo_collect::Collector>>, [FakeHandle; 3]) {
    let (fast, fh) = fake("fast", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let (slow, sh) = fake(
        "slow",
        Cadence::Every(5_000),
        &[Module::Cpu],
        &["cpu.loadavg{window=1}"],
    );
    let (cap, ch) = fake(
        "capacity",
        Cadence::Every(60_000),
        &[Module::Disk],
        &["disk.free{vol=/}"],
    );
    (vec![fast, slow, cap], [fh, sh, ch])
}

fn m1_rows(h: &Harness) -> Vec<PageRow> {
    let mut r = h.reader();
    let CursorRead::Page(page) = r
        .read_after(h.host, Tier::M1, None, 1_000, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("truncated");
    };
    page.rows
}

#[test]
fn half_second_interval_keeps_wall_clock_periods() {
    let (collectors, [fast, slow, cap]) = three_periods();
    let mut h = Harness::new("interval-500", collectors, &at_interval(500));
    h.visible();
    assert_eq!(h.clock.period(), Some(Duration::from_millis(500)));
    // 130 ticks = 65 s.
    h.ticks(130);
    assert_eq!(fast.samples(), 130, "every tick");
    assert_eq!(slow.samples(), 13, "every 5 s, not every 5 ticks");
    assert_eq!(cap.samples(), 2, "every 60 s, not every 60 ticks");
    let frames = h.frames();
    // The 5 s series is held between samples (2.5 x 5 s), never stale here.
    assert!(
        frames
            .iter()
            .all(|f| !value(f, "cpu.loadavg{window=1}", true).is_nan())
    );
    let (rows, _) = s10_rows(&h);
    assert_eq!(rows.len(), 6, "T0+0.5 s to T0+65 s closes six 10 s buckets");
    assert_eq!(m1_rows(&h).len(), 1);
    assert!(gaps(&h).is_empty());
}

#[test]
fn thirty_second_interval_fills_m1_and_leaves_s10_holes_without_gaps() {
    let (collectors, [fast, slow, cap]) = three_periods();
    let mut h = Harness::new("interval-30s", collectors, &at_interval(30_000));
    assert_eq!(h.clock.period(), Some(Duration::from_secs(30)));
    // 10 ticks: T0 + 30 s .. T0 + 300 s.
    h.ticks(10);
    assert_eq!(fast.samples(), 10);
    assert_eq!(slow.samples(), 10, "a 5 s period at 30 s is every tick");
    assert_eq!(
        cap.samples(),
        5,
        "a 60 s period at 30 s is every other tick"
    );
    let frames = h.frames();
    for f in &frames {
        // Held for 2.5 x max(period, interval): the 60 s series covers its off ticks,
        // the fast one is current for 75 s.
        assert!(!value(f, "disk.free{vol=/}", true).is_nan());
        assert!(!value(f, "cpu.total", true).is_nan());
    }
    // S10: one row per tick (bucket with a sample); the two buckets in between have no
    // sample, which is "no sample", never a gap row.
    let (rows, _) = s10_rows(&h);
    assert_eq!(rows.len(), 9, "the tenth tick's bucket is still open");
    let ts: Vec<i64> = rows.iter().map(|r| r.bucket_ts - T0).collect();
    assert_eq!(ts[..3], [30_000, 60_000, 90_000]);
    assert!(
        gaps(&h).is_empty(),
        "no gap for an interval longer than 10 s"
    );
    // M1: minute 0 has the T0+30 s tick, minutes 1 to 4 two ticks each; minute 5 is open.
    let m1 = m1_rows(&h);
    assert_eq!(m1.len(), 5, "{m1:?}");
    assert_eq!(m1[0].bucket_ts, T0);
}

#[test]
fn sixty_second_interval_fills_m1_once_a_minute() {
    let (collectors, [fast, slow, cap]) = three_periods();
    let mut h = Harness::new("interval-60s", collectors, &at_interval(60_000));
    assert_eq!(h.clock.period(), Some(Duration::from_secs(60)));
    h.ticks(10); // T0 + 1 min .. T0 + 10 min
    assert_eq!(
        (fast.samples(), slow.samples(), cap.samples()),
        (10, 10, 10)
    );
    let m1 = m1_rows(&h);
    assert_eq!(m1.len(), 9, "every closed minute has its one sample");
    assert!(m1.iter().all(|r| !r.stats[0].is_nan()));
    let (s10, _) = s10_rows(&h);
    assert_eq!(
        s10.len(),
        9,
        "one S10 row per minute, five empty buckets between"
    );
    assert!(gaps(&h).is_empty(), "no gap rows, and no stall gap at 60 s");

    // A series that stops arriving still turns into a gap in held: 2.5 x 60 s.
    fast.set_emit(false);
    h.drain();
    h.ticks(3);
    let held: Vec<f32> = h
        .frames()
        .iter()
        .map(|f| value(f, "cpu.total", true))
        .collect();
    assert!(!held[0].is_nan() && !held[1].is_nan(), "{held:?}");
    assert!(held[2].is_nan(), "third missed sample is a gap: {held:?}");
}

#[test]
fn detail_interest_switches_ioreport_between_tray_and_window_rates() {
    let (ior, ih) = fake(
        "ioreport",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Detail,
        },
        &[Module::Cpu],
        &["cpu.cluster.freq{cluster=P0}"],
    );
    let mut h = Harness::new("detail", vec![ior], &settings_all_on());
    // In the background the tick is 2 s (D-094).
    h.ticks(15);
    assert_eq!(ih.samples(), 3, "tray-only: every 10 s");
    // Process interest is a different interest.
    h.ctl.set_process_interest(Some(0));
    h.ticks(5);
    assert_eq!(ih.samples(), 4);
    h.ctl.set_detail_interest(true);
    assert_eq!(h.ctl.detail_interest(), 1);
    h.ticks(5);
    assert_eq!(
        ih.samples(),
        9,
        "a window shows detail: every tick, from the next tick"
    );
    h.ctl.set_detail_interest(false);
    h.ctl.set_detail_interest(false);
    assert_eq!(h.ctl.detail_interest(), 0, "saturates at zero");
    h.ticks(10);
    assert_eq!(ih.samples(), 10, "back to every 10 s");
    // In tray-only mode the held value covers 2.5 x 10 s.
    let held: Vec<f32> = h
        .frames()
        .iter()
        .map(|f| value(f, "cpu.cluster.freq{cluster=P0}", true))
        .collect();
    assert!(held.iter().all(|v| !v.is_nan()), "{held:?}");
}

#[test]
fn live_collectors_run_every_tick_only_while_a_window_or_the_menu_bar_shows_them() {
    // D-067: the network collector, hidden from the menu bar by default.
    let (net, nh) = fake(
        "network",
        kelvo_collect::LIVE_OR_IDLE,
        &[Module::Network],
        &["net.rx{iface=en0}"],
    );
    // The CPU collector, drawn in the menu bar by default.
    let (cpu, ch) = fake(
        "cpu",
        kelvo_collect::LIVE_OR_IDLE,
        &[Module::Cpu],
        &["cpu.total"],
    );
    let mut s = settings_all_on();
    let mut h = Harness::new("live", vec![net, cpu], &s);
    // In the background the tick is 2 s (D-094): 20 s.
    h.ticks(10);
    assert_eq!(nh.samples(), 2, "nothing shows network: every 10 s");
    assert_eq!(ch.samples(), 10, "the menu bar shows CPU: every tick");

    // Process interest alone (a window streaming only the process table) is not Live.
    h.ctl.set_process_interest(Some(0));
    h.ticks(5);
    assert_eq!(nh.samples(), 3);
    h.ctl.set_process_interest(None);

    // A visible window with a stream shows everything.
    h.ctl.set_detail_interest(true);
    h.ticks(5);
    assert_eq!(nh.samples(), 8, "a window is open: every tick");
    h.ctl.set_detail_interest(false);
    h.ticks(10);
    assert_eq!(nh.samples(), 9, "window closed: back to every 10 s");

    // The menu bar gains network and drops CPU.
    s.modules.get_mut(&Module::Network).unwrap().menu_bar = MenuBarMode::ValueLabel;
    s.modules.get_mut(&Module::Cpu).unwrap().menu_bar = MenuBarMode::Hidden;
    h.ctl.apply_settings(&s);
    h.pump();
    let (n0, c0) = (nh.samples(), ch.samples());
    h.ticks(10);
    assert_eq!(
        nh.samples() - n0,
        10,
        "the menu bar shows network: every tick"
    );
    assert_eq!(ch.samples() - c0, 2, "CPU left the menu bar: every 10 s");

    // Between samples the menu bar's value is held, so the idle module still draws.
    let held: Vec<f32> = h
        .frames()
        .iter()
        .map(|f| value(f, "cpu.total", true))
        .collect();
    assert!(held.iter().all(|v| !v.is_nan()), "{held:?}");
}

#[test]
fn a_collector_with_no_series_is_still_sampled() {
    // The processes collector probes Supported with no series: process rows are not
    // series. It used to be left inactive, so no process rows ever reached the bus.
    let (procs, ph) = fake(
        "processes",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        },
        &[],
        &[],
    );
    ph.0.lock().unwrap().processes = 3;
    let (cpu, _) = fake("cpu", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let mut h = Harness::new("no-series", vec![procs, cpu], &settings_all_on());
    h.visible();
    h.ticks(11);
    assert_eq!(ph.samples(), 2);
    let batches = h
        .drain()
        .into_iter()
        .filter(|m| matches!(m, BusMsg::Processes(_)))
        .count();
    assert_eq!(batches, 2);

    // A series-less collector of a switched-off module is not sampled.
    let (gpu, gh) = fake("gpu-clients", Cadence::EveryTick, &[Module::Gpu], &[]);
    let mut off = settings_all_on();
    off.modules.get_mut(&Module::Gpu).unwrap().enabled = false;
    let mut h = Harness::new("no-series-off", vec![gpu], &off);
    h.visible();
    h.ticks(3);
    assert_eq!(gh.samples(), 0);
}

// ---- wall-clock steps (D-064) ------------------------------------------------------------

/// The `cpu.total` stats `(min, max, avg)` of an M1 row.
fn cpu_total(h: &Harness, row: &PageRow) -> [f32; 3] {
    let mut r = h.reader();
    let CursorRead::Page(page) = r
        .read_after(h.host, Tier::M1, None, 1_000, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("truncated");
    };
    let layout = page
        .layouts
        .iter()
        .find(|l| l.layout_no == row.layout_no)
        .unwrap();
    let i = layout
        .series
        .iter()
        .position(|k| k.to_string() == "cpu.total")
        .unwrap();
    [row.stats[i * 3], row.stats[i * 3 + 1], row.stats[i * 3 + 2]]
}

#[test]
fn wall_clock_stepping_back_never_rewrites_an_older_bucket() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-back", collectors, &settings_all_on());
    h.visible();
    h.ticks(90); // n 0..=89 at T0 + 1..90 s: minute T0 closed, T0 + 60 s open
    let before = m1_rows(&h);
    assert_eq!(before.len(), 1);
    let first_minute = cpu_total(&h, &before[0]);
    let layout_no = h.live.layout().unwrap().layout_no;
    h.drain();

    // NTP steps the wall clock back ten minutes.
    h.clock.jump_wall(-600_000);
    h.tick();
    let stepped_at = T0 + 91_000 - 600_000;
    let msgs = h.drain();
    let first_layout = msgs
        .iter()
        .position(|m| matches!(m, BusMsg::Layout(l) if l.layout_no == layout_no + 1));
    let first_frame = msgs
        .iter()
        .position(|m| matches!(m, BusMsg::Frame(f) if f.ts_ms == stepped_at));
    assert!(
        first_layout.is_some() && first_layout < first_frame,
        "a new layout number comes before the stepped frame"
    );
    let timelines: Vec<u32> = msgs
        .iter()
        .filter_map(|m| match m {
            BusMsg::Frame(f) => Some(f.timeline),
            _ => None,
        })
        .collect();
    assert_eq!(timelines, [1], "the stepped frame starts timeline 1");
    let ring = h.live.backfill(i64::MIN);
    assert_eq!(
        ring[0].start_ms, stepped_at,
        "every row at or after the stepped time is gone"
    );

    let g = gaps(&h);
    assert_eq!(g.len(), 1, "{g:?}");
    assert_eq!(g[0].reason, GapReason::ClockChanged);
    assert_eq!(g[0].start_ms, stepped_at);
    assert_eq!(
        g[0].end_ms,
        Some(T0 + 120_000),
        "until the end of the newest bucket already written"
    );

    // Eleven minutes on the new clock: it passes T0 + 120 s and persists again.
    h.ticks(700);
    let rows = m1_rows(&h);
    let minutes: Vec<i64> = rows.iter().map(|r| r.bucket_ts).collect();
    assert_eq!(minutes, [T0, T0 + 60_000, T0 + 120_000]);
    assert_eq!(cpu_total(&h, &rows[0]), first_minute, "minute T0 untouched");
    assert_eq!(
        cpu_total(&h, &rows[1])[1],
        89.0,
        "minute T0 + 60 s holds only what the old clock measured"
    );
    let (s10, _) = s10_rows(&h);
    assert!(
        s10.iter()
            .all(|r| r.bucket_ts >= T0 + 120_000 || (T0..T0 + 100_000).contains(&r.bucket_ts)),
        "no 10 s bucket from the replayed span"
    );
    assert_eq!(gaps(&h).len(), 1, "no stall or sleep gap");
}

/// At 0.5 s two ticks is 1 s, less than an ordinary NTP correction. The threshold has
/// a 2 s floor, so a 1.1 s jump forward is a late tick, not a step.
#[test]
fn a_small_wall_clock_jump_at_a_fast_interval_is_not_a_step() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-slew", collectors, &at_interval(500));
    h.ticks(10);
    h.drain();
    h.clock.jump_wall(1_100);
    h.ticks(4);
    assert!(
        h.frames().iter().all(|f| f.timeline == 0),
        "no new timeline"
    );
    assert!(
        gaps(&h).iter().all(|g| g.reason != GapReason::ClockChanged),
        "no clock_changed gap"
    );
}

#[test]
fn wall_clock_stepping_forward_is_a_gap_not_a_stall() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-forward", collectors, &settings_all_on());
    h.visible();
    h.ticks(30); // last at T0 + 30 s
    let layout_no = h.live.layout().unwrap().layout_no;
    h.clock.jump_wall(600_000);
    h.tick(); // T0 + 631 s
    assert_eq!(h.live.layout().unwrap().layout_no, layout_no + 1);
    h.ticks(60);
    let g = gaps(&h);
    assert_eq!(g.len(), 1, "{g:?}");
    assert_eq!(g[0].reason, GapReason::ClockChanged);
    assert_eq!(g[0].start_ms, T0 + 31_000);
    assert_eq!(g[0].end_ms, Some(T0 + 631_000));
    let minutes: Vec<i64> = m1_rows(&h).iter().map(|r| r.bucket_ts).collect();
    assert_eq!(
        minutes,
        [T0, T0 + 600_000],
        "the skipped minutes have no rows"
    );
}

// ---- degraded store and power-event interleavings ----------------------------------------

#[test]
fn live_only_engine_runs_without_a_store() {
    let (collectors, fast, _) = cpu_and_loadavg();
    let mut h = Harness::live_only("live-only", collectors, &settings_all_on());
    h.ticks(70);
    assert_eq!(h.frames().len(), 70);
    let ack = h.power.will_sleep(h.clock.now());
    h.pump();
    assert!(ack.recv_timeout(Duration::from_secs(1)).is_ok());
    h.clock.advance(Duration::from_secs(60));
    h.power.did_wake(h.clock.now());
    h.pump();
    h.ctl.set_paused(true);
    h.pump();
    h.ctl.set_paused(false);
    h.pump();
    h.ticks(2);
    assert_eq!(fast.samples(), 72);
    assert!(
        !h.live.backfill(i64::MIN).is_empty(),
        "the ring still backfills"
    );
    h.ctl.shutdown();
    assert!(!h.engine.pump());
}

#[test]
fn a_failing_store_never_stops_sampling() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("store-gone", collectors, &settings_all_on());
    h.ticks(5);
    // Every write from here on fails (the writer thread is gone).
    h.store.take().unwrap().close().unwrap();
    h.ticks(70); // closes buckets: queued writes fail
    let ack = h.power.will_sleep(h.clock.now());
    h.pump(); // the flush before sleep fails
    assert!(
        ack.recv_timeout(Duration::from_secs(1)).is_ok(),
        "sleep is still acknowledged"
    );
    h.power.did_wake(h.clock.now());
    h.pump();
    h.drain();
    h.ticks(3);
    assert_eq!(h.frames().len(), 3, "frames still flow");
    h.ctl.shutdown();
    assert!(!h.engine.pump());
}

#[test]
fn a_wake_without_a_sleep_reprobes_and_writes_no_gap() {
    let (collectors, fast, _) = cpu_and_loadavg();
    let mut h = Harness::new("wake-only", collectors, &settings_all_on());
    h.ticks(3);
    let probes = fast.probes();
    let starts = h.clock.starts_stops();
    h.power.did_wake(h.clock.now());
    h.pump();
    assert!(!h.ctl.status().asleep);
    assert_eq!(
        h.clock.starts_stops(),
        starts,
        "the ticker was not restarted"
    );
    h.ticks(1);
    assert_eq!(
        fast.probes(),
        probes + 1,
        "devices may have changed: re-probed"
    );
    assert!(gaps(&h).is_empty());
}

#[test]
fn settings_changed_while_asleep_apply_on_wake() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("settings-asleep", collectors, &settings_all_on());
    h.ticks(3);
    let slept = h.clock.now();
    let _ack = h.power.will_sleep(slept);
    h.pump();
    let mut next = at_interval(2_000);
    next.modules.get_mut(&Module::Disk).unwrap().enabled = false;
    h.ctl.apply_settings(&next);
    h.pump();
    assert!(
        !h.clock.is_running(),
        "a new interval does not wake the ticker"
    );
    h.clock.advance(Duration::from_secs(60));
    h.power.did_wake(h.clock.now());
    h.pump();
    assert_eq!(h.clock.period(), Some(Duration::from_secs(2)));
    assert_eq!(h.ctl.status().interval_ms, 2_000);
    let g = gaps(&h);
    let sleep = g.iter().find(|g| g.reason == GapReason::Sleep).unwrap();
    assert_eq!(sleep.end_ms, Some(slept.wall_ms + 60_000));
    let disk = g
        .iter()
        .find(|g| g.reason == GapReason::ModuleDisabled)
        .unwrap();
    assert_eq!((disk.module, disk.end_ms), (Some(Module::Disk), None));
}

/// Pause, sleep, resume and wake in either order leave one contiguous run of closed gaps
/// and a running ticker, never a gap left open or two gaps over the same span.
#[test]
fn pause_sleep_resume_wake_interleavings() {
    // Pause, sleep, resume while asleep, wake.
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("interleave-a", collectors, &settings_all_on());
    h.ticks(2);
    let p = h.clock.now().wall_ms;
    h.ctl.set_paused(true);
    h.pump();
    h.clock.advance(Duration::from_secs(10));
    let _ack = h.power.will_sleep(h.clock.now());
    h.pump();
    h.clock.advance(Duration::from_secs(10));
    let r = h.clock.now().wall_ms;
    h.ctl.set_paused(false);
    h.pump();
    assert!(!h.clock.is_running(), "still asleep");
    h.clock.advance(Duration::from_secs(10));
    let w = h.clock.now().wall_ms;
    h.power.did_wake(h.clock.now());
    h.pump();
    assert!(h.clock.is_running());
    let spans: Vec<_> = gaps(&h)
        .iter()
        .map(|g| (g.reason, g.start_ms, g.end_ms))
        .collect();
    assert_eq!(
        spans,
        [
            (GapReason::Paused, p, Some(r)),
            (GapReason::Sleep, r, Some(w))
        ]
    );

    // Sleep, pause while asleep, wake, resume.
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("interleave-b", collectors, &settings_all_on());
    h.ticks(2);
    let s = h.clock.now().wall_ms;
    let _ack = h.power.will_sleep(h.clock.now());
    h.pump();
    h.clock.advance(Duration::from_secs(10));
    h.ctl.set_paused(true);
    h.pump();
    h.clock.advance(Duration::from_secs(10));
    let w = h.clock.now().wall_ms;
    h.power.did_wake(h.clock.now());
    h.pump();
    assert!(!h.clock.is_running(), "still paused");
    assert!(h.ctl.status().paused && !h.ctl.status().asleep);
    h.clock.advance(Duration::from_secs(10));
    let r = h.clock.now().wall_ms;
    h.ctl.set_paused(false);
    h.pump();
    assert!(h.clock.is_running());
    let spans: Vec<_> = gaps(&h)
        .iter()
        .map(|g| (g.reason, g.start_ms, g.end_ms))
        .collect();
    assert_eq!(
        spans,
        [
            (GapReason::Sleep, s, Some(w)),
            (GapReason::Paused, w, Some(r))
        ]
    );
}

/// D-064: a prune runs in batches and handles queued work between them, so the flush
/// before sleep (which the system waits on) does not wait for a long prune. Batches of 7
/// make the prune 3,000 batches long, so it cannot end before the flush is queued, and
/// the rows deleted when the flush is answered are a whole number of batches.
#[test]
fn the_flush_before_sleep_does_not_wait_for_a_long_prune() {
    const DAY: i64 = 86_400_000;
    const BATCH: u64 = 7;
    const OLD: i64 = 21_000;
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::with_prune_batch("prune-yield", collectors, &settings_all_on(), BATCH);
    h.ticks(15);
    drop(h.reader()); // commits the layout and the first rows

    // 10 s rows from long ago, written straight into the file.
    let conn = rusqlite::Connection::open(h.dir.0.join("history.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
         INSERT INTO tier_10s (host_id, bucket_ts, layout_id, seq, blob)
         SELECT (SELECT min(id) FROM hosts), ?1 + i * 10000, (SELECT min(id) FROM layouts),
                -i, x'0000803f0000803f0000803f'
         FROM n",
        [T0 - 60 * DAY, OLD],
    )
    .unwrap();
    let old = |c: &rusqlite::Connection| -> i64 {
        c.query_row(
            "SELECT count(*) FROM tier_10s WHERE bucket_ts < ?1",
            [T0 - DAY],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(old(&conn), OLD);

    let writer = h.store.as_ref().unwrap().writer();
    let now = h.clock.now().wall_ms;
    let pruner = std::thread::spawn(move || {
        writer.prune(
            now,
            kelvo_store::Retention {
                s10_ms: DAY,
                m1_ms: 7 * DAY,
                history_ms: 7 * DAY,
                proc_snap_ms: DAY,
                max_bytes: kelvo_store::Retention::DEFAULT_MAX_BYTES,
            },
        )
    });
    // The first batch is gone: the prune is under way.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while old(&conn) == OLD {
        assert!(std::time::Instant::now() < deadline, "prune never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    let ack = h.power.will_sleep(h.clock.now());
    h.pump(); // flushes the store, then acks
    assert!(ack.recv_timeout(Duration::from_secs(1)).is_ok());
    let left = old(&conn);
    assert!(
        left > 0,
        "the flush ran between prune batches, not after the whole prune"
    );
    assert_eq!(
        (OLD - left) % BATCH as i64,
        0,
        "{} rows deleted: whole batches of {BATCH}",
        OLD - left
    );
    let report = pruner.join().unwrap().unwrap();
    assert_eq!(report.s10_rows, OLD as u64);
    assert_eq!(old(&conn), 0);
    let g = gaps(&h);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].reason, GapReason::Sleep, "the sleep gap was committed");
}

/// `reset_history` swaps the store under a running engine: the old file gets its open
/// gaps closed, the new one gets the gaps the current state needs, and sampling never
/// stops.
#[test]
fn swapping_the_store_closes_gaps_in_the_old_and_reopens_them_in_the_new() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut off = settings_all_on();
    off.modules.get_mut(&Module::Disk).unwrap().enabled = false;
    let mut h = Harness::new("swap-store", collectors, &off);
    h.ticks(3);
    h.ctl.set_paused(true);
    h.pump();
    h.clock.advance(Duration::from_secs(5));
    let detached_at = h.clock.now().wall_ms;
    let done = h.ctl.set_store(None);
    h.pump();
    assert!(done.recv_timeout(Duration::from_secs(1)).is_ok());
    let old = gaps(&h);
    assert_eq!(old.len(), 2, "{old:?}");
    assert!(
        old.iter().all(|g| g.end_ms == Some(detached_at)),
        "closed in the old file"
    );

    // A fresh file, as after a reset.
    let dir = TempDir::new("swap-store-new");
    let fresh = open_store(&dir);
    fresh.writer().upsert_host(host_record()).unwrap();
    h.clock.advance(Duration::from_secs(5));
    let attached_at = h.clock.now().wall_ms;
    let done = h.ctl.set_store(Some(fresh.writer()));
    h.pump();
    assert!(done.recv_timeout(Duration::from_secs(1)).is_ok());
    h.ctl.set_paused(false);
    h.pump();
    h.ticks(2);
    h.ctl.shutdown();
    h.pump();
    fresh.writer().flush().unwrap();
    let new: Vec<_> = fresh
        .reader()
        .unwrap()
        .gaps(h.host, 0, i64::MAX)
        .unwrap()
        .into_iter()
        .map(|g| (g.reason, g.module, g.start_ms))
        .collect();
    assert!(
        new.contains(&(GapReason::Paused, None, attached_at)),
        "{new:?}"
    );
    assert!(new.contains(&(GapReason::ModuleDisabled, Some(Module::Disk), attached_at)));
    assert_eq!(gaps(&h).len(), 2, "nothing more went to the old file");
}

// ---- long backward clock steps (D-070) ---------------------------------------------------

/// A clock that ran days ahead and is stepped back holds persistence for at most an hour,
/// not until it catches up with the newest bucket. The rows the wrong clock wrote from
/// the end of the hold on are dropped, so the new timeline never upserts into them.
#[test]
fn a_long_backward_step_holds_history_for_at_most_an_hour() {
    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-back-far", collectors, &settings_all_on());
    h.visible();
    h.ticks(90); // minute T0 closed, T0 + 60 s open, on a clock three days ahead
    assert_eq!(h.ctl.status().history_held_until, None);

    h.clock.jump_wall(-3 * DAY);
    h.tick();
    let stepped_at = T0 + 91_000 - 3 * DAY;
    // An hour after the step, up to the next minute boundary.
    let held_until = T0 - 3 * DAY + HOUR + 120_000;
    assert_eq!(h.ctl.status().history_held_until, Some(held_until));
    let g: Vec<_> = gaps(&h)
        .into_iter()
        .map(|g| (g.reason, g.start_ms, g.end_ms))
        .collect();
    assert_eq!(g, [(GapReason::ClockChanged, stepped_at, Some(held_until))]);
    assert!(m1_rows(&h).is_empty(), "the wrong clock's minutes are gone");

    h.ticks(3_800);
    assert_eq!(h.ctl.status().history_held_until, None, "persisting again");
    let minutes: Vec<i64> = m1_rows(&h).iter().map(|r| r.bucket_ts).collect();
    assert!(minutes.len() >= 2, "{minutes:?}");
    assert!(
        minutes.iter().all(|&m| m >= held_until && m < T0),
        "{minutes:?}"
    );
}

/// The discard deletes the gaps the old clock opened, the open `module_disabled` gaps
/// among them. The engine opens them again, so the switched-off span is still a gap and
/// switching the module back on closes it.
#[test]
fn a_long_backward_step_keeps_a_disabled_module_gapped() {
    const DAY: i64 = 24 * 3_600_000;
    let (cpu, _) = fake("cpu", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let (pwr, _) = fake(
        "pwr",
        Cadence::EveryTick,
        &[Module::Power],
        &["power.system"],
    );
    let mut h = Harness::new("clock-back-disabled", vec![cpu, pwr], &settings_all_on());
    h.visible();
    h.ticks(90);
    let mut off = settings_all_on();
    off.modules.get_mut(&Module::Power).unwrap().enabled = false;
    h.ctl.apply_settings(&off);
    h.pump();

    h.clock.jump_wall(-3 * DAY);
    h.tick();
    let stepped_at = T0 + 91_000 - 3 * DAY;
    let held_until = h.ctl.status().history_held_until.unwrap();
    let disabled = |end| {
        [
            (
                GapReason::ModuleDisabled,
                Some(Module::Power),
                stepped_at,
                end,
            ),
            (
                GapReason::ModuleDisabled,
                Some(Module::Sensors),
                stepped_at,
                end,
            ),
        ]
    };
    let mut want = vec![(GapReason::ClockChanged, None, stepped_at, Some(held_until))];
    want.extend(disabled(None));
    assert_eq!(committed_gaps(&h), want, "reopened at the step");

    h.ticks(10);
    let back_at = h.clock.now().wall_ms;
    h.ctl.apply_settings(&settings_all_on());
    h.pump();
    let mut want = vec![(GapReason::ClockChanged, None, stepped_at, Some(held_until))];
    want.extend(disabled(Some(back_at)));
    assert_eq!(committed_gaps(&h), want);
}

/// A discard the store could not commit keeps the hold past its end: persisting would
/// upsert into the old clock's rows. Once the store commits again, the retry drops them
/// and minutes are written again.
#[test]
fn a_failed_discard_holds_history_until_a_retry_succeeds() {
    const DAY: i64 = 24 * 3_600_000;
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-back-discard-fails", collectors, &settings_all_on());
    h.visible();
    h.ticks(90);
    let old_minutes: Vec<i64> = m1_rows(&h).iter().map(|r| r.bucket_ts).collect();
    assert_eq!(old_minutes, [T0], "the old clock's minute is on disk");

    let writer = h.store.as_ref().unwrap().writer();
    writer.set_fail_commits(true);
    h.clock.jump_wall(-3 * DAY);
    h.tick();
    let held_until = h.ctl.status().history_held_until.unwrap();
    // Past the end of the hold, still failing.
    h.ticks(3_700);
    assert!(h.clock.now().wall_ms > held_until + 60_000);
    assert_eq!(
        h.ctl.status().history_held_until,
        Some(held_until),
        "the hold stays while the discard is pending"
    );

    writer.set_fail_commits(false);
    let minutes: Vec<i64> = m1_rows(&h).iter().map(|r| r.bucket_ts).collect();
    assert_eq!(
        minutes,
        [T0],
        "nothing persisted while the discard was pending"
    );

    h.tick();
    let retry_at = h.clock.now().wall_ms;
    assert_eq!(h.ctl.status().history_held_until, None, "persisting again");
    h.ticks(129);
    let gaps = committed_gaps(&h);
    let minutes: Vec<i64> = m1_rows(&h).iter().map(|r| r.bucket_ts).collect();
    assert!(minutes.len() >= 2, "{minutes:?}");
    assert!(
        minutes.iter().all(|&m| m >= held_until && m < T0),
        "the retry dropped the old clock's minute: {minutes:?}"
    );
    // Nothing was persisted from the step to the retry: the clock gap covers all of it.
    let stepped_at = T0 + 91_000 - 3 * DAY;
    assert!(
        clock_gaps_cover(&gaps, stepped_at, retry_at),
        "{gaps:?}, retry at {retry_at}"
    );
}

/// Whether the `clock_changed` gaps in `gaps`, merged, cover `[from, to]` with no hole.
fn clock_gaps_cover(gaps: &[GapRow], from: i64, to: i64) -> bool {
    let mut spans: Vec<(i64, i64)> = gaps
        .iter()
        .filter(|g| g.0 == GapReason::ClockChanged)
        .filter_map(|g| g.3.map(|end| (g.2, end)))
        .collect();
    spans.sort_unstable();
    let mut reached = from;
    for (start, end) in spans {
        if start <= reached {
            reached = reached.max(end);
        }
    }
    reached >= to
}

/// A forward step after the hold ended, while the first step's discard still fails. The
/// remembered gap widens over both steps, so after the retry nothing between the first
/// step and the retry is left without a gap.
#[test]
fn a_forward_step_during_a_pending_discard_widens_the_clock_gap() {
    const DAY: i64 = 24 * 3_600_000;
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new(
        "clock-forward-discard-pending",
        collectors,
        &settings_all_on(),
    );
    h.visible();
    h.ticks(90);
    h.reader();
    let writer = h.store.as_ref().unwrap().writer();
    writer.set_fail_commits(true);
    h.clock.jump_wall(-3 * DAY);
    h.tick();
    let stepped_at = T0 + 91_000 - 3 * DAY;
    let held_until = h.ctl.status().history_held_until.unwrap();
    h.ticks(4_300);
    assert!(h.clock.now().wall_ms > held_until + 600_000);
    h.clock.jump_wall(300_000);
    h.tick();
    h.ticks(5);

    writer.set_fail_commits(false);
    h.tick();
    let retry_at = h.clock.now().wall_ms;
    assert_eq!(h.ctl.status().history_held_until, None, "persisting again");
    let gaps = committed_gaps(&h);
    assert!(
        clock_gaps_cover(&gaps, stepped_at, retry_at),
        "{gaps:?}, from {stepped_at} to {retry_at}"
    );
}

/// A small backward step after the hold ended, while the first step's discard still
/// fails, needs no hold of its own. The pending discard keeps the hold anyway: persisting
/// before it succeeds would upsert into the old clock's rows.
#[test]
fn a_small_backward_step_keeps_the_hold_of_a_pending_discard() {
    const DAY: i64 = 24 * 3_600_000;
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-back-twice", collectors, &settings_all_on());
    h.ticks(90);
    h.reader();
    let writer = h.store.as_ref().unwrap().writer();
    writer.set_fail_commits(true);
    h.clock.jump_wall(-3 * DAY);
    h.tick();
    let held_until = h.ctl.status().history_held_until.unwrap();
    h.ticks(3_700);
    assert!(h.clock.now().wall_ms > held_until + 60_000);
    h.clock.jump_wall(-30_000);
    h.tick();
    let stepped_to = h.clock.now().wall_ms;
    assert!(stepped_to > held_until, "the second step needs no hold");
    h.ticks(5);
    assert_eq!(
        h.ctl.status().history_held_until,
        Some(stepped_to),
        "the pending discard holds history"
    );

    writer.set_fail_commits(false);
    h.tick();
    assert_eq!(h.ctl.status().history_held_until, None, "persisting again");
    h.ticks(130);
    let minutes: Vec<i64> = m1_rows(&h).iter().map(|r| r.bucket_ts).collect();
    assert!(!minutes.is_empty());
    assert!(
        minutes.iter().all(|&m| m >= held_until && m < T0),
        "the retry dropped the old clock's minute: {minutes:?}"
    );
}

/// A forward step while an older step's discard is pending: the discard's cut falls
/// inside the new step's gap, and the gaps are written again after the discard, from the
/// first step to the second step's end.
#[test]
fn a_step_while_a_discard_is_pending_keeps_its_gap() {
    const HOUR: i64 = 3_600_000;
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-step-discard-pending", collectors, &settings_all_on());
    h.visible();
    h.ticks(90);
    h.reader();
    let writer = h.store.as_ref().unwrap().writer();
    writer.set_fail_commits(true);
    h.clock.jump_wall(-3 * 24 * HOUR);
    h.tick();
    let held_until = h.ctl.status().history_held_until.unwrap();
    h.ticks(10);
    writer.set_fail_commits(false);

    h.clock.jump_wall(2 * HOUR);
    h.tick();
    let ts = h.clock.now().wall_ms;
    assert!(
        ts - 2 * HOUR < held_until && held_until < ts,
        "the cut is inside"
    );
    let stepped_at = T0 + 91_000 - 3 * 24 * HOUR;
    let gaps = committed_gaps(&h);
    assert!(clock_gaps_cover(&gaps, stepped_at, ts), "{gaps:?}");
}

/// The hold protects the file the old clock wrote. A store attached after the step
/// starts persisting at once.
#[test]
fn a_new_store_is_not_held_by_an_earlier_clock_step() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clock-back-swap", collectors, &settings_all_on());
    h.visible();
    h.ticks(90);
    h.clock.jump_wall(-600_000);
    h.tick();
    assert_eq!(
        h.ctl.status().history_held_until,
        Some(T0 + 120_000),
        "a short step holds until the newest bucket written"
    );

    let dir = TempDir::new("clock-back-swap-new");
    let fresh = open_store(&dir);
    fresh.writer().upsert_host(host_record()).unwrap();
    let done = h.ctl.set_store(Some(fresh.writer()));
    h.pump();
    assert!(done.recv_timeout(Duration::from_secs(1)).is_ok());
    assert_eq!(h.ctl.status().history_held_until, None);

    h.ticks(130);
    fresh.writer().flush().unwrap();
    let mut r = fresh.reader().unwrap();
    let CursorRead::Page(page) = r
        .read_after(h.host, Tier::M1, None, 1_000, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("truncated");
    };
    assert!(
        !page.rows.is_empty(),
        "the new file gets minutes before the clock reaches T0 + 120 s"
    );
}

#[test]
fn a_minute_where_a_collector_speeds_up_weighs_samples_by_their_span() {
    // `cpu.total` is a span mean (D-090): every 10 s with nothing shown, every tick
    // once a window shows detail.
    let (ior, _) = fake(
        "ioreport",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Detail,
        },
        &[Module::Cpu],
        &["cpu.total"],
    );
    let mut h = Harness::new("span-weighted", vec![ior], &settings_all_on());
    // In the background tick n lands at T0 + 2(n + 1) s and reads n. Idle reads: n = 0, 5
    // and 10, at T0 + 2, 12 and 22 s.
    h.ticks(12);
    h.visible();
    // Every 1 s tick from T0 + 25 s: n = 12..=46; n = 47 at T0 + 60 s closes the minute.
    h.ticks(36);
    let rows = m1_rows(&h);
    assert_eq!(rows.len(), 1);
    let [min, max, avg] = cpu_total(&h, &rows[0]);
    assert_eq!((min, max), (0.0, 46.0));
    // n = 0 covers its period (10 s), n = 5 and 10 cover 10 s each, n = 12 the 3 s since
    // n = 10, and n = 13..=46 (summing to 1003) one second each. Count-weighted it would
    // be (0 + 5 + 10 + 12 + 1003) / 38 = 27.1.
    let want = (5.0 * 10_000.0 + 10.0 * 10_000.0 + 12.0 * 3_000.0 + 1003.0 * 1_000.0)
        / (30_000.0 + 3_000.0 + 34_000.0);
    assert!((avg - want).abs() < 1e-3, "avg {avg}, want {want}");
}

#[test]
fn a_minute_where_a_collector_slows_down_weighs_samples_by_their_span() {
    let (ior, ih) = fake(
        "ioreport",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Detail,
        },
        &[Module::Cpu],
        &["cpu.total"],
    );
    let mut h = Harness::new("span-weighted-slow", vec![ior], &settings_all_on());
    // Tick n lands at T0 + (n + 1) s and reads n: every tick for n = 0..=29, then idle in
    // the background, on 2 s ticks.
    h.visible();
    let mut sampled = Vec::new();
    for n in 0.. {
        if n == 30 {
            h.ctl.set_detail_interest(false);
        }
        let before = ih.samples();
        h.tick();
        let at = h.clock.now().wall_ms;
        if at >= T0 + 60_000 {
            break;
        }
        if ih.samples() > before {
            sampled.push((f64::from(n), at));
        }
    }
    let idle: Vec<i64> = sampled
        .iter()
        .map(|&(_, at)| at)
        .filter(|&at| at > T0 + 30_000)
        .collect();
    assert!(
        idle.windows(2).all(|w| w[1] - w[0] >= 10_000) && !idle.is_empty(),
        "idle reads are 10 s apart: {idle:?}"
    );
    let rows = m1_rows(&h);
    assert_eq!(rows.len(), 1);
    let [_, _, avg] = cpu_total(&h, &rows[0]);
    // Each value covers the time since the previous read; n = 0 its period, 1 s.
    let (mut sum, mut weight, mut prev) = (0.0, 0.0, T0);
    for &(n, at) in &sampled {
        let span = (at - prev) as f64;
        sum += n * span;
        weight += span;
        prev = at;
    }
    let want = (sum / weight) as f32;
    assert!((avg - want).abs() < 1e-3, "avg {avg}, want {want}");
}

#[test]
fn a_read_without_a_value_moves_the_span_of_the_next_one() {
    // A rate collector that drops one value (a counter reset, an implausible rate): the
    // next value covers one period since that read, not two.
    let (fast, fh) = fake("fast", Cadence::EveryTick, &[Module::Cpu], &["cpu.total"]);
    let mut h = Harness::new("span-after-skip", vec![fast], &settings_all_on());
    h.visible();
    h.ticks(30);
    fh.set_emit(false);
    h.tick();
    fh.set_emit(true);
    // n = 31..=58, and n = 59 at T0 + 60 s closes the minute.
    h.ticks(29);
    let rows = m1_rows(&h);
    assert_eq!(rows.len(), 1);
    let [_, _, avg] = cpu_total(&h, &rows[0]);
    // Every value weighs 1 s: the plain mean of 0..=58 without 30.
    let want = ((0..=58).sum::<i32>() - 30) as f32 / 58.0;
    assert!((avg - want).abs() < 1e-3, "avg {avg}, want {want}");
}

#[test]
fn history_answers_through_now_from_the_rows_the_writer_has_not_committed() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("through-now", collectors, &settings_all_on());
    h.visible();
    // n = 0..=24: buckets T0 (n 0..=8) and T0 + 10 s (n 9..=18) closed, T0 + 20 s open.
    h.ticks(25);
    let query = kelvo_store::HistoryQuery {
        host: h.host,
        selectors: vec![kelvo_schema::SeriesSelector {
            metric: kelvo_schema::MetricId::from_static("cpu.total"),
            labels: kelvo_schema::Labels::new(),
        }],
        from_ms: T0,
        to_ms: T0 + 60_000,
        tier: kelvo_store::TierChoice::Fixed(Tier::S10),
        max_points: 100,
    };
    let avgs = |r: &kelvo_store::HistoryResult| -> Vec<(i64, f32)> {
        r.series
            .iter()
            .flat_map(|s| s.points.iter().map(|p| (p.t, p.avg)))
            .collect()
    };
    let want = vec![(T0, 4.0), (T0 + 10_000, 13.5), (T0 + 20_000, 21.5)];
    // Nothing is committed yet (the test store commits only on flush).
    let mut r = h.store.as_ref().unwrap().reader().unwrap();
    assert!(r.history(&query).unwrap().series.is_empty());
    let recent = || h.live.recent_rows(h.host, T0, T0 + 60_000);
    assert_eq!(avgs(&r.history_after(&query, recent).unwrap()), want);
    // Committed, the stored rows and the engine's copies count once.
    let mut r = h.reader();
    assert_eq!(avgs(&r.history_after(&query, recent).unwrap()), want);
    // Live-only: the engine's rows alone.
    assert_eq!(
        avgs(&kelvo_store::history_recent(&query, &recent()).unwrap()),
        want
    );
}

#[test]
fn cleared_history_does_not_come_back_from_the_engine() {
    let (collectors, _, _) = cpu_and_loadavg();
    let mut h = Harness::new("clear-recent", collectors, &settings_all_on());
    // n = 0..=24: two 10 s buckets closed, one open; nothing committed.
    h.ticks(25);
    let writer = h.store.as_ref().unwrap().writer();
    // As `clear_history` does it.
    h.live.forget_recent_rows();
    writer.clear_host(h.host, h.clock.now().wall_ms).unwrap();
    h.live.forget_recent_rows();
    assert!(h.live.recent_rows(h.host, T0, T0 + 60_000).is_empty());
    // n = 25..=34: the open bucket starts over at n = 25.
    h.ticks(10);
    let query = kelvo_store::HistoryQuery {
        host: h.host,
        selectors: vec![kelvo_schema::SeriesSelector {
            metric: kelvo_schema::MetricId::from_static("cpu.total"),
            labels: kelvo_schema::Labels::new(),
        }],
        from_ms: T0,
        to_ms: T0 + 60_000,
        tier: kelvo_store::TierChoice::Fixed(Tier::S10),
        max_points: 100,
    };
    let recent = || h.live.recent_rows(h.host, T0, T0 + 60_000);
    let mut r = h.reader();
    let result = r.history_after(&query, recent).unwrap();
    let mins: Vec<f32> = result
        .series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.min))
        .collect();
    assert!(!mins.is_empty());
    assert!(
        mins.iter().all(|&m| m >= 25.0),
        "only what came after: {mins:?}"
    );
}

#[test]
fn history_holds_follow_each_series_slowest_period() {
    let (mut collectors, _) = three_periods();
    let (ior, _) = fake(
        "ioreport",
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Detail,
        },
        &[Module::Gpu],
        &["gpu.util"],
    );
    collectors.push(ior);
    let mut h = Harness::new("history-holds", collectors, &settings_all_on());
    h.ticks(2);
    let now = h.clock.now().wall_ms;
    let hold = |k: &str, bucket: i64| h.live.history_hold_ms(&key(k), bucket, now - 3_600_000);
    // Every tick at 1 s, backed off to 2 s at worst: the bucket is longer.
    assert_eq!(hold("cpu.total", 10_000), 10_000);
    assert_eq!(hold("gpu.util", 10_000), 25_000, "10 s with nothing shown");
    assert_eq!(hold("disk.free{vol=/}", 10_000), 150_000, "every 60 s");
    assert_eq!(hold("disk.free{vol=/}", 900_000), 900_000);
}

#[test]
fn a_range_from_before_a_faster_interval_keeps_the_slower_hold() {
    let (collectors, _) = three_periods();
    let mut h = Harness::new("history-holds-interval", collectors, &at_interval(10_000));
    h.ticks(3);
    // 10 s, backed off to 20 s at worst.
    let before = h.clock.now().wall_ms;
    let hold = |h: &Harness, from: i64| h.live.history_hold_ms(&key("cpu.total"), 10_000, from);
    assert_eq!(hold(&h, before - 3_600_000), 50_000);
    h.ctl.apply_settings(&at_interval(1_000));
    h.pump();
    h.ticks(3);
    let after = h.clock.now().wall_ms;
    assert_eq!(
        hold(&h, after - 1_000),
        10_000,
        "sampled at 1 s since the change"
    );
    assert_eq!(
        hold(&h, before - 3_600_000),
        50_000,
        "the samples before the change were up to 20 s apart"
    );
}
