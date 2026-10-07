//! The popover: a 360 × 680 pt non-activating panel under the status item
//! (architecture.md, Popover lifecycle; D-024, D-033 to D-036).
//!
//! This module holds the portable parts: where the panel goes, the state shared between
//! the tray click and the window events, and open-latency bookkeeping. The AppKit panel
//! itself is in `panel.rs` (macOS only).
//!
//! Open latency is measured from the tray mouse-down in Rust to the webview's second
//! animation frame after the panel is shown (the first frame painted with the panel on
//! screen), which the page reports back with `report_popover_paint`.

#[cfg(target_os = "macos")]
mod panel;
#[cfg(target_os = "macos")]
pub use panel::{create, hide, toggle, web_content_terminated};

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kelvo_schema::lock::LockExt;

pub const LABEL: &str = "popover";
/// The page the panel loads; the frontend maps it to its `/popover` route.
pub const ROUTE: &str = "popover";
pub const WIDTH_PT: f64 = 360.0;
pub const HEIGHT_PT: f64 = 680.0;
/// Space between the menu bar and the panel's top edge.
const GAP_PT: f64 = 4.0;
/// Closest the panel gets to a screen edge.
const MARGIN_PT: f64 = 8.0;
/// A tray click this soon after the panel lost focus is the click that took the focus:
/// the user meant "close", so it must not reopen the panel.
const REOPEN_GUARD: Duration = Duration::from_millis(350);
/// Open latencies kept for the p95.
const LATENCY_WINDOW: usize = 50;

/// A rectangle in screen points with a bottom-left origin (AppKit's global space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The panel's frame for a status item at `anchor` on a screen whose visible area (below
/// the menu bar, beside the Dock) is `screen`: centred under the item, kept `MARGIN_PT`
/// inside the screen edges, and shortened if the screen is too short for 680 pt.
pub fn place(anchor: Rect, screen: Rect) -> Rect {
    let w = WIDTH_PT.min(screen.w - 2.0 * MARGIN_PT).max(1.0);
    let top = anchor.y.min(screen.y + screen.h) - GAP_PT;
    let h = HEIGHT_PT.min(top - screen.y - MARGIN_PT).max(1.0);
    let min_x = screen.x + MARGIN_PT;
    let max_x = (screen.x + screen.w - MARGIN_PT - w).max(min_x);
    let x = (anchor.x + anchor.w / 2.0 - w / 2.0).clamp(min_x, max_x);
    Rect {
        x,
        y: top - h,
        w,
        h,
    }
}

/// Click-to-paint latency samples.
#[derive(Debug, Default)]
pub struct OpenLatency {
    next_token: u32,
    pending: Option<(u32, Instant)>,
    samples: VecDeque<f64>,
}

impl OpenLatency {
    /// A panel show that started at `clicked`; returns the token the page reports back.
    pub fn start(&mut self, clicked: Instant) -> u32 {
        self.next_token = self.next_token.wrapping_add(1);
        self.pending = Some((self.next_token, clicked));
        self.next_token
    }

    /// The page painted for `token`. Returns the latency in ms, once per show.
    pub fn finish(&mut self, token: u32, now: Instant) -> Option<f64> {
        match self.pending {
            Some((t, clicked)) if t == token => {
                self.pending = None;
                let ms = now.saturating_duration_since(clicked).as_secs_f64() * 1000.0;
                if self.samples.len() == LATENCY_WINDOW {
                    self.samples.pop_front();
                }
                self.samples.push_back(ms);
                Some(ms)
            }
            _ => None,
        }
    }

    /// Nearest-rank p95 over the last `LATENCY_WINDOW` opens.
    pub fn p95(&self) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        let mut v: Vec<f64> = self.samples.iter().copied().collect();
        v.sort_by(f64::total_cmp);
        let rank = ((0.95 * v.len() as f64).ceil() as usize).clamp(1, v.len());
        v.get(rank - 1).copied()
    }

    pub fn count(&self) -> usize {
        self.samples.len()
    }
}

#[derive(Debug, Default)]
struct Inner {
    /// When the panel was last hidden, for [`REOPEN_GUARD`].
    hidden_at: Option<Instant>,
    /// The WebContent process died while the panel was showing; reload on the next hide
    /// (D-035).
    reload_on_hide: bool,
    latency: OpenLatency,
}

/// Popover state managed by Tauri.
#[derive(Debug, Default)]
pub struct Popover {
    inner: Mutex<Inner>,
}

impl Popover {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock_ok()
    }

    /// Whether a click now should be swallowed because it is the one that just hid the
    /// panel.
    fn recently_hidden(&self, now: Instant) -> bool {
        self.lock()
            .hidden_at
            .is_some_and(|t| now.saturating_duration_since(t) < REOPEN_GUARD)
    }

    /// Records a hide; returns whether the webview must reload now.
    fn hidden(&self, now: Instant) -> bool {
        let mut i = self.lock();
        i.hidden_at = Some(now);
        std::mem::take(&mut i.reload_on_hide)
    }

    fn set_reload_on_hide(&self) {
        self.lock().reload_on_hide = true;
    }

    fn start_open(&self, clicked: Instant) -> u32 {
        self.lock().latency.start(clicked)
    }

    /// The page reported its first paint after a show.
    pub fn painted(&self, token: u32) {
        let mut i = self.lock();
        if let Some(ms) = i.latency.finish(token, Instant::now()) {
            tracing::debug!(
                open_ms = format!("{ms:.1}"),
                p95_ms = i.latency.p95().map(|p| format!("{p:.1}")),
                n = i.latency.count(),
                "popover opened"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 949.0,
    };

    fn item(x: f64) -> Rect {
        Rect {
            x,
            y: 949.0,
            w: 40.0,
            h: 33.0,
        }
    }

    #[test]
    fn centred_under_the_item() {
        let r = place(item(1000.0), SCREEN);
        assert_eq!(r.w, 360.0);
        assert_eq!(r.h, 680.0);
        assert_eq!(r.x, 1020.0 - 180.0);
        assert_eq!(r.y + r.h, 949.0 - GAP_PT);
    }

    #[test]
    fn kept_inside_the_screen_edges() {
        let right = place(item(1490.0), SCREEN);
        assert_eq!(right.x + right.w, 1512.0 - MARGIN_PT);
        let left = place(item(-30.0), SCREEN);
        assert_eq!(left.x, MARGIN_PT);
        // A second display to the left of the main one, at negative x.
        let other = Rect {
            x: -1920.0,
            y: 0.0,
            w: 1920.0,
            h: 1055.0,
        };
        let r = place(
            Rect {
                x: -10.0,
                y: 1055.0,
                w: 40.0,
                h: 24.0,
            },
            other,
        );
        assert_eq!(r.x + r.w, -MARGIN_PT);
    }

    #[test]
    fn shortened_on_a_short_screen() {
        let short = Rect {
            x: 0.0,
            y: 0.0,
            w: 1024.0,
            h: 600.0,
        };
        let r = place(
            Rect {
                x: 500.0,
                y: 600.0,
                w: 30.0,
                h: 24.0,
            },
            short,
        );
        assert_eq!(r.y, MARGIN_PT);
        assert_eq!(r.h, 600.0 - GAP_PT - MARGIN_PT);
    }

    #[test]
    fn latency_p95_and_stale_tokens() {
        let mut l = OpenLatency::default();
        let t0 = Instant::now();
        let a = l.start(t0);
        let b = l.start(t0);
        assert_eq!(l.finish(a, t0), None, "a superseded show does not count");
        assert!(l.finish(b, t0 + Duration::from_millis(80)).is_some());
        assert_eq!(l.finish(b, t0), None, "counted once");
        for ms in 1..=99u64 {
            let t = l.start(t0);
            l.finish(t, t0 + Duration::from_millis(ms));
        }
        // The last 50 samples are 50..=99 ms; nearest-rank p95 is the 48th, 97 ms.
        assert_eq!(l.count(), 50);
        let p95 = l.p95().unwrap();
        assert!((p95 - 97.0).abs() < 0.01, "{p95}");
    }

    #[test]
    fn reopen_guard() {
        let p = Popover::default();
        let t = Instant::now();
        assert!(!p.recently_hidden(t));
        p.set_reload_on_hide();
        assert!(p.hidden(t), "the deferred reload happens on hide");
        assert!(!p.hidden(t), "once");
        assert!(p.recently_hidden(t + Duration::from_millis(100)));
        assert!(!p.recently_hidden(t + Duration::from_millis(400)));
    }
}
