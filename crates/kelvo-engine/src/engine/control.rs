//! What other threads hold: the state shared with the engine thread, [`EngineControl`]
//! and [`EngineHandle`], and the status the engine publishes.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use kelvo_schema::lock::LockExt;
use kelvo_schema::{Capabilities, HostId, Module, PowerSource, Settings};
use kelvo_store::Writer;

use super::Engine;
use crate::bus::{BusMsg, EngineStatus};
use crate::inbox::{Command, Inbox, SleepAck};

/// `Shared::process_period` when no window wants process rows.
pub(super) const NO_PROCESS_INTEREST: u32 = u32::MAX;

/// State shared between the engine thread and its control handles. The ring buffer and
/// latest frame are not here: they belong to the host's [`crate::LiveHub`] (D-066).
pub(super) struct Shared {
    pub(super) host: HostId,
    pub(super) caps: Mutex<Arc<Capabilities>>,
    pub(super) status: Mutex<EngineStatus>,
    /// The shortest period any visible window wants process rows at, ms
    /// ([`NO_PROCESS_INTEREST`]: none).
    pub(super) process_period: AtomicU32,
    /// Windows showing detail the tray does not (D-061).
    pub(super) detail: AtomicU32,
    /// A visible window with process interest shows network rates (D-081).
    pub(super) network: AtomicBool,
    /// A visible window with process interest shows per-process GPU time.
    pub(super) gpu: AtomicBool,
    /// A visible window with process interest shows the ports processes listen on.
    pub(super) ports: AtomicBool,
    /// The "Network history" setting (D-089): per-app bytes are sampled on every process
    /// tick and persisted. Off, the per-process network collector runs only while a view
    /// asks for rates (D-082).
    pub(super) net_history: AtomicBool,
}

/// Adds or removes one unit of a counted interest; never goes below zero. Returns the
/// count before.
fn count_interest(n: &AtomicU32, interested: bool) -> u32 {
    if interested {
        n.fetch_add(1, Ordering::AcqRel)
    } else {
        n.try_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            Some(v.saturating_sub(1))
        })
        .unwrap_or_else(|v| v)
    }
}

/// A cloneable handle for controlling and reading a running engine.
#[derive(Clone)]
pub struct EngineControl {
    pub(super) inbox: Inbox,
    pub(super) shared: Arc<Shared>,
}

impl EngineControl {
    pub fn host(&self) -> HostId {
        self.shared.host
    }

    /// Pause stops the collectors and writes a `paused` gap; the bus stays up.
    pub fn set_paused(&self, paused: bool) {
        self.inbox.command(Command::Pause(paused));
    }

    /// Applies interval, battery slowdown and module switches.
    pub fn apply_settings(&self, settings: &Settings) {
        self.inbox
            .command(Command::Settings(Box::new(settings.clone())));
    }

    /// Detaches from the store (`None`) or attaches to a new one; see
    /// [`crate::SourceControl::set_store`]. The receiver gets a message (or disconnects)
    /// once the engine has applied it.
    pub fn set_store(&self, store: Option<Writer>) -> crossbeam_channel::Receiver<()> {
        let (ack, done) = SleepAck::new();
        self.inbox.command(Command::SetStore(store, ack));
        done
    }

    /// Re-probes the collectors of `modules` on the next tick, as an IOKit device hint
    /// would. A changed series set publishes a new layout and capabilities revision.
    pub fn reprobe(&self, modules: &[Module]) {
        self.inbox.hint(modules);
    }

    /// How often a window wants process rows: `None` when no window does (the processes
    /// collector, `Cadence::Adaptive`, samples every 10 s), otherwise the shortest period
    /// any visible window asked for (`Some(0)`: every tick). The caller aggregates the
    /// windows (the shell's live registry); this replaces the previous value.
    pub fn set_process_interest(&self, period_ms: Option<u32>) {
        self.shared.process_period.store(
            period_ms.map_or(NO_PROCESS_INTEREST, |p| p.min(NO_PROCESS_INTEREST - 1)),
            Ordering::Release,
        );
    }

    pub fn process_interest(&self) -> Option<u32> {
        let p = self.shared.process_period.load(Ordering::Acquire);
        (p != NO_PROCESS_INTEREST).then_some(p)
    }

    /// Counts visible windows that show detail the tray does not (cluster frequency and
    /// residency, GPU states, component power). While none does ("tray-only mode"),
    /// `Cadence::Adaptive` collectors keyed on `Interest::Detail` (IOReport) run at their
    /// idle period instead of every tick (D-061), and the engine is backgrounded
    /// ([`EngineStatus::backgrounded`], D-094). Each `true` must be matched by one
    /// `false`.
    ///
    /// When the count crosses between zero and one the engine re-chooses its base tick
    /// at once, not on its next (up to [`BACKGROUND_TICK_MS`] away) tick, and the
    /// receiver gets a message (or disconnects) once the new status is published, so a
    /// caller can hold a window's stream until its first status has the visible tick.
    /// `None` when the count did not cross.
    ///
    /// [`BACKGROUND_TICK_MS`]: super::BACKGROUND_TICK_MS
    pub fn set_detail_interest(&self, interested: bool) -> Option<crossbeam_channel::Receiver<()>> {
        let before = count_interest(&self.shared.detail, interested);
        let crossed = if interested { before == 0 } else { before == 1 };
        crossed.then(|| {
            let (ack, done) = SleepAck::new();
            self.inbox.command(Command::Detail(ack));
            done
        })
    }

    pub fn detail_interest(&self) -> u32 {
        self.shared.detail.load(Ordering::Acquire)
    }

    /// Whether a visible window with process interest shows per-process network rates.
    /// While one does, the per-process network collector runs on the process ticks;
    /// when none does it is released and holds nothing open (D-081). Counts only while
    /// [`EngineControl::set_process_interest`] has a period. The caller aggregates the
    /// windows; this replaces the previous value.
    pub fn set_network_process_interest(&self, interested: bool) {
        self.shared.network.store(interested, Ordering::Release);
    }

    pub fn network_process_interest(&self) -> bool {
        self.shared.network.load(Ordering::Acquire)
    }

    /// Whether a visible window with process interest shows per-process GPU time. Works
    /// like [`EngineControl::set_network_process_interest`]: the per-process GPU
    /// collector runs on the process ticks while it is set and is released when it is
    /// not (D-085).
    pub fn set_gpu_process_interest(&self, interested: bool) {
        self.shared.gpu.store(interested, Ordering::Release);
    }

    pub fn gpu_process_interest(&self) -> bool {
        self.shared.gpu.load(Ordering::Acquire)
    }

    /// Whether a visible window with process interest shows the TCP ports processes
    /// listen on. Works like [`EngineControl::set_gpu_process_interest`]: the ports
    /// collector fills the rows on the process ticks while it is set.
    pub fn set_port_process_interest(&self, interested: bool) {
        self.shared.ports.store(interested, Ordering::Release);
    }

    pub fn port_process_interest(&self) -> bool {
        self.shared.ports.load(Ordering::Acquire)
    }

    /// The "Network history" setting (D-089), on by default. On, the per-process network
    /// collector samples on every tick the processes collector samples (every 10 s with
    /// only the tray open) and stays open across view changes, and per-app bytes go to
    /// the store and to the hub's ring ([`crate::LiveHub::recent_net_buckets`]). Off, it
    /// runs only while a view asks for rates and is released when none does (D-082), and
    /// nothing is recorded; the open buckets are written first. Takes effect on the next
    /// tick.
    pub fn set_network_history(&self, on: bool) {
        self.shared.net_history.store(on, Ordering::Release);
    }

    pub fn network_history(&self) -> bool {
        self.shared.net_history.load(Ordering::Acquire)
    }

    pub fn capabilities(&self) -> Arc<Capabilities> {
        Arc::clone(&self.shared.caps.lock_ok())
    }

    pub fn status(&self) -> EngineStatus {
        self.shared.status.lock_ok().clone()
    }

    /// Asks the engine to stop. [`EngineHandle`] also waits for it.
    pub fn shutdown(&self) {
        self.inbox.command(Command::Shutdown);
    }
}

/// A running engine thread. Dropping it shuts the engine down and joins the thread.
pub struct EngineHandle {
    pub(super) control: EngineControl,
    pub(super) thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
    pub fn control(&self) -> &EngineControl {
        &self.control
    }

    /// Stops the engine (flushing buckets and closing open gaps) and waits for it.
    pub fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.control.shutdown();
            if thread.join().is_err() {
                tracing::error!("engine thread panicked");
            }
        }
    }
}

impl std::ops::Deref for EngineHandle {
    type Target = EngineControl;
    fn deref(&self) -> &EngineControl {
        &self.control
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Engine {
    fn status_now(&self) -> EngineStatus {
        let ps = self.power_state;
        EngineStatus {
            interval_ms: self.interval_ms,
            paused: self.paused,
            asleep: self.asleep.is_some(),
            on_battery: ps.on_battery,
            low_power_mode: ps.low_power_mode,
            backed_off: self.visible_interval() != self.settings.interval_ms.max(100),
            backgrounded: self.backgrounded(),
            display_idle: ps.display_asleep || ps.screen_locked,
            history_held_until: self.persist_from,
            performance: self.performance(),
            power_source: PowerSource::resolve(ps.on_battery, ps.charging),
            primary_iface: self.primary_iface.clone(),
        }
    }

    pub(super) fn publish_status(&self) {
        let status = self.status_now();
        let mut cur = self.shared.status.lock_ok();
        if *cur != status {
            *cur = status.clone();
            drop(cur);
            self.sink.live.publish(BusMsg::Status(status));
        }
    }

    pub(super) fn force_publish_status(&self) {
        let status = self.status_now();
        *self.shared.status.lock_ok() = status.clone();
        self.sink.live.publish(BusMsg::Status(status));
    }
}
