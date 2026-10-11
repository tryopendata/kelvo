use kelvo_schema::lock::LockExt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Source wall time advances using controller monotonic time, even while offline.
#[derive(Clone, Default)]
pub struct HostClock(Arc<Mutex<Option<(i64, Instant)>>>);
impl HostClock {
    pub fn observe(&self, remote_ms: i64) {
        *self.0.lock_ok() = Some((remote_ms, Instant::now()));
    }
    pub fn now_ms(&self) -> Option<i64> {
        self.0.lock_ok().as_ref().map(|(t, received)| {
            t.saturating_add(received.elapsed().as_millis().min(i64::MAX as u128) as i64)
        })
    }
    pub fn age_ms(&self) -> Option<u64> {
        self.0
            .lock_ok()
            .as_ref()
            .map(|(_, t)| t.elapsed().as_millis().min(u64::MAX as u128) as u64)
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
}
