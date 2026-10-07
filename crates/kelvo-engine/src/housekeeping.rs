//! History housekeeping: the pruning schedule, the low-disk check (D-057) and the
//! history health they produce (D-059). Lives in the engine rather than the app shell so a
//! headless agent (v4) prunes and guards its store the same way. The store provides the
//! mechanisms ([`Writer::prune`], [`LowDiskGuard`]); this module decides when to run them
//! and what the result means for the UI.
//!
//! The loop runs on its own thread and sleeps between rounds; each round uses whatever
//! writer the caller hands it at that moment, so a store replaced by a reset is pruned
//! too, and a round with no store open is skipped.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use kelvo_schema::lock::LockExt;
use kelvo_schema::settings::HistorySettings;
use kelvo_store::{LowDiskGuard, PruneReport, Retention, StatVfs, StoreError, Writer};

use crate::clock::wall_ms;

/// When housekeeping rounds run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// First prune after launch: late enough not to compete with startup.
    pub first_prune_after: Duration,
    /// Pruning cadence. Rows age out by the hour at most, so hourly keeps the file within
    /// a percent or so of its retention size.
    pub prune_every: Duration,
    /// Free-space check cadence for the low-disk guard (D-057): one `statvfs`, not per
    /// tick.
    pub disk_check_every: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            first_prune_after: Duration::from_secs(120),
            prune_every: Duration::from_secs(3_600),
            disk_check_every: Duration::from_secs(300),
        }
    }
}

/// The store's retention for the history settings: `retention_days` of history (minutes
/// for the last 7 days, 15-minute buckets before that, D-076) under the user's size limit
/// (D-057, D-059).
pub fn retention_for(h: &HistorySettings) -> Retention {
    Retention {
        max_bytes: h.size_limit_bytes(),
        ..Retention::with_days(h.retention_days)
    }
}

/// What housekeeping knows about the history store: the low-disk pause and the size-limit
/// trim. The app shell maps it to its IPC type of the same name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryHealth {
    /// The volume is almost full: 10 s history is not being written.
    pub low_disk_paused: bool,
    /// Set once the size limit has trimmed history: it now starts here, ms epoch.
    pub trimmed_before_ms: Option<i64>,
    /// The size limit in effect at that trim, bytes.
    pub trimmed_limit_bytes: Option<u64>,
    /// False when even the last day does not fit under the limit.
    pub cap_met: bool,
}

impl Default for HistoryHealth {
    fn default() -> Self {
        Self {
            low_disk_paused: false,
            trimmed_before_ms: None,
            trimmed_limit_bytes: None,
            cap_met: true,
        }
    }
}

impl HistoryHealth {
    /// The health after history was cleared: no trim to report.
    pub fn without_trim(self) -> Self {
        Self {
            trimmed_before_ms: None,
            trimmed_limit_bytes: None,
            cap_met: true,
            ..self
        }
    }
}

/// The health after one housekeeping round. `report` is the prune's result when this
/// round pruned. A trim records where history now starts and the limit that caused it;
/// the note clears once time-based retention alone would have dropped that history
/// anyway, since from then on the limit is no longer what shortens it.
pub fn next_health(
    prev: HistoryHealth,
    report: Option<&PruneReport>,
    low_disk_paused: bool,
    now_ms: i64,
    retention: &Retention,
) -> HistoryHealth {
    let mut h = HistoryHealth {
        low_disk_paused,
        ..prev
    };
    let Some(report) = report else {
        return h;
    };
    match report.cap_trim {
        Some(trim) => {
            h.trimmed_before_ms = Some(trim.earliest_ts_ms);
            h.trimmed_limit_bytes = Some(retention.max_bytes);
            h.cap_met = trim.cap_met;
        }
        None => {
            h.cap_met = true;
            if h.trimmed_before_ms
                .is_some_and(|t| t <= now_ms - retention.history_ms)
            {
                h.trimmed_before_ms = None;
                h.trimmed_limit_bytes = None;
            }
        }
    }
    h
}

/// Wakes the housekeeping thread. The thread ends when this is dropped.
pub struct HousekeepingHandle {
    wake: Sender<()>,
}

impl HousekeepingHandle {
    /// Prunes now rather than at the next scheduled round (the retention or size limit
    /// changed). A wake while a round runs is kept and starts another right after.
    pub fn prune_now(&self) {
        // Full means a wake is already pending, which is all this asks for.
        let _ = self.wake.try_send(());
    }
}

/// What the housekeeping thread works with.
pub struct Housekeeping<W, R, C> {
    /// The database file; its volume is what the low-disk guard checks.
    pub path: PathBuf,
    pub schedule: Schedule,
    /// The writer of the store open now, if any.
    pub writer: W,
    /// The retention in effect now.
    pub retention: R,
    /// Shared with the caller, who reads it and resets it (a history reset, a clear).
    pub health: Arc<Mutex<HistoryHealth>>,
    /// Called with the new health whenever a round changes it.
    pub on_change: C,
}

impl<W, R, C> Housekeeping<W, R, C>
where
    W: Fn() -> Option<Writer> + Send + 'static,
    R: Fn() -> Retention + Send + 'static,
    C: Fn(HistoryHealth) + Send + 'static,
{
    /// Prunes on the schedule (and whenever the handle asks) with the retention at that
    /// moment. Checks free space every `disk_check_every` and after each prune, and
    /// pauses 10 s history while the disk is almost full (D-057).
    pub fn spawn(self) -> std::io::Result<HousekeepingHandle> {
        let (wake, rx) = crossbeam_channel::bounded(1);
        std::thread::Builder::new()
            .name("kelvo-housekeeping".into())
            .spawn(move || self.run(&rx))?;
        Ok(HousekeepingHandle { wake })
    }

    fn run(self, wake: &Receiver<()>) {
        let mut next_prune = Instant::now() + self.schedule.first_prune_after;
        loop {
            let wait = next_prune
                .saturating_duration_since(Instant::now())
                .min(self.schedule.disk_check_every);
            let triggered = match wake.recv_timeout(wait) {
                Ok(()) => true,
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            let prune = triggered || Instant::now() >= next_prune;
            if prune {
                next_prune = Instant::now() + self.schedule.prune_every;
            }
            let Some(writer) = (self.writer)() else {
                continue;
            };
            let retention = (self.retention)();
            let now = wall_ms();
            match round(&writer, &self.path, now, prune.then_some(retention)) {
                Ok((report, paused)) => {
                    if let Some(report) = &report {
                        tracing::info!(?retention, ?report, "history pruned");
                        log_cap_trim(report, retention.max_bytes);
                    }
                    let changed = {
                        let mut h = self.health.lock_ok();
                        let next = next_health(*h, report.as_ref(), paused, now, &retention);
                        let changed = next != *h;
                        *h = next;
                        changed.then_some(next)
                    };
                    if let Some(next) = changed {
                        tracing::info!(?next, "history health changed");
                        (self.on_change)(next);
                    }
                }
                // The store closed under the round (quit, or a reset): the next round
                // uses the new one, if any.
                Err(StoreError::WriterGone) => {}
                Err(e) => tracing::error!("history housekeeping failed: {e}"),
            }
        }
    }
}

/// One round: prune when `retention` is given, then check free space. A failed
/// free-space read is logged and leaves the mode as it was.
fn round(
    writer: &Writer,
    path: &Path,
    now: i64,
    retention: Option<Retention>,
) -> kelvo_store::Result<(Option<PruneReport>, bool)> {
    let report = retention.map(|r| writer.prune(now, r)).transpose()?;
    let mut guard = LowDiskGuard::new(StatVfs, path, writer.clone());
    let paused = match guard.check(now) {
        Ok(paused) => paused,
        Err(StoreError::Io(e)) => {
            tracing::warn!("reading free disk space: {e}");
            guard.paused()
        }
        Err(e) => return Err(e),
    };
    Ok((report, paused))
}

fn log_cap_trim(report: &PruneReport, max_bytes: u64) {
    let Some(trim) = report.cap_trim else {
        return;
    };
    let cap_mb = max_bytes / 1_000_000;
    if trim.cap_met {
        tracing::warn!(
            earliest_ts_ms = trim.earliest_ts_ms,
            m1_rows = trim.m1_rows,
            m15_rows = trim.m15_rows,
            size_before = trim.size_before,
            size_after = report.size_bytes,
            "history trimmed to stay under {cap_mb} MB"
        );
    } else {
        tracing::error!(
            size_after = report.size_bytes,
            "history is over {cap_mb} MB even with only the last day kept"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use kelvo_store::{CapTrim, Store, StoreConfig};

    use super::*;

    const DAY: i64 = Retention::DAY_MS;
    const NOW: i64 = 1_800_000_000_000;

    fn history(days: u16, mb: u32) -> HistorySettings {
        HistorySettings {
            retention_days: days,
            size_limit_mb: mb,
            network_history: true,
        }
    }

    fn trimmed(earliest_ts_ms: i64, cap_met: bool) -> PruneReport {
        PruneReport {
            cap_trim: Some(CapTrim {
                earliest_ts_ms,
                cap_met,
                ..CapTrim::default()
            }),
            ..PruneReport::default()
        }
    }

    #[test]
    fn retention_follows_days_and_size_limit() {
        for mb in HistorySettings::SIZE_LIMITS_MB {
            let r = retention_for(&history(90, mb));
            assert_eq!(r.max_bytes, u64::from(mb) * 1_000_000);
            assert_eq!(r.history_ms, 90 * DAY);
            assert_eq!(r.m1_ms, Retention::M1_WINDOW_MS);
            // Only the cap and the history retention come from settings.
            assert_eq!(r.s10_ms, Retention::default().s10_ms);
            assert_eq!(r.proc_snap_ms, Retention::default().proc_snap_ms);
        }
        assert_eq!(
            retention_for(&kelvo_schema::Settings::default().history),
            Retention::default(),
            "the default setting is D-057's default cap"
        );
    }

    #[test]
    fn a_trim_records_where_history_starts_and_the_limit() {
        let r = retention_for(&history(90, 300));
        let h = next_health(
            HistoryHealth::default(),
            Some(&trimmed(NOW - 40 * DAY, true)),
            false,
            NOW,
            &r,
        );
        assert_eq!(h.trimmed_before_ms, Some(NOW - 40 * DAY));
        assert_eq!(h.trimmed_limit_bytes, Some(300_000_000));
        assert!(h.cap_met);

        let h = next_health(h, Some(&trimmed(NOW - DAY, false)), false, NOW, &r);
        assert!(!h.cap_met, "the last day does not fit");
    }

    #[test]
    fn the_trim_note_stays_until_retention_passes_it() {
        let r = retention_for(&history(30, 150));
        let start = HistoryHealth {
            trimmed_before_ms: Some(NOW - 20 * DAY),
            trimmed_limit_bytes: Some(150_000_000),
            ..HistoryHealth::default()
        };
        // A later prune with no trim: history still starts at the trim, which is newer
        // than the retention cutoff, so the limit is still what shortened it.
        let pruned = PruneReport::default();
        assert_eq!(next_health(start, Some(&pruned), false, NOW, &r), start);
        // A round without a prune (free-space check only) never clears it.
        let later = NOW + 15 * DAY;
        assert_eq!(next_health(start, None, false, later, &r), start);
        // Once retention alone would have dropped it, the note goes.
        let h = next_health(start, Some(&pruned), false, later, &r);
        assert_eq!(h, HistoryHealth::default());
    }

    #[test]
    fn low_disk_pause_follows_each_check() {
        let r = Retention::default();
        let h = next_health(HistoryHealth::default(), None, true, NOW, &r);
        assert!(h.low_disk_paused);
        assert_ne!(h, HistoryHealth::default(), "a change to emit");
        let h = next_health(h, None, false, NOW, &r);
        assert_eq!(h, HistoryHealth::default());
    }

    #[test]
    fn clearing_forgets_the_trim_and_keeps_the_pause() {
        let h = HistoryHealth {
            low_disk_paused: true,
            trimmed_before_ms: Some(NOW),
            trimmed_limit_bytes: Some(150_000_000),
            cap_met: false,
        };
        let cleared = h.without_trim();
        assert_eq!(cleared.trimmed_before_ms, None);
        assert_eq!(cleared.trimmed_limit_bytes, None);
        assert!(cleared.cap_met && cleared.low_disk_paused);
    }

    fn wait_until(what: &str, f: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `prune_now` runs a round right away instead of waiting for the schedule, a round
    /// with no store open is skipped, and dropping the handle ends the thread.
    #[test]
    fn prune_now_runs_a_round_and_dropping_the_handle_ends_the_thread() {
        let dir = std::env::temp_dir().join(format!(
            "kelvo-engine-housekeeping-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.sqlite");
        let store = Store::open(StoreConfig::new(&path)).unwrap();
        let writer = store.writer();
        let open = Arc::new(Mutex::new(false));
        let rounds = Arc::new(AtomicU32::new(0));
        let asked = Arc::new(AtomicU32::new(0));
        let hour = Duration::from_secs(3_600);
        let handle = Housekeeping {
            path,
            schedule: Schedule {
                first_prune_after: hour,
                prune_every: hour,
                disk_check_every: hour,
            },
            writer: {
                let (open, asked) = (Arc::clone(&open), Arc::clone(&asked));
                move || {
                    asked.fetch_add(1, Ordering::SeqCst);
                    (*open.lock().unwrap()).then(|| writer.clone())
                }
            },
            retention: {
                let rounds = Arc::clone(&rounds);
                move || {
                    rounds.fetch_add(1, Ordering::SeqCst);
                    Retention::default()
                }
            },
            health: Arc::default(),
            on_change: |_| {},
        }
        .spawn()
        .unwrap();

        handle.prune_now();
        wait_until("the skipped round", || asked.load(Ordering::SeqCst) == 1);
        assert_eq!(rounds.load(Ordering::SeqCst), 0, "no store, no round");

        *open.lock().unwrap() = true;
        handle.prune_now();
        wait_until("the round", || rounds.load(Ordering::SeqCst) == 1);

        drop(handle);
        // The thread holds the writer closure; once it ends, `asked` is its only owner
        // besides this test.
        wait_until("the thread to end", || Arc::strong_count(&asked) == 1);
        store.close().unwrap();
    }
}
