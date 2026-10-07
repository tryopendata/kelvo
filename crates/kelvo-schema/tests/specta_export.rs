//! Exports every IPC-facing type with the same exporter and serde format tauri-specta
//! uses (default `Typescript`, `specta_serde::PhasesFormat`). A type the app shell could
//! not export (an unannotated `i64`, a serde attribute specta rejects) fails here.

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
        .register::<Snapshot>();
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
}
