//! How often the engine samples a collector: [`Cadence`], the wall-clock [`Every`]
//! period and the idle period of collectors nobody shows live.

use crate::{Interest, Interests, Tick};

/// How often the engine samples a collector. Periods are wall-clock minimums measured on
/// the continuous clock, not tick multiples, so they mean the same at every base tick: a
/// collector is sampled at most once per tick, and no more often than its period. When
/// the period is shorter than the base tick it is sampled every tick (D-061).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    EveryTick,
    /// At least this many ms apart (and at most once per tick).
    Every(u32),
    /// Sampled only while someone has this interest (per-process network, v1.2). When
    /// the interest ends the engine calls [`Collector::release`].
    ///
    /// [`Collector::release`]: crate::Collector::release
    OnDemand(Interest),
    /// Every tick while someone has `interest`, otherwise at least `idle_ms` apart
    /// (processes: 10 s idle; IOReport: 10 s in tray-only mode; the per-tick collectors
    /// on [`Interest::Live`]: [`IDLE_MS`] when neither a window nor the menu bar shows
    /// them).
    Adaptive {
        idle_ms: u32,
        interest: Interest,
    },
}

/// The idle period of a collector nobody shows live (D-067): one sample per 10 s history
/// bucket. Counter-derived rates (CPU load, network and disk bytes) cover the whole span
/// since the previous sample, so their S10 and M1 averages stay exact. Gauges (memory,
/// GPU utilization, SMC watts) would get one point per bucket, so they stay on
/// [`Cadence::EveryTick`]; only counter-derived collectors use this (D-070).
pub const IDLE_MS: u32 = 10_000;

/// The cadence of a collector that runs every tick while shown and at [`IDLE_MS`]
/// otherwise.
pub const LIVE_OR_IDLE: Cadence = Cadence::Adaptive {
    idle_ms: IDLE_MS,
    interest: Interest::Live,
};

impl Cadence {
    /// The minimum period right now: `Some(0)` for every tick, `None` for not at all.
    pub fn period_ms(self, interests: Interests) -> Option<u32> {
        match self {
            Cadence::EveryTick => Some(0),
            Cadence::Every(ms) => Some(ms),
            Cadence::OnDemand(i) => interests.has(i).then_some(0),
            Cadence::Adaptive { idle_ms, interest } => {
                Some(if interests.has(interest) { 0 } else { idle_ms })
            }
        }
    }
}

/// Whether `elapsed_ns` since the last sample covers a minimum period of `period_ms` at
/// a base tick of `interval_ms`. Consecutive ticks can be closer than the interval by
/// the timer's leeway (10% of the interval), so a quarter of the smaller of the two is
/// forgiven: at 1 s a 5 s period is due from 4.75 s, at 2 s three ticks after the last
/// sample (6 s), at 30 s on every tick.
pub fn period_elapsed(elapsed_ns: u64, period_ms: u32, interval_ms: u32) -> bool {
    let slack_ms = period_ms.min(interval_ms) / 4;
    let need_ns = u64::from(period_ms.saturating_sub(slack_ms)) * 1_000_000;
    elapsed_ns >= need_ns
}

/// A wall-clock minimum period: the engine keeps one per collector, and a collector
/// keeps one for series it reads less often than it runs (`cpu.loadavg` every 5 s,
/// `fan.max` every 60 s). [`Every::due`] is true on the first call and then once the
/// period has passed on the continuous clock, whatever the base tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Every {
    period_ms: u32,
    last_ns: Option<u64>,
}

impl Every {
    pub const fn new(period_ms: u32) -> Self {
        Self {
            period_ms,
            last_ns: None,
        }
    }

    /// Whether the period has passed at `tick`; if so, `tick` becomes the last sample.
    pub fn due(&mut self, tick: &Tick) -> bool {
        self.due_with(self.period_ms, tick)
    }

    /// [`Every::due`] with a period that varies from call to call (adaptive cadences).
    pub fn due_with(&mut self, period_ms: u32, tick: &Tick) -> bool {
        let due = match self.last_ns {
            None => true,
            Some(last) => period_elapsed(
                tick.continuous_ns.saturating_sub(last),
                period_ms,
                tick.interval_ms,
            ),
        };
        if due {
            self.last_ns = Some(tick.continuous_ns);
        }
        due
    }

    /// Forgets the last sample, so the next call is due (after a probe or a wake).
    pub fn reset(&mut self) {
        self.last_ns = None;
    }

    pub fn period_ms(&self) -> u32 {
        self.period_ms
    }
}
