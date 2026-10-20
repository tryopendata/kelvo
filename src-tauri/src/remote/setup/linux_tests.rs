//! The Linux setup path: saved-host compatibility, preparation, error mapping, and a
//! by-hand run against a real server.
use super::*;
use kelvo_remote::agent_install::AgentInstallError;

/// The linux-tailscale probe as `LinuxProbe` serializes it (root, Fedora Asahi).
fn asahi_probe() -> LinuxProbe {
    serde_json::from_value(serde_json::json!({
        "arch": "aarch64",
        "kernel": "6.17.4-400.asahi.fc41.aarch64+16k",
        "hostname": "asahi",
        "pretty_name": "Fedora Linux Asahi Remix 41 (Server Edition)",
        "uid": 0,
        "user": "root",
        "home": "/root",
        "xdg_runtime_dir": null,
        "xdg_state_home": null,
        "xdg_config_home": null,
        "systemd_version": 256,
        "system_state": "degraded",
        "user_state": null,
        "linger": false,
        "selinux": "Enforcing",
        "restorecon": true,
        "digest": "sha256sum",
        "data_parent": "/var/lib",
        "disk_free_bytes": 14_554_480_640_u64,
        "disk_total_bytes": 161_008_844_800_u64,
        "cgroup_fs": "cgroup2fs",
        "docker_socket": true,
        "page_size": 16384,
        "system_agent": null,
        "user_agent": null
    }))
    .unwrap()
}

fn with_agent(mut probe: LinuxProbe, version: &str, id: HostId) -> LinuxProbe {
    probe.system_agent = Some(
        serde_json::from_value(serde_json::json!({
            "binary": "/usr/local/bin/kelvo-agent",
            "version": { "version": version, "proto": 1, "min_compatible": 1 },
            "status": {
                "running": true,
                "host_id": id,
                "epoch": null,
                "version": version,
            }
        }))
        .unwrap(),
    );
    probe
}

#[test]
fn a_root_login_prepares_a_system_service_with_the_disclosure_facts() {
    let prep = preparation("linux-tailscale", &asahi_probe(), &BTreeMap::new()).unwrap();
    assert_eq!(prep.kind, RemoteHostKind::Linux);
    assert!(!prep.installed && !prep.compatible);
    assert_eq!(prep.name, "asahi");
    let linux = prep.linux.unwrap();
    assert_eq!(linux.layout, RemoteAgentLayout::System);
    assert!(linux.linger_ok, "a system service does not need lingering");
    assert_eq!(linux.binary_path, "/usr/local/bin/kelvo-agent");
    assert_eq!(linux.data_dir, "/var/lib/kelvo-agent");
    assert_eq!(linux.selinux.as_deref(), Some("Enforcing"));
    assert_eq!(
        linux.distro.as_deref(),
        Some("Fedora Linux Asahi Remix 41 (Server Edition)")
    );
    assert_eq!(linux.budget_mb, AGENT_BUDGET_MB);
    // The 2 GB low-disk reserve plus the budget.
    let needed = linux.needed_bytes.unwrap();
    assert!(needed > u64::from(AGENT_BUDGET_MB) * 1_000_000, "{needed}");
    assert!(linux.free_bytes.unwrap() > needed);
    assert!(linux.warnings.is_empty());
    assert_eq!(linux.duplicate_of, None);
}

#[test]
fn a_user_login_without_linger_carries_the_enable_linger_command() {
    let mut probe = asahi_probe();
    probe.uid = 1000;
    probe.user = "riley".into();
    probe.home = Some("/home/riley".into());
    probe.xdg_runtime_dir = Some("/run/user/1000".into());
    probe.user_state = Some("running".into());
    let linux = preparation("box", &probe, &BTreeMap::new())
        .unwrap()
        .linux
        .unwrap();
    assert_eq!(linux.layout, RemoteAgentLayout::User);
    assert!(!linux.linger_ok);
    assert_eq!(linux.binary_path, "/home/riley/.local/bin/kelvo-agent");
    assert!(
        linux
            .warnings
            .iter()
            .any(|w| w.contains("sudo loginctl enable-linger riley")),
        "{:?}",
        linux.warnings
    );
}

#[test]
fn an_installed_agent_is_compared_with_this_build_and_its_id_with_saved_hosts() {
    let id = HostId(Uuid::new_v4());
    let current = with_agent(asahi_probe(), env!("CARGO_PKG_VERSION"), id);
    let prep = preparation("linux-tailscale", &current, &BTreeMap::new()).unwrap();
    assert!(prep.installed && prep.compatible);
    assert_eq!(prep.host_id, Some(id.to_string()));
    let linux = prep.linux.unwrap();
    assert_eq!(
        linux.needed_bytes, None,
        "its history already has its share"
    );
    assert_eq!(
        linux.existing_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );

    let old = with_agent(asahi_probe(), "0.0.1", id);
    let prep = preparation("linux-tailscale", &old, &BTreeMap::new()).unwrap();
    assert!(prep.installed && !prep.compatible, "an older agent updates");

    let mut saved = BTreeMap::new();
    saved.insert(id, saved_host(id, "Basement server", agent_command()));
    let prep = preparation("linux-tailscale", &current, &saved).unwrap();
    assert_eq!(
        prep.linux.unwrap().duplicate_of.as_deref(),
        Some("Basement server")
    );
}

fn agent_command() -> RemoteCommand {
    RemoteCommand::Agent {
        bin: "/usr/local/bin/kelvo-agent".into(),
        system: true,
        socket: None,
    }
}

fn saved_host(id: HostId, name: &str, command: RemoteCommand) -> SavedHost {
    let mut record = kelvo_engine::host_record(id);
    record.is_local = false;
    record.display_name = name.into();
    SavedHost {
        config: RemoteHostConfig {
            id: id.to_string(),
            name: name.into(),
            lan: None,
            tailscale: Some("linux-tailscale".into()),
            enabled: true,
            mirror_budget_mb: 100,
        },
        record,
        command,
    }
}

#[test]
fn hosts_saved_before_linux_support_load_as_mac_helpers() {
    let id = HostId(Uuid::new_v4());
    let host = saved_host(id, "Studio Mac", RemoteCommand::default());
    // The file as builds before Linux support wrote it: `helper_path`, no `command`.
    let old = serde_json::json!({
        id.to_string(): {
            "config": host.config,
            "record": host.record,
            "helper_path": "/Applications/Kelvo.app/Contents/MacOS/kelvo-remote-helper",
        }
    });
    let (saved, errors) = parse_saved(&serde_json::to_vec_pretty(&old).unwrap());
    assert!(errors.is_empty(), "{errors:?}");
    let loaded = saved.get(&id).unwrap();
    assert_eq!(
        loaded.command,
        RemoteCommand::MacHelper(
            "/Applications/Kelvo.app/Contents/MacOS/kelvo-remote-helper".into()
        )
    );
    assert!(!loaded.is_linux());

    // A Linux host round-trips its agent command.
    let linux = saved_host(id, "asahi", agent_command());
    let bytes = serde_json::to_vec(&BTreeMap::from([(id, linux)])).unwrap();
    let (saved, _) = parse_saved(&bytes);
    assert!(saved.get(&id).unwrap().is_linux());
}

#[test]
fn one_unreadable_saved_host_does_not_drop_the_others() {
    let good = HostId(Uuid::new_v4());
    let host = saved_host(good, "Studio Mac", RemoteCommand::default());
    let file = serde_json::json!({
        good.to_string(): host,
        Uuid::new_v4().to_string(): { "config": "from a newer build" },
        "not-a-uuid": host,
    });
    let (saved, errors) = parse_saved(&serde_json::to_vec(&file).unwrap());
    assert_eq!(saved.len(), 1);
    assert!(saved.contains_key(&good));
    assert_eq!(errors.len(), 2, "{errors:?}");
    let (saved, errors) = parse_saved(b"not json");
    assert!(saved.is_empty() && errors.len() == 1);
}

#[test]
fn agent_install_errors_map_to_kinds_the_ui_explains() {
    use kelvo_remote::AgentLayout;
    let kind = |e: AgentInstallError| {
        serde_json::to_value(CommandError::from(e)).unwrap()["kind"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    for (error, expected) in [
        (
            AgentInstallError::UnsupportedArch("riscv64".into()),
            "remote_incompatible",
        ),
        (
            AgentInstallError::NoSystemd {
                detail: "no systemctl".into(),
            },
            "remote_incompatible",
        ),
        (AgentInstallError::NoDigestTool, "remote_incompatible"),
        (
            AgentInstallError::LowDisk {
                path: "/var/lib".into(),
                free_bytes: 1,
                needed_bytes: 2,
            },
            "remote_low_disk",
        ),
        (
            AgentInstallError::OtherLayout {
                existing: AgentLayout::User,
                binary: "/home/r/.local/bin/kelvo-agent".into(),
            },
            "remote_agent_conflict",
        ),
        (
            AgentInstallError::NotRunning {
                detail: "no answer".into(),
                status: None,
            },
            "remote_agent_not_running",
        ),
        (
            AgentInstallError::DigestMismatch {
                expected: "ab".into(),
            },
            "remote_unreachable",
        ),
        (
            AgentInstallError::Remote(kelvo_remote::RemoteError::Authentication("key".into())),
            "remote_auth",
        ),
        (
            AgentInstallError::Remote(kelvo_remote::RemoteError::AgentNotRunning("s".into())),
            "remote_agent_not_running",
        ),
    ] {
        assert_eq!(kind(error), expected);
    }
    let failed = CommandError::from(AgentInstallError::UpdateFailed {
        cause: Box::new(AgentInstallError::NoDigestTool),
        restored: true,
    });
    assert!(matches!(
        failed,
        CommandError::RemoteAgentUpdateFailed { restored: true, .. }
    ));
}

#[test]
fn only_an_older_agent_offers_an_update() {
    use super::super::commands::older_version;
    assert!(older_version("0.1.0", "0.2.0"));
    assert!(older_version("0.9.9", "0.10.0"));
    assert!(!older_version("0.2.0", "0.2.0"));
    assert!(!older_version("0.3.0", "0.2.0"));
    assert!(!older_version("0.2.0+abc", "0.2.0"));
    assert!(
        older_version("garbage", "0.2.0"),
        "unparsed parts compare as 0"
    );
}

/// The Linux path end to end against a real server, with the binaries from
/// `make agent-linux`: the probe, install, a session and live frames through
/// `RemoteSource`, an update, an identity reset (the re-pair state), and an uninstall
/// with purge that leaves the server as it was. Needs `ssh $KELVO_QA_ALIAS` (default
/// `linux-tailscale`) to reach a Linux host with no Kelvo agent installed.
#[tokio::test]
#[ignore = "needs a real Linux server over SSH and `make agent-linux`; run by hand"]
async fn linux_host_lifecycle_against_a_real_server() {
    let alias = std::env::var("KELVO_QA_ALIAS").unwrap_or_else(|_| "linux-tailscale".into());
    let config = RemoteHostConfig {
        id: String::new(),
        name: alias.clone(),
        lan: None,
        tailscale: Some(alias.clone()),
        enabled: true,
        mirror_budget_mb: 100,
    };
    let (route, probe) = linux_probe(None, Some(alias.clone())).await.unwrap();
    assert!(
        probe.system_agent.is_none() && probe.user_agent.is_none(),
        "the server already has an agent; this test only runs against a clean server"
    );
    let prep = preparation(&route, &probe, &BTreeMap::new()).unwrap();
    println!("prepare: {}", serde_json::to_string_pretty(&prep).unwrap());

    let triple = agent_bundle::agent_triple(&probe.arch).unwrap();
    let (path, bytes) = agent_bundle::built_agent(triple).unwrap();
    println!("binary: {} ({} bytes)", path.display(), bytes.len());
    let binary = AgentBinary::new(bytes);

    let installed = install_agent(&route, &probe, Some(&binary)).await.unwrap();
    assert_eq!(installed, Installed::Fresh);
    println!("install: fresh");

    let result = lifecycle(&route, &config, &binary).await;
    let report = agent_install::uninstall(&route, true).await;
    println!("uninstall --purge: {report:?}");
    let after = agent_install::probe_linux(&route).await.unwrap();
    result.unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert!(after.system_agent.is_none() && after.user_agent.is_none());
    println!("after uninstall: no agent in either layout");
}

async fn lifecycle(
    alias: &str,
    config: &RemoteHostConfig,
    binary: &AgentBinary,
) -> Result<(), String> {
    let local = HostId(Uuid::new_v4());
    let probe = agent_install::probe_linux(alias)
        .await
        .map_err(|e| e.to_string())?;
    let target = probe.target().map_err(|e| e.to_string())?;
    let command = target.remote_command();
    let started = std::time::Instant::now();
    let session = connect(config, command.clone(), local, None)
        .await
        .map_err(|e| e.to_string())?;
    let hello = session.hello.clone();
    session.close().await;
    println!(
        "connect: {} {} as {} ({:?}, kernel {:?}) app_version {} in {:?}",
        hello.host.info.os_version,
        hello.host.info.chip.as_deref().unwrap_or("?"),
        hello.host.id,
        hello.host.info.os,
        hello.host.info.kernel,
        hello.app_version,
        started.elapsed()
    );

    // Live frames through the same source the app registers.
    let record = HostRecord::from_identity(hello.host.clone(), false);
    let frames = live_frames(record.clone(), local, &command, config).await?;
    println!("live: {frames}");

    let updated = agent_install::update(alias, &probe, &binary.bytes, &binary.sha256)
        .await
        .map_err(|e| e.to_string())?;
    println!(
        "update: running={} id={:?} version={}",
        updated.running, updated.host_id, updated.version
    );
    if updated.host_id != Some(hello.host.id) {
        return Err("the update changed the host id".into());
    }
    let session = connect(config, command.clone(), local, Some(hello.host.id))
        .await
        .map_err(|e| e.to_string())?;
    session.close().await;
    println!("reconnect after update: same id");

    let reset = agent_install::reset_identity(alias, &probe)
        .await
        .map_err(|e| e.to_string())?;
    println!("reset-identity: new id {:?}", reset.host_id);
    match connect(config, command.clone(), local, Some(hello.host.id)).await {
        Err(CommandError::RemoteIdentity { message }) => {
            println!("paired connect after reset: identity mismatch ({message})");
        }
        Err(other) => return Err(format!("expected an identity mismatch, got {other:?}")),
        Ok(_) => return Err("the old id still connected after reset".into()),
    }
    let session = connect(config, command, local, None)
        .await
        .map_err(|e| e.to_string())?;
    let new_id = session.hello.host.id;
    session.close().await;
    println!("re-pair connect: {new_id}");
    if Some(new_id) != reset.host_id || new_id == hello.host.id {
        return Err("re-pair did not see the new id".into());
    }
    Ok(())
}

async fn live_frames(
    record: HostRecord,
    local: HostId,
    command: &RemoteCommand,
    config: &RemoteHostConfig,
) -> Result<String, String> {
    let source = RemoteSource::new(
        record.clone(),
        local,
        EndpointConfig {
            lan: config.lan.clone(),
            tailscale: config.tailscale.clone(),
            command: command.clone(),
        },
        Arc::new(|s| {
            if let Some(e) = &s.error {
                println!("  source: {e}");
            }
        }),
        None,
    );
    let registry = crate::hosts::HostRegistry::new();
    let entry = registry.insert(record, source);
    entry.start(None).map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    let mut seen = Vec::new();
    while started.elapsed() < std::time::Duration::from_secs(20) && seen.len() < 3 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if let Some(frame) = entry.latest_frame()
            && seen.last() != Some(&frame.ts_ms)
        {
            seen.push(frame.ts_ms);
        }
    }
    let first = entry.latest_frame();
    let entry2 = entry.clone();
    tokio::task::spawn_blocking(move || entry2.stop())
        .await
        .map_err(|e| e.to_string())?;
    let frame = first.ok_or("no live frame within 20 s")?;
    if seen.len() < 2 {
        return Err(format!("only {} live frames in 20 s", seen.len()));
    }
    Ok(format!(
        "{} frames (ts_ms {:?}) within {:?}, {} series in the last",
        seen.len(),
        seen,
        started.elapsed(),
        frame.layout.series.len()
    ))
}
