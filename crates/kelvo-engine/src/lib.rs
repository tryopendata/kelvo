//! The sampler: the loop behind the [`Ticker`] and [`PowerSignals`] traits, the S10/M1
//! rollup accumulators, the bus with its per-host [`LiveHub`] (one-hour ring buffer,
//! latest frame), the [`Source`] abstraction, and the event [`detect`]ors and alert
//! rules (v1.2).
//!
//! Wires `kelvo-collect` to `kelvo-store`. Knows nothing about Tauri or windows.
//!
//! # Shape
//!
//! - [`Engine`] owns the collectors and runs on one thread at utility QoS. Everything it
//!   reacts to (ticks, sleep/wake, device hints, commands) arrives on one queue, its
//!   [`Inbox`], and is handled in order.
//! - [`Ticker`]: GCD `DispatchSource` timer on macOS ([`macos::GcdTicker`]), a sleeping
//!   thread elsewhere ([`ThreadTicker`]), [`FakeTicker`] in tests.
//! - [`PowerSignals`]: IOKit system power plus polled battery, Low Power Mode, display and
//!   lock state on macOS ([`macos::MacPowerSignals`]); [`FakePowerSignals`] in tests.
//! - [`DeviceHints`]: IOKit match notifications for disks and network interfaces, which
//!   trigger a re-probe and, if anything changed, a new layout and capabilities revision.
//! - [`Bus`]: `tokio::sync::broadcast` per host. Publishing never blocks; a lagging
//!   [`Subscriber`] drops the oldest messages and can report it ([`Recv::Lagged`]).
//! - [`LiveHub`]: a host's bus plus what late joiners need (ring, latest frame, layout,
//!   status). Sources publish through it, so any source's output backfills windows.
//! - [`LocalSource`] wraps the engine as a [`Source`].
//! - [`select_processes`] cuts a process batch to a consumer's [`ProcessView`].
//! - [`attribute`] splits a range's per-app network bytes into apps, overhead and the
//!   "System and other" remainder (D-089).
//! - [`identity`]: the local host id, its `host-id` file and its machine binding (D-071).
//! - [`Housekeeping`]: the history pruning schedule, the low-disk check and the
//!   [`HistoryHealth`] they produce, on a thread of their own.
//!
//! # Frames, held values and the Snapshot
//!
//! Collectors push values only on their own cadence, a wall-clock minimum period
//! (D-061): disk capacity every 60 s, load average every 5 s, IOReport every 10 s while
//! only the tray is open, PMP power only when its counters move (D-043). A [`LiveFrame`]
//! therefore has two value arrays:
//!
//! - `values`: what was measured on this tick, `NaN` for everything else. The ring buffer,
//!   backfill and the rollups read this, so a held value is never mistaken for a
//!   measurement and history never repeats a reading.
//! - `held`: a latest-value cache. Each value stays current for 2.5 times its sampling
//!   period (the largest of the catalog period, its collector's current period and the
//!   base tick) and is `NaN` after that. [`LiveFrame::snapshot`] builds the typed
//!   `Snapshot` from it, so slow series show on every frame while a series that stops
//!   arriving still turns into a gap.
//!
//! # Gaps the engine writes
//!
//! `sleep` (WillSleep to DidWake, end measured on the continuous clock; also a stall of
//! more than 5 ticks with no sleep event), `paused`, and `module_disabled` with its module
//! (Settings has one switch for Power and Sensors, so turning it off writes one gap for
//! each). Open gaps are closed on shutdown.

mod accum;
mod bus;
mod clock;
pub mod detect;
mod engine;
mod hints;
mod housekeeping;
pub mod identity;
mod inbox;
mod live;
#[cfg(target_os = "macos")]
pub mod macos;
mod netacc;
mod netview;
mod power;
mod procview;
mod ring;
mod source;
mod ticker;
mod usage;

pub use accum::RECENT_ROWS_MS;
pub use bus::{
    BUS_CAPACITY, Bus, BusMsg, EngineStatus, FrameLayout, LiveFrame, ProcessBatch, Recv, Subscriber,
};
pub use clock::{ClockReading, continuous_ns, now, wall_ms};
pub use engine::{
    BACKGROUND_TICK_MS, Engine, EngineControl, EngineHandle, EngineParts, EngineSettings,
    PERFORMANCE_IDLE_PROCESS_MS, PERFORMANCE_IDLE_SENSOR_MS, PERFORMANCE_VISIBLE_MS,
    PerformanceSlowdown, STALE_DEN, STALE_NUM, background_interval_ms, backoff_interval_ms,
    effective_interval_ms, hold_ms, performance_period,
};
pub use hints::DeviceHints;
pub use housekeeping::{
    HistoryHealth, Housekeeping, HousekeepingHandle, Schedule, next_health, retention_for,
};
pub use inbox::{Inbox, SleepAck};
/// The rows of a [`ProcessBatch`], re-exported for the same reason.
pub use kelvo_collect::ProcessSample;
/// Re-exported so engine users need not depend on `kelvo-collect` for it (D-044).
pub use kelvo_collect::Tick;
/// Where learned CPU power scales persist; the app shell implements it (D-065).
pub use kelvo_collect::calib::{NoScaleStore, ScaleStore};
/// Quit and Force Quit's OS side, for the app shell's `process_signal` (D-029, D-065).
pub use kelvo_collect::process_control;
/// Collector periods the app shell's sampling plans quote (D-092).
pub use kelvo_collect::{IDLE_MS, TEMPERATURE_PERIOD_MS};
/// The Network page's address line reads the primary interface's addresses.
pub use kelvo_collect::{IfaceAddrs, egress_interface, interface_addresses};
pub use live::{LiveHub, RecentNet, WARM_LAYOUT_NO};
pub use netacc::{NET_BUCKET_MS, NET_RING_BUCKETS};
pub use netview::{
    AppBytes, CLAMP_SLACK_BYTES, DirectionSplit, HEADER_BYTES_PER_PACKET, NetAttribution,
    attribute, split_direction,
};
pub use power::{
    FakePower, FakePowerSignals, NoPowerSignals, PowerEvent, PowerSignals, PowerState,
};
pub use procview::{ProcessSort, ProcessView, select_processes};
pub use ring::{BackfillSegment, RING_MAX_ROWS, RING_SPAN_MS, Ring};
pub use source::{
    DETAIL_WAIT, LocalSource, SET_STORE_WAIT, Source, SourceControl, SourceError, SourceHandle,
    SourceSink, network_history_enabled,
};
pub use ticker::{FakeClock, FakeTicker, ThreadTicker, Ticker, leeway_for};
pub use usage::{USAGE_BUCKET_MS, UsageApp, UsageByApp, UsageKey, UsageProc, UsageTotal};
