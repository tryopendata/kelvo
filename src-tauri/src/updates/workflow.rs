use anyhow::{Context, anyhow};
use kelvo_schema::lock::LockExt;
use kelvo_update::{Client, ProtocolRange, VerifiedRelease};
use std::{fs, time::Duration};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use super::{Candidate, UpdateStatus, Updates, cache, gate};
use crate::CommandError;

const TARGET: &str = "darwin-aarch64";

fn candidate(app: &AppHandle, release: &VerifiedRelease) -> anyhow::Result<Option<Candidate>> {
    let protocol = ProtocolRange {
        min: kelvo_proto::MIN_COMPATIBLE,
        max: kelvo_proto::PROTO_VERSION,
    };
    let Some(artifact) = release.candidate(env!("CARGO_PKG_VERSION"), protocol, TARGET)? else {
        return Ok(None);
    };
    let info = release.release();
    let metadata = serde_json::json!({
        "version": info.version,
        "notes": info.notes,
        "platforms": { TARGET: { "url": artifact.url, "signature": artifact.signature.as_deref().map(str::trim) } }
    });
    let update = app
        .updater_builder()
        .target(TARGET)
        .timeout(Duration::from_secs(30))
        .build()?
        .restore_update(metadata)?
        .ok_or_else(|| anyhow!("The release is no longer an update"))?;
    Ok(Some(Candidate {
        update,
        artifact: artifact.clone(),
        manifest: release.manifest_bytes().to_vec(),
        signature: release.signature().into(),
    }))
}

impl Updates {
    pub(super) async fn check(&self, app: &AppHandle) -> Result<UpdateStatus, CommandError> {
        if matches!(self.status(), UpdateStatus::Ready { .. }) {
            return Ok(self.status());
        }
        let previous = self.status();
        let previous_package = self.candidate.lock_ok().as_ref().map(|candidate| {
            (
                candidate.update.version.clone(),
                candidate.artifact.sha256.clone(),
            )
        });
        self.set(app, UpdateStatus::Checking);
        let client = Client::production().map_err(|e| self.failed(app, e))?;
        let release = client.fetch().await.map_err(|e| self.failed(app, e))?;
        let next = match candidate(app, &release) {
            Ok(candidate) => candidate,
            Err(error) => {
                *self.candidate.lock_ok() = None;
                if matches!(
                    error.downcast_ref::<kelvo_update::Error>(),
                    Some(kelvo_update::Error::Incompatible { .. })
                ) {
                    return Ok(self.set(app, UpdateStatus::CompatibilityBlocked {
                        version: release.release().version.clone(),
                        message: "This release drops an SSH protocol version. Upgrade connected controllers first, then quit Kelvo and install the newer release from its DMG.".into(),
                    }));
                }
                return Err(self.failed(app, error));
            }
        };
        let status = match &next {
            Some(candidate)
                if previous.has_download()
                    && previous_package.as_ref().is_some_and(|(version, digest)| {
                        version == &candidate.update.version && digest == &candidate.artifact.sha256
                    }) =>
            {
                previous
            }
            Some(candidate) => UpdateStatus::Available {
                version: candidate.update.version.clone(),
                notes: release.release().notes.clone(),
            },
            None => UpdateStatus::UpToDate,
        };
        *self.candidate.lock_ok() = next;
        Ok(self.set(app, status))
    }

    pub(super) async fn restore(&self, app: &AppHandle) -> anyhow::Result<()> {
        // Manual work before the startup delay owns its candidate, including an
        // already-installed package. Cache restoration must never overwrite it.
        let Some(_operation) = self.restoration_guard().await else {
            return Ok(());
        };
        let handle = app.clone();
        let restored = tauri::async_runtime::spawn_blocking(move || {
            let Some(release) = cache::load(&handle)? else {
                return Ok::<_, anyhow::Error>(None);
            };
            let Some(candidate) = candidate(&handle, &release)? else {
                return Ok(None);
            };
            cache::verified_bytes(&handle, &candidate)?;
            Ok(Some(candidate))
        })
        .await??;
        if let Some(candidate) = restored {
            self.set(
                app,
                UpdateStatus::Downloaded {
                    version: candidate.update.version.clone(),
                },
            );
            *self.candidate.lock_ok() = Some(candidate);
        }
        Ok(())
    }

    pub(super) async fn download(
        &self,
        app: &AppHandle,
        candidate: &Candidate,
    ) -> Result<(), CommandError> {
        let handle = app.clone();
        let cached = candidate.clone();
        if tauri::async_runtime::spawn_blocking(move || {
            cache::verified_bytes(&handle, &cached)?;
            cache::save(&handle, &cached)
        })
        .await
        .is_ok_and(|result| result.is_ok())
        {
            return Ok(());
        }
        self.set(
            app,
            UpdateStatus::Downloading {
                version: candidate.update.version.clone(),
                percent: None,
            },
        );
        let path = cache::artifact_path(app).map_err(|e| self.failed(app, e))?;
        // A crash leaves either the previous metadata or an untrusted partial package;
        // restoration authenticates both and never accepts either mismatch.
        if let Err(error) = fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(self.failed(app, error));
        }
        let client = Client::production().map_err(|e| self.failed(app, e))?;
        client
            .download(&candidate.artifact, &path)
            .await
            .map_err(|e| self.failed(app, e))?;
        let handle = app.clone();
        let cached = candidate.clone();
        tauri::async_runtime::spawn_blocking(move || {
            cache::verified_bytes(&handle, &cached)?;
            cache::save(&handle, &cached)
        })
        .await
        .map_err(|e| self.failed(app, e))?
        .map_err(|e| self.failed(app, e))?;
        self.set(
            app,
            UpdateStatus::Downloaded {
                version: candidate.update.version.clone(),
            },
        );
        Ok(())
    }

    pub(super) async fn advance(&self, app: &AppHandle) -> Result<(), CommandError> {
        if !gate::automatic_allowed(app) {
            return Ok(());
        }
        if !matches!(
            self.status(),
            UpdateStatus::Available { .. }
                | UpdateStatus::Downloaded { .. }
                | UpdateStatus::Waiting { .. }
        ) {
            return Ok(());
        }
        let candidate = self.candidate.lock_ok().clone();
        if let Some(candidate) = candidate {
            if matches!(self.status(), UpdateStatus::Available { .. }) {
                self.download(app, &candidate).await?;
            }
            self.install(app, candidate, true).await?;
        }
        Ok(())
    }

    pub(super) async fn install(
        &self,
        app: &AppHandle,
        mut candidate: Candidate,
        automatic: bool,
    ) -> Result<(), CommandError> {
        let version = candidate.update.version.clone();
        // Avoid rereading a large cached archive every minute while an interactive
        // window remains open. Eligibility is checked again under the gate below.
        if automatic && (!gate::automatic_allowed(app) || gate::interactive_window_open(app)) {
            self.set(app, UpdateStatus::Waiting { version, reason: "Waiting until automatic installation is enabled and Kelvo windows are closed.".into() });
            return Ok(());
        }
        // Reverify on the worker before taking the gate, then recheck mutable policy
        // under the gate immediately before handing the bytes to the native installer.
        let handle = app.clone();
        let cached = candidate.clone();
        let bytes =
            tauri::async_runtime::spawn_blocking(move || cache::verified_bytes(&handle, &cached))
                .await
                .map_err(|e| self.failed(app, e))?
                .map_err(|e| self.failed(app, e))?;
        let guard = match self.installation_guard(automatic, || {
            gate::automatic_allowed(app) && !gate::interactive_window_open(app)
        }) {
            Ok(guard) => guard,
            Err(blocked) => {
                self.set(
                    app,
                    UpdateStatus::Waiting {
                        version,
                        reason: blocked.reason().into(),
                    },
                );
                return Ok(());
            }
        };
        candidate.update.allow_elevation = !automatic;
        if automatic {
            crate::startup::mark_background_restart(app, &version)
                .map_err(|e| self.failed(app, e))?;
        }
        self.set(
            app,
            UpdateStatus::Installing {
                version: version.clone(),
            },
        );
        let result =
            tauri::async_runtime::spawn_blocking(move || candidate.update.install(bytes)).await;
        if let Err(error) = result
            .context("running the update installer")
            .and_then(|result| result.map_err(Into::into))
        {
            crate::startup::clear_background_restart(app);
            if automatic && authorization_required(&error) {
                self.set(app, UpdateStatus::NeedsAuthorization { version });
                return Ok(());
            }
            return Err(self.failed(app, error));
        }
        *self.candidate.lock_ok() = None;
        self.set(app, UpdateStatus::Ready { version });
        // Keep the exclusive guard alive through Exit and shutdown. Otherwise queued
        // commands could start a destructive operation between this call and Exit.
        app.manage(InstallationCommitted { _guard: guard });
        app.request_restart();
        Ok(())
    }
}

struct InstallationCommitted {
    _guard: tokio::sync::OwnedRwLockWriteGuard<()>,
}

fn authorization_required(error: &anyhow::Error) -> bool {
    error.downcast_ref::<tauri_plugin_updater::Error>().is_some_and(|error| {
        matches!(error, tauri_plugin_updater::Error::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied)
    })
}
