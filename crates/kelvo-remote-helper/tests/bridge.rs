use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    process::Command,
};

#[tokio::test]
async fn helper_relays_binary_frames_in_both_directions_and_exits_on_eof() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let paths = kelvo_remote::SocketPaths::new(
        dir.path()
            .join("Library/Application Support/com.tryopendata.kelvo"),
    );
    std::fs::create_dir_all(paths.metrics.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&paths.metrics).unwrap();
    let mut helper = Command::new(env!("CARGO_BIN_EXE_kelvo-remote-helper"))
        .args(["serve", "--stdio"])
        .env("HOME", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut input = helper.stdin.take().unwrap();
    let mut output = helper.stdout.take().unwrap();
    // No newline: CBOR frames must flush immediately rather than waiting for text IO.
    let from_app = [0, 0, 0, 5, 0xA1, 0x61, 0x74, 0x61, 0x78];
    socket.write_all(&from_app).await.unwrap();
    let mut response = [0; 9];
    tokio::time::timeout(Duration::from_secs(3), output.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response, from_app);
    let from_controller = [0, 0, 0, 3, 0x01, 0x02, 0x03];
    input.write_all(&from_controller).await.unwrap();
    let mut received = [0; 7];
    tokio::time::timeout(Duration::from_secs(3), socket.read_exact(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, from_controller);
    input.shutdown().await.unwrap();
    drop(input);
    assert!(
        tokio::time::timeout(Duration::from_secs(3), helper.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(
        !dir.path()
            .join("Library/Application Support/com.tryopendata.kelvo/history.sqlite")
            .exists()
    );
}
