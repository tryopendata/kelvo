//! Power state the engine reacts to (architecture.md infra 6). The engine never calls a
//! power API directly.
//!
//! Two channels, because the signals behave differently:
//! - System sleep and wake are events that must be handled before the machine sleeps.
//!   They arrive through the [`Inbox`] as [`PowerEvent`]s, `WillSleep` with a
//!   [`SleepAck`].
//! - On battery, Low Power Mode, display sleep and screen lock are states. The engine
//!   reads them once per tick through [`PowerSignals::poll`], and implementations keep
//!   that cheap (change notifications or rate-limited reads).

use std::sync::{Arc, Mutex};

use kelvo_schema::lock::LockExt;

use crate::clock::ClockReading;
use crate::inbox::{Inbox, SleepAck};

/// States the engine reads each tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerState {
    pub on_battery: bool,
    /// The internal battery reports charging, read with `on_battery` on the same
    /// power-source change (D-092). Always false without a battery.
    pub charging: bool,
    pub low_power_mode: bool,
    pub display_asleep: bool,
    pub screen_locked: bool,
}

/// System sleep transitions, delivered through the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEvent {
    WillSleep,
    DidWake,
}

pub trait PowerSignals: Send + 'static {
    /// Starts delivering sleep and wake events to `inbox`. Called once.
    fn start(&mut self, inbox: Inbox);
    /// The current state. Called once per tick, so it must be cheap.
    fn poll(&mut self) -> PowerState;
}

/// A host with no power signals: always on AC, never sleeps (Linux until v4).
#[derive(Default)]
pub struct NoPowerSignals;

impl PowerSignals for NoPowerSignals {
    fn start(&mut self, _inbox: Inbox) {}

    fn poll(&mut self) -> PowerState {
        PowerState::default()
    }
}

#[derive(Default)]
struct FakePowerState {
    state: PowerState,
    inbox: Option<Inbox>,
}

/// Power signals driven by a test through [`FakePower`].
pub struct FakePowerSignals {
    shared: Arc<Mutex<FakePowerState>>,
}

/// The test's handle on [`FakePowerSignals`].
#[derive(Clone)]
pub struct FakePower {
    shared: Arc<Mutex<FakePowerState>>,
}

impl FakePowerSignals {
    pub fn new() -> (FakePowerSignals, FakePower) {
        let shared = Arc::new(Mutex::new(FakePowerState::default()));
        (
            FakePowerSignals {
                shared: Arc::clone(&shared),
            },
            FakePower { shared },
        )
    }
}

impl PowerSignals for FakePowerSignals {
    fn start(&mut self, inbox: Inbox) {
        self.shared.lock_ok().inbox = Some(inbox);
    }

    fn poll(&mut self) -> PowerState {
        self.shared.lock_ok().state
    }
}

impl FakePower {
    pub fn set(&self, f: impl FnOnce(&mut PowerState)) {
        f(&mut self.shared.lock_ok().state);
    }

    /// Sends `WillSleep` stamped `at`. Returns the receiver the engine's ack arrives on
    /// (or disconnects, if the engine dropped it).
    pub fn will_sleep(&self, at: ClockReading) -> crossbeam_channel::Receiver<()> {
        let (ack, rx) = SleepAck::new();
        if let Some(inbox) = &self.shared.lock_ok().inbox {
            inbox.power(PowerEvent::WillSleep, at, Some(ack));
        }
        rx
    }

    pub fn did_wake(&self, at: ClockReading) {
        if let Some(inbox) = &self.shared.lock_ok().inbox {
            inbox.power(PowerEvent::DidWake, at, None);
        }
    }
}
