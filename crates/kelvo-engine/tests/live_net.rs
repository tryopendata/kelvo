//! Live network attribution on this Mac (D-089), tray-only: the real engine with its real
//! collectors and a throwaway store, no window interest, while curl downloads. Checks
//! that a sustained download and one that exits between samples are both named "curl"
//! with curl's own byte count, that a range inside the last minute reads from the ring,
//! and that apps, other apps, overhead and system add up to the interface total.
//!
//! ```text
//! cargo test --release -p kelvo-engine --test live_net -- --ignored --nocapture
//! KELVO_LIVE_BROWSERS=1 ...   # also a Chrome and a Safari download (opened in the
//!                             # background; the downloaded files are deleted after)
//! KELVO_LIVE_URL=...          # curl's file, at least 150 MB (default: Hetzner's 1 GB test file)
//! KELVO_LIVE_BROWSER_URL=...  # the browsers' file, fetched whole (default: Hetzner's 100 MB)
//! ```
//!
//! Needs the network and takes about four minutes (seven with browsers). Keep other large
//! transfers off while it runs: the remainder split assumes curl dominates the interface.

#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use kelvo_engine::{
    EngineParts, LiveHub, LocalSource, NetAttribution, Source, SourceSink, attribute, wall_ms,
};
use kelvo_schema::{HostId, HostInfo, HostRecord, OsKind, Settings};
use kelvo_store::{Store, StoreConfig};

/// Longer than LEAD_MS + TAIL_MS apart, so one phase's padded range holds none of the
/// previous phase's bytes.
const QUIET: Duration = Duration::from_secs(35);
/// The tray-only network period is 10 s and a sample's bytes are split pro rata over its
/// interval, so a phase's bytes can land in the bucket before it starts and up to one
/// bucket after it ends.
const LEAD_MS: i64 = 10_000;
const TAIL_MS: i64 = 20_000;

fn record() -> HostRecord {
    HostRecord {
        id: HostId(uuid::Uuid::from_u128(0x6b65_6c76_6f2d_6c69_7665)),
        is_local: true,
        display_name: "live-net".into(),
        info: HostInfo {
            os: OsKind::MacOs,
            os_version: String::new(),
            model: None,
            chip: None,
            chip_known: false,
            cpu_topology: Vec::new(),
            mem_total_bytes: 0,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            boot_mounts: Vec::new(),
        },
    }
}

struct Live {
    host: HostId,
    hub: LiveHub,
    store: Store,
}

impl Live {
    fn read(&self, from_ms: i64, to_ms: i64) -> NetAttribution {
        let recent = self.hub.recent_net_buckets(from_ms, to_ms);
        let read = self
            .store
            .reader()
            .unwrap()
            .net_by_app_with(self.host, from_ms, to_ms, &recent)
            .unwrap();
        attribute(read)
    }
}

/// `curl` to /dev/null; returns (start ms, end ms, bytes curl received).
fn curl(args: &[&str], url: &str) -> (i64, i64, u64) {
    let t0 = wall_ms();
    let out = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-w", "%{size_download}"])
        .args(args)
        .arg(url)
        .output()
        .unwrap();
    let t1 = wall_ms();
    let bytes = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    (t0, t1, bytes)
}

fn report(label: &str, n: &NetAttribution) {
    let iface = n.rx.iface_bytes;
    println!(
        "[live] {label}: {} s, measured {} ms, iface rx {iface} B tx {} B, clamped {}",
        (n.to_ms - n.from_ms) / 1_000,
        n.measured_ms,
        n.tx.iface_bytes,
        n.clamped(),
    );
    for a in n.apps.iter().take(5) {
        println!(
            "[live]   {:<28} rx {:>12} tx {:>10}",
            a.name, a.rx_bytes, a.tx_bytes
        );
    }
    let pct = |b: u64| 100.0 * b as f64 / iface.max(1) as f64;
    println!(
        "[live]   rx: apps {} ({:.2}%) other apps {} overhead {} ({:.2}%) system {} ({:.2}%)",
        n.rx.attributed_bytes - n.other_apps_rx_bytes,
        pct(n.rx.attributed_bytes - n.other_apps_rx_bytes),
        n.other_apps_rx_bytes,
        n.rx.overhead_bytes,
        pct(n.rx.overhead_bytes),
        n.rx.system_bytes,
        pct(n.rx.system_bytes),
    );
}

/// The parts add up to the interface total in both directions.
fn assert_adds_up(n: &NetAttribution) {
    for d in [n.rx, n.tx] {
        if !d.clamped {
            assert_eq!(
                d.attributed_bytes + d.overhead_bytes + d.system_bytes,
                d.iface_bytes
            );
        }
    }
}

fn rx_of(n: &NetAttribution, name: &str) -> u64 {
    n.apps
        .iter()
        .find(|a| a.name == name)
        .map_or(0, |a| a.rx_bytes)
}

/// Opens `url` in `app` without bringing it forward, waits, then deletes what landed in
/// ~/Downloads under the URL's file name since `since`.
fn browser_download(live: &Live, app: &str, url: &str, wait: Duration) -> bool {
    std::thread::sleep(QUIET);
    let since = SystemTime::now();
    let t0 = wall_ms();
    let opened = Command::new("open")
        .args(["-g", "-a", app, url])
        .status()
        .unwrap();
    assert!(opened.success(), "open -a {app}");
    std::thread::sleep(wait);
    let t1 = wall_ms();
    std::thread::sleep(Duration::from_millis(TAIL_MS as u64));
    let n = live.read(t0 - LEAD_MS, t1 + TAIL_MS);
    report(app, &n);
    assert_adds_up(&n);

    let stem = url.rsplit('/').next().unwrap();
    let stem = stem.split('.').next().unwrap();
    let downloads = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Downloads");
    for e in std::fs::read_dir(&downloads).unwrap().flatten() {
        let fresh = e
            .metadata()
            .and_then(|m| m.created())
            .is_ok_and(|c| c >= since);
        if fresh && e.file_name().to_string_lossy().starts_with(stem) {
            println!("[live]   removing {}", e.path().display());
            // Safari's partial download is a `.download` bundle.
            let _ = std::fs::remove_file(e.path()).or_else(|_| std::fs::remove_dir_all(e.path()));
        }
    }
    top(&n) == Some(app)
}

fn top(n: &NetAttribution) -> Option<&str> {
    n.apps.first().map(|a| a.name.as_str())
}

fn check(fails: &mut Vec<String>, ok: bool, what: &str) {
    if !ok {
        println!("[live] FAIL {what}");
        fails.push(what.to_owned());
    }
}

/// The ring's 10 s buckets over a range: measured span, interface and `name`'s rx.
fn buckets(live: &Live, from_ms: i64, to_ms: i64, name: &str) {
    for (t, b) in live.hub.recent_net_buckets(from_ms, to_ms) {
        let app: u64 = b
            .apps
            .iter()
            .filter(|a| a.name.as_deref() == Some(name))
            .map(|a| a.rx_bytes)
            .sum();
        println!(
            "[live]     bucket {:+6} s measured {:>5} ms iface rx {:>11} {name} rx {:>11}",
            (t - from_ms) / 1_000,
            b.measured_ms,
            b.iface_rx_bytes,
            app
        );
    }
}

#[test]
#[ignore = "live: real collectors, the network, about four minutes"]
fn live_network_attribution() {
    let url = std::env::var("KELVO_LIVE_URL")
        .unwrap_or_else(|_| "https://ash-speed.hetzner.com/1GB.bin".into());
    let dir = std::env::temp_dir().join(format!("kelvo-live-net-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(StoreConfig::new(dir.join("history.sqlite"))).unwrap();
    let rec = record();
    let host = rec.id;
    let writer = store.writer();
    writer.upsert_host(rec.clone()).unwrap();
    writer.flush().unwrap();

    let settings = Settings::default();
    assert!(settings.history.network_history);
    let parts = EngineParts::platform(Arc::new(kelvo_engine::NoScaleStore));
    let source = Arc::new(LocalSource::new(rec, parts, settings));
    let hub = LiveHub::new(kelvo_engine::Bus::default());
    // Tray-only: no interest registered, as with every window closed.
    let mut handle = Arc::clone(&source)
        .start(SourceSink {
            live: hub.clone(),
            store: Some(writer),
        })
        .unwrap();
    let live = Live { host, hub, store };
    let started = Instant::now();

    // (a) A sustained 5 MB/s download for 30 s.
    std::thread::sleep(QUIET);
    let (a0, a1, a_bytes) = curl(&["--limit-rate", "5M", "--max-time", "30"], &url);
    std::thread::sleep(Duration::from_millis(TAIL_MS as u64));
    let a = live.read(a0 - LEAD_MS, a1 + TAIL_MS);
    report("a: curl 5 MB/s 30 s", &a);
    let a_curl = rx_of(&a, "curl");
    let a_err = (a_curl as f64 - a_bytes as f64) / a_bytes as f64 * 100.0;
    println!(
        "[live]   curl says {a_bytes} B; attributed {a_curl} B ({a_err:+.2}%); overhead share {:.2}% of iface",
        100.0 * a.rx.overhead_bytes as f64 / a.rx.iface_bytes.max(1) as f64
    );
    buckets(&live, a0 - LEAD_MS, a1 + TAIL_MS, "curl");
    let mut fails = Vec::new();
    check(
        &mut fails,
        top(&a) == Some("curl"),
        "a: curl is the top app",
    );
    check(
        &mut fails,
        a_err.abs() < 5.0,
        "a: within 5% of curl's count",
    );
    assert_adds_up(&a);

    // (b) A short download that exits between two 10 s samples.
    std::thread::sleep(QUIET);
    let (b0, b1, b_bytes) = curl(&["-r", "0-3999999"], &url);
    println!("[live] b: curl took {} ms for {b_bytes} B", b1 - b0);
    std::thread::sleep(Duration::from_millis(TAIL_MS as u64));
    let b = live.read(b0 - LEAD_MS, b1 + TAIL_MS);
    report("b: short curl", &b);
    let b_curl = rx_of(&b, "curl");
    let b_err = (b_curl as f64 - b_bytes as f64) / b_bytes as f64 * 100.0;
    println!("[live]   curl says {b_bytes} B; attributed {b_curl} B ({b_err:+.2}%)");
    buckets(&live, b0 - LEAD_MS, b1 + TAIL_MS, "curl");
    check(
        &mut fails,
        b1 - b0 < 10_000,
        "b: the short download took under 10 s",
    );
    check(
        &mut fails,
        b_err.abs() < 5.0,
        "b: the exited curl keeps its name and bytes",
    );
    assert_adds_up(&b);

    // (e) The last minute is answered, from the ring (nothing is committed for 5 min).
    let now = wall_ms();
    let e = live.read(now - 60_000, now);
    let ring = live.hub.recent_net_buckets(now - 60_000, now);
    let stored = live
        .store
        .reader()
        .unwrap()
        .net_by_app(host, now - 60_000, now)
        .unwrap();
    println!(
        "[live] e: last 60 s: {} ring buckets, {} apps, iface rx {} B; store alone: {} apps, measured {} ms",
        ring.len(),
        e.apps.len(),
        e.rx.iface_bytes,
        stored.apps.len(),
        stored.measured_ms
    );
    check(
        &mut fails,
        !ring.is_empty() && !e.apps.is_empty() && e.measured_ms > 0,
        "e: the last minute is answered",
    );

    if std::env::var_os("KELVO_LIVE_BROWSERS").is_some() {
        let file = std::env::var("KELVO_LIVE_BROWSER_URL")
            .unwrap_or_else(|_| "https://ash-speed.hetzner.com/100MB.bin".into());
        for app in ["Google Chrome", "Safari"] {
            let ok = browser_download(&live, app, &file, Duration::from_secs(45));
            check(&mut fails, ok, &format!("{app}'s download is the top app"));
        }
    }

    // The whole run, after shutdown: committed rows alone match what the ring answered.
    let whole = (a0 - QUIET.as_millis() as i64, wall_ms());
    let before = live.read(whole.0, whole.1);
    if let Some(ctl) = source.engine() {
        ctl.shutdown();
    }
    handle.stop();
    let after = attribute(
        live.store
            .reader()
            .unwrap()
            .net_by_app(host, whole.0, whole.1)
            .unwrap(),
    );
    report("whole run, ring + store", &before);
    report("whole run, store after shutdown", &after);
    println!("[live] ran {} s", started.elapsed().as_secs());
    assert_adds_up(&after);
    check(
        &mut fails,
        rx_of(&after, "curl") == rx_of(&before, "curl"),
        "the committed rows match the ring",
    );
    let Live { store, .. } = live;
    store.close().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(fails.is_empty(), "{fails:#?}");
}
