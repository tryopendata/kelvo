//! `export_csv`'s file side: the store streams CSV rows ([`kelvo_store::Reader::export_csv`])
//! through a buffered writer into a temporary file next to the one the user picked in
//! the save dialog, which is then renamed over it. A failed or interrupted export leaves
//! the user's existing file (if any) as it was, and removes its temporary file.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use kelvo_store::{ExportQuery, StoreError};

use crate::error::CommandError;
use crate::history::History;
use crate::ipc::ExportOutcome;

/// The name the save dialog proposes when the request has none.
pub const DEFAULT_FILE_NAME: &str = "kelvo-history.csv";

/// The dialog's proposed name: the request's, without path separators, ending in `.csv`.
pub fn file_name(requested: Option<&str>) -> String {
    let name: String = requested
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':') && !c.is_control())
        .collect();
    let name = name.trim().trim_start_matches('.');
    if name.is_empty() {
        DEFAULT_FILE_NAME.to_string()
    } else if name.to_ascii_lowercase().ends_with(".csv") {
        name.to_string()
    } else {
        format!("{name}.csv")
    }
}

/// `.kelvo-<pid>-<n>.tmp` beside `path`: hidden in Finder, on the same volume so the
/// rename is atomic, short whatever the target's name (a 255-byte name plus a suffix
/// would be too long), and unique, so two exports to one path never share a file.
fn temp_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".kelvo-{}-{n}.tmp", std::process::id()))
}

/// Writes `query` as CSV to `path`, replacing any file there only once the export is
/// complete and on disk. Commits the writer's pending batch first, so the newest
/// minutes (up to 5, D-070) are in the file. Read failures come back as the store's
/// errors, file failures as [`CommandError::Export`]; either way `path` is untouched and
/// the temporary file is removed. Blocking: call it off the main thread.
pub fn write_csv(
    history: &History,
    query: &ExportQuery,
    path: &Path,
) -> Result<ExportOutcome, CommandError> {
    history.writer()?.flush()?;
    let temp = temp_path(path);
    let export_error = |e: std::io::Error| CommandError::Export {
        message: format!("{}: {e}", path.display()),
    };
    let written = File::create(&temp).map_err(export_error).and_then(|file| {
        let mut out = BufWriter::new(file);
        let summary = history
            .read(|r| Ok(r.export_csv(query, &mut out)))?
            .map_err(|e| match e {
                StoreError::Io(e) => export_error(e),
                e => e.into(),
            })?;
        let file = out.into_inner().map_err(|e| export_error(e.into_error()))?;
        file.sync_all().map_err(export_error)?;
        let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        drop(file);
        std::fs::rename(&temp, path).map_err(export_error)?;
        Ok((summary, bytes))
    });
    let (summary, bytes) = match written {
        Ok(w) => w,
        Err(e) => {
            match std::fs::remove_file(&temp) {
                Ok(()) => {}
                Err(rm) if rm.kind() == std::io::ErrorKind::NotFound => {}
                Err(rm) => {
                    tracing::warn!(path = %temp.display(), "removing a failed export: {rm}")
                }
            }
            return Err(e);
        }
    };
    tracing::info!(
        path = %path.display(),
        rows = summary.rows,
        gap_rows = summary.gap_rows,
        bytes,
        "history exported"
    );
    Ok(ExportOutcome::Saved {
        path: path.display().to_string(),
        rows: summary.rows,
        gap_rows: summary.gap_rows,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use kelvo_schema::{
        Gap, GapReason, HostId, HostInfo, HostRecord, Labels, MetricId, OsKind, SeriesKey,
        SeriesSelector, Tier,
    };
    use kelvo_store::{BucketRow, TierChoice};

    use super::*;

    const T0: i64 = 1_788_220_800_000;
    const MIN: i64 = 60_000;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kelvo-shell-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record() -> HostRecord {
        HostRecord {
            id: HostId(uuid::Uuid::from_u128(7)),
            is_local: true,
            display_name: "Mac".into(),
            info: HostInfo {
                os: OsKind::MacOs,
                os_version: "27.0".into(),
                model: None,
                chip: None,
                chip_known: true,
                cpu_topology: Vec::new(),
                mem_total_bytes: 0,
                boot_time_ms: 0,
                gpu_dvfs_mhz: Vec::new(),
                boot_mounts: Vec::new(),
            },
        }
    }

    /// Ten minutes of `cpu.total`, then a sleep gap.
    fn history(dir: &Path) -> (History, ExportQuery) {
        let history = History::open(dir);
        let h = record();
        let w = history.register_host(&h).unwrap();
        let series: Arc<[SeriesKey]> = vec![SeriesKey::parse("cpu.total").unwrap()].into();
        let minute = |i: i64| {
            let v = i as f32 * 1.5;
            w.write_bucket(BucketRow {
                host: h.id,
                tier: Tier::M1,
                bucket_ts: T0 + i * MIN,
                series: Arc::clone(&series),
                stats: vec![v - 1.0, v + 1.0, v],
            })
            .unwrap();
        };
        (0..8).for_each(minute);
        w.flush().unwrap();
        // The newest minutes and the gap are still in the writer's batch (D-070): the
        // store commits them every 5 minutes, and this test never waits that long.
        (8..10).for_each(minute);
        w.write_gap(
            h.id,
            Gap::host(T0 + 10 * MIN, Some(T0 + 20 * MIN), GapReason::Sleep).unwrap(),
        )
        .unwrap();
        let query = ExportQuery {
            host: h.id,
            selectors: vec![SeriesSelector {
                metric: MetricId::from_static("cpu.total"),
                labels: Labels::new(),
            }],
            from_ms: T0,
            to_ms: T0 + 30 * MIN,
            tier: TierChoice::Fixed(Tier::M1),
        };
        (history, query)
    }

    #[test]
    fn writes_the_file_with_uncommitted_minutes_and_reports_what_it_wrote() {
        let dir = temp_dir("export");
        let (history, query) = history(&dir);
        let path = dir.join("out.csv");
        std::fs::write(&path, "an older file").unwrap();
        let outcome = write_csv(&history, &query, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            outcome,
            ExportOutcome::Saved {
                path: path.display().to_string(),
                rows: 10,
                gap_rows: 1,
                bytes: text.len() as u64,
            }
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "time_utc,time_ms,cpu.total_avg,cpu.total_min,cpu.total_max,gap_reason,gap_end_ms"
        );
        assert_eq!(
            lines[2],
            format!("2026-09-01T00:01:00Z,{},1.5,0.5,2.5,,", T0 + MIN)
        );
        assert_eq!(
            lines[10],
            format!("2026-09-01T00:09:00Z,{},13.5,12.5,14.5,,", T0 + 9 * MIN),
            "the minutes not yet committed are in the file"
        );
        assert_eq!(
            lines[11],
            format!(
                "2026-09-01T00:10:00Z,{},,,,sleep,{}",
                T0 + 10 * MIN,
                T0 + 20 * MIN
            )
        );
        assert_eq!(lines.len(), 12);
        assert_eq!(temp_files(&dir), Vec::<String>::new());
        history.close();
    }

    /// Temporary export files left in `dir`.
    fn temp_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".kelvo-") || n.ends_with("-tmp"))
            .collect()
    }

    #[test]
    fn a_file_name_at_the_length_limit_exports() {
        let dir = temp_dir("export-long-name");
        let (history, query) = history(&dir);
        // 255 bytes, the most a macOS (APFS) or Linux file name can hold.
        let path = dir.join(format!("{}.csv", "k".repeat(251)));
        let outcome = write_csv(&history, &query, &path).unwrap();
        assert!(matches!(outcome, ExportOutcome::Saved { rows: 10, .. }));
        assert!(path.exists());
        assert_eq!(temp_files(&dir), Vec::<String>::new());
        history.close();
    }

    #[test]
    fn a_failed_export_leaves_the_old_file_and_no_temporary_one() {
        let dir = temp_dir("export-fails");
        let (history, query) = history(&dir);
        // A directory that does not exist: the file cannot be created.
        let missing = dir.join("gone").join("out.csv");
        let err = write_csv(&history, &query, &missing).unwrap_err();
        assert!(matches!(err, CommandError::Export { .. }), "{err:?}");

        // The user confirmed replacing a file, then the read fails (an unknown host)
        // after the export started writing: the old file is still there, whole.
        let path = dir.join("out.csv");
        std::fs::write(&path, "the user's older export").unwrap();
        let err = write_csv(
            &history,
            &ExportQuery {
                host: HostId(uuid::Uuid::from_u128(99)),
                ..query
            },
            &path,
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::UnknownHost { .. }), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "the user's older export"
        );
        assert_eq!(
            temp_files(&dir),
            Vec::<String>::new(),
            "the temporary file is removed"
        );
        history.close();
    }

    #[test]
    fn proposed_names_are_plain_csv_file_names() {
        assert_eq!(file_name(None), DEFAULT_FILE_NAME);
        assert_eq!(file_name(Some("  ")), DEFAULT_FILE_NAME);
        assert_eq!(file_name(Some("kelvo-2026-10-04")), "kelvo-2026-10-04.csv");
        assert_eq!(file_name(Some("week.CSV")), "week.CSV");
        assert_eq!(file_name(Some("../../etc/passwd")), "etcpasswd.csv");
    }
}
