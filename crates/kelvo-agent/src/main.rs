#![cfg_attr(test, allow(clippy::too_many_lines))]
//! `kelvo-agent`: the headless collector for hosts without the app (D-116). `run` owns
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
mod maintenance;
mod settings;
mod update;

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
        Command::Run(at) => {
            let paths = at.resolve(&env)?;
            update::transaction::public_starting(&paths)?;
            daemon::run(&paths, at.system).await?;
        }
        Command::RunTrial(at) => {
            let paths = at.resolve(&env)?;
            anyhow::ensure!(
                paths.socket == update::transaction::directory(&paths).join("trial.sock"),
                "trial requires its private socket"
            );
            anyhow::ensure!(
                update::transaction::read(&paths)?
                    .is_some_and(|j| j.phase == update::transaction::Phase::Prepared),
                "trial requires a prepared transaction"
            );
            daemon::run(&paths, at.system).await?;
        }
        Command::UpdateStatus(at) => println!(
            "{}",
            serde_json::to_string(&update::status(&at.resolve(&env)?)?)?
        ),
        Command::Update { at, scheduled } => {
            let paths = at.resolve(&env)?;
            if scheduled {
                update::scheduled(at.layout(), &paths).await?;
            } else {
                update::request(at.layout(), &paths).await?;
            }
        }
        Command::Recover { at, restart } => {
            update::recover(at.layout(), &at.resolve(&env)?, restart).await?
        }
        Command::Serve(at) => return daemon::serve(&at.resolve(&env)?.socket).await,
        Command::Status(at) => {
            println!(
                "{}",
                commands::status(at.layout(), &at.resolve(&env)?).await
            );
        }
        Command::Configure { at, json } => {
            let paths = at.resolve(&env)?;
            let _lock = maintenance::lock(&paths)?;
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
        Command::UnitTemplates(at) => {
            let paths = at.resolve(&env)?;
            let templates: Vec<_> =
                std::iter::once((paths.unit.clone(), at.layout().unit_text(&paths)))
                    .chain(at.layout().update_units(&paths))
                    .collect();
            println!("{}", serde_json::to_string(&templates)?);
        }
        Command::InstallUnit(at) => {
            let paths = at.resolve(&env)?;
            let _lock = maintenance::lock(&paths)?;
            println!("{}", commands::install_unit(at.layout(), &paths)?);
        }
        Command::Uninstall { at, purge } => {
            let paths = at.resolve(&env)?;
            update::disable(at.layout()).await?;
            let _lock = maintenance::lock(&paths)?;
            maintenance::systemctl(at.layout(), &["disable", "--now", "kelvo-agent.service"])
                .await?;
            println!(
                "{}",
                serde_json::to_string(&commands::uninstall(&paths, purge))?
            );
        }
        Command::ResetIdentity(at) => {
            let paths = at.resolve(&env)?;
            let lock = maintenance::lock(&paths)?;
            maintenance::systemctl(at.layout(), &["stop", "kelvo-agent.service"]).await?;
            let reset = commands::reset_identity(&paths, at.system);
            drop(lock);
            maintenance::systemctl(at.layout(), &["start", "kelvo-agent.service"]).await?;
            println!("{}", reset?);
        }
        Command::Sample { seconds } => println!("{}", commands::sample(seconds).await?),
    }
    Ok(ExitCode::SUCCESS)
}
