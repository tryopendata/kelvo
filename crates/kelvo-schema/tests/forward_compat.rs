//! Forward compatibility of wire-facing and stored enums (D-040): a value this build has
//! never seen, encoded as CBOR the way a newer peer would send it, decodes as the enum's
//! `Unknown` variant instead of failing.

// clippy.toml allows unwrap inside #[test] functions only; the shared check helper in this
// test-only file panics on failure by design.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fmt::Debug;

use ciborium::Value;
use kelvo_schema::{
    Capabilities, CoreKind, Gap, GapReason, Module, ModuleCap, OsKind, Tier, UnsupportedReason,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

fn map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (text(k), v)).collect())
}

fn decode<T: DeserializeOwned>(v: &Value) -> Result<T, String> {
    let mut buf = Vec::new();
    ciborium::into_writer(v, &mut buf).map_err(|e| e.to_string())?;
    ciborium::from_reader(buf.as_slice()).map_err(|e| e.to_string())
}

/// Every known value round-trips through CBOR and JSON with `as_str` as its text, and an
/// unseen string decodes as `unknown`.
fn check_text_enum<T>(known: &[T], unknown: T, as_str: fn(T) -> &'static str, unseen: &str)
where
    T: Copy + Debug + PartialEq + Serialize + DeserializeOwned,
{
    for &k in known.iter().chain([&unknown]) {
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            format!("\"{}\"", as_str(k)),
            "serialized text of {k:?} matches as_str"
        );
        assert_eq!(decode::<T>(&text(as_str(k))).unwrap(), k, "{k:?}");
    }
    assert_eq!(
        decode::<T>(&text(unseen)).unwrap(),
        unknown,
        "unseen {unseen:?}"
    );
    assert_eq!(
        serde_json::from_str::<T>(&format!("\"{unseen}\"")).unwrap(),
        unknown
    );
    assert!(!known.contains(&unknown), "ALL excludes Unknown");
}

#[test]
fn module() {
    check_text_enum(&Module::ALL, Module::Unknown, Module::as_str, "npu");
}

#[test]
fn gap_reason() {
    check_text_enum(
        &GapReason::ALL,
        GapReason::Unknown,
        GapReason::as_str,
        "lid_closed",
    );
}

#[test]
fn unsupported_reason() {
    check_text_enum(
        &UnsupportedReason::ALL,
        UnsupportedReason::Unknown,
        UnsupportedReason::as_str,
        "thermal_lockout",
    );
}

#[test]
fn tier() {
    check_text_enum(&Tier::ALL, Tier::Unknown, Tier::as_str, "h1");
}

#[test]
fn os_kind() {
    check_text_enum(&OsKind::ALL, OsKind::Unknown, OsKind::as_str, "free_bsd");
}

#[test]
fn core_kind() {
    check_text_enum(&CoreKind::ALL, CoreKind::Unknown, CoreKind::as_str, "super");
}

#[test]
fn text_enums_still_reject_non_strings() {
    assert!(decode::<Module>(&Value::Integer(3.into())).is_err());
    assert!(decode::<GapReason>(&map(vec![("sleep", Value::Null)])).is_err());
}

#[test]
fn module_cap_known_states() {
    for cap in [
        ModuleCap::Available { series: 16 },
        ModuleCap::Unsupported(UnsupportedReason::NoHardware),
        ModuleCap::NotPresent,
        ModuleCap::Unknown,
    ] {
        let mut buf = Vec::new();
        ciborium::into_writer(&cap, &mut buf).unwrap();
        assert_eq!(
            ciborium::from_reader::<ModuleCap, _>(buf.as_slice()).unwrap(),
            cap
        );
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(serde_json::from_str::<ModuleCap>(&json).unwrap(), cap);
    }
    // Unknown fields inside a known state are ignored, like everywhere else.
    let extra = map(vec![(
        "available",
        map(vec![
            ("series", Value::Integer(4.into())),
            ("added_later", Value::Bool(true)),
        ]),
    )]);
    assert_eq!(
        decode::<ModuleCap>(&extra).unwrap(),
        ModuleCap::Available { series: 4 }
    );
}

#[test]
fn module_cap_unseen_state_with_or_without_content() {
    let cases = [
        ("bare string", text("throttled")),
        (
            "map content",
            map(vec![(
                "degraded",
                map(vec![("series", Value::Integer(3.into()))]),
            )]),
        ),
        (
            "scalar content",
            map(vec![("degraded", Value::Integer(3.into()))]),
        ),
        (
            "array content",
            map(vec![("degraded", Value::Array(vec![]))]),
        ),
        ("null content", map(vec![("degraded", Value::Null)])),
    ];
    for (what, v) in cases {
        assert_eq!(
            decode::<ModuleCap>(&v).unwrap(),
            ModuleCap::Unknown,
            "{what}"
        );
    }
    // An unseen reason inside a known state keeps the state.
    assert_eq!(
        decode::<ModuleCap>(&map(vec![("unsupported", text("thermal_lockout"))])).unwrap(),
        ModuleCap::Unsupported(UnsupportedReason::Unknown)
    );
}

#[test]
fn module_cap_rejects_malformed_known_state() {
    let bad = map(vec![("available", text("lots"))]);
    assert!(decode::<ModuleCap>(&bad).is_err());
    let two = map(vec![
        ("not_present", Value::Null),
        ("available", map(vec![("series", Value::Integer(1.into()))])),
    ]);
    assert!(decode::<ModuleCap>(&two).is_err());
    assert!(decode::<ModuleCap>(&Value::Integer(1.into())).is_err());
}

#[test]
fn capabilities_drop_unknown_modules() {
    let v = map(vec![
        (
            "modules",
            map(vec![
                (
                    "cpu",
                    map(vec![(
                        "available",
                        map(vec![("series", Value::Integer(18.into()))]),
                    )]),
                ),
                (
                    "npu",
                    map(vec![(
                        "available",
                        map(vec![("series", Value::Integer(2.into()))]),
                    )]),
                ),
                ("tpu", text("not_present")),
                ("gpu", map(vec![("degraded", Value::Null)])),
            ]),
        ),
        ("revision", Value::Integer(5.into())),
    ]);
    let caps: Capabilities = decode(&v).unwrap();
    let expected = Capabilities {
        modules: BTreeMap::from([
            (Module::Cpu, ModuleCap::Available { series: 18 }),
            (Module::Gpu, ModuleCap::Unknown),
        ]),
        revision: 5,
        process_network: false,
        process_gpu: false,
    };
    assert_eq!(caps, expected);
}

#[test]
fn gap_with_unseen_reason_or_module() {
    let unseen_reason = map(vec![
        ("start_ms", Value::Integer(1.into())),
        ("end_ms", Value::Integer(2.into())),
        ("module", Value::Null),
        ("reason", text("lid_closed")),
    ]);
    let g: Gap = decode(&unseen_reason).unwrap();
    assert_eq!(g.reason, GapReason::Unknown);
    g.validate().unwrap();
    assert!(
        g.affects(Module::Cpu),
        "a whole-host unknown gap blanks every module"
    );

    let unseen_module = map(vec![
        ("start_ms", Value::Integer(1.into())),
        ("end_ms", Value::Null),
        ("module", text("npu")),
        ("reason", text("module_disabled")),
    ]);
    let g: Gap = decode(&unseen_module).unwrap();
    assert_eq!(g.module, Some(Module::Unknown));
    for m in Module::ALL {
        assert!(
            !g.affects(m),
            "a gap for an unknown module blanks no known module"
        );
    }
}
