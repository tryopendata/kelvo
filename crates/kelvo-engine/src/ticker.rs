//! The engine's one timer (architecture.md infra 6). The engine never calls a timer API
//! directly: it starts and stops a [`Ticker`], which delivers ticks into its [`Inbox`].
//!
//! - macOS: [`crate::macos::GcdTicker`], a GCD `DispatchSource` timer on a utility-QoS
//!   queue with 10% leeway so the kernel can coalesce wakeups.
//! - Elsewhere: [`ThreadTicker`], a sleeping thread (the Linux `timerfd` ticker is v4).
//! - Tests: [`FakeTicker`], driven by hand through its [`FakeClock`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use kelvo_schema::lock::LockExt;

use crate::clock::{self, ClockReading};
use crate::inbox::Inbox;

/// A periodic timer that delivers ticks to an [`Inbox`].
pub trait Ticker: Send + 'static {
    /// Starts (or restarts after [`Ticker::stop`]) ticking every `period`, with up to
    /// `leeway` of allowed lateness per tick. Restarting with a new period is how the
    /// engine reconfigures the interval. Implementations align the first tick to a
    /// wall-clock multiple of `period` where they can.
    fn start(&mut self, inbox: Inbox, period: Duration, leeway: Duration);
    /// Stops ticking. No tick is delivered after this returns.
    fn stop(&mut self);
    /// The current reading of both clocks, from the same source the ticks use.
    fn now(&self) -> ClockReading;
}

/// The leeway the engine asks for: 10% of the period (architecture.md, Engine).
pub fn leeway_for(period: Duration) -> Duration {
    period / 10
}

/// Delay from `now_ms` to the next wall-clock multiple of `period_ms`.
pub(crate) fn delay_to_boundary(now_ms: i64, period_ms: i64) -> i64 {
    if period_ms <= 0 {
        return 0;
    }
    let rem = now_ms.rem_euclid(period_ms);
    if rem == 0 { 0 } else { period_ms - rem }
}

/// A portable ticker: one thread sleeping until each wall-clock-aligned deadline. Used
/// where no platform timer is wired up (Linux until v4).
#[derive(Default)]
pub struct ThreadTicker {
    running: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

impl ThreadTicker {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Ticker for ThreadTicker {
    fn start(&mut self, inbox: Inbox, period: Duration, _leeway: Duration) {
        self.stop();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let period_ms = i64::try_from(period.as_millis()).unwrap_or(1_000).max(1);
        let spawned = std::thread::Builder::new()
            .name("kelvo-ticker".into())
            .spawn(move || {
                while !flag.load(Ordering::Acquire) {
                    let wait = delay_to_boundary(clock::wall_ms(), period_ms);
                    let wait = if wait == 0 { period_ms } else { wait };
                    std::thread::park_timeout(Duration::from_millis(wait.unsigned_abs()));
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    inbox.tick(clock::now());
                }
            });
        match spawned {
            Ok(handle) => self.running = Some((stop, handle)),
            Err(e) => tracing::error!("cannot start ticker thread: {e}"),
        }
    }

    fn stop(&mut self) {
        if let Some((stop, handle)) = self.running.take() {
            stop.store(true, Ordering::Release);
            handle.thread().unpark();
            let _ = handle.join();
        }
    }

    fn now(&self) -> ClockReading {
        clock::now()
    }
}

impl Drop for ThreadTicker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct FakeState {
    now: ClockReading,
    running: Option<(Inbox, Duration)>,
    started: u32,
    stopped: u32,
}

impl Default for ClockReading {
    fn default() -> Self {
        // 2026-10-05T00:00:00Z, so bucket math runs on realistic epochs.
        ClockReading {
            wall_ms: 1_791_158_400_000,
            continuous_ns: 1_000_000_000_000,
        }
    }
}

/// A ticker that only ticks when told to, for tests. Hand the [`FakeTicker`] to the
/// engine and keep the [`FakeClock`].
///
/// It reproduces the production semantics that matter for gaps and buckets: a stopped
/// ticker delivers nothing, both clocks advance during a simulated sleep, a skipped tick
/// advances time without a delivery, and the wall clock can jump on its own.
pub struct FakeTicker {
    state: Arc<Mutex<FakeState>>,
}

/// The test's handle on a [`FakeTicker`].
#[derive(Clone)]
pub struct FakeClock {
    state: Arc<Mutex<FakeState>>,
}

impl FakeTicker {
    /// A stopped fake ticker at 2026-10-05T00:00:00Z.
    pub fn new() -> (FakeTicker, FakeClock) {
        Self::at(ClockReading::default())
    }

    pub fn at(start: ClockReading) -> (FakeTicker, FakeClock) {
        let state = Arc::new(Mutex::new(FakeState {
            now: start,
            ..FakeState::default()
        }));
        (
            FakeTicker {
                state: Arc::clone(&state),
            },
            FakeClock { state },
        )
    }
}

impl Ticker for FakeTicker {
    fn start(&mut self, inbox: Inbox, period: Duration, _leeway: Duration) {
        let mut s = self.state.lock_ok();
        s.running = Some((inbox, period));
        s.started += 1;
    }

    fn stop(&mut self) {
        let mut s = self.state.lock_ok();
        if s.running.take().is_some() {
            s.stopped += 1;
        }
    }

    fn now(&self) -> ClockReading {
        self.state.lock_ok().now
    }
}

fn advance(now: &mut ClockReading, by: Duration) {
    now.wall_ms += i64::try_from(by.as_millis()).unwrap_or(i64::MAX);
    now.continuous_ns += u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
}

impl FakeClock {
    /// Advances both clocks by one period and delivers a tick, if the ticker is running.
    /// Returns whether a tick was delivered.
    pub fn tick(&self) -> bool {
        let mut s = self.state.lock_ok();
        let Some((inbox, period)) = s.running.clone() else {
            return false;
        };
        advance(&mut s.now, period);
        inbox.tick(s.now)
    }

    /// Advances both clocks by one period without a tick: a tick the timer missed.
    pub fn skip(&self) {
        let mut s = self.state.lock_ok();
        if let Some((_, period)) = s.running.clone() {
            advance(&mut s.now, period);
        }
    }

    /// Advances both clocks by `by` (a sleep, or time passing while stopped).
    pub fn advance(&self, by: Duration) {
        advance(&mut self.state.lock_ok().now, by);
    }

    /// Moves only the wall clock (an NTP step, a manual clock change).
    pub fn jump_wall(&self, by_ms: i64) {
        self.state.lock_ok().now.wall_ms += by_ms;
    }

    /// Sets the wall clock so the next tick lands exactly on `wall_ms`.
    pub fn set_next_tick_wall(&self, wall_ms: i64) {
        let mut s = self.state.lock_ok();
        let period_ms = s
            .running
            .as_ref()
            .map(|(_, p)| i64::try_from(p.as_millis()).unwrap_or(1_000))
            .unwrap_or(1_000);
        s.now.wall_ms = wall_ms - period_ms;
    }

    pub fn now(&self) -> ClockReading {
        self.state.lock_ok().now
    }

    pub fn is_running(&self) -> bool {
        self.state.lock_ok().running.is_some()
    }

    /// The period the ticker was last started with, if running.
    pub fn period(&self) -> Option<Duration> {
        self.state.lock_ok().running.as_ref().map(|(_, p)| *p)
    }

    /// How many times the ticker was started and stopped.
    pub fn starts_stops(&self) -> (u32, u32) {
        let s = self.state.lock_ok();
        (s.started, s.stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_delay() {
        assert_eq!(delay_to_boundary(10_000, 1_000), 0);
        assert_eq!(delay_to_boundary(10_250, 1_000), 750);
        assert_eq!(delay_to_boundary(10_250, 2_000), 1_750);
        assert_eq!(delay_to_boundary(-250, 1_000), 250);
    }

    #[test]
    fn fake_ticker_delivers_only_while_running() {
        let (mut t, clock) = FakeTicker::new();
        let (inbox, rx) = Inbox::new();
        assert!(!clock.tick(), "stopped ticker delivers nothing");
        t.start(inbox.clone(), Duration::from_secs(1), Duration::ZERO);
        let before = clock.now();
        assert!(clock.tick());
        assert_eq!(clock.now().wall_ms - before.wall_ms, 1_000);
        assert!(!clock.tick(), "previous tick still pending: skipped");
        assert_eq!(rx.len(), 1);
        t.stop();
        assert!(!clock.is_running());
    }

    #[test]
    fn thread_ticker_ticks_and_stops() {
        let mut t = ThreadTicker::new();
        let (inbox, rx) = Inbox::new();
        t.start(inbox.clone(), Duration::from_millis(20), Duration::ZERO);
        let first = rx.recv_timeout(Duration::from_secs(2));
        assert!(first.is_ok());
        t.stop();
        inbox.tick_taken();
        while rx.try_recv().is_ok() {}
        std::thread::sleep(Duration::from_millis(60));
        assert!(rx.try_recv().is_err(), "no tick after stop");
    }
}
