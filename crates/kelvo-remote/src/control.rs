//! Update policy/status shared by the SSH control CLI and the desktop shell.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RemoteUpdateStatus {
    pub supported: bool,
    pub check_updates: bool,
    pub auto_install_updates: bool,
    pub status: String,
    pub version: Option<String>,
    pub message: Option<String>,
}

impl RemoteUpdateStatus {
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            check_updates: false,
            auto_install_updates: false,
            status: "unsupported".into(),
            version: None,
            message: Some("Update this host once to enable automatic updates.".into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HelperVersion {
    pub version: String,
    pub proto: u16,
    pub min_compatible: u16,
    #[serde(default)]
    pub automatic_updates: bool,
}

impl HelperVersion {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            proto: kelvo_proto::PROTO_VERSION,
            min_compatible: kelvo_proto::MIN_COMPATIBLE,
            automatic_updates: true,
        }
    }
}

pub async fn helper_version(alias: &str, path: &str) -> crate::Result<HelperVersion> {
    let text = crate::install::run_remote(
        alias,
        &crate::transport::helper_command(path, "version --json"),
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| crate::RemoteError::Protocol(e.to_string()))
}

pub async fn helper_update_status(
    alias: &str,
    path: &str,
) -> crate::Result<crate::ControlResponse> {
    let text = crate::install::run_remote(
        alias,
        &crate::transport::helper_command(path, "update-status --json"),
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| crate::RemoteError::Protocol(e.to_string()))
}

pub async fn agent_update_status(
    alias: &str,
    target: &crate::agent_install::AgentTarget,
) -> crate::agent_install::Result<RemoteUpdateStatus> {
    let w = target.words()?;
    let text = crate::install::run_remote(
        alias,
        &crate::transport::sh_c(&format!("exec {} update-status --json{}", w.bin, w.all))?,
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| crate::RemoteError::Protocol(e.to_string()).into())
}

pub async fn agent_update_policy(
    alias: &str,
    target: &crate::agent_install::AgentTarget,
    check_updates: bool,
    auto_install_updates: bool,
) -> crate::agent_install::Result<RemoteUpdateStatus> {
    let w = target.words()?;
    let policy = serde_json::json!({"general": {"check_updates": check_updates, "auto_install_updates": auto_install_updates}}).to_string();
    let script = format!(
        "exec {} configure {}{}",
        w.bin,
        crate::transport::sh_word(&policy)?,
        w.data_only
    );
    crate::install::run_remote(alias, &crate::transport::sh_c(&script)?).await?;
    agent_update_status(alias, target).await
}

pub async fn request_agent_update(
    alias: &str,
    target: &crate::agent_install::AgentTarget,
) -> crate::agent_install::Result<()> {
    let w = target.words()?;
    let script = format!("exec {} update --request{}", w.bin, w.all);
    crate::install::run_remote(alias, &crate::transport::sh_c(&script)?).await?;
    Ok(())
}

/// Version numbers describe releases; the negotiated protocol describes compatibility.
pub fn protocol_compatible(min: u16, max: u16) -> bool {
    kelvo_proto::negotiate(
        &kelvo_proto::Offer::local(Default::default()),
        &kelvo_proto::Offer {
            min_compatible: min,
            proto_version: max,
            features: Default::default(),
        },
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_uses_the_protocol_interval() {
        assert!(protocol_compatible(
            kelvo_proto::MIN_COMPATIBLE,
            kelvo_proto::PROTO_VERSION
        ));
        assert!(protocol_compatible(
            kelvo_proto::MIN_COMPATIBLE,
            kelvo_proto::PROTO_VERSION + 1
        ));
        assert!(!protocol_compatible(
            kelvo_proto::PROTO_VERSION + 1,
            kelvo_proto::PROTO_VERSION + 2
        ));
        assert!(!protocol_compatible(2, 1));
    }
}
