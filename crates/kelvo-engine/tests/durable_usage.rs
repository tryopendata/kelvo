//! Recorder through real engine scheduling, recent snapshots and SQLite boundaries.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]
mod common;
use common::*;
use kelvo_collect::{
    Cadence, CollectError, Collector, CollectorId, Interest, Probe, ProcessSample, SampleBuf, Tick,
    UsageObservation,
};
use kelvo_schema::durable_usage::{
    HistoricalUsageMetric, HistoricalUsageRequest, UsageIncompleteCause,
};
use kelvo_schema::{Capabilities, Entitlement, Module};
use kelvo_store::usage::{UsageBucket, cause_mask};

struct Processes {
    partial_gpu: bool,
    previous: Option<u64>,
    reset_during_read: Option<std::sync::Arc<std::sync::Mutex<Option<kelvo_engine::LiveHub>>>>,
}
impl Collector for Processes {
    fn id(&self) -> CollectorId {
        CollectorId("processes")
    }
    fn cadence(&self) -> Cadence {
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        }
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }
    fn probe(&mut self) -> Probe {
        self.previous = None;
        Probe::Supported(Vec::new())
    }
    fn fill_capabilities(&self, caps: &mut Capabilities) {
        caps.process_energy = true;
        caps.process_network = true;
        caps.process_gpu = self.partial_gpu;
    }
    fn sample(&mut self, t: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let Some(previous) = self.previous.replace(t.continuous_ns) else {
            out.set_usage_observation(
                0,
                UsageObservation {
                    span_ms: 0,
                    causes: [Some(UsageIncompleteCause::NoBaseline), None, None, None],
                },
            );
            return Ok(());
        };
        let secs = (t.continuous_ns - previous) as f32 / 1e9;
        for pid in 0..32 {
            out.push_process(ProcessSample {
                pid,
                start_time_us: 100 + i64::from(pid),
                name: format!("helper-{pid}").into(),
                cpu_pct: 1.0,
                mem_bytes: 100,
                compressed_bytes: None,
                threads: 1,
                idle_wakeups_per_s: 0.0,
                energy: 0.1,
                energy_j: 0.1 * secs,
                interval_s: secs,
                app: Some("App".into()),
                app_main: pid == 0,
                disk_read_bps: 0.0,
                disk_write_bps: 0.0,
                net_rx_bps: None,
                net_tx_bps: None,
                gpu_pct: None,
                ports: None,
                user: "me".into(),
            });
        }
        out.observe_processes((secs * 1000.0).round() as u64, 33, 32, true);
        if self.partial_gpu {
            out.set_process_gpu_measured((secs * 1000.0).round() as u64);
            out.set_usage_observation(
                1,
                UsageObservation {
                    span_ms: (secs * 1000.0).round() as u64,
                    causes: [Some(UsageIncompleteCause::ReadFailed), None, None, None],
                },
            );
            out.push_process_gpu(kelvo_collect::ProcessGpu { pid: 0, pct: 10.0 });
        }
        if let Some(live) = self
            .reset_during_read
            .as_ref()
            .and_then(|slot| slot.lock().unwrap().take())
        {
            out.set_process_net_measured(kelvo_collect::Interval {
                prev_ns: 0,
                now_ns: t.continuous_ns,
            });
            out.set_net_session_from_ns(0);
            out.push_process_net(kelvo_collect::ProcessNet {
                pid: 0,
                identity: Some("Late owner".into()),
                rx_bytes: 0,
                tx_bytes: 0,
                rx_bps: 0.0,
                tx_bps: 0.0,
                late_rx_bytes: 100,
                late_tx_bytes: 50,
            });
            live.forget_recent_rows();
        }
        Ok(())
    }
}
fn harness(name: &str) -> Harness {
    let mut h = Harness::new(
        name,
        vec![Box::new(Processes {
            partial_gpu: false,
            previous: None,
            reset_during_read: None,
        })],
        &settings_all_on(),
    );
    h.visible();
    h.ctl.set_process_interest(Some(0));
    h
}
fn recent(h: &Harness) -> Vec<UsageBucket> {
    h.live.recent_usage_buckets(T0, T0 + 600_000)
}
fn cpu(buckets: &[UsageBucket]) -> f64 {
    buckets
        .iter()
        .flat_map(|b| &b.apps)
        .map(|a| a.facts.cpu_seconds)
        .sum()
}

#[test]
fn every_helper_survives_and_concurrent_ram_and_coverage_are_not_process_sums() {
    let mut h = harness("durable-all-helpers");
    h.ticks(12);
    let buckets = recent(&h);
    let first = buckets.iter().find(|b| b.from_ms == T0).unwrap();
    let app = first
        .apps
        .iter()
        .find(|a| a.name.as_ref() == "App")
        .unwrap();
    assert_eq!(first.processes.len(), 32);
    assert_eq!(app.facts.mem_peak_bytes, 3200);
    assert_eq!(app.facts.present_ms, 9000);
    assert_eq!(app.facts.coverage[0].measured_ms, 9000);
    assert!((app.facts.cpu_seconds - 2.88).abs() < 1e-6);
    assert!((app.facts.energy_j - 28.8).abs() < 1e-5);
    assert_ne!(
        first.source[0].causes & cause_mask(UsageIncompleteCause::UnreadableProcess),
        0
    );
    assert_ne!(
        first.source[0].causes & cause_mask(UsageIncompleteCause::NoBaseline),
        0
    );
    h.ctl.set_paused(true);
    h.pump();
    let store = h.store.as_ref().unwrap();
    store.writer().flush().unwrap();
    let request = HistoricalUsageRequest {
        host: h.host,
        from_ms: T0,
        to_ms: T0 + 20_000,
        by: HistoricalUsageMetric::Cpu,
        app_names: Vec::new(),
    };
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(&request, || recent(&h))
        .unwrap();
    assert_eq!(reply.apps.len(), 1);
    assert!(!reply.ranking_complete);
    assert_eq!(reply.apps[0].figures.mem_peak_bytes, Some(3200));
    assert!(
        (reply.apps[0].figures.cpu_avg_pct.unwrap() - 32.0).abs() < 1e-6,
        "recent replaces queued durable facts rather than adding them"
    );
}

#[test]
fn clear_history_drops_open_facts_and_the_next_tick_cannot_resurrect_them() {
    let mut h = harness("durable-clear");
    h.ticks(12);
    assert!(cpu(&recent(&h)) > 0.0);
    h.live.forget_recent_rows();
    h.ticks(3); // A tick wins the race before the queued delete.
    h.store
        .as_ref()
        .unwrap()
        .writer()
        .clear_host(h.host, T0 + 12_000)
        .unwrap();
    h.live.forget_recent_rows();
    h.ticks(12);
    let buckets = recent(&h);
    assert!(buckets.iter().all(|b| b.from_ms >= T0 + 20_000));
    h.ctl.set_paused(true);
    h.pump();
    let store = h.store.as_ref().unwrap();
    store.writer().flush().unwrap();
    let request = HistoricalUsageRequest {
        host: h.host,
        from_ms: T0,
        to_ms: T0 + 20_000,
        by: HistoricalUsageMetric::Cpu,
        app_names: Vec::new(),
    };
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(&request, || h.live.recent_usage_buckets(T0, T0 + 20_000))
        .unwrap();
    assert!(reply.apps.is_empty());
}

#[test]
fn module_disabled_fields_are_not_recorded_from_other_module_process_reads() {
    let mut h = harness("durable-disabled");
    h.ticks(12);
    let mut settings = settings_all_on();
    settings.modules.get_mut(&Module::Cpu).unwrap().enabled = false;
    h.ctl.apply_settings(&settings);
    h.pump();
    h.ticks(12);
    let bucket = recent(&h)
        .into_iter()
        .find(|b| b.from_ms == T0 + 20_000)
        .unwrap();
    assert_eq!(bucket.source[0].measured_ms, 0);
    assert!(bucket.apps.iter().all(|a| a.facts.cpu_seconds == 0.0));
    assert!(bucket.apps.iter().any(|a| a.facts.mem_peak_bytes > 0));
}

#[test]
fn clock_backstep_discards_overlapping_recent_facts_and_does_not_replace_held_history() {
    let mut h = harness("durable-clock-back");
    h.ticks(12);
    h.clock.jump_wall(-5000);
    h.ticks(1);
    assert!(
        recent(&h).is_empty(),
        "overlapping old-timeline buckets are gone, held ticks add nothing"
    );
    let held_until = h.live.status().history_held_until.unwrap();
    assert_eq!(
        held_until,
        T0 + 60_000,
        "the minute rollup establishes the hold edge"
    );
    h.ticks(64); // Wall time reaches 72s after the 5s backward step.
    let buckets = recent(&h);
    assert!(!buckets.is_empty());
    assert!(buckets.iter().all(|b| b.from_ms >= held_until));
    h.ctl.set_paused(true);
    h.pump();
    let store = h.store.as_ref().unwrap();
    store.writer().flush().unwrap();
    let request = HistoricalUsageRequest {
        host: h.host,
        from_ms: held_until,
        to_ms: held_until + 10_000,
        by: HistoricalUsageMetric::Cpu,
        app_names: Vec::new(),
    };
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(&request, || recent(&h))
        .unwrap();
    assert_eq!(reply.apps.len(), 1);
    assert_eq!(reply.apps[0].figures.mem_peak_bytes, Some(3200));
}

#[test]
fn reset_during_a_collector_read_discards_that_batch_including_old_session_late_bytes() {
    let reset = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut h = Harness::new(
        "durable-reset-during-read",
        vec![Box::new(Processes {
            partial_gpu: false,
            previous: None,
            reset_during_read: Some(reset.clone()),
        })],
        &settings_all_on(),
    );
    h.visible();
    h.ctl.set_process_interest(Some(0));
    h.ticks(12);
    *reset.lock().unwrap() = Some(h.live.clone());
    h.ticks(1);
    assert!(
        recent(&h).is_empty(),
        "a reset between read and account discards every fact"
    );
    h.store
        .as_ref()
        .unwrap()
        .writer()
        .clear_host(h.host, T0 + 13_000)
        .unwrap();
    h.live.forget_recent_rows();
    h.ticks(12);
    let snapshots = recent(&h);
    assert!(snapshots.iter().all(|b| b.from_ms >= T0 + 20_000));
    assert!(snapshots.iter().all(|b| b.uncertain_network.is_empty()));
    assert!(
        h.live
            .recent_usage_apps(T0, T0 + 60_000)
            .iter()
            .all(|b| b.processes.is_empty())
    );
    let process = h.live.recent_usage_processes(T0, T0 + 60_000, "App");
    assert!(
        process
            .iter()
            .flat_map(|b| &b.processes)
            .all(|p| p.app_name.as_ref() == "App")
    );
    assert!(
        h.live
            .recent_usage_processes(T0, T0 + 60_000, "Unknown")
            .iter()
            .all(|b| b.processes.is_empty())
    );
}

#[test]
fn failed_commit_keeps_recent_facts_and_retry_does_not_double_count() {
    let mut h = harness("durable-commit-retry");
    let writer = h.store.as_ref().unwrap().writer();
    writer.set_fail_commits(true);
    h.ticks(120);
    assert!(writer.flush().is_err());
    let request = HistoricalUsageRequest {
        host: h.host,
        from_ms: T0,
        to_ms: T0 + 120_000,
        by: HistoricalUsageMetric::Cpu,
        app_names: Vec::new(),
    };
    let read = || {
        h.store
            .as_ref()
            .unwrap()
            .reader()
            .unwrap()
            .historical_usage_after(&request, || {
                h.live.recent_usage_apps(request.from_ms, request.to_ms)
            })
            .unwrap()
    };
    let before = read();
    assert_eq!(before.apps.len(), 1);
    assert!((before.apps[0].figures.cpu_avg_pct.unwrap() - 32.0).abs() < 1e-9);
    writer.set_fail_commits(false);
    writer.flush().unwrap();
    let after = read();
    assert_eq!(after.apps, before.apps);
    assert_eq!(after.range, before.range);
}

#[test]
fn public_partial_gpu_queries_keep_unknown_children_unavailable_and_observed_app_lower_bound() {
    let (gpu, _) = fake(
        "gpu_per_process",
        Cadence::OnDemand(Interest::GpuProcesses),
        &[Module::Gpu],
        &[],
    );
    let mut h = Harness::new(
        "durable-partial-gpu",
        vec![
            gpu,
            Box::new(Processes {
                previous: None,
                partial_gpu: true,
                reset_during_read: None,
            }),
        ],
        &settings_all_on(),
    );
    h.visible();
    h.ctl.set_process_interest(Some(0));
    h.ticks(12);
    let store = h.store.as_ref().unwrap();
    let request = HistoricalUsageRequest {
        host: h.host,
        from_ms: T0,
        to_ms: T0 + 20_000,
        by: HistoricalUsageMetric::Gpu,
        app_names: Vec::new(),
    };
    let apps = store
        .reader()
        .unwrap()
        .historical_usage_after(&request, || {
            h.live.recent_usage_apps(request.from_ms, request.to_ms)
        })
        .unwrap();
    assert!(!apps.ranking_complete);
    assert_eq!(apps.apps[0].figures.gpu_avg_pct, Some(10.0));
    let request = kelvo_schema::durable_usage::UsageProcessRequest {
        host: h.host,
        from_ms: T0,
        to_ms: T0 + 20_000,
        by: HistoricalUsageMetric::Gpu,
        app_name: "App".into(),
        page_size: 100,
        cursor: None,
    };
    let reply = store
        .reader()
        .unwrap()
        .usage_processes_after(&request, || {
            h.live
                .recent_usage_processes(request.from_ms, request.to_ms, "App")
        })
        .unwrap();
    assert_eq!(
        reply
            .processes
            .iter()
            .find(|p| p.pid == 1)
            .unwrap()
            .figures
            .gpu_avg_pct,
        None
    );
    assert_eq!(
        reply
            .processes
            .iter()
            .find(|p| p.pid == 0)
            .unwrap()
            .figures
            .gpu_avg_pct,
        Some(10.0)
    );
}
