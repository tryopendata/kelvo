//! The wire types of the last build before D-116, vendored from the merge base
//! (`git show 2b0b7cb:crates/kelvo-proto/src/message.rs` and
//! `crates/kelvo-schema/src/{host,caps,catalog,compat}.rs`), so a skew test decodes
//! D-116 fixtures with that build's real decoder instead of a model of it.
//!
//! Every vendored type keeps its fields and serde attributes exactly. Trimmed: the
//! `specta` derives and attributes (no serde effect), methods the decode does not use,
//! doc comments, and [`Message`]'s variants other than the two the fixtures carry
//! (each variant decodes independently under the adjacent `t`/`c` tagging). Paths into
//! `crate::compat` now go through this module.
//!
//! Never edit these to make a test pass: they are what an old peer runs.

// Vendored decode-only types: most fields are only checked by some tests, and the
// `Serialize` derives stay to keep the definitions as they shipped.
#![allow(dead_code)]

pub mod caps;
pub mod catalog;
pub mod compat;
pub mod host;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use caps::{Capabilities, ModuleCap, UnsupportedReason};
pub use catalog::Module;
pub use host::{ClusterInfo, CoreKind, HostId, HostIdentity, OsKind};

// The old build sent `Hello` unboxed; boxing it here would no longer be its type.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Message {
    Hello(Hello),
    CapabilitiesChanged(Capabilities),

    /// A newer peer sent a message type this build does not know: log and skip.
    #[serde(other)]
    Unknown,
}

/// First message from each side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub proto_version: u16,
    pub min_compatible: u16,
    pub app_version: String,
    pub host: HostIdentity,
    pub db_instance_uuid: Uuid,
    pub capabilities: Capabilities,
    pub features: BTreeSet<String>,
}
