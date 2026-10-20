#![allow(clippy::too_many_lines)] // Integration scenarios keep setup and assertions together.
//! Codec round-trip and version-skew tests (architecture.md infra 3).
//!
//! `fixtures/v1/` holds one CBOR frame body per message variant, encoded by the v1 codec.
//! Every later release must still decode them to the same values. To (re)write them after
//! an intentional, compatible addition, run:
//!
//! ```sh
//! KELVO_WRITE_FIXTURES=1 cargo test -p kelvo-proto --test skew
//! ```
//!
//! Never regenerate them to make an incompatible change pass; that is the change this test
//! exists to catch.

// clippy.toml allows unwrap inside #[test] functions only; the fixture helpers in this
// test-only file panic on bad fixtures by design.
#![allow(clippy::unwrap_used)]

mod pre_d116;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use ciborium::Value;
use kelvo_proto::{
    ErrorCode, Hello, HelloAck, HostSummary, KNOWN_MESSAGE_TAGS, LiveFrame, LiveProcesses,
    LiveTier, Message, SyncPage, SyncRequest, WireBucket, WireEvent, WireGap, WireLayout,
    WireProcess, decode_message, encode_frame, encode_message, read_frame, write_frame,
};
use kelvo_schema::{
    Capabilities, Catalog, ClusterInfo, CoreKind, Cursor, DiskDevice, FrameView, GapReason, HostId,
    HostIdentity, HostInfo, Module, ModuleCap, OsKind, SeriesKey, Snapshot, Tier,
    UnsupportedReason,
};
use uuid::Uuid;

fn key(s: &str) -> SeriesKey {
    SeriesKey::parse(s).unwrap()
}

fn caps() -> Capabilities {
    let mut modules = BTreeMap::new();
    modules.insert(Module::Cpu, ModuleCap::Available { series: 18 });
    modules.insert(
        Module::Sensors,
        ModuleCap::Unsupported(UnsupportedReason::UnknownChip),
    );
    modules.insert(Module::Battery, ModuleCap::NotPresent);
    Capabilities {
        modules,
        revision: 2,
        process_network: false,
        process_gpu: false,
        // Absent from the sender: a pre-D-116 peer keeps its columns.
        process_energy: true,
        process_wakeups: true,
        process_disk_io: true,
        disk_devices: Vec::new(),
    }
}

fn layout() -> WireLayout {
    WireLayout {
        layout_no: 1,
        series: vec![
            key("cpu.total"),
            key("cpu.load{core=P0}"),
            key("thermal.zone{sensor=PMU tdie4}"),
        ],
    }
}

/// The `HostInfo` fields D-092 and D-116 added. v1.0's `hello` has none, which decodes as empty.
#[derive(Default)]
struct HostFacts {
    gpu_dvfs_mhz: Vec<u32>,
    kernel: Option<String>,
    boot_mounts: Vec<String>,
}

fn hello(facts: HostFacts) -> Message {
    Message::Hello(Box::new(Hello {
        proto_version: 1,
        min_compatible: 1,
        app_version: "1.0.0".into(),
        host: HostIdentity {
            id: HostId(Uuid::from_u128(0xA11CE)),
            display_name: "build box".into(),
            info: HostInfo {
                os: OsKind::MacOs,
                os_version: "27.0".into(),
                model: Some("Mac16,6".into()),
                chip: Some("Apple M4 Pro".into()),
                chip_known: true,
                cpu_topology: vec![ClusterInfo {
                    name: "P0".into(),
                    kind: CoreKind::Performance,
                    cores: vec!["P0".into(), "P1".into()],
                    dvfs_mhz: vec![1260, 4512],
                }],
                mem_total_bytes: 24 << 30,
                boot_time_ms: 1_790_000_000_000,
                gpu_dvfs_mhz: facts.gpu_dvfs_mhz,
                kernel: facts.kernel,
                boot_mounts: facts.boot_mounts,
            },
        },
        db_instance_uuid: Uuid::from_u128(0xE),
        capabilities: caps(),
        features: BTreeSet::from([
            "rows.buckets".to_string(),
            "rows.events".to_string(),
            "rows.gaps".to_string(),
            "zstd-pages".to_string(),
        ]),
    }))
}

/// One sample of every message variant, named for its fixture file.
fn samples() -> Vec<(&'static str, Message)> {
    let epoch = Uuid::from_u128(0xE);
    vec![
        ("hello", hello(HostFacts::default())),
        (
            "hello_host_facts",
            hello(HostFacts {
                gpu_dvfs_mhz: vec![338, 618, 1312, 1242, 1380],
                boot_mounts: vec!["/".into(), "/System/Volumes/Data".into()],
                kernel: None,
            }),
        ),
        (
            "hello_ack",
            Message::HelloAck(HelloAck {
                proto_version: 1,
                features: BTreeSet::new(),
            }),
        ),
        (
            "ping",
            Message::Ping {
                nonce: 7,
                t0_ms: 1_790_000_000_000,
            },
        ),
        (
            "pong",
            Message::Pong {
                nonce: 7,
                t0_ms: 1_790_000_000_000,
                t_remote_ms: 1_790_000_000_012,
            },
        ),
        (
            "subscribe",
            Message::Subscribe {
                tiers: vec![LiveTier::Live1s],
            },
        ),
        ("unsubscribe", Message::Unsubscribe),
        ("layout_def", Message::LayoutDef(layout())),
        (
            "live",
            Message::Live(LiveFrame {
                interval_ms: 0,
                timeline: 0,
                holds: Vec::new(),
                ts_ms: 1_790_000_000_000,
                layout_no: 1,
                values: vec![12.5, 40.0, 61.25],
                // v1.0's frame: no held values, which is also how it encodes.
                held: Vec::new(),
            }),
        ),
        (
            "live_held",
            Message::Live(LiveFrame {
                interval_ms: 0,
                timeline: 0,
                holds: Vec::new(),
                ts_ms: 1_790_000_001_000,
                layout_no: 1,
                values: vec![13.0, 41.0, 61.5],
                held: vec![13.0, 40.0, 61.25],
            }),
        ),
        (
            "live_processes",
            Message::LiveProcesses(LiveProcesses {
                gpu_span_ms: None,
                ts_ms: 1_790_000_000_000,
                rows: vec![WireProcess {
                    ports: None,
                    interval_s: 0.0,
                    energy_j: 0.0,
                    app: None,
                    app_main: false,
                    pid: 812,
                    start_time_us: 1_789_990_000_000_000,
                    name: "kernel_task".into(),
                    cpu_pct: 3.5,
                    mem_bytes: 52 << 20,
                    compressed_bytes: None,
                    threads: 410,
                    idle_wakeups_per_s: 12.0,
                    energy: 0.4,
                    disk_read_bps: 0.0,
                    disk_write_bps: 4096.0,
                    user: "root".into(),
                    net_rx_bps: None,
                    net_tx_bps: None,
                    gpu_pct: None,
                }],
            }),
        ),
        ("capabilities_changed", Message::CapabilitiesChanged(caps())),
        (
            "sync_request",
            Message::SyncRequest(SyncRequest {
                from_ms: None,
                max_bytes: None,
                tier: Tier::M1,
                after: Some(Cursor { epoch, seq: 41_200 }),
                max_rows: 500,
            }),
        ),
        (
            "sync_page",
            Message::SyncPage(SyncPage {
                tier: Tier::M1,
                epoch,
                layouts: vec![layout()],
                rows: vec![WireBucket {
                    seq: 41_201,
                    bucket_ts: 1_790_000_040_000,
                    layout_no: 1,
                    stats: vec![1.0, 30.0, 12.0, 0.0, 100.0, 41.5, 55.0, 70.0, 61.0],
                }],
                gaps: vec![
                    WireGap {
                        source_id: None,
                        seq: 41_202,
                        start_ms: 1_790_000_100_000,
                        end_ms: Some(1_790_003_700_000),
                        module: None,
                        reason: GapReason::Sleep,
                    },
                    WireGap {
                        source_id: None,
                        seq: 41_203,
                        start_ms: 1_790_003_800_000,
                        end_ms: None,
                        module: Some(Module::Disk),
                        reason: GapReason::ModuleDisabled,
                    },
                ],
                events: vec![WireEvent {
                    source_id: None,
                    seq: 41_204,
                    ts_ms: 1_790_000_050_000,
                    kind: "fans_ramped".into(),
                    payload: vec![0xa1, 0x61, 0x70, 0x01],
                }],
                last_seq: 41_204,
                more: false,
            }),
        ),
        // The 15-minute tier (D-076): its own cursor, gated on `rows.m15`.
        (
            "sync_request_m15",
            Message::SyncRequest(SyncRequest {
                from_ms: None,
                max_bytes: None,
                tier: Tier::M15,
                after: Some(Cursor { epoch, seq: 900 }),
                max_rows: 500,
            }),
        ),
        (
            "sync_page_m15",
            Message::SyncPage(SyncPage {
                tier: Tier::M15,
                epoch,
                layouts: vec![layout()],
                rows: vec![WireBucket {
                    seq: 901,
                    bucket_ts: 1_789_999_200_000,
                    layout_no: 1,
                    stats: vec![1.0, 30.0, 12.0, 0.0, 100.0, 41.5, 50.0, 72.0, 60.5],
                }],
                gaps: Vec::new(),
                events: Vec::new(),
                last_seq: 901,
                more: false,
            }),
        ),
        (
            "truncated",
            Message::Truncated {
                tier: Tier::M1,
                earliest_ts_ms: 1_789_000_000_000,
                epoch,
            },
        ),
        (
            "host_summary",
            Message::HostSummary(HostSummary {
                ts_ms: 1_790_000_000_000,
                online: true,
                headline: vec![(key("cpu.total"), 12.5), (key("power.system"), 14.8)],
            }),
        ),
        (
            "error",
            Message::Error {
                code: ErrorCode::IncompatibleVersion,
                message: "need proto 2".into(),
            },
        ),
    ]
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1")
}

#[test]
fn every_variant_round_trips() {
    for (name, msg) in samples().into_iter().chain(remote_samples()) {
        let body = encode_message(&msg).unwrap();
        assert_eq!(decode_message(&body).unwrap(), msg, "{name}");
    }
}

#[test]
fn v1_fixtures_decode_with_current_code() {
    let dir = fixtures_dir();
    if std::env::var_os("KELVO_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, msg) in samples().into_iter().chain(remote_samples()) {
            std::fs::write(
                dir.join(format!("{name}.cbor")),
                encode_message(&msg).unwrap(),
            )
            .unwrap();
        }
    }
    let mut checked = 0;
    for (name, msg) in samples().into_iter().chain(remote_samples()) {
        let path = dir.join(format!("{name}.cbor"));
        let body = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()));
        assert_eq!(decode_message(&body).unwrap(), msg, "fixture {name}");
        checked += 1;
    }
    // Every file in the directory is a checked fixture: a variant cannot lose its fixture
    // silently, and a stale extra file is noticed.
    let on_disk = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(checked, on_disk);
    assert!(checked >= 14);
}

fn cbor(v: &Value) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::into_writer(v, &mut buf).unwrap();
    buf
}

fn tagged(t: &str, c: Option<Value>) -> Vec<u8> {
    let mut m = vec![(Value::Text("t".into()), Value::Text(t.into()))];
    if let Some(c) = c {
        m.push((Value::Text("c".into()), c));
    }
    cbor(&Value::Map(m))
}

/// A newer source's status must not force an older controller to reconnect.
#[test]
fn source_status_with_unknown_reasons_decodes() {
    // Hand-built CBOR, independent of this build's enum serializer.
    let body = tagged(
        "SourceStatus",
        Some(Value::Map(vec![
            (
                Value::Text("interval_ms".into()),
                Value::Integer(2000.into()),
            ),
            (
                Value::Text("performance".into()),
                Value::Text("thermal_budget".into()),
            ),
            (
                Value::Text("power_source".into()),
                Value::Text("external_pack".into()),
            ),
        ])),
    );
    let Message::SourceStatus(status) = decode_message(&body).unwrap() else {
        panic!("expected SourceStatus");
    };
    assert_eq!(status.interval_ms, 2000);
    assert_eq!(status.performance, kelvo_schema::PerformanceReason::Unknown);
    assert!(!status.performance.is_on());
    assert_eq!(status.power_source, kelvo_schema::PowerSource::Unknown);
}

/// Settles the `#[serde(other)]` question in architecture.md (D-038): a message type this
/// build does not know decodes as `Unknown` whatever its content looks like.
#[test]
fn unknown_variant_with_content_decodes_as_unknown() {
    let content = Value::Map(vec![
        (
            Value::Text("fleet".into()),
            Value::Array(vec![Value::Integer(1.into())]),
        ),
        (
            Value::Text("nested".into()),
            Value::Map(vec![(Value::Text("x".into()), Value::Float(1.5))]),
        ),
    ]);
    let cases = [
        ("map content", tagged("FleetDigest", Some(content))),
        (
            "array content",
            tagged("FleetDigest", Some(Value::Array(vec![Value::Bool(true)]))),
        ),
        (
            "scalar content",
            tagged("FleetDigest", Some(Value::Integer(3.into()))),
        ),
        ("null content", tagged("FleetDigest", Some(Value::Null))),
        ("no content", tagged("FleetDigest", None)),
    ];
    for (what, body) in cases {
        assert_eq!(decode_message(&body).unwrap(), Message::Unknown, "{what}");
    }
}

/// Pins why `decode_message` needs its fallback (D-038): plain serde with
/// `#[serde(other)]` on this adjacently tagged enum accepts an unknown tag only when the
/// content is absent or null. If serde ever changes this, the test fails and the fallback
/// can be reconsidered.
#[test]
fn raw_serde_other_rejects_unknown_variant_content() {
    let raw = |body: &[u8]| ciborium::from_reader::<Message, _>(body);
    let map = Value::Map(vec![(Value::Text("x".into()), Value::Integer(1.into()))]);
    assert!(raw(&tagged("FleetDigest", Some(map))).is_err());
    assert!(raw(&tagged("FleetDigest", Some(Value::Array(vec![])))).is_err());
    assert!(raw(&tagged("FleetDigest", Some(Value::Integer(3.into())))).is_err());
    assert_eq!(
        raw(&tagged("FleetDigest", Some(Value::Null))).unwrap(),
        Message::Unknown
    );
    assert_eq!(raw(&tagged("FleetDigest", None)).unwrap(), Message::Unknown);
}

#[test]
fn known_tag_list_matches_the_enum() {
    let tags: BTreeSet<String> = samples()
        .into_iter()
        .chain(remote_samples())
        .map(|(_, msg)| {
            let v: Value = ciborium::from_reader(encode_message(&msg).unwrap().as_slice()).unwrap();
            let Value::Map(m) = v else {
                panic!("message is not a map")
            };
            m.into_iter()
                .find_map(|(k, v)| (k == Value::Text("t".into())).then_some(v))
                .and_then(|v| v.into_text().ok())
                .unwrap()
        })
        .collect();
    let known: BTreeSet<String> = KNOWN_MESSAGE_TAGS.iter().map(|s| s.to_string()).collect();
    // samples() has one entry per variant except Unknown; this match fails to compile when
    // a variant is added, as a reminder to add a sample and a tag.
    let _exhaustive = |m: Message| match m {
        Message::Hello(_)
        | Message::HelloAck(_)
        | Message::Ping { .. }
        | Message::Pong { .. }
        | Message::Subscribe { .. }
        | Message::Unsubscribe
        | Message::Interest(_)
        | Message::SourceStatus(_)
        | Message::RecentRequest { .. }
        | Message::RecentRows { .. }
        | Message::LayoutDef(_)
        | Message::Live(_)
        | Message::LiveProcesses(_)
        | Message::CapabilitiesChanged(_)
        | Message::SyncRequest(_)
        | Message::SyncPage(_)
        | Message::Truncated { .. }
        | Message::HostSummary(_)
        | Message::Error { .. }
        | Message::Unknown => (),
    };
    assert_eq!(tags, known);
}

#[test]
fn unknown_fields_in_known_messages_are_ignored() {
    let ping = Value::Map(vec![
        (Value::Text("nonce".into()), Value::Integer(9.into())),
        (Value::Text("t0_ms".into()), Value::Integer(5.into())),
        (
            Value::Text("added_in_v2".into()),
            Value::Text("ignored".into()),
        ),
    ]);
    assert_eq!(
        decode_message(&tagged("Ping", Some(ping))).unwrap(),
        Message::Ping { nonce: 9, t0_ms: 5 }
    );
}

#[test]
fn malformed_known_variant_is_still_an_error() {
    // `other` must not swallow a known message with a broken body.
    let bad = tagged("Ping", Some(Value::Text("not a struct".into())));
    assert!(decode_message(&bad).is_err());
}

#[test]
fn live_frame_with_unknown_metric_is_accepted_and_ignored() {
    // A newer sender's layout includes a metric this build has never heard of.
    let layout = Message::LayoutDef(WireLayout {
        layout_no: 3,
        series: vec![
            key("cpu.total"),
            key("npu.quantum_flux{tile=0}"),
            key("power.system"),
        ],
    });
    let frame = Message::Live(LiveFrame {
        interval_ms: 0,
        timeline: 0,
        holds: Vec::new(),
        ts_ms: 1_000,
        layout_no: 3,
        values: vec![12.0, 99.0, f32::NAN],
        held: Vec::new(),
    });
    let (Message::LayoutDef(layout), Message::Live(frame)) = (
        decode_message(&encode_message(&layout).unwrap()).unwrap(),
        decode_message(&encode_message(&frame).unwrap()).unwrap(),
    ) else {
        panic!("variants changed in transit");
    };
    assert!(frame.values[2].is_nan(), "NaN survives CBOR");

    let catalog = Catalog::builtin();
    let known: Vec<_> = frame
        .known_values(&layout, &catalog)
        .unwrap()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    assert_eq!(known.len(), 2);
    assert_eq!(known[0], ("cpu.total".to_string(), 12.0));
    assert_eq!(known[1].0, "power.system");

    // The Snapshot built from the same frame shows the known values and nothing else.
    let fv = FrameView::new(frame.ts_ms, &layout.series, &frame.values).unwrap();
    let snap = Snapshot::from_frame(HostId(Uuid::nil()), &fv, &catalog);
    assert_eq!(snap.cpu.unwrap().total, Some(12.0));
    assert_eq!(snap.power.unwrap().system, None);
}

/// The other direction of the `held` addition: a frame with held values decodes on a
/// build that only knows `ts_ms`, `layout_no` and `values` (unknown fields are ignored),
/// and a frame without it decodes here with `held` empty.
#[test]
fn held_values_are_additive() {
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct V1Frame {
        ts_ms: i64,
        layout_no: u32,
        values: Vec<f32>,
    }
    #[derive(serde::Deserialize)]
    struct V1Message {
        t: String,
        c: V1Frame,
    }
    let new = Message::Live(LiveFrame {
        interval_ms: 0,
        timeline: 0,
        holds: Vec::new(),
        ts_ms: 5,
        layout_no: 2,
        values: vec![1.0],
        held: vec![1.0],
    });
    let old: V1Message = ciborium::from_reader(encode_message(&new).unwrap().as_slice()).unwrap();
    assert_eq!(old.t, "Live");
    assert_eq!(
        old.c,
        V1Frame {
            ts_ms: 5,
            layout_no: 2,
            values: vec![1.0]
        }
    );

    let v1_body = std::fs::read(fixtures_dir().join("live.cbor")).unwrap();
    let Message::Live(f) = decode_message(&v1_body).unwrap() else {
        panic!("live fixture")
    };
    assert!(f.held.is_empty());
    assert_eq!(
        encode_message(&Message::Live(f)).unwrap(),
        v1_body,
        "a frame without held values encodes as v1.0 did"
    );
}

/// The other direction of the D-092 `HostInfo` facts: a `hello` carrying them decodes on
/// a build whose `HostInfo` predates them (unknown fields are ignored), with every field
/// it knows intact. The forward direction is the v1 `hello` fixture decoding here with
/// both empty (`v1_fixtures_decode_with_current_code`).
#[test]
fn host_facts_are_additive() {
    /// `HostInfo` as v1.0 had it, frozen.
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct V1Cluster {
        name: String,
        kind: String,
        cores: Vec<String>,
        dvfs_mhz: Vec<u32>,
    }
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct V1HostInfo {
        os: String,
        os_version: String,
        model: Option<String>,
        chip: Option<String>,
        chip_known: bool,
        cpu_topology: Vec<V1Cluster>,
        mem_total_bytes: u64,
        boot_time_ms: i64,
    }
    #[derive(serde::Deserialize)]
    struct V1Identity {
        display_name: String,
        info: V1HostInfo,
    }
    #[derive(serde::Deserialize)]
    struct V1Hello {
        proto_version: u16,
        host: V1Identity,
    }
    #[derive(serde::Deserialize)]
    struct V1Message {
        t: String,
        c: V1Hello,
    }
    let body = std::fs::read(fixtures_dir().join("hello_host_facts.cbor")).unwrap();
    let old: V1Message = ciborium::from_reader(body.as_slice()).unwrap();
    assert_eq!(old.t, "Hello");
    assert_eq!(old.c.proto_version, 1);
    assert_eq!(old.c.host.display_name, "build box");
    assert_eq!(
        old.c.host.info,
        V1HostInfo {
            os: "mac_os".into(),
            os_version: "27.0".into(),
            model: Some("Mac16,6".into()),
            chip: Some("Apple M4 Pro".into()),
            chip_known: true,
            cpu_topology: vec![V1Cluster {
                name: "P0".into(),
                kind: "performance".into(),
                cores: vec!["P0".into(), "P1".into()],
                dvfs_mhz: vec![1260, 4512],
            }],
            mem_total_bytes: 24 << 30,
            boot_time_ms: 1_790_000_000_000,
        }
    );
}

/// Per-process network and GPU rates (v1.2) round-trip when present, and are additive:
/// the v1 `live_processes` fixture (without them) keeps decoding to `None` (checked by
/// `v1_fixtures_decode_with_current_code`), and a row with them decodes on a build that
/// does not know the fields.
#[test]
fn process_rates_round_trip_and_are_additive() {
    let row = |net: Option<(f32, f32)>, gpu: Option<f32>| WireProcess {
        ports: None,
        interval_s: 0.0,
        energy_j: 0.0,
        app: None,
        app_main: false,
        pid: 4242,
        start_time_us: 1_789_990_000_000_000,
        name: "curl".into(),
        cpu_pct: 12.5,
        mem_bytes: 8 << 20,
        compressed_bytes: Some(1 << 20),
        threads: 3,
        idle_wakeups_per_s: 1.0,
        energy: 0.2,
        disk_read_bps: 0.0,
        disk_write_bps: 0.0,
        user: "me".into(),
        net_rx_bps: net.map(|n| n.0),
        net_tx_bps: net.map(|n| n.1),
        gpu_pct: gpu,
    };
    let msg = Message::LiveProcesses(LiveProcesses {
        gpu_span_ms: None,
        ts_ms: 1_790_000_000_000,
        rows: vec![
            row(Some((6_220_000.0, 41_000.0)), Some(37.5)),
            // A zero is a measured zero, not absent (D-082).
            row(Some((0.0, 0.0)), Some(0.0)),
            row(None, Some(3.0)),
            row(Some((10.0, 0.0)), None),
        ],
    });
    let body = encode_message(&msg).unwrap();
    assert_eq!(decode_message(&body).unwrap(), msg);

    #[derive(serde::Deserialize)]
    struct V1Row {
        pid: i32,
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct V1Batch {
        rows: Vec<V1Row>,
    }
    #[derive(serde::Deserialize)]
    struct V1Message {
        c: V1Batch,
    }
    let old: V1Message = ciborium::from_reader(body.as_slice()).unwrap();
    assert_eq!(old.c.rows.len(), 4);
    assert_eq!(
        (old.c.rows[0].pid, old.c.rows[0].name.as_str()),
        (4242, "curl")
    );
}

#[test]
fn known_values_rejects_mismatched_frames() {
    let catalog = Catalog::builtin();
    let l = layout();
    let wrong_no = LiveFrame {
        interval_ms: 0,
        timeline: 0,
        holds: Vec::new(),
        ts_ms: 0,
        layout_no: 9,
        values: vec![0.0; 3],
        held: Vec::new(),
    };
    assert!(wrong_no.known_values(&l, &catalog).is_err());
    let wrong_len = LiveFrame {
        interval_ms: 0,
        timeline: 0,
        holds: Vec::new(),
        ts_ms: 0,
        layout_no: 1,
        values: vec![0.0; 2],
        held: Vec::new(),
    };
    assert!(wrong_len.known_values(&l, &catalog).is_err());
}

#[test]
fn framing_round_trips_a_stream() {
    let mut stream = Vec::new();
    for (_, msg) in samples() {
        write_frame(&mut stream, &msg).unwrap();
    }
    let first = encode_frame(&Message::Unsubscribe).unwrap();
    let body_len = u32::from_be_bytes(first[..4].try_into().unwrap()) as usize;
    assert_eq!(body_len, first.len() - 4, "u32 big-endian length prefix");

    let mut r = stream.as_slice();
    for (name, msg) in samples() {
        assert_eq!(read_frame(&mut r).unwrap(), Some(msg), "{name}");
    }
    assert_eq!(read_frame(&mut r).unwrap(), None, "clean EOF");
}

#[test]
fn framing_rejects_truncated_and_oversized() {
    let frame = encode_frame(&Message::Unsubscribe).unwrap();
    let mut cut = &frame[..frame.len() - 1];
    assert!(read_frame(&mut cut).is_err(), "EOF inside the body");
    let mut half_len = &frame[..2];
    assert!(read_frame(&mut half_len).is_err(), "EOF inside the length");

    let huge = (kelvo_proto::MAX_FRAME_LEN + 1).to_be_bytes();
    let mut r = huge.as_slice();
    assert!(matches!(
        read_frame(&mut r),
        Err(kelvo_proto::CodecError::TooLarge(_))
    ));
}

/// D-076 skew, the old direction: a build from before the 15-minute tier decodes `"m15"`
/// as its `Unknown` tier (D-040) rather than failing the whole message, and so answers
/// the request with an error instead of dropping the connection. Pinned with that build's
/// tier list and the same text-enum fallback.
#[test]
fn m15_is_an_unknown_tier_to_a_pre_m15_build() {
    #[derive(Debug, PartialEq)]
    enum PreM15Tier {
        Live1s,
        S10,
        M1,
        Unknown,
    }
    impl<'de> serde::Deserialize<'de> for PreM15Tier {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            Ok(match String::deserialize(d)?.as_str() {
                "live1s" => PreM15Tier::Live1s,
                "s10" => PreM15Tier::S10,
                "m1" => PreM15Tier::M1,
                _ => PreM15Tier::Unknown,
            })
        }
    }
    #[derive(serde::Deserialize, Debug)]
    struct PreM15Request {
        tier: PreM15Tier,
        max_rows: u32,
    }
    #[derive(serde::Deserialize)]
    struct Envelope {
        t: String,
        c: PreM15Request,
    }
    let body = std::fs::read(fixtures_dir().join("sync_request_m15.cbor")).unwrap();
    let old: Envelope = ciborium::from_reader(body.as_slice()).unwrap();
    assert_eq!(old.t, "SyncRequest");
    assert_eq!(old.c.tier, PreM15Tier::Unknown);
    assert_eq!(old.c.max_rows, 500);
    let m1 = std::fs::read(fixtures_dir().join("sync_request.cbor")).unwrap();
    let old: Envelope = ciborium::from_reader(m1.as_slice()).unwrap();
    assert_eq!(old.c.tier, PreM15Tier::M1, "the pin decodes known tiers");
    // And this build decodes a tier newer than itself the same way.
    let future = Value::Map(vec![
        (Value::Text("tier".into()), Value::Text("h1".into())),
        (Value::Text("after".into()), Value::Null),
        (Value::Text("max_rows".into()), Value::Integer(5.into())),
    ]);
    let Message::SyncRequest(req) = decode_message(&tagged("SyncRequest", Some(future))).unwrap()
    else {
        panic!("not a request")
    };
    assert_eq!(req.tier, Tier::Unknown);
}

#[test]
fn bucket_stats_by_index() {
    let Some((_, Message::SyncPage(page))) = samples().into_iter().find(|(n, _)| *n == "sync_page")
    else {
        panic!("no sync page sample");
    };
    let row = &page.rows[0];
    assert_eq!(row.stat(1), Some((0.0, 100.0, 41.5)));
    assert_eq!(row.stat(3), None);
}

/// A Linux agent's `Hello` with every D-116 wire addition: a kernel, uniform cores, the
/// Containers module, a permission-denied module, no energy or wakeup columns and a disk
/// inventory.
fn hello_linux() -> Message {
    let mut modules = BTreeMap::new();
    modules.insert(Module::Cpu, ModuleCap::Available { series: 12 });
    modules.insert(
        Module::Power,
        ModuleCap::Unsupported(UnsupportedReason::PermissionDenied),
    );
    modules.insert(Module::Containers, ModuleCap::Available { series: 40 });
    Message::Hello(Box::new(Hello {
        proto_version: 1,
        min_compatible: 1,
        app_version: "1.1.0".into(),
        host: HostIdentity {
            id: HostId(Uuid::from_u128(0x11_4E7)),
            display_name: "asahi".into(),
            info: HostInfo {
                os: OsKind::Linux,
                os_version: "Fedora Linux Asahi Remix 41 (Server Edition)".into(),
                model: Some("Apple Mac mini (M1, 2020)".into()),
                chip: Some("Apple M1".into()),
                chip_known: false,
                cpu_topology: vec![ClusterInfo {
                    name: "C0".into(),
                    kind: CoreKind::Uniform,
                    cores: vec!["0".into(), "1".into()],
                    dvfs_mhz: vec![600, 2064],
                }],
                mem_total_bytes: 16 << 30,
                boot_time_ms: 1_790_000_000_000,
                gpu_dvfs_mhz: Vec::new(),
                kernel: Some("6.17.0-400.asahi.fc41.aarch64+16k".into()),
                boot_mounts: Vec::new(),
            },
        },
        db_instance_uuid: Uuid::from_u128(0xE3),
        capabilities: Capabilities {
            modules,
            revision: 1,
            process_network: false,
            process_gpu: false,
            process_energy: false,
            process_wakeups: false,
            // The fixture predates the field, so it decodes as the default.
            process_disk_io: true,
            disk_devices: vec![DiskDevice {
                dev: "nvme0n1".into(),
                name: "APPLE SSD AP0512Q".into(),
                internal: Some(true),
                default: true,
            }],
        },
        features: BTreeSet::from(["rows.buckets".to_string(), "rows.gaps".to_string()]),
    }))
}

fn capabilities_linux() -> Message {
    Message::CapabilitiesChanged(Capabilities {
        modules: BTreeMap::from([(Module::Cpu, ModuleCap::Available { series: 12 })]),
        revision: 4,
        process_network: false,
        process_gpu: false,
        process_energy: false,
        process_wakeups: false,
        process_disk_io: false,
        disk_devices: Vec::new(),
    })
}

/// D-116's `process_disk_io`: absent from every older sender's capabilities (a Mac, the
/// first Linux agents), where it reads as true; and a false from a newer agent is
/// ignored by a build that predates it.
#[test]
fn process_disk_io_skew() {
    for name in ["hello", "capabilities_changed", "hello_linux"] {
        let body = std::fs::read(fixtures_dir().join(format!("{name}.cbor"))).unwrap();
        let caps = match decode_message(&body).unwrap() {
            Message::Hello(h) => h.capabilities,
            Message::CapabilitiesChanged(c) => c,
            other => panic!("{name}: {other:?}"),
        };
        assert!(caps.process_disk_io, "{name}");
    }
    #[derive(serde::Deserialize)]
    struct PreCaps {
        revision: u64,
        process_energy: bool,
    }
    #[derive(serde::Deserialize)]
    struct Envelope {
        t: String,
        c: PreCaps,
    }
    let body = std::fs::read(fixtures_dir().join("capabilities_linux.cbor")).unwrap();
    let old: Envelope = ciborium::from_reader(body.as_slice()).unwrap();
    assert_eq!(
        (old.t.as_str(), old.c.revision, old.c.process_energy),
        ("CapabilitiesChanged", 4, false)
    );
}

/// D-116, the forward direction: a pre-D-116 peer's `hello` (the v1 fixture) decodes with
/// no kernel, the energy and wakeup columns present and no disk inventory.
#[test]
fn d116_fields_default_for_an_older_peer() {
    let body = std::fs::read(fixtures_dir().join("hello.cbor")).unwrap();
    let Message::Hello(h) = decode_message(&body).unwrap() else {
        panic!("hello fixture")
    };
    assert_eq!(h.host.info.kernel, None);
    assert!(h.capabilities.process_energy && h.capabilities.process_wakeups);
    assert!(h.capabilities.disk_devices.is_empty());
    let body = std::fs::read(fixtures_dir().join("capabilities_changed.cbor")).unwrap();
    let Message::CapabilitiesChanged(c) = decode_message(&body).unwrap() else {
        panic!("capabilities fixture")
    };
    assert!(c.process_energy && c.process_wakeups);
}

/// D-116, the old direction: a Linux agent's `hello` and `capabilities_changed` decode
/// on a build from before D-116, with that build's own types ([`pre_d116`], vendored
/// from the merge base). The new fields are ignored, `uniform` and `permission_denied`
/// read as `Unknown`, the Containers entry is dropped like any unknown module (D-040),
/// and everything else is intact.
#[test]
fn d116_values_decode_on_a_pre_d116_build() {
    use pre_d116 as old;

    let body = std::fs::read(fixtures_dir().join("hello_linux.cbor")).unwrap();
    let old::Message::Hello(hello) = ciborium::from_reader(body.as_slice()).unwrap() else {
        panic!("hello_linux is a Hello")
    };
    assert_eq!(
        (
            hello.proto_version,
            hello.min_compatible,
            hello.app_version.as_str()
        ),
        (1, 1, "1.1.0")
    );
    assert_eq!(hello.host.id, old::HostId(Uuid::from_u128(0x11_4E7)));
    assert_eq!(hello.db_instance_uuid, Uuid::from_u128(0xE3));
    assert_eq!(hello.host.display_name, "asahi");
    let info = &hello.host.info;
    assert_eq!(
        (info.os, info.mem_total_bytes, info.boot_time_ms),
        (old::OsKind::Linux, 16 << 30, 1_790_000_000_000)
    );
    assert!(info.os_version.starts_with("Fedora"));
    assert_eq!(
        (info.model.as_deref(), info.chip.as_deref(), info.chip_known),
        (Some("Apple Mac mini (M1, 2020)"), Some("Apple M1"), false)
    );
    assert_eq!(
        info.cpu_topology,
        [old::ClusterInfo {
            name: "C0".into(),
            kind: old::CoreKind::Unknown,
            cores: vec!["0".into(), "1".into()],
            dvfs_mhz: vec![600, 2064],
        }]
    );
    assert_eq!(
        hello.capabilities,
        old::Capabilities {
            modules: BTreeMap::from([
                (old::Module::Cpu, old::ModuleCap::Available { series: 12 }),
                (
                    old::Module::Power,
                    old::ModuleCap::Unsupported(old::UnsupportedReason::Unknown)
                ),
            ]),
            revision: 1,
            process_network: false,
            process_gpu: false,
        },
        "containers is dropped, permission_denied is unknown"
    );

    let body = std::fs::read(fixtures_dir().join("capabilities_linux.cbor")).unwrap();
    let old::Message::CapabilitiesChanged(caps) = ciborium::from_reader(body.as_slice()).unwrap()
    else {
        panic!("capabilities_linux is CapabilitiesChanged")
    };
    assert_eq!(
        caps,
        old::Capabilities {
            modules: BTreeMap::from([(old::Module::Cpu, old::ModuleCap::Available { series: 12 })]),
            revision: 4,
            process_network: false,
            process_gpu: false,
        }
    );
}

fn remote_samples() -> Vec<(&'static str, Message)> {
    vec![
        // A Linux agent's handshake (D-116).
        ("hello_linux", hello_linux()),
        // A non-root agent: no energy, wakeup or disk columns (D-116).
        ("capabilities_linux", capabilities_linux()),
        (
            "live_remote",
            Message::Live(LiveFrame {
                interval_ms: 2000,
                timeline: 7,
                holds: vec![1000, 4000, 2000],
                ts_ms: 1_790_000_000_000,
                layout_no: 1,
                values: vec![12.5, 40.0, 61.25],
                held: vec![12.0, 39.0, 60.0],
            }),
        ),
        (
            "interest",
            Message::Interest(kelvo_proto::RemoteInterest {
                detail: true,
                process_period_ms: Some(1000),
                network_processes: true,
                gpu_processes: false,
                port_processes: true,
            }),
        ),
        (
            "source_status",
            Message::SourceStatus(kelvo_proto::SourceStatus {
                interval_ms: 1000,
                history_available: true,
                ..Default::default()
            }),
        ),
        (
            "recent_request",
            Message::RecentRequest {
                from_ms: 1000,
                to_ms: 2000,
            },
        ),
        (
            "recent_rows",
            Message::RecentRows {
                epoch: Uuid::from_u128(3),
                tier: Tier::S10,
                complete_to_ms: 2000,
                layouts: vec![],
                rows: vec![],
            },
        ),
    ]
}
