//! Network history in the engine (D-089): the per-process network collector samples on
//! the process ticks with no window open, per-app and interface bytes land in the same
//! 10 s buckets, closed buckets reach the store and the hub's ring, and sleep, clock
//! steps and the setting going off leave honest measured spans.

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::*;
use kelvo_collect::{Cadence, Interest};
use kelvo_engine::{Bus, Engine, EngineParts, FakePowerSignals, FakeTicker, LiveHub, SourceSink};
use kelvo_schema::settings::{BarSettings, ReadoutSettings};
use kelvo_schema::{Module, Settings};
use kelvo_store::{NetBucket, Store};

/// Every module on, the menu bar showing none of them: the network collector runs at its
/// 10 s idle period unless a window shows detail. The tests built on [`harness`] have a
/// window showing detail open, so the tick is 1 s and the process ticks 10 s apart; the
/// background's slower cadence (D-094) has its own test.
fn settings() -> Settings {
    let mut s = settings_all_on();
    s.menu_bar.bars = BarSettings {
        cpu: false,
        gpu: false,
        memory: false,
    };
    s.menu_bar.readouts = ReadoutSettings {
        temperature: false,
        ..ReadoutSettings::default()
    };
    s
}

fn processes() -> (Box<dyn kelvo_collect::Collector>, FakeHandle) {
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
    (procs, ph)
}

/// Processes, the per-process network fake and the interface fake.
fn harness(name: &str) -> (Harness, FakeNetHandle, FakeNetHandle) {
    let (procs, _) = processes();
    let (net, nh) = fake_net();
    let (iface, ih) = fake_iface();
    let mut h = Harness::new(name, vec![procs, net, iface], &settings());
    h.visible();
    (h, nh, ih)
}

type FakeNetHandle = std::sync::Arc<std::sync::Mutex<FakeNetState>>;

fn rx_of(b: &NetBucket, name: Option<&str>) -> u64 {
    b.apps
        .iter()
        .find(|a| a.name.as_deref() == name)
        .map_or(0, |a| a.rx_bytes)
}

/// `(bucket offset from T0 in s, measured ms)` of the hub's buckets.
fn ring_shape(h: &Harness) -> Vec<(i64, u32)> {
    h.live
        .recent_net_buckets(i64::MIN, i64::MAX)
        .iter()
        .map(|(t, b)| ((t - T0) / 1_000, b.measured_ms))
        .collect()
}

#[test]
fn tray_only_samples_on_process_ticks_and_history_off_samples_nothing() {
    let (procs, ph) = processes();
    let (net, nh) = fake_net();
    let mut h = Harness::new("history-cadence", vec![procs, net], &settings());
    // In the background: 2 s ticks, processes every 30 s (D-094). 120 s.
    h.ticks(60);
    let s = nh.lock().unwrap();
    assert_eq!(s.sampled_on, [0, 15, 30, 45], "the process ticks only");
    assert_eq!(ph.samples(), 4);
    assert_eq!(s.releases, 0, "one session across samples");
    drop(s);

    h.ctl.set_network_history(false);
    h.ticks(1);
    assert_eq!(
        nh.lock().unwrap().releases,
        1,
        "the setting off releases it"
    );
    h.ticks(40);
    assert_eq!(
        nh.lock().unwrap().samples,
        4,
        "off: no calls with no view asking"
    );

    // D-082's path is untouched: a view asking for rates gets them on its ticks.
    h.ctl.set_process_interest(Some(0));
    h.ctl.set_network_process_interest(true);
    h.ticks(3);
    assert_eq!(nh.lock().unwrap().samples, 7);
    h.ctl.set_network_process_interest(false);
    h.ctl.set_process_interest(None);
    h.ticks(1);
    assert_eq!(nh.lock().unwrap().releases, 2);

    // Back on: process ticks again (60 s: two), and a view going away no longer
    // releases it.
    h.ctl.set_network_history(true);
    h.ticks(30);
    let s = nh.lock().unwrap();
    assert_eq!(s.samples, 9, "{:?}", s.sampled_on);
    assert_eq!(s.releases, 2);
}

#[test]
fn a_window_showing_rates_keeps_its_faster_cadence() {
    let (h0, nh, _) = harness("history-window");
    let mut h = h0;
    h.ctl.set_process_interest(Some(0));
    h.ctl.set_network_process_interest(true);
    h.ticks(5);
    assert_eq!(nh.lock().unwrap().samples, 5);
    // The view hides: back to the process ticks, without a release.
    h.ctl.set_network_process_interest(false);
    h.ctl.set_process_interest(None);
    h.ticks(20);
    let s = nh.lock().unwrap();
    assert_eq!(s.releases, 0);
    assert_eq!(s.samples, 7, "{:?}", s.sampled_on);
}

#[test]
fn app_and_interface_bytes_share_buckets_in_the_store_and_the_ring() {
    let (mut h, _, _) = harness("history-bytes");
    // The interface every tick (a window shows detail), apps every 10 s: different
    // intervals, the same buckets.
    h.ctl.set_detail_interest(true);
    h.ticks(61);

    let ring = h.live.recent_net_buckets(T0, T0 + 70_000);
    let shape: Vec<_> = ring
        .iter()
        .map(|(t, b)| ((t - T0) / 1_000, b.measured_ms))
        .collect();
    // Measured from the baseline at 1 s; the open bucket at 60 s has 1 s so far.
    assert_eq!(
        shape,
        [
            (0, 9_000),
            (10, 10_000),
            (20, 10_000),
            (30, 10_000),
            (40, 10_000),
            (50, 10_000),
            (60, 1_000)
        ]
    );
    for (t, b) in &ring {
        let secs = u64::from(b.measured_ms) / 1_000;
        assert_eq!(rx_of(b, Some(FAKE_APP)), 1_000 * secs, "bucket {t}");
        assert_eq!(
            rx_of(b, None),
            2_000 * secs,
            "pid 101 has no identity: other apps"
        );
        assert_eq!(b.iface_rx_bytes, IFACE_RX_BPS * secs, "bucket {t}");
        assert_eq!(b.iface_tx_pkts, IFACE_TX_BPS * secs / 100, "bucket {t}");
    }

    let mut r = h.reader();
    let got = r.net_by_app(h.host, T0, T0 + 60_000).unwrap();
    assert_eq!(got.measured_ms, 59_000);
    assert_eq!(got.iface_rx_bytes, IFACE_RX_BPS * 59);
    let alpha = got
        .apps
        .iter()
        .find(|a| a.name.as_deref() == Some(FAKE_APP));
    assert_eq!(
        alpha.map(|a| (a.rx_bytes, a.tx_bytes)),
        Some((59_000, 5_900))
    );

    // The ring stands in for rows the store has not committed: with every ring bucket
    // passed in, the open one is counted too.
    let with = r.net_by_app_with(h.host, T0, T0 + 70_000, &ring).unwrap();
    assert_eq!(with.measured_ms, 60_000);
}

#[test]
fn sleep_flushes_partial_buckets_and_measures_nothing_while_asleep() {
    let (mut h, _, _) = harness("history-sleep");
    h.ticks(15);
    // The sample at 11 s put 1 s into the bucket at 10 s.
    let ack = h.power.will_sleep(h.clock.now());
    h.pump();
    assert!(ack.recv_timeout(Duration::from_secs(1)).is_ok());
    let mut r = h.reader();
    let partial = r.net_by_app(h.host, T0 + 10_000, T0 + 20_000).unwrap();
    assert_eq!(partial.measured_ms, 1_000, "flushed at sleep, as measured");

    h.clock.advance(Duration::from_secs(120));
    h.power.did_wake(h.clock.now());
    h.pump();
    h.ticks(25);
    let shape = ring_shape(&h);
    // Woke at 135 s: both collectors start from a baseline at 136 s; nothing between.
    assert_eq!(
        shape.iter().filter(|(s, _)| (20..130).contains(s)).count(),
        0,
        "{shape:?}"
    );
    assert!(shape.contains(&(10, 1_000)), "{shape:?}");
    let after: u32 = shape
        .iter()
        .filter(|(s, _)| *s >= 130)
        .map(|(_, m)| m)
        .sum();
    assert_eq!(
        after, 20_000,
        "process ticks 136 s, 146 s, 156 s: {shape:?}"
    );
    let mut r = h.reader();
    let slept = r.net_by_app(h.host, T0 + 10_000, T0 + 130_000).unwrap();
    assert_eq!(slept.measured_ms, 1_000);
}

#[test]
fn a_clock_step_resets_the_buckets_without_counting_the_step() {
    let (mut h, _, _) = harness("history-step");
    h.ticks(15);
    h.clock.jump_wall(600_000);
    h.ticks(20);
    let shape = ring_shape(&h);
    // Before the step: 1 s to 11 s measured, written as [0 s] 9 s and [10 s] 1 s. After
    // it (wall 616 s on), only spans after the step tick count: nothing lands in the
    // skipped ten minutes.
    assert!(shape.iter().all(|(s, _)| *s < 20 || *s >= 610), "{shape:?}");
    assert!(
        shape.contains(&(0, 9_000)) && shape.contains(&(10, 1_000)),
        "{shape:?}"
    );
    let after: u32 = shape
        .iter()
        .filter(|(s, _)| *s >= 610)
        .map(|(_, m)| m)
        .sum();
    // The step tick is wall 616 s; process ticks follow at 621 s and 631 s. The first
    // sample's interval reaches back to 611 s and counts only from 616 s.
    assert_eq!(after, 15_000, "{shape:?}");

    // Stepping back drops the ring's buckets past the new time.
    h.clock.jump_wall(-300_000);
    h.ticks(1);
    assert!(
        ring_shape(&h).iter().all(|(s, _)| *s < 340),
        "{:?}",
        ring_shape(&h)
    );
}

#[test]
fn history_off_flushes_and_stops_recording() {
    let (mut h, nh, _) = harness("history-off");
    h.ticks(15);
    h.ctl.set_network_history(false);
    h.ticks(1);
    let mut r = h.reader();
    let open = r.net_by_app(h.host, T0 + 10_000, T0 + 20_000).unwrap();
    assert_eq!(
        open.measured_ms, 1_000,
        "the open bucket is written as measured"
    );
    assert_eq!(
        ring_shape(&h),
        [(0, 9_000), (10, 1_000)],
        "closed as it was"
    );
    h.ticks(60);
    let mut r = h.reader();
    assert_eq!(
        r.net_by_app(h.host, T0 + 20_000, T0 + 80_000)
            .unwrap()
            .measured_ms,
        0
    );
    assert_eq!(nh.lock().unwrap().samples, 2);
}

#[test]
fn switching_the_network_module_off_resets_the_buckets() {
    let (mut h, _, _) = harness("history-module");
    h.ticks(15);
    let mut s = settings();
    s.modules.get_mut(&Module::Network).unwrap().enabled = false;
    h.ctl.apply_settings(&s);
    h.ticks(30);
    assert_eq!(
        ring_shape(&h),
        [(0, 9_000), (10, 1_000)],
        "closed as it was"
    );
    let mut r = h.reader();
    let got = r.net_by_app(h.host, T0, T0 + 50_000).unwrap();
    assert_eq!(got.measured_ms, 10_000);
}

/// Where the hub says the engine's buckets stop being final, as an offset from `T0`
/// in s.
fn complete_to(h: &Harness) -> Option<i64> {
    h.live
        .recent_net(i64::MIN, i64::MAX)
        .complete_to_ms
        .map(|t| (t - T0) / 1_000)
}

#[test]
fn a_bucket_is_open_until_both_streams_report_past_it() {
    let (mut h, _, _) = harness("history-phase");
    // The interface every tick; apps on the process ticks, 1 s, 11 s, 21 s and so on:
    // 1 s off the bucket grid, the way NetworkStatistics runs with no process view.
    h.ctl.set_detail_interest(true);
    h.ticks(61);
    assert_eq!(complete_to(&h), Some(60));
    // The interface has reported to 70 s; the apps only to 61 s. The bucket at 60 s
    // holds 1 s of apps against 10 s of interface, so it is not complete.
    h.ticks(9);
    assert_eq!(complete_to(&h), Some(60), "{:?}", ring_shape(&h));
    let ring = h.live.recent_net(T0 + 60_000, T0 + 70_000);
    assert_eq!(ring.buckets.len(), 1);
    assert_eq!(ring.buckets[0].1.measured_ms, 1_000);
    // The process tick at 71 s reports past it: closed, whole for both streams.
    h.ticks(1);
    assert_eq!(complete_to(&h), Some(70));
    let ring = h.live.recent_net(T0 + 60_000, T0 + 70_000);
    let b = &ring.buckets[0].1;
    assert_eq!(
        (b.measured_ms, b.iface_rx_bytes),
        (10_000, IFACE_RX_BPS * 10)
    );
    assert_eq!(rx_of(b, Some(FAKE_APP)), 10_000);
}

/// A window showing per-process rates, so NetworkStatistics samples every tick whether
/// history is on or off, and the interface every tick too.
fn harness_with_rates(name: &str) -> Harness {
    let (h, _, _) = harness(name);
    h.ctl.set_process_interest(Some(0));
    h.ctl.set_network_process_interest(true);
    h.ctl.set_detail_interest(true);
    h
}

/// The stored bucket at `T0 + secs`: measured ms and Alpha's rx bytes.
fn stored(h: &Harness, secs: i64) -> (u64, u64) {
    let from = T0 + secs * 1_000;
    let got = h.reader().net_by_app(h.host, from, from + 10_000).unwrap();
    let alpha = got
        .apps
        .iter()
        .find(|a| a.name.as_deref() == Some(FAKE_APP))
        .map_or(0, |a| a.rx_bytes);
    (got.measured_ms, alpha)
}

#[test]
fn history_back_on_within_a_bucket_keeps_the_part_already_written() {
    let mut h = harness_with_rates("history-toggle");
    // Apps every tick from the baseline at 1 s: the bucket at 10 s has 10 s to 13 s.
    h.ticks(13);
    h.ctl.set_network_history(false);
    h.ticks(1);
    assert_eq!(stored(&h, 10), (3_000, 3_000), "written at the toggle");
    // Back on at 15 s, still inside the bucket at 10 s.
    h.ctl.set_network_history(true);
    h.ticks(25);
    assert_eq!(
        stored(&h, 10),
        (3_000, 3_000),
        "the rest of the bucket after re-enabling does not replace the row"
    );
    let shape = ring_shape(&h);
    assert_eq!(
        shape.iter().filter(|(s, _)| *s == 10).count(),
        1,
        "{shape:?}"
    );
    assert!(shape.contains(&(10, 3_000)), "{shape:?}");
    // From the next bucket on, counting is whole again.
    assert_eq!(stored(&h, 20), (10_000, 10_000));
}

#[test]
fn the_network_module_back_on_within_a_bucket_keeps_the_part_already_written() {
    let mut h = harness_with_rates("history-module-toggle");
    h.ticks(13);
    let mut off = settings();
    off.modules.get_mut(&Module::Network).unwrap().enabled = false;
    h.ctl.apply_settings(&off);
    h.ticks(1);
    assert_eq!(stored(&h, 10), (3_000, 3_000));
    h.ctl.apply_settings(&settings());
    h.ticks(25);
    assert_eq!(stored(&h, 10), (3_000, 3_000));
    let shape = ring_shape(&h);
    assert_eq!(
        shape.iter().filter(|(s, _)| *s == 10).count(),
        1,
        "{shape:?}"
    );
}

/// Shuts `h`'s engine down (it flushes its open buckets) and starts another on the same
/// store with fresh collectors, its wall clock `wall_shift_ms` from where the old one
/// stopped: an app restart, with the clock changed in between when that is not zero.
fn restarted(h: Harness, wall_shift_ms: i64) -> Harness {
    h.ctl.shutdown();
    let Harness {
        mut engine,
        store,
        host,
        dir,
        clock,
        ..
    } = h;
    engine.pump();
    drop(engine);
    let mut at = clock.now();
    at.wall_ms += wall_shift_ms;
    at.continuous_ns += 2_000_000_000;
    let (ticker, clock) = FakeTicker::at(at);
    let (power_signals, power) = FakePowerSignals::new();
    let (procs, _) = processes();
    let (net, _) = fake_net();
    let (iface, _) = fake_iface();
    let bus = Bus::default();
    let sub = bus.subscribe();
    let live = LiveHub::new(bus.clone());
    let engine = Engine::new(
        host,
        EngineParts {
            collectors: vec![procs, net, iface],
            ticker: Box::new(ticker),
            power: Box::new(power_signals),
            hints: None,
        },
        SourceSink {
            live: live.clone(),
            store: store.as_ref().map(Store::writer),
        },
        &settings(),
    );
    let ctl = engine.control();
    let mut h = Harness {
        engine,
        ctl,
        clock,
        power,
        sub,
        bus,
        live,
        store,
        host,
        dir,
    };
    h.engine.start();
    assert!(h.engine.pump());
    h.ctl.set_process_interest(Some(0));
    h.ctl.set_network_process_interest(true);
    h.visible();
    h
}

#[test]
fn a_restart_within_a_bucket_keeps_the_part_the_last_run_wrote() {
    let mut h = harness_with_rates("history-restart");
    h.ticks(13);
    // Shut down at 13 s (the bucket at 10 s written with 3 s), started again at 15 s.
    let mut h = restarted(h, 2_000);
    assert_eq!(stored(&h, 10), (3_000, 3_000), "written at shutdown");
    h.ticks(25);
    assert_eq!(
        stored(&h, 10),
        (3_000, 3_000),
        "the new run counts from the stored row's end, not over it"
    );
    assert_eq!(stored(&h, 20), (10_000, 10_000));
}

#[test]
fn a_stored_row_ahead_of_the_clock_does_not_hold_counting_back() {
    let mut h = harness_with_rates("history-restart-behind");
    h.ticks(13);
    // Started again with the clock a minute behind: the row at 10 s is in its future.
    let mut h = restarted(h, -60_000);
    h.ticks(40);
    assert_eq!(
        stored(&h, -40),
        (10_000, 10_000),
        "counted from the start, not from the old row's end"
    );
}

#[test]
fn reattaching_the_store_within_a_bucket_keeps_the_part_already_written() {
    let mut h = harness_with_rates("history-reattach");
    h.ticks(13);
    let writer = h.store.as_ref().unwrap().writer();
    let done = h.ctl.set_store(Some(writer));
    h.pump();
    assert!(done.recv_timeout(Duration::from_secs(1)).is_ok());
    assert_eq!(stored(&h, 10), (3_000, 3_000), "flushed at the swap");
    h.ticks(25);
    assert_eq!(stored(&h, 10), (3_000, 3_000));
    assert_eq!(stored(&h, 20), (10_000, 10_000));
}

#[test]
fn until_the_first_sample_the_edge_is_the_bucket_the_engine_started_in() {
    let h = harness_with_rates("history-start-edge");
    // A fresh store, the engine started at 18 s.
    let mut h = restarted(h, 18_000);
    assert_eq!(complete_to(&h), Some(10), "nothing counts before the start");
    // The baseline sample at 19 s adds nothing; the bucket at 10 s will still get
    // [19 s, 20 s), so it is not final although the clock's bucket is.
    h.ticks(1);
    assert_eq!(complete_to(&h), Some(10));
    h.ticks(2);
    assert_eq!(complete_to(&h), Some(20), "both streams past 20 s");
    assert_eq!(ring_shape(&h), [(10, 1_000), (20, 1_000)]);
}
