//! Install, update, uninstall and identity reset, each a short sequence of scripts.
//! Same shape as the Mac path (`install.rs`): an update keeps the previous binary and
//! a copy of the history until the new daemon answers, and puts both back if it does
//! not.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::probe::{AgentStatus, LinuxProbe, probe_with};
use super::upload::{drop_previous, upload_with};
use super::{AgentInstallError, AgentTarget, Remote, Result, STEP_TIMEOUT, Ssh, run_script, unit};
use crate::transport::sh_word;

/// Installs the agent on a host with no agent yet: validates the probe for `budget_mb`,
/// uploads `bytes` (checked against `sha256_hex` on the host), writes the unit, records
/// the history budget, enables and starts the service, and waits up to 10 s for it.
pub async fn install(
    alias: &str,
    probe: &LinuxProbe,
    bytes: &[u8],
    sha256_hex: &str,
    budget_mb: u32,
) -> Result<AgentStatus> {
    install_with(&Ssh(alias), probe, bytes, sha256_hex, budget_mb).await
}

pub(crate) async fn install_with(
    remote: &impl Remote,
    probe: &LinuxProbe,
    bytes: &[u8],
    sha256_hex: &str,
    budget_mb: u32,
) -> Result<AgentStatus> {
    probe.validate(budget_mb)?;
    if let Some(existing) = probe.existing() {
        return Err(AgentInstallError::AlreadyInstalled {
            binary: existing.binary.clone(),
        });
    }
    let target = probe.target()?;
    upload_with(remote, &target, bytes, sha256_hex).await?;
    let config = serde_json::json!({ "history": { "size_limit_mb": budget_mb } }).to_string();
    let script = unit::activate(&target.words()?, &sh_word(&config)?);
    running(&run_script(remote, &script, None, STEP_TIMEOUT).await?)
}

/// Replaces the installed agent with `bytes`, keeping its history: stops it, copies the
/// history aside, uploads (keeping the old binary), rewrites the unit, starts it and
/// waits up to 10 s. If any step fails, the old binary and history are put back and
/// the old agent restarted, and the error is [`AgentInstallError::UpdateFailed`].
pub async fn update(
    alias: &str,
    probe: &LinuxProbe,
    bytes: &[u8],
    sha256_hex: &str,
) -> Result<AgentStatus> {
    update_with(&Ssh(alias), probe, bytes, sha256_hex).await
}

pub(crate) async fn update_with(
    remote: &impl Remote,
    probe: &LinuxProbe,
    bytes: &[u8],
    sha256_hex: &str,
) -> Result<AgentStatus> {
    let target = probe.target()?;
    if probe.existing().is_none() {
        return Err(AgentInstallError::NotInstalled {
            binary: target.paths.binary.to_string_lossy().into_owned(),
        });
    }
    let words = target.words()?;
    if probe
        .existing()
        .and_then(|agent| agent.version.as_ref())
        .is_some_and(|version| version.automatic_updates)
    {
        return running(
            &run_script(remote, &unit::request_update(&words), None, STEP_TIMEOUT).await?,
        );
    }
    let attempt = async {
        run_script(remote, &unit::stop_and_back_up(&words), None, STEP_TIMEOUT).await?;
        upload_with(remote, &target, bytes, sha256_hex).await?;
        running(&run_script(remote, &unit::restart(&words), None, STEP_TIMEOUT).await?)
    };
    match attempt.await {
        Ok(status) => {
            // The update stands; leftovers only cost disk, so failing to clean is fine.
            if let Err(e) = finish(remote, &target).await {
                tracing::warn!("removing the update's backup: {e}");
            }
            Ok(status)
        }
        Err(cause) => {
            let restored =
                match run_script(remote, &unit::roll_back(&words), None, STEP_TIMEOUT).await {
                    Ok(text) => key_values(&text).get("restored").copied() == Some("1"),
                    Err(e) => {
                        tracing::warn!("rolling the agent update back: {e}");
                        false
                    }
                };
            Err(AgentInstallError::UpdateFailed {
                cause: Box::new(cause),
                restored,
            })
        }
    }
}

async fn finish(remote: &impl Remote, target: &AgentTarget) -> Result<()> {
    run_script(
        remote,
        &unit::drop_backup(&target.words()?),
        None,
        STEP_TIMEOUT,
    )
    .await?;
    drop_previous(remote, target).await
}

/// The agent's status, without waiting.
pub async fn status(alias: &str, target: &AgentTarget) -> Result<AgentStatus> {
    status_with(&Ssh(alias), target).await
}

pub(crate) async fn status_with(remote: &impl Remote, target: &AgentTarget) -> Result<AgentStatus> {
    let w = target.words()?;
    let text = run_script(
        remote,
        &format!("exec {} status --json{}", w.bin, w.all),
        None,
        STEP_TIMEOUT,
    )
    .await?;
    AgentStatus::parse(&text)
}

/// Gives the host a new id (stopping and restarting the daemon around it) and returns
/// the restarted daemon's status, which carries the new id. History recorded under the
/// old id stays with the old mirror on the controller.
pub async fn reset_identity(alias: &str, probe: &LinuxProbe) -> Result<AgentStatus> {
    reset_identity_with(&Ssh(alias), probe).await
}

pub(crate) async fn reset_identity_with(
    remote: &impl Remote,
    probe: &LinuxProbe,
) -> Result<AgentStatus> {
    let target = probe.target()?;
    let text = run_script(
        remote,
        &unit::reset_identity(&target.words()?),
        None,
        STEP_TIMEOUT,
    )
    .await?;
    running(&text)
}

/// How one uninstall step went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum StepOutcome {
    Ok,
    /// Nothing to remove.
    Absent,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallStep {
    pub name: String,
    pub outcome: StepOutcome,
}

/// Every uninstall step and how it went. Uninstall never fails as a whole (D-105).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallReport {
    pub steps: Vec<UninstallStep>,
}

impl UninstallReport {
    /// Whether every step succeeded or had nothing to do.
    pub fn is_clean(&self) -> bool {
        self.steps
            .iter()
            .all(|s| !matches!(s.outcome, StepOutcome::Failed(_)))
    }

    fn failed(name: &str, error: impl std::fmt::Display) -> Self {
        Self {
            steps: vec![UninstallStep {
                name: name.into(),
                outcome: StepOutcome::Failed(error.to_string()),
            }],
        }
    }

    fn parse(text: &str) -> Self {
        let steps = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .flat_map(|(name, outcome)| {
                if name == "agent-report" {
                    return match serde_json::from_str::<Self>(outcome) {
                        Ok(report) if !report.steps.is_empty() => report.steps,
                        _ => Self::failed("agent", "invalid or empty uninstall report").steps,
                    };
                }
                vec![UninstallStep {
                    name: name.into(),
                    outcome: match outcome {
                        "ok" => StepOutcome::Ok,
                        "absent" => StepOutcome::Absent,
                        other => StepOutcome::Failed(
                            other.strip_prefix("failed: ").unwrap_or(other).to_string(),
                        ),
                    },
                }]
            })
            .collect();
        Self { steps }
    }
}

/// Removes the agent this login installs (with `purge`, its history and identity too).
/// Probes first to find it. Never fails: every problem, including an unreachable host,
/// is a failed step in the report.
pub async fn uninstall(alias: &str, purge: bool) -> UninstallReport {
    uninstall_with(&Ssh(alias), purge).await
}

pub(crate) async fn uninstall_with(remote: &impl Remote, purge: bool) -> UninstallReport {
    let target = match probe_with(remote).await.and_then(|p| p.target()) {
        Ok(target) => target,
        Err(e) => return UninstallReport::failed("probe", e),
    };
    uninstall_target(remote, &target, purge).await
}

/// Removes the agent at `target`, found by an earlier probe. Never fails, like
/// [`uninstall`].
pub async fn uninstall_at(alias: &str, target: &AgentTarget, purge: bool) -> UninstallReport {
    uninstall_target(&Ssh(alias), target, purge).await
}

pub(crate) async fn uninstall_target(
    remote: &impl Remote,
    target: &AgentTarget,
    purge: bool,
) -> UninstallReport {
    let script = match target.words() {
        Ok(w) => unit::uninstall(&w, purge),
        Err(e) => return UninstallReport::failed("paths", e),
    };
    match run_script(remote, &script, None, STEP_TIMEOUT).await {
        Ok(text) => UninstallReport::parse(&text),
        Err(e) => UninstallReport::failed("connect", e),
    }
}

/// The `status=` line of a wait, which must say the daemon is running.
fn running(text: &str) -> Result<AgentStatus> {
    let line = key_values(text).get("status").copied().unwrap_or_default();
    if line.is_empty() {
        return Err(AgentInstallError::NotRunning {
            detail: "the agent printed no status".into(),
            status: None,
        });
    }
    let status = AgentStatus::parse(line)?;
    if status.running {
        Ok(status)
    } else {
        Err(AgentInstallError::NotRunning {
            detail: "the service started but the agent did not answer on its socket".into(),
            status: Some(Box::new(status)),
        })
    }
}

fn key_values(text: &str) -> BTreeMap<&str, &str> {
    text.lines().filter_map(|l| l.split_once('=')).collect()
}
