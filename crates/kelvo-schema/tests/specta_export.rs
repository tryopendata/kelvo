#![allow(clippy::too_many_lines)] // Integration scenarios keep setup and assertions together.
//! Exports every IPC-facing type with the same exporter and serde format tauri-specta
//! uses (default `Typescript`, `specta_serde::PhasesFormat`). A type the app shell could
//! not export (an unannotated `i64`, a serde attribute specta rejects) fails here.

use kelvo_schema::durable_usage::{
    HistoricalUsageReply, HistoricalUsageRequest, UsageProcessReply, UsageProcessRequest,
};
use kelvo_schema::{
    AlertRule, Capabilities, Cursor, Event, Gap, HostId, HostRecord, Module, SeriesKey,
    SeriesSelector, Settings, Snapshot, Tier,
};
use specta::Types;
use specta_typescript::Typescript;

#[test]
fn ipc_types_export_to_typescript() {
    let types = Types::default()
        .register::<HostId>()
        .register::<HostRecord>()
        .register::<Capabilities>()
        .register::<SeriesKey>()
        .register::<SeriesSelector>()
        .register::<Module>()
        .register::<Tier>()
        .register::<Cursor>()
        .register::<Gap>()
        .register::<Settings>()
        .register::<AlertRule>()
        .register::<Event>()
        .register::<Snapshot>()
        .register::<HistoricalUsageRequest>()
        .register::<HistoricalUsageReply>()
        .register::<UsageProcessRequest>()
        .register::<UsageProcessReply>();
    let ts = Typescript::default()
        .export(&types, specta_serde::PhasesFormat)
        .unwrap_or_else(|e| panic!("export failed: {e}"));

    // Spot-check the shapes the frontend relies on.
    assert!(ts.contains("export type HostId = string"), "{ts}");
    assert!(ts.contains(r#"export type Module = "cpu""#), "{ts}");
    assert!(
        ts.contains("export type Labels = ([string, string])[]"),
        "{ts}"
    );
    // i64 fields marked JsSafeInt export as a plain, non-nullable number.
    assert!(ts.contains("seq: number,"), "{ts}");
    assert!(ts.contains("end_ms: number | null,"), "{ts}");
    assert!(ts.contains("start_time_us: number,"), "{ts}");
    assert!(ts.contains("mem_peak_bytes: number | null,"), "{ts}");
    assert!(
        ts.contains("network_uncertainty: UsageNetworkUncertainty[]"),
        "{ts}"
    );
}
