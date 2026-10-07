//! Low-level Apple Silicon sources vendored from macmon.
//!
//! Upstream: <https://github.com/vladkens/macmon>, `src_lib/sources.rs` and
//! `src_lib/metrics.rs` at commit 98010ed (v0.8.2, 2026-10-04).
//! Copyright (c) 2024 vladkens. MIT License; the full text is in `LICENSE-macmon` next
//! to this file and must travel with it (D-042).
//!
//! What changed from upstream:
//! - Split into one file per source (CoreFoundation helpers, IOKit registry, IOReport,
//!   SMC, HID, SoC tables) and reduced to what Kelvo's collectors call.
//! - IOReport subscriptions are built per group with `IOReportCopyChannelsInGroup` and
//!   filtered by channel name, instead of copying all ~11k channels and filtering.
//!   Channel metadata is read from the first sample rather than assumed to line up with
//!   the subscription dictionary (subscribing to everything on an M3 Max returned 11,684
//!   channels but 11,519 sample items).
//! - Owned CoreFoundation objects are `core_foundation::base::CFType` wrappers released
//!   on drop; IOKit objects (`io_object_t`) are released by [`iokit::IoObject`]. Upstream
//!   leaked registry entries and a few CF objects.
//! - No `unwrap`/`panic!`: every failure is an `Option` or a typed error, because a
//!   sampler tick must never take the process down.
//! - Every `unsafe` block carries a `SAFETY:` comment.
//! - [`iokit::IoObject`]'s handle and the matching, iterator, property and release
//!   declarations are visible to the first-party `macos/iokit.rs`, which adds child
//!   walks and single-property reads to the same type rather than keeping a second
//!   IOKit wrapper.
//!
//! The collectors that turn these readings into series live one level up
//! (`macos/ioreport.rs`, `macos/smc.rs`, `macos/sensors.rs`, `macos/hid.rs`).

pub(crate) mod cf;
pub(crate) mod hid;
pub(crate) mod iokit;
pub(crate) mod ioreport;
pub(crate) mod smc;
pub(crate) mod soc;
