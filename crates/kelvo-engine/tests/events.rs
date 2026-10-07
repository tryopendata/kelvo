//! Detectors and alert rules inside the engine (v1.2): an event is stored, readable
//! before any batch commit (D-083), and published on the bus; alert rules follow
//! settings.

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use kelvo_collect::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};
use kelvo_engine::BusMsg;
use kelvo_schema::{
    AlertCause, Entitlement, Event, EventDetail, Module, SeriesKey, Settings, ThermalState,
};

/// Emits `thermal.state` with whatever value the test set, every tick.
struct Thermal(Arc<Mutex<f32>>, SeriesKey);

impl Collector for Thermal {
    fn id(&self) -> CollectorId {
        CollectorId("thermal_script")
    }
    fn cadence(&self) -> Cadence {
        Cadence::EveryTick
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }
    fn modules(&self) -> &'static [Module] {
        &[Module::Sensors]
    }
    fn probe(&mut self) -> Probe {
        Probe::Supported(vec![self.1.clone()])
    }
    fn sample(&mut self, _: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        out.push(&self.1, *self.0.lock().unwrap());
        Ok(())
    }
}

fn harness(name: &str, settings: &Settings) -> (Harness, Arc<Mutex<f32>>) {
    let level = Arc::new(Mutex::new(0.0));
    let c = Thermal(Arc::clone(&level), key("thermal.state"));
    (Harness::new(name, vec![Box::new(c)], settings), level)
}

fn bus_events(h: &mut Harness) -> Vec<Arc<Event>> {
    h.drain()
        .into_iter()
        .filter_map(|m| match m {
            BusMsg::Event(e) => Some(e),
            _ => None,
        })
        .collect()
}

/// Reads events without flushing, until `n` are there or a few seconds pass: the
/// engine's commit is queued, not awaited.
fn committed_events(h: &Harness, n: usize) -> Vec<Event> {
    let store = h.store.as_ref().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let got = store.reader().unwrap().events(h.host, 0, i64::MAX).unwrap();
        if got.len() >= n || Instant::now() > deadline {
            return got;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_event_is_committed_and_published_at_once() {
    let mut settings = settings_all_on();
    settings.alerts.thermal_serious = true;
    let (mut h, level) = harness("events-commit", &settings);
    h.ticks(30);
    *level.lock().unwrap() = 2.0;
    h.ticks(15);

    let bus = bus_events(&mut h);
    let kinds: Vec<&str> = bus.iter().map(|e| e.detail.kind()).collect();
    // The alert has no hold (for 0 s); the level change holds 10 s first.
    assert_eq!(kinds, ["alert", "thermal_state"], "{bus:?}");
    assert!(matches!(
        &bus[0].detail,
        EventDetail::Alert {
            cause: AlertCause::ThermalState {
                state: ThermalState::Serious
            },
            ..
        }
    ));

    // The store commits only on flush here (an hour's interval): the events are
    // readable anyway, because the engine asked for a commit.
    let stored = committed_events(&h, 2);
    assert_eq!(
        stored,
        bus.iter().map(|e| (**e).clone()).collect::<Vec<_>>(),
        "stored and published events match"
    );
}

/// Alerts are off by default, and switching one on takes effect without a restart.
#[test]
fn alert_rules_follow_settings() {
    let settings = settings_all_on();
    let (mut h, level) = harness("events-settings", &settings);
    *level.lock().unwrap() = 3.0;
    h.ticks(20);
    assert!(bus_events(&mut h).is_empty(), "no alert while off");

    let mut on = settings.clone();
    on.alerts.thermal_serious = true;
    h.ctl.apply_settings(&on);
    h.pump();
    h.ticks(2);
    let kinds: Vec<&str> = bus_events(&mut h).iter().map(|e| e.detail.kind()).collect();
    assert_eq!(kinds, ["alert"]);
}

/// After sleep the detectors start over: a level seen before sleeping is not compared
/// with one after waking.
#[test]
fn sleep_resets_the_detectors() {
    let (mut h, level) = harness("events-sleep", &settings_all_on());
    *level.lock().unwrap() = 0.0;
    h.ticks(20);
    let _ack = h.power.will_sleep(h.clock.now());
    h.pump();
    h.clock.advance(Duration::from_secs(600));
    h.power.did_wake(h.clock.now());
    h.pump();
    *level.lock().unwrap() = 1.0;
    h.ticks(30);
    assert!(bus_events(&mut h).is_empty());
}

/// Emits `power.package` like IOReport: every 10 s with only the tray open, every tick
/// while a window shows detail.
struct Power(Arc<Mutex<f32>>, SeriesKey);

impl Collector for Power {
    fn id(&self) -> CollectorId {
        CollectorId("power_script")
    }
    fn cadence(&self) -> Cadence {
        Cadence::Adaptive {
            idle_ms: 10_000,
            interest: kelvo_collect::Interest::Detail,
        }
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }
    fn modules(&self) -> &'static [Module] {
        &[Module::Power]
    }
    fn probe(&mut self) -> Probe {
        Probe::Supported(vec![self.1.clone()])
    }
    fn sample(&mut self, _: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        out.push(&self.1, *self.0.lock().unwrap());
        Ok(())
    }
}

/// With no window open the detectors still see power spikes (D-083): readings 10 s
/// apart are the collector's period, not holes. Opening a window changes the spacing
/// to every tick without restarting the warm-up.
#[test]
fn power_spike_fires_tray_only_and_after_a_window_opens() {
    let watts = Arc::new(Mutex::new(4.0));
    let c = Power(Arc::clone(&watts), key("power.package"));
    let mut h = Harness::new("events-power", vec![Box::new(c)], &settings_all_on());
    let spikes = |h: &mut Harness| -> usize {
        bus_events(h)
            .iter()
            .filter(|e| e.detail.kind() == "power_spike")
            .count()
    };
    h.ticks(300);
    *watts.lock().unwrap() = 20.0;
    h.ticks(120);
    assert_eq!(spikes(&mut h), 1, "tray-only plateau");

    *watts.lock().unwrap() = 4.0;
    h.ticks(600);
    h.ctl.set_detail_interest(true);
    h.ticks(5);
    *watts.lock().unwrap() = 20.0;
    h.ticks(30);
    assert_eq!(spikes(&mut h), 1, "after a window opened");
}
