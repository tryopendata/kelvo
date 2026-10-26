use super::*;
use kelvo_collect::{Interval, ProcessNet, UsageObservation};

fn row(pid: i32, cpu: f32, mem: u64, secs: f32) -> ProcessSample {
    ProcessSample {
        pid,
        start_time_us: i64::from(pid),
        name: format!("p{pid}").into(),
        cpu_pct: cpu,
        mem_bytes: mem,
        compressed_bytes: None,
        threads: 1,
        idle_wakeups_per_s: 0.0,
        energy: 1.0,
        energy_j: secs,
        interval_s: secs,
        app: Some("App".into()),
        app_main: false,
        disk_read_bps: 0.0,
        disk_write_bps: 0.0,
        net_rx_bps: None,
        net_tx_bps: None,
        gpu_pct: Some(10.0),
        ports: None,
        user: "me".into(),
    }
}
fn tick(ts: i64) -> Tick {
    Tick {
        n: 0,
        wall_ms: ts,
        continuous_ns: ts as u64 * 1_000_000,
        interval_ms: 1000,
    }
}
fn buf(span: u64) -> SampleBuf {
    let mut buf = SampleBuf::new();
    for resource in [0, 1, 2, 3] {
        buf.set_usage_observation(
            resource,
            UsageObservation {
                span_ms: span,
                causes: [None; 4],
            },
        );
    }
    buf
}

#[test]
fn concurrent_app_durations_are_a_union_and_ram_is_a_simultaneous_sum() {
    let mut recorder = UsageRecorder::default();
    let rows = [row(1, 10.0, 100, 10.0), row(2, 20.0, 200, 5.0)];
    recorder.record(tick(10_000), &rows, &buf(10_000), [true; 5], [true; 5]);
    let snapshots = recorder.snapshots();
    let b = &snapshots[0];
    let app = &b.apps[0].facts;
    assert_eq!(app.present_ms, 10_000);
    assert_eq!(app.coverage[0].measured_ms, 10_000);
    assert_eq!(app.mem_peak_bytes, 300);
    assert_eq!(app.mem_byte_ms, 2_000_000.0);
    assert_eq!(app.cpu_seconds, 2.0);
    assert_eq!(app.energy_j, 15.0);
    assert_eq!(b.processes.len(), 2);
    // Another sample can have a larger helper after the other has left; maxima
    // across samples never add the departed helper's historical maximum.
    recorder.record(
        tick(15_000),
        &[row(2, 0.0, 400, 5.0)],
        &buf(5000),
        [true; 5],
        [true; 5],
    );
    let snapshots = recorder.snapshots();
    assert_eq!(snapshots[1].apps[0].facts.mem_peak_bytes, 400);
}

#[test]
fn gpu_uses_its_own_read_span_and_pid_reuse_retains_both_instances() {
    let mut recorder = UsageRecorder::default();
    let mut buf = buf(1000);
    buf.set_process_gpu_measured(30_000);
    recorder.record(
        tick(30_000),
        &[row(1, 10.0, 100, 1.0)],
        &buf,
        [true; 5],
        [true; 5],
    );
    let snapshots = recorder.snapshots();
    assert_eq!(snapshots.len(), 3);
    assert!(
        (snapshots
            .iter()
            .map(|b| b.apps[0].facts.gpu_seconds)
            .sum::<f64>()
            - 3.0)
            .abs()
            < 1e-9
    );
    assert!(
        (snapshots
            .iter()
            .map(|b| b.apps[0].facts.cpu_seconds)
            .sum::<f64>()
            - 0.1)
            .abs()
            < 1e-9
    );
    let mut reused = row(1, 10.0, 100, 1.0);
    reused.start_time_us = 999;
    recorder.record(tick(31_000), &[reused], &buf, [true; 5], [true; 5]);
    assert!(
        recorder
            .snapshots()
            .iter()
            .flat_map(|b| &b.processes)
            .any(|p| p.start_time_us == 999)
    );
    assert!(
        recorder
            .snapshots()
            .iter()
            .flat_map(|b| &b.processes)
            .any(|p| p.start_time_us == 1)
    );
}

#[test]
fn exact_network_bytes_are_conserved_and_late_bytes_stay_out_of_exact_totals() {
    let mut recorder = UsageRecorder::default();
    let mut buf = SampleBuf::new();
    buf.set_process_net_measured(Interval {
        prev_ns: 0,
        now_ns: 30_000_000_000,
    });
    buf.set_usage_observation(
        4,
        UsageObservation {
            span_ms: 30_000,
            causes: [Some(UsageIncompleteCause::IdentityLimit), None, None, None],
        },
    );
    buf.set_net_session_from_ns(0);
    buf.push_process_net(ProcessNet {
        pid: 1,
        identity: Some("App".into()),
        rx_bytes: 10,
        tx_bytes: 7,
        rx_bps: 0.0,
        tx_bps: 0.0,
        late_rx_bytes: 100,
        late_tx_bytes: 70,
    });
    recorder.record(tick(30_000), &[], &buf, [true; 5], [true; 5]);
    let snapshots = recorder.snapshots();
    assert_eq!(
        snapshots
            .iter()
            .flat_map(|b| &b.apps)
            .map(|a| a.facts.network_rx_bytes)
            .sum::<u64>(),
        10
    );
    assert_eq!(
        snapshots
            .iter()
            .flat_map(|b| &b.apps)
            .map(|a| a.facts.network_tx_bytes)
            .sum::<u64>(),
        7
    );
    let mut ring = UsageRecent::default();
    ring.update_owned(snapshots.clone(), 30_000);
    let selected = ring.snapshot(0, 10_000);
    assert!(
        selected.iter().any(|b| b.from_ms == 20_000),
        "discovery after the selected end still contributes uncertainty"
    );
    let uncertainty = selected
        .iter()
        .flat_map(|b| &b.uncertain_network)
        .next()
        .unwrap();
    assert_eq!(
        (uncertainty.possible_from_ms, uncertainty.possible_to_ms),
        (0, 30_000)
    );
    assert_eq!(uncertainty.rx_bytes, 100);
    assert!(
        snapshots
            .iter()
            .all(|b| b.source[4].causes & cause_mask(UsageIncompleteCause::IdentityLimit) != 0)
    );
    ring.clear();
    assert!(ring.snapshot(0, 30_000).is_empty());
}

#[test]
fn restarted_recorder_excludes_the_prior_partial_bucket_and_disabled_resources() {
    let mut recorder = UsageRecorder::default();
    recorder.restart(10_000);
    recorder.record(
        tick(15_000),
        &[row(1, 10.0, 100, 15.0)],
        &buf(15_000),
        [false, true, true, true, true],
        [true; 5],
    );
    let snapshots = recorder.snapshots();
    assert_eq!(snapshots.len(), 1);
    let b = &snapshots[0];
    assert_eq!(b.from_ms, 10_000);
    assert_eq!(b.source[0].measured_ms, 0);
    assert_eq!(b.apps[0].facts.cpu_seconds, 0.0);
    assert_eq!(b.apps[0].facts.present_ms, 5000);
    recorder.drop_after(12_000);
    assert!(recorder.snapshots().is_empty());
}
