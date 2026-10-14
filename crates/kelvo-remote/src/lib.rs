#![cfg_attr(test, allow(clippy::too_many_lines))]
//! Persistent, direct SSH sessions. No network listener, collector, or Tauri dependency.
mod clock;
mod codec;
pub mod install;
mod mirror;
mod mirror_error;
pub use mirror_error::MirrorError;
mod pages;
pub use mirror::Mirror;
mod server;
mod socket;
mod source;
mod transport;
pub mod trust;

pub use clock::HostClock;
pub use kelvo_proto::RemoteInterest;
pub use pages::{page_from_wire, page_to_wire};
pub use server::status_to_wire;
pub use server::{ServerProvider, serve_connection, serve_metrics};
pub use socket::{ControlRequest, ControlResponse, SocketPaths, control_request, serve_control};
pub use source::{ConnectionStatus, MirrorSink, RemoteSource, StatusCallback};
pub use transport::{EndpointConfig, Route, SshSession, connect, ssh_command};

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
                | Self::Protocol(_)
        )
    }
}
pub type Result<T> = std::result::Result<T, RemoteError>;
