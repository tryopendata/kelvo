//! Explicit first-use trust. A candidate contains the exact keys the user reviews;
//! acceptance never performs a second scan or silently accepts a changed key.
use crate::{RemoteError, Result};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

#[derive(Clone, Debug)]
pub struct TrustCandidate {
    pub alias: String,
    pub hostname: String,
    pub port: u16,
    pub fingerprints: Vec<String>,
    known_hosts: PathBuf,
    all_known_hosts: Vec<PathBuf>,
    lookup: String,
    keys: Vec<u8>,
    created: Instant,
}
#[expect(
    clippy::too_many_lines,
    reason = "service split tracked in architecture refactor package G"
)]
pub async fn inspect_host(alias: &str) -> Result<TrustCandidate> {
    let _validated_command = crate::ssh_command(alias)?;
    // -G must precede the host; ssh_command's -- host is already last, so construct
    // a separate read-only config invocation after validating the alias above.
    let config = bounded(
        Command::new("/usr/bin/ssh").args(["-G", "-oPermitLocalCommand=no", "--", alias]),
        None,
    )
    .await?;
    let options: HashMap<String, String> = String::from_utf8_lossy(&config)
        .lines()
        .filter_map(|line| {
            line.split_once(' ')
                .map(|(k, v)| (k.to_string(), v.trim().to_string()))
        })
        .collect();
    for option in ["proxycommand", "proxyjump"] {
        if options.get(option).is_some_and(|v| v != "none") {
            return Err(RemoteError::HostKey("This alias uses an SSH proxy. Verify its host key in Terminal, then retry in Kelvo.".into()));
        }
    }
    let hostname = options
        .get("hostname")
        .cloned()
        .ok_or_else(|| RemoteError::Protocol("SSH did not resolve a hostname".into()))?;
    if hostname.starts_with('-') || hostname.chars().any(char::is_whitespace) {
        return Err(RemoteError::Protocol("invalid SSH hostname".into()));
    }
    let port = options
        .get("port")
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|p| *p > 0)
        .ok_or_else(|| RemoteError::Protocol("invalid SSH port".into()))?;
    if port != 22 && options.get("hostkeyalias").is_some_and(|v| v != "none") {
        return Err(RemoteError::HostKey(
            "Verify custom HostKeyAlias with a nonstandard port in Terminal, then retry in Kelvo."
                .into(),
        ));
    }
    let lookup = options
        .get("hostkeyalias")
        .filter(|v| v.as_str() != "none")
        .cloned()
        .unwrap_or_else(|| {
            if port == 22 {
                hostname.clone()
            } else {
                format!("[{hostname}]:{port}")
            }
        });
    if lookup
        .chars()
        .any(|c| c.is_whitespace() || c == ',' || c == '\n')
    {
        return Err(RemoteError::Protocol(
            "unsupported SSH host-key alias".into(),
        ));
    }
    let home = std::env::var("HOME").map_err(|_| RemoteError::Protocol("HOME missing".into()))?;
    let resolve = |value: &str| -> Result<PathBuf> {
        if value.contains('%') || value.contains('"') || value.contains('\'') {
            return Err(RemoteError::HostKey(
                "Verify this custom known-hosts configuration in Terminal, then retry.".into(),
            ));
        }
        let value = if let Some(rest) = value.strip_prefix("~/") {
            format!("{home}/{rest}")
        } else {
            value.to_string()
        };
        if value == "none" || !value.starts_with('/') {
            return Err(RemoteError::HostKey(
                "SSH has no writable user known-hosts file; configure trust in Terminal.".into(),
            ));
        }
        Ok(value.into())
    };
    let user_paths = options
        .get("userknownhostsfile")
        .ok_or_else(|| RemoteError::Protocol("SSH known-hosts path missing".into()))?
        .split_whitespace()
        .map(resolve)
        .collect::<Result<Vec<_>>>()?;
    let known_hosts = user_paths
        .first()
        .cloned()
        .ok_or_else(|| RemoteError::Protocol("SSH known-hosts path missing".into()))?;
    let mut all_known_hosts = user_paths;
    if let Some(paths) = options.get("globalknownhostsfile") {
        for value in paths.split_whitespace() {
            if value != "none" {
                all_known_hosts.push(resolve(value)?);
            }
        }
    }
    ensure_unknown(&lookup, &all_known_hosts).await?;
    let scanned = bounded(
        Command::new("/usr/bin/ssh-keyscan").args([
            "-T",
            "5",
            "-p",
            &port.to_string(),
            "-t",
            "ed25519,ecdsa,rsa",
            "--",
            &hostname,
        ]),
        None,
    )
    .await?;
    let keys = normalize_keys(&lookup, &scanned)?;
    let fingerprint_text = bounded(
        Command::new("/usr/bin/ssh-keygen").args(["-l", "-f", "/dev/stdin", "-E", "sha256"]),
        Some(&keys),
    )
    .await?;
    let fingerprints = String::from_utf8_lossy(&fingerprint_text)
        .lines()
        .map(str::to_string)
        .collect();
    Ok(TrustCandidate {
        alias: alias.into(),
        hostname,
        port,
        fingerprints,
        known_hosts,
        all_known_hosts,
        lookup,
        keys,
        created: Instant::now(),
    })
}
fn normalize_keys(lookup: &str, scanned: &[u8]) -> Result<Vec<u8>> {
    let mut lines = std::collections::BTreeSet::new();
    for line in String::from_utf8_lossy(scanned).lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3
            || !matches!(
                fields[1],
                "ssh-ed25519"
                    | "ecdsa-sha2-nistp256"
                    | "ecdsa-sha2-nistp384"
                    | "ecdsa-sha2-nistp521"
                    | "ssh-rsa"
            )
            || !fields[2]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
        {
            return Err(RemoteError::Protocol("invalid host-key scan".into()));
        }
        lines.insert(format!("{lookup} {} {}\n", fields[1], fields[2]));
    }
    if lines.is_empty() || lines.len() > 6 {
        return Err(RemoteError::HostKey(
            "No supported SSH host keys were received.".into(),
        ));
    }
    Ok(lines.into_iter().collect::<String>().into_bytes())
}
async fn ensure_unknown(lookup: &str, paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        if !path.exists() {
            continue;
        }
        let mut command = Command::new("/usr/bin/ssh-keygen");
        command
            .args(["-F", lookup, "-f"])
            .arg(path)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(5), command.output())
            .await
            .map_err(|_| RemoteError::Transport("known-hosts lookup timed out".into()))??;
        if output.status.success() && !output.stdout.is_empty() {
            return Err(RemoteError::HostKey("This host already has a known key. Kelvo will not replace it; inspect any changed-key warning in Terminal.".into()));
        }
        if output.status.code() != Some(1) {
            return Err(RemoteError::HostKey(
                "Could not inspect SSH known-hosts; verify this host in Terminal.".into(),
            ));
        }
    }
    Ok(())
}
pub async fn accept_host(candidate: &TrustCandidate) -> Result<()> {
    if candidate.created.elapsed() > Duration::from_secs(300) {
        return Err(RemoteError::HostKey(
            "Host-key review expired; scan again.".into(),
        ));
    }
    let parent = candidate
        .known_hosts
        .parent()
        .ok_or_else(|| RemoteError::Protocol("invalid known-hosts path".into()))?;
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    // Avoid following a replaced known-hosts symlink, and don't broaden existing modes.
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&candidate.known_hosts)?;
    let metadata = file.metadata()?;
    // SAFETY: geteuid is always valid; flock borrows a live file descriptor.
    if metadata.uid() != unsafe { libc::geteuid() }
        || !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o022 != 0
    {
        return Err(RemoteError::HostKey(
            "known-hosts must be an ordinary file owned by this user".into(),
        ));
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(RemoteError::Transport(
            "known-hosts is being updated; retry".into(),
        ));
    }
    ensure_unknown(&candidate.lookup, &candidate.all_known_hosts).await?;
    if std::fs::symlink_metadata(&candidate.known_hosts)?.ino() != metadata.ino() {
        return Err(RemoteError::HostKey(
            "known-hosts changed during review; retry".into(),
        ));
    }
    // A newline prevents an unterminated existing last line from merging with the key.
    file.write_all(b"\n")?;
    file.write_all(&candidate.keys)?;
    file.sync_all()?;
    Ok(())
}
async fn bounded(command: &mut Command, input: Option<&[u8]>) -> Result<Vec<u8>> {
    command
        .kill_on_drop(true)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| RemoteError::Transport("missing stdin".into()))?;
        stdin.write_all(input).await?;
        stdin.shutdown().await?;
    }
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| RemoteError::Transport("missing stdout".into()))?
        .take(65537);
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| RemoteError::Transport("missing stderr".into()))?
        .take(65537);
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let (a, b, status) = tokio::join!(
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err),
            child.wait()
        );
        a?;
        b?;
        if out.len() > 65536 || err.len() > 65536 {
            return Err(RemoteError::Protocol("SSH trust output too large".into()));
        }
        if !status?.success() {
            return Err(RemoteError::Transport(
                String::from_utf8_lossy(&err).trim().to_string(),
            ));
        }
        Ok(out)
    })
    .await
    .map_err(|_| RemoteError::Transport("SSH trust inspection timed out".into()))?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn acceptance_pins_reviewed_bytes_and_never_replaces_existing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key");
        let generated = Command::new("/usr/bin/ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .status()
            .await
            .unwrap();
        assert!(generated.success());
        let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
        let fields: Vec<_> = public.split_whitespace().collect();
        let known = dir.path().join("known_hosts");
        let bytes = format!("mac {} {}\n", fields[0], fields[1]).into_bytes();
        let candidate = TrustCandidate {
            alias: "mac".into(),
            hostname: "mac.local".into(),
            port: 22,
            fingerprints: vec![],
            known_hosts: known.clone(),
            all_known_hosts: vec![known.clone()],
            lookup: "mac".into(),
            keys: bytes.clone(),
            created: Instant::now(),
        };
        accept_host(&candidate).await.unwrap();
        assert_eq!(
            std::fs::read(&known).unwrap(),
            [b"\n".as_slice(), bytes.as_slice()].concat()
        );
        let original = std::fs::read(&known).unwrap();
        assert!(accept_host(&candidate).await.is_err());
        assert_eq!(std::fs::read(&known).unwrap(), original);
    }
    #[tokio::test]
    async fn expired_review_cannot_write_known_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let known = dir.path().join("known_hosts");
        let candidate = TrustCandidate {
            alias: "mac".into(),
            hostname: "mac.local".into(),
            port: 22,
            fingerprints: vec![],
            known_hosts: known.clone(),
            all_known_hosts: vec![known.clone()],
            lookup: "mac".into(),
            keys: vec![],
            created: Instant::now() - Duration::from_secs(301),
        };
        assert!(accept_host(&candidate).await.is_err());
        assert!(!known.exists());
    }
    #[test]
    fn rewrites_only_lookup_and_rejects_malformed_scans() {
        let normalized = normalize_keys(
            "[mac]:2222",
            b"# comment\n127.0.0.1 ssh-ed25519 AAAA\n127.0.0.1 ssh-ed25519 AAAA\n",
        )
        .unwrap();
        assert_eq!(normalized, b"[mac]:2222 ssh-ed25519 AAAA\n");
        assert!(normalize_keys("mac", b"host ssh-ed25519 key;evil\n").is_err());
        assert!(normalize_keys("mac", b"# only comment\n").is_err());
    }
}
