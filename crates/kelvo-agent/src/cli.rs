//! The command line. Small enough for a hand parser; the workspace has no CLI crate.
use std::path::PathBuf;

use anyhow::{Context, bail};
use kelvo_remote::{AgentEnv, AgentLayout, AgentPaths};

pub const USAGE: &str = "\
usage: kelvo-agent <command> [options]

commands:
  run [--system] [--data-dir P] [--socket P]   sample this host and serve it on the socket
  serve --stdio [--system] [--socket P]        bridge stdin/stdout to the running daemon
  status [--json] [--system] [--data-dir P] [--socket P]
  version [--json]
  configure '<json>' [--system] [--data-dir P] merge settings into settings.json (applies on restart)
  install-unit [--system] [--data-dir P] [--socket P]
                                               write the systemd unit (does not start it); the
                                               user unit runs with these paths
  uninstall [--purge] [--system] [--data-dir P]
  reset-identity [--system] [--data-dir P]     give this host a new id
  sample [--json]                              sample for a few seconds and print every series

--system selects the root layout (/var/lib/kelvo-agent, /run/kelvo-agent); without it
the per-user layout under $HOME is used.";

/// Which layout, and the paths a command overrides.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Location {
    pub system: bool,
    pub data_dir: Option<PathBuf>,
    pub socket: Option<PathBuf>,
}

impl Location {
    pub fn layout(&self) -> AgentLayout {
        if self.system {
            AgentLayout::System
        } else {
            AgentLayout::User
        }
    }

    /// The layout's paths with the overrides applied.
    pub fn resolve(&self, env: &AgentEnv) -> anyhow::Result<AgentPaths> {
        let mut paths = self
            .layout()
            .paths(env)
            .context("resolving the agent's paths")?;
        if let Some(dir) = &self.data_dir {
            paths.data_dir.clone_from(dir);
        }
        if let Some(socket) = &self.socket {
            paths.socket.clone_from(socket);
        }
        Ok(paths)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Run(Location),
    Serve(Location),
    Status(Location),
    Version,
    Configure { at: Location, json: String },
    InstallUnit(Location),
    Uninstall { at: Location, purge: bool },
    ResetIdentity(Location),
    Sample,
    Help,
}

/// The flags one command accepts.
#[derive(Clone, Copy)]
struct Accepts {
    system: bool,
    data_dir: bool,
    socket: bool,
    json: bool,
}

const fn accepts(system: bool, data_dir: bool, socket: bool, json: bool) -> Accepts {
    Accepts {
        system,
        data_dir,
        socket,
        json,
    }
}

#[derive(Default)]
struct Parsed {
    at: Location,
    stdio: bool,
    purge: bool,
    positional: Vec<String>,
}

pub fn parse(args: &[String]) -> anyhow::Result<Command> {
    let Some((name, rest)) = args.split_first() else {
        return Ok(Command::Help);
    };
    let flags = match name.as_str() {
        "run" => accepts(true, true, true, false),
        "serve" => accepts(true, false, true, false),
        "status" => accepts(true, true, true, true),
        "version" | "--version" => accepts(false, false, false, true),
        "configure" | "uninstall" | "reset-identity" => accepts(true, true, false, false),
        "install-unit" => accepts(true, true, true, false),
        "sample" => accepts(false, false, false, true),
        "help" | "--help" | "-h" => return Ok(Command::Help),
        other => bail!("unknown command {other:?}\n\n{USAGE}"),
    };
    let p = parse_flags(name, rest, flags)?;
    let positional = |want: usize| -> anyhow::Result<()> {
        if p.positional.len() == want {
            Ok(())
        } else {
            bail!("{name}: unexpected arguments {:?}", p.positional)
        }
    };
    Ok(match name.as_str() {
        "run" => {
            positional(0)?;
            Command::Run(p.at)
        }
        "serve" => {
            positional(0)?;
            if !p.stdio {
                bail!("serve: only --stdio is supported");
            }
            Command::Serve(p.at)
        }
        "status" => {
            positional(0)?;
            Command::Status(p.at)
        }
        "configure" => {
            positional(1)?;
            let json = p.positional.into_iter().next().unwrap_or_default();
            Command::Configure { at: p.at, json }
        }
        "install-unit" => {
            positional(0)?;
            Command::InstallUnit(p.at)
        }
        "uninstall" => {
            positional(0)?;
            Command::Uninstall {
                at: p.at,
                purge: p.purge,
            }
        }
        "reset-identity" => {
            positional(0)?;
            Command::ResetIdentity(p.at)
        }
        "sample" => {
            positional(0)?;
            Command::Sample
        }
        _ => {
            positional(0)?;
            Command::Version
        }
    })
}

fn parse_flags(name: &str, rest: &[String], flags: Accepts) -> anyhow::Result<Parsed> {
    let mut p = Parsed::default();
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| -> anyhow::Result<PathBuf> {
            match args.next() {
                Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
                _ => bail!("{name}: {flag} needs a path"),
            }
        };
        match arg.as_str() {
            "--system" if flags.system => p.at.system = true,
            "--data-dir" if flags.data_dir => p.at.data_dir = Some(value("--data-dir")?),
            "--socket" if flags.socket => p.at.socket = Some(value("--socket")?),
            "--json" if flags.json => {}
            "--stdio" if name == "serve" => p.stdio = true,
            "--purge" if name == "uninstall" => p.purge = true,
            flag if flag.starts_with("--") => bail!("{name}: unknown option {flag}"),
            _ => p.positional.push(arg.clone()),
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse_str(line: &str) -> anyhow::Result<Command> {
        let args: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        parse(&args)
    }

    #[test]
    fn run_takes_layout_and_path_overrides() {
        assert_eq!(parse_str("run").unwrap(), Command::Run(Location::default()));
        assert_eq!(
            parse_str("run --system --data-dir /d --socket /s.sock").unwrap(),
            Command::Run(Location {
                system: true,
                data_dir: Some("/d".into()),
                socket: Some("/s.sock".into()),
            })
        );
        assert!(parse_str("run --data-dir").is_err());
        assert!(parse_str("run --json").is_err());
        assert!(parse_str("run extra").is_err());
    }

    #[test]
    fn serve_requires_stdio() {
        assert_eq!(
            parse_str("serve --stdio --socket /s").unwrap(),
            Command::Serve(Location {
                socket: Some("/s".into()),
                ..Location::default()
            })
        );
        assert!(parse_str("serve").is_err());
        assert!(parse_str("serve --stdio --data-dir /d").is_err());
    }

    #[test]
    fn configure_takes_one_json_argument() {
        let args = vec![
            "configure".to_string(),
            r#"{"history": {"retention_days": 7}}"#.to_string(),
            "--system".to_string(),
        ];
        assert_eq!(
            parse(&args).unwrap(),
            Command::Configure {
                at: Location {
                    system: true,
                    ..Location::default()
                },
                json: r#"{"history": {"retention_days": 7}}"#.into(),
            }
        );
        assert!(parse_str("configure").is_err());
        assert!(parse_str("configure {} {}").is_err());
    }

    #[test]
    fn simple_commands_and_flags() {
        assert_eq!(parse_str("version --json").unwrap(), Command::Version);
        assert_eq!(parse_str("--version").unwrap(), Command::Version);
        assert_eq!(parse_str("sample --json").unwrap(), Command::Sample);
        assert_eq!(parse_str("").unwrap(), Command::Help);
        assert_eq!(
            parse_str("uninstall --purge").unwrap(),
            Command::Uninstall {
                at: Location::default(),
                purge: true
            }
        );
        assert_eq!(
            parse_str("install-unit --system").unwrap(),
            Command::InstallUnit(Location {
                system: true,
                ..Location::default()
            })
        );
        assert!(parse_str("install-unit --purge").is_err());
        assert_eq!(
            parse_str("install-unit --data-dir /d --socket /s").unwrap(),
            Command::InstallUnit(Location {
                system: false,
                data_dir: Some("/d".into()),
                socket: Some("/s".into()),
            })
        );
        assert!(parse_str("frobnicate").is_err());
    }

    #[test]
    fn overrides_replace_layout_paths() {
        let env = AgentEnv {
            home: Some("/home/a".into()),
            ..AgentEnv::default()
        };
        let at = Location {
            system: false,
            data_dir: Some("/tmp/d".into()),
            socket: None,
        };
        let paths = at.resolve(&env).unwrap();
        assert_eq!(paths.data_dir, Path::new("/tmp/d"));
        // The socket keeps the layout default; only an explicit --socket moves it.
        assert_eq!(
            paths.socket,
            Path::new("/home/a/.local/state/kelvo-agent/metrics.sock")
        );
        assert_eq!(
            Location {
                system: true,
                ..Location::default()
            }
            .resolve(&AgentEnv::default())
            .unwrap()
            .data_dir,
            Path::new("/var/lib/kelvo-agent")
        );
    }
}
