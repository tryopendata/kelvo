//! Convert domain failures at the IPC boundary, without inspecting display strings.
use crate::error::CommandError;
use kelvo_engine::docker::{DockerServiceError, storage::DockerStorageError};

pub(super) fn task_failed(message: &'static str, source: impl std::fmt::Display) -> CommandError {
    tracing::error!(error = %source, message, "Docker command task failed");
    CommandError::Internal {
        message: message.into(),
    }
}

impl From<DockerServiceError> for CommandError {
    fn from(error: DockerServiceError) -> Self {
        match error {
            DockerServiceError::InvalidRequest(message) => Self::InvalidArgument {
                message: message.into(),
            },
            DockerServiceError::NotFound(message) => Self::DockerNotFound {
                message: message.into(),
            },
            DockerServiceError::SnapshotExpired(message) => Self::DockerSnapshotExpired {
                message: message.into(),
            },
            DockerServiceError::ResourceLimit(message) => Self::DockerResourceLimit {
                message: message.into(),
            },
            DockerServiceError::Store(source) => source.into(),
            DockerServiceError::Collector(source) => Self::Docker {
                message: source.to_string(),
            },
            DockerServiceError::Metadata { message, .. } => Self::StoreCorrupt {
                message: message.into(),
            },
            DockerServiceError::Serialization(source) => Self::Internal {
                message: source.to_string(),
            },
            DockerServiceError::WriteFailed(message) => Self::Store { message },
            DockerServiceError::Unavailable(message) => Self::Docker {
                message: message.into(),
            },
        }
    }
}
impl From<DockerStorageError> for CommandError {
    fn from(error: DockerStorageError) -> Self {
        let message = error.to_string();
        match error {
            DockerStorageError::Selection | DockerStorageError::VolumeConfirmation => {
                Self::InvalidArgument { message }
            }
            DockerStorageError::Review => Self::DockerSnapshotExpired { message },
            DockerStorageError::Busy => Self::DockerResourceLimit { message },
            DockerStorageError::Operation => Self::DockerNotFound { message },
            DockerStorageError::Unavailable(_) => Self::Docker { message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_store::StoreError;

    fn kind(error: impl Into<CommandError>) -> String {
        serde_json::to_value(error.into()).unwrap()["kind"]
            .as_str()
            .unwrap()
            .into()
    }

    #[test]
    fn query_error_categories_survive_ipc_serialization() {
        // Identical diagnostics must still produce different machine-readable results.
        assert_eq!(
            kind(DockerServiceError::InvalidRequest("same")),
            "invalid_argument"
        );
        assert_eq!(
            kind(DockerServiceError::NotFound("same")),
            "docker_not_found"
        );
        assert_eq!(
            kind(DockerServiceError::SnapshotExpired("same")),
            "docker_snapshot_expired"
        );
        assert_eq!(
            kind(DockerServiceError::ResourceLimit("same")),
            "docker_resource_limit"
        );
        assert_eq!(kind(DockerServiceError::Unavailable("same")), "docker");
    }

    #[test]
    fn docker_store_failures_use_existing_store_recovery_categories() {
        assert_eq!(
            kind(DockerServiceError::Store(StoreError::WriterGone)),
            "history_unavailable"
        );
        assert_eq!(
            kind(DockerServiceError::Store(StoreError::Corrupt(
                "bad metadata".into()
            ))),
            "store_corrupt"
        );
        assert_eq!(
            kind(DockerServiceError::Store(StoreError::TooNew {
                found: 9,
                supported: 2
            })),
            "store_too_new"
        );
    }

    #[test]
    fn cleanup_errors_distinguish_revalidation_from_busy_and_missing_results() {
        assert_eq!(kind(DockerStorageError::Review), "docker_snapshot_expired");
        assert_eq!(kind(DockerStorageError::Busy), "docker_resource_limit");
        assert_eq!(kind(DockerStorageError::Operation), "docker_not_found");
        assert_eq!(
            kind(DockerStorageError::VolumeConfirmation),
            "invalid_argument"
        );
        assert_eq!(kind(DockerStorageError::Selection), "invalid_argument");
    }
}
