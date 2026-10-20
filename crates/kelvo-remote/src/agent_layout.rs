//! Where the headless agent lives on a Linux host (D-115), shared by the agent and the
//! controller that installs it. A root SSH login gets the system layout and a systemd
//! system unit; any other user gets the per-user layout and a user unit.
//!
//! Every path is a pure function of [`AgentEnv`], so both sides compute the same paths
//! from the same environment and tests never read the process environment.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{RemoteError, Result};

/// What `kelvo-agent serve --stdio` prints to stderr when its daemon is not running,
/// before exiting with status 3. The controller recognises it in the SSH stderr and
/// reports [`RemoteError::AgentNotRunning`], with no wire change.
pub const AGENT_NOT_RUNNING: &str = "kelvo-agent: not running";

/// The unit name in both layouts.
pub const UNIT_NAME: &str = "kelvo-agent.service";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentLayout {
    /// Installed by root: `/usr/local/bin`, `/var/lib`, `/run`, a system unit.
    System,
    /// Installed by any other user under their home, with a user unit.
    User,
}

/// The environment the paths depend on. Unset, empty or relative XDG variables are
/// ignored, as the XDG base directory spec says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentEnv {
    pub home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
}

impl AgentEnv {
    /// This process's environment.
    pub fn from_process() -> Self {
        let var = |name| std::env::var_os(name);
        Self::from_vars(
            var("HOME"),
            var("XDG_STATE_HOME"),
            var("XDG_RUNTIME_DIR"),
            var("XDG_CONFIG_HOME"),
        )
    }

    pub(crate) fn from_vars(
        home: Option<OsString>,
        state: Option<OsString>,
        runtime: Option<OsString>,
        config: Option<OsString>,
    ) -> Self {
        let absolute = |v: Option<OsString>| v.map(PathBuf::from).filter(|p| p.is_absolute());
        Self {
            home: absolute(home),
            xdg_state_home: absolute(state),
            xdg_runtime_dir: absolute(runtime),
            xdg_config_home: absolute(config),
        }
    }

    fn home(&self) -> Result<&Path> {
        self.home
            .as_deref()
            .ok_or_else(|| RemoteError::Protocol("HOME unavailable".into()))
    }
}

/// Every path of one layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentPaths {
    pub binary: PathBuf,
    pub data_dir: PathBuf,
    pub socket: PathBuf,
    pub unit: PathBuf,
}

impl AgentLayout {
    /// The layout for an effective uid: root installs the system layout.
    pub fn for_uid(uid: u32) -> Self {
        if uid == 0 { Self::System } else { Self::User }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
        }
    }

    /// The paths of this layout. The user layout needs `HOME`.
    ///
    /// | | System | User |
    /// |---|---|---|
    /// | binary | `/usr/local/bin/kelvo-agent` | `~/.local/bin/kelvo-agent` |
    /// | data | `/var/lib/kelvo-agent` | `${XDG_STATE_HOME:-~/.local/state}/kelvo-agent` |
    /// | socket | `/run/kelvo-agent/metrics.sock` | `$XDG_RUNTIME_DIR/kelvo-agent/metrics.sock`, else in the data dir |
    /// | unit | `/etc/systemd/system/kelvo-agent.service` | `${XDG_CONFIG_HOME:-~/.config}/systemd/user/kelvo-agent.service` |
    pub fn paths(self, env: &AgentEnv) -> Result<AgentPaths> {
        match self {
            Self::System => Ok(AgentPaths {
                binary: PathBuf::from("/usr/local/bin/kelvo-agent"),
                data_dir: PathBuf::from("/var/lib/kelvo-agent"),
                socket: PathBuf::from("/run/kelvo-agent/metrics.sock"),
                unit: Path::new("/etc/systemd/system").join(UNIT_NAME),
            }),
            Self::User => {
                let home = env.home()?;
                let data_dir = env
                    .xdg_state_home
                    .clone()
                    .unwrap_or_else(|| home.join(".local/state"))
                    .join("kelvo-agent");
                let socket = env.xdg_runtime_dir.as_ref().map_or_else(
                    || data_dir.join("metrics.sock"),
                    |run| run.join("kelvo-agent/metrics.sock"),
                );
                let config = env
                    .xdg_config_home
                    .clone()
                    .unwrap_or_else(|| home.join(".config"));
                Ok(AgentPaths {
                    binary: home.join(".local/bin/kelvo-agent"),
                    socket,
                    unit: config.join("systemd/user").join(UNIT_NAME),
                    data_dir,
                })
            }
        }
    }

    /// The unit file text for `paths`.
    ///
    /// The system unit runs as root with systemd-managed state and runtime directories
    /// and a read-only view of the rest of the system. The user unit leaves out
    /// `ProtectSystem`, `ProtectHome`, `PrivateTmp` and the directory settings: in a
    /// user manager they need unprivileged user namespaces, and where those are missing
    /// the unit fails to start; the agent creates its own private directories there.
    ///
    /// The user unit names its data directory and socket explicitly. The user manager's
    /// environment is not the SSH session's (`XDG_*` set in a shell profile never reach
    /// it), so the paths the controller computed from the session are baked in rather
    /// than left for the daemon to derive again.
    pub fn unit_text(self, paths: &AgentPaths) -> String {
        let exec = unit_quote(&paths.binary.to_string_lossy());
        let data = unit_quote(&paths.data_dir.to_string_lossy());
        let socket = unit_quote(&paths.socket.to_string_lossy());
        match self {
            Self::System => format!(
                "[Unit]\n\
                 Description=Kelvo agent\n\
                 \n\
                 [Service]\n\
                 ExecStart={exec} run --system\n\
                 StateDirectory=kelvo-agent\n\
                 RuntimeDirectory=kelvo-agent\n\
                 RuntimeDirectoryMode=0700\n\
                 ProtectSystem=strict\n\
                 ProtectHome=yes\n\
                 PrivateTmp=yes\n\
                 NoNewPrivileges=yes\n\
                 Nice=10\n\
                 IOSchedulingClass=best-effort\n\
                 IOSchedulingPriority=7\n\
                 MemoryMax=96M\n\
                 Restart=on-failure\n\
                 \n\
                 [Install]\n\
                 WantedBy=multi-user.target\n"
            ),
            Self::User => format!(
                "[Unit]\n\
                 Description=Kelvo agent\n\
                 \n\
                 [Service]\n\
                 ExecStart={exec} run --data-dir {data} --socket {socket}\n\
                 NoNewPrivileges=yes\n\
                 Nice=10\n\
                 IOSchedulingClass=best-effort\n\
                 IOSchedulingPriority=7\n\
                 MemoryMax=96M\n\
                 Restart=on-failure\n\
                 \n\
                 [Install]\n\
                 WantedBy=default.target\n"
            ),
        }
    }
}

/// A path as one systemd command-line word: `%` and `\` escaped, quoted when it holds
/// whitespace or a quote.
fn unit_quote(path: &str) -> String {
    let escaped = path
        .replace('\\', "\\\\")
        .replace('%', "%%")
        .replace('"', "\\\"");
    if escaped
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '\'')
    {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: &str, state: &str, runtime: &str, config: &str) -> AgentEnv {
        let v = |s: &str| (!s.is_empty()).then(|| OsString::from(s));
        AgentEnv::from_vars(v(home), v(state), v(runtime), v(config))
    }

    #[test]
    fn system_paths_ignore_the_environment() {
        let paths = AgentLayout::System.paths(&AgentEnv::default()).unwrap();
        assert_eq!(paths.binary, Path::new("/usr/local/bin/kelvo-agent"));
        assert_eq!(paths.data_dir, Path::new("/var/lib/kelvo-agent"));
        assert_eq!(paths.socket, Path::new("/run/kelvo-agent/metrics.sock"));
        assert_eq!(
            paths.unit,
            Path::new("/etc/systemd/system/kelvo-agent.service")
        );
        assert_eq!(
            AgentLayout::System
                .paths(&env("/home/a", "/s", "/r", "/c"))
                .unwrap(),
            paths
        );
    }

    #[test]
    fn user_paths_default_under_home() {
        let paths = AgentLayout::User
            .paths(&env("/home/a", "", "", ""))
            .unwrap();
        assert_eq!(paths.binary, Path::new("/home/a/.local/bin/kelvo-agent"));
        assert_eq!(
            paths.data_dir,
            Path::new("/home/a/.local/state/kelvo-agent")
        );
        // No runtime dir: the socket lives in the data dir.
        assert_eq!(
            paths.socket,
            Path::new("/home/a/.local/state/kelvo-agent/metrics.sock")
        );
        assert_eq!(
            paths.unit,
            Path::new("/home/a/.config/systemd/user/kelvo-agent.service")
        );
    }

    #[test]
    fn user_paths_follow_xdg_variables() {
        let paths = AgentLayout::User
            .paths(&env("/home/a", "/st", "/run/user/1000", "/cfg"))
            .unwrap();
        assert_eq!(paths.binary, Path::new("/home/a/.local/bin/kelvo-agent"));
        assert_eq!(paths.data_dir, Path::new("/st/kelvo-agent"));
        assert_eq!(
            paths.socket,
            Path::new("/run/user/1000/kelvo-agent/metrics.sock")
        );
        assert_eq!(
            paths.unit,
            Path::new("/cfg/systemd/user/kelvo-agent.service")
        );
    }

    #[test]
    fn relative_xdg_variables_are_ignored_and_home_is_required() {
        let paths = AgentLayout::User
            .paths(&env("/home/a", "rel/state", "run", "cfg"))
            .unwrap();
        assert_eq!(
            paths,
            AgentLayout::User
                .paths(&env("/home/a", "", "", ""))
                .unwrap()
        );
        assert!(
            AgentLayout::User
                .paths(&env("", "/st", "/r", "/c"))
                .is_err()
        );
        assert!(
            AgentLayout::User
                .paths(&env("relative", "", "", ""))
                .is_err()
        );
    }

    #[test]
    fn root_gets_the_system_layout() {
        assert_eq!(AgentLayout::for_uid(0), AgentLayout::System);
        assert_eq!(AgentLayout::for_uid(1000), AgentLayout::User);
    }

    #[test]
    fn system_unit_matches_the_plan() {
        let paths = AgentLayout::System.paths(&AgentEnv::default()).unwrap();
        let text = AgentLayout::System.unit_text(&paths);
        assert_eq!(
            text,
            "[Unit]\nDescription=Kelvo agent\n\n[Service]\n\
             ExecStart=/usr/local/bin/kelvo-agent run --system\n\
             StateDirectory=kelvo-agent\nRuntimeDirectory=kelvo-agent\nRuntimeDirectoryMode=0700\n\
             ProtectSystem=strict\nProtectHome=yes\nPrivateTmp=yes\nNoNewPrivileges=yes\n\
             Nice=10\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\n\
             MemoryMax=96M\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n"
        );
    }

    #[test]
    fn user_unit_drops_root_only_hardening() {
        let paths = AgentLayout::User
            .paths(&env("/home/a", "", "/run/user/1000", ""))
            .unwrap();
        let text = AgentLayout::User.unit_text(&paths);
        // The session's paths are baked in: the user manager may not share its XDG_*.
        assert!(text.contains(
            "ExecStart=/home/a/.local/bin/kelvo-agent run \
             --data-dir /home/a/.local/state/kelvo-agent \
             --socket /run/user/1000/kelvo-agent/metrics.sock\n"
        ));
        let spaced = AgentLayout::User
            .paths(&env("/home/my user", "", "", ""))
            .unwrap();
        assert!(AgentLayout::User.unit_text(&spaced).contains(
            "ExecStart=\"/home/my user/.local/bin/kelvo-agent\" run \
             --data-dir \"/home/my user/.local/state/kelvo-agent\" \
             --socket \"/home/my user/.local/state/kelvo-agent/metrics.sock\"\n"
        ));
        assert!(text.contains("WantedBy=default.target"));
        assert!(text.contains("NoNewPrivileges=yes") && text.contains("MemoryMax=96M"));
        for root_only in [
            "ProtectSystem",
            "ProtectHome",
            "PrivateTmp",
            "StateDirectory",
            "RuntimeDirectory",
            "--system",
        ] {
            assert!(!text.contains(root_only), "{root_only} in the user unit");
        }
    }

    #[test]
    fn unit_paths_are_escaped_for_systemd() {
        assert_eq!(unit_quote("/a/b"), "/a/b");
        assert_eq!(unit_quote("/home/my user/x"), "\"/home/my user/x\"");
        assert_eq!(unit_quote("/a%b\\c"), "/a%%b\\\\c");
        assert_eq!(unit_quote("/a\"b"), "\"/a\\\"b\"");
    }
}
