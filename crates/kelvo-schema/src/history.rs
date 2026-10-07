//! Tiers, sync cursors and gaps (architecture.md infra 4 and the Store section).
//!
//! Every persisted row (tier buckets, gaps, events, process snapshots) carries a `seq` that
//! is monotonic within one database file. A [`Cursor`] is `(epoch, seq)`, where `epoch` is
//! the remote database's `db_instance_uuid`; a changed epoch means the remote database
//! was recreated and the controller resyncs that host from scratch.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::catalog::Module;

/// A resolution of data. [`Tier::S10`], [`Tier::M1`] and [`Tier::M15`] are persisted and
/// synced; [`Tier::Live1s`] is the in-memory ring buffer and the live stream. M1 keeps the
/// last 7 days; older minutes are rolled down into M15 for the rest of the retention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Live1s,
    S10,
    M1,
    /// 15-minute buckets: minutes older than the M1 window, rolled down by the store.
    /// Older peers decode it as [`Tier::Unknown`] (D-040); it is synced only when both
    /// sides negotiated [`SyncRowKind::M15`].
    M15,
    /// A tier this build does not know, named by a newer peer (D-040). It has no bucket
    /// width and is never persisted; a sync request for it is answered with an error.
    Unknown,
}

impl Tier {
    /// The tiers that have a table and a sync cursor.
    pub const PERSISTED: [Tier; 3] = [Tier::S10, Tier::M1, Tier::M15];

    /// Every known tier (not `Unknown`).
    pub const ALL: [Tier; 4] = [Tier::Live1s, Tier::S10, Tier::M1, Tier::M15];

    /// The snake_case text form, as serialized.
    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::Live1s => "live1s",
            Tier::S10 => "s10",
            Tier::M1 => "m1",
            Tier::M15 => "m15",
            Tier::Unknown => "unknown",
        }
    }

    /// Bucket width in milliseconds. Buckets start at wall-clock multiples of this width.
    /// `Live1s` is the default base tick; the real live interval follows settings. `None`
    /// for [`Tier::Unknown`].
    pub const fn bucket_ms(self) -> Option<i64> {
        match self {
            Tier::Live1s => Some(1_000),
            Tier::S10 => Some(10_000),
            Tier::M1 => Some(60_000),
            Tier::M15 => Some(900_000),
            Tier::Unknown => None,
        }
    }

    pub const fn is_persisted(self) -> bool {
        matches!(self, Tier::S10 | Tier::M1 | Tier::M15)
    }

    /// Start of the bucket containing `ts_ms` (floor to the bucket width, also for
    /// negative timestamps). `None` for [`Tier::Unknown`].
    pub const fn bucket_start(self, ts_ms: i64) -> Option<i64> {
        match self.bucket_ms() {
            Some(w) => Some(ts_ms - ts_ms.rem_euclid(w)),
            None => None,
        }
    }
}

crate::compat::text_enum_deserialize!(Tier);

/// A sync position in a remote database: everything with `seq > self.seq` in database
/// `epoch` has not been seen yet. Kept per `(host, tier)` on the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct Cursor {
    /// The remote `db_instance_uuid`.
    pub epoch: Uuid,
    /// Exported to TS as `number`; see [`crate::JS_SAFE_INT_MAX`].
    #[specta(type = crate::JsSafeInt)]
    pub seq: i64,
}

/// A kind of row a sync page carries. Every kind is gated on a negotiated `Hello.features`
/// entry, [`SyncRowKind::feature`]: a sender puts only kinds both sides listed into a
/// page, and the receiver's cursor remembers which kinds it covered. A receiver that
/// later negotiates a kind its cursor did not cover reads that tier again from the start,
/// so rows of a kind it could not receive before are never skipped (D-041, D-064).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SyncRowKind {
    /// Tier buckets (`rows` in a page).
    Buckets,
    /// Gap rows; they ride the M1 cursor.
    Gaps,
    /// Event rows; they ride the M1 cursor.
    Events,
    /// Buckets of the [`Tier::M15`] tier, which builds before it do not have. Also tells
    /// the sender that the receiver takes M15: without it, minutes the sender rolled down
    /// into M15 count as pruned for the receiver's M1 cursor, so it records a
    /// `truncated` gap instead of a silent hole.
    M15,
}

impl SyncRowKind {
    pub const ALL: [SyncRowKind; 4] = [
        SyncRowKind::Buckets,
        SyncRowKind::Gaps,
        SyncRowKind::Events,
        SyncRowKind::M15,
    ];

    /// The `Hello.features` entry that enables this kind.
    pub const fn feature(self) -> &'static str {
        match self {
            SyncRowKind::Buckets => "rows.buckets",
            SyncRowKind::Gaps => "rows.gaps",
            SyncRowKind::Events => "rows.events",
            SyncRowKind::M15 => "rows.m15",
        }
    }

    pub fn from_feature(s: &str) -> Option<SyncRowKind> {
        SyncRowKind::ALL.into_iter().find(|k| k.feature() == s)
    }

    const fn bit(self) -> u8 {
        match self {
            SyncRowKind::Buckets => 1,
            SyncRowKind::Gaps => 2,
            SyncRowKind::Events => 4,
            SyncRowKind::M15 => 8,
        }
    }
}

/// A set of [`SyncRowKind`]s: what a connection negotiated, or what a stored cursor
/// covered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SyncKinds(u8);

impl SyncKinds {
    pub const NONE: SyncKinds = SyncKinds(0);
    /// Every kind this build knows.
    pub const ALL: SyncKinds = SyncKinds(1 | 2 | 4 | 8);

    /// The kinds named by these feature strings; anything else is ignored.
    pub fn from_features<'a>(features: impl IntoIterator<Item = &'a str>) -> SyncKinds {
        features
            .into_iter()
            .filter_map(SyncRowKind::from_feature)
            .fold(SyncKinds::NONE, SyncKinds::with)
    }

    #[must_use]
    pub const fn with(self, kind: SyncRowKind) -> SyncKinds {
        SyncKinds(self.0 | kind.bit())
    }

    pub const fn contains(self, kind: SyncRowKind) -> bool {
        self.0 & kind.bit() != 0
    }

    /// Whether every kind in `other` is in `self`.
    pub const fn covers(self, other: SyncKinds) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn iter(self) -> impl Iterator<Item = SyncRowKind> {
        SyncRowKind::ALL
            .into_iter()
            .filter(move |k| self.contains(*k))
    }

    /// The feature strings of these kinds, for `Hello.features`.
    pub fn features(self) -> impl Iterator<Item = &'static str> {
        self.iter().map(SyncRowKind::feature)
    }

    /// The text kept with a stored cursor: feature strings joined by `,`.
    pub fn to_stored(self) -> String {
        self.features().collect::<Vec<_>>().join(",")
    }

    /// Inverse of [`SyncKinds::to_stored`]. Unknown entries are dropped, so a cursor
    /// written by a newer build covers at most what this build knows.
    pub fn from_stored(s: &str) -> SyncKinds {
        SyncKinds::from_features(s.split(',').filter(|f| !f.is_empty()))
    }
}

/// Why nothing was measured for a span. Stored as snake_case text in `gaps.reason`
/// ([`GapReason::as_str`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GapReason {
    /// The machine slept. Written by the engine on `WillSleep`, closed on `DidWake`.
    Sleep,
    /// Kelvo was not running. Written by the store on startup, from the last persisted
    /// bucket to now.
    AppNotRunning,
    /// The user paused sampling.
    Paused,
    /// A module was switched off in Settings. The only reason that sets [`Gap::module`].
    ModuleDisabled,
    /// (v4) The agent pruned past the controller's cursor.
    Truncated,
    /// (v4) A remote connection dropped. Provisional until sync catches up.
    SourceOffline,
    /// The wall clock stepped (a manual change or an NTP step) while sampling. Covers the
    /// wall-clock span the engine could not persist: a forward step's skipped span, or,
    /// after a backward step, the time until the clock is past what was already written
    /// (D-064).
    ClockChanged,
    /// A batch of history failed to commit and its rows were lost. Written over the lost
    /// span by the next successful commit (D-064).
    WriteFailed,
    /// A reason this build does not know, from a newer peer or a newer build's database
    /// (D-040). Still a gap: charts draw it as a generic gap band. Not in
    /// [`GapReason::ALL`]; stored as `"unknown"`.
    Unknown,
}

impl GapReason {
    pub const ALL: [GapReason; 8] = [
        GapReason::Sleep,
        GapReason::AppNotRunning,
        GapReason::Paused,
        GapReason::ModuleDisabled,
        GapReason::Truncated,
        GapReason::SourceOffline,
        GapReason::ClockChanged,
        GapReason::WriteFailed,
    ];

    /// The text stored in `gaps.reason`.
    pub const fn as_str(self) -> &'static str {
        match self {
            GapReason::Sleep => "sleep",
            GapReason::AppNotRunning => "app_not_running",
            GapReason::Paused => "paused",
            GapReason::ModuleDisabled => "module_disabled",
            GapReason::Truncated => "truncated",
            GapReason::SourceOffline => "source_offline",
            GapReason::ClockChanged => "clock_changed",
            GapReason::WriteFailed => "write_failed",
            GapReason::Unknown => "unknown",
        }
    }

    /// Inverse of [`GapReason::as_str`] for the known reasons. `None` for anything else.
    pub fn parse(s: &str) -> Option<GapReason> {
        GapReason::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// Reads `gaps.reason` text: any value this build does not know is
    /// [`GapReason::Unknown`], because a gap with an unfamiliar reason is still a gap.
    pub fn from_stored(s: &str) -> GapReason {
        GapReason::parse(s).unwrap_or(GapReason::Unknown)
    }
}

crate::compat::text_enum_deserialize!(GapReason);

/// An explicit span with no data. Charts split series at gaps; nothing interpolates
/// across one.
///
/// `module` is `None` for a whole-host gap and `Some` only for
/// [`GapReason::ModuleDisabled`] (or a [`GapReason::Unknown`] reason from a newer peer,
/// which may scope a gap to a module), so charts for other modules ignore it. [`Gap::new`]
/// enforces that pairing; the fields are public for decoding and the store, so callers
/// that build one by hand should call [`Gap::validate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Gap {
    /// Millisecond epoch, host clock. Exported to TS as `number`.
    #[specta(type = crate::JsSafeInt)]
    pub start_ms: i64,
    /// `None` while the gap is still open (asleep now, paused now).
    #[specta(type = Option<crate::JsSafeInt>)]
    pub end_ms: Option<i64>,
    pub module: Option<Module>,
    pub reason: GapReason,
}

impl Gap {
    pub fn new(
        start_ms: i64,
        end_ms: Option<i64>,
        module: Option<Module>,
        reason: GapReason,
    ) -> Result<Self, GapError> {
        let gap = Self {
            start_ms,
            end_ms,
            module,
            reason,
        };
        gap.validate()?;
        Ok(gap)
    }

    /// A whole-host gap (any reason except `ModuleDisabled`).
    pub fn host(start_ms: i64, end_ms: Option<i64>, reason: GapReason) -> Result<Self, GapError> {
        Self::new(start_ms, end_ms, None, reason)
    }

    /// A `module_disabled` gap for one module.
    pub fn module_disabled(start_ms: i64, end_ms: Option<i64>, module: Module) -> Self {
        Self {
            start_ms,
            end_ms,
            module: Some(module),
            reason: GapReason::ModuleDisabled,
        }
    }

    pub fn validate(&self) -> Result<(), GapError> {
        match (self.reason, self.module) {
            (GapReason::ModuleDisabled, None) => return Err(GapError::ModuleRequired),
            (r, Some(_)) if r != GapReason::ModuleDisabled && r != GapReason::Unknown => {
                return Err(GapError::ModuleNotAllowed(r));
            }
            _ => {}
        }
        if let Some(end) = self.end_ms
            && end < self.start_ms
        {
            return Err(GapError::EndBeforeStart);
        }
        Ok(())
    }

    /// Whether the gap blanks data of `module`: whole-host gaps affect every module.
    pub fn affects(&self, module: Module) -> bool {
        self.module.is_none_or(|m| m == module)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GapError {
    #[error("a module_disabled gap must name its module")]
    ModuleRequired,
    #[error("only module_disabled gaps name a module, not {0:?}")]
    ModuleNotAllowed(GapReason),
    #[error("gap ends before it starts")]
    EndBeforeStart,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_start_aligns_to_wall_clock() {
        assert_eq!(
            Tier::S10.bucket_start(1_790_000_012_345),
            Some(1_790_000_010_000)
        );
        assert_eq!(
            Tier::M1.bucket_start(1_790_000_012_345),
            Some(1_789_999_980_000)
        );
        assert_eq!(Tier::M1.bucket_start(-1), Some(-60_000));
        assert_eq!(
            Tier::M15.bucket_start(1_790_000_012_345),
            Some(1_789_999_200_000)
        );
        assert!(Tier::M15.is_persisted());
        assert_eq!(Tier::Unknown.bucket_start(5), None);
        assert!(!Tier::Unknown.is_persisted());
        assert!(!Tier::Live1s.is_persisted());
        assert!(Tier::PERSISTED.iter().all(|t| t.is_persisted()));
    }

    #[test]
    fn gap_reason_text_matches_store_ddl() {
        let texts: Vec<_> = GapReason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(
            texts,
            [
                "sleep",
                "app_not_running",
                "paused",
                "module_disabled",
                "truncated",
                "source_offline",
                "clock_changed",
                "write_failed"
            ]
        );
        assert_eq!(GapReason::from_stored("lid_closed"), GapReason::Unknown);
        assert_eq!(GapReason::from_stored("unknown"), GapReason::Unknown);
        for r in GapReason::ALL {
            assert_eq!(GapReason::parse(r.as_str()), Some(r));
            assert_eq!(GapReason::from_stored(r.as_str()), r);
            assert_eq!(
                serde_json::to_string(&r).unwrap(),
                format!("\"{}\"", r.as_str())
            );
        }
    }

    #[test]
    fn module_only_on_module_disabled() {
        assert!(Gap::host(0, Some(10), GapReason::Sleep).is_ok());
        assert_eq!(
            Gap::new(0, None, None, GapReason::ModuleDisabled),
            Err(GapError::ModuleRequired)
        );
        assert_eq!(
            Gap::new(0, None, Some(Module::Cpu), GapReason::Paused),
            Err(GapError::ModuleNotAllowed(GapReason::Paused))
        );
        assert_eq!(
            Gap::host(10, Some(5), GapReason::Sleep),
            Err(GapError::EndBeforeStart)
        );
        assert!(
            Gap::module_disabled(0, None, Module::Disk)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn sync_kinds_from_features_and_stored_text() {
        let k = SyncKinds::from_features(["rows.gaps", "zstd-pages", "rows.buckets"]);
        assert!(k.contains(SyncRowKind::Buckets) && k.contains(SyncRowKind::Gaps));
        assert!(!k.contains(SyncRowKind::Events));
        assert_eq!(k.to_stored(), "rows.buckets,rows.gaps");
        assert_eq!(SyncKinds::from_stored(&k.to_stored()), k);
        assert_eq!(SyncKinds::from_stored(""), SyncKinds::NONE);
        assert_eq!(
            SyncKinds::from_stored("rows.gaps,rows.future"),
            SyncKinds::NONE.with(SyncRowKind::Gaps)
        );
        assert!(SyncKinds::ALL.covers(k));
        assert!(!k.covers(SyncKinds::ALL));
        assert_eq!(SyncKinds::ALL.iter().count(), SyncRowKind::ALL.len());
    }

    #[test]
    fn affects() {
        let host = Gap::host(0, None, GapReason::Sleep).unwrap();
        let disk = Gap::module_disabled(0, None, Module::Disk);
        assert!(host.affects(Module::Cpu));
        assert!(disk.affects(Module::Disk));
        assert!(!disk.affects(Module::Cpu));
    }

    #[test]
    fn gap_json_shape() {
        let g = Gap::module_disabled(1, None, Module::Cpu);
        assert_eq!(
            serde_json::to_string(&g).unwrap(),
            r#"{"start_ms":1,"end_ms":null,"module":"cpu","reason":"module_disabled"}"#
        );
    }
}
