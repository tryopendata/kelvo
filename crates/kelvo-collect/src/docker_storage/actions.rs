//! Revalidate reviewed identities before issuing one explicit action.
use super::*;

impl DockerStorageAdapter {
    pub async fn execute(
        &self,
        selection: &DockerStorageSelection,
        reviewed: &DockerStorageResource,
        can_dispatch: impl Fn() -> bool,
    ) -> DockerStorageItemResult {
        let result = |status, message: String, reclaimed_bytes| DockerStorageItemResult {
            selection: selection.clone(),
            status,
            message,
            reclaimed_bytes,
        };
        let Ok(_permit) = self.action_permit.try_acquire() else {
            return result(
                DockerStorageItemStatus::Skipped,
                "Another Docker action is running".into(),
                None,
            );
        };
        if selection.kind != reviewed.kind || selection.id != reviewed.id {
            return result(
                DockerStorageItemStatus::Skipped,
                "Selection does not match review".into(),
                None,
            );
        }
        let fresh = match self.scan(0).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return result(
                    DockerStorageItemStatus::Skipped,
                    "Cannot refresh references and identity".into(),
                    None,
                );
            }
        };
        let Some(current) = fresh
            .resources
            .iter()
            .find(|r| r.kind == selection.kind && r.id == selection.id)
        else {
            return result(
                DockerStorageItemStatus::Skipped,
                "Resource no longer exists".into(),
                None,
            );
        };
        let eligible = match selection.action {
            DockerStorageAction::Start => false,
            DockerStorageAction::Delete => current.can_delete && reviewed.can_delete,
            DockerStorageAction::Stop => current.can_stop && reviewed.can_stop,
        };
        if !eligible || current.fingerprint != reviewed.fingerprint || !fresh.complete {
            return result(
                DockerStorageItemStatus::Skipped,
                "Resource changed, is in use, or has incomplete evidence; review again".into(),
                None,
            );
        }
        // Volume names are reusable. This narrows, but cannot eliminate, Docker's check/delete race.
        if selection.kind == DockerStorageKind::Volume {
            let inspected = match self.scan_client.inspect_volume(&selection.id).await {
                Ok(v) => serde_json::to_value(v).ok(),
                Err(_) => None,
            };
            if inspected.as_ref().map(volume_fingerprint).as_ref() != Some(&reviewed.fingerprint) {
                return result(
                    DockerStorageItemStatus::Skipped,
                    "Volume identity changed or is unavailable".into(),
                    None,
                );
            }
        }
        if self
            .scan_client
            .info()
            .await
            .ok()
            .and_then(|info| info.id)
            .as_deref()
            != Some(self.engine.id.as_str())
        {
            return result(
                DockerStorageItemStatus::Skipped,
                "Docker daemon identity changed or is unavailable".into(),
                None,
            );
        }
        if !can_dispatch() {
            return result(
                DockerStorageItemStatus::Skipped,
                "Engine generation or availability changed".into(),
                None,
            );
        }
        self.dispatch(selection).await
    }
    /// Revalidate only the selected container. This lane never scans storage.
    pub async fn container_action(
        &self,
        container_id: &str,
        action: DockerContainerAction,
        can_dispatch: impl Fn() -> bool,
    ) -> DockerStorageItemResult {
        let selection = DockerStorageSelection {
            kind: DockerStorageKind::Container,
            id: container_id.into(),
            action: action.into(),
        };
        let skipped = |message: &str| DockerStorageItemResult {
            selection: selection.clone(),
            status: DockerStorageItemStatus::Skipped,
            message: message.into(),
            reclaimed_bytes: None,
        };
        let Ok(_permit) = self.action_permit.try_acquire() else {
            return skipped("Another Docker action is running");
        };
        if !valid_id(container_id) {
            return skipped("Container identity is invalid");
        }
        let inspected = match self
            .scan_client
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
        {
            Ok(container) => container,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                return skipped("Container no longer exists");
            }
            Err(_) => return skipped("Container information is unavailable; no action was sent"),
        };
        if inspected.id.as_deref() != Some(container_id) {
            return skipped("Container identity changed or is unavailable");
        }
        let lifecycle = inspected
            .state
            .and_then(|state| state.status)
            .map(|status| status.to_string());
        match action {
            DockerContainerAction::Start => {
                if !matches!(lifecycle.as_deref(), Some("exited" | "created")) {
                    return skipped("Only an exited or newly created container can be started");
                }
            }
            DockerContainerAction::Stop => {
                if !matches!(
                    lifecycle.as_deref(),
                    Some("running" | "restarting" | "paused")
                ) {
                    return skipped("Container is no longer running");
                }
                match inspected.host_config.and_then(|config| config.auto_remove) {
                    Some(false) => (),
                    Some(true) => {
                        return skipped(
                            "Stop is unavailable because Docker would automatically remove this container",
                        );
                    }
                    None => {
                        return skipped(
                            "Stop is unavailable because the container host configuration is unknown",
                        );
                    }
                }
            }
        }
        if self
            .scan_client
            .info()
            .await
            .ok()
            .and_then(|info| info.id)
            .as_deref()
            != Some(self.engine.id.as_str())
        {
            return skipped("Docker engine changed or is unavailable");
        }
        if !can_dispatch() {
            return skipped("Docker engine changed, paused, or disconnected before dispatch");
        }
        self.dispatch(&selection).await
    }
    async fn dispatch(&self, selection: &DockerStorageSelection) -> DockerStorageItemResult {
        let result = |status, message: String, reclaimed_bytes| DockerStorageItemResult {
            selection: selection.clone(),
            status,
            message,
            reclaimed_bytes,
        };
        let response = tokio::time::timeout(DEADLINE, self.mutate(selection)).await;
        match response {
            Ok(Ok(MutationOutcome::NoRemoval)) => result(
                DockerStorageItemStatus::Skipped,
                "Docker did not remove the selected cache; it may now be in use. Refresh and review again".into(),
                None,
            ),
            Ok(Ok(MutationOutcome::Accepted(bytes))) => result(
                DockerStorageItemStatus::Succeeded,
                "Docker accepted the action".into(),
                bytes,
            ),
            Ok(Err(bollard::errors::Error::DockerResponseServerError { status_code, message })) => {
                result(
                    if status_code == 404 || status_code == 409 {
                        DockerStorageItemStatus::Skipped
                    } else {
                        DockerStorageItemStatus::Failed
                    },
                    format!("Docker returned HTTP {status_code}: {}; no retry was sent", bounded(&message, 512)),
                    None,
                )
            }
            _ => result(
                DockerStorageItemStatus::OutcomeUnknown,
                "Connection or deadline ended after dispatch; rescan before further action".into(),
                None,
            ),
        }
    }
    async fn mutate(
        &self,
        selection: &DockerStorageSelection,
    ) -> Result<MutationOutcome, bollard::errors::Error> {
        match (selection.kind, selection.action) {
            (DockerStorageKind::Container, DockerStorageAction::Start) => {
                self.action_client
                    .start_container(&selection.id, None::<StartContainerOptions>)
                    .await?
            }
            (DockerStorageKind::Container, DockerStorageAction::Stop) => {
                self.action_client
                    .stop_container(
                        &selection.id,
                        Some(StopContainerOptions {
                            t: Some(30),
                            signal: None,
                        }),
                    )
                    .await?
            }
            (DockerStorageKind::Container, DockerStorageAction::Delete) => {
                self.action_client
                    .remove_container(
                        &selection.id,
                        Some(RemoveContainerOptions {
                            force: false,
                            v: false,
                            link: false,
                        }),
                    )
                    .await?
            }
            (DockerStorageKind::Image, DockerStorageAction::Delete) => {
                self.action_client
                    .remove_image(
                        &selection.id,
                        Some(RemoveImageOptions {
                            force: false,
                            noprune: true,
                        }),
                        None,
                    )
                    .await?;
            }
            (DockerStorageKind::Volume, DockerStorageAction::Delete) => {
                self.action_client
                    .remove_volume(&selection.id, Some(RemoveVolumeOptions { force: false }))
                    .await?
            }
            (DockerStorageKind::BuildCache, DockerStorageAction::Delete) if self.exact_cache => {
                let pattern = cache_pattern(&selection.id).ok_or_else(|| {
                    bollard::errors::Error::IOError {
                        err: std::io::Error::other("Invalid cache ID"),
                    }
                })?;
                let response = self
                    .action_client
                    .prune_build(Some(PruneBuildOptions {
                        all: Some(true),
                        filters: Some(HashMap::from([("id".into(), vec![pattern])])),
                        ..Default::default()
                    }))
                    .await?;
                if response
                    .caches_deleted
                    .as_ref()
                    .is_some_and(|ids| ids.iter().any(|id| id != &selection.id))
                {
                    return Err(bollard::errors::Error::IOError {
                        err: std::io::Error::other(
                            "Daemon reported cache deletion outside selection",
                        ),
                    });
                }
                if !response
                    .caches_deleted
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&selection.id))
                {
                    return Ok(MutationOutcome::NoRemoval);
                }
                return Ok(MutationOutcome::Accepted(bytes(response.space_reclaimed)));
            }
            _ => {
                return Err(bollard::errors::Error::IOError {
                    err: std::io::Error::other("Unsupported action"),
                });
            }
        }
        Ok(MutationOutcome::Accepted(None))
    }
}
