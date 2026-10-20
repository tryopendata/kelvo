//! What a host is and what it already has, from one read-only script.
use std::collections::BTreeMap;
use std::ffi::OsString;

use serde::{Deserialize, Serialize};

use super::{AgentInstallError, AgentTarget, Remote, Result, STEP_TIMEOUT, Ssh, run_script};
use crate::install::InstallProbe;
use crate::{AgentEnv, AgentLayout, RemoteError};

/// The probe. Prints `key=value` lines and changes nothing; every check that can fail
/// prints an empty value or nothing rather than stopping the script.
pub(crate) const PROBE_SCRIPT: &str = r#"
kv() { printf "%s=%s\n" "$1" "$2"; }
uid=$(id -u)
kv os "$(uname -s)"
kv arch "$(uname -m)"
kv uid "$uid"
kv user "$(id -un 2>/dev/null)"
kv home "${HOME:-}"
kv xdg_runtime_dir "${XDG_RUNTIME_DIR:-}"
kv xdg_state_home "${XDG_STATE_HOME:-}"
kv xdg_config_home "${XDG_CONFIG_HOME:-}"
if command -v systemctl >/dev/null 2>&1; then
  kv systemd "$(systemctl --version 2>/dev/null | { read -r _ v _; echo "$v"; })"
  kv system_state "$(systemctl is-system-running 2>/dev/null)"
  if [ "$uid" != 0 ]; then kv user_state "$(systemctl --user is-system-running 2>/dev/null)"; fi
fi
if command -v loginctl >/dev/null 2>&1; then
  kv linger "$(loginctl show-user "$(id -un 2>/dev/null)" -p Linger --value 2>/dev/null)"
fi
if command -v getenforce >/dev/null 2>&1; then kv selinux "$(getenforce 2>/dev/null)"; fi
if command -v restorecon >/dev/null 2>&1; then kv restorecon yes; fi
for tool in sha256sum shasum openssl; do
  if command -v $tool >/dev/null 2>&1; then kv sha256 $tool; break; fi
done
if [ "$uid" = 0 ]; then d=/var/lib/kelvo-agent
else case "${XDG_STATE_HOME:-}" in /*) d=$XDG_STATE_HOME/kelvo-agent;; *) d=${HOME:-/}/.local/state/kelvo-agent;; esac
fi
while [ ! -d "$d" ]; do d=$(dirname "$d"); done
kv data_parent "$d"
df -Pk "$d" 2>/dev/null | { read -r _; read -r _ total _ avail _; kv disk_total_kb "$total"; kv disk_free_kb "$avail"; }
kv cgroup_fs "$(stat -fc %T /sys/fs/cgroup 2>/dev/null)"
if [ -r /var/run/docker.sock ] && [ -w /var/run/docker.sock ]; then kv docker yes; fi
kv page_size "$(getconf PAGESIZE 2>/dev/null)"
agent() {
  if [ -e "$2" ]; then
    kv "$1_bin" "$2"
    if [ -x "$2" ]; then
      kv "$1_version" "$("$2" version --json 2>/dev/null | head -n 1)"
      kv "$1_status" "$("$2" status --json $3 2>/dev/null | head -n 1)"
    fi
  fi
}
agent system /usr/local/bin/kelvo-agent --system
if [ -n "${HOME:-}" ]; then agent user "$HOME/.local/bin/kelvo-agent" ""; fi
exit 0
"#;

/// A tiny first step: which probe to run.
const UNAME_SCRIPT: &str = "uname -s";

/// A host, by the OS it runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostProbe {
    Mac(InstallProbe),
    Linux(Box<LinuxProbe>),
}

/// How the probe verifies a sha256.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestTool {
    Sha256sum,
    Shasum,
    Openssl,
}

impl DigestTool {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "sha256sum" => Some(Self::Sha256sum),
            "shasum" => Some(Self::Shasum),
            "openssl" => Some(Self::Openssl),
            _ => None,
        }
    }

    /// A shell fragment that sets `$actual` to the hex digest of `"$tmp"`.
    pub(crate) fn script(self) -> &'static str {
        match self {
            // `<hex>  <path>`
            Self::Sha256sum => r#"actual=$(sha256sum "$tmp"); actual=${actual%% *}"#,
            Self::Shasum => r#"actual=$(shasum -a 256 "$tmp"); actual=${actual%% *}"#,
            // `SHA2-256(<path>)= <hex>` (OpenSSL 3) or `SHA256(<path>)= <hex>`
            Self::Openssl => r#"actual=$(openssl dgst -sha256 "$tmp"); actual=${actual##* }"#,
        }
    }
}

/// `kelvo-agent version --json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentVersion {
    pub version: String,
    #[serde(default)]
    pub proto: u16,
    #[serde(default)]
    pub min_compatible: u16,
}

/// `kelvo-agent status --json`. A running daemon reports its id and history epoch; a
/// stopped one only the id stored in its data directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatus {
    pub running: bool,
    pub host_id: Option<kelvo_schema::HostId>,
    /// The daemon's history database id (`Hello.db_instance_uuid`).
    pub epoch: Option<String>,
    /// The binary's version.
    pub version: String,
    /// The running daemon's version, which lags `version` until it restarts.
    #[serde(default)]
    pub daemon_version: Option<String>,
    #[serde(default)]
    pub layout: String,
    #[serde(default)]
    pub data_dir: String,
    #[serde(default)]
    pub socket: String,
}

impl AgentStatus {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        serde_json::from_str(text.trim()).map_err(|e| {
            RemoteError::Protocol(format!("invalid agent status {:?}: {e}", text.trim())).into()
        })
    }
}

/// An agent binary found in one layout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingAgent {
    pub binary: String,
    /// `None` when it is not executable or did not answer.
    pub version: Option<AgentVersion>,
    pub status: Option<AgentStatus>,
}

/// A Linux host, as the probe found it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinuxProbe {
    pub arch: String,
    pub uid: u32,
    pub user: String,
    pub home: Option<String>,
    pub xdg_runtime_dir: Option<String>,
    pub xdg_state_home: Option<String>,
    pub xdg_config_home: Option<String>,
    /// `systemctl --version`'s number; `None` without systemctl.
    pub systemd_version: Option<u32>,
    /// `systemctl is-system-running`: `running`, `degraded`, `offline`...
    pub system_state: Option<String>,
    /// `systemctl --user is-system-running`, for a non-root login.
    pub user_state: Option<String>,
    pub linger: Option<bool>,
    /// `getenforce`: `Enforcing`, `Permissive`, `Disabled`; `None` without SELinux tools.
    pub selinux: Option<String>,
    pub restorecon: bool,
    pub digest: Option<DigestTool>,
    /// The nearest existing directory to the data directory, and its volume's space.
    pub data_parent: Option<String>,
    pub disk_free_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    /// `stat -fc %T /sys/fs/cgroup`: `cgroup2fs` on a unified hierarchy.
    pub cgroup_fs: Option<String>,
    pub docker_socket: bool,
    pub page_size: Option<u64>,
    pub system_agent: Option<ExistingAgent>,
    pub user_agent: Option<ExistingAgent>,
}

impl LinuxProbe {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let mut kv = BTreeMap::new();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                kv.insert(key, value.trim());
            }
        }
        let text = |key: &str| {
            kv.get(key)
                .filter(|v| !v.is_empty())
                .map(|v| (*v).to_string())
        };
        let number = |key: &str| kv.get(key).and_then(|v| v.parse::<u64>().ok());
        let uid = kv
            .get("uid")
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| RemoteError::Protocol("the probe printed no uid".into()))?;
        let agent = |layout: &str| {
            text(&format!("{layout}_bin")).map(|binary| ExistingAgent {
                binary,
                version: text(&format!("{layout}_version"))
                    .and_then(|v| serde_json::from_str(&v).ok()),
                status: text(&format!("{layout}_status")).and_then(|v| AgentStatus::parse(&v).ok()),
            })
        };
        Ok(Self {
            arch: text("arch").unwrap_or_default(),
            uid,
            user: text("user").unwrap_or_default(),
            home: text("home"),
            xdg_runtime_dir: text("xdg_runtime_dir"),
            xdg_state_home: text("xdg_state_home"),
            xdg_config_home: text("xdg_config_home"),
            systemd_version: kv.get("systemd").and_then(|v| v.parse().ok()),
            system_state: text("system_state"),
            user_state: text("user_state"),
            linger: text("linger").map(|v| v == "yes"),
            selinux: text("selinux"),
            restorecon: kv.get("restorecon") == Some(&"yes"),
            digest: kv.get("sha256").and_then(|v| DigestTool::parse(v)),
            data_parent: text("data_parent"),
            disk_free_bytes: number("disk_free_kb").map(|kb| kb.saturating_mul(1024)),
            disk_total_bytes: number("disk_total_kb").map(|kb| kb.saturating_mul(1024)),
            cgroup_fs: text("cgroup_fs"),
            docker_socket: kv.get("docker") == Some(&"yes"),
            page_size: number("page_size"),
            system_agent: agent("system"),
            user_agent: agent("user"),
        })
    }

    /// Root installs the system layout; anyone else the user layout.
    pub fn layout(&self) -> AgentLayout {
        AgentLayout::for_uid(self.uid)
    }

    /// The login environment the paths derive from, by the same rules as the agent.
    pub fn env(&self) -> AgentEnv {
        let os = |v: &Option<String>| v.as_ref().map(OsString::from);
        AgentEnv::from_vars(
            os(&self.home),
            os(&self.xdg_state_home),
            os(&self.xdg_runtime_dir),
            os(&self.xdg_config_home),
        )
    }

    /// The agent already in the selected layout, if any.
    pub fn existing(&self) -> Option<&ExistingAgent> {
        match self.layout() {
            AgentLayout::System => self.system_agent.as_ref(),
            AgentLayout::User => self.user_agent.as_ref(),
        }
    }

    /// Where this login installs the agent.
    pub fn target(&self) -> Result<AgentTarget> {
        let layout = self.layout();
        Ok(AgentTarget {
            layout,
            paths: layout.paths(&self.env())?,
            digest: self.digest.ok_or(AgentInstallError::NoDigestTool)?,
        })
    }

    /// Whether the agent can be installed here with a history budget of `budget_mb`
    /// (decimal megabytes, as `HistorySettings::size_limit_mb`). The free-space floor is
    /// the store's low-disk pause threshold plus the budget, checked only for a fresh
    /// install: an existing agent's history already occupies its share.
    pub fn validate(&self, budget_mb: u32) -> Result<()> {
        if !matches!(self.arch.as_str(), "aarch64" | "x86_64") {
            return Err(AgentInstallError::UnsupportedArch(self.arch.clone()));
        }
        self.validate_systemd()?;
        if self.digest.is_none() {
            return Err(AgentInstallError::NoDigestTool);
        }
        let (other, other_layout) = match self.layout() {
            AgentLayout::System => (&self.user_agent, AgentLayout::User),
            AgentLayout::User => (&self.system_agent, AgentLayout::System),
        };
        if let Some(other) = other {
            return Err(AgentInstallError::OtherLayout {
                existing: other_layout,
                binary: other.binary.clone(),
            });
        }
        if self.existing().is_none()
            && let (Some(free), Some(total)) = (self.disk_free_bytes, self.disk_total_bytes)
        {
            let needed = kelvo_store::low_disk_threshold(total)
                .saturating_add(u64::from(budget_mb).saturating_mul(1_000_000));
            if free < needed {
                return Err(AgentInstallError::LowDisk {
                    path: self.data_parent.clone().unwrap_or_default(),
                    free_bytes: free,
                    needed_bytes: needed,
                });
            }
        }
        self.target().map(drop)
    }

    fn validate_systemd(&self) -> Result<()> {
        let usable = |state: &Option<String>| {
            state
                .as_deref()
                .is_some_and(|s| !matches!(s, "offline" | "unknown"))
        };
        if self.systemd_version.is_none() {
            return Err(AgentInstallError::NoSystemd {
                detail: "no systemctl".into(),
            });
        }
        if !usable(&self.system_state) {
            return Err(AgentInstallError::NoSystemd {
                detail: format!(
                    "systemd is {}",
                    self.system_state.as_deref().unwrap_or("not running")
                ),
            });
        }
        if self.layout() == AgentLayout::User && !usable(&self.user_state) {
            return Err(AgentInstallError::NoUserManager {
                user: self.user.clone(),
                detail: format!(
                    "the user manager is {}",
                    self.user_state.as_deref().unwrap_or("not reachable")
                ),
            });
        }
        Ok(())
    }

    /// What setup should tell the user before installing. Not blocking.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.layout() == AgentLayout::User && self.linger != Some(true) {
            out.push(format!(
                "{0} is not lingering, so the agent stops when {0} logs out and starts only at their next login. To keep it running, run `sudo loginctl enable-linger {0}`",
                self.user
            ));
        }
        out
    }
}

/// Probes `alias`: a Mac through [`crate::install::probe`], a Linux host through the
/// agent probe, validated for a history budget of `budget_mb`.
pub async fn probe_host(alias: &str, budget_mb: u32) -> Result<HostProbe> {
    let os = run_script(&Ssh(alias), UNAME_SCRIPT, None, STEP_TIMEOUT).await?;
    match os.trim() {
        "Darwin" => Ok(HostProbe::Mac(crate::install::probe(alias).await?)),
        "Linux" => {
            let probe = probe_linux(alias).await?;
            probe.validate(budget_mb)?;
            Ok(HostProbe::Linux(Box::new(probe)))
        }
        other => Err(RemoteError::Incompatible(format!(
            "Kelvo monitors macOS and Linux hosts; this one runs {}",
            if other.is_empty() {
                "an unknown OS"
            } else {
                other
            }
        ))
        .into()),
    }
}

/// The Linux probe alone, unvalidated.
pub async fn probe_linux(alias: &str) -> Result<LinuxProbe> {
    probe_with(&Ssh(alias)).await
}

pub(crate) async fn probe_with(remote: &impl Remote) -> Result<LinuxProbe> {
    LinuxProbe::parse(&run_script(remote, PROBE_SCRIPT, None, STEP_TIMEOUT).await?)
}
