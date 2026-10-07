//! The Kelvo sync protocol: length-prefixed framing, the CBOR codec (via `ciborium`),
//! the [`Message`] enum, and the handshake with version negotiation.
//!
//! Depends on `kelvo-schema` only. Never touches the store or the collectors. The webview
//! never sees CBOR; nothing in v1 exercises this crate over a network, which is why the
//! round-trip and skew fixtures in `tests/` run in CI from v1.
//!
//! Skew rules a receiver relies on:
//! - an unknown message type decodes as [`Message::Unknown`] (log and skip);
//! - unknown map fields inside a known message are ignored (serde's default);
//! - unknown enum values (a new module, gap reason, capability state, tier, error code)
//!   decode as that enum's `Unknown` variant, and consumers skip them (D-040);
//! - series with an unknown `metric_id` are carried and then skipped
//!   ([`LiveFrame::known_values`]);
//! - a `SyncPage` row kind is sent only when both sides listed its `rows.*` feature
//!   ([`Negotiated::sync_kinds`]), because an unknown field is dropped silently and the
//!   receiver's cursor would move past rows it never stored (D-064).

mod codec;
mod handshake;
mod message;

pub use codec::{
    CodecError, KNOWN_MESSAGE_TAGS, MAX_FRAME_LEN, decode_message, encode_frame, encode_message,
    read_frame, write_frame,
};
pub use handshake::{HandshakeError, Negotiated, Offer, local_features, negotiate};
pub use message::{
    ErrorCode, Hello, HelloAck, HostSummary, LayoutMismatch, LiveFrame, LiveProcesses, LiveTier,
    Message, SyncPage, SyncRequest, WireBucket, WireEvent, WireGap, WireLayout, WireProcess,
};

/// The protocol version this build speaks.
pub const PROTO_VERSION: u16 = 1;
/// The oldest protocol version this build still speaks.
pub const MIN_COMPATIBLE: u16 = 1;
