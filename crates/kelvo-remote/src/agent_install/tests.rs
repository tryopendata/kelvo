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

/// `state/fail-stop` makes `stop` fail; `state/no-user-manager` makes the user manager
/// report `offline`.
const SYSTEMCTL: &str = r#"#!/bin/sh
s=$KELVO_FAKE_STATE
echo "systemctl $*" >> "$s/log"
user=0
[ "$1" = --user ] && { user=1; shift; }
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
  is-system-running)
    if [ $user = 1 ] && [ -e "$s/no-user-manager" ]; then echo offline; else echo running; fi;;
  enable) [ "$2" = --now ] && start;;
  start) start;;
  stop) [ -e "$s/fail-stop" ] && { echo "Failed to stop kelvo-agent.service" >&2; exit 1; }
    rm -f "$s/running";;
  disable) rm -f "$s/running";;
esac
exit 0
"#;

/// `cp` fails while `state/fail-cp` exists. `uname -m` says aarch64 where macOS says
/// arm64, so validation sees a supported architecture under this machine's sh too.
const STUBS: [(&str, &str); 5] = [
    (
        "cp",
        "#!/bin/sh\nif [ -e \"$KELVO_FAKE_STATE/fail-cp\" ]; then echo 'cp: No space left on device' >&2; exit 1; fi\nexec /bin/cp \"$@\"\n",
    ),
    (
        "uname",
        "#!/bin/sh\nfor u in /usr/bin/uname /bin/uname; do if [ -x \"$u\" ]; then break; fi; done\nif [ \"$1\" = -m ]; then m=$(\"$u\" -m); if [ \"$m\" = arm64 ]; then m=aarch64; fi; echo \"$m\"; else exec \"$u\" \"$@\"; fi\n",
    ),
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

#[tokio::test]
async fn capable_uninstall_preserves_structured_failures_over_the_shell() {
    let shell = Shell::local();
    let target = shell.probe().await.target().unwrap();
    for report in [
        r#"{"steps":[{"name":"binary","outcome":{"outcome":"failed","detail":"Permission denied"}},{"name":"data_dir","outcome":{"outcome":"failed","detail":"Read-only file system"}}]}"#,
        "{}",
    ] {
        let agent = format!(
            "#!/bin/sh\ncase $1 in\nversion) echo '{{\"automatic_updates\":true}}';;\nuninstall) echo 'warning: Permission denied' >&2; printf '%s\\n' {};;\nesac\n",
            shell_quote(report)
        );
        shell.setup(&format!(
            "mkdir -p {}; printf '%s' {} > {}; chmod 755 {}",
            q(shell.paths.binary.parent().unwrap()),
            shell_quote(&agent),
            q(&shell.paths.binary),
            q(&shell.paths.binary)
        ));
        let result = super::flows::uninstall_target(&shell, &target, true).await;
        assert!(!result.is_clean(), "{result:?}");
        if report != "{}" {
            assert!(
                result.steps.iter().any(|step| step.name == "data_dir"
                    && step.outcome == StepOutcome::Failed("Read-only file system".into())),
                "{result:?}"
            );
            assert!(
                result.steps.iter().any(|step| step.name == "binary"
                    && step.outcome == StepOutcome::Failed("Permission denied".into())),
                "{result:?}"
            );
        }
        assert!(
            result
                .steps
                .iter()
                .any(|step| step.name == "reload" && step.outcome == StepOutcome::Ok)
        );
    }
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
    assert!(!probe.data_exists);
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
    assert!(probe.data_exists, "a later failed install must keep it");
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

/// Updates that fail before replacing anything: `stop` fails while a complete backup
/// from an earlier rollback is still there, and copying the history aside fails. Both
/// leave the current history and binary alone, restart the agent, report it restored
/// and leave no backup behind.
async fn failed_backups(shell: Shell) {
    let paths = shell.paths.clone();
    let state = shell.base.join("state");
    let backup = paths.data_dir.join("backup");
    let v1 = fake_agent("v1", true);
    install_with(&shell, &shell.probe().await, &v1, &sha(&v1), 150)
        .await
        .unwrap();
    let probe = shell.probe().await;
    let target = probe.target().unwrap();
    shell.write(&paths.data_dir.join("history.sqlite"), "current history");
    shell.write(&paths.data_dir.join("history.sqlite-wal"), "current wal");
    let v2 = fake_agent("v2", true);
    let expect_restored = |result: super::Result<_>, step: &str| match result {
        Err(AgentInstallError::UpdateFailed { restored, cause }) => {
            assert!(restored, "{step}: the restart alone decides, after {cause}");
        }
        other => panic!("{step}: expected a failed update, got {other:?}"),
    };
    let expect_untouched = |step: &str| {
        assert_eq!(
            shell
                .read(&paths.data_dir.join("history.sqlite"))
                .as_deref(),
            Some("current history"),
            "{step}"
        );
        assert_eq!(
            shell
                .read(&paths.data_dir.join("history.sqlite-wal"))
                .as_deref(),
            Some("current wal"),
            "{step}"
        );
        assert_eq!(shell.read(&paths.binary).unwrap().as_bytes(), v1, "{step}");
        assert!(!shell.exists(&backup), "{step}: the backup remains");
    };

    // A stale backup from an earlier rollback, then `stop` fails.
    shell.setup(&format!(
        "mkdir -p {b}; printf 'stale history' > {b}/history.sqlite; : > {b}/complete; : > {s}/fail-stop; chmod -R a+rwX {b}",
        b = q(&backup),
        s = q(&state),
    ));
    expect_restored(
        update_with(&shell, &probe, &v2, &sha(&v2)).await,
        "stop fails",
    );
    expect_untouched("stop fails");
    assert!(
        super::flows::status_with(&shell, &target)
            .await
            .unwrap()
            .running
    );

    // Copying the history aside fails part-way.
    shell.setup(&format!(
        "rm -f {s}/fail-stop; : > {s}/fail-cp; chmod a+rw {s}/fail-cp",
        s = q(&state)
    ));
    expect_restored(
        update_with(&shell, &probe, &v2, &sha(&v2)).await,
        "cp fails",
    );
    expect_untouched("cp fails");
}

#[tokio::test]
async fn failed_backups_under_this_machines_sh() {
    failed_backups(Shell::local()).await;
}

#[tokio::test]
#[ignore = "needs docker; make remote-scripts-test"]
async fn failed_backups_under_dash_as_root() {
    failed_backups(Shell::docker("debian:bookworm-slim", None)).await;
}

#[tokio::test]
#[ignore = "needs docker; make remote-scripts-test"]
async fn failed_backups_under_busybox_as_a_user() {
    failed_backups(Shell::docker("busybox:stable", Some(1000))).await;
}

/// Update and remove take the probe unvalidated: a user whose manager is offline
/// cannot take a new install, but its agent can still be updated or removed.
#[tokio::test]
async fn only_an_install_validates_the_probe() {
    let shell = Shell::local();
    shell.setup(&format!(
        ": > {}",
        q(&shell.base.join("state/no-user-manager"))
    ));
    let probe = |budget| super::probe::probe_linux_with(&shell, budget);
    assert!(matches!(
        probe(Some(150)).await,
        Err(AgentInstallError::NoUserManager { .. })
    ));
    let unvalidated = probe(None).await.unwrap();
    assert_eq!(unvalidated.user_state.as_deref(), Some("offline"));
}

#[test]
fn install_waits_as_long_as_update_for_the_daemon() {
    let probe = LinuxProbe::parse(ASAHI_PROBE).unwrap();
    let w = probe.target().unwrap().words().unwrap();
    let polls = |script: &str| {
        script
            .lines()
            .find_map(|l| l.strip_prefix("while [ $i -lt "))
            .and_then(|rest| rest.split(' ').next())
            .map(str::to_owned)
    };
    // 4 polls a second: the daemon may wait 3 s for Docker names before it binds.
    assert_eq!(
        polls(&super::unit::activate(&w, "x")).as_deref(),
        Some("40")
    );
    assert_eq!(polls(&super::unit::restart(&w)).as_deref(), Some("40"));
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
    assert!(probe.docker == super::DockerSocket::Readable && probe.restorecon);
    assert!(!probe.data_exists);
    assert!(probe.other_user_agents.is_empty());
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
        Err(AgentInstallError::OtherLayout { existing: AgentLayout::User, binary, user: None })
            if binary == "/root/.local/bin/kelvo-agent"
    ));
    // ...including another login's user service, which root's probe also looks for.
    let other = with("other_user_agent=ann active /home/ann/.local/bin/kelvo-agent\n");
    assert_eq!(
        other.other_user_agents,
        [super::OtherUserAgent {
            user: "ann".into(),
            binary: "/home/ann/.local/bin/kelvo-agent".into(),
            active: true,
        }]
    );
    let err = other.validate(150).unwrap_err();
    assert!(
        matches!(&err, AgentInstallError::OtherLayout { existing: AgentLayout::User, user: Some(u), .. } if u == "ann"),
        "{err:?}"
    );
    assert!(
        err.to_string()
            .contains("installed as user for ann at /home/ann/.local/bin/kelvo-agent"),
        "{err}"
    );
    assert!(
        !with("other_user_agent=bob  /home/bob/.local/bin/kelvo-agent\n").other_user_agents[0]
            .active,
        "no user manager to ask"
    );
    // The data directory a removed host's agent kept, and the Docker socket's states.
    assert!(with("data_exists=yes\n").data_exists);
    assert_eq!(
        with("docker=unreadable\n").docker,
        super::DockerSocket::Unreadable
    );
    assert_eq!(with("docker=absent\n").docker, super::DockerSocket::Absent);
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
    send(&super::probe_host("a", Some(150)));
    send(&super::uninstall_at("a", &target, true));
    send(&super::reset_identity("a", &probe));
}
