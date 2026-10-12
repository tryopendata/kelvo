use crate::{RemoteError, Result, codec};
use kelvo_proto::{Hello, Message, Negotiated, Offer};
use kelvo_schema::HostId;
use serde::{Deserialize, Serialize};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub lan: Option<String>,
    pub tailscale: Option<String>,
    /// Absolute remote path. Empty uses the normal per-user installation.
    #[serde(default)]
    pub helper_path: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Lan,
    Tailscale,
}
impl EndpointConfig {
    pub fn validate(&self) -> Result<()> {
        if self.lan.is_none() && self.tailscale.is_none() {
            return Err(RemoteError::Protocol(
                "at least one SSH alias is required".into(),
            ));
        }
        for alias in [&self.lan, &self.tailscale].into_iter().flatten() {
            validate_alias(alias)?;
        }
        Ok(())
    }
}
fn validate_alias(alias: &str) -> Result<()> {
    if alias.is_empty()
        || alias.starts_with('-')
        || alias.len() > 255
        || alias.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(RemoteError::Protocol("invalid SSH alias".into()));
    }
    Ok(())
}
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
/// A dedicated connection whose lifetime belongs entirely to Kelvo.
pub fn ssh_command(alias: &str) -> Result<Command> {
    validate_alias(alias)?;
    let mut command = Command::new("/usr/bin/ssh");
    command.args([
        "-T",
        "-oBatchMode=yes",
        "-oStrictHostKeyChecking=yes",
        "-oConnectTimeout=5",
        "-oConnectionAttempts=1",
        "-oServerAliveInterval=15",
        "-oServerAliveCountMax=3",
        "-oControlMaster=no",
        "-oControlPath=none",
        "-oClearAllForwardings=yes",
        "-oForwardAgent=no",
        "-oForwardX11=no",
        "-oPermitLocalCommand=no",
        "-oRequestTTY=no",
        "--",
        alias,
    ]);
    command.kill_on_drop(true);
    Ok(command)
}
pub fn helper_command(path: &str, operation: &str) -> String {
    let path = if path.is_empty() {
        "\"$HOME\"/Applications/Kelvo.app/Contents/MacOS/kelvo-remote-helper".to_string()
    } else {
        shell_quote(path)
    };
    format!("exec {path} {operation}")
}
pub struct SshSession {
    pub hello: Hello,
    pub negotiated: Negotiated,
    pub route: Route,
    pub reader: std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>,
    pub writer: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    pub(crate) child: Option<Child>,
    _stderr: Option<codec::AbortOnDrop>,
}
impl SshSession {
    #[cfg(test)]
    pub(crate) fn test(stream: tokio::io::DuplexStream, hello: Hello) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        Self {
            negotiated: kelvo_proto::negotiate(
                &Offer::local(kelvo_proto::local_features()),
                &Offer::from(&hello),
            )
            .unwrap(),
            hello,
            route: Route::Lan,
            reader: Box::pin(reader),
            writer: Box::pin(writer),
            child: None,
            _stderr: None,
        }
    }
    pub async fn close(mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

async fn dial(
    alias: &str,
    route: Route,
    config: &EndpointConfig,
    local: HostId,
    expected: Option<HostId>,
) -> Result<SshSession> {
    let mut command = ssh_command(alias)?;
    command.arg(helper_command(&config.helper_path, "serve --stdio"));
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut reader = child
        .stdout
        .take()
        .ok_or_else(|| RemoteError::Transport("SSH stdout missing".into()))?;
    let mut writer = child
        .stdin
        .take()
        .ok_or_else(|| RemoteError::Transport("SSH stdin missing".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| RemoteError::Transport("SSH stderr missing".into()))?;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let output = captured.clone();
    let task = codec::AbortOnDrop(tokio::spawn(async move {
        let mut buf = [0; 1024];
        while let Ok(n) = stderr.read(&mut buf).await {
            if n == 0 {
                break;
            }
            if let Ok(mut text) = output.lock() {
                let keep = n.min(8192_usize.saturating_sub(text.len()));
                text.extend_from_slice(&buf[..keep]);
            }
        }
    }));
    // ConnectTimeout bounds TCP/auth startup; this additionally bounds helper and handshake.
    let handshake = tokio::time::timeout(Duration::from_secs(10), codec::read(&mut reader)).await;
    let hello = match handshake {
        Ok(Ok(Message::Hello(h))) => h,
        other => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            tokio::task::yield_now().await;
            let detail = captured
                .lock()
                .map(|b| String::from_utf8_lossy(&b).trim().to_string())
                .unwrap_or_default();
            tracing::debug!(?other, %detail, "remote handshake failed");
            let error = if detail.contains("Permission denied")
                || detail.contains("Host key verification failed")
                || detail.contains("REMOTE HOST IDENTIFICATION")
            {
                RemoteError::Authentication(format!("{alias}: {detail}"))
            } else {
                RemoteError::Transport(format!(
                    "{alias}: {}",
                    if detail.is_empty() {
                        handshake_failure(&other).to_string()
                    } else {
                        detail
                    }
                ))
            };
            return Err(error);
        }
    };
    if hello.host.id == local {
        return Err(RemoteError::Identity(
            "cannot connect this Mac to itself".into(),
        ));
    }
    if expected.is_some_and(|id| id != hello.host.id) {
        return Err(RemoteError::Identity(format!(
            "{alias} identified {}",
            hello.host.id
        )));
    }
    let negotiated = kelvo_proto::negotiate(
        &Offer::local(kelvo_proto::local_features()),
        &Offer::from(&hello),
    )
    .map_err(|e| RemoteError::Incompatible(e.to_string()))?;
    codec::write(&mut writer, &Message::HelloAck(negotiated.ack())).await?;
    Ok(SshSession {
        hello,
        negotiated,
        route,
        reader: Box::pin(reader),
        writer: Box::pin(writer),
        child: Some(child),
        _stderr: Some(task),
    })
}

fn handshake_failure(
    outcome: &std::result::Result<Result<Message>, tokio::time::error::Elapsed>,
) -> &'static str {
    match outcome {
        Err(_) => "The other Mac did not answer in time.",
        Ok(Err(RemoteError::Io(error))) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            "The connection closed before Kelvo could start on the other Mac."
        }
        Ok(Err(RemoteError::Io(_))) => {
            "The connection failed while Kelvo was starting on the other Mac."
        }
        Ok(Err(_)) => "The other Mac sent an invalid Kelvo greeting.",
        Ok(Ok(_)) => "The other Mac did not send a Kelvo greeting.",
    }
}

/// Concurrent routes with a bounded preference grace. Futures own SSH children and
/// cancellation kills losers (Tokio's process reaper collects their exit status).
pub async fn connect(
    config: &EndpointConfig,
    local: HostId,
    expected: Option<HostId>,
) -> Result<SshSession> {
    config.validate()?;
    match (&config.lan, &config.tailscale) {
        (Some(lan), Some(tail)) => {
            race_routes(
                dial(lan, Route::Lan, config, local, expected),
                dial(tail, Route::Tailscale, config, local, expected),
                Duration::from_millis(500),
                |s: &SshSession| s.hello.host.id,
            )
            .await
        }
        (Some(alias), None) => dial(alias, Route::Lan, config, local, expected).await,
        (None, Some(alias)) => dial(alias, Route::Tailscale, config, local, expected).await,
        _ => Err(RemoteError::Protocol("no endpoints".into())),
    }
}
async fn race_routes<T, A, B, F>(lan: A, tail: B, grace: Duration, identity: F) -> Result<T>
where
    A: std::future::Future<Output = Result<T>>,
    B: std::future::Future<Output = Result<T>>,
    F: Fn(&T) -> HostId,
{
    let mut a = Box::pin(lan);
    let mut b = Box::pin(tail);
    tokio::select! {
        first=&mut a=>match first {Ok(s)=>Ok(s),Err(e)=>match b.await {Ok(s) if e.retryable()=>Ok(s),Ok(_)=>Err(e),Err(other)=>Err(combine(e,other))}},
        first=&mut b=>match first {
            Err(e)=>match a.await {Ok(s) if e.retryable()=>Ok(s),Ok(_)=>Err(e),Err(other)=>Err(combine(other,e))},
            Ok(tail)=>match tokio::time::timeout(grace,&mut a).await {
                Ok(Ok(lan))=>if identity(&lan)==identity(&tail){Ok(lan)}else{Err(RemoteError::Identity("LAN and Tailscale identify different Macs".into()))},
                Ok(Err(e)) if !e.retryable()=>Err(e),
                _=>Ok(tail),
            }
        }
    }
}

fn combine(a: RemoteError, b: RemoteError) -> RemoteError {
    if !a.retryable() {
        return a;
    }
    if !b.retryable() {
        return b;
    }
    RemoteError::Transport(format!("LAN: {a}; Tailscale: {b}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn handshake_failures_use_plain_sentences() {
        let timeout = tokio::time::timeout(
            Duration::from_secs(1),
            std::future::pending::<Result<Message>>(),
        )
        .await;
        assert_eq!(
            handshake_failure(&timeout),
            "The other Mac did not answer in time."
        );
        let eof = Ok(Err(RemoteError::Io(
            std::io::ErrorKind::UnexpectedEof.into(),
        )));
        assert_eq!(
            handshake_failure(&eof),
            "The connection closed before Kelvo could start on the other Mac."
        );
        let io = Ok(Err(RemoteError::Io(
            std::io::ErrorKind::ConnectionReset.into(),
        )));
        assert_eq!(
            handshake_failure(&io),
            "The connection failed while Kelvo was starting on the other Mac."
        );
        let invalid = Ok(Err(RemoteError::Protocol("bad frame".into())));
        assert_eq!(
            handshake_failure(&invalid),
            "The other Mac sent an invalid Kelvo greeting."
        );
        assert_eq!(
            handshake_failure(&Ok(Ok(Message::Unknown))),
            "The other Mac did not send a Kelvo greeting."
        );
    }
    #[tokio::test]
    async fn prefers_lan_within_grace_and_drops_loser() {
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        struct Peer {
            route: Route,
            drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        impl Drop for Peer {
            fn drop(&mut self) {
                self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let a = drops.clone();
        let b = drops.clone();
        let selected = race_routes(
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(Peer {
                    route: Route::Lan,
                    drops: a,
                })
            },
            async move {
                Ok(Peer {
                    route: Route::Tailscale,
                    drops: b,
                })
            },
            Duration::from_millis(50),
            |_| HostId(uuid::Uuid::nil()),
        )
        .await
        .unwrap();
        assert_eq!(selected.route, Route::Lan);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[tokio::test(start_paused = true)]
    async fn dead_lan_does_not_delay_tailscale_until_timeout() {
        let start = tokio::time::Instant::now();
        let selected = race_routes(
            async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(Route::Lan)
            },
            async { Ok(Route::Tailscale) },
            Duration::from_millis(20),
            |_| HostId(uuid::Uuid::nil()),
        )
        .await
        .unwrap();
        assert_eq!(selected, Route::Tailscale);
        assert_eq!(start.elapsed(), Duration::from_millis(20));
    }
    #[tokio::test]
    async fn mismatching_routes_are_rejected() {
        let selected = race_routes(
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Ok(HostId(uuid::Uuid::from_u128(1)))
            },
            async { Ok(HostId(uuid::Uuid::from_u128(2))) },
            Duration::from_millis(50),
            |id| *id,
        )
        .await;
        assert!(matches!(selected, Err(RemoteError::Identity(_))));
    }
    #[tokio::test]
    async fn all_failed_routes_report_both_errors() {
        let result = race_routes::<Route, _, _, _>(
            async { Err(RemoteError::Transport("local unavailable".into())) },
            async { Err(RemoteError::Transport("vpn unavailable".into())) },
            Duration::from_millis(1),
            |_| HostId(uuid::Uuid::nil()),
        )
        .await;
        let text = result.unwrap_err().to_string();
        assert!(text.contains("local unavailable"));
        assert!(text.contains("vpn unavailable"));
    }
    #[test]
    fn ssh_disables_shared_connections_and_interactive_auth() {
        let c = ssh_command("my-mac").unwrap();
        let args: Vec<_> = c
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-oControlPath=none".into()));
        assert!(args.contains(&"-oStrictHostKeyChecking=yes".into()));
        assert!(ssh_command("-oProxyCommand=evil").is_err());
    }
    #[test]
    fn quotes_remote_paths_without_expansion() {
        assert_eq!(shell_quote("/a'b/$x"), "'/a'\\''b/$x'");
    }
}
