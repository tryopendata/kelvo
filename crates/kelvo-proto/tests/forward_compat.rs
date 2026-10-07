//! Messages from a newer peer that carry enum values this build has never seen (D-040).
//!
//! `fixtures/skew/` holds CBOR bodies built by hand, the way a hypothetical v2 peer would
//! encode them: a new OS, core kind, module, capability state, unsupported reason, gap
//! reason, live tier and error code. The current code must decode each one, with the
//! unseen values mapped to `Unknown` (or dropped, for unknown module keys) and everything
//! else intact. To rewrite the files after changing the builders below:
//!
//! ```sh
//! KELVO_WRITE_FIXTURES=1 cargo test -p kelvo-proto --test forward_compat
//! ```

// clippy.toml allows unwrap inside #[test] functions only; the fixture helpers in this
// test-only file panic on bad fixtures by design.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use ciborium::Value;
use kelvo_proto::{ErrorCode, LiveTier, Message, WireGap, decode_message};
use kelvo_schema::{
    Capabilities, CoreKind, GapReason, Module, ModuleCap, OsKind, Tier, UnsupportedReason,
};

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

fn int(i: i64) -> Value {
    Value::Integer(i.into())
}

fn map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (text(k), v)).collect())
}

fn msg(t: &str, c: Value) -> Value {
    map(vec![("t", text(t)), ("c", c)])
}

/// UUIDs are CBOR byte strings (serde's non-human-readable form).
fn uuid_bytes(n: u128) -> Value {
    Value::Bytes(uuid::Uuid::from_u128(n).as_bytes().to_vec())
}

/// Capabilities as a v2 peer might send them: a new module, a new state, a new reason.
fn caps_v2() -> Value {
    map(vec![
        (
            "modules",
            map(vec![
                (
                    "cpu",
                    map(vec![("available", map(vec![("series", int(18))]))]),
                ),
                // New state carrying content.
                (
                    "gpu",
                    map(vec![("degraded", map(vec![("series", int(3))]))]),
                ),
                // Known state, new reason.
                (
                    "sensors",
                    map(vec![("unsupported", text("thermal_lockout"))]),
                ),
                // New module.
                (
                    "npu",
                    map(vec![("available", map(vec![("series", int(4))]))]),
                ),
                // New content-free state.
                ("battery", text("hibernating")),
            ]),
        ),
        ("revision", int(7)),
        ("added_in_v2", Value::Bool(true)),
    ])
}

fn expected_caps() -> Capabilities {
    Capabilities {
        modules: BTreeMap::from([
            (Module::Cpu, ModuleCap::Available { series: 18 }),
            (Module::Gpu, ModuleCap::Unknown),
            (
                Module::Sensors,
                ModuleCap::Unsupported(UnsupportedReason::Unknown),
            ),
            (Module::Battery, ModuleCap::Unknown),
        ]),
        revision: 7,
        process_network: false,
        process_gpu: false,
    }
}

fn hello_v2() -> Value {
    msg(
        "Hello",
        map(vec![
            ("proto_version", int(2)),
            ("min_compatible", int(1)),
            ("app_version", text("4.3.0")),
            (
                "host",
                map(vec![
                    ("id", uuid_bytes(0xA11CE)),
                    // A field this build does not know, inside a known struct.
                    ("site", text("rack 3")),
                    ("display_name", text("build box")),
                    (
                        "info",
                        map(vec![
                            ("os", text("free_bsd")),
                            ("os_version", text("15.0")),
                            ("model", Value::Null),
                            ("chip", Value::Null),
                            ("chip_known", Value::Bool(false)),
                            (
                                "cpu_topology",
                                Value::Array(vec![map(vec![
                                    ("name", text("X0")),
                                    ("kind", text("super")),
                                    ("cores", Value::Array(vec![text("X0")])),
                                    ("dvfs_mhz", Value::Array(vec![int(5000)])),
                                ])]),
                            ),
                            ("mem_total_bytes", int(64 << 30)),
                            ("boot_time_ms", int(1_790_000_000_000)),
                        ]),
                    ),
                ]),
            ),
            ("db_instance_uuid", uuid_bytes(0xE2)),
            ("capabilities", caps_v2()),
            ("features", Value::Array(vec![text("zstd-pages")])),
        ]),
    )
}

fn gap(seq: i64, module: Value, reason: &str) -> Value {
    map(vec![
        ("seq", int(seq)),
        ("start_ms", int(1_790_000_100_000)),
        ("end_ms", int(1_790_000_200_000)),
        ("module", module),
        ("reason", text(reason)),
    ])
}

fn sync_page_v2() -> Value {
    msg(
        "SyncPage",
        map(vec![
            ("tier", text("m1")),
            ("epoch", uuid_bytes(0xE2)),
            ("layouts", Value::Array(vec![])),
            ("rows", Value::Array(vec![])),
            (
                "gaps",
                Value::Array(vec![
                    gap(10, Value::Null, "sleep"),
                    gap(11, Value::Null, "lid_closed"),
                    gap(12, text("npu"), "module_disabled"),
                    gap(13, text("disk"), "thermal_shutdown"),
                ]),
            ),
            ("events", Value::Array(vec![])),
            ("last_seq", int(13)),
            ("more", Value::Bool(false)),
        ]),
    )
}

fn subscribe_v2() -> Value {
    msg(
        "Subscribe",
        map(vec![(
            "tiers",
            Value::Array(vec![text("live1s"), text("live100ms")]),
        )]),
    )
}

fn error_v2() -> Value {
    msg(
        "Error",
        map(vec![
            ("code", text("rate_limited")),
            ("message", text("slow down")),
        ]),
    )
}

fn sync_request_v2() -> Value {
    msg(
        "SyncRequest",
        map(vec![
            ("tier", text("h1")),
            ("after", Value::Null),
            ("max_rows", int(500)),
        ]),
    )
}

fn fixtures() -> Vec<(&'static str, Value)> {
    vec![
        ("hello_unknown_enums", hello_v2()),
        ("sync_page_unknown_enums", sync_page_v2()),
        ("subscribe_unknown_tier", subscribe_v2()),
        ("error_unknown_code", error_v2()),
        ("sync_request_unknown_tier", sync_request_v2()),
    ]
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skew")
}

/// Reads a fixture (writing it first when `KELVO_WRITE_FIXTURES` is set) and checks the
/// file still holds exactly what the builder produces.
fn load(name: &str) -> Message {
    let (_, value) = fixtures().into_iter().find(|(n, _)| *n == name).unwrap();
    let mut built = Vec::new();
    ciborium::into_writer(&value, &mut built).unwrap();
    let path = fixtures_dir().join(format!("{name}.cbor"));
    if std::env::var_os("KELVO_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(fixtures_dir()).unwrap();
        std::fs::write(&path, &built).unwrap();
    }
    let body =
        std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()));
    assert_eq!(body, built, "fixture {name} differs from its builder");
    decode_message(&body).unwrap_or_else(|e| panic!("fixture {name} failed to decode: {e}"))
}

#[test]
fn every_skew_fixture_is_checked() {
    let on_disk = std::fs::read_dir(fixtures_dir()).unwrap().count();
    assert_eq!(on_disk, fixtures().len());
}

#[test]
fn hello_with_unknown_enums_decodes() {
    let Message::Hello(h) = load("hello_unknown_enums") else {
        panic!("not a Hello");
    };
    assert_eq!(h.proto_version, 2);
    assert_eq!(h.host.info.os, OsKind::Unknown);
    assert_eq!(h.host.info.cpu_topology[0].kind, CoreKind::Unknown);
    assert_eq!(h.host.info.cpu_topology[0].dvfs_mhz, vec![5000]);
    assert_eq!(h.capabilities, expected_caps());
    assert_eq!(h.features, BTreeSet::from(["zstd-pages".to_string()]));
}

#[test]
fn sync_page_with_unknown_enums_decodes() {
    let Message::SyncPage(p) = load("sync_page_unknown_enums") else {
        panic!("not a SyncPage");
    };
    assert_eq!(p.tier, Tier::M1);
    let g = |seq, module, reason| WireGap {
        seq,
        start_ms: 1_790_000_100_000,
        end_ms: Some(1_790_000_200_000),
        module,
        reason,
    };
    assert_eq!(
        p.gaps,
        vec![
            g(10, None, GapReason::Sleep),
            g(11, None, GapReason::Unknown),
            g(12, Some(Module::Unknown), GapReason::ModuleDisabled),
            g(13, Some(Module::Disk), GapReason::Unknown),
        ]
    );
    assert_eq!(p.last_seq, 13);
}

#[test]
fn subscribe_with_unknown_tier_decodes() {
    assert_eq!(
        load("subscribe_unknown_tier"),
        Message::Subscribe {
            tiers: vec![LiveTier::Live1s, LiveTier::Unknown]
        }
    );
}

#[test]
fn error_with_unknown_code_decodes() {
    assert_eq!(
        load("error_unknown_code"),
        Message::Error {
            code: ErrorCode::Unknown,
            message: "slow down".into()
        }
    );
}

#[test]
fn sync_request_for_unknown_tier_decodes() {
    let Message::SyncRequest(r) = load("sync_request_unknown_tier") else {
        panic!("not a SyncRequest");
    };
    assert_eq!(r.tier, Tier::Unknown);
    assert!(!r.tier.is_persisted());
}

#[test]
fn capabilities_changed_with_unknown_enums_decodes() {
    let mut body = Vec::new();
    ciborium::into_writer(&msg("CapabilitiesChanged", caps_v2()), &mut body).unwrap();
    assert_eq!(
        decode_message(&body).unwrap(),
        Message::CapabilitiesChanged(expected_caps())
    );
}
