//! Live calibration of a fast power reading against a slow, accurate energy counter
//! (D-054).
//!
//! On the M3 Max the SMC P-cluster power keys update every second but read about 25% low.
//! The PMP energy counters (IOReport) are accurate but only move every ~5 minutes on
//! macOS 27 (D-043). The [`Calibrator`] integrates the fast reading (watts times the time
//! since its previous sample) between two PMP moves, divides the PMP energy over the same
//! span by it, and keeps a smoothed scale to multiply the fast reading by.
//!
//! The window runs from one observed PMP move to a later one. Both ends are known only
//! to within the sample period of the slow reading, so a window closes only when it is at
//! least [`MIN_WINDOW_NS`] long and that period is at most [`MAX_RESOLUTION`] of it.
//! Until then it keeps growing. A fast step longer than [`MAX_FAST_STEP_NS`] (sleep, an
//! interval of 5 s or more, where one reading per step no longer stands for the step's
//! average) or a missing fast reading drops the window; the next move anchors a new one.
//! The scale itself is kept across dropped windows and re-probes.
//!
//! A new session starts from a seed rather than from 1.0: the scale an earlier session
//! learned for this chip, loaded through a [`ScaleStore`] the app shell implements, or the
//! chip's measured default. The PMP counters can go half an hour without moving (D-054),
//! and an unseeded reading is about 25% low all that time (D-065). The first window that
//! closes moves the seed half way to its ratio, like any later window.
//!
//! Portable: no platform calls, so it is tested with synthetic windows on any host.

/// Shortest window that can produce a calibration.
pub const MIN_WINDOW_NS: u64 = 60_000_000_000;
/// Largest slow-reading sample period, as a fraction of the window.
pub const MAX_RESOLUTION: f64 = 0.05;
/// Longest fast step that still counts as an integral of the fast reading.
pub const MAX_FAST_STEP_NS: u64 = 2_500_000_000;
/// Clamp for a window's ratio. The M3 Max reads 1.27 to 1.33 (D-054); anything outside
/// this range is a misread window, not a real scale.
pub const SCALE_MIN: f64 = 0.5;
pub const SCALE_MAX: f64 = 2.0;
/// Weight of a new window in the smoothed scale.
pub const ALPHA: f64 = 0.5;
/// A window with less fast energy than this (joules) is too idle to divide by.
pub const MIN_FAST_J: f64 = 1.0;

/// Where learned scales persist across restarts, keyed by chip (the CPU brand string,
/// "Apple M3 Max"). `kelvo-collect` never writes files; the app shell implements this.
///
/// `save` runs on the engine thread each time a window closes (at most about once a
/// minute, usually every 5 minutes or less often), so a file write there is acceptable
/// but it must not block for long.
pub trait ScaleStore: Send + Sync + 'static {
    /// The scale an earlier session learned for `chip`.
    fn load(&self, chip: &str) -> Option<f64>;
    /// The smoothed scale after a window closed.
    fn save(&self, chip: &str, scale: f64);
}

/// Keeps nothing: every session starts from the chip's default. Tests and tools.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoScaleStore;

impl ScaleStore for NoScaleStore {
    fn load(&self, _chip: &str) -> Option<f64> {
        None
    }

    fn save(&self, _chip: &str, _scale: f64) {}
}

/// Whether `scale` could be a real scale: finite and inside the clamp.
pub fn plausible_scale(scale: f64) -> bool {
    (SCALE_MIN..=SCALE_MAX).contains(&scale)
}

/// How far the current scale can be trusted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalibState {
    /// No scale: readings are raw.
    Uncalibrated,
    /// A scale from an earlier session or the chip's default; no window closed yet.
    Seeded(f64),
    /// At least one window closed in this session.
    Calibrated(f64),
}

impl CalibState {
    /// The factor to multiply the fast reading by.
    pub fn factor(self) -> f64 {
        match self {
            CalibState::Uncalibrated => 1.0,
            CalibState::Seeded(s) | CalibState::Calibrated(s) => s,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Window {
    anchor_ns: u64,
    fast_j: f64,
    slow_j: f64,
    /// Coarsest slow sample period seen at either end of the window.
    res_ns: u64,
}

/// See the module docs.
#[derive(Clone, Debug, Default)]
pub struct Calibrator {
    window: Option<Window>,
    last_fast_ns: Option<u64>,
    scale: Option<f64>,
    windows: u32,
}

impl Calibrator {
    /// The smoothed scale, or `None` before the first window closed.
    pub fn scale(&self) -> Option<f64> {
        self.scale
    }

    /// Windows closed so far.
    pub fn windows(&self) -> u32 {
        self.windows
    }

    /// See [`CalibState`].
    pub fn state(&self) -> CalibState {
        match (self.scale, self.windows) {
            (None, _) => CalibState::Uncalibrated,
            (Some(s), 0) => CalibState::Seeded(s),
            (Some(s), _) => CalibState::Calibrated(s),
        }
    }

    /// Starts from `scale` until a window closes. Ignored once a window has closed in
    /// this session (that scale is better), and for an implausible value (a corrupt
    /// file).
    pub fn seed(&mut self, scale: f64) {
        if self.windows == 0 && plausible_scale(scale) {
            self.scale = Some(scale);
        }
    }

    /// Drops the open window (re-probe, resume). The scale is kept.
    pub fn restart(&mut self) {
        self.window = None;
        self.last_fast_ns = None;
    }

    /// One fast reading at `t_ns` (monotonic). `watts` is `None` when the reading is
    /// missing or implausible.
    pub fn fast(&mut self, watts: Option<f64>, t_ns: u64) {
        let step = self.last_fast_ns.map(|l| t_ns.saturating_sub(l));
        self.last_fast_ns = Some(t_ns);
        match (watts, step) {
            (Some(w), Some(step)) if step <= MAX_FAST_STEP_NS => {
                if let Some(win) = &mut self.window {
                    win.fast_j += w * step as f64 / 1e9;
                }
            }
            // The first reading only sets the baseline; nothing is integrated yet.
            (Some(_), None) => {}
            _ => self.window = None,
        }
    }

    /// One slow sample at `t_ns` covering the last `period_ns`, with `joules` of energy.
    /// `moved` is whether the slow counters moved in it. Call it after [`Self::fast`] for
    /// the same tick. Returns the window's clamped ratio when one closes.
    pub fn slow(&mut self, joules: f64, moved: bool, period_ns: u64, t_ns: u64) -> Option<f64> {
        if !moved {
            return None;
        }
        let Some(win) = &mut self.window else {
            // The first move after a start only marks a known point: its energy covers
            // a span that began before anything was integrated.
            self.anchor(period_ns, t_ns);
            return None;
        };
        win.slow_j += joules.max(0.0);
        win.res_ns = win.res_ns.max(period_ns);
        let span = t_ns.saturating_sub(win.anchor_ns);
        if span < MIN_WINDOW_NS || win.res_ns as f64 > MAX_RESOLUTION * span as f64 {
            return None;
        }
        let (fast_j, slow_j) = (win.fast_j, win.slow_j);
        self.anchor(period_ns, t_ns);
        if fast_j < MIN_FAST_J || slow_j <= 0.0 {
            return None;
        }
        let ratio = (slow_j / fast_j).clamp(SCALE_MIN, SCALE_MAX);
        self.scale = Some(match self.scale {
            Some(s) => s + ALPHA * (ratio - s),
            None => ratio,
        });
        self.windows += 1;
        Some(ratio)
    }

    fn anchor(&mut self, period_ns: u64, t_ns: u64) {
        // An anchor needs a fast baseline at or before it; without one the window could
        // not integrate its first step.
        if self.last_fast_ns.is_none() {
            return;
        }
        self.window = Some(Window {
            anchor_ns: t_ns,
            fast_j: 0.0,
            slow_j: 0.0,
            res_ns: period_ns,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    /// Drives a synthetic machine: the fast reading is `true_w * truth_scale` every
    /// second; the slow counter is sampled every `slow_period` seconds and moves every
    /// `refresh` seconds with the true energy since its previous refresh.
    struct Sim {
        cal: Calibrator,
        t: u64,
        last_refresh: u64,
        pending_j: f64,
    }

    impl Sim {
        fn new() -> Self {
            Self {
                cal: Calibrator::default(),
                t: 0,
                last_refresh: 0,
                pending_j: 0.0,
            }
        }

        /// Runs for `secs` at 1 s fast steps. The slow counter is sampled every
        /// `slow_period` s and refreshes every `refresh` s. Returns the closed ratios.
        fn run(
            &mut self,
            secs: u64,
            true_w: impl Fn(u64) -> f64,
            truth_scale: f64,
            slow_period: u64,
            refresh: u64,
        ) -> Vec<f64> {
            let mut out = Vec::new();
            for _ in 0..secs {
                self.t += 1;
                let w = true_w(self.t);
                self.pending_j += w;
                self.cal.fast(Some(w * truth_scale), self.t * S);
                if self.t.is_multiple_of(slow_period) {
                    let moved = self.t - self.last_refresh >= refresh;
                    let j = if moved {
                        self.last_refresh = self.t;
                        std::mem::take(&mut self.pending_j)
                    } else {
                        0.0
                    };
                    out.extend(self.cal.slow(j, moved, slow_period * S, self.t * S));
                }
            }
            out
        }
    }

    #[test]
    fn no_scale_before_the_first_full_window() {
        let mut sim = Sim::new();
        // The first move only anchors; nothing closes until the second one.
        let r = sim.run(299, |_| 6.0, 0.75, 1, 300);
        assert!(r.is_empty());
        assert_eq!(sim.cal.scale(), None);
        let r = sim.run(1, |_| 6.0, 0.75, 1, 300);
        assert!(r.is_empty(), "first move anchors");
        let r = sim.run(300, |_| 6.0, 0.75, 1, 300);
        assert_eq!(r.len(), 1);
        let s = sim.cal.scale().unwrap();
        assert!((s - 1.0 / 0.75).abs() < 1e-9, "{s}");
    }

    #[test]
    fn varying_load_over_the_window_still_matches_energy() {
        let mut sim = Sim::new();
        // Load swings 1 to 20 W; the ratio is of energies, so the swing does not matter.
        let w = |t: u64| if (t / 7).is_multiple_of(3) { 20.0 } else { 1.0 };
        let r = sim.run(1500, w, 0.8, 1, 300);
        assert_eq!(r.len(), 4);
        for x in r {
            assert!((x - 1.25).abs() < 1e-9, "{x}");
        }
    }

    #[test]
    fn tray_only_slow_period_of_10_s_still_calibrates() {
        let mut sim = Sim::new();
        let r = sim.run(1200, |_| 5.0, 0.75, 10, 300);
        assert_eq!(r.len(), 3);
        assert!((sim.cal.scale().unwrap() - 4.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn coarse_resolution_waits_for_a_longer_window() {
        let mut sim = Sim::new();
        // A 60 s slow period against 300 s refreshes is 20% resolution: no window closes
        // at 300 s. Spans grow by refreshes until 60 / span <= 5%, at 1200 s.
        let r = sim.run(900, |_| 5.0, 0.75, 60, 300);
        assert!(r.is_empty());
        let r = sim.run(1200, |_| 5.0, 0.75, 60, 300);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn counters_moving_every_tick_close_after_a_minute() {
        let mut sim = Sim::new();
        let r = sim.run(130, |_| 5.0, 0.5, 1, 1);
        assert_eq!(r.len(), 2, "anchor at 1 s, closes at 61 s and 121 s");
        assert!((sim.cal.scale().unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn ratio_is_clamped_and_smoothed() {
        let mut sim = Sim::new();
        sim.run(600, |_| 5.0, 0.75, 1, 300);
        let s1 = sim.cal.scale().unwrap();
        assert!((s1 - 4.0 / 3.0).abs() < 1e-9);
        // A window where the fast reading is 10x low clamps at 2.0, then moves half way.
        let r = sim.run(300, |_| 5.0, 0.1, 1, 300);
        assert_eq!(r, [SCALE_MAX]);
        let s2 = sim.cal.scale().unwrap();
        assert!((s2 - (s1 + ALPHA * (2.0 - s1))).abs() < 1e-9, "{s2}");
        let r = sim.run(300, |_| 5.0, 10.0, 1, 300);
        assert_eq!(r, [SCALE_MIN]);
    }

    #[test]
    fn idle_window_is_skipped() {
        let mut sim = Sim::new();
        let r = sim.run(600, |_| 0.001, 0.75, 1, 300);
        assert!(r.is_empty(), "0.3 J over 300 s is below MIN_FAST_J");
        assert_eq!(sim.cal.scale(), None);
        assert_eq!(sim.cal.windows(), 0);
    }

    #[test]
    fn a_long_fast_step_drops_the_window() {
        let mut sim = Sim::new();
        sim.run(400, |_| 5.0, 0.75, 1, 300);
        // Sleep: the fast reading jumps 600 s. The slow counter then moves with energy
        // that the fast side never saw.
        sim.t += 600;
        sim.cal.fast(Some(5.0 / 0.75), sim.t * S);
        assert!(sim.cal.window.is_none());
        let before = sim.cal.windows();
        sim.cal.slow(9_999.0, true, S, sim.t * S);
        assert_eq!(sim.cal.windows(), before, "the wake move only re-anchors");
        assert_eq!(sim.cal.scale(), None);
    }

    #[test]
    fn a_missing_fast_reading_drops_the_window() {
        let mut sim = Sim::new();
        sim.run(310, |_| 5.0, 0.75, 1, 300);
        assert!(sim.cal.window.is_some());
        sim.cal.fast(None, (sim.t + 1) * S);
        assert!(sim.cal.window.is_none());
    }

    #[test]
    fn a_seed_scales_until_the_first_window_then_smooths_from_it() {
        let mut sim = Sim::new();
        sim.cal.seed(1.2);
        assert_eq!(sim.cal.state(), CalibState::Seeded(1.2));
        // The first move only anchors: still the seed.
        sim.run(300, |_| 6.0, 0.75, 1, 300);
        assert_eq!(sim.cal.state(), CalibState::Seeded(1.2));
        let r = sim.run(300, |_| 6.0, 0.75, 1, 300);
        assert_eq!(r.len(), 1);
        let want = 1.2 + ALPHA * (1.0 / 0.75 - 1.2);
        let CalibState::Calibrated(s) = sim.cal.state() else {
            panic!("not calibrated: {:?}", sim.cal.state());
        };
        assert!((s - want).abs() < 1e-9, "{s}");
    }

    #[test]
    fn a_seed_never_replaces_a_scale_learned_this_session() {
        let mut sim = Sim::new();
        sim.run(600, |_| 5.0, 0.75, 1, 300);
        let learned = sim.cal.scale();
        sim.cal.seed(1.9);
        assert_eq!(sim.cal.scale(), learned);
    }

    #[test]
    fn implausible_seeds_are_ignored() {
        let mut cal = Calibrator::default();
        for bad in [f64::NAN, f64::INFINITY, 0.0, -1.3, SCALE_MAX + 0.01] {
            cal.seed(bad);
            assert_eq!(cal.state(), CalibState::Uncalibrated, "{bad}");
        }
        cal.seed(SCALE_MIN);
        assert_eq!(cal.state(), CalibState::Seeded(SCALE_MIN));
        assert_eq!(cal.state().factor(), SCALE_MIN);
        assert_eq!(CalibState::Uncalibrated.factor(), 1.0);
    }

    #[test]
    fn restart_keeps_the_scale() {
        let mut sim = Sim::new();
        sim.run(600, |_| 5.0, 0.75, 1, 300);
        let s = sim.cal.scale();
        assert!(s.is_some());
        sim.cal.restart();
        assert_eq!(sim.cal.scale(), s);
        assert!(sim.cal.window.is_none());
    }
}
