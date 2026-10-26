#![allow(clippy::too_many_lines, clippy::unwrap_used)]
//! Public recording -> SQLite -> restart/rollup -> query behavior.
mod common;

use common::*;
use kelvo_schema::durable_usage::*;
use kelvo_store::{Retention, StoreError, usage::*};
use std::sync::Arc;

fn coverage() -> [UsageSourceCoverage; 5] {
    [UsageSourceCoverage {
        supported: true,
        measured_ms: 10_000,
        causes: 0,
    }; 5]
}
fn facts(cpu: f64, gpu: f64, mem: u64) -> UsageFacts {
    UsageFacts {
        cpu_seconds: cpu,
        gpu_seconds: gpu,
        energy_j: cpu / 2.0,
        mem_peak_bytes: mem,
        mem_byte_ms: mem as f64 * 10_000.0,
        present_ms: 10_000,
        network_rx_bytes: 100,
        network_tx_bytes: 20,
        coverage: coverage(),
    }
}
fn one(at: i64, cpu: f64) -> UsageBucket {
    UsageBucket {
        from_ms: at,
        to_ms: at + 10_000,
        source: coverage(),
        apps: vec![UsageAppFacts {
            name: Arc::from("Editor"),
            facts: facts(cpu, 0.0, 100),
        }],
        processes: vec![UsageProcessFacts {
            app_name: Arc::from("Editor"),
            pid: 42,
            start_time_us: 1000,
            name: Arc::from("Editor"),
            facts: facts(cpu, 0.0, 100),
        }],
        uncertain_network: Vec::new(),
    }
}
fn request(from: i64, to: i64, by: HistoricalUsageMetric) -> HistoricalUsageRequest {
    HistoricalUsageRequest {
        host: host(1).id,
        from_ms: from,
        to_ms: to,
        by,
        app_names: Vec::new(),
    }
}
fn children(from: i64, to: i64) -> UsageProcessRequest {
    UsageProcessRequest {
        host: host(1).id,
        from_ms: from,
        to_ms: to,
        app_name: "Editor".into(),
        by: HistoricalUsageMetric::Cpu,
        page_size: 1,
        cursor: None,
    }
}

#[test]
fn full_population_is_ranked_per_resource_after_interval_aggregation_and_restart() {
    let dir = temp_dir("usage-ranks");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    for n in 0..2 {
        let mut b = one(T0 + n * 10_000, 10.0);
        for i in 0..12 {
            b.apps.push(UsageAppFacts {
                name: Arc::from(format!("Worker {i}")),
                facts: facts(
                    if n == 0 { 11.0 } else { 0.0 },
                    if i == 11 { 4.0 } else { 0.0 },
                    100 + i as u64,
                ),
            });
        }
        w.write_usage_bucket(host(1).id, b).unwrap();
    }
    w.flush().unwrap();
    drop(w);
    store.close().unwrap();
    let store = open(&dir, "h.sqlite");
    let mut r = store.reader().unwrap();
    let cpu = r
        .historical_usage_after(
            &request(T0, T0 + 20_000, HistoricalUsageMetric::Cpu),
            Vec::new,
        )
        .unwrap();
    assert_eq!(cpu.apps.len(), 10);
    assert_eq!(cpu.apps[0].name, "Editor");
    assert_eq!(cpu.apps[0].figures.cpu_avg_pct, Some(100.0));
    assert!(cpu.ranking_complete);
    let gpu = r
        .historical_usage_after(
            &request(T0, T0 + 20_000, HistoricalUsageMetric::Gpu),
            Vec::new,
        )
        .unwrap();
    assert_eq!(gpu.apps[0].name, "Worker 11");
    assert_eq!(gpu.apps[0].figures.gpu_avg_pct, Some(40.0));
}

#[test]
fn recent_buckets_replace_committed_facts_even_when_commit_races_the_read() {
    let dir = temp_dir("usage-recent");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    w.write_usage_bucket(host(1).id, one(T0, 1.0)).unwrap();
    w.flush().unwrap();
    let newer = one(T0, 3.0);
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(
            &request(T0, T0 + 10_000, HistoricalUsageMetric::Cpu),
            || {
                w.write_usage_bucket(host(1).id, newer.clone()).unwrap();
                w.flush().unwrap();
                vec![newer]
            },
        )
        .unwrap();
    assert_eq!(reply.apps[0].figures.cpu_avg_pct, Some(30.0));
    assert_eq!(reply.range.coverage[0].coverage.measured_ms, 10_000);
}

#[test]
fn process_instances_paginate_without_pid_reuse_or_silent_loss() {
    let dir = temp_dir("usage-pages");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    for n in 0..3 {
        let mut b = one(T0 + n * 10_000, 1.0);
        b.processes[0].start_time_us = 1000 + n;
        w.write_usage_bucket(host(1).id, b).unwrap();
    }
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let mut q = children(T0, T0 + 30_000);
    let mut seen = Vec::new();
    loop {
        let page = r.usage_processes_after(&q, Vec::new).unwrap();
        assert_eq!(page.processes.len(), 1);
        seen.push((page.processes[0].pid, page.processes[0].start_time_us));
        if page.next_cursor.is_none() {
            break;
        }
        q.cursor = page.next_cursor;
    }
    assert_eq!(seen, vec![(42, 1000), (42, 1001), (42, 1002)]);
    let first = r
        .usage_processes_after(&children(T0, T0 + 30_000), Vec::new)
        .unwrap();
    let mut changed = one(T0, 8.0);
    changed.processes[0].start_time_us = 1000;
    w.write_usage_bucket(host(1).id, changed).unwrap();
    w.flush().unwrap();
    q.cursor = first.next_cursor;
    assert!(matches!(
        r.usage_processes_after(&q, Vec::new),
        Err(StoreError::Usage(_))
    ));
}

#[test]
fn late_owner_after_the_selection_preserves_uncertainty_without_assigning_bytes() {
    let dir = temp_dir("usage-late");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut before = one(T0, 1.0);
    before.apps[0].facts.network_rx_bytes = 0;
    before.apps[0].facts.network_tx_bytes = 0;
    w.write_usage_bucket(host(1).id, before).unwrap();
    w.flush().unwrap();
    let mut later = one(T0 + 60_000, 1.0);
    later.uncertain_network.push(UsageNetworkUncertainty {
        app_name: Some("Editor".into()),
        possible_from_ms: T0,
        possible_to_ms: T0 + 70_000,
        rx_bytes: 500,
        tx_bytes: 100,
        records: 1,
    });
    let q = request(T0, T0 + 10_000, HistoricalUsageMetric::Network);
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(&q, || vec![later.clone()])
        .unwrap();
    assert_eq!(reply.apps[0].figures.network_rx_bytes, Some(0));
    assert!(!reply.ranking_complete);
    assert_eq!(reply.network_uncertainty[0].rx_bytes, 500);
    w.write_usage_bucket(host(1).id, later.clone()).unwrap();
    w.flush().unwrap();
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(&q, || vec![later])
        .unwrap();
    assert_eq!(
        reply.network_uncertainty[0].rx_bytes, 500,
        "durable and recent uncertainty must not double count"
    );
}

#[test]
fn rollup_preserves_integrals_and_app_peak_but_excludes_partial_retained_edges() {
    let dir = temp_dir("usage-rollup");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut first = one(T0, 2.0);
    first.apps[0].facts.mem_peak_bytes = 150;
    let mut second = one(T0 + 10_000, 3.0);
    second.apps[0].facts.mem_peak_bytes = 200;
    w.write_usage_bucket(host(1).id, first).unwrap();
    w.write_usage_bucket(host(1).id, second).unwrap();
    w.flush().unwrap();
    let q = request(T0, T0 + MIN, HistoricalUsageMetric::Memory);
    let before = store
        .reader()
        .unwrap()
        .historical_usage_after(&q, Vec::new)
        .unwrap();
    w.prune(T0 + 2 * DAY, Retention::default()).unwrap();
    let minute = store
        .reader()
        .unwrap()
        .historical_usage_after(&q, Vec::new)
        .unwrap();
    assert_eq!(minute.apps[0].figures.mem_peak_bytes, Some(200));
    assert_eq!(
        minute.apps[0].figures.energy_j,
        before.apps[0].figures.energy_j
    );
    assert_eq!(minute.range.spans[0].resolution_ms, 60_000);
    let edge = store
        .reader()
        .unwrap()
        .historical_usage_after(
            &request(T0, T0 + 15_000, HistoricalUsageMetric::Cpu),
            Vec::new,
        )
        .unwrap();
    assert!(edge.apps.is_empty());
    assert!(!edge.ranking_complete);
    assert!(
        edge.range.coverage[0]
            .coverage
            .causes
            .contains(&UsageIncompleteCause::PartialBoundary)
    );
    w.prune(T0 + 8 * DAY, Retention::default()).unwrap();
    let quarter = store
        .reader()
        .unwrap()
        .historical_usage_after(
            &request(T0, T0 + 15 * MIN, HistoricalUsageMetric::Cpu),
            Vec::new,
        )
        .unwrap();
    assert_eq!(
        quarter.apps[0].figures.energy_j,
        before.apps[0].figures.energy_j
    );
    assert_eq!(quarter.range.spans[0].resolution_ms, 900_000);
}

#[test]
fn unknown_or_cleared_history_never_returns_proven_absent_zero_values() {
    let dir = temp_dir("usage-clear");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    w.write_usage_bucket(host(1).id, one(T0, 1.0)).unwrap();
    w.flush().unwrap();
    let mut q = request(T0 - DAY, T0 - DAY + MIN, HistoricalUsageMetric::Cpu);
    q.app_names = vec!["Editor".into()];
    let old = store
        .reader()
        .unwrap()
        .historical_usage_after(&q, Vec::new)
        .unwrap();
    assert_eq!(old.apps[0].figures.cpu_avg_pct, None);
    assert!(!old.ranking_complete);
    w.clear_host(host(1).id, T0 + MIN).unwrap();
    q.from_ms = T0;
    q.to_ms = T0 + 10_000;
    let cleared = store
        .reader()
        .unwrap()
        .historical_usage_after(&q, Vec::new)
        .unwrap();
    assert_eq!(cleared.apps[0].figures.cpu_avg_pct, None);
    assert!(!cleared.apps[0].process_details_available);
}

#[test]
fn source_only_change_invalidates_process_continuation_and_network_only_has_no_children() {
    let dir = temp_dir("usage-source-cursor");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = one(T0, 1.0);
    let mut p = b.processes[0].clone();
    p.pid = 43;
    b.processes.push(p);
    let mut network = facts(0.0, 0.0, 0);
    network.coverage[..4].fill(UsageSourceCoverage::default());
    b.apps.push(UsageAppFacts {
        name: Arc::from("Network only"),
        facts: network,
    });
    w.write_usage_bucket(host(1).id, b.clone()).unwrap();
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let mut q = children(T0, T0 + 10_000);
    q.cursor = r.usage_processes_after(&q, Vec::new).unwrap().next_cursor;
    assert!(q.cursor.is_some());
    b.source[0].causes = cause_mask(UsageIncompleteCause::UnreadableProcess);
    w.write_usage_bucket(host(1).id, b).unwrap();
    w.flush().unwrap();
    assert!(matches!(
        r.usage_processes_after(&q, Vec::new),
        Err(StoreError::Usage(_))
    ));
    let apps = r
        .historical_usage_after(
            &request(T0, T0 + 10_000, HistoricalUsageMetric::Network),
            Vec::new,
        )
        .unwrap();
    assert!(
        !apps
            .apps
            .iter()
            .find(|a| a.name == "Network only")
            .unwrap()
            .process_details_available
    );
}

#[test]
fn retention_removes_details_and_recent_cannot_resurrect_pruned_intervals() {
    let dir = temp_dir("usage-pruned");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let b = one(T0, 1.0);
    w.write_usage_bucket(host(1).id, b.clone()).unwrap();
    w.flush().unwrap();
    w.prune(T0 + 32 * DAY, Retention::default()).unwrap();
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(
            &request(T0, T0 + 10_000, HistoricalUsageMetric::Cpu),
            || vec![b],
        )
        .unwrap();
    assert!(reply.apps.is_empty());
    assert!(
        reply.range.coverage[0]
            .coverage
            .causes
            .contains(&UsageIncompleteCause::Pruned)
    );
    assert!(
        store
            .reader()
            .unwrap()
            .usage_processes_after(&children(T0, T0 + 10_000), Vec::new)
            .unwrap()
            .processes
            .is_empty()
    );
}

#[test]
fn replaying_unresolved_uncertainty_is_idempotent() {
    let dir = temp_dir("usage-unknown-replay");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = one(T0, 1.0);
    b.uncertain_network.push(UsageNetworkUncertainty {
        app_name: None,
        possible_from_ms: T0,
        possible_to_ms: T0 + 10_000,
        rx_bytes: 12,
        tx_bytes: 3,
        records: 1,
    });
    w.write_usage_bucket(host(1).id, b.clone()).unwrap();
    w.flush().unwrap();
    w.write_usage_bucket(host(1).id, b).unwrap();
    w.flush().unwrap();
    let reply = store
        .reader()
        .unwrap()
        .historical_usage_after(
            &request(T0, T0 + 10_000, HistoricalUsageMetric::Network),
            Vec::new,
        )
        .unwrap();
    assert_eq!(reply.network_uncertainty[0].rx_bytes, 12);
    assert_eq!(reply.network_uncertainty[0].records, 1);
}

#[test]
fn usage_only_cap_reclaims_churn_identities_without_dangling_retained_details() {
    let dir = temp_dir("usage-cap");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    for day in 0..8 {
        let mut b = one(T0 + day * DAY, 1.0);
        for id in 0..2000 {
            b.processes.push(UsageProcessFacts {
                app_name: Arc::from("Editor"),
                pid: 100 + id,
                start_time_us: day * DAY * 1000 + id as i64,
                name: Arc::from(format!("worker-{day}-{id}")),
                facts: facts(0.1, 0.0, 10),
            });
        }
        w.write_usage_bucket(host(1).id, b).unwrap();
    }
    w.flush().unwrap();
    let now = T0 + 8 * DAY;
    let full = w.prune(now, Retention::default()).unwrap();
    let report = w
        .prune(
            now,
            Retention {
                max_bytes: full.size_bytes * 4 / 5,
                ..Retention::default()
            },
        )
        .unwrap();
    assert!(
        report.cap_trim.as_ref().is_some_and(|t| t.cap_met),
        "{report:?}"
    );
    let mut r = store.reader().unwrap();
    let q = children(T0 + 7 * DAY, T0 + 7 * DAY + MIN);
    assert!(
        !r.usage_processes_after(&q, Vec::new)
            .unwrap()
            .processes
            .is_empty()
    );
    let old = r
        .historical_usage_after(&request(T0, T0 + MIN, HistoricalUsageMetric::Cpu), Vec::new)
        .unwrap();
    assert!(old.apps.is_empty());
    assert!(
        old.range.coverage[0]
            .coverage
            .causes
            .contains(&UsageIncompleteCause::Pruned)
    );
}

#[test]
fn identical_late_observations_preserve_multiplicity_and_open_bucket_replacement() {
    let dir = temp_dir("usage-uncertain-multiplicity");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = one(T0, 1.0);
    let u = UsageNetworkUncertainty {
        app_name: Some("Editor".into()),
        possible_from_ms: T0,
        possible_to_ms: T0 + 10_000,
        rx_bytes: 12,
        tx_bytes: 3,
        records: 1,
    };
    b.uncertain_network = vec![u.clone(), u.clone()];
    w.write_usage_bucket(host(1).id, b.clone()).unwrap();
    w.flush().unwrap();
    w.write_usage_bucket(host(1).id, b.clone()).unwrap();
    w.flush().unwrap();
    let q = request(T0, T0 + 10_000, HistoricalUsageMetric::Network);
    let mut r = store.reader().unwrap();
    let reply = r.historical_usage_after(&q, || vec![b.clone()]).unwrap();
    assert_eq!(reply.network_uncertainty[0].records, 2);
    assert_eq!(reply.network_uncertainty[0].rx_bytes, 24);
    b.uncertain_network.push(u);
    let recent = r.historical_usage_after(&q, || vec![b.clone()]).unwrap();
    assert_eq!(recent.network_uncertainty[0].records, 3);
    assert_eq!(recent.network_uncertainty[0].rx_bytes, 36);
    w.write_usage_bucket(host(1).id, b).unwrap();
    w.flush().unwrap();
    let stored = r.historical_usage_after(&q, Vec::new).unwrap();
    assert_eq!(stored.network_uncertainty, recent.network_uncertainty);
}
