#![allow(clippy::too_many_lines)] // Integration scenarios keep setup and assertions together.
#![allow(clippy::unwrap_used)]
//! The built binary end to end: `run` in a temp dir, `serve --stdio` bridging a client
//! that speaks the protocol over the bridge's stdin and stdout, as SSH would carry it.
//! The engine runs this platform's real collectors, so nothing here depends on which
//! metrics exist beyond some live frame arriving.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use kelvo_proto::{Hello, Message, Offer, RemoteInterest, SyncRequest};
use kelvo_schema::{OsKind, Tier};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const AGENT: &str = env!("CARGO_BIN_EXE_kelvo-agent");

struct Daemon {
    child: Child,
    socket: PathBuf,
    // Short paths: a Unix socket path is limited to about 100 bytes.
    _dir: tempfile::TempDir,
}

impl Daemon {
    async fn start() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("ka")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = dir.path().join("s/metrics.sock");
        let child = Command::new(AGENT)
            .arg("run")
            .arg("--data-dir")
            .arg(dir.path().join("data"))
            .arg("--socket")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut daemon = Self {
            child,
            socket,
            _dir: dir,
        };
        // The engine probes its collectors before it serves; on a Mac that takes seconds.
        for _ in 0..300 {
            if daemon.socket.exists() {
                return daemon;
            }
            if let Some(status) = daemon.child.try_wait().unwrap() {
                panic!("the daemon exited early: {status}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the daemon never bound {}", daemon.socket.display());
    }

    /// SIGTERM, then the exit status: the daemon must stop cleanly and unlink its socket.
    async fn terminate(mut self) {
        let pid = self.child.id().unwrap();
        let status = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .await
            .unwrap();
        assert!(status.success());
        let exit = tokio::time::timeout(Duration::from_secs(15), self.child.wait())
            .await
            .expect("the daemon must stop after SIGTERM")
            .unwrap();
        assert!(exit.success(), "daemon exit: {exit}");
        assert!(!self.socket.exists(), "the socket must be removed");
    }
}

struct Bridge {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

impl Bridge {
    fn spawn(socket: &Path) -> Self {
        let mut child = Command::new(AGENT)
            .args(["serve", "--stdio", "--socket"])
            .arg(socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take().unwrap(),
            output: child.stdout.take().unwrap(),
            child,
        }
    }

    async fn write(&mut self, message: &Message) {
        self.input
            .write_all(&kelvo_proto::encode_frame(message).unwrap())
            .await
            .unwrap();
        self.input.flush().await.unwrap();
    }

    /// The next message, or `None` when the bridge's output closes.
    async fn read(&mut self, within: Duration) -> Option<Message> {
        tokio::time::timeout(within, async {
            let length = self.output.read_u32().await.ok()?;
            assert!(length <= kelvo_proto::MAX_FRAME_LEN);
            let mut body = vec![0; length as usize];
            self.output.read_exact(&mut body).await.unwrap();
            Some(kelvo_proto::decode_message(&body).unwrap())
        })
        .await
        .expect("the agent must make protocol progress")
    }

    /// Reads until `want` matches, skipping the other messages a live session sends
    /// meanwhile (status, capabilities, layouts, summaries).
    async fn until<T>(&mut self, within: Duration, want: impl Fn(Message) -> Option<T>) -> T {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let message = self
                .read(left)
                .await
                .expect("the session closed before the expected message");
            if let Some(found) = want(message) {
                return found;
            }
        }
    }

    async fn hello(&mut self) -> Hello {
        match self.read(Duration::from_secs(10)).await {
            Some(Message::Hello(hello)) => *hello,
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    async fn close(mut self) {
        drop(self.input);
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn this_os() -> OsKind {
    if cfg!(target_os = "linux") {
        OsKind::Linux
    } else {
        OsKind::MacOs
    }
}

fn sync_request() -> Message {
    Message::SyncRequest(SyncRequest {
        tier: Tier::S10,
        after: None,
        max_rows: 100,
        from_ms: None,
        max_bytes: None,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_controller_reaches_the_daemon_through_serve_stdio() {
    let daemon = Daemon::start().await;
    let mut bridge = Bridge::spawn(&daemon.socket);

    let hello = bridge.hello().await;
    assert_eq!(hello.host.info.os, this_os());
    assert_eq!(hello.app_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(hello.proto_version, kelvo_proto::PROTO_VERSION);
    assert!(
        !hello.db_instance_uuid.is_nil(),
        "history must be available"
    );
    let negotiated = kelvo_proto::negotiate(
        &Offer::local(kelvo_proto::local_features()),
        &Offer::from(&hello),
    )
    .unwrap();
    bridge.write(&Message::HelloAck(negotiated.ack())).await;
    assert!(matches!(
        bridge.read(Duration::from_secs(5)).await,
        Some(Message::SourceStatus(_))
    ));

    // Summary mode until a viewer asks for detail: a HostSummary every 5 s.
    let summary = bridge
        .until(Duration::from_secs(15), |m| match m {
            Message::HostSummary(s) => Some(s),
            _ => None,
        })
        .await;
    assert!(summary.online);

    bridge
        .write(&Message::Interest(RemoteInterest {
            detail: true,
            ..RemoteInterest::default()
        }))
        .await;
    let frame = bridge
        .until(Duration::from_secs(10), |m| match m {
            Message::Live(frame) => Some(frame),
            _ => None,
        })
        .await;
    assert!(!frame.values.is_empty(), "a live frame carries values");

    bridge.write(&sync_request()).await;
    let page = bridge
        .until(Duration::from_secs(10), |m| match m {
            Message::SyncPage(page) => Some(page),
            Message::Error { message, .. } => panic!("sync failed: {message}"),
            _ => None,
        })
        .await;
    assert_eq!(page.epoch, hello.db_instance_uuid);
    assert_eq!(page.tier, Tier::S10);

    // `status` sees the same daemon while it serves.
    let status = Command::new(AGENT)
        .args(["status", "--json", "--socket"])
        .arg(&daemon.socket)
        .output()
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["running"], true);
    assert_eq!(status["host_id"], hello.host.id.to_string());
    assert_eq!(status["epoch"], hello.db_instance_uuid.to_string());

    bridge.close().await;
    daemon.terminate().await;
}

/// An older controller that knows only buckets and gaps negotiates down: its history
/// pages carry no events. One with no features at all (the v1.0 fixture's ack) lacks
/// stable row ids, so the daemon ends its session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn older_controllers_negotiate_down_or_are_refused() {
    let daemon = Daemon::start().await;

    let mut older = Bridge::spawn(&daemon.socket);
    let hello = older.hello().await;
    let features: BTreeSet<String> = ["rows.buckets", "rows.gaps", "rows.stable-ids"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let offer = Offer {
        proto_version: 1,
        min_compatible: 1,
        features: features.clone(),
    };
    let negotiated = kelvo_proto::negotiate(&offer, &Offer::from(&hello)).unwrap();
    assert_eq!(negotiated.features, features);
    older.write(&Message::HelloAck(negotiated.ack())).await;
    older.write(&sync_request()).await;
    let page = older
        .until(Duration::from_secs(10), |m| match m {
            Message::SyncPage(page) => Some(page),
            _ => None,
        })
        .await;
    assert!(page.events.is_empty());
    older.close().await;

    let fixture = include_bytes!("../../kelvo-proto/tests/fixtures/v1/hello_ack.cbor");
    let ack = kelvo_proto::decode_message(fixture).unwrap();
    assert!(matches!(&ack, Message::HelloAck(a) if a.features.is_empty()));
    let mut oldest = Bridge::spawn(&daemon.socket);
    oldest.hello().await;
    oldest.write(&ack).await;
    assert!(
        oldest.read(Duration::from_secs(10)).await.is_none(),
        "the daemon must end a session without stable row ids"
    );
    oldest.close().await;

    daemon.terminate().await;
}

#[tokio::test]
async fn serve_without_a_daemon_reports_not_running() {
    let dir = tempfile::Builder::new()
        .prefix("ka")
        .tempdir_in("/tmp")
        .unwrap();
    let output = Command::new(AGENT)
        .args(["serve", "--stdio", "--socket"])
        .arg(dir.path().join("metrics.sock"))
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        kelvo_remote::AGENT_NOT_RUNNING
    );
    assert!(output.stdout.is_empty());
    // A stale socket file left by a crash refuses connections: the same answer.
    let stale = dir.path().join("stale.sock");
    drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());
    let output = Command::new(AGENT)
        .args(["serve", "--stdio", "--socket"])
        .arg(&stale)
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
}

#[tokio::test]
async fn version_reports_the_workspace_and_protocol_versions() {
    let output = Command::new(AGENT)
        .args(["version", "--json"])
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let version: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(version["proto"], kelvo_proto::PROTO_VERSION);
    assert_eq!(version["min_compatible"], kelvo_proto::MIN_COMPATIBLE);
}
