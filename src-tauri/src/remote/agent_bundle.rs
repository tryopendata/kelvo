//! The Linux agent binaries bundled as app resources (`agent/kelvo-agent-<triple>`,
//! from `make agent-linux`). Before one is uploaded to a host it is read and re-hashed
//! against the digest `build.rs` compiled in, so a resource that was swapped or damaged
//! after the build never reaches a host.
#![expect(
    dead_code,
    reason = "Milestone D's Linux install and update flows are the first callers"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use tauri::{AppHandle, Manager as _};

mod digests {
    include!(concat!(env!("OUT_DIR"), "/agent_digests.rs"));
}

#[derive(Debug, thiserror::Error)]
pub enum AgentBundleError {
    #[error("Kelvo has no agent for {machine} hosts (supported: aarch64, x86_64)")]
    UnsupportedArch { machine: String },
    #[error(
        "This build has no Linux agent for {triple}. Run `make agent-linux`, then rebuild the app"
    )]
    NotBuilt { triple: String },
    #[error("The bundled agent at {} could not be read: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "The bundled agent at {} does not match this build (expected sha256 {expected}, found {actual}). Reinstall Kelvo",
        path.display()
    )]
    DigestMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
}

/// The agent target for a host's `uname -m`.
pub fn agent_triple(machine: &str) -> Result<&'static str, AgentBundleError> {
    match machine.trim() {
        "aarch64" | "arm64" => Ok("aarch64-unknown-linux-musl"),
        "x86_64" | "amd64" => Ok("x86_64-unknown-linux-musl"),
        other => Err(AgentBundleError::UnsupportedArch {
            machine: other.to_owned(),
        }),
    }
}

/// The bundled agent for `triple`: its path and verified bytes.
pub fn bundled_agent(
    app: &AppHandle,
    triple: &str,
) -> Result<(PathBuf, Vec<u8>), AgentBundleError> {
    let mut dirs = Vec::with_capacity(2);
    if let Ok(resources) = app.path().resource_dir() {
        dirs.push(resources.join("agent"));
    }
    // `tauri dev` and debug builds without the remote overlay bundle no resources; use
    // the binaries `make agent-linux` wrote, the same files build.rs hashed.
    if cfg!(debug_assertions) {
        dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/agent"));
    }
    load(&dirs, triple, digests::AGENT_SHA256)
}

/// Reads `kelvo-agent-<triple>` from the first directory that has it and checks it
/// against `table`.
fn load(
    dirs: &[PathBuf],
    triple: &str,
    table: &[(&str, &str)],
) -> Result<(PathBuf, Vec<u8>), AgentBundleError> {
    let expected = table
        .iter()
        .find(|(t, _)| *t == triple)
        .map(|(_, hex)| *hex)
        .ok_or_else(|| AgentBundleError::NotBuilt {
            triple: triple.to_owned(),
        })?;
    let name = format!("kelvo-agent-{triple}");
    let path = dirs
        .iter()
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
        .or_else(|| dirs.first().map(|d| d.join(&name)))
        .ok_or_else(|| AgentBundleError::NotBuilt {
            triple: triple.to_owned(),
        })?;
    let bytes = std::fs::read(&path).map_err(|source| AgentBundleError::Read {
        path: path.clone(),
        source,
    })?;
    let actual = hex_sha256(&bytes);
    if actual != expected {
        return Err(AgentBundleError::DigestMismatch {
            path,
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok((path, bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIPLE: &str = "aarch64-unknown-linux-musl";

    #[test]
    fn uname_machine_maps_to_agent_triple() {
        assert_eq!(agent_triple("aarch64").unwrap(), TRIPLE);
        assert_eq!(agent_triple("arm64\n").unwrap(), TRIPLE);
        assert_eq!(agent_triple("x86_64").unwrap(), "x86_64-unknown-linux-musl");
        assert_eq!(agent_triple("amd64").unwrap(), "x86_64-unknown-linux-musl");
        for other in ["armv7l", "riscv64", "i686", ""] {
            assert!(
                matches!(
                    agent_triple(other),
                    Err(AgentBundleError::UnsupportedArch { .. })
                ),
                "{other:?}"
            );
        }
    }

    #[test]
    fn verified_agent_loads_from_first_directory_that_has_it() {
        let empty = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"\x7fELF agent".to_vec();
        std::fs::write(dir.path().join(format!("kelvo-agent-{TRIPLE}")), &bytes).unwrap();
        let digest = hex_sha256(&bytes);
        let table = [(TRIPLE, digest.as_str())];

        let dirs = [empty.path().to_path_buf(), dir.path().to_path_buf()];
        let (path, loaded) = load(&dirs, TRIPLE, &table).unwrap();
        assert_eq!(path, dir.path().join(format!("kelvo-agent-{TRIPLE}")));
        assert_eq!(loaded, bytes);
    }

    #[test]
    fn changed_agent_fails_the_digest_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(format!("kelvo-agent-{TRIPLE}")),
            b"tampered",
        )
        .unwrap();
        let built = hex_sha256(b"original");
        let table = [(TRIPLE, built.as_str())];

        let err = load(&[dir.path().to_path_buf()], TRIPLE, &table).unwrap_err();
        match err {
            AgentBundleError::DigestMismatch {
                expected, actual, ..
            } => {
                assert_eq!(expected, built);
                assert_eq!(actual, hex_sha256(b"tampered"));
            }
            other => panic!("expected DigestMismatch, got {other:?}"),
        }
    }

    #[test]
    fn missing_digest_or_file_says_how_to_build_it() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = [dir.path().to_path_buf()];

        let err = load(&dirs, TRIPLE, &[]).unwrap_err();
        assert!(matches!(err, AgentBundleError::NotBuilt { .. }));
        assert!(err.to_string().contains("make agent-linux"), "{err}");

        let digest = hex_sha256(b"x");
        let table = [(TRIPLE, digest.as_str())];
        let err = load(&dirs, TRIPLE, &table).unwrap_err();
        assert!(matches!(err, AgentBundleError::Read { .. }), "{err:?}");
    }

    #[test]
    fn compiled_table_matches_the_binaries_on_disk() {
        // Whatever `make agent-linux` left beside the manifest is what build.rs hashed.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/agent");
        for (triple, _) in digests::AGENT_SHA256 {
            load(std::slice::from_ref(&dir), triple, digests::AGENT_SHA256).unwrap();
        }
    }
}
