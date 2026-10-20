//! Installing, updating and removing the headless Linux agent over SSH (D-115). The Mac
//! app's path stays in [`crate::install`].
//!
//! Every script here reaches the remote as `exec /bin/sh -c '<script>'`
//! ([`crate::transport::sh_c`]), so the login shell (root's is zsh on the target host;
//! fish or nu elsewhere) only ever sees one single-quoted word, and every value inside
//! is a double-quoted [`crate::transport::sh_word`]. The scripts are plain POSIX sh and
//! are run under dash and busybox in the script tests.
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::transport::{sh_c, sh_word, ssh_command};
use crate::{AgentLayout, AgentPaths, RemoteCommand, RemoteError};

mod flows;
mod probe;
mod unit;
mod upload;

#[cfg(test)]
mod tests;

pub use flows::{
    StepOutcome, UninstallReport, UninstallStep, install, reset_identity, status, uninstall,
    uninstall_at, update,
};
pub use probe::{
    AgentStatus, AgentVersion, DigestTool, DockerSocket, ExistingAgent, HostProbe, LinuxProbe,
    OtherUserAgent, probe_host, probe_linux,
};
pub use upload::{run_remote_with_stdin, upload_agent};

/// Why an agent step failed. Steps that only need SSH report [`RemoteError`] through
/// [`AgentInstallError::Remote`].
#[derive(Debug, thiserror::Error)]
pub enum AgentInstallError {
    #[error(transparent)]
    Remote(#[from] RemoteError),
    #[error("Kelvo's agent runs on 64-bit ARM and x86 Linux; this host is {0}")]
    UnsupportedArch(String),
    /// No usable systemd. Hosts without it would need the `@reboot` cron fallback, which
    /// is not built yet.
    #[error(
        "Kelvo's agent needs systemd, and this host has none ({detail}). Hosts without systemd are not supported yet"
    )]
    NoSystemd { detail: String },
    /// A non-root login with no systemd user manager to run the user unit.
    #[error(
        "{user} has no systemd user manager ({detail}). Log in as root, or enable lingering with `sudo loginctl enable-linger {user}`"
    )]
    NoUserManager { user: String, detail: String },
    #[error("This host has no sha256sum, shasum or openssl to verify the upload with")]
    NoDigestTool,
    #[error(
        "{path} has {free_bytes} bytes free; the agent needs {needed_bytes} (the history budget plus the low-disk reserve)"
    )]
    LowDisk {
        path: String,
        free_bytes: u64,
        needed_bytes: u64,
    },
    /// A host gets one agent: the other layout, or for root another login's user layout,
    /// already has one (D-115). `user` names that other login.
    #[error(
        "This host already runs a Kelvo agent installed as {}{} at {binary}; remove it first",
        existing.as_str(),
        user.as_deref().map(|u| format!(" for {u}")).unwrap_or_default()
    )]
    OtherLayout {
        existing: AgentLayout,
        binary: String,
        user: Option<String>,
    },
    #[error("The Kelvo agent is already installed at {binary}; update it instead")]
    AlreadyInstalled { binary: String },
    #[error("No Kelvo agent is installed at {binary}")]
    NotInstalled { binary: String },
    /// The bytes that arrived are not the ones sent; nothing was replaced.
    #[error("The uploaded agent did not match its sha256 {expected}; nothing was replaced")]
    DigestMismatch { expected: String },
    #[error("The Kelvo agent did not start: {detail}")]
    NotRunning {
        detail: String,
        status: Option<Box<AgentStatus>>,
    },
    /// The update failed. `restored` says whether the previous binary and history were
    /// put back and the old agent is running again.
    #[error("Updating the agent failed ({cause}); {}", if *restored { "the previous version was restored" } else { "restoring the previous version also failed" })]
    UpdateFailed {
        cause: Box<AgentInstallError>,
        restored: bool,
    },
}

pub type Result<T> = std::result::Result<T, AgentInstallError>;

/// Where one install lives on the host, and how to verify uploads there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTarget {
    pub layout: AgentLayout,
    pub paths: AgentPaths,
    pub digest: DigestTool,
}

impl AgentTarget {
    /// What an SSH session runs to reach this agent. The user layout names its socket,
    /// as its unit does, so the session never derives it from a different environment.
    pub fn remote_command(&self) -> RemoteCommand {
        RemoteCommand::Agent {
            bin: self.paths.binary.to_string_lossy().into_owned(),
            system: self.layout == AgentLayout::System,
            socket: (self.layout == AgentLayout::User)
                .then(|| self.paths.socket.to_string_lossy().into_owned()),
        }
    }

    pub(crate) fn words(&self) -> Result<Words> {
        let word = |path: &Path| -> Result<String> {
            let text = path.to_str().ok_or_else(|| {
                RemoteError::Protocol(format!("non-UTF-8 remote path {}", path.display()))
            })?;
            Ok(sh_word(text)?)
        };
        let bin = word(&self.paths.binary)?;
        let data = word(&self.paths.data_dir)?;
        let socket = word(&self.paths.socket)?;
        let (all, data_only, systemctl) = match self.layout {
            AgentLayout::System => (
                " --system".to_string(),
                " --system".to_string(),
                "systemctl",
            ),
            AgentLayout::User => (
                format!(" --data-dir {data} --socket {socket}"),
                format!(" --data-dir {data}"),
                "systemctl --user",
            ),
        };
        Ok(Words {
            unit: word(&self.paths.unit)?,
            bin,
            data,
            socket,
            all,
            data_only,
            systemctl,
        })
    }
}

/// A target's paths as `/bin/sh` words, and the flags that point agent commands at them.
pub(crate) struct Words {
    pub bin: String,
    pub data: String,
    pub socket: String,
    pub unit: String,
    /// For `run`, `status` and `install-unit`.
    pub all: String,
    /// For `configure`, `uninstall` and `reset-identity`.
    pub data_only: String,
    pub systemctl: &'static str,
}

/// What a remote command printed and its exit status.
#[derive(Debug)]
pub(crate) struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a remote command line. SSH in production; a local or container shell in tests.
pub(crate) trait Remote {
    async fn exec(
        &self,
        command: &str,
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> crate::Result<Output>;
}

/// The host behind an SSH alias, with the same strict options as live sessions.
pub(crate) struct Ssh<'a>(pub &'a str);

impl Remote for Ssh<'_> {
    async fn exec(
        &self,
        command: &str,
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> crate::Result<Output> {
        let mut cmd = ssh_command(self.0)?;
        cmd.arg(command);
        let output = collect(cmd, stdin, timeout).await?;
        // 255 is ssh's own failure: connection, host key or authentication.
        if output.code == Some(255) {
            return Err(crate::install::setup_ssh_error(
                output.stderr.trim().to_string(),
            ));
        }
        Ok(output)
    }
}

/// Spawns `cmd`, feeds it `stdin`, and collects bounded output within `timeout`.
pub(crate) async fn collect(
    mut cmd: tokio::process::Command,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> crate::Result<Output> {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let input = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RemoteError::Transport("missing stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RemoteError::Transport("missing stderr".into()))?;
    crate::install::setup_timeout(timeout, async move {
        let feed = async move {
            if let (Some(mut input), Some(bytes)) = (input, stdin) {
                input.write_all(bytes).await?;
                input.shutdown().await?;
            }
            Ok::<(), std::io::Error>(())
        };
        let (fed, out, err, status) =
            tokio::join!(feed, bounded(stdout), bounded(stderr), child.wait());
        let status = status?;
        // A remote that exits early closes its stdin; its status says why.
        if status.success() {
            fed?;
        }
        Ok(Output {
            code: status.code(),
            stdout: out?,
            stderr: err?,
        })
    })
    .await
}

const OUTPUT_BOUND: u64 = 64 * 1024;

async fn bounded(stream: impl AsyncRead + Unpin) -> crate::Result<String> {
    let mut bytes = Vec::new();
    stream
        .take(OUTPUT_BOUND + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > OUTPUT_BOUND {
        return Err(RemoteError::Protocol("setup output exceeded bound".into()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Runs `script` through `/bin/sh` on the remote and returns its stdout, or its stderr
/// as the error when it exits non-zero.
pub(crate) async fn run_script(
    remote: &impl Remote,
    script: &str,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<String> {
    let output = remote.exec(&sh_c(script)?, stdin, timeout).await?;
    if output.code == Some(0) {
        return Ok(output.stdout);
    }
    Err(failed(&output).into())
}

/// The error for a script that exited non-zero.
pub(crate) fn failed(output: &Output) -> RemoteError {
    let detail = output.stderr.trim();
    RemoteError::Transport(if detail.is_empty() {
        format!("remote command failed with status {:?}", output.code)
    } else {
        detail.to_string()
    })
}

/// Setup steps that take more than a round trip: starting the daemon, copying history.
pub(crate) const STEP_TIMEOUT: Duration = Duration::from_secs(60);
/// Uploading the agent binary.
pub(crate) const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);
