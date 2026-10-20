//! Read-only Docker worker/store diagnostic. Isolated temporary history, no user database.
//! cargo run -p kelvo-engine --example docker_diagnostic -- 60 visible
//! For a real 24-hour run use `86400 background`; compare with `86400 off`.
//! Reports only counts, resource costs and latency; never names, IDs or log content.
use anyhow::{Context, bail};
use kelvo_engine::docker::DockerHandle;
use kelvo_schema::{HostId, HostInfo, HostRecord, OsKind, docker::*};
use kelvo_store::{Store, StoreConfig};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[tokio::main(flavor = "current_thread")]
#[expect(
    clippy::too_many_lines,
    reason = "split when substantially changed; architecture refactor section 10"
)]
async fn main() -> anyhow::Result<()> {
    let seconds = std::env::args()
        .nth(1)
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(60);
    if !(1..=86400).contains(&seconds) {
        bail!("Duration must be 1–86400 seconds");
    }
    let mode = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "background".into());
    if !matches!(mode.as_str(), "visible" | "background" | "off") {
        bail!("Mode must be visible, background or off");
    }
    let directory = tempfile::Builder::new()
        .prefix("kelvo-docker-diagnostic-")
        .tempdir()?;
    let path = directory.path().join("history.sqlite");
    let store = Store::open(StoreConfig::new(&path))?;
    let writer = store.writer();
    let host = HostId(uuid::Uuid::new_v4());
    writer.upsert_host(HostRecord {
        id: host,
        is_local: true,
        display_name: "Isolated diagnostic host".into(),
        info: HostInfo {
            os: if cfg!(target_os = "macos") {
                OsKind::MacOs
            } else {
                OsKind::Linux
            },
            os_version: String::new(),
            model: None,
            chip: None,
            chip_known: false,
            cpu_topology: Vec::new(),
            mem_total_bytes: 0,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            kernel: None,
            boot_mounts: Vec::new(),
        },
    })?;
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME unavailable")?);
    let handle = DockerHandle::new(host, DockerSettings::default(), home);
    handle.recorder.attach(writer.clone(), path.clone());
    if mode != "off" {
        let engines = handle.discover().await;
        let reachable: Vec<_> = engines.iter().filter(|e| e.reachable).collect();
        let engine = reachable
            .iter()
            .find(|e| e.preferred)
            .copied()
            .or_else(|| (reachable.len() == 1).then(|| reachable[0]))
            .context("No single reachable local engine; select an active Unix context")?;
        handle.configure(DockerSettings {
            enabled: true,
            selected_engine: Some(engine.socket.clone()),
            ..Default::default()
        })?;
    }
    handle.set_visible(mode == "visible");
    let task = tokio::spawn(handle.clone().run());
    eprintln!(
        "Read-only headless diagnostic, {seconds}s, {mode}. Temporary isolated history; process CPU excludes WebKit and daemon/VM. A short run is not a 24-hour soak."
    );
    let start = Instant::now();
    let mut last_cost = cost();
    let mut last_report = start;
    for elapsed in 1..=seconds {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if elapsed % 10 != 0 && elapsed != seconds {
            continue;
        }
        let snapshot = handle.snapshot();
        let mut query_ms = None;
        if let Some(engine) = snapshot.engine {
            let h = handle.clone();
            let now = kelvo_engine::wall_ms();
            let query = DockerHistoryRequest {
                engine_id: engine.id,
                container_id: None,
                from_ms: now - 900000,
                to_ms: now + 1,
                max_points: 1000,
            };
            let before = Instant::now();
            let result = tokio::task::spawn_blocking(move || h.history(&query)).await?;
            query_ms = Some(before.elapsed().as_secs_f64() * 1000.);
            if result.is_err() {
                eprintln!("Bounded history query unavailable");
            }
        }
        let now = Instant::now();
        let current_cost = cost();
        let stats = writer.docker_storage_status();
        println!(
            "{}",
            serde_json::json!({"elapsed_s":start.elapsed().as_secs(),"status":snapshot.status,"running":snapshot.aggregate.running,"measured_cpu":snapshot.aggregate.cpu_measured,"expected":snapshot.aggregate.expected,"partial":snapshot.aggregate.partial,"source_interval_ms":snapshot.interval_ms,"process_cpu_pct":(current_cost.0-last_cost.0)/now.duration_since(last_report).as_secs_f64()*100.,"peak_rss_bytes":current_cost.1,"logical_bytes":stats.logical_bytes,"allowance_bytes":stats.allowance_bytes,"pending_writes":stats.pending_writes,"pending_bytes":stats.pending_bytes,"persistence_paused":stats.paused,"file_bytes":std::fs::metadata(&path).map(|m|m.len()).unwrap_or(0),"wal_bytes":std::fs::metadata(path.with_file_name("history.sqlite-wal")).map(|m|m.len()).unwrap_or(0),"history_query_ms":query_ms})
        );
        last_cost = current_cost;
        last_report = now;
    }
    handle.shutdown();
    task.await?;
    writer.flush()?;
    handle.recorder.detach();
    drop(handle);
    drop(writer);
    store.close()?;
    Ok(())
}
fn cost() -> (f64, u64) {
    // SAFETY: getrusage initializes the writable rusage for this process only.
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    // SAFETY: usage points to valid writable storage and RUSAGE_SELF has no external target.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return (0., 0);
    }
    let seconds = usage.ru_utime.tv_sec as f64
        + usage.ru_stime.tv_sec as f64
        + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.;
    let rss = usage.ru_maxrss.max(0) as u64;
    (
        seconds,
        if cfg!(target_os = "macos") {
            rss
        } else {
            rss.saturating_mul(1024)
        },
    )
}
