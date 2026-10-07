//! Per-process network rates from the private NetworkStatistics framework (D-081).
//!
//! `NStatManager` reports byte counts per flow (TCP and UDP sources) with the owning
//! pid. The framework is only in the dyld shared cache, so it is opened with `dlopen` and
//! every function and dictionary key is looked up with `dlsym` once ([`api`]); a missing
//! framework, symbol or key makes the collector probe `Unsupported`, and the UI hides the
//! network columns (`Capabilities::process_network`). Nothing links against it (D-058:
//! the binary's load commands stay system-only; this `dlopen` of a `/System` framework is
//! the by-hand review that decision asks for).
//!
//! Verified on macOS 27 (Darwin 27.0.0, Apple Silicon), unprivileged and unsandboxed: it
//! needs no entitlement and sees only flows owned by the current user. Root and system
//! daemons (mDNSResponder, softwareupdated, VPN tunnels) are not attributed; seeing them
//! needs `com.apple.private.network.statistics`, which an ad-hoc signed app cannot hold.
//! Key names (`processID`, `rxBytes`, `txBytes`, `ifLoopback`, `interface`,
//! `uniqueProcessID`, `processName`) were read from the exported constants on that release only;
//! [`parse_counts`](ffi::parse_counts) is covered by fixture dictionaries, the live read by ignored tests.
//!
//! Lifecycle: the manager exists only while a visible view asks for network rates
//! (`Cadence::OnDemand(Interest::NetworkProcesses)`). The first sample creates it (a
//! serial dispatch queue, all TCP and UDP sources) and sets the baseline; the engine calls
//! [`Collector::release`] when the last such view hides, which destroys it, so nothing is
//! held or called otherwise.
//!
//! Flows that were open before the manager was created report `processID` 0 in their
//! counts until a description query names them (measured on macOS 27: 127 of 130 sources
//! without one), so the first sample also runs `NStatManagerQueryAllSourcesDescriptions`,
//! and later samples repeat it, at most every [`DESCRIBE_EVERY_MS`], while some flow's
//! owner is unknown. Their bytes never go to pid 0. When a description names the owner
//! of a flow opened after the baseline, what it moved meanwhile goes to the owner's byte
//! totals as `late` bytes ([`ProcessNet::late_rx_bytes`], history only, D-089), never
//! to `rx_bps`/`tx_bps`, so the wait does not show as a one-sample spike (D-082). A flow
//! that was open before the baseline drops its wait: those bytes may predate it.
//!
//! Per sample: `NStatManagerQueryAllSources`, a wait for its completion block, then the
//! [`Ledger`](ledger::Ledger) turns cumulative per-flow counts into per-pid bytes since the last sample.
//! A query with no completion within [`QUERY_TIMEOUT`] fails the sample with
//! [`CollectError::Timeout`] and drops the manager, so the next sample starts a fresh one
//! (and a new baseline). After [`FAIL_LIMIT`] failures in a row (timeouts or a manager
//! that will not start) the collector backs off for [`BACKOFF`]: samples in between
//! report nothing, so rows stay "not measured", and the engine thread does not wait on
//! a stuck framework every process tick.
//! A flow that closed in between gets one last counts callback before its removed block,
//! so its final bytes are folded into the pid it belonged to.
//!
//! Only flows on an interface the network collector reports (Wi-Fi, Ethernet, cellular:
//! [`network::is_reported_interface`]) are counted, so the per-app bytes split the same
//! traffic as the interface totals (D-089). Loopback, VPN tunnels, bridges and AWDL are
//! skipped, judged by the flow's `interface` index: the interface-type flags describe the
//! underlying link, so a Tailscale flow on `utun4` reports `ifWiFi` (measured on macOS 27),
//! and one end of a 127.0.0.1 connection was seen without `ifLoopback`. Counting those
//! made apps exceed the interface, and the Apps table's shares pass 100%. A flow with no
//! interface index (about 0.05% of bytes on the dev Mac) is skipped too.
//!
//! Each entry carries bytes as well as rates, and the app identity the bytes belong to
//! (D-089), for network history. The identity is resolved in the callback that first
//! names a flow's owner ([`libproc::app_identity`]): a new flow's first unsolicited
//! counts arrive about 1.5 to 3 s after it opens, usually while the process is alive.
//! It is cached by `uniqueProcessID` (`kNStatSrcKeyUPID`, which, unlike the pid, is
//! never reused), and falls back to the dictionary's `processName`
//! (`kNStatSrcKeyProcessName`, present in every observed callback) when the process
//! is already gone. Names are interned per manager, so the per-sample path clones
//! reference counts and allocates nothing; closed flows keep their pid and identity
//! until the next settle, so a process that exits between samples keeps its name.

use std::collections::HashMap;
use std::ffi::CStr;
use std::time::Duration;

use kelvo_schema::{Entitlement, Module, UnsupportedReason};

use self::ffi::{Session, api};
use self::ledger::{Breaker, PID_ROOM, SettleMap, relock};
use super::{libproc, network};
use crate::{
    Cadence, CollectError, Collector, CollectorId, Every, Interest, Interval, Probe, ProcessNet,
    SampleBuf, Tick,
};

pub const ID: CollectorId = CollectorId("net_per_process");

/// How long a sample waits for the query's completion block. A query took 0.6 to 2.4 ms
/// in the D-081 spike.
const QUERY_TIMEOUT: Duration = Duration::from_millis(250);

/// How often flows with an unknown owner are described again.
const DESCRIBE_EVERY_MS: u32 = 10_000;

/// Consecutive failures (a query timeout or a manager that would not start) before the
/// collector stops trying for [`BACKOFF`].
const FAIL_LIMIT: u32 = 3;

/// How long the collector waits before reopening a manager that kept failing.
const BACKOFF: Duration = Duration::from_secs(60);

const FRAMEWORK: &CStr =
    c"/System/Library/PrivateFrameworks/NetworkStatistics.framework/NetworkStatistics";

mod ffi;
mod ledger;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::print_stdout)]
mod tests;

// ---- collector --------------------------------------------------------------------------

/// Per-process network rates (see the module docs).
pub struct NetPerProcess {
    session: Option<Session>,
    /// Continuous time of the last settle.
    last_ns: Option<u64>,
    /// Bytes per pid and identity of one settle, reused.
    bytes: SettleMap,
    /// Paces description queries while some flow's owner is unknown.
    describe: Every,
    /// Backs off from a framework that keeps failing. Kept across releases, so a view
    /// opening again does not pay for a stuck framework sooner.
    breaker: Breaker,
}

impl Default for NetPerProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl NetPerProcess {
    pub fn new() -> Self {
        Self {
            session: None,
            last_ns: None,
            bytes: HashMap::new(),
            describe: Every::new(DESCRIBE_EVERY_MS),
            breaker: Breaker::default(),
        }
    }

    /// Whether a manager is open (for tests: zero cost when nobody is looking).
    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }
}

impl Collector for NetPerProcess {
    fn id(&self) -> CollectorId {
        ID
    }

    fn cadence(&self) -> Cadence {
        Cadence::OnDemand(Interest::NetworkProcesses)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::NetworkStatistics]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Network]
    }

    fn probe(&mut self) -> Probe {
        // An open session survives a re-probe (an interface came or went): its flows
        // are still valid and dropping it would cost a baseline.
        if api().is_some() {
            Probe::Supported(Vec::new())
        } else {
            Probe::Unsupported {
                reason: UnsupportedReason::NoHardware,
            }
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        out.reserve_process_net(PID_ROOM);
        if self.session.is_none() && !self.breaker.allows(now) {
            // Backing off: no rates, so rows stay "not measured" until it ends.
            return Ok(());
        }
        let session = match &mut self.session {
            Some(s) => s,
            None => {
                let api = api().ok_or(CollectError::NotProbed)?;
                self.last_ns = None;
                self.describe.reset();
                self.bytes.reserve(PID_ROOM);
                match Session::start(api) {
                    Ok(s) => self.session.insert(s),
                    Err(e) => {
                        self.breaker.failed(now);
                        return Err(e);
                    }
                }
            }
        };
        // Flows that were open before the manager report pid 0 until described; the
        // first sample describes everything, later ones only while an owner is unknown.
        let queried = session.query(false).and_then(|()| {
            let unresolved = relock(&session.ledger).unresolved() > 0;
            if (unresolved || self.last_ns.is_none()) && self.describe.due(tick) {
                session.query(true)
            } else {
                Ok(())
            }
        });
        if let Err(e) = queried {
            // A completion that never came may come later, or never: start over with
            // a fresh manager and baseline rather than wait on this one again.
            self.session = None;
            self.last_ns = None;
            self.breaker.failed(now);
            return Err(e);
        }
        self.breaker.succeeded();
        let measured = relock(&session.ledger).settle(&mut self.bytes);
        let prev = self.last_ns.replace(tick.continuous_ns);
        let (true, Some(prev)) = (measured, prev) else {
            return Ok(());
        };
        let secs = tick.continuous_ns.saturating_sub(prev) as f64 / 1e9;
        if secs <= 0.0 {
            return Ok(());
        }
        out.set_process_net_measured(Interval {
            prev_ns: prev,
            now_ns: tick.continuous_ns,
        });
        push_settled(&self.bytes, secs, out);
        Ok(())
    }

    fn release(&mut self) {
        self.session = None;
        self.last_ns = None;
    }
}

/// One [`ProcessNet`] per settled pid and identity that moved anything, over an
/// interval of `secs`. Late bytes are carried apart: history only, never the rate.
fn push_settled(bytes: &SettleMap, secs: f64, out: &mut SampleBuf) {
    for (&(pid, _), s) in bytes {
        if s.rx == 0 && s.tx == 0 && s.late_rx == 0 && s.late_tx == 0 {
            continue;
        }
        out.push_process_net(ProcessNet {
            pid,
            // A reference count, not a copy: names are interned by the ledger.
            identity: s.ident.clone(),
            rx_bytes: s.rx,
            tx_bytes: s.tx,
            rx_bps: (s.rx as f64 / secs) as f32,
            tx_bps: (s.tx as f64 / secs) as f32,
            late_rx_bytes: s.late_rx,
            late_tx_bytes: s.late_tx,
        });
    }
}
