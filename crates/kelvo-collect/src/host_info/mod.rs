//! The local host's facts: the `HostRecord` a source registers and the handshake sends
//! (name, OS, model, chip, CPU topology, memory, boot time). Read once at start, never
//! on a tick. It lives beside the collectors whose OS reads it shares, so the app and the
//! headless agent build the same record. The engine re-exports [`host_record`] because
//! the app shell may not depend on `kelvo-collect` (`.claude/rules/rust.md`).

pub mod machine;

#[cfg(not(target_os = "macos"))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
pub use linux::host_record;
#[cfg(target_os = "macos")]
pub use macos::host_record;
