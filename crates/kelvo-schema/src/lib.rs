//! Kelvo's data vocabulary: the metric catalog, series keys (`metric_id` plus labels),
//! host identity, capabilities, alert-rule data, settings and the typed `Snapshot` view.
//!
//! Pure data. No I/O, no tokio, no OS APIs; every other crate depends on this one.
//!
//! # Map for downstream crates
//!
//! | Need | Type |
//! |---|---|
//! | Identify a series | [`SeriesKey`] = [`MetricId`] + [`Labels`]; `Display`/[`SeriesKey::parse`] for text, [`Labels::canonical`] for the store's `series.labels` column |
//! | Know what a metric is | [`Catalog::builtin`], [`Catalog::validate`], [`MetricDef`] (`persisted` decides tier storage) |
//! | Identify a host | [`HostId`] (UUID), [`HostRecord`] (controller side, with `is_local`), [`HostIdentity`] (on the wire), [`HostInfo`] |
//! | Tiers and sync | [`Tier`] (`bucket_ms`, `bucket_start`, `PERSISTED`), [`Cursor`] (`epoch` + `seq`), [`SyncRowKind`]/[`SyncKinds`] (row kinds gated on negotiated features) |
//! | Gaps | [`Gap`], [`GapReason`] (`as_str`/`parse` give the `gaps.reason` text), nullable [`Gap::module`] set only for `module_disabled`, [`Module::as_str`] for `gaps.module` |
//! | Capabilities | [`Capabilities`], [`ModuleCap`], [`UnsupportedReason`], [`Entitlement`] |
//! | Settings | [`Settings`] with [`Settings::validate`] |
//! | Alerts (v1.2) | [`AlertRule`] (built-ins [`AlertRule::hot_process`], [`AlertRule::thermal_serious`]), [`Condition`], [`SeriesSelector`], [`AlertSettings`] |
//! | Events (v1.2) | [`Event`], [`EventDetail`] (`kind` gives the `events.kind` text), [`DetectorThresholds::DEFAULT`] |
//! | UI view | [`Snapshot::from_frame`] over a [`FrameView`] |
//! | Forward compatibility | Wire-facing and stored enums have an `Unknown` variant for values from newer peers or builds; never produced locally, skipped by consumers (D-040) |
//!
//! The store's interned IDs (`HostRef`, `SeriesId`, `LayoutId`, `Layout`) are not here:
//! they never leave the database and live in `kelvo-store`. The wire form of a layout
//! (`WireLayout`) lives in `kelvo-proto`.
//!
//! # Integers that cross to JavaScript
//!
//! Millisecond timestamps and `seq` are `i64` in Rust. They cross the Tauri bridge as JSON
//! numbers, which JavaScript reads as `f64` and represents exactly only up to
//! [`JS_SAFE_INT_MAX`] (2^53 - 1). Both stay far below it: a millisecond epoch reaches 2^53
//! in the year 287,396, and `seq` would need about 1.4 million persisted rows per second
//! for 200 years (Kelvo writes well under one per second). A test pins both bounds.
//!
//! specta forbids exporting `i64`/`u64` by default for exactly this reason, so every such
//! field that is exported carries `#[specta(type = crate::JsSafeInt)]` (TS `number`), next
//! to a doc comment pointing here. Values that are not timestamps, `seq`, revisions or
//! byte counts must use a 32-bit or smaller type instead.

mod alert;
mod caps;
mod catalog;
mod codes;
mod compat;
mod event;
mod history;
mod host;
mod series;
pub mod settings;
mod snapshot;

pub use alert::{AlertRule, Cmp, Condition, ThermalState};
pub use caps::{Capabilities, Entitlement, ModuleCap, UnsupportedReason};
pub use catalog::{CATALOG, Catalog, CatalogError, MetricDef, MetricKind, Module, Unit};
pub use codes::{CpuPowerCalibration, MetricCode, PressureLevel, metric_codes};
pub use event::{AlertCause, DetectorThresholds, Event, EventDetail, PowerComponent};
pub use history::{Cursor, Gap, GapError, GapReason, SyncKinds, SyncRowKind, Tier};
pub use host::{ClusterInfo, CoreKind, HostId, HostIdentity, HostInfo, HostRecord, OsKind};
pub use series::{Labels, MetricId, SeriesKey, SeriesParseError, SeriesSelector};
pub use settings::{AlertSettings, PerformanceReason, PowerSource, Settings, SettingsError};
pub use snapshot::{
    BatteryView, ClusterView, CoreView, CpuView, DiskDeviceView, DiskView, FanView,
    FrameLenMismatch, FrameView, GpuView, InterfaceView, LabeledValue, MemoryView, NetworkView,
    PowerView, SensorsView, Snapshot, VolumeView,
};

/// Crate name. The phase 0 placeholder tests in `kelvo-store`, `kelvo-collect` and
/// `kelvo-engine` link against it; remove it once those crates use real types.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

/// The largest integer JavaScript represents exactly (`Number.MAX_SAFE_INTEGER`, 2^53 - 1).
/// Every `i64` timestamp and `seq` sent to the webview stays below it; see the crate docs.
pub const JS_SAFE_INT_MAX: i64 = (1 << 53) - 1;

/// specta stand-in for an `i64`/`u64` field bounded by [`JS_SAFE_INT_MAX`]: exports as a
/// plain TS `number`. Use it as `#[specta(type = crate::JsSafeInt)]`, never as a runtime
/// type. (`f64` would not do: specta exports it as `number | null` because of NaN.)
pub struct JsSafeInt;

impl specta::Type for JsSafeInt {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <i32 as specta::Type>::definition(types)
    }
}

/// Whether `v` survives a round trip through a JavaScript `number`.
pub const fn is_js_safe(v: i64) -> bool {
    -JS_SAFE_INT_MAX <= v && v <= JS_SAFE_INT_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    // Julian year: 365.25 days.
    const MS_PER_YEAR: i64 = 36_525 * 24 * 3_600 * 1_000 / 100;

    #[test]
    fn timestamps_and_seq_stay_js_safe_for_200_years() {
        // Millisecond epoch in the year 2226, 200 years past the first release.
        let ts_2226 = (2226 - 1970) * MS_PER_YEAR;
        assert!(is_js_safe(ts_2226));
        assert!(
            ts_2226 < JS_SAFE_INT_MAX / 1_000,
            "three orders of magnitude of headroom"
        );

        // seq: one per persisted row. Real rate is under 1/s (a few tier rows per minute,
        // a process snapshot every 10 s). Assume 10,000 rows/s for 200 years anyway.
        let seconds_200y = 200 * MS_PER_YEAR / 1_000;
        let seq_200y = 10_000 * seconds_200y;
        assert!(is_js_safe(seq_200y));

        assert!(is_js_safe(JS_SAFE_INT_MAX));
        assert!(!is_js_safe(JS_SAFE_INT_MAX + 1));
    }
}
