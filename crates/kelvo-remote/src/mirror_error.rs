//! Internal mirror failures remain typed until the connection status is published.
use kelvo_engine::HistoryStoreError;
use kelvo_store::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("Obsolete {operation} epoch")]
    EpochChanged { operation: &'static str },
    #[error("{0}")]
    ResourceLimit(&'static str),
    #[error(transparent)]
    Unavailable(HistoryStoreError),
    #[error(transparent)]
    Store(HistoryStoreError),
}

impl From<HistoryStoreError> for MirrorError {
    fn from(error: HistoryStoreError) -> Self {
        match error {
            error @ (HistoryStoreError::Unavailable { .. }
            | HistoryStoreError::Store(StoreError::WriterGone)) => Self::Unavailable(error),
            error => Self::Store(error),
        }
    }
}

impl From<StoreError> for MirrorError {
    fn from(error: StoreError) -> Self {
        HistoryStoreError::from(error).into()
    }
}
