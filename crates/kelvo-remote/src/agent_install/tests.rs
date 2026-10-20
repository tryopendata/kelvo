//! The scripts the controller sends, run by real shells against stub `systemctl`,
//! `loginctl`, `getenforce` and `restorecon`, with a shell-script stand-in for the agent
//! that keeps its state in files.
//!
//! The local variant runs under this machine's `/bin/sh` (dash on the ubuntu CI job,
//! bash in POSIX mode on macOS). The Docker variants run dash (debian) and busybox ash,
//! as root (the system layout) and as uid 1000 (the user layout): `make
//! remote-scripts-test`.
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use super::flows::{install_with, reset_identity_with, uninstall_with, update_with};
use super::probe::{LinuxProbe, probe_with};
use super::upload::upload_with;
use super::{AgentInstallError, Output, Remote, StepOutcome};
use crate::transport::shell_quote;
use crate::{AgentEnv, AgentLayout, AgentPaths};

const HOST_ID: &str = "11111111-1111-4111-8111-111111111111";
const NEW_ID: &str = "22222222-2222-4222-8222-222222222222";
const EPOCH: &str = "33333333-3333-4333-8333-333333333333";

const SYSTEMCTL: &str = r#"#!/bin/sh
s=$KELVO_FAKE_STATE
echo "systemctl $*" >> "$s/log"
[ "$1" = --user ] && shift
start() {
  if grep -q "health: good" "$KELVO_FAKE_BIN"; then
    mkdir -p "$KELVO_FAKE_DATA"
    [ -e "$KELVO_FAKE_DATA/host-id" ] || echo HOST_ID > "$KELVO_FAKE_DATA/host-id"
    [ -e "$s/epoch" ] || echo EPOCH > "$s/epoch"
    : > "$s/running"
  else
    # A broken build: it migrates the history, then never answers on its socket.
    echo migrated > "$KELVO_FAKE_DATA/history.sqlite"
    rm -f "$KELVO_FAKE_DATA/history.sqlite-wal"
  fi
}
case $1 in
  --version) echo "systemd 256 (256.4-1.fc41)";;
  is-system-running) echo running;;
  enable) [ "$2" = --now ] && start;;
  start) start;;
  stop|disable) rm -f "$s/running";;
esac
exit 0
"#;

const STUBS: [(&str, &str); 3] = [
    ("loginctl", "#!/bin/sh\necho no\n"),
    ("getenforce", "#!/bin/sh\necho Enforcing\n"),
    (
        "restorecon",
        "#!/bin/sh\necho \"restorecon $*\" >> \"$KELVO_FAKE_STATE/log\"\n",
    ),
];

/// The stand-in agent. `health: good|bad` decides whether the stub systemd starts it.
fn fake_agent(build: &str, good: bool) -> Vec<u8> {
    let health = if good { "good" } else { "bad" };
    format!(
        r#"#!/bin/sh
# health: {health}
s=$KELVO_FAKE_STATE
cmd=$1; shift
data=$HOME/.local/state/kelvo-agent; sock=; pos=; purge=0
while [ $# -gt 0 ]; do
  case $1 in
    --system) data=/var/lib/kelvo-agent;;
    --data-dir) data=$2; shift;;
    --socket) sock=$2; shift;;
    --purge) purge=1;;
    --json) ;;
    *) pos=$1;;
  esac
  shift
done
echo "agent {build} $cmd" >> "$s/log"
case $cmd in
  version) echo '{{"version":"{build}","proto":1,"min_compatible":1}}';;
  status)
    id=$(head -n 1 "$data/host-id" 2>/dev/null)
    if [ -n "$id" ]; then id="\"$id\""; else id=null; fi
    if [ -e "$s/running" ]; then
      echo "{{\"running\":true,\"host_id\":$id,\"epoch\":\"$(cat "$s/epoch")\",\"version\":\"{build}\",\"daemon_version\":\"{build}\",\"layout\":\"x\",\"data_dir\":\"$data\",\"socket\":\"$sock\"}}"
    else
      echo "{{\"running\":false,\"host_id\":$id,\"epoch\":null,\"version\":\"{build}\"}}"
    fi;;
  install-unit)
    mkdir -p "${{KELVO_FAKE_UNIT%/*}}"
    echo "ExecStart=$0 run $data $sock" > "$KELVO_FAKE_UNIT"
    echo written;;
  configure) mkdir -p "$data"; printf "%s" "$pos" > "$data/settings.json";;
  uninstall) rm -f "$KELVO_FAKE_UNIT"; if [ $purge = 1 ]; then rm -rf "$data"; fi; echo "{{}}";;
  reset-identity) echo {NEW_ID} > "$data/host-id"; cat "$data/host-id";;
esac
exit 0
"#
    )
    .into_bytes()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

enum Kind {
    Local,
    Docker { container: String, uid: Option<u32> },
}

/// A shell standing in for the SSH login: it runs the controller's command line as
/// the login shell would, with the stubs first on `PATH`.
struct Shell {
    kind: Kind,
    /// The fixture root as the shell sees it: stubs, state, home.
    base: PathBuf,
    paths: AgentPaths,
    _dir: Option<tempfile::TempDir>,
}

impl Drop for Shell {
    fn drop(&mut self) {
        if let Kind::Docker { container, .. } = &self.kind {
            let _ = std::process::Command::new("docker")
                .args(["rm", "-f", container])
                .output();
        }
    }
}

impl Shell {
    fn local() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let shell = Self {
            kind: Kind::Local,
            paths: user_paths(&base),
            base,
            _dir: Some(dir),
        };
        shell.install_stubs();
        shell
    }

    /// A container of `image` whose login is root (the system layout) or `uid`.
    fn docker(image: &str, uid: Option<u32>) -> Self {
        let out = std::process::Command::new("docker")
            .args(["run", "-d", "--rm", image, "sleep", "600"])
            .output()
            .expect("docker is needed for this test");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let container = String::from_utf8(out.stdout).unwrap().trim().to_string();
        let base = PathBuf::from("/k");
        let paths = match uid {
            None => AgentLayout::System.paths(&AgentEnv::default()).unwrap(),
            Some(_) => user_paths(&base),
        };
        let shell = Self {
            kind: Kind::Docker { container, uid },
            base,
            paths,
            _dir: None,
        };
        shell.install_stubs();
        shell
    }

    fn install_stubs(&self) {
        let stubs = self.base.join("stubs");
        let state = self.base.join("state");
        let home = self.base.join("home");
        let mut setup = format!("set -e; mkdir -p {} {} {}", q(&stubs), q(&state), q(&home));
        let systemctl = SYSTEMCTL
            .replace("HOST_ID", HOST_ID)
            .replace("EPOCH", EPOCH);
        for (name, text) in std::iter::once(("systemctl", systemctl.as_str())).chain(STUBS) {
            let path = stubs.join(name);
            setup.push_str(&format!(
                "; printf '%s' {} > {}; chmod 755 {}",
                shell_quote(text),
                q(&path),
                q(&path)
            ));
        }
        if let Kind::Docker { uid: Some(uid), .. } = self.kind {
            setup.push_str(&format!("; chown -R {uid} {}", q(&self.base)));
        }
        self.setup(&setup);
    }

    fn env(&self) -> Vec<(String, String)> {
        let s = |p: &Path| p.to_string_lossy().into_owned();
        vec![
            (
                "PATH".into(),
                format!(
                    "{}:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                    s(&self.base.join("stubs"))
                ),
            ),
            ("HOME".into(), s(&self.base.join("home"))),
            ("KELVO_FAKE_STATE".into(), s(&self.base.join("state"))),
            ("KELVO_FAKE_BIN".into(), s(&self.paths.binary)),
            ("KELVO_FAKE_DATA".into(), s(&self.paths.data_dir)),
            ("KELVO_FAKE_UNIT".into(), s(&self.paths.unit)),
        ]
    }

    fn command(&self, command: &str, root: bool) -> std::process::Command {
        match &self.kind {
            Kind::Local => {
                let mut c = std::process::Command::new("/bin/sh");
                c.arg("-c").arg(command).env_clear().envs(self.env());
                c
            }
            Kind::Docker { container, uid } => {
                let mut c = std::process::Command::new("docker");
                c.args(["exec", "-i"]);
                if let (Some(uid), false) = (uid, root) {
                    c.args(["-u", &uid.to_string()]);
                }
                for (k, v) in self.env() {
                    c.arg("-e").arg(format!("{k}={v}"));
                }
                c.args([container.as_str(), "/bin/sh", "-c", command]);
                c
            }
        }
    }

    /// Fixture setup, as root in a container.
    fn setup(&self, script: &str) {
        let out = self.command(script, true).output().unwrap();
        assert!(
            out.status.success(),
            "setup: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn std_command(&self, command: &str) -> std::process::Command {
        self.command(command, false)
    }

    /// A file's contents as the login user sees them; `None` when absent.
    fn read(&self, path: &Path) -> Option<String> {
        let out = self
            .std_command(&format!("cat {}", q(path)))
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn exists(&self, path: &Path) -> bool {
        self.std_command(&format!("test -e {}", q(path)))
            .status()
            .unwrap()
            .success()
    }

    fn write(&self, path: &Path, text: &str) {
        let out = self
            .std_command(&format!("printf '%s' {} > {}", shell_quote(text), q(path)))
            .output()
            .unwrap();
        assert!(out.status.success());
    }

    fn log(&self) -> String {
        self.read(&self.base.join("state/log")).unwrap_or_default()
    }

    async fn probe(&self) -> LinuxProbe {
        let mut probe = probe_with(self).await.unwrap();
        // macOS says arm64; the scripts do not care, validation does.
        if probe.arch == "arm64" {
            probe.arch = "aarch64".into();
        }
        probe
    }
}

impl Remote for Shell {
    async fn exec(
        &self,
        command: &str,
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> crate::Result<Output> {
        let command = tokio::process::Command::from(self.command(command, false));
        super::collect(command, stdin, timeout).await
    }
}

fn user_paths(base: &Path) -> AgentPaths {
    AgentLayout::User
        .paths(&AgentEnv {
            home: Some(base.join("home")),
            ..AgentEnv::default()
        })
        .unwrap()
}

fn q(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

/// Install, a rejected upload, a failed update that rolls back, a good update, an
/// identity reset and two uninstalls, on one shell.
async fn lifecycle(shell: Shell, layout: AgentLayout) {
    let paths = shell.paths.clone();
    let probe = shell.probe().await;
    assert_eq!(probe.layout(), layout);
    assert_eq!(probe.systemd_version, Some(256));
    assert_eq!(probe.system_state.as_deref(), Some("running"));
    assert_eq!(probe.selinux.as_deref(), Some("Enforcing"));
    assert!(probe.restorecon);
    assert!(probe.digest.is_some());
    assert!(probe.disk_free_bytes.is_some_and(|b| b > 0), "{probe:?}");
    assert_eq!(probe.existing(), None);
    assert_eq!(probe.target().unwrap().paths, paths);
    if layout == AgentLayout::User {
        assert_eq!(probe.user_state.as_deref(), Some("running"));
        assert_eq!(probe.linger, Some(false));
        assert!(probe.warnings()[0].contains("loginctl enable-linger"));
    }

    // Install.
    let v1 = fake_agent("v1", true);
    let status = install_with(&shell, &probe, &v1, &sha(&v1), 150)
        .await
        .unwrap();
    assert!(status.running);
    assert_eq!(status.host_id.unwrap().to_string(), HOST_ID);
    assert_eq!(status.epoch.as_deref(), Some(EPOCH));
    assert_eq!(shell.read(&paths.binary).unwrap().as_bytes(), v1);
    assert!(shell.exists(&paths.unit));
    assert_eq!(
        shell.read(&paths.data_dir.join("settings.json")).unwrap(),
        r#"{"history":{"size_limit_mb":150}}"#
    );
    let log = shell.log();
    assert!(log.contains("enable --now kelvo-agent.service"), "{log}");
    assert!(
        log.contains(&format!("restorecon -F {}", paths.binary.display())),
        "{log}"
    );
    let mode = shell
        .std_command(&format!("ls -l {}", q(&paths.binary)))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&mode.stdout).starts_with("-rwxr-xr-x"));
    let probe = shell.probe().await;
    let existing = probe.existing().unwrap();
    assert_eq!(existing.version.as_ref().unwrap().version, "v1");
    assert!(existing.status.as_ref().unwrap().running);
    assert!(matches!(
        install_with(&shell, &probe, &v1, &sha(&v1), 150).await,
        Err(AgentInstallError::AlreadyInstalled { .. })
    ));

    // A damaged upload replaces nothing and leaves nothing behind.
    let target = probe.target().unwrap();
    let v2 = fake_agent("v2", true);
    let wrong = sha(b"something else");
    assert!(matches!(
        upload_with(&shell, &target, &v2, &wrong).await,
        Err(AgentInstallError::DigestMismatch { expected }) if expected == wrong
    ));
    assert_eq!(shell.read(&paths.binary).unwrap().as_bytes(), v1);
    let bindir = paths.binary.parent().unwrap();
    assert!(!shell.exists(&bindir.join(".kelvo-agent.tmp")));

    // A build that migrates the history and never answers: both come back.
    shell.write(&paths.data_dir.join("history.sqlite"), "v1 history");
    shell.write(&paths.data_dir.join("history.sqlite-wal"), "v1 wal");
    let broken = fake_agent("v2", false);
    match update_with(&shell, &probe, &broken, &sha(&broken)).await {
        Err(AgentInstallError::UpdateFailed { cause, restored }) => {
            assert!(restored, "rollback failed after {cause}");
            assert!(
                matches!(*cause, AgentInstallError::NotRunning { .. }),
                "{cause}"
            );
        }
        other => panic!("expected a rolled-back update, got {other:?}"),
    }
    assert_eq!(shell.read(&paths.binary).unwrap().as_bytes(), v1);
    assert_eq!(
        shell.read(&paths.data_dir.join("history.sqlite")).unwrap(),
        "v1 history"
    );
    assert_eq!(
        shell
            .read(&paths.data_dir.join("history.sqlite-wal"))
            .unwrap(),
        "v1 wal"
    );
    assert!(!shell.exists(&paths.data_dir.join("backup")));
    assert!(!shell.exists(&bindir.join(".kelvo-agent.prev")));
    assert!(
        super::flows::status_with(&shell, &target)
            .await
            .unwrap()
            .running
    );

    // A good update keeps the history and the epoch.
    let status = update_with(&shell, &probe, &v2, &sha(&v2)).await.unwrap();
    assert_eq!(status.version, "v2");
    assert_eq!(status.epoch.as_deref(), Some(EPOCH));
    assert_eq!(shell.read(&paths.binary).unwrap().as_bytes(), v2);
    assert_eq!(
        shell.read(&paths.data_dir.join("history.sqlite")).unwrap(),
        "v1 history"
    );
    assert!(!shell.exists(&paths.data_dir.join("backup")));
    assert!(!shell.exists(&bindir.join(".kelvo-agent.prev")));
    let log = shell.log();
    assert!(log.contains("stop kelvo-agent.service"), "{log}");

    // Identity reset restarts under the new id.
    let status = reset_identity_with(&shell, &probe).await.unwrap();
    assert!(status.running);
    assert_eq!(status.host_id.unwrap().to_string(), NEW_ID);

    // Uninstall removes everything, and again finds nothing to do.
    let report = uninstall_with(&shell, true).await;
    assert!(report.is_clean(), "{report:?}");
    let outcome = |name: &str| {
        report
            .steps
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.outcome.clone())
    };
    assert_eq!(outcome("binary"), Some(StepOutcome::Ok));
    assert_eq!(outcome("agent"), Some(StepOutcome::Ok));
    for path in [&paths.binary, &paths.unit, &paths.data_dir] {
        assert!(!shell.exists(path), "{} remains", path.display());
    }
    let again = uninstall_with(&shell, true).await;
    assert!(again.is_clean(), "{again:?}");
    for name in ["stop", "agent", "unit", "binary"] {
        assert_eq!(
            again
                .steps
                .iter()
                .find(|s| s.name == name)
                .map(|s| &s.outcome),
            Some(&StepOutcome::Absent),
            "{name}: {again:?}"
        );
    }
}

#[tokio::test]
async fn lifecycle_under_this_machines_sh() {
    lifecycle(Shell::local(), AgentLayout::User).await;
}

#[tokio::test]
#[ignore = "needs docker; make remote-scripts-test"]
async fn lifecycle_under_dash_as_root() {
    lifecycle(
        Shell::docker("debian:bookworm-slim", None),
        AgentLayout::System,
    )
    .await;
}

#[tokio::test]
#[ignore = "needs docker; make remote-scripts-test"]
async fn lifecycle_under_dash_as_a_user() {
    lifecycle(
        Shell::docker("debian:bookworm-slim", Some(1000)),
        AgentLayout::User,
    )
    .await;
}

#[tokio::test]
#[ignore = "needs docker; make remote-scripts-test"]
async fn lifecycle_under_busybox_as_root() {
    lifecycle(Shell::docker("busybox:stable", None), AgentLayout::System).await;
}

#[tokio::test]
#[ignore = "needs docker; make remote-scripts-test"]
async fn lifecycle_under_busybox_as_a_user() {
    lifecycle(
        Shell::docker("busybox:stable", Some(1000)),
        AgentLayout::User,
    )
    .await;
}

/// The probe of linux-tailscale (root, Fedora Asahi), trimmed.
const ASAHI_PROBE: &str = r#"os=Linux
arch=aarch64
kernel=6.17.4-400.asahi.fc41.aarch64+16k
pretty_name=Fedora Linux Asahi Remix 41 (Server Edition)
uid=0
user=root
home=/root
xdg_runtime_dir=
xdg_state_home=
xdg_config_home=
systemd=256
system_state=degraded
linger=no
selinux=Enforcing
restorecon=yes
sha256=sha256sum
data_parent=/var/lib
disk_total_kb=157235200
disk_free_kb=14213360
cgroup_fs=cgroup2fs
docker=yes
page_size=16384
"#;

#[test]
fn probe_parses_and_validates() {
    let probe = LinuxProbe::parse(ASAHI_PROBE).unwrap();
    assert_eq!(probe.layout(), AgentLayout::System);
    assert_eq!(probe.page_size, Some(16384));
    assert_eq!(
        probe.pretty_name.as_deref(),
        Some("Fedora Linux Asahi Remix 41 (Server Edition)")
    );
    assert_eq!(
        probe.kernel.as_deref(),
        Some("6.17.4-400.asahi.fc41.aarch64+16k")
    );
    assert_eq!(probe.disk_free_bytes, Some(14_213_360 * 1024));
    assert_eq!(probe.cgroup_fs.as_deref(), Some("cgroup2fs"));
    assert!(probe.docker_socket && probe.restorecon);
    assert_eq!(probe.linger, Some(false));
    assert_eq!(probe.xdg_runtime_dir, None);
    // A degraded system still runs units.
    probe.validate(150).unwrap();
    assert!(probe.warnings().is_empty(), "root needs no linger");
    assert_eq!(
        probe.target().unwrap().remote_command(),
        crate::RemoteCommand::Agent {
            bin: "/usr/local/bin/kelvo-agent".into(),
            system: true,
            socket: None,
        }
    );

    let with = |extra: &str| LinuxProbe::parse(&format!("{ASAHI_PROBE}{extra}")).unwrap();
    // 14 GB free: a 13 GB budget plus the 2 GB reserve does not fit.
    assert!(matches!(
        probe.validate(13_000),
        Err(AgentInstallError::LowDisk { .. })
    ));
    // ...unless the agent is already there, holding its history.
    with("system_bin=/usr/local/bin/kelvo-agent\n")
        .validate(13_000)
        .unwrap();
    // One agent per host.
    assert!(matches!(
        with("user_bin=/root/.local/bin/kelvo-agent\n").validate(150),
        Err(AgentInstallError::OtherLayout { existing: AgentLayout::User, binary })
            if binary == "/root/.local/bin/kelvo-agent"
    ));
    assert!(matches!(
        with("arch=armv7l\n").validate(150),
        Err(AgentInstallError::UnsupportedArch(arch)) if arch == "armv7l"
    ));
    assert!(matches!(
        with("system_state=offline\n").validate(150),
        Err(AgentInstallError::NoSystemd { .. })
    ));
    let no_systemd: String = ASAHI_PROBE
        .lines()
        .filter(|l| !l.starts_with("systemd=") && !l.starts_with("system_state="))
        .map(|l| format!("{l}\n"))
        .collect();
    let err = LinuxProbe::parse(&no_systemd)
        .unwrap()
        .validate(150)
        .unwrap_err();
    assert!(matches!(err, AgentInstallError::NoSystemd { .. }));
    assert!(err.to_string().contains("not supported yet"), "{err}");
    assert!(matches!(
        with("sha256=\n").validate(150),
        Err(AgentInstallError::NoDigestTool)
    ));
    assert!(LinuxProbe::parse("os=Linux\n").is_err(), "no uid");
}

#[test]
fn user_probe_names_its_socket_and_needs_a_user_manager() {
    let text = "uid=1000\nuser=ann\nhome=/home/ann\narch=x86_64\nsystemd=255\n\
                system_state=running\nsha256=shasum\nxdg_runtime_dir=/run/user/1000\n";
    let probe = LinuxProbe::parse(text).unwrap();
    assert!(matches!(
        probe.validate(150),
        Err(AgentInstallError::NoUserManager { user, .. }) if user == "ann"
    ));
    let probe = LinuxProbe::parse(&format!("{text}user_state=running\nlinger=yes\n")).unwrap();
    probe.validate(150).unwrap();
    assert!(probe.warnings().is_empty());
    let target = probe.target().unwrap();
    assert_eq!(
        target.remote_command(),
        crate::RemoteCommand::Agent {
            bin: "/home/ann/.local/bin/kelvo-agent".into(),
            system: false,
            socket: Some("/run/user/1000/kelvo-agent/metrics.sock".into()),
        }
    );
    let words = target.words().unwrap();
    assert_eq!(
        words.all,
        r#" --data-dir "/home/ann/.local/state/kelvo-agent" --socket "/run/user/1000/kelvo-agent/metrics.sock""#
    );
    assert_eq!(words.systemctl, "systemctl --user");
}

#[test]
fn every_script_survives_the_login_shell() {
    let probe = LinuxProbe::parse(ASAHI_PROBE).unwrap();
    let target = probe.target().unwrap();
    let w = target.words().unwrap();
    for script in [
        super::probe::PROBE_SCRIPT.to_string(),
        super::upload::upload_script(&target, &"a".repeat(64)).unwrap(),
        super::unit::activate(&w, r#""{\"history\":{}}""#),
        super::unit::stop_and_back_up(&w),
        super::unit::restart(&w),
        super::unit::roll_back(&w),
        super::unit::reset_identity(&w),
        super::unit::uninstall(&w, true),
    ] {
        crate::transport::sh_c(&script).unwrap();
    }
    assert!(super::upload::upload_script(&target, "abc").is_err());
}

#[test]
fn flows_are_send() {
    fn send<T: Send>(_: &T) {}
    let probe = LinuxProbe::parse(ASAHI_PROBE).unwrap();
    let target = probe.target().unwrap();
    send(&super::install("a", &probe, &[], "", 150));
    send(&super::update("a", &probe, &[], ""));
    send(&super::uninstall("a", true));
    send(&super::upload_agent("a", &target, &[], ""));
    send(&super::probe_host("a", 150));
    send(&super::reset_identity("a", &probe));
}
