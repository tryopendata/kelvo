//! Message types (architecture.md infra 3 and 4). A one-way door: field names and the
//! `t`/`c` tagging are the wire format.

use std::collections::BTreeSet;

use kelvo_schema::{Capabilities, Catalog, GapReason, HostIdentity, Module, SeriesKey, Tier};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Every frame body is one `Message`, adjacently tagged: `{"t": "Live", "c": {...}}`.
///
/// A message type this build does not know decodes as [`Message::Unknown`] (log and skip),
/// whether or not it carries content; see [`crate::decode_message`] and D-038.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Message {
    Hello(Hello),
    HelloAck(HelloAck),
    Ping {
        nonce: u64,
        t0_ms: i64,
    },
    Pong {
        nonce: u64,
        t0_ms: i64,
        t_remote_ms: i64,
    },

    // Live, ephemeral, no seq.
    Subscribe {
        tiers: Vec<LiveTier>,
    },
    Unsubscribe,
    /// Session-scoped layout number to series keys.
    LayoutDef(WireLayout),
    Live(LiveFrame),
    /// Process rows from one sample, sent while the controller has process interest for
    /// the host. Added after v1.0's fixtures: an older receiver decodes it as `Unknown`.
    LiveProcesses(LiveProcesses),
    CapabilitiesChanged(Capabilities),

    // Durable, cursor-based.
    SyncRequest(SyncRequest),
    SyncPage(SyncPage),
    /// The sender pruned past the requested cursor. The receiver writes a `truncated` gap
    /// up to `earliest_ts_ms` and asks again from the start.
    Truncated {
        tier: Tier,
        earliest_ts_ms: i64,
        epoch: Uuid,
    },

    /// Reserved in v1, first used in v4 by the fleet view.
    HostSummary(HostSummary),

    Error {
        code: ErrorCode,
        message: String,
    },

    /// A newer peer sent a message type this build does not know: log and skip.
    #[serde(other)]
    Unknown,
}

/// First message from each side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// Highest protocol version the sender speaks.
    pub proto_version: u16,
    /// Lowest protocol version the sender still speaks.
    pub min_compatible: u16,
    pub app_version: String,
    /// Who the sender is. Never says whether the receiver should treat it as local:
    /// that is a property of the receiver's store (architecture.md infra 2).
    pub host: HostIdentity,
    /// The sender's database instance; the epoch of every cursor into it.
    pub db_instance_uuid: Uuid,
    pub capabilities: Capabilities,
    /// Optional behaviours the sender supports, for example `"zstd-pages"`, and the
    /// [`SyncPage`] row kinds it can send or ingest (`"rows.buckets"`, `"rows.gaps"`,
    /// `"rows.events"`, `"rows.m15"`; see [`kelvo_schema::SyncRowKind`]).
    pub features: BTreeSet<String>,
}

/// The controller's reply to [`Hello`]: the negotiated version and feature set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelloAck {
    pub proto_version: u16,
    pub features: BTreeSet<String>,
}

/// Live tiers a subscriber can ask for. v1 has only the base tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveTier {
    Live1s,
    /// A live tier this build does not know, asked for by a newer peer (D-040). Not
    /// served; the rest of the subscription still is.
    #[serde(other)]
    Unknown,
}

/// A layout as it travels: a connection-local number and the series keys, in blob order.
/// Keys travel as strings, never as a store's interned IDs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireLayout {
    /// Valid only within this connection (live) or this page (sync).
    pub layout_no: u32,
    pub series: Vec<SeriesKey>,
}

/// One tick of live values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiveFrame {
    /// Remote clock, authoritative for that host.
    pub ts_ms: i64,
    pub layout_no: u32,
    /// One per series in the layout. `NaN` means present in the layout but not sampled
    /// this tick.
    pub values: Vec<f32>,
    /// The sender's latest-value cache (D-047): each series' latest value while it is
    /// still current, `NaN` once stale, in layout order. Empty from a sender that does not
    /// send it (older builds); the receiver then holds values itself. Omitted from the
    /// encoding when empty, so a frame without it is byte-identical to v1.0's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<f32>,
}

/// One sample of process rows (`Message::LiveProcesses`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiveProcesses {
    /// Remote clock.
    pub ts_ms: i64,
    pub rows: Vec<WireProcess>,
}

/// One process at one sample. Every field has a default, so a sender that drops a field
/// it cannot read (a Linux agent without energy) still decodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WireProcess {
    pub pid: i32,
    /// Process start, microseconds since the Unix epoch; with `pid` it names the process.
    pub start_time_us: i64,
    pub name: String,
    /// Percent of one core.
    pub cpu_pct: f32,
    pub mem_bytes: u64,
    pub compressed_bytes: Option<u64>,
    pub threads: u32,
    pub idle_wakeups_per_s: f32,
    /// Watts (approximate).
    pub energy: f32,
    pub disk_read_bps: f32,
    pub disk_write_bps: f32,
    pub user: String,
    /// Bytes per second received; absent when not sampled (D-081). Omitted when `None`,
    /// so a row without it encodes as v1.0's did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_rx_bps: Option<f32>,
    /// Bytes per second sent; see `net_rx_bps`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_tx_bps: Option<f32>,
    /// Percent of the whole GPU; absent when not sampled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_pct: Option<f32>,
}

/// A few headline values for fleet cards (v4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostSummary {
    pub ts_ms: i64,
    pub online: bool,
    pub headline: Vec<(SeriesKey, f32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncRequest {
    /// A persisted tier ([`Tier::is_persisted`]).
    pub tier: Tier,
    /// `None` means from the earliest row.
    pub after: Option<kelvo_schema::Cursor>,
    pub max_rows: u32,
}

/// One page of persisted rows after a cursor.
///
/// Each row kind (`rows`, `gaps`, `events`, and any kind a later version adds) is sent
/// only when its `rows.*` feature was negotiated, so a receiver never advances its cursor
/// past rows it could not read (D-041, D-064). A sender leaves an unnegotiated kind's
/// list empty; a receiver that negotiates more kinds later resyncs from the start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncPage {
    pub tier: Tier,
    /// The sender's `db_instance_uuid`.
    pub epoch: Uuid,
    /// Every layout referenced by `rows` in this page.
    pub layouts: Vec<WireLayout>,
    pub rows: Vec<WireBucket>,
    pub gaps: Vec<WireGap>,
    pub events: Vec<WireEvent>,
    /// Highest `seq` in this page; the receiver's next cursor.
    pub last_seq: i64,
    pub more: bool,
}

/// One closed bucket of a persisted tier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireBucket {
    pub seq: i64,
    /// Bucket start, ms epoch, sender's clock.
    pub bucket_ts: i64,
    /// Refers to a [`WireLayout`] in the same page.
    pub layout_no: u32,
    /// `(min, max, avg)` triples, one per series in layout order: the same values as the
    /// store's `blob` column. Length is `3 * layout.series.len()`.
    pub stats: Vec<f32>,
}

impl WireBucket {
    /// `(min, max, avg)` of the series at `index` in the layout.
    pub fn stat(&self, index: usize) -> Option<(f32, f32, f32)> {
        let s = self.stats.get(index * 3..index * 3 + 3)?;
        match *s {
            [min, max, avg] => Some((min, max, avg)),
            _ => None,
        }
    }
}

/// A gap row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGap {
    pub seq: i64,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    /// `None` for whole-host gaps; set only for `module_disabled`.
    pub module: Option<Module>,
    pub reason: GapReason,
}

/// An event row (v1.2 detectors). `kind` stays a string so a newer peer's event kinds
/// pass through; `payload` is the store's CBOR blob, carried as a CBOR byte string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEvent {
    pub seq: i64,
    pub ts_ms: i64,
    pub kind: String,
    #[serde(with = "cbor_bytes")]
    pub payload: Vec<u8>,
}

/// Protocol-level error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// No protocol version both sides speak.
    IncompatibleVersion,
    /// A message arrived that the receiver did not expect in this state.
    UnexpectedMessage,
    /// A message decoded but its contents are invalid (unknown layout, bad lengths).
    Malformed,
    /// The sender failed internally.
    Internal,
    /// A code this build does not know, from a newer peer (D-040). The message text still
    /// says what went wrong.
    #[serde(other)]
    Unknown,
}

/// Why a frame's values cannot be matched to a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LayoutMismatch {
    #[error("frame refers to layout {frame} but the layout is {layout}")]
    LayoutNo { frame: u32, layout: u32 },
    #[error("frame has {values} values for {series} series")]
    Length { series: usize, values: usize },
}

impl LiveFrame {
    /// The frame's values paired with their keys, skipping series whose metric this build
    /// does not know (or whose labels do not match its definition). This is how a v1
    /// receiver reads frames from a newer sender.
    pub fn known_values<'a>(
        &'a self,
        layout: &'a WireLayout,
        catalog: &'a Catalog,
    ) -> Result<impl Iterator<Item = (&'a SeriesKey, f32)> + 'a, LayoutMismatch> {
        if self.layout_no != layout.layout_no {
            return Err(LayoutMismatch::LayoutNo {
                frame: self.layout_no,
                layout: layout.layout_no,
            });
        }
        if self.values.len() != layout.series.len() {
            return Err(LayoutMismatch::Length {
                series: layout.series.len(),
                values: self.values.len(),
            });
        }
        Ok(layout
            .series
            .iter()
            .zip(self.values.iter().copied())
            .filter(move |(k, _)| catalog.validate(k).is_ok()))
    }
}

/// Serializes `Vec<u8>` as a CBOR byte string instead of an array of integers.
mod cbor_bytes {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a byte string")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
        }
        d.deserialize_byte_buf(V)
    }
}
