//! The error every command returns (`.claude/rules/rust.md`, Error handling). Serialized
//! with a `kind` tag so the frontend can switch on it.

use kelvo_schema::{HostId, JsSafeInt};
use kelvo_store::StoreError;
use kelvo_store::rusqlite::ErrorCode;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandError {
    #[error("update: {message}")]
    Update { message: String },
    /// SSH login to the other Mac failed (key, agent or account).
    #[error("{message}")]
    RemoteAuth { message: String },
    /// The other Mac's host key isn't trusted yet, or needs checking in Terminal.
    #[error("{message}")]
    HostKeyUnknown { message: String },
    /// The other Mac can't run this Kelvo (architecture, macOS or protocol version).
    #[error("{message}")]
    RemoteIncompatible { message: String },
    /// The Mac that answered is not the one this host was added as.
    #[error("{message}")]
    RemoteIdentity { message: String },
    /// The other Mac could not be reached, or the connection broke.
    #[error("{message}")]
    RemoteUnreachable { message: String },
    /// A bounded, sanitized Docker monitoring error.
    #[error("Docker: {message}")]
    Docker { message: String },
    /// The requested container, retained run or storage resource no longer exists.
    #[error("Docker: {message}")]
    DockerNotFound { message: String },
    /// The preview or inventory changed before the action could be performed.
    #[error("Docker: {message}")]
    DockerSnapshotExpired { message: String },
    /// A bounded request exceeded the service's supported resource limit.
    #[error("Docker: {message}")]
    DockerResourceLimit { message: String },
    /// No host with this id is registered.
    #[error("unknown host {host}")]
    UnknownHost { host: HostId },
    /// The host is not this Mac, and the data asked for is kept only on the host it
    /// describes: per-app network bytes are not synced (D-089).
    #[error("host {host} does not share this data")]
    RemoteHost { host: HostId },
    /// There is no history store: it could not be opened at launch, the local host could
    /// not be registered in it, or a reset failed. Kelvo runs live-only. `reason` says
    /// why when it is known; `reset_history` can recover from everything but `locked`.
    #[error("history is unavailable")]
    HistoryUnavailable {
        #[specta(optional)]
        reason: Option<HistoryUnavailableReason>,
    },
    /// The database is busy (another writer holds it, or a lock wait timed out). Worth one
    /// retry.
    #[error("history store is busy: {message}")]
    StoreBusy { message: String },
    /// The database file is damaged. `reset_history` moves it aside and starts a new one.
    #[error("history store is corrupt: {message}")]
    StoreCorrupt { message: String },
    /// A newer Kelvo wrote the database; this build cannot read it.
    #[error("history store schema {found} is newer than this build supports ({supported})")]
    StoreTooNew {
        #[specta(type = JsSafeInt)]
        found: i64,
        #[specta(type = JsSafeInt)]
        supported: i64,
    },
    /// Any other history query or write failure.
    #[error("history store: {message}")]
    Store { message: String },
    /// A disk space scan could not start, or its report is gone.
    #[error("{message}")]
    DiskSpace { message: String },
    /// A bug or a crashed worker inside Kelvo (a command's task panicked). Not retryable.
    #[error("internal error: {message}")]
    Internal { message: String },
    /// The settings patch produced settings that fail validation. Nothing changed.
    #[error("invalid settings: {message}")]
    InvalidSettings { message: String },
    /// The settings file could not be written. Nothing changed.
    #[error("cannot save settings: {message}")]
    SettingsNotSaved { message: String },
    /// Registering or unregistering the login item failed. Nothing changed.
    #[error("launch at login: {message}")]
    LaunchAtLogin { message: String },
    /// An argument is out of range (a negative span, an empty selector list).
    #[error("invalid argument: {message}")]
    InvalidArgument { message: String },
    /// A window could not be created or shown.
    #[error("window: {message}")]
    Window { message: String },
    /// The export file could not be created or written (no permission, disk full). No
    /// partial file is left behind.
    #[error("export: {message}")]
    Export { message: String },
    /// The public address lookup failed: offline, the service unreachable or timing
    /// out, or an answer that is not an address (a captive portal).
    #[error("public address: {message}")]
    PublicIp { message: String },
}

impl CommandError {
    /// `history_unavailable` with no known reason.
    pub const fn history_unavailable() -> Self {
        CommandError::HistoryUnavailable { reason: None }
    }
}

pub use kelvo_engine::HistoryUnavailableReason;

fn sqlite_code(e: &StoreError) -> Option<ErrorCode> {
    match e {
        StoreError::Sqlite(e) => e.sqlite_error_code(),
        _ => None,
    }
}

fn is_corrupt(e: &StoreError) -> bool {
    matches!(e, StoreError::Corrupt(_) | StoreError::Cbor(_))
        || matches!(
            sqlite_code(e),
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
        )
}

fn is_busy(e: &StoreError) -> bool {
    matches!(e, StoreError::Locked { .. })
        || matches!(
            sqlite_code(e),
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
        )
}

impl From<StoreError> for CommandError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::UnknownHost(host) => CommandError::UnknownHost { host },
            StoreError::WriterGone => CommandError::history_unavailable(),
            StoreError::TooNew { found, supported } => {
                CommandError::StoreTooNew { found, supported }
            }
            e if is_busy(&e) => CommandError::StoreBusy {
                message: e.to_string(),
            },
            e if is_corrupt(&e) => CommandError::StoreCorrupt {
                message: e.to_string(),
            },
            e => CommandError::Store {
                message: e.to_string(),
            },
        }
    }
}

impl From<kelvo_engine::HistoryStoreError> for CommandError {
    fn from(error: kelvo_engine::HistoryStoreError) -> Self {
        match error {
            kelvo_engine::HistoryStoreError::Unavailable { reason } => {
                Self::HistoryUnavailable { reason }
            }
            kelvo_engine::HistoryStoreError::Store(error) => error.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kelvo_store::rusqlite;

    use super::*;

    fn sqlite(code: i32) -> StoreError {
        StoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            None,
        ))
    }

    #[test]
    fn store_errors_map_to_typed_command_errors() {
        let kind = |e: StoreError| {
            serde_json::to_value(CommandError::from(e)).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(
            kind(StoreError::Locked {
                path: PathBuf::from("h.sqlite")
            }),
            "store_busy"
        );
        assert_eq!(kind(sqlite(rusqlite::ffi::SQLITE_BUSY)), "store_busy");
        assert_eq!(kind(sqlite(rusqlite::ffi::SQLITE_CORRUPT)), "store_corrupt");
        assert_eq!(kind(sqlite(rusqlite::ffi::SQLITE_NOTADB)), "store_corrupt");
        assert_eq!(
            kind(StoreError::Corrupt("bad blob".into())),
            "store_corrupt"
        );
        assert_eq!(
            kind(StoreError::TooNew {
                found: 9,
                supported: 2
            }),
            "store_too_new"
        );
        assert_eq!(kind(StoreError::WriterGone), "history_unavailable");
        assert_eq!(kind(sqlite(rusqlite::ffi::SQLITE_FULL)), "store");
    }

    #[test]
    fn unavailable_reasons_say_why() {
        let json =
            |e: &StoreError| serde_json::to_value(HistoryUnavailableReason::from(e)).unwrap();
        assert_eq!(
            json(&StoreError::Locked {
                path: PathBuf::from("h")
            }),
            serde_json::json!({ "kind": "locked" })
        );
        assert_eq!(
            json(&StoreError::TooNew {
                found: 9,
                supported: 2
            }),
            serde_json::json!({ "kind": "too_new", "found": 9, "supported": 2 })
        );
        assert_eq!(
            json(&sqlite(rusqlite::ffi::SQLITE_NOTADB))["kind"],
            "corrupt"
        );
        assert_eq!(json(&sqlite(rusqlite::ffi::SQLITE_IOERR))["kind"], "failed");
        // With a reason the error carries it (`null` when unknown).
        let e = CommandError::HistoryUnavailable {
            reason: Some(HistoryUnavailableReason::Locked),
        };
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            serde_json::json!({ "kind": "history_unavailable", "reason": { "kind": "locked" } })
        );
    }
}
