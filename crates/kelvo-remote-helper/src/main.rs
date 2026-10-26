#![cfg_attr(test, allow(clippy::too_many_lines))]
//! Only bridges authenticated local sockets; it never starts a collector or opens SQLite.
use kelvo_remote::{ControlRequest, SocketPaths, control_request, stdio_bridge};

fn main() {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("kelvo-remote-helper: {error}");
            std::process::exit(1);
        }
    };
    let result = runtime.block_on(run());
    // Without waiting on blocking tasks: `serve`'s stdin reader stays blocked in read(2)
    // after the app closes the session, and dropping the runtime would wait for it and
    // keep the bridge (and the SSH session) alive.
    runtime.shutdown_background();
    if let Err(error) = result {
        eprintln!("kelvo-remote-helper: {error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("version") {
        println!(
            "{}",
            serde_json::to_string(&kelvo_remote::control::HelperVersion::current())?
        );
        return Ok(());
    }
    let paths = SocketPaths::for_current_user()?;
    match args.first().map(String::as_str) {
        Some("serve") if args.get(1).map(String::as_str) == Some("--stdio") => {
            stdio_bridge(&paths.metrics).await?;
        }
        Some("status") => {
            println!(
                "{}",
                serde_json::to_string(
                    &control_request(&paths.control, &ControlRequest::Status).await?
                )?
            );
        }
        Some("update-status") => {
            println!(
                "{}",
                serde_json::to_string(
                    &control_request(&paths.control, &ControlRequest::UpdateStatus).await?
                )?
            );
        }
        Some("configure") => {
            let request: ControlRequest =
                serde_json::from_str(args.get(1).ok_or("configure requires a JSON request")?)?;
            if !matches!(
                request,
                ControlRequest::Configure { .. } | ControlRequest::UpdatePolicy { .. }
            ) {
                return Err("configure requires configuration request".into());
            }
            println!(
                "{}",
                serde_json::to_string(&control_request(&paths.control, &request).await?)?
            );
        }
        _ => {
            return Err(
                "usage: kelvo-remote-helper serve --stdio | version --json | status | update-status --json | configure '<json>'".into(),
            );
        }
    }
    Ok(())
}
