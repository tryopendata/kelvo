//! Device-change hints (architecture.md infra 9). A disk or network interface appearing
//! or going away is a reason to re-probe the collectors of that module. On macOS these
//! come from IOKit match notifications ([`crate::macos::IoKitDeviceHints`]).

use crate::inbox::Inbox;

/// A source of "devices of module X changed" hints, delivered with [`Inbox::hint`].
/// Hints are coalesced: the engine re-probes once, on its next tick.
pub trait DeviceHints: Send + 'static {
    /// Starts watching. Called once; the watch lives until the value is dropped.
    fn start(&mut self, inbox: Inbox);
}
