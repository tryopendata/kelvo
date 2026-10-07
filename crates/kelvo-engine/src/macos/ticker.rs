//! The engine's timer on macOS: a GCD `DispatchSource` timer on a serial utility-QoS
//! queue, with the leeway the engine passes (10%) so the kernel can coalesce wakeups.

use std::ffi::c_void;
use std::time::Duration;

use super::dispatch::{self as d, DispatchObject, Queue};
use crate::clock::{self, ClockReading};
use crate::inbox::Inbox;
use crate::ticker::{Ticker, delay_to_boundary};

struct TimerCtx {
    inbox: Inbox,
}

extern "C" fn on_timer(ctx: *mut c_void) {
    // SAFETY: the context is the `TimerCtx` boxed in `start`. It is freed only after the
    // source is cancelled and the queue drained (`stop`), so it is live here.
    let ctx = unsafe { &*(ctx as *const TimerCtx) };
    ctx.inbox.tick(clock::now());
}

struct Running {
    source: DispatchObject,
    ctx: Box<TimerCtx>,
}

pub struct GcdTicker {
    queue: Option<Queue>,
    running: Option<Running>,
}

// SAFETY: the raw dispatch source is a thread-safe libdispatch object; the boxed context
// is only read by the handler, which `stop` fences off before freeing it.
unsafe impl Send for GcdTicker {}

impl Default for GcdTicker {
    fn default() -> Self {
        Self::new()
    }
}

impl GcdTicker {
    pub fn new() -> Self {
        Self {
            queue: None,
            running: None,
        }
    }
}

fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

impl Ticker for GcdTicker {
    fn start(&mut self, inbox: Inbox, period: Duration, leeway: Duration) {
        self.stop();
        if self.queue.is_none() {
            self.queue = Queue::utility(c"com.tryopendata.kelvo.ticker");
        }
        let Some(queue) = &self.queue else {
            tracing::error!("cannot create the ticker queue");
            return;
        };
        // SAFETY: the timer source type is a libdispatch constant; the queue is valid.
        let source = unsafe {
            d::dispatch_source_create(&raw const d::_dispatch_source_type_timer, 0, 0, queue.raw())
        };
        if source.is_null() {
            tracing::error!("cannot create the ticker timer source");
            return;
        }
        let ctx = Box::new(TimerCtx { inbox });
        let period_ms = i64::try_from(period.as_millis()).unwrap_or(1_000).max(1);
        // First tick on the next wall-clock multiple of the period, so frames and bucket
        // boundaries line up. Later ticks follow the period.
        let mut delay_ms = delay_to_boundary(clock::wall_ms(), period_ms);
        if delay_ms == 0 {
            delay_ms = period_ms;
        }
        // SAFETY: `source` is a valid, suspended timer source. The context pointer stays
        // valid until `stop` cancels the source and drains the queue.
        unsafe {
            d::dispatch_set_context(source, (&raw const *ctx).cast_mut().cast());
            d::dispatch_source_set_event_handler_f(source, on_timer);
            d::dispatch_source_set_timer(
                source,
                d::dispatch_time(d::TIME_NOW, delay_ms.saturating_mul(1_000_000)),
                nanos(period),
                nanos(leeway),
            );
            d::dispatch_resume(source);
        }
        self.running = Some(Running { source, ctx });
    }

    fn stop(&mut self) {
        let Some(Running { source, ctx }) = self.running.take() else {
            return;
        };
        // SAFETY: `source` is the live source created in `start`. Cancelling stops further
        // handler invocations; draining the serial queue waits out one in flight; then the
        // source and its context can go.
        unsafe { d::dispatch_source_cancel(source) };
        if let Some(q) = &self.queue {
            q.drain();
        }
        // SAFETY: we own the reference from dispatch_source_create, and it was resumed.
        unsafe { d::dispatch_release(source) };
        drop(ctx);
    }

    fn now(&self) -> ClockReading {
        clock::now()
    }
}

impl Drop for GcdTicker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_ticker_ticks_on_period_and_stops() {
        let mut t = GcdTicker::new();
        let (inbox, rx) = Inbox::new();
        let period = Duration::from_millis(50);
        t.start(inbox.clone(), period, period / 10);
        let mut stamps = Vec::new();
        for _ in 0..4 {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(crate::inbox::Input::Tick(tick)) => stamps.push(tick),
                _ => panic!("no tick"),
            }
            inbox.tick_taken();
        }
        assert_eq!(
            stamps.iter().map(|s| s.n).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        // The lower bound is the check: ticks never come faster than the period. The upper
        // bound only catches a stalled timer; shared CI runners have stretched three 50 ms
        // periods to 400 ms.
        let span = stamps[3].continuous_ns - stamps[0].continuous_ns;
        assert!(
            (100_000_000..2_000_000_000).contains(&span),
            "three periods of 50 ms, got {span} ns"
        );
        t.stop();
        while rx.try_recv().is_ok() {}
        inbox.tick_taken();
        std::thread::sleep(Duration::from_millis(150));
        assert!(rx.try_recv().is_err(), "no tick after stop");
    }
}
