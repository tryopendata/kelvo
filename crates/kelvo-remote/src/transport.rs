use crate::{RemoteError, Result, codec};
use kelvo_proto::{Hello, Message, Negotiated, Offer};
use kelvo_schema::HostId;
use serde::{Deserialize, Serialize};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub lan: Option<String>,
    pub tailscale: Option<String>,
    /// What the SSH session runs on the remote. `helper_path`, the field this replaced,
    /// still reads: a saved Mac host's path string is its [`RemoteCommand::MacHelper`].
    #[serde(default, alias = "helper_path")]
    pub command: RemoteCommand,
}

/// The program an SSH session starts on the remote host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RemoteCommand {
    /// The app's bundled `kelvo-remote-helper` at this absolute path. Empty uses the
    /// normal per-user installation. Serialized as the bare path string, the format of
    /// the old `helper_path` field.
    MacHelper(String),
    /// The headless Linux agent (D-116). `system` selects the root layout; `socket`, when
    /// set, is the daemon's socket, passed explicitly so the session and the user
    /// manager agree on it whatever the login environment says.
    Agent {
        bin: String,
        system: bool,
        #[serde(default)]
        socket: Option<String>,
    },
}

impl Default for RemoteCommand {
    fn default() -> Self {
        Self::MacHelper(String::new())
    }
}

impl RemoteCommand {
    /// The remote command line that starts the stdio session.
    pub fn serve_command(&self) -> Result<String> {
        match self {
            Self::MacHelper(path) => Ok(helper_command(path, "serve --stdio")),
            Self::Agent {
                bin,
                system,
                socket,
            } => {
                let mut inner = format!("exec {} serve --stdio", sh_word(bin)?);
                if *system {
                    inner.push_str(" --system");
                }
                if let Some(socket) = socket {
                    inner.push_str(" --socket ");
                    inner.push_str(&sh_word(socket)?);
                }
                sh_c(&inner)
            }
        }
    }
}

/// `script` for the remote login shell to hand to `/bin/sh` untouched, whatever that
/// shell is (zsh, fish and nu each have their own syntax). The script travels as one
/// single-quoted word, so it may hold no `'`, and no double backslash, which fish reads
/// as an escape even inside single quotes. Values go in through [`sh_word`].
pub(crate) fn sh_c(script: &str) -> Result<String> {
    if script.contains('\'') || script.contains("\\\\") {
        return Err(RemoteError::Protocol(
            "remote script cannot be quoted for every login shell".into(),
        ));
    }
    Ok(format!("exec /bin/sh -c '{script}'"))
}

/// `value` as one double-quoted `/bin/sh` word that survives [`sh_c`]'s outer quotes:
/// `"`, `$` and backquote are escaped; `'`, backslashes and control characters are
/// refused, since no quoting carries them through every login shell.
pub(crate) fn sh_word(value: &str) -> Result<String> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c == '\'' || c == '\\' || c.is_control())
    {
        return Err(RemoteError::Protocol(format!(
            "unsupported characters in remote path {value:?}"
        )));
    }
    let mut word = String::with_capacity(value.len() + 2);
    word.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '$' | '`') {
            word.push('\\');
        }
        word.push(c);
    }
    word.push('"');
    Ok(word)
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
    command.arg(config.command.serve_command()?);
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
        Ok(Ok(Message::Hello(h))) => *h,
        other => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            tokio::task::yield_now().await;
            let detail = captured
                .lock()
                .map(|b| String::from_utf8_lossy(&b).trim().to_string())
                .unwrap_or_default();
            tracing::debug!(?other, %detail, "remote handshake failed");
            return Err(stderr_error(alias, detail, &other));
        }
    };
    if hello.host.id == local {
        return Err(RemoteError::Identity(
            "cannot connect this Mac to itself".into(),
        ));
    }
    if let Some(expected) = expected
        && expected != hello.host.id
    {
        return Err(RemoteError::IdentityMismatch {
            alias: alias.to_string(),
            expected,
            actual: hello.host.id,
        });
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

/// The error for a session that ended before its `Hello`, from the remote's stderr.
fn stderr_error(
    alias: &str,
    detail: String,
    outcome: &std::result::Result<Result<Message>, tokio::time::error::Elapsed>,
) -> RemoteError {
    // SSH's own failures read the same at setup and on every later connection.
    let ssh = crate::install::setup_ssh_error(format!("{alias}: {detail}"));
    if !matches!(ssh, RemoteError::Transport(_)) {
        ssh
    } else if detail
        .lines()
        .any(|line| line.trim() == crate::AGENT_NOT_RUNNING)
    {
        RemoteError::AgentNotRunning(alias.to_string())
    } else if detail.is_empty() {
        RemoteError::Transport(format!("{alias}: {}", handshake_failure(outcome)))
    } else {
        RemoteError::Transport(format!("{alias}: {detail}"))
    }
}

fn handshake_failure(
    outcome: &std::result::Result<Result<Message>, tokio::time::error::Elapsed>,
) -> &'static str {
    match outcome {
        Err(_) => "The hostdid not answer in time.",
        Ok(Err(RemoteError::Io(error))) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            "The connection closed before Kelvo could start on the host."
        }
        Ok(Err(RemoteError::Io(_))) => {
            "The connection failed while Kelvo was starting on the host."
        }
        Ok(Err(_)) => "The hostsent an invalid Kelvo greeting.",
        Ok(Ok(_)) => "The hostdid not send a Kelvo greeting.",
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
            "The hostdid not answer in time."
        );
        let eof = Ok(Err(RemoteError::Io(
            std::io::ErrorKind::UnexpectedEof.into(),
        )));
        assert_eq!(
            handshake_failure(&eof),
            "The connection closed before Kelvo could start on the host."
        );
        let io = Ok(Err(RemoteError::Io(
            std::io::ErrorKind::ConnectionReset.into(),
        )));
        assert_eq!(
            handshake_failure(&io),
            "The connection failed while Kelvo was starting on the host."
        );
        let invalid = Ok(Err(RemoteError::Protocol("bad frame".into())));
        assert_eq!(
            handshake_failure(&invalid),
            "The hostsent an invalid Kelvo greeting."
        );
        assert_eq!(
            handshake_failure(&Ok(Ok(Message::Unknown))),
            "The hostdid not send a Kelvo greeting."
        );
    }
    #[test]
    fn agent_marker_on_stderr_is_a_typed_not_running_error() {
        let eof = Ok(Err(RemoteError::Io(
            std::io::ErrorKind::UnexpectedEof.into(),
        )));
        let error = stderr_error("server", "kelvo-agent: not running".into(), &eof);
        assert!(matches!(&error, RemoteError::AgentNotRunning(alias) if alias == "server"));
        assert!(error.retryable());
        // Among other login noise, still recognised; a mention in other text is not.
        let noisy = "Warning: Permanently added\nkelvo-agent: not running\n".to_string();
        assert!(matches!(
            stderr_error("server", noisy, &eof),
            RemoteError::AgentNotRunning(_)
        ));
        assert!(matches!(
            stderr_error("server", "sh: kelvo-agent: not found".into(), &eof),
            RemoteError::Transport(_)
        ));
        assert!(matches!(
            stderr_error("server", String::new(), &eof),
            RemoteError::Transport(text) if text.contains("closed before")
        ));
    }
    /// A changed host key found on a later connection gets the host-key advice the
    /// setup flow gives, not "could not sign in".
    #[test]
    fn ssh_failures_classify_as_they_do_at_setup() {
        let eof = Ok(Err(RemoteError::Io(
            std::io::ErrorKind::UnexpectedEof.into(),
        )));
        let changed = "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\nHost key verification failed.";
        assert!(matches!(
            stderr_error("server", changed.into(), &eof),
            RemoteError::HostKey(text) if text.starts_with("server: ")
        ));
        assert!(matches!(
            stderr_error(
                "server",
                "user@server: Permission denied (publickey).".into(),
                &eof
            ),
            RemoteError::Authentication(_)
        ));
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

    /// `remote-hosts.json` as the shell writes it today: the endpoint's fields are the
    /// config's aliases plus the top-level `helper_path`.
    const SAVED_HOST: &str = r#"{
      "config": {"id": "6f1c1b7e-0a43-4c47-9d7e-0d5f7f1b2a10", "name": "Studio",
                 "lan": "studio", "tailscale": "studio-ts", "enabled": true,
                 "mirror_budget_mb": 150},
      "record": {"id": "6f1c1b7e-0a43-4c47-9d7e-0d5f7f1b2a10", "is_local": false},
      "helper_path": "/Users/a/Applications/Kelvo.app/Contents/MacOS/kelvo-remote-helper"
    }"#;

    #[test]
    fn old_saved_hosts_read_as_the_mac_helper() {
        let saved: serde_json::Value = serde_json::from_str(SAVED_HOST).unwrap();
        let old = serde_json::json!({
            "lan": saved["config"]["lan"],
            "tailscale": saved["config"]["tailscale"],
            "helper_path": saved["helper_path"],
        });
        let config: EndpointConfig = serde_json::from_value(old).unwrap();
        let helper = "/Users/a/Applications/Kelvo.app/Contents/MacOS/kelvo-remote-helper";
        assert_eq!(config.command, RemoteCommand::MacHelper(helper.into()));
        assert_eq!(config.lan.as_deref(), Some("studio"));
        // The same top-level key also reads as a `command` on its own, so a saved host
        // can rename its field with `#[serde(alias = "helper_path")]`.
        let command: RemoteCommand = serde_json::from_value(saved["helper_path"].clone()).unwrap();
        assert_eq!(command, config.command);
        // An endpoint written before helper_path existed is the default installation.
        let bare: EndpointConfig =
            serde_json::from_str(r#"{"lan": "a", "tailscale": null}"#).unwrap();
        assert_eq!(bare.command, RemoteCommand::MacHelper(String::new()));
        // Both shapes round-trip.
        for command in [
            config.command.clone(),
            RemoteCommand::Agent {
                bin: "/usr/local/bin/kelvo-agent".into(),
                system: true,
                socket: None,
            },
            RemoteCommand::Agent {
                bin: "/home/a/.local/bin/kelvo-agent".into(),
                system: false,
                socket: Some("/run/user/1000/kelvo-agent/metrics.sock".into()),
            },
        ] {
            let endpoint = EndpointConfig {
                lan: Some("a".into()),
                tailscale: None,
                command,
            };
            let text = serde_json::to_string(&endpoint).unwrap();
            assert_eq!(
                serde_json::from_str::<EndpointConfig>(&text).unwrap(),
                endpoint
            );
        }
    }

    #[test]
    fn agent_serve_command_is_one_word_for_any_login_shell() {
        let system = RemoteCommand::Agent {
            bin: "/usr/local/bin/kelvo-agent".into(),
            system: true,
            socket: None,
        };
        assert_eq!(
            system.serve_command().unwrap(),
            r#"exec /bin/sh -c 'exec "/usr/local/bin/kelvo-agent" serve --stdio --system'"#
        );
        let user = RemoteCommand::Agent {
            bin: "/home/my user/.local/bin/kelvo-agent".into(),
            system: false,
            socket: Some("/run/user/1000/k\"$x`/metrics.sock".into()),
        };
        let command = user.serve_command().unwrap();
        assert_eq!(
            command,
            r#"exec /bin/sh -c 'exec "/home/my user/.local/bin/kelvo-agent" serve --stdio --socket "/run/user/1000/k\"\$x\`/metrics.sock"'"#
        );
        // What /bin/sh actually receives: the words come back exactly.
        let script = command
            .strip_prefix("exec /bin/sh -c '")
            .and_then(|s| s.strip_suffix('\''))
            .unwrap()
            .replace("exec ", "printf '%s\\n' ");
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "/home/my user/.local/bin/kelvo-agent\nserve\n--stdio\n--socket\n/run/user/1000/k\"$x`/metrics.sock\n"
        );
        for bad in ["/a'b", "/a\\b", "/a\nb", ""] {
            let command = RemoteCommand::Agent {
                bin: bad.into(),
                system: true,
                socket: None,
            };
            assert!(matches!(
                command.serve_command(),
                Err(RemoteError::Protocol(_))
            ));
        }
        assert!(sh_c("echo 'x'").is_err());
        assert!(sh_c("echo a\\\\b").is_err());
        assert_eq!(
            RemoteCommand::default().serve_command().unwrap(),
            helper_command("", "serve --stdio")
        );
    }
}
