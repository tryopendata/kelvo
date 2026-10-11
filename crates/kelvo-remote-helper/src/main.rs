//! Only bridges authenticated local sockets; it never starts a collector or opens SQLite.
use kelvo_remote::{ControlRequest, SocketPaths, control_request};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
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
    let paths = SocketPaths::for_current_user()?;
    match args.first().map(String::as_str) {
        Some("serve") if args.get(1).map(String::as_str) == Some("--stdio") => {
            let stream = tokio::net::UnixStream::connect(&paths.metrics).await?;
            let (mut read, mut write) = stream.into_split();
            let mut input = tokio::io::stdin();
            let mut output = tokio::io::stdout();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            tokio::select! {
                result=async {tokio::io::copy(&mut input,&mut write).await?;write.shutdown().await}=>{result?;},
                result=async {let mut buffer=[0u8;16*1024];loop{let count=read.read(&mut buffer).await?;if count==0 {break;}output.write_all(&buffer[..count]).await?;output.flush().await?;}Ok::<(),std::io::Error>(())}=>{result?;}
            }
        }
        Some("status") => {
            println!(
                "{}",
                serde_json::to_string(
                    &control_request(&paths.control, &ControlRequest::Status).await?
                )?
            );
        }
        Some("configure") => {
            let request: ControlRequest =
                serde_json::from_str(args.get(1).ok_or("configure requires a JSON request")?)?;
            if !matches!(request, ControlRequest::Configure { .. }) {
                return Err("configure requires configuration request".into());
            }
            println!(
                "{}",
                serde_json::to_string(&control_request(&paths.control, &request).await?)?
            );
        }
        _ => {
            return Err(
                "usage: kelvo-remote-helper serve --stdio | status | configure '<json>'".into(),
            );
        }
    }
    Ok(())
}
