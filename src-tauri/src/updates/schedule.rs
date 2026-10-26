//! Wall-clock deadlines catch sleep/wake without replaying missed checks.
use super::Updates;
use crate::AppState;
use kelvo_schema::lock::LockExt;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const POLL: Duration = Duration::from_secs(60);

struct Schedule {
    next: SystemTime,
    jitter: Duration,
}
impl Schedule {
    fn new(now: SystemTime, jitter: Duration) -> Self {
        Self {
            next: now + Duration::from_secs(30),
            jitter,
        }
    }
    fn take_due(&mut self, now: SystemTime) -> bool {
        if now < self.next {
            return false;
        }
        self.next = now + DAY + self.jitter;
        true
    }
}

pub(super) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Stable per-host jitter prevents every login/reboot synchronizing daily checks.
        let host = app.state::<AppState>().local.to_string();
        let seed = host.bytes().fold(0_u64, |seed, byte| {
            seed.wrapping_mul(31).wrapping_add(u64::from(byte))
        });
        let mut schedule = Schedule::new(SystemTime::now(), Duration::from_secs(seed % 1801));
        tokio::time::sleep(Duration::from_secs(30)).await;
        let state = app.state::<Updates>();
        if let Err(error) = state.restore(&app).await {
            tracing::info!(%error, "ignoring invalid cached update");
        }
        loop {
            if let Ok(_operation) = state.operation.try_lock() {
                let settings = app.state::<AppState>().settings.settings();
                // Advance the deadline even while opted out, so polling never requests
                // repeatedly after an error or an extended machine sleep.
                let previous = state.status();
                if schedule.take_due(SystemTime::now())
                    && settings.general.check_updates
                    && settings.onboarding.completed
                    && let Err(error) = state.check(&app).await
                {
                    tracing::info!(%error, "automatic update check");
                    // A network failure must not strand an already authenticated
                    // package. An incompatible/invalid target clears the candidate.
                    if previous.has_download() && state.candidate.lock_ok().is_some() {
                        state.set(&app, previous);
                    }
                }
                if let Err(error) = state.advance(&app).await {
                    tracing::info!(%error, "automatic update installation");
                }
            }
            tokio::time::sleep(POLL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_after_startup_then_once_after_a_long_sleep() {
        let start = std::time::UNIX_EPOCH + Duration::from_secs(1000);
        let jitter = Duration::from_secs(900);
        let mut schedule = Schedule::new(start, jitter);
        assert!(!schedule.take_due(start + Duration::from_secs(29)));
        assert!(schedule.take_due(start + Duration::from_secs(30)));
        assert!(!schedule.take_due(start + DAY));
        assert!(schedule.take_due(start + DAY * 10));
        assert!(!schedule.take_due(start + DAY * 10 + POLL));
        assert!(!schedule.take_due(start + DAY * 11));
        assert!(schedule.take_due(start + DAY * 11 + jitter));
    }
}
