//! The Mac path of Add host: installing or updating Kelvo.app, then its commit and
//! rollback phases.
//! Side-effecting install/configure calls settle before cancellation is observed.
use super::*;

pub(super) async fn prepare(
    alias: &str,
    p: kelvo_remote::install::InstallProbe,
) -> Result<RemoteHostPreparation, CommandError> {
    let version = if let Some(helper) = p.helper_path.as_deref() {
        kelvo_remote::control::helper_version(alias, helper)
            .await
            .ok()
    } else {
        None
    };
    let compatible = version
        .as_ref()
        .is_some_and(|v| kelvo_remote::control::protocol_compatible(v.min_compatible, v.proto));
    let installed_version = if let Some(version) = &version {
        Some(version.version.clone())
    } else if let Some(helper) = p.helper_path.as_deref() {
        // Legacy helpers expose only --version. It cannot prove compatibility, but
        // still prevents offering a downgrade to a newer installation.
        Some(kelvo_remote::install::version(alias, helper).await?)
    } else {
        None
    };
    let newer_installed = installed_version
        .as_deref()
        .is_some_and(|v| super::super::commands::older_version(env!("CARGO_PKG_VERSION"), v));
    Ok(RemoteHostPreparation {
        kind: RemoteHostKind::Mac,
        installed: p.app_path.is_some(),
        compatible,
        newer_installed,
        host_id: None,
        name: p.user.clone(),
        message: if newer_installed && !compatible {
            Some("The other Mac runs a newer Kelvo. Update this Mac before connecting.".into())
        } else if !compatible {
            Some(if p.app_path.is_some(){"Quit Kelvo on the other Mac, then update it with this build. Your settings and history are kept."}else{"Install this Kelvo build to enable remote monitoring"}.into())
        } else {
            None
        },
        linux: None,
    })
}

pub(super) struct Installation {
    alias: String,
    app_path: String,
    helper: String,
    update: Option<kelvo_remote::install::InstallUpdate>,
}
impl Installation {
    pub(super) async fn prepare(
        alias: &str,
        mut probe: kelvo_remote::install::InstallProbe,
        install: bool,
    ) -> Result<Self, CommandError> {
        let mut update = None;
        if install {
            if let Some(helper) = probe.helper_path.as_deref() {
                let version = kelvo_remote::install::version(alias, helper).await?;
                if super::super::commands::older_version(env!("CARGO_PKG_VERSION"), &version) {
                    return Err(CommandError::RemoteIncompatible { message: "The other Mac runs a newer Kelvo. Update this Mac first; remote apps are never downgraded.".into() });
                }
            }
            let exe = std::env::current_exe().map_err(failure)?;
            let bundle = exe
                .ancestors()
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or_else(|| failure("Use a packaged Kelvo app to install another Mac"))?;
            if let Some(existing) = probe.app_path.as_deref() {
                let transaction = kelvo_remote::install::update_app(alias, bundle, existing)
                    .await
                    .map_err(CommandError::from)?;
                probe = transaction.probe.clone();
                update = Some(transaction);
            } else {
                probe = kelvo_remote::install::install_app(alias, bundle)
                    .await
                    .map_err(CommandError::from)?;
            }
        }
        let app_path = probe
            .app_path
            .ok_or_else(|| failure("Kelvo app was not found"))?;
        let helper = probe
            .helper_path
            .ok_or_else(|| failure("Install a Kelvo build with remote monitoring support"))?;

        Ok(Installation {
            alias: alias.to_owned(),
            app_path,
            helper,
            update,
        })
    }
    pub(super) async fn connect_and_register(
        &self,
        app: &AppHandle,
        mut config: RemoteHostConfig,
        show_menu_bar: bool,
        before: &mut Option<ControlResponse>,
        cancel: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<RemoteHostRecord, CommandError> {
        let state = app.state::<AppState>();
        check_setup_cancel(cancel)?;
        kelvo_remote::install::launch(&self.alias, &self.app_path)
            .await
            .map_err(CommandError::from)?;
        let snapshot = setup_step(cancel, wait_for_control(&self.alias, &self.helper)).await?;
        let host = snapshot
            .host
            .as_ref()
            .ok_or_else(|| failure("Kelvo returned no machine identity"))?;
        if host.id == state.local {
            return Err(failure("This is this Mac"));
        }
        if state.remotes.saved.lock_ok().contains_key(&host.id) {
            return Err(failure("This Mac is already in your host list"));
        }
        let expected = host.id;
        *before = Some(snapshot);
        let response = kelvo_remote::install::configure(
            &self.alias,
            &self.helper,
            &ControlRequest::Configure {
                allow_remote_monitoring: true,
                show_menu_bar_icon: Some(show_menu_bar),
                launch_at_login: Some(true),
            },
        )
        .await
        .map_err(CommandError::from)?;
        if !response.ok {
            return Err(failure(response.message));
        }
        check_setup_cancel(cancel)?;
        let session = setup_step(cancel, async {
            kelvo_remote::connect(
                &EndpointConfig {
                    lan: config.lan.clone(),
                    tailscale: config.tailscale.clone(),
                    command: kelvo_remote::RemoteCommand::MacHelper(self.helper.clone()),
                },
                state.local,
                Some(expected),
            )
            .await
            .map_err(CommandError::from)
        })
        .await?;
        let mut record = HostRecord::from_identity(session.hello.host.clone(), false);
        session.close().await;
        check_setup_cancel(cancel)?;
        config.id = record.id.to_string();
        record.display_name = config.name.clone();
        register_new(
            app,
            config,
            record,
            kelvo_remote::RemoteCommand::MacHelper(self.helper.clone()),
        )
        .await
    }
    pub(super) async fn finish(&self) {
        if let Some(update) = &self.update
            && let Err(error) = kelvo_remote::install::finish_update(&self.alias, update).await
        {
            tracing::warn!(
                "remote update succeeded; backup retained at {}: {error}",
                update.backup_path
            );
        }
    }
    pub(super) async fn recover(
        &self,
        before: Option<ControlResponse>,
        error: CommandError,
    ) -> CommandError {
        let mut recovery = Vec::new();
        if let Some(snapshot) = before
            && let Some(enabled) = snapshot.allow_remote_monitoring
        {
            match kelvo_remote::install::configure(
                &self.alias,
                &self.helper,
                &ControlRequest::Configure {
                    allow_remote_monitoring: enabled,
                    show_menu_bar_icon: snapshot.show_menu_bar_icon,
                    launch_at_login: snapshot.launch_at_login,
                },
            )
            .await
            {
                Ok(response) if response.ok => {}
                other => {
                    tracing::warn!("remote settings restoration failed: {other:?}");
                    recovery.push("Kelvo could not restore the previous settings on the other Mac. Check its settings and try again.".into())
                }
            }
        }
        if let Some(update) = &self.update
            && let Err(rollback) = kelvo_remote::install::rollback_update(&self.alias, update).await
        {
            recovery.push(format!("The previous app and its data remain backed up at {} and {}. The new app/data are retained. Quit Kelvo on the other Mac before restoring the matched app/data backup: {rollback}",update.backup_path,update.data_backup_path));
        }
        if recovery.is_empty() {
            error
        } else {
            failure(format!("{error}. {}", recovery.join(" ")))
        }
    }
}
