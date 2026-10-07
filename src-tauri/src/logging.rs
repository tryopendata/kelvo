//! `tracing` to `~/Library/Logs/com.tryopendata.kelvo/` (v1-local-monitor.md 6.5), plus stderr
//! in debug builds. `RUST_LOG` overrides the default `info` level.
//!
//! Rotation is daily with the five newest files kept. `tracing-appender` rotates by time
//! only; at Kelvo's log volume (startup, settings changes, collector errors rate-limited
//! to one a minute) a day stays far below the 5 MB per file the plan budgets.
//!
//! A log directory that cannot be created or written (root-owned after a `sudo` run, a
//! full disk) does not stop the launch: logging falls back to stderr, which Console.app
//! still shows for the process.

use std::path::Path;

use anyhow::Context;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub const LOG_FILES_KEPT: usize = 5;

fn file_appender(dir: &Path) -> anyhow::Result<RollingFileAppender> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("kelvo")
        .filename_suffix("log")
        .max_log_files(LOG_FILES_KEPT)
        .build(dir)
        .context("creating the log file appender")
}

/// Installs the global subscriber: the log files in `dir` (`None` when the directory
/// could not be resolved), or stderr only when they cannot be opened. Keep the guard alive
/// for the life of the app: dropping it flushes and stops the background writer. `None`
/// when there is no file writer.
pub fn init(dir: Option<&Path>) -> Option<WorkerGuard> {
    let appender = dir
        .context("the log directory could not be resolved")
        .and_then(file_appender);
    let (file, guard, failed) = match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false);
            (Some(layer), Some(guard), None)
        }
        Err(e) => (None, None, Some(e)),
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr = (cfg!(debug_assertions) || failed.is_some())
        .then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    if let Err(e) = tracing_subscriber::registry()
        .with(filter)
        .with(file)
        .with(stderr)
        .try_init()
    {
        eprintln!("installing the tracing subscriber: {e}");
    }
    if let Some(e) = failed {
        tracing::error!("cannot write log files, logging to stderr only: {e:#}");
    }
    guard
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unusable_log_dir_is_an_error_not_a_panic() {
        let parent = std::env::temp_dir().join(format!(
            "kelvo-shell-logs-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join("file"), "").unwrap();
        assert!(file_appender(&parent.join("file").join("logs")).is_err());
        assert!(file_appender(&parent.join("logs")).is_ok());
    }
}
