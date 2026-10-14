//! Service failures retain their cause until the IPC boundary chooses a public error.
use kelvo_collect::docker::DockerError;
use kelvo_store::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum DockerServiceError {
    #[error("{0}")]
    InvalidRequest(&'static str),
    #[error("{0}")]
    NotFound(&'static str),
    #[error("{0}")]
    Unavailable(&'static str),
    #[error("{0}")]
    SnapshotExpired(&'static str),
    #[error("{0}")]
    ResourceLimit(&'static str),
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Collector(#[source] DockerError),
    #[error("{message}")]
    Metadata {
        message: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    Serialization(#[from] serde_json::Error),
    /// The asynchronous writer acknowledgement already contains a bounded diagnostic.
    #[error("{0}")]
    WriteFailed(String),
}

impl From<DockerError> for DockerServiceError {
    fn from(error: DockerError) -> Self {
        match error {
            DockerError::Endpoint => {
                Self::InvalidRequest("Only an absolute local Unix socket is supported")
            }
            DockerError::NotFound => Self::NotFound("Container no longer exists"),
            DockerError::Busy => Self::ResourceLimit("Docker request pool is busy; retry shortly"),
            source => Self::Collector(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn adapter_error_retains_source_but_display_remains_sanitized() {
        let api = DockerError::Api(std::io::Error::other("private daemon detail").into());
        let error = DockerServiceError::from(api);
        assert!(!error.to_string().contains("private daemon detail"));
        assert!(
            error
                .source()
                .unwrap()
                .downcast_ref::<DockerError>()
                .is_some()
        );
    }
    #[test]
    fn store_error_keeps_its_typed_cause_in_the_source_chain() {
        let error =
            DockerServiceError::from(StoreError::Io(std::io::Error::other("reader failed")));
        let source = error
            .source()
            .unwrap()
            .downcast_ref::<StoreError>()
            .unwrap();
        assert!(matches!(source, StoreError::Io(_)));
        assert!(
            source
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .is_some()
        );
    }
}
