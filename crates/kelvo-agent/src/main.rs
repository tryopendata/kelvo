#![cfg_attr(test, allow(clippy::too_many_lines))]
//! `kelvo-agent`: the headless collector for hosts without the app (D-115). `run` owns
//! the engine and this host's history and serves them on a private socket; `serve
//! --stdio` bridges an SSH session to it. The other commands are the one-shot steps
//! the controller runs while installing, checking, updating and removing it.
use std::process::ExitCode;

use anyhow::Context;
use kelvo_remote::AgentEnv;

mod cli;
mod commands;
mod container_names;
mod daemon;
mod host;
mod settings;

use cli::Command;

fn main() -> ExitCode {
    init_logging();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&args).and_then(run) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("kelvo-agent: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Logs go to stderr, which journald captures under the unit. `RUST_LOG` filters them.
fn init_logging() {
    use std::io::IsTerminal;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .try_init();
}

fn run(command: Command) -> anyhow::Result<ExitCode> {
    // Two workers: `set_interest` can block one for up to 200 ms while the engine
    // switches to its visible tick, which must not stall every other session.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    let result = runtime.block_on(dispatch(command));
    // Without waiting on blocking tasks: `serve`'s stdin reader stays blocked in read(2)
    // after the daemon closes the session, and waiting for it would keep the bridge
    // alive. Everything that must finish (the engine stop) was awaited above.
    runtime.shutdown_background();
    result
}

async fn dispatch(command: Command) -> anyhow::Result<ExitCode> {
    let env = AgentEnv::from_process();
    match command {
        Command::Help => println!("{}", cli::USAGE),
        Command::Version => println!("{}", commands::version()),
        Command::Run(at) => daemon::run(&at.resolve(&env)?, at.system).await?,
        Command::Serve(at) => return daemon::serve(&at.resolve(&env)?.socket).await,
        Command::Status(at) => {
            println!(
                "{}",
                commands::status(at.layout(), &at.resolve(&env)?).await
            );
        }
        Command::Configure { at, json } => {
            let paths = at.resolve(&env)?;
            let applied = settings::configure(&paths.data_dir, &json)?;
            println!(
                "{}",
                serde_json::json!({
                    "written": paths.data_dir.join(settings::SETTINGS_FILE),
                    "applies": "on restart",
                    "history": applied.history,
                })
            );
        }
        Command::InstallUnit(at) => {
            println!(
                "{}",
                commands::install_unit(at.layout(), &at.resolve(&env)?)?
            );
        }
        Command::Uninstall { at, purge } => {
            println!("{}", commands::uninstall(&at.resolve(&env)?, purge));
        }
        Command::ResetIdentity(at) => {
            println!(
                "{}",
                commands::reset_identity(&at.resolve(&env)?, at.system)?
            );
        }
        Command::Sample { seconds } => println!("{}", commands::sample(seconds).await?),
    }
    Ok(ExitCode::SUCCESS)
}
