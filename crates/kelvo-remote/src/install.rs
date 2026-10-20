//! Narrow setup operations. Endpoints and paths are never interpolated unquoted.
use crate::{
    ControlRequest, ControlResponse, RemoteError, Result,
    transport::{helper_command, shell_quote, ssh_command},
};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallProbe {
    pub os: String,
    #[serde(default)]
    pub os_version: String,
    pub arch: String,
    pub user: String,
    pub console_user: String,
    pub app_path: Option<String>,
    pub helper_path: Option<String>,
}

/// Execute a fixed setup command using the same strict SSH policy as live sessions.
/// Output is bounded; stalled helpers are killed through Command::kill_on_drop.
pub async fn run_remote(alias: &str, command: &str) -> Result<String> {
    run_remote_with_timeout(alias, command, Duration::from_secs(30)).await
}

async fn run_remote_with_timeout(alias: &str, command: &str, timeout: Duration) -> Result<String> {
    let mut cmd = ssh_command(alias)?;
    cmd.arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RemoteError::Transport("missing stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RemoteError::Transport("missing stderr".into()))?;
    setup_timeout(timeout, async {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut stdout = stdout.take(65537);
        let mut stderr = stderr.take(65537);
        let (out, err, status) = tokio::join!(
            stdout.read_to_end(&mut output),
            stderr.read_to_end(&mut errors),
            child.wait()
        );
        out?;
        err?;
        let status = status?;
        if output.len() > 65536 || errors.len() > 65536 {
            return Err(RemoteError::Protocol("setup output exceeded bound".into()));
        }
        if !status.success() {
            return Err(setup_ssh_error(
                String::from_utf8_lossy(&errors).trim().to_string(),
            ));
        }
        String::from_utf8(output).map_err(|e| RemoteError::Protocol(e.to_string()))
    })
    .await
}

pub(crate) async fn setup_timeout<T>(
    timeout: Duration,
    operation: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| RemoteError::Transport("setup command timed out".into()))?
}
const RESTORE_TIMEOUT: Duration = Duration::from_secs(600);

pub(crate) fn setup_ssh_error(message: String) -> RemoteError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("host key verification failed")
        || lower.contains("remote host identification has changed")
        || lower.contains("no host key is known")
    {
        RemoteError::HostKey(message)
    } else if lower.contains("permission denied") || lower.contains("authentication failed") {
        RemoteError::Authentication(message)
    } else {
        RemoteError::Transport(message)
    }
}

pub async fn probe(alias: &str) -> Result<InstallProbe> {
    let text=run_remote(alias,r#"printf '%s\n' "$(uname -s)" "$(sw_vers -productVersion)" "$(uname -m)" "$(id -un)" "$(stat -f '%Su' /dev/console)"; for app in "$HOME/Applications/Kelvo.app" /Applications/Kelvo.app; do if [ -d "$app" ]; then printf '%s\n' "$app"; if [ -x "$app/Contents/MacOS/kelvo-remote-helper" ]; then printf '%s\n' "$app/Contents/MacOS/kelvo-remote-helper"; fi; break; fi; done"#).await?;
    let mut lines = text.lines();
    let p = InstallProbe {
        os: lines.next().unwrap_or_default().into(),
        os_version: lines.next().unwrap_or_default().into(),
        arch: lines.next().unwrap_or_default().into(),
        user: lines.next().unwrap_or_default().into(),
        console_user: lines.next().unwrap_or_default().into(),
        app_path: lines.next().filter(|s| !s.is_empty()).map(Into::into),
        helper_path: lines.next().filter(|s| !s.is_empty()).map(Into::into),
    };
    if p.os != "Darwin" || p.arch != "arm64" {
        return Err(RemoteError::Incompatible(
            "requires an Apple Silicon Mac".into(),
        ));
    }
    if p.os_version
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
        < 26
    {
        return Err(RemoteError::Incompatible(
            "Kelvo requires macOS 26 or newer".into(),
        ));
    }
    if p.user != p.console_user {
        return Err(RemoteError::Authentication(
            "SSH user must match the logged-in desktop user".into(),
        ));
    }
    Ok(p)
}
pub async fn version(alias: &str, path: &str) -> Result<String> {
    Ok(run_remote(alias, &helper_command(path, "--version"))
        .await?
        .trim()
        .to_string())
}
pub async fn configure(
    alias: &str,
    path: &str,
    request: &ControlRequest,
) -> Result<ControlResponse> {
    let json = serde_json::to_string(request).map_err(|e| RemoteError::Protocol(e.to_string()))?;
    let text = run_remote(
        alias,
        &helper_command(path, &format!("configure {}", shell_quote(&json))),
    )
    .await?;
    serde_json::from_str(&text).map_err(|e| RemoteError::Protocol(e.to_string()))
}
pub async fn status(alias: &str, path: &str) -> Result<ControlResponse> {
    let text = run_remote(alias, &helper_command(path, "status")).await?;
    serde_json::from_str(&text).map_err(|e| RemoteError::Protocol(e.to_string()))
}
pub async fn launch(alias: &str, app_path: &str) -> Result<()> {
    run_remote(
        alias,
        &format!(
            "/usr/bin/open -g {} --args --remote-setup",
            shell_quote(app_path)
        ),
    )
    .await?;
    Ok(())
}
/// Fresh installation only. Existing bundles require an explicit update transaction
/// from the settings owner; this function never replaces them.
pub async fn install_app(alias: &str, local_bundle: &Path) -> Result<InstallProbe> {
    let before = probe(alias).await?;
    if before.app_path.is_some() {
        return Err(RemoteError::Protocol(
            "Kelvo already installed; explicit update required".into(),
        ));
    }
    transfer_app(alias, local_bundle, None, None, None).await?;
    probe(alias).await
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallUpdate {
    pub probe: InstallProbe,
    pub backup_path: String,
    pub data_path: String,
    pub data_backup_path: String,
}
/// Stage and swap an explicitly requested update. The caller validates the launch
/// before finishing; until then the old bundle remains available for rollback.
pub async fn update_app(
    alias: &str,
    local_bundle: &Path,
    existing_app: &str,
) -> Result<InstallUpdate> {
    if !existing_app.ends_with("/Kelvo.app") || !existing_app.starts_with('/') {
        return Err(RemoteError::Protocol("invalid installed app path".into()));
    }
    let backup = format!("{}.kelvo-backup-{}", existing_app, uuid::Uuid::new_v4());
    let home = run_remote(alias, "printf '%s' \"$HOME\"").await?;
    if !home.starts_with('/') || home.contains('\n') {
        return Err(RemoteError::Protocol("invalid remote home path".into()));
    }
    let data = format!("{home}/Library/Application Support/com.tryopendata.kelvo");
    let data_backup = format!(
        "{data}.kelvo-backup-{}",
        backup
            .rsplit_once(".kelvo-backup-")
            .map(|(_, id)| id)
            .unwrap_or_default()
    );
    transfer_app(
        alias,
        local_bundle,
        Some(existing_app),
        Some(&backup),
        Some((&data, &data_backup)),
    )
    .await?;
    let result = probe(alias).await;
    match result {
        Ok(probe) => Ok(InstallUpdate {
            probe,
            backup_path: backup,
            data_path: data,
            data_backup_path: data_backup,
        }),
        Err(error) => {
            let _ = restore_bundle(alias, existing_app, &backup, &data, &data_backup).await;
            Err(error)
        }
    }
}
pub async fn finish_update(alias: &str, update: &InstallUpdate) -> Result<()> {
    validate_backup(update)?;
    run_remote(
        alias,
        &format!(
            "/bin/rm -rf -- {} {}",
            shell_quote(&update.backup_path),
            shell_quote(&update.data_backup_path)
        ),
    )
    .await?;
    Ok(())
}
pub async fn rollback_update(alias: &str, update: &InstallUpdate) -> Result<()> {
    let app = validate_backup(update)?;
    restore_bundle(
        alias,
        app,
        &update.backup_path,
        &update.data_path,
        &update.data_backup_path,
    )
    .await
}
fn validate_backup(update: &InstallUpdate) -> Result<&str> {
    let app = update
        .probe
        .app_path
        .as_deref()
        .ok_or_else(|| RemoteError::Protocol("update app missing".into()))?;
    let prefix = format!("{app}.kelvo-backup-");
    if !update.backup_path.starts_with(&prefix)
        || uuid::Uuid::parse_str(&update.backup_path[prefix.len()..]).is_err()
    {
        return Err(RemoteError::Protocol("invalid backup path".into()));
    }
    let suffix = &update.backup_path[prefix.len()..];
    if !update
        .data_path
        .ends_with("/Library/Application Support/com.tryopendata.kelvo")
        || update.data_backup_path != format!("{}.kelvo-backup-{suffix}", update.data_path)
    {
        return Err(RemoteError::Protocol("invalid data backup path".into()));
    }
    Ok(app)
}
async fn restore_bundle(
    alias: &str,
    app: &str,
    backup: &str,
    data: &str,
    data_backup: &str,
) -> Result<()> {
    restore_bundle_with_runner(
        app,
        backup,
        data,
        data_backup,
        |command, timeout| async move { run_remote_with_timeout(alias, &command, timeout).await },
    )
    .await
}

async fn restore_bundle_with_runner<F: std::future::Future<Output = Result<String>>>(
    app: &str,
    backup: &str,
    data: &str,
    data_backup: &str,
    run: impl FnOnce(String, Duration) -> F,
) -> Result<()> {
    run(
        restore_script(app, backup, data, data_backup, true),
        RESTORE_TIMEOUT,
    )
    .await?;
    Ok(())
}

fn restore_script(
    app: &str,
    backup: &str,
    data: &str,
    data_backup: &str,
    check_running: bool,
) -> String {
    let failed = format!("{app}.failed-{}", uuid::Uuid::new_v4());
    let failed_data = format!("{data}.failed-{}", uuid::Uuid::new_v4());
    let stopped = if check_running {
        "if /usr/bin/pgrep -ix kelvo >/dev/null; then echo 'Quit Kelvo before restoring the previous version' >&2; exit 1; fi;"
    } else {
        ""
    };
    // Restore app data before putting the old binary back, so an incompatible old
    // executable is never paired with a migrated database even if restoration fails.
    format!(
        "set -eu; {stopped} app={}; backup={}; data={}; snapshot={}; failed={}; failed_data={}; test -d \"$backup\"; test -f \"$snapshot/complete\"; if test -e \"$data\"; then /bin/mv \"$data\" \"$failed_data\"; fi; if test -d \"$snapshot/data\"; then if ! /usr/bin/ditto \"$snapshot/data\" \"$data\"; then /bin/rm -rf -- \"$data\"; if test -e \"$failed_data\"; then /bin/mv \"$failed_data\" \"$data\"; fi; exit 1; fi; fi; /bin/mv \"$app\" \"$failed\"; if ! /bin/mv \"$backup\" \"$app\"; then /bin/mv \"$failed\" \"$app\"; echo 'App data restored; bundle restore needs recovery' >&2; exit 1; fi; /bin/rm -rf -- \"$failed\" \"$failed_data\" \"$snapshot\"",
        shell_quote(app),
        shell_quote(backup),
        shell_quote(data),
        shell_quote(data_backup),
        shell_quote(&failed),
        shell_quote(&failed_data)
    )
}
fn snapshot_script(data: &str, backup: &str) -> String {
    format!(
        "data={}; snapshot={}; test ! -e \"$snapshot\"; /bin/mkdir -m 700 \"$snapshot\"; if test -d \"$data\"; then /usr/bin/ditto \"$data\" \"$snapshot/data\"; fi; /usr/bin/touch \"$snapshot/complete\";",
        shell_quote(data),
        shell_quote(backup)
    )
}

async fn transfer_app(
    alias: &str,
    local_bundle: &Path,
    target: Option<&str>,
    backup: Option<&str>,
    data_backup: Option<(&str, &str)>,
) -> Result<()> {
    if local_bundle.file_name().and_then(|s| s.to_str()) != Some("Kelvo.app")
        || !local_bundle
            .join("Contents/MacOS/kelvo-remote-helper")
            .is_file()
    {
        return Err(RemoteError::Protocol(
            "a packaged Kelvo.app with bundled helper is required".into(),
        ));
    }
    let archive = std::env::temp_dir().join(format!("kelvo-install-{}.zip", uuid::Uuid::new_v4()));
    let _cleanup = ArchiveCleanup(archive.clone());
    let status = tokio::process::Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
        .arg(local_bundle)
        .arg(&archive)
        .kill_on_drop(true)
        .status()
        .await?;
    if !status.success() {
        return Err(RemoteError::Transport("failed to package app".into()));
    }
    let hash = tokio::process::Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(&archive)
        .output()
        .await?;
    if !hash.status.success() {
        return Err(RemoteError::Transport("failed to hash package".into()));
    }
    let hash = String::from_utf8_lossy(&hash.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(RemoteError::Protocol("invalid package digest".into()));
    }
    let destination = target
        .map(shell_quote)
        .unwrap_or_else(|| "\"$HOME\"/Applications/Kelvo.app".into());
    let snapshot = data_backup
        .map(|(data, backup)| snapshot_script(data, backup))
        .unwrap_or_default();
    let replace = if let Some(backup) = backup {
        format!(
            "if /usr/bin/pgrep -ix kelvo >/dev/null; then echo 'Quit Kelvo before updating' >&2; exit 1; fi; test -d \"$dest\"; test ! -e {}; {snapshot} if /usr/bin/pgrep -ix kelvo >/dev/null; then echo 'Kelvo reopened during update; quit and retry' >&2; exit 1; fi; /bin/mv \"$dest\" {}; if ! /bin/mv \"$stage/Kelvo.app\" \"$dest\"; then /bin/mv {} \"$dest\"; exit 1; fi",
            shell_quote(backup),
            shell_quote(backup),
            shell_quote(backup)
        )
    } else {
        "if test -e \"$dest\"; then echo 'Kelvo already installed' >&2; exit 1; fi; /bin/mv -n \"$stage/Kelvo.app\" \"$dest\"; test ! -e \"$stage/Kelvo.app\"".into()
    };
    let script = format!(
        r#"set -eu; dest={destination}; parent=$(/usr/bin/dirname "$dest"); mkdir -p "$parent"; stage=$(mktemp -d "$parent/.kelvo-install.XXXXXX"); trap 'rm -rf "$stage"' EXIT; cat > "$stage/app.zip"; actual=$(/usr/bin/shasum -a 256 "$stage/app.zip"); test "${{actual%% *}}" = {}; /usr/bin/ditto -x -k "$stage/app.zip" "$stage"; test -x "$stage/Kelvo.app/Contents/MacOS/kelvo-remote-helper"; /usr/bin/codesign --verify --deep --strict "$stage/Kelvo.app"; {replace}"#,
        shell_quote(&hash)
    );
    let mut cmd = ssh_command(alias)?;
    let mut child = cmd
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| RemoteError::Transport("missing installer stdin".into()))?;
    let mut package = tokio::fs::File::open(&archive).await?;
    tokio::time::timeout(Duration::from_secs(300), async move {
        let mut input = input;
        tokio::io::copy(&mut package, &mut input).await?;
        input.shutdown().await?;
        drop(input);
        let result = child.wait_with_output().await?;
        if !result.status.success() {
            return Err(RemoteError::Transport(
                String::from_utf8_lossy(&result.stderr).trim().to_string(),
            ));
        }
        Ok(())
    })
    .await
    .map_err(|_| RemoteError::Transport("installation timed out".into()))?
}
struct ArchiveCleanup(std::path::PathBuf);
impl Drop for ArchiveCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn restoration_can_outlast_standard_setup_timeout() {
        let result = restore_bundle_with_runner(
            "/Applications/Kelvo.app",
            "/Applications/Kelvo.app.backup",
            "/data",
            "/snapshot",
            |_, timeout| {
                setup_timeout(timeout, async {
                    tokio::time::sleep(Duration::from_secs(31)).await;
                    Ok("restored".into())
                })
            },
        )
        .await;
        result.unwrap();
        assert!(
            setup_timeout(Duration::from_secs(30), async {
                tokio::time::sleep(Duration::from_secs(31)).await;
                Ok(())
            })
            .await
            .is_err()
        );
    }

    #[test]
    fn setup_distinguishes_host_trust_from_login_and_connectivity() {
        assert!(matches!(
            setup_ssh_error("Host key verification failed.".into()),
            RemoteError::HostKey(_)
        ));
        assert!(matches!(
            setup_ssh_error("Permission denied (publickey).".into()),
            RemoteError::Authentication(_)
        ));
        assert!(matches!(
            setup_ssh_error("Connection refused".into()),
            RemoteError::Transport(_)
        ));
    }

    // Executes the macOS deployment script locally, including /usr/bin/ditto.
    #[cfg(target_os = "macos")]
    #[test]
    fn restoring_after_database_migration_restores_both_binary_and_data() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let app = root.join("Kelvo.app");
        let old = root.join("old.app");
        let data = root.join("data");
        let snapshot = root.join("snapshot");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(app.join("binary"), "new").unwrap();
        std::fs::write(old.join("binary"), "old").unwrap();
        std::fs::write(data.join("history"), "schema4").unwrap();
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                &format!(
                    "set -eu; {}",
                    snapshot_script(data.to_str().unwrap(), snapshot.to_str().unwrap())
                ),
            ])
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(data.join("history"), "schema5").unwrap();
        let script = restore_script(
            app.to_str().unwrap(),
            old.to_str().unwrap(),
            data.to_str().unwrap(),
            snapshot.to_str().unwrap(),
            false,
        );
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read_to_string(app.join("binary")).unwrap(), "old");
        assert_eq!(
            std::fs::read_to_string(data.join("history")).unwrap(),
            "schema4"
        );
        assert!(!snapshot.exists());
    }
}
