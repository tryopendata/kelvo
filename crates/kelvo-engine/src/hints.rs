//! Device-change hints (architecture.md infra 9). A disk or network interface appearing
//! or going away is a reason to re-probe the collectors of that module. On macOS these
//! come from IOKit match notifications ([`crate::macos::IoKitDeviceHints`]).
//!
//! Linux has none, on purpose (D-115): its network and disk collectors rescan their
//! filtered sets (NIC-backed and tunnel interfaces, whole hardware disks) every 30 s and
//! bump `series_revision`, which re-probes each alone and keeps its baselines. A 30 s
//! poll of `/sys/class/net` and `/sys/block` would be no faster, and a hint re-probes
//! every collector of the module, resetting all of its rates.

use crate::inbox::Inbox;

/// A source of "devices of module X changed" hints, delivered with [`Inbox::hint`].
/// Hints are coalesced: the engine re-probes once, on its next tick.
pub trait DeviceHints: Send + 'static {
    /// Starts watching. Called once; the watch lives until the value is dropped.
    fn start(&mut self, inbox: Inbox);
}
