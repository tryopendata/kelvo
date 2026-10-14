//! Cross-language ordering fixture: emitted by the real registry with deterministic time.
use super::*;
use serde_json::{Value, json};

fn trace(messages: impl IntoIterator<Item = LiveMsg>) -> Vec<Value> {
    messages
        .into_iter()
        .filter_map(|message| match message {
            LiveMsg::Holds { .. } => None, // Holds have their own catalog/expiry contract tests.
            LiveMsg::Backfill { start_ms, rows, .. } => {
                Some(json!({"kind": "backfill", "start_ms": start_ms, "rows": rows.len()}))
            }
            LiveMsg::BackfillEarlier { start_ms, rows, .. } => {
                Some(json!({"kind": "backfill_earlier", "start_ms": start_ms, "rows": rows.len()}))
            }
            LiveMsg::Frame { ts_ms, .. } => Some(json!({"kind": "frame", "ts_ms": ts_ms})),
            message => Some(json!({"kind": serde_json::to_value(message).unwrap()["kind"]})),
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn frontend_protocol_fixture_matches_native_registry() {
    let feed = FakeFeed::new();
    let layout = layout(1, 1);
    for i in 1..=1500 {
        feed.tick(&layout, i * 1000, 0.0);
    }
    let reg = registry(&["dashboard"]);
    let (live_sink, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), live_sink, req(3_600_000));
    let initial = trace(rx.try_iter());
    feed.tick(&layout, 1_501_000, 1.0);
    settle().await;
    let after_frame = trace(rx.try_iter());
    reg.window_closed("dashboard");
    settle().await;
    feed.tick(&layout, 1_502_000, 2.0);
    settle().await;
    let disconnected = trace(rx.try_iter());
    // Cancel while the older chunks are still deferred. Neither their timer nor
    // a subsequent frame may publish into the closed window.
    let cancel_reg = registry(&["dashboard"]);
    let (cancel_sink, cancel_rx, _) = sink();
    cancel_reg.subscribe("dashboard", feed.clone(), cancel_sink, req(3_600_000));
    let _ = trace(cancel_rx.try_iter());
    cancel_reg.window_closed("dashboard");
    tokio::time::advance(Duration::from_secs(2)).await;
    feed.tick(&layout, 1_503_000, 3.0);
    settle().await;
    let cancelled = trace(cancel_rx.try_iter());
    let unknown = HostId(Uuid::nil());
    let result = json!({
        "initial": initial, "after_frame": after_frame, "disconnected": disconnected, "cancelled": cancelled,
        "typed_failure": {"status": "error", "error": crate::error::CommandError::UnknownHost { host: unknown }},
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/live-protocol.json");
    if std::env::var_os("KELVO_UPDATE_PROTOCOL_FIXTURE").is_some() {
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&result).unwrap()),
        )
        .unwrap();
    }
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        result, saved,
        "regenerate with KELVO_UPDATE_PROTOCOL_FIXTURE=1, review Rust/mock parity together"
    );
}
