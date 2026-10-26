use super::*;

const MANIFEST: &[u8] = include_bytes!("../tests/fixtures/release.json");
const SIGNATURE: &str = include_str!("../tests/fixtures/release.json.sig");
const TEST_KEY: &str = include_str!("../tests/fixtures/public-key.txt");
const PROTOCOL: ProtocolRange = ProtocolRange { min: 1, max: 1 };

fn fixture() -> VerifiedRelease {
    verify_with_key(MANIFEST, SIGNATURE, TEST_KEY).unwrap()
}

#[test]
fn signature_authenticates_version_protocol_and_artifacts() {
    assert_eq!(fixture().release().version, "9.0.0");
    for (old, new) in [
        ("9.0.0", "9.0.1"),
        ("\"min\": 1", "\"min\": 2"),
        ("\"size\": 4", "\"size\": 5"),
    ] {
        let altered = std::str::from_utf8(MANIFEST).unwrap().replace(old, new);
        assert_ne!(altered.as_bytes(), MANIFEST);
        assert!(matches!(
            verify_with_key(altered.as_bytes(), SIGNATURE, TEST_KEY),
            Err(Error::Signature(_))
        ));
    }
    assert!(
        verify_manifest(MANIFEST, SIGNATURE).is_err(),
        "test signatures must not be trusted by production"
    );
    let cached = fixture();
    assert!(verify_with_key(cached.manifest_bytes(), cached.signature(), TEST_KEY).is_ok());
}

#[test]
fn selects_only_newer_releases_preserving_the_entire_protocol_range() {
    let release = fixture();
    assert!(
        release
            .candidate("8.0.0", PROTOCOL, "linux-aarch64")
            .unwrap()
            .is_some()
    );
    for current in ["9.0.0", "10.0.0"] {
        assert!(
            release
                .candidate(current, PROTOCOL, "linux-aarch64")
                .unwrap()
                .is_none()
        );
    }
    assert!(matches!(
        release.candidate("8.0.0", PROTOCOL, "linux-x86_64"),
        Err(Error::MissingTarget(_))
    ));
    for installed in [
        ProtocolRange { min: 1, max: 3 },
        ProtocolRange { min: 0, max: 1 },
        ProtocolRange { min: 3, max: 2 },
    ] {
        assert!(matches!(
            release.candidate("8.0.0", installed, "linux-aarch64"),
            Err(Error::Incompatible { .. })
        ));
    }
    assert!(!ProtocolRange { min: 2, max: 3 }.contains(ProtocolRange { min: 1, max: 2 }));
}

#[test]
fn validates_release_contract_before_any_download() {
    let good = fixture().release().clone();
    for version in ["9.0.1-beta", "9.0.0+build", "v9.0.0", "9.0.0/../bad"] {
        let mut release = good.clone();
        release.version = version.into();
        assert!(validate(&release).is_err());
    }
    for url in [
        "http://github.com/tryopendata/kelvo/releases/download/v9.0.0/a",
        "https://evil.invalid/a",
        "https://github.com/tryopendata/kelvo/releases/download/v8.0.0/a",
    ] {
        let mut release = good.clone();
        release.artifacts.get_mut("linux-aarch64").unwrap().url = url.into();
        assert!(validate(&release).is_err());
    }
    let mut release = good.clone();
    release.schema_version = 2;
    assert!(validate(&release).is_err());
    release = good.clone();
    release.artifacts.get_mut("linux-aarch64").unwrap().size = MAX_ARTIFACT_BYTES + 1;
    assert!(validate(&release).is_err());
}

#[test]
fn verifies_staged_bytes_and_rejects_tampering_or_truncation() {
    let release = fixture();
    let artifact = release.release().artifacts.get("linux-aarch64").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent");
    std::fs::write(&path, b"test").unwrap();
    verify_artifact(artifact, &path).unwrap();
    for bytes in [b"evil".as_slice(), b"tes", b"test-more"] {
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(
            verify_artifact(artifact, &path),
            Err(Error::Integrity)
        ));
    }
}

async fn server(response: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/agent", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await.unwrap();
        stream.write_all(&response).await.unwrap();
    });
    (url, task)
}

#[tokio::test]
async fn real_download_is_bounded_verified_and_never_overwrites_an_existing_stage() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("download");
    for body in ["test", "evil", "tes", "test-more"] {
        let response = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}").into_bytes();
        let (url, task) = server(response).await;
        let mut artifact = fixture()
            .release()
            .artifacts
            .get("linux-aarch64")
            .unwrap()
            .clone();
        artifact.url = url.clone();
        let client = Client::local(url);
        let result = client.download(&artifact, &destination).await;
        task.await.unwrap();
        if body == "test" {
            result.unwrap();
            verify_artifact(&artifact, &destination).unwrap();
            assert!(client.download(&artifact, &destination).await.is_err());
            assert_eq!(std::fs::read(&destination).unwrap(), b"test");
            std::fs::remove_file(&destination).unwrap();
        } else {
            assert!(result.is_err());
            assert!(!destination.exists());
        }
    }
}

#[test]
fn compiled_trust_anchor_matches_the_mac_updater() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../../src-tauri/tauri.conf.json")).unwrap();
    assert_eq!(
        PUBLIC_KEY.trim(),
        config["plugins"]["updater"]["pubkey"].as_str().unwrap()
    );
}
