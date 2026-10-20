//! Putting the agent binary in place: streamed over SSH stdin, verified on the host,
//! then renamed over the old one.
use std::time::Duration;

use super::{
    AgentInstallError, AgentTarget, Remote, Result, Ssh, UPLOAD_TIMEOUT, failed, run_script,
};
use crate::transport::sh_c;

/// Exit status of the upload script when the bytes do not match their digest.
const DIGEST_MISMATCH: i32 = 3;

/// Runs `command` (a full remote command line) on `alias` with `bytes` on its stdin and
/// returns its stdout; a non-zero exit is an error carrying its stderr.
pub async fn run_remote_with_stdin(
    alias: &str,
    command: &str,
    bytes: &[u8],
    timeout: Duration,
) -> crate::Result<String> {
    let output = Ssh(alias).exec(command, Some(bytes), timeout).await?;
    if output.code == Some(0) {
        Ok(output.stdout)
    } else {
        Err(failed(&output))
    }
}

/// The upload script for `target`, checking the bytes against `sha256_hex`.
///
/// The bytes land in the destination directory, so on SELinux hosts they are created
/// with that directory's label (`bin_t` in `/usr/local/bin`) and the rename keeps it;
/// `restorecon` then settles anything policy says differently. The binary in place, if
/// any, is kept as `.kelvo-agent.prev` for an update's rollback.
pub(crate) fn upload_script(target: &AgentTarget, sha256_hex: &str) -> Result<String> {
    if sha256_hex.len() != 64 || !sha256_hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(crate::RemoteError::Protocol("invalid sha256 digest".into()).into());
    }
    let words = target.words()?;
    let bin = words.bin;
    let expected = sha256_hex.to_ascii_lowercase();
    let digest = target.digest.script();
    Ok(format!(
        r#"set -eu
umask 077
bin={bin}
dir=${{bin%/*}}
tmp=$dir/.kelvo-agent.tmp
prev=$dir/.kelvo-agent.prev
cleanup() {{ rm -f "$tmp"; }}
trap cleanup EXIT
mkdir -p "$dir"
cat > "$tmp"
{digest}
if [ "$actual" != {expected} ]; then echo "kelvo-agent upload: digest mismatch ($actual)" >&2; exit {DIGEST_MISMATCH}; fi
chmod 0755 "$tmp"
if [ -e "$bin" ]; then cp -p "$bin" "$prev"; fi
mv -f "$tmp" "$bin"
if command -v restorecon >/dev/null 2>&1; then restorecon -F "$bin" || true; fi
"#
    ))
}

/// Uploads `bytes` as the agent binary of `target` once the host has checked them
/// against `sha256_hex`. A mismatch removes the upload, leaves the old binary in place
/// and is [`AgentInstallError::DigestMismatch`].
pub async fn upload_agent(
    alias: &str,
    target: &AgentTarget,
    bytes: &[u8],
    sha256_hex: &str,
) -> Result<()> {
    upload_with(&Ssh(alias), target, bytes, sha256_hex).await
}

pub(crate) async fn upload_with(
    remote: &impl Remote,
    target: &AgentTarget,
    bytes: &[u8],
    sha256_hex: &str,
) -> Result<()> {
    let script = upload_script(target, sha256_hex)?;
    let output = remote
        .exec(&sh_c(&script)?, Some(bytes), UPLOAD_TIMEOUT)
        .await?;
    match output.code {
        Some(0) => Ok(()),
        Some(DIGEST_MISMATCH) => Err(AgentInstallError::DigestMismatch {
            expected: sha256_hex.to_ascii_lowercase(),
        }),
        _ => Err(failed(&output).into()),
    }
}

/// Removes the previous binary an upload kept, once an update has succeeded.
pub(crate) async fn drop_previous(remote: &impl Remote, target: &AgentTarget) -> Result<()> {
    let bin = target.words()?.bin;
    run_script(
        remote,
        &format!("bin={bin}; rm -f \"${{bin%/*}}/.kelvo-agent.prev\""),
        None,
        super::STEP_TIMEOUT,
    )
    .await
    .map(drop)
}
