use std::path::PathBuf;

use kelvo_schema::{GapError, HostId, Tier};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Another `Store` holds the writer lock on this file: a second app instance, a dev
    /// build sharing the installed app's data, or (v4) an agent (architecture.md infra 8).
    #[error("history database {} is in use by another Kelvo process", path.display())]
    Locked { path: PathBuf },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    TooNew { found: i64, supported: i64 },
    #[error("corrupt database: {0}")]
    Corrupt(String),
    #[error("host {0} is not in the store")]
    UnknownHost(HostId),
    /// A non-local host was upserted with the local host's id: a cloned `host-id` on
    /// another Mac. Merging would demote this Mac's own host and mix the two (D-071).
    #[error("host {0} is this Mac's own id; a remote host cannot use it")]
    HostConflict(HostId),
    #[error("tier {0:?} is not persisted")]
    NotPersisted(Tier),
    #[error("bucket has {stats} stats for {series} series (expected 3 per series)")]
    StatsLength { series: usize, stats: usize },
    #[error("layout lists series {0} twice")]
    DuplicateSeries(String),
    #[error("page row refers to layout {0}, which the page does not define")]
    PageLayout(u32),
    #[error("invalid gap: {0}")]
    Gap(#[from] GapError),
    /// A bucket timestamp that is not on its tier's grid.
    #[error("bucket at {ts_ms} is not aligned to {width_ms} ms")]
    Misaligned { ts_ms: i64, width_ms: i64 },
    #[error("cbor: {0}")]
    Cbor(String),
    /// The writer thread has stopped, or is stopping: a long operation (pruning) that
    /// sees a shutdown queued behind it ends early with this.
    #[error("the store writer thread has stopped")]
    WriterGone,
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;
