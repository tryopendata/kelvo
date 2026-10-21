//! Per-app use over a range, from a host's in-memory usage ring (D-093, D-099): the
//! engine's answer to a usage query, and the wire's (D-117). Not metric series and never
//! stored. Every field has a default, so a field added later decodes from an older host.
use serde::{Deserialize, Serialize};

// Also what the usage ring sorts by. The doc comment is the webview's, since the shell
// takes this over IPC as it is (D-100).
/// What `query_usage_by_app` sorts apps by (D-099).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UsageKey {
    /// Average CPU.
    Cpu,
    /// Average GPU.
    Gpu,
    /// Peak footprint.
    Memory,
    /// Bytes read plus written.
    Disk,
    /// Joules.
    Energy,
}

/// One process's use over a range.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageProc {
    pub pid: i32,
    pub start_time_us: i64,
    pub name: String,
    /// Average percent of one core over the range's covered time.
    pub cpu_avg_pct: f64,
    /// Average percent of the GPU over the time GPU was measured; `None` when it never was.
    pub gpu_avg_pct: Option<f64>,
    pub mem_peak_b: u64,
    pub read_b: f64,
    pub write_b: f64,
    pub energy_j: f64,
    pub avg_w: f64,
    /// Listed by the newest batch: still running as of then.
    pub running: bool,
    /// Its app bundle's main executable.
    pub app_main: bool,
}

/// One app's use over a range.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageApp {
    /// The app identity (D-089), or the process name when none was resolved.
    pub name: String,
    pub cpu_avg_pct: f64,
    pub gpu_avg_pct: Option<f64>,
    /// Largest footprint of its processes summed at one sample.
    pub mem_peak_b: u64,
    /// Average footprint while it was running.
    pub mem_avg_b: u64,
    pub read_b: f64,
    pub write_b: f64,
    pub energy_j: f64,
    pub avg_w: f64,
    /// Kept processes, ordered like the apps.
    pub processes: Vec<UsageProc>,
}

/// Every process's use over a range, before any floor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageTotal {
    pub cpu_avg_pct: f64,
    pub gpu_avg_pct: Option<f64>,
    pub read_b: f64,
    pub write_b: f64,
    pub energy_j: f64,
    pub avg_w: f64,
}

/// Per-app use over a range.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageByApp {
    /// The range the sums cover: the request widened to whole buckets.
    pub from_ms: i64,
    pub to_ms: i64,
    /// When the ring started counting, `None` before any batch. A range starting earlier
    /// was only measured from here.
    pub since_ms: Option<i64>,
    /// Buckets ending at or before this are final: the next sample's interval starts
    /// at the newest one, so it charges only later time. `None` before any batch.
    pub complete_to_ms: Option<i64>,
    /// The newest batch: no process time after it is counted yet.
    pub latest_ms: Option<i64>,
    /// Time inside the range a process sample covered, ms. Averages divide by this; 0
    /// means nothing was measured, not zero use.
    pub covered_ms: i64,
    /// The part of `covered_ms` whose samples measured GPU.
    pub gpu_covered_ms: i64,
    pub total: UsageTotal,
    /// The largest `limit` by the requested key, largest first.
    pub apps: Vec<UsageApp>,
}
