//! Local disk I/O views. Request times describe completed driver requests, not utilization.
use crate::{Gap, HostId, JsSafeInt, Tier};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct DiskIoDevice {
    pub dev: String,
    pub name: String,
    pub internal: Option<bool>,
    pub request_time_supported: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct DiskIoDevices {
    pub devices: Vec<DiskIoDevice>,
    pub default_device: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, specta::Type)]
pub struct DiskIoRequest {
    pub host: HostId,
    /// None selects all devices; otherwise an exact BSD device name.
    pub device: Option<String>,
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    pub max_points: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct DiskIoPoint {
    #[specta(type = JsSafeInt)]
    pub t: i64,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    pub read_ops_per_sec: Option<f64>,
    pub write_ops_per_sec: Option<f64>,
    pub read_request_ms: Option<f64>,
    pub write_request_ms: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct DiskIoPage {
    /// Summary range widened to source slots, clipped at the host's current time.
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    pub tier: Tier,
    #[specta(type = JsSafeInt)]
    pub bucket_ms: i64,
    pub points: Vec<DiskIoPoint>,
    pub gaps: Vec<Gap>,
    /// Covered throughput integrals for the requested range, clipped at now.
    pub read_bytes: Option<f64>,
    pub write_bytes: Option<f64>,
    #[specta(type = JsSafeInt)]
    pub read_covered_ms: i64,
    #[specta(type = JsSafeInt)]
    pub write_covered_ms: i64,
}
/// Only these new internal rates normalize their integral over a whole persisted
/// bucket (D-110). Ordinary rates retain their measured-span mean.
pub fn preserves_request_integral(metric: &str) -> bool {
    matches!(
        metric,
        "disk.read_timed_ops"
            | "disk.write_timed_ops"
            | "disk.read_time"
            | "disk.write_time"
            | "disk.read_timed_ops_total"
            | "disk.write_timed_ops_total"
            | "disk.read_time_total"
            | "disk.write_time_total"
    )
}
/// Numerator and denominator must describe the same valid completed-request spans.
pub fn request_ms(time_ms_per_sec: Option<f64>, timed_ops_per_sec: Option<f64>) -> Option<f64> {
    let time = time_ms_per_sec.filter(|v| v.is_finite() && *v >= 0.)?;
    let ops = timed_ops_per_sec.filter(|v| v.is_finite() && *v > 0.)?;
    Some(time / ops)
}

/// Authoritative sums over the collector population at each observation.
pub const TOTAL_METRICS: [&str; 8] = [
    "disk.read_total",
    "disk.write_total",
    "disk.read_ops_total",
    "disk.write_ops_total",
    "disk.read_timed_ops_total",
    "disk.write_timed_ops_total",
    "disk.read_time_total",
    "disk.write_time_total",
];
