use kelvo_schema::lock::LockExt;
use std::sync::{Arc, Mutex};

/// Source wall time advances using a controller monotonic clock that keeps counting
/// while the controller sleeps, so it stays right even while the host is offline.
#[derive(Clone, Default)]
pub struct HostClock(Arc<Mutex<Option<(i64, u64)>>>);
impl HostClock {
    pub fn observe(&self, remote_ms: i64) {
        self.observe_at(remote_ms, kelvo_engine::continuous_ns());
    }
    pub fn now_ms(&self) -> Option<i64> {
        self.now_at(kelvo_engine::continuous_ns())
    }
    pub fn age_ms(&self) -> Option<u64> {
        self.age_at(kelvo_engine::continuous_ns())
    }
    fn observe_at(&self, remote_ms: i64, now_ns: u64) {
        *self.0.lock_ok() = Some((remote_ms, now_ns));
    }
    fn now_at(&self, now_ns: u64) -> Option<i64> {
        let elapsed = i64::try_from(self.age_at(now_ns)?).unwrap_or(i64::MAX);
        self.0
            .lock_ok()
            .as_ref()
            .map(|(t, _)| t.saturating_add(elapsed))
    }
    fn age_at(&self, now_ns: u64) -> Option<u64> {
        self.0
            .lock_ok()
            .as_ref()
            .map(|(_, received)| now_ns.saturating_sub(*received) / 1_000_000)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_time_is_independent_of_controller_wall_clock() {
        let clock = HostClock::default();
        assert_eq!(clock.now_ms(), None);
        clock.observe(-1_000_000);
        assert!((-1_000_000..-999_900).contains(&clock.now_ms().unwrap()));
        clock.observe(9_000_000);
        assert!((9_000_000..9_000_100).contains(&clock.now_ms().unwrap()));
    }
    /// An hour asleep counts: the host's history "now" after a wake is an hour on.
    #[test]
    fn source_time_advances_across_controller_sleep() {
        let clock = HostClock::default();
        clock.observe_at(9_000_000, 5_000_000_000);
        let hour_ns = 3_600 * 1_000_000_000;
        assert_eq!(
            clock.now_at(5_000_000_000 + hour_ns),
            Some(9_000_000 + 3_600_000)
        );
        assert_eq!(clock.age_at(5_000_000_000 + hour_ns), Some(3_600_000));
    }
    /// The clock the session uses is the one that counts sleep.
    #[test]
    fn source_time_uses_the_continuous_clock() {
        let clock = HostClock::default();
        clock.observe(9_000_000);
        let later = kelvo_engine::continuous_ns() + 2_000_000_000;
        assert!((9_002_000..9_002_100).contains(&clock.now_at(later).unwrap()));
    }
}
