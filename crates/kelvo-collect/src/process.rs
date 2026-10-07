//! The process row (v1-local-monitor.md 6.2). Processes are not series: each sample is a
//! row the engine forwards to the bus and the store's `proc_snap`.

use std::sync::Arc;

/// One process at one sample. A process is identified by `(pid, start_time_us)`, since
/// pids are reused.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessSample {
    pub pid: i32,
    /// Process start, microseconds since the Unix epoch.
    pub start_time_us: i64,
    /// Shared with the collector's per-process cache, so a row costs no string copy
    /// (D-062: hundreds of rows per tick while the process table is open).
    pub name: Arc<str>,
    /// Percent of one core over the interval since the previous sample; can exceed 100.
    pub cpu_pct: f32,
    /// Physical footprint (what Activity Monitor calls Memory).
    pub mem_bytes: u64,
    /// Compressed memory attributed to the process. `None` when the platform does not say.
    pub compressed_bytes: Option<u64>,
    pub threads: u32,
    /// Package idle wakeups per second over the interval.
    pub idle_wakeups_per_s: f32,
    /// Approximate energy impact over the interval; see the platform collector for the
    /// formula. Comparable between processes on one host, not an Activity Monitor match.
    pub energy: f32,
    /// Joules over the interval since the previous sample: `energy` times the interval.
    /// Summing it over every sample of a range gives the process's energy in that range,
    /// whatever the sampling cadence was.
    pub energy_j: f32,
    /// The app the process belongs to, by the identity rule per-app network uses
    /// (D-089): "Google Chrome" for its helpers, "Safari" for WebKit's XPC services,
    /// `node` for a CLI. Resolved once per process. `None` when its path could not be
    /// read.
    pub app: Option<Arc<str>>,
    /// The process is its app bundle's main executable (`X.app/Contents/MacOS/…`, no
    /// nested bundle): quitting it quits the app.
    pub app_main: bool,
    pub disk_read_bps: f32,
    pub disk_write_bps: f32,
    /// Bytes per second received over the interval, loopback excluded (D-081). `None`
    /// when per-process network was not sampled for this row: nobody showing it, or the
    /// API is unavailable. `Some(0.0)` for a process with no traffic.
    pub net_rx_bps: Option<f32>,
    /// Bytes per second sent; see `net_rx_bps`.
    pub net_tx_bps: Option<f32>,
    /// Share of the whole GPU over the interval, in percent (0 to 100): GPU time the
    /// process's command buffers used divided by wall time. `None` when per-process GPU
    /// was not sampled for this row; `Some(0.0)` for a process that used none.
    pub gpu_pct: Option<f32>,
    /// TCP ports the process listens on, ascending. `None` when ports were not read for
    /// this row (no visible window shows them); empty when it listens on none.
    pub ports: Option<Arc<[u16]>>,
    /// Owning user name, or the numeric uid when it has no name.
    pub user: Arc<str>,
}

/// One process's network traffic over the batch's interval
/// ([`crate::SampleBuf::process_net_interval`]), from the per-process network collector
/// (D-081, D-089). The engine joins the rates to [`ProcessSample`] rows by pid (a
/// process absent from the batch had no traffic) and sums the bytes by `identity` for
/// network history.
///
/// A batch has at most one entry per `(pid, identity)`. The pid is the one the flows
/// belonged to: a process that exited during the interval keeps its entry, so its
/// bytes still count for its identity, and the rate join simply finds no row for it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessNet {
    pub pid: i32,
    /// The app the bytes are charged to ("Google Chrome" for its helpers, "Safari" for
    /// WebKit's networking service, "git" for `git-remote-https`), resolved once per
    /// process while it was alive. Interned: every entry of one app shares one
    /// allocation. `None` when nothing named it, or past the cap on new names per hour.
    pub identity: Option<Arc<str>>,
    /// Bytes received over the interval, loopback excluded.
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// `rx_bytes` over the interval's length.
    pub rx_bps: f32,
    pub tx_bps: f32,
    /// Bytes moved before the interval by flows that opened after the collector's
    /// baseline, while their owner was unknown, charged now that it is known (D-089).
    /// For history only: not in `rx_bytes` or the rates.
    pub late_rx_bytes: u64,
    pub late_tx_bytes: u64,
}

/// One process's share of the GPU over the interval since the previous sample, from the
/// per-process GPU collector. The engine joins it to [`ProcessSample`] rows by pid; a
/// process absent from the batch used no GPU time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessGpu {
    pub pid: i32,
    /// Percent of the whole GPU, 0 to 100.
    pub pct: f32,
}
