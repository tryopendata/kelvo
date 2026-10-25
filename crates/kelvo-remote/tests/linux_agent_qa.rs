#![allow(clippy::unwrap_used, clippy::too_many_lines)]
//! The Linux agent's whole lifecycle on a real host over SSH: probe, install, a live
//! session, update with history kept, uninstall with nothing left. It installs for
//! real and uninstalls at the end, even after a failure. Run by hand:
//!
//! ```sh
//! make agent-linux
//! KELVO_QA_ALIAS=linux-tailscale \
//! KELVO_QA_AGENT=src-tauri/binaries/agent/kelvo-agent-aarch64-unknown-linux-musl \
//!   cargo test -p kelvo-remote --test linux_agent_qa -- --ignored --nocapture
//! ```
//!
//! The SSH login should be root (the system layout); the SELinux checks assume it.
use std::time::Duration;

use kelvo_proto::{Message, RemoteInterest};
use kelvo_remote::agent_install::{self, HostProbe};
use kelvo_remote::install::run_remote;
use kelvo_remote::{EndpointConfig, RemoteError, SshSession};
use kelvo_schema::HostId;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BUDGET_MB: u32 = 150;

async fn read(session: &mut SshSession) -> Message {
    tokio::time::timeout(Duration::from_secs(15), async {
        let length = session.reader.read_u32().await.unwrap();
        let mut body = vec![0; length as usize];
        session.reader.read_exact(&mut body).await.unwrap();
        kelvo_proto::decode_message(&body).unwrap()
    })
    .await
    .expect("the agent must make protocol progress")
}

async fn rows(alias: &str) -> u64 {
    let out = run_remote(
        alias,
        r#"python3 -c 'import sqlite3; c = sqlite3.connect("file:/var/lib/kelvo-agent/history.sqlite?mode=ro", uri=True); print(sum(c.execute("select count(*) from " + t).fetchone()[0] for t in ("tier_10s", "tier_1m", "series", "gaps")))'"#,
    )
    .await
    .unwrap();
    out.trim().parse().unwrap()
}

async fn sh(alias: &str, command: &str) -> String {
    let out = run_remote(alias, &format!("({command}) 2>&1; true"))
        .await
        .unwrap();
    println!("$ {command}\n{}", out.trim_end());
    out
}

async fn lifecycle(alias: String, bytes: Vec<u8>, sha: String) {
    let alias = alias.as_str();
    let probe = match agent_install::probe_host(alias, Some(BUDGET_MB))
        .await
        .unwrap()
    {
        HostProbe::Linux(probe) => *probe,
        HostProbe::Mac(p) => panic!("{alias} is a Mac: {p:?}"),
    };
    println!("probe: {probe:#?}\nwarnings: {:?}", probe.warnings());
    assert!(probe.existing().is_none(), "start from a clean host");
    let target = probe.target().unwrap();
    println!("target: {target:?}");

    let installed = agent_install::install(alias, &probe, &bytes, &sha, BUDGET_MB)
        .await
        .unwrap();
    println!("installed: {installed:?}");
    assert!(installed.running);
    let host_id = installed.host_id.unwrap();

    // A live session through the agent's own serve command.
    let config = EndpointConfig {
        lan: Some(alias.to_string()),
        tailscale: None,
        command: target.remote_command(),
    };
    println!("serve: {}", config.command.serve_command().unwrap());
    let local = HostId(uuid::Uuid::new_v4());
    let mut session = kelvo_remote::connect(&config, local, Some(host_id))
        .await
        .unwrap();
    println!(
        "Hello: host {} ({:?} {}), agent {}, epoch {}",
        session.hello.host.id,
        session.hello.host.info.os,
        session.hello.host.info.os_version,
        session.hello.app_version,
        session.hello.db_instance_uuid,
    );
    assert_eq!(session.hello.host.id, host_id);
    let interest = Message::Interest(RemoteInterest {
        detail: true,
        ..RemoteInterest::default()
    });
    session
        .writer
        .write_all(&kelvo_proto::encode_frame(&interest).unwrap())
        .await
        .unwrap();
    session.writer.flush().await.unwrap();
    let mut live = 0;
    while live < 3 {
        if let Message::Live(frame) = read(&mut session).await {
            live += 1;
            println!(
                "Live ts_ms={} values={} layout={}",
                frame.ts_ms,
                frame.values.len(),
                frame.layout_no
            );
        }
    }
    session.close().await;

    // A wrong expected id is a typed mismatch carrying both ids.
    let other = HostId(uuid::Uuid::new_v4());
    match kelvo_remote::connect(&config, local, Some(other)).await {
        Err(RemoteError::IdentityMismatch {
            expected, actual, ..
        }) => {
            assert_eq!((expected, actual), (other, host_id));
            println!("identity mismatch: expected {expected}, actual {actual}");
        }
        Err(e) => panic!("expected an identity mismatch, got {e}"),
        Ok(_) => panic!("expected an identity mismatch, got a session"),
    }

    let status = agent_install::status(alias, &target).await.unwrap();
    println!("status: {status:?}");
    let label = sh(alias, "ls -Z /usr/local/bin/kelvo-agent").await;
    assert!(label.contains(":bin_t:"), "{label}");
    sh(alias, "systemctl is-active kelvo-agent").await;

    // Update twice with the same bytes. The daemon commits history in batches, so the
    // first baseline can be empty; the first update's stop flushes it, which gives the
    // second a committed baseline to keep.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let mut counts = vec![rows(alias).await];
    for _ in 0..2 {
        let probe = agent_install::probe_linux(alias).await.unwrap();
        let updated = agent_install::update(alias, &probe, &bytes, &sha)
            .await
            .unwrap();
        println!("updated: {updated:?}");
        assert_eq!(updated.host_id, Some(host_id));
        assert_eq!(updated.epoch, status.epoch, "the update kept the history");
        counts.push(rows(alias).await);
    }
    println!("history rows before, after the first and after the second update: {counts:?}");
    assert!(counts.windows(2).all(|w| w[1] >= w[0]), "{counts:?}");
    assert!(counts[1] > 0, "{counts:?}");
    let leftovers = sh(
        alias,
        "ls -a /usr/local/bin | grep kelvo; ls /var/lib/kelvo-agent",
    )
    .await;
    assert!(!leftovers.contains(".prev") && !leftovers.contains("backup"));
    let label = sh(alias, "ls -Z /usr/local/bin/kelvo-agent").await;
    assert!(label.contains(":bin_t:"), "{label}");
    let avc = sh(alias, "ausearch -m avc -ts recent -c kelvo-agent").await;
    assert!(avc.contains("<no matches>"), "SELinux denials: {avc}");
}

/// A controller that connects while no window is open, then opens one, draws the hour
/// the agent recorded before it connected: the live ring refills from the synced
/// mirror. Read-only against an agent already installed (no install, no uninstall):
///
/// ```sh
/// KELVO_QA_ALIAS=linux-tailscale \
///   cargo test -p kelvo-remote --test linux_agent_qa live_ring -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "connects to an agent installed on a real host; see the test docs"]
async fn live_ring_refills_from_history_on_a_real_host() {
    use kelvo_engine::{LiveHub, Source as _, SourceSink};
    use std::sync::Arc;

    let alias = std::env::var("KELVO_QA_ALIAS").unwrap_or_else(|_| "linux-tailscale".into());
    let probe = agent_install::probe_linux(&alias).await.unwrap();
    assert!(probe.existing().is_some(), "install the agent first");
    let config = EndpointConfig {
        lan: Some(alias.clone()),
        tailscale: None,
        command: probe.target().unwrap().remote_command(),
    };
    let local = HostId(uuid::Uuid::new_v4());
    let session = kelvo_remote::connect(&config, local, None).await.unwrap();
    let record = kelvo_schema::HostRecord::from_identity(session.hello.host.clone(), false);
    session.close().await;

    let dir = tempfile::tempdir().unwrap();
    let history = Arc::new(kelvo_engine::HistoryStore::open(dir.path()));
    assert!(history.register_host(&record).is_some());
    let mirror = Arc::new(kelvo_remote::Mirror::new(history, record.id, BUDGET_MB));
    let source =
        kelvo_remote::RemoteSource::new(record, local, config, Arc::new(|_| {}), Some(mirror));
    let hub = LiveHub::default();
    let mut handle = source
        .start(SourceSink {
            live: hub.clone(),
            store: None,
        })
        .unwrap();
    // Rows with a measured value, oldest first, and the timelines they are on.
    let measured = || {
        let segs = hub.backfill(i64::MIN);
        let rows: Vec<i64> = segs
            .iter()
            .flat_map(|s| {
                s.rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.iter().any(|v| v.is_finite()))
                    .map(|(i, _)| s.start_ms + i as i64 * i64::from(s.interval_ms))
            })
            .collect();
        let timelines: std::collections::BTreeSet<u32> = segs.iter().map(|s| s.timeline).collect();
        (rows, timelines)
    };

    // No window: summary frames only, while the mirror catches up.
    tokio::time::sleep(Duration::from_secs(30)).await;
    let (before, _) = measured();
    println!(
        "before a window: {} ring rows, {} measured",
        hub.backfill(i64::MIN)
            .iter()
            .map(|s| s.rows.len())
            .sum::<usize>(),
        before.len()
    );
    assert!(before.is_empty(), "summary frames measure nothing");

    let opened = kelvo_engine::wall_ms();
    handle.control().set_detail_interest(true);
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let (rows, timelines) = loop {
        let (rows, timelines) = measured();
        if rows.first().is_some_and(|t| *t < opened - 10 * 60_000)
            || std::time::Instant::now() > deadline
        {
            break (rows, timelines);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    handle.stop();
    let oldest = rows.first().copied().unwrap_or(opened);
    let stored = rows.iter().filter(|t| **t < opened).count();
    println!(
        "after opening: {} measured rows, {stored} before the window opened, oldest {} s before it, timelines {timelines:?}",
        rows.len(),
        (opened - oldest) / 1000,
    );
    assert!(
        oldest < opened - 10 * 60_000,
        "the ring holds the history from before the window opened"
    );
    assert_eq!(timelines.len(), 1, "one timeline after the refill");
    // No hole between the stored hour and the live frames: consecutive measured rows
    // are at most one 10 s bucket apart.
    let widest = rows.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
    println!("widest step between measured rows: {widest} ms");
    assert!(widest <= 20_000, "a hole of {widest} ms");
}

/// Updates the agent already installed to `KELVO_QA_AGENT` through the app's update path
/// (history kept, unit rewritten), as the controller's "Update agent" does:
///
/// ```sh
/// KELVO_QA_ALIAS=linux-tailscale \
/// KELVO_QA_AGENT=src-tauri/binaries/agent/kelvo-agent-aarch64-unknown-linux-musl \
///   cargo test -p kelvo-remote --test linux_agent_qa update_the -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "updates the agent installed on a real host; see the test docs"]
async fn update_the_installed_agent() {
    let alias = std::env::var("KELVO_QA_ALIAS").unwrap_or_else(|_| "linux-tailscale".into());
    let bytes = std::fs::read(std::env::var("KELVO_QA_AGENT").unwrap()).unwrap();
    let sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let probe = agent_install::probe_linux(&alias).await.unwrap();
    assert!(probe.existing().is_some(), "install the agent first");
    let before = rows(&alias).await;
    let status = agent_install::update(&alias, &probe, &bytes, &sha)
        .await
        .unwrap();
    println!("updated: {status:?}");
    assert!(status.running);
    assert!(rows(&alias).await >= before, "history kept");
    sh(
        &alias,
        "grep Protect /etc/systemd/system/kelvo-agent.service",
    )
    .await;
}

/// Remote queries (D-117) against an agent already installed: per-app use over the last
/// ten minutes, then a space scan of `/` explored the way the dashboard does, then
/// cleared. Prints the agent's memory before and after; the scan's report is dropped at
/// the end. Set `KELVO_QA_DETAIL_SOAK_SECS=3600` to hold continuous detail demand for
/// the retention horizon before scanning, with live/query and memory observations.
///
/// ```sh
/// KELVO_QA_ALIAS=linux-tailscale \
///   cargo test -p kelvo-remote --test linux_agent_qa queries -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "connects to an agent installed on a real host; see the test docs"]
async fn queries_answer_on_a_real_host() {
    use kelvo_engine::{LiveHub, Source as _, SourceSink};
    use kelvo_proto::{Answer, Query};
    use kelvo_schema::disk_space::SpacePhase;
    use std::sync::Arc;

    let alias = std::env::var("KELVO_QA_ALIAS").unwrap_or_else(|_| "linux-tailscale".into());
    let probe = agent_install::probe_linux(&alias).await.unwrap();
    assert!(probe.existing().is_some(), "install the agent first");
    let config = EndpointConfig {
        lan: Some(alias.clone()),
        tailscale: None,
        command: probe.target().unwrap().remote_command(),
    };
    let local = HostId(uuid::Uuid::new_v4());
    let session = kelvo_remote::connect(&config, local, None).await.unwrap();
    let record = kelvo_schema::HostRecord::from_identity(session.hello.host.clone(), false);
    session.close().await;
    let source = kelvo_remote::RemoteSource::new(record, local, config, Arc::new(|_| {}), None);
    let hub = LiveHub::default();
    let mut handle = source
        .clone()
        .start(SourceSink {
            live: hub.clone(),
            store: None,
        })
        .unwrap();
    handle.control().set_detail_interest(true);
    let wait = Duration::from_secs(20);
    let call = |query: Query| {
        let source = source.clone();
        async move {
            for _ in 0..30 {
                match source.call(query.clone(), wait).await {
                    Err(RemoteError::Transport(m)) if m.contains("not connected") => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    other => return other.unwrap(),
                }
            }
            panic!("never connected")
        }
    };
    let memory = || {
        sh(
            &alias,
            "systemctl show kelvo-agent -p MemoryCurrent -p MemoryPeak",
        )
    };

    // Opt in to a full retention horizon with detail demand held continuously.
    // This does not install, restart, reset identity, or replace history.
    let soak_secs = std::env::var("KELVO_QA_DETAIL_SOAK_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("detail soak seconds must be an integer")
        })
        .unwrap_or(0);
    if soak_secs > 0 {
        assert!(matches!(
            call(Query::SpaceVolumes).await,
            Answer::SpaceVolumes(_)
        ));
        let started = std::time::Instant::now();
        let mut frames = std::collections::BTreeSet::new();
        let mut previous_count = 0;
        let mut next_check = Duration::ZERO;
        while started.elapsed() < Duration::from_secs(soak_secs) {
            if let Some(frame) = hub.latest_frame()
                && frame.values.iter().any(|value| value.is_finite())
            {
                frames.insert(frame.ts_ms);
            }
            if started.elapsed() >= next_check {
                let now = kelvo_engine::wall_ms();
                let Answer::UsageByApp(usage) = call(Query::UsageByApp {
                    from_ms: now - 600_000,
                    to_ms: now,
                    by: kelvo_schema::usage::UsageKey::Memory,
                    limit: 5,
                })
                .await
                else {
                    panic!("expected usage during detail soak")
                };
                if next_check > Duration::ZERO {
                    assert!(
                        frames.len() > previous_count,
                        "detail stopped advancing during soak"
                    );
                    assert!(usage.covered_ms > 0 && !usage.apps.is_empty());
                }
                previous_count = frames.len();
                println!(
                    "detail soak: {} s, {} distinct measured frames",
                    started.elapsed().as_secs(),
                    frames.len()
                );
                sh(
                    &alias,
                    "date -u; systemctl show kelvo-agent -p MainPID -p NRestarts \
                     -p MemoryCurrent -p MemoryPeak -p MemoryHigh -p MemoryMax \
                     -p MemorySwapMax -p MemorySwapCurrent; \
                     cat /sys/fs/cgroup/system.slice/kelvo-agent.service/memory.pressure; \
                     cat /sys/fs/cgroup/system.slice/kelvo-agent.service/memory.events; \
                     grep '^anon ' /sys/fs/cgroup/system.slice/kelvo-agent.service/memory.stat",
                )
                .await;
                next_check += Duration::from_secs(60);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        assert!(
            frames.len() > previous_count,
            "detail stopped advancing in the final soak interval"
        );
        println!(
            "detail soak complete: {} s, {} distinct measured frames",
            started.elapsed().as_secs(),
            frames.len()
        );
    }

    let now = kelvo_engine::wall_ms();
    let Answer::UsageByApp(usage) = call(Query::UsageByApp {
        from_ms: now - 600_000,
        to_ms: now,
        by: kelvo_schema::usage::UsageKey::Memory,
        limit: 5,
    })
    .await
    else {
        panic!("expected usage")
    };
    println!(
        "usage: covered {} s since {:?}",
        usage.covered_ms / 1000,
        usage.since_ms
    );
    for app in &usage.apps {
        println!(
            "  {:<24} mem avg {:>6} MB  cpu {:>5.1}%",
            app.name,
            app.mem_avg_b / 1_000_000,
            app.cpu_avg_pct
        );
    }
    assert!(usage.covered_ms > 0 && !usage.apps.is_empty());

    let Answer::SpaceVolumes(volumes) = call(Query::SpaceVolumes).await else {
        panic!("expected volumes")
    };
    for v in &volumes {
        println!(
            "volume {:<24} used {:>6.1} of {:>6.1} GB",
            v.mount,
            v.used_bytes / 1e9,
            v.total_bytes / 1e9
        );
    }
    assert_eq!(volumes.first().map(|v| v.mount.as_str()), Some("/"));
    memory().await;

    let started = std::time::Instant::now();
    let start = Query::SpaceStart {
        volume: "/".into(),
        include: Vec::new(),
    };
    assert!(matches!(call(start).await, Answer::SpaceStatus(_)));
    let mut anon_peak = 0u64;
    let mut frames = std::collections::BTreeSet::new();
    let status = loop {
        let Answer::SpaceStatus(status) = call(Query::SpaceStatus).await else {
            panic!("expected a status")
        };
        let anon = run_remote(
            &alias,
            "grep '^anon ' /sys/fs/cgroup/system.slice/kelvo-agent.service/memory.stat",
        )
        .await
        .unwrap_or_default();
        let anon = anon.split_whitespace().nth(1).and_then(|n| n.parse().ok());
        anon_peak = anon_peak.max(anon.unwrap_or(0));
        if let Some(frame) = hub.latest_frame()
            && frame.values.iter().any(|v| v.is_finite())
        {
            frames.insert(frame.ts_ms);
        }
        if status.phase != SpacePhase::Scanning || started.elapsed() > Duration::from_secs(900) {
            break status;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    println!(
        "scan: {:?} in {:.0} s, {} items, {:.1} GB measured",
        status.phase,
        started.elapsed().as_secs_f64(),
        status.items,
        status.measured_bytes / 1e9
    );
    println!(
        "agent anonymous memory peak during the scan: {} MB",
        anon_peak / 1_000_000
    );
    memory().await;
    assert_eq!(status.phase, SpacePhase::Done);
    assert!(
        frames.len() >= 2,
        "live detail must advance during the scan"
    );
    println!(
        "{} distinct measured live frames during the scan",
        frames.len()
    );
    // Shared extents (snapshots) must not count the same space many times over.
    assert!(
        status.measured_bytes <= status.used_bytes * 1.15,
        "measured {} of {} used",
        status.measured_bytes,
        status.used_bytes
    );
    let report_id = status.report_id.unwrap();
    let Answer::SpaceReport(Some(report)) = call(Query::SpaceReport).await else {
        panic!("expected the report")
    };
    println!(
        "report: {} unreadable, skipped {:?}",
        report.unreadable, report.skipped
    );
    let Answer::SpaceSubtree(root) = call(Query::SpaceSubtree {
        report: report_id,
        node: None,
    })
    .await
    else {
        panic!("expected the root")
    };
    for child in root.children.iter().take(8) {
        println!("  {:<24} {:>6.1} GB", child.name, child.bytes / 1e9);
    }
    let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"home") && names.contains(&"var"),
        "{names:?}"
    );
    assert!(
        !names.contains(&"proc") && !names.contains(&"boot"),
        "{names:?}"
    );
    let Answer::SpaceFiles(files) = call(Query::SpaceLargestFiles { report: report_id }).await
    else {
        panic!("expected files")
    };
    assert!(!files.is_empty());

    assert!(matches!(call(Query::SpaceClear).await, Answer::Done));
    let Answer::SpaceStatus(after) = call(Query::SpaceStatus).await else {
        panic!("expected a status")
    };
    assert_eq!(after.phase, SpacePhase::Idle);
    for cycle in 1..=3 {
        assert!(matches!(
            call(Query::SpaceStart {
                volume: "/".into(),
                include: Vec::new(),
            })
            .await,
            Answer::SpaceStatus(_)
        ));
        assert!(matches!(call(Query::SpaceCancel).await, Answer::Done));
        assert!(matches!(call(Query::SpaceClear).await, Answer::Done));
        let Answer::SpaceStatus(after) = call(Query::SpaceStatus).await else {
            panic!("expected a status")
        };
        assert_eq!(after.phase, SpacePhase::Idle);
        println!("scan/cancel/release cycle {cycle}: idle");
    }
    handle.stop();
    memory().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "installs the agent on a real host; see the module docs"]
async fn linux_agent_lifecycle_on_a_real_host() {
    let alias = std::env::var("KELVO_QA_ALIAS").unwrap_or_else(|_| "linux-tailscale".into());
    let path = std::env::var("KELVO_QA_AGENT").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!("agent {path}: {} bytes, sha256 {sha}", bytes.len());
    let outcome = tokio::spawn(lifecycle(alias.clone(), bytes, sha)).await;

    // Always leave the host clean.
    let report = agent_install::uninstall(&alias, true).await;
    println!("uninstall: {report:#?}");
    let left = sh(
        &alias,
        "ls -d /usr/local/bin/kelvo-agent /usr/local/bin/.kelvo-agent.prev /etc/systemd/system/kelvo-agent.service /var/lib/kelvo-agent /run/kelvo-agent; systemctl status kelvo-agent",
    )
    .await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic.into_panic());
    }
    assert!(report.is_clean(), "{report:?}");
    assert!(!left.contains("/kelvo-agent "), "{left}");
    assert_eq!(
        left.matches("No such file or directory").count(),
        5,
        "{left}"
    );
    assert!(left.contains("could not be found"), "{left}");
}
