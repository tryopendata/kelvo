//! Frame pacing for the status items (D-077, D-080).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use kelvo_engine::EngineStatus;

use super::{ItemKey, TrayContent, TrayFrame, TrayItem};

/// Shortest time between two drawn frames while sampling at the base tick (D-077). Each
/// redraw costs AppKit 5 to 8 ms of main-thread CPU after `setImage:` (D-073), whatever
/// the image; sampling stays at the base tick.
pub const REDRAW_PERIOD: Duration = Duration::from_secs(2);
/// A changed frame this close to the end of the period draws on arrival, so frames on a
/// 1 s tick with some jitter draw every other tick instead of waiting for the timer.
pub(super) const EARLY: Duration = Duration::from_millis(250);
/// How long past the period a held frame waits for a newer frame before the timer draws
/// it. Long enough that the next tick's frame normally comes first and the timer never
/// fires while frames flow.
pub(super) const GRACE: Duration = Duration::from_millis(500);

/// Paces the tray (D-077, D-080): skips frames that would draw the same image, and draws
/// at most once per period for all status items together, so every item that changed is
/// drawn in the same main-thread call rather than each on its own clock. Between draws
/// it holds each item's newest changed frame, so the timer can draw them if no newer
/// frame arrives and no item is left showing a stale image. Counts every outcome, per
/// item, for the debug log.
#[derive(Debug, Default)]
pub struct Pacer {
    /// What each item shows. `None` when its last draw failed: it still shows an older
    /// image. An item with no entry has never been drawn.
    last: BTreeMap<ItemKey, Option<TrayFrame>>,
    last_draw: Option<Instant>,
    pending: BTreeMap<ItemKey, TrayContent>,
    pub drawn: u64,
    /// Equal to the item's last drawn frame.
    pub skipped: u64,
    /// Changed, but inside the period: held (and mostly replaced by a newer frame).
    pub held: u64,
}

impl Pacer {
    /// The redraw period: [`REDRAW_PERIOD`], doubled while the engine is backed off for
    /// battery or Low Power Mode (its tick is then doubled too), and doubled again when
    /// `slowed`, in Performance mode (D-088) or in the background with no window showing
    /// detail (D-094): 2, 4 or 8 s. With a window open the menu bar keeps pace with it.
    pub fn period(backed_off: bool, slowed: bool) -> Duration {
        REDRAW_PERIOD * (1 + u32::from(backed_off)) * (1 + u32::from(slowed))
    }

    /// [`Pacer::period`] for the engine's status. In the background it is 4 s whatever
    /// the back-off: the background tick replaces the back-off rather than doubling, and
    /// a slower tick spaces the frames out by itself (D-094).
    pub fn for_status(s: &EngineStatus) -> Duration {
        if s.backgrounded {
            return Self::period(false, true);
        }
        Self::period(s.backed_off, s.performance.is_on())
    }

    /// Offers the newest content of every item on screen. Returns the items to draw now,
    /// together: those that differ from what they show, when the period is up, or nothing
    /// was drawn yet, or an item has never been drawn (it was just created), or `urgent`
    /// (the paused state). Otherwise the changed items are held for [`Pacer::due`]. An
    /// item missing from `items` is no longer on screen: its held frame is dropped.
    pub fn offer(
        &mut self,
        items: Vec<TrayItem>,
        now: Instant,
        period: Duration,
        urgent: bool,
    ) -> Vec<TrayItem> {
        self.pending
            .retain(|k, _| items.iter().any(|item| item.key == *k));
        let mut changed = Vec::new();
        for item in items {
            match self.last.get(&item.key) {
                Some(Some(frame)) if *frame == item.content.frame => {
                    // What the item shows is current again: nothing is left to draw.
                    self.pending.remove(&item.key);
                    self.skipped += 1;
                }
                _ => changed.push(item),
            }
        }
        if changed.is_empty() {
            return changed;
        }
        let fresh = changed
            .iter()
            .any(|item| !self.last.contains_key(&item.key));
        let due = match self.last_draw {
            Some(t) => urgent || fresh || now + EARLY >= t + period,
            None => true,
        };
        if due {
            // Every held frame is either in `changed` (newer) or was dropped above.
            self.pending.clear();
            self.drawing(changed, now)
        } else {
            self.held += changed.len() as u64;
            for item in changed {
                self.pending.insert(item.key, item.content);
            }
            Vec::new()
        }
    }

    /// When the held frames are drawn if no newer frames come first; `None` when nothing
    /// is held.
    pub fn deadline(&self, period: Duration) -> Option<Instant> {
        if self.pending.is_empty() {
            return None;
        }
        self.last_draw.map(|t| t + period + GRACE)
    }

    /// Every held frame, once their deadline has passed.
    pub fn due(&mut self, now: Instant, period: Duration) -> Vec<TrayItem> {
        match self.deadline(period) {
            Some(d) if now >= d => {}
            _ => return Vec::new(),
        }
        let items = std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(key, content)| TrayItem { key, content })
            .collect();
        self.drawing(items, now)
    }

    /// Drops the held frames: the display went idle and nothing is drawn until it wakes.
    pub fn drop_pending(&mut self) {
        self.pending.clear();
    }

    /// Forgets `key`'s last drawn frame: drawing it failed (render or `set_icon`), so the
    /// item still shows an older image. Its next frame, even one equal to the failed
    /// frame, is drawn once the period is up (or by the held-frame timer), not skipped as
    /// shown. The draw time is kept, so a failure that persists retries at the paced
    /// rate (D-077) rather than on every frame.
    pub fn forget_last(&mut self, key: ItemKey) {
        self.last.insert(key, None);
    }

    /// Forgets `key` entirely: its status item was removed. If it comes back, its first
    /// frame draws on arrival.
    pub fn remove(&mut self, key: ItemKey) {
        self.last.remove(&key);
        self.pending.remove(&key);
    }

    fn drawing(&mut self, items: Vec<TrayItem>, now: Instant) -> Vec<TrayItem> {
        for item in &items {
            self.last.insert(item.key, Some(item.content.frame.clone()));
        }
        self.last_draw = Some(now);
        self.drawn += items.len() as u64;
        items
    }
}
