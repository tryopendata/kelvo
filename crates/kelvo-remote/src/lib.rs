#![cfg_attr(test, allow(clippy::too_many_lines))]
//! Persistent, direct SSH sessions. No network listener, collector, or Tauri dependency.
pub mod agent_install;
pub mod agent_layout;
mod clock;
mod codec;
mod host_server;
pub mod install;
mod mirror;
mod mirror_error;
mod mirror_sink;
pub use mirror_error::MirrorError;
mod pages;
pub use mirror::Mirror;
mod server;
mod socket;
mod source;
mod stdio;
mod transport;
pub mod trust;

pub use agent_layout::{AGENT_NOT_RUNNING, AgentEnv, AgentLayout, AgentPaths};
pub use clock::HostClock;
pub use host_server::{HostServer, InterestSet, ServedHost};
pub use kelvo_proto::RemoteInterest;
pub use mirror_sink::MirrorSink;
pub use pages::{page_from_wire, page_to_wire};
pub use server::status_to_wire;
pub use server::{ServerProvider, headline_metrics, serve_connection, serve_metrics};
pub use socket::{ControlRequest, ControlResponse, SocketPaths, control_request, serve_control};
pub use source::{ConnectionStatus, RemoteSource, StatusCallback};
pub use stdio::stdio_bridge;
pub use transport::{EndpointConfig, RemoteCommand, Route, SshSession, connect, ssh_command};

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("{0}")]
    Transport(String),
    #[error("SSH authentication requires attention: {0}")]
    Authentication(String),
    #[error("SSH host-key verification requires attention: {0}")]
    HostKey(String),
    #[error("incompatible remote: {0}")]
    Incompatible(String),
    #[error("remote host identity does not match: {0}")]
    Identity(String),
    /// The host answered with a different id than the one paired. Carries both, so the
    /// shell can offer to re-pair it as a new host (D-116).
    #[error(
        "remote host identity does not match: {alias} identified {actual}, expected {expected}"
    )]
    IdentityMismatch {
        alias: String,
        expected: kelvo_schema::HostId,
        actual: kelvo_schema::HostId,
    },
    /// The host has `kelvo-agent` but its daemon is not running ([`AGENT_NOT_RUNNING`]).
    /// Retryable: the service manager restarts it.
    #[error("Kelvo agent is not running on {0}")]
    AgentNotRunning(String),
    #[error("invalid remote message: {0}")]
    Protocol(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Codec(#[from] kelvo_proto::CodecError),
}
impl RemoteError {
    pub fn retryable(&self) -> bool {
        !matches!(
            self,
            Self::Authentication(_)
                | Self::HostKey(_)
                | Self::Incompatible(_)
                | Self::Identity(_)
                | Self::IdentityMismatch { .. }
                | Self::Protocol(_)
        )
    }
}
pub type Result<T> = std::result::Result<T, RemoteError>;
