//! Which status items to create and remove, retries for the ones that failed, and what
//! a drawn frame changes on an item.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use super::{ItemKey, TrayItem};

/// How the status items on screen change to match the wanted ones.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ItemChange {
    /// Shown but no longer wanted.
    pub remove: Vec<ItemKey>,
    /// Wanted but not shown, in the order to create them.
    pub create: Vec<ItemKey>,
}

/// What to remove and create so the items on screen are `wanted`. macOS puts a new
/// status item to the left of Kelvo's existing ones, so new items are created last to
/// first and come out in `wanted`'s (module) order. Once a position is saved under the
/// item's autosave name, macOS uses that instead. A key in `waiting` could not be
/// created earlier and is not tried again yet ([`Retries`]).
pub fn item_change(
    shown: &BTreeSet<ItemKey>,
    wanted: &[TrayItem],
    waiting: &BTreeSet<ItemKey>,
) -> ItemChange {
    let remove = shown
        .iter()
        .copied()
        .filter(|k| !wanted.iter().any(|w| w.key == *k))
        .collect();
    let create = wanted
        .iter()
        .rev()
        .map(|w| w.key)
        .filter(|k| !shown.contains(k) && !waiting.contains(k))
        .collect();
    ItemChange { remove, create }
}

/// How long a status item that could not be created waits before it is tried again.
pub const RETRY_AFTER: Duration = Duration::from_secs(30);

/// Status items that could not be created, and when each is tried again. A failure
/// that persists retries every [`RETRY_AFTER`] and is logged once per streak, not on
/// every frame.
#[derive(Debug, Default)]
pub struct Retries {
    next: BTreeMap<ItemKey, Instant>,
}

impl Retries {
    /// Records that creating `key` failed at `now`. True for the first failure of a
    /// streak (since the item was last created, or was last unwanted): the one to log.
    pub fn failed(&mut self, key: ItemKey, now: Instant) -> bool {
        self.next.insert(key, now + RETRY_AFTER).is_none()
    }

    /// `key` was created: its streak is over.
    pub fn created(&mut self, key: ItemKey) {
        self.next.remove(&key);
    }

    /// Forgets the keys no longer wanted, and returns those still waiting at `now`.
    pub fn waiting(&mut self, wanted: &[TrayItem], now: Instant) -> BTreeSet<ItemKey> {
        self.next.retain(|k, _| wanted.iter().any(|w| w.key == *k));
        self.next
            .iter()
            .filter(|(_, at)| **at > now)
            .map(|(k, _)| *k)
            .collect()
    }

    /// When the next waiting key is due, if any.
    pub fn deadline(&self) -> Option<Instant> {
        self.next.values().min().copied()
    }

    /// Makes every waiting key due at `now`: the settings changed or the display woke.
    /// The streaks are kept, so a failure that persists is not logged again.
    pub fn retry_now(&mut self, now: Instant) {
        for at in self.next.values_mut() {
            *at = now;
        }
    }
}

/// What a drawn frame has to change on the status item besides the image itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemChanges {
    /// The image's pixel size changed: the status item must resize (the slow path through
    /// tray-icon, which also resizes its click target). Otherwise only the image is
    /// swapped, at the same size.
    pub resize: bool,
    /// The VoiceOver label, when its words changed.
    pub label: Option<String>,
}

/// What the status item shows now, so a drawn frame touches only what differs.
#[derive(Debug, Default)]
pub struct ItemState {
    size: Option<(u32, u32)>,
    label: Option<String>,
}

impl ItemState {
    /// Records an image of `width` x `height` px with `label` as shown, and returns what
    /// differs from the last one.
    pub fn changes(&mut self, width: u32, height: u32, label: &str) -> ItemChanges {
        let resize = self.size != Some((width, height));
        self.size = Some((width, height));
        let label = (self.label.as_deref() != Some(label)).then(|| {
            self.label = Some(label.to_owned());
            label.to_owned()
        });
        ItemChanges { resize, label }
    }

    /// Forgets what is shown (a set failed), so the next frame takes the full path.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}
