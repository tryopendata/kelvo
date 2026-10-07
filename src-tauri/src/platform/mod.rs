//! Platform pieces of the shell. Kelvo's app shell ships on macOS only; the fallback keeps
//! the crate compiling elsewhere.

#[cfg(target_os = "macos")]
pub mod appkit;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
pub use other::*;
