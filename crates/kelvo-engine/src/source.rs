//! Sources produce data for one host (architecture.md infra 5). Every UI query goes to
//! the controller's own store and bus by `host_id`; a source only produces into them.
//!
//! v1 has one source, [`LocalSource`], wrapping the in-process engine. v4 adds a
//! `RemoteSource` that speaks `kelvo-proto` over SSH and writes into the same sink.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use kelvo_schema::lock::LockExt;
use kelvo_schema::{Capabilities, Event, HostRecord, Settings};
use kelvo_store::Writer;

use crate::bus::EngineStatus;
use crate::engine::{Engine, EngineControl, EngineHandle, EngineParts};
use crate::live::LiveHub;

/// How long [`SourceControl::set_store`] waits for the engine. The engine answers
/// between ticks; only a stuck store write would take this long.
pub const SET_STORE_WAIT: Duration = Duration::from_secs(5);

/// How long [`SourceControl::set_detail_interest`] waits for the engine to publish the
/// visible tick (D-094). The engine answers between ticks and before the open-time
/// sample, so this covers one slow tick in flight; a window shown after it opens on the
/// background tick's status and switches.
pub const DETAIL_WAIT: Duration = Duration::from_millis(200);

/// Where a source's output goes. The architecture sketch split live frames and capability
/// changes into two publishers; both are messages on the one per-host [`LiveHub`] here,
/// which also keeps the ring buffer and latest frame for every consumer (D-066).
#[derive(Clone)]
pub struct SourceSink {
    pub live: LiveHub,
    /// The controller's store writer. `None` runs live-only (history unavailable).
    pub store: Option<Writer>,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("source already started")]
    AlreadyStarted,
    #[error("store: {0}")]
    Store(#[from] kelvo_store::StoreError),
    #[error("cannot start the engine thread: {0}")]
    Spawn(#[from] std::io::Error),
}

/// Controls a started source. Implemented by the local engine; a remote source maps these
/// to protocol messages or ignores what it cannot do.
pub trait SourceControl: Send + Sync {
    fn set_paused(&self, paused: bool);
    /// How often any visible window wants process rows: `None` when none does, otherwise
    /// the shortest period asked for (`0`: every tick). The caller aggregates windows.
    fn set_process_interest(&self, period_ms: Option<u32>);
    /// Whether a visible window with process interest shows network rates (D-081).
    fn set_network_process_interest(&self, interested: bool);
    /// Whether a visible window with process interest shows GPU time.
    fn set_gpu_process_interest(&self, interested: bool);
    /// Whether a visible window with process interest shows listening ports.
    fn set_port_process_interest(&self, interested: bool);
    /// The "Network history" setting (D-089); see [`EngineControl::set_network_history`].
    fn set_network_history(&self, on: bool);
    /// A window that shows detail the tray does not is visible (D-061). Adding the first
    /// unit returns once the source publishes the status for it, or after
    /// [`DETAIL_WAIT`], so the window's stream starts on the visible tick (D-094).
    fn set_detail_interest(&self, interested: bool);
    fn apply_settings(&self, settings: &Settings);
    /// Detaches from the store (`None`: flush, close open gaps, run live-only) or
    /// attaches to a new one (opens a session and reopens the gaps the current state
    /// needs). Used when history is reset while the source runs. Returns once applied,
    /// or after [`SET_STORE_WAIT`].
    fn set_store(&self, store: Option<Writer>);
    fn status(&self) -> EngineStatus;
    /// Stops producing and waits for it. Idempotent.
    fn stop(&mut self);
}

/// A started source. Dropping it stops the source.
pub struct SourceHandle {
    inner: Box<dyn SourceControl>,
}

impl SourceHandle {
    pub fn new(inner: Box<dyn SourceControl>) -> Self {
        Self { inner }
    }

    pub fn control(&self) -> &dyn SourceControl {
        self.inner.as_ref()
    }

    pub fn stop(&mut self) {
        self.inner.stop();
    }
}

impl Drop for SourceHandle {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

pub trait Source: Send + Sync + 'static {
    fn host(&self) -> HostRecord;
    /// The latest capabilities; empty before the source has started.
    fn capabilities(&self) -> Capabilities;
    /// Begins producing into `sink`.
    fn start(self: Arc<Self>, sink: SourceSink) -> Result<SourceHandle, SourceError>;
}

impl SourceControl for EngineHandle {
    fn set_paused(&self, paused: bool) {
        self.control().set_paused(paused);
    }

    fn set_process_interest(&self, period_ms: Option<u32>) {
        self.control().set_process_interest(period_ms);
    }

    fn set_network_process_interest(&self, interested: bool) {
        self.control().set_network_process_interest(interested);
    }

    fn set_gpu_process_interest(&self, interested: bool) {
        self.control().set_gpu_process_interest(interested);
    }

    fn set_port_process_interest(&self, interested: bool) {
        self.control().set_port_process_interest(interested);
    }

    fn set_detail_interest(&self, interested: bool) {
        if let Some(done) = self.control().set_detail_interest(interested)
            && interested
            && let Err(crossbeam_channel::RecvTimeoutError::Timeout) =
                done.recv_timeout(DETAIL_WAIT)
        {
            tracing::debug!("the engine did not publish the visible tick in time");
        }
    }

    fn set_network_history(&self, on: bool) {
        self.control().set_network_history(on);
    }

    fn apply_settings(&self, settings: &Settings) {
        self.control().apply_settings(settings);
    }

    fn set_store(&self, store: Option<Writer>) {
        let done = self.control().set_store(store);
        if done.recv_timeout(SET_STORE_WAIT).is_err() {
            tracing::warn!("the engine did not confirm the store change in time");
        }
    }

    fn status(&self) -> EngineStatus {
        self.control().status()
    }

    fn stop(&mut self) {
        EngineHandle::stop(self);
    }
}

/// Whether the engine records per-app network history (D-089) under `settings`: the
/// "Network history" setting, always off in the `appstore` build, which has no
/// NetworkStatistics collector (`Capabilities::process_network` is false there).
pub fn network_history_enabled(settings: &Settings) -> bool {
    settings.history.network_history && !cfg!(feature = "appstore")
}

/// The in-process engine as a source.
///
/// The caller registers the host in the store before starting it (the app shell does,
/// and falls back to live-only when that fails); the source never writes the host row.
pub struct LocalSource {
    record: HostRecord,
    settings: Settings,
    parts: Mutex<Option<EngineParts>>,
    /// Stored alert events the engine seeds its cooldowns from when it starts.
    alert_history: Mutex<Vec<Event>>,
    control: OnceLock<EngineControl>,
}

impl LocalSource {
    /// A source for this machine with the given engine parts (usually
    /// [`EngineParts::platform`]) and the settings to start with.
    pub fn new(record: HostRecord, parts: EngineParts, settings: Settings) -> Self {
        Self {
            record,
            settings,
            parts: Mutex::new(Some(parts)),
            alert_history: Mutex::new(Vec::new()),
            control: OnceLock::new(),
        }
    }

    /// Recent stored alert events (the last [`crate::detect::alert_history_ms`]), so the
    /// engine's alert cooldowns survive a restart. Takes effect when the source starts.
    pub fn seed_alert_history(&self, events: Vec<Event>) {
        *self.alert_history.lock_ok() = events;
    }

    /// The running engine's control handle, once started.
    pub fn engine(&self) -> Option<&EngineControl> {
        self.control.get()
    }
}

impl Source for LocalSource {
    fn host(&self) -> HostRecord {
        self.record.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.control
            .get()
            .map(|c| (*c.capabilities()).clone())
            .unwrap_or_default()
    }

    fn start(self: Arc<Self>, sink: SourceSink) -> Result<SourceHandle, SourceError> {
        let parts = self
            .parts
            .lock_ok()
            .take()
            .ok_or(SourceError::AlreadyStarted)?;
        let mut engine = Engine::new(self.record.id, parts, sink, &self.settings);
        let history = std::mem::take(&mut *self.alert_history.lock_ok());
        engine.seed_alert_history(&history);
        let control = engine.control();
        // Before the first tick, so a user who turned it off never opens a session.
        control.set_network_history(network_history_enabled(&self.settings));
        let _ = self.control.set(control);
        let handle = engine.spawn()?;
        Ok(SourceHandle::new(Box::new(handle)))
    }
}
