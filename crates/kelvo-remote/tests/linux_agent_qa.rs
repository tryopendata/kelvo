#![allow(clippy::unwrap_used, clippy::too_many_lines)]
//! The Linux agent's whole lifecycle on a real host over SSH: probe, install, a live
//! session, update with history kept, uninstall with nothing left. It installs for
//! real and uninstalls at the end, even after a failure. Run by hand:
//!
//! ```sh
//! make agent-linux
//! KELVO_QA_ALIAS=linux-tailscale \
//! KELVO_QA_AGENT=src-tauri/binaries/agent/kelvo-agent-aarch64-unknown-linux-musl \
//!   cargo test -p kelvo-remote --test linux_agent_qa -- --ignored --nocapture
//! ```
//!
//! The SSH login should be root (the system layout); the SELinux checks assume it.
use std::time::Duration;

use kelvo_proto::{Message, RemoteInterest};
use kelvo_remote::agent_install::{self, HostProbe};
use kelvo_remote::install::run_remote;
use kelvo_remote::{EndpointConfig, RemoteError, SshSession};
use kelvo_schema::HostId;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BUDGET_MB: u32 = 150;

async fn read(session: &mut SshSession) -> Message {
    tokio::time::timeout(Duration::from_secs(15), async {
        let length = session.reader.read_u32().await.unwrap();
        let mut body = vec![0; length as usize];
        session.reader.read_exact(&mut body).await.unwrap();
        kelvo_proto::decode_message(&body).unwrap()
    })
    .await
    .expect("the agent must make protocol progress")
}

async fn rows(alias: &str) -> u64 {
    let out = run_remote(
        alias,
        r#"python3 -c 'import sqlite3; c = sqlite3.connect("file:/var/lib/kelvo-agent/history.sqlite?mode=ro", uri=True); print(sum(c.execute("select count(*) from " + t).fetchone()[0] for t in ("tier_10s", "tier_1m", "series", "gaps")))'"#,
    )
    .await
    .unwrap();
    out.trim().parse().unwrap()
}

async fn sh(alias: &str, command: &str) -> String {
    let out = run_remote(alias, &format!("({command}) 2>&1; true"))
        .await
        .unwrap();
    println!("$ {command}\n{}", out.trim_end());
    out
}

async fn lifecycle(alias: String, bytes: Vec<u8>, sha: String) {
    let alias = alias.as_str();
    let probe = match agent_install::probe_host(alias, Some(BUDGET_MB))
        .await
        .unwrap()
    {
        HostProbe::Linux(probe) => *probe,
        HostProbe::Mac(p) => panic!("{alias} is a Mac: {p:?}"),
    };
    println!("probe: {probe:#?}\nwarnings: {:?}", probe.warnings());
    assert!(probe.existing().is_none(), "start from a clean host");
    let target = probe.target().unwrap();
    println!("target: {target:?}");

    let installed = agent_install::install(alias, &probe, &bytes, &sha, BUDGET_MB)
        .await
        .unwrap();
    println!("installed: {installed:?}");
    assert!(installed.running);
    let host_id = installed.host_id.unwrap();

    // A live session through the agent's own serve command.
    let config = EndpointConfig {
        lan: Some(alias.to_string()),
        tailscale: None,
        command: target.remote_command(),
    };
    println!("serve: {}", config.command.serve_command().unwrap());
    let local = HostId(uuid::Uuid::new_v4());
    let mut session = kelvo_remote::connect(&config, local, Some(host_id))
        .await
        .unwrap();
    println!(
        "Hello: host {} ({:?} {}), agent {}, epoch {}",
        session.hello.host.id,
        session.hello.host.info.os,
        session.hello.host.info.os_version,
        session.hello.app_version,
        session.hello.db_instance_uuid,
    );
    assert_eq!(session.hello.host.id, host_id);
    let interest = Message::Interest(RemoteInterest {
        detail: true,
        ..RemoteInterest::default()
    });
    session
        .writer
        .write_all(&kelvo_proto::encode_frame(&interest).unwrap())
        .await
        .unwrap();
    session.writer.flush().await.unwrap();
    let mut live = 0;
    while live < 3 {
        if let Message::Live(frame) = read(&mut session).await {
            live += 1;
            println!(
                "Live ts_ms={} values={} layout={}",
                frame.ts_ms,
                frame.values.len(),
                frame.layout_no
            );
        }
    }
    session.close().await;

    // A wrong expected id is a typed mismatch carrying both ids.
    let other = HostId(uuid::Uuid::new_v4());
    match kelvo_remote::connect(&config, local, Some(other)).await {
        Err(RemoteError::IdentityMismatch {
            expected, actual, ..
        }) => {
            assert_eq!((expected, actual), (other, host_id));
            println!("identity mismatch: expected {expected}, actual {actual}");
        }
        Err(e) => panic!("expected an identity mismatch, got {e}"),
        Ok(_) => panic!("expected an identity mismatch, got a session"),
    }

    let status = agent_install::status(alias, &target).await.unwrap();
    println!("status: {status:?}");
    let label = sh(alias, "ls -Z /usr/local/bin/kelvo-agent").await;
    assert!(label.contains(":bin_t:"), "{label}");
    sh(alias, "systemctl is-active kelvo-agent").await;

    // Update twice with the same bytes. The daemon commits history in batches, so the
    // first baseline can be empty; the first update's stop flushes it, which gives the
    // second a committed baseline to keep.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let mut counts = vec![rows(alias).await];
    for _ in 0..2 {
        let probe = agent_install::probe_linux(alias).await.unwrap();
        let updated = agent_install::update(alias, &probe, &bytes, &sha)
            .await
            .unwrap();
        println!("updated: {updated:?}");
        assert_eq!(updated.host_id, Some(host_id));
        assert_eq!(updated.epoch, status.epoch, "the update kept the history");
        counts.push(rows(alias).await);
    }
    println!("history rows before, after the first and after the second update: {counts:?}");
    assert!(counts.windows(2).all(|w| w[1] >= w[0]), "{counts:?}");
    assert!(counts[1] > 0, "{counts:?}");
    let leftovers = sh(
        alias,
        "ls -a /usr/local/bin | grep kelvo; ls /var/lib/kelvo-agent",
    )
    .await;
    assert!(!leftovers.contains(".prev") && !leftovers.contains("backup"));
    let label = sh(alias, "ls -Z /usr/local/bin/kelvo-agent").await;
    assert!(label.contains(":bin_t:"), "{label}");
    let avc = sh(alias, "ausearch -m avc -ts recent -c kelvo-agent").await;
    assert!(avc.contains("<no matches>"), "SELinux denials: {avc}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "installs the agent on a real host; see the module docs"]
async fn linux_agent_lifecycle_on_a_real_host() {
    let alias = std::env::var("KELVO_QA_ALIAS").unwrap_or_else(|_| "linux-tailscale".into());
    let path = std::env::var("KELVO_QA_AGENT").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!("agent {path}: {} bytes, sha256 {sha}", bytes.len());
    let outcome = tokio::spawn(lifecycle(alias.clone(), bytes, sha)).await;

    // Always leave the host clean.
    let report = agent_install::uninstall(&alias, true).await;
    println!("uninstall: {report:#?}");
    let left = sh(
        &alias,
        "ls -d /usr/local/bin/kelvo-agent /usr/local/bin/.kelvo-agent.prev /etc/systemd/system/kelvo-agent.service /var/lib/kelvo-agent /run/kelvo-agent; systemctl status kelvo-agent",
    )
    .await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic.into_panic());
    }
    assert!(report.is_clean(), "{report:?}");
    assert!(!left.contains("/kelvo-agent "), "{left}");
    assert_eq!(
        left.matches("No such file or directory").count(),
        5,
        "{left}"
    );
    assert!(left.contains("could not be found"), "{left}");
}
