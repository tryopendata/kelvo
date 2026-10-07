//! The engine's single input queue. Ticks, power events, device hints and commands all
//! arrive here, so the engine thread handles them one at a time and in order.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender};
use kelvo_collect::Tick;
use kelvo_schema::{Module, Settings};
use kelvo_store::Writer;

use crate::clock::ClockReading;
use crate::power::PowerEvent;

/// Sent with [`PowerEvent::WillSleep`]. The engine answers once it has flushed and
/// stopped; the platform waits for that (bounded) before letting the system sleep.
/// Dropping it counts as the answer. A store swap is acknowledged the same way.
#[derive(Debug)]
pub struct SleepAck(Sender<()>);

impl SleepAck {
    /// A new ack and the receiver the platform waits on.
    pub fn new() -> (SleepAck, Receiver<()>) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        (SleepAck(tx), rx)
    }

    pub(crate) fn done(self) {
        let _ = self.0.try_send(());
    }
}

pub(crate) enum Command {
    Pause(bool),
    Settings(Box<Settings>),
    /// Detach from the store (`None`) or attach to a new one; acknowledged once done.
    SetStore(Option<Writer>, SleepAck),
    /// Detail interest crossed between zero and one (D-094): re-choose the base tick;
    /// acknowledged once the status is published.
    Detail(SleepAck),
    Shutdown,
}

pub(crate) enum Input {
    Tick(Tick),
    Power {
        event: PowerEvent,
        at: ClockReading,
        ack: Option<SleepAck>,
    },
    /// Devices of these modules appeared or went away: re-probe their collectors.
    Hint(Vec<Module>),
    Cmd(Command),
}

/// A cloneable sender into the engine. Tickers, power signals and device watchers hold
/// one; it never blocks.
#[derive(Clone)]
pub struct Inbox {
    tx: Sender<Input>,
    /// Set while a tick sits in the queue unprocessed. A tick that arrives while one is
    /// still pending is skipped, as a coalesced timer would skip it, so a slow tick never
    /// builds a backlog.
    tick_pending: Arc<AtomicBool>,
    /// Ticks delivered so far; the next tick's `n`. Skipped ticks do not count.
    next_n: Arc<AtomicU64>,
}

impl Inbox {
    pub(crate) fn new() -> (Inbox, Receiver<Input>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            Inbox {
                tx,
                tick_pending: Arc::new(AtomicBool::new(false)),
                next_n: Arc::new(AtomicU64::new(0)),
            },
            rx,
        )
    }

    /// Delivers a tick taken at `at`. Returns `false` when it was skipped because the
    /// previous tick is still unprocessed, or the engine is gone.
    pub fn tick(&self, at: ClockReading) -> bool {
        if self.tick_pending.swap(true, Ordering::AcqRel) {
            return false;
        }
        let n = self.next_n.fetch_add(1, Ordering::AcqRel);
        let tick = Tick {
            n,
            wall_ms: at.wall_ms,
            continuous_ns: at.continuous_ns,
            // The engine fills in the base tick in effect when it takes the tick.
            interval_ms: 0,
        };
        if self.tx.send(Input::Tick(tick)).is_err() {
            self.tick_pending.store(false, Ordering::Release);
            return false;
        }
        true
    }

    /// A delivered tick is waiting in the queue.
    pub(crate) fn tick_pending(&self) -> bool {
        self.tick_pending.load(Ordering::Acquire)
    }

    /// A tick the engine takes itself at `at`, outside the ticker's schedule, numbered
    /// like a delivered one.
    pub(crate) fn extra_tick(&self, at: ClockReading) -> Tick {
        Tick {
            n: self.next_n.fetch_add(1, Ordering::AcqRel),
            wall_ms: at.wall_ms,
            continuous_ns: at.continuous_ns,
            interval_ms: 0,
        }
    }

    /// Delivers a power event observed at `at`.
    pub fn power(&self, event: PowerEvent, at: ClockReading, ack: Option<SleepAck>) {
        let _ = self.tx.send(Input::Power { event, at, ack });
    }

    /// Asks the engine to re-probe the collectors of `modules` on its next tick.
    pub fn hint(&self, modules: &[Module]) {
        let _ = self.tx.send(Input::Hint(modules.to_vec()));
    }

    pub(crate) fn command(&self, cmd: Command) {
        let _ = self.tx.send(Input::Cmd(cmd));
    }

    /// Called by the engine when it takes a tick off the queue.
    pub(crate) fn tick_taken(&self) {
        self.tick_pending.store(false, Ordering::Release);
    }
}
