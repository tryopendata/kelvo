//! Remote queries (D-117): what `Message::Request` asks and `Message::Reply` answers. A
//! one-way door like the rest of the wire: variants and fields are added, never renamed.
//!
//! A query or answer kind this build does not know decodes as `Unknown`, so a host asked
//! something new answers `Answer::Error { code: Unsupported }` and the session goes on.

use kelvo_schema::disk_space::{
    SpaceFile, SpaceReport, SpaceScanStatus, SpaceSubtree, SpaceTarget, SpaceVolume,
};
use kelvo_schema::usage::{UsageByApp, UsageKey};
use serde::{Deserialize, Serialize};

use crate::ErrorCode;

/// Adjacently tagged like [`crate::Message`]: `{"q": "UsageByApp", "a": {...}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "q", content = "a")]
pub enum Query {
    /// Per-app use over a range, from the host's in-memory usage ring: the last hour.
    UsageByApp {
        from_ms: i64,
        to_ms: i64,
        by: UsageKey,
        limit: u32,
    },
    /// The host's space report service (D-114), one call per `SpaceService` method.
    SpaceVolumes,
    SpaceStart {
        volume: String,
        include: Vec<String>,
    },
    SpaceStatus,
    SpaceCancel,
    /// Cancels and drops the scan this session started, if it did (the controller's
    /// dashboard closed).
    SpaceClear,
    SpaceReport,
    SpaceSubtree {
        report: u32,
        node: Option<u32>,
    },
    SpaceLargestFiles {
        report: u32,
    },
    SpacePath {
        report: u32,
        target: SpaceTarget,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "a", content = "v")]
pub enum Answer {
    UsageByApp(UsageByApp),
    SpaceVolumes(Vec<SpaceVolume>),
    SpaceStatus(SpaceScanStatus),
    SpaceReport(Option<SpaceReport>),
    SpaceSubtree(SpaceSubtree),
    SpaceFiles(Vec<SpaceFile>),
    SpacePath(String),
    /// Done, with nothing to return (`SpaceCancel`, `SpaceClear`).
    Done,
    /// The query failed on the host; `message` is for the user.
    Error {
        code: ErrorCode,
        message: String,
    },
    #[serde(other)]
    Unknown,
}

/// An adjacently tagged enum with an `Unknown` variant for kinds it cannot decode.
pub(crate) trait Tagged: serde::de::DeserializeOwned {
    fn unknown() -> Self;
}

impl Tagged for Query {
    fn unknown() -> Self {
        Self::Unknown
    }
}

impl Tagged for Answer {
    fn unknown() -> Self {
        Self::Unknown
    }
}

/// Decodes `T`, or `T::unknown()` when it does not decode: a kind this build does not
/// know, or a known kind whose body changed shape. `#[serde(other)]` alone accepts an
/// unknown tag only without content (see [`crate::decode_message`]). The request id sits
/// outside the query, so a call that cannot be read fails alone and the session goes on.
pub(crate) fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Tagged,
{
    let value = ciborium::Value::deserialize(d)?;
    Ok(value.deserialized::<T>().unwrap_or_else(|_| T::unknown()))
}
