//! Folding minute rows into 15-minute rows (D-076), and network rows into coarser network
//! rows (D-089, [`NetFold`]).
//!
//! A 15-minute bucket's row for one layout is built from that layout's minute rows in the
//! bucket: min of the minutes' mins, max of their maxes, and the mean of their averages.
//! Minute rows carry no sample counts, so every minute with a value weighs the same. A
//! minute whose average is `NaN` (in the layout, never sampled) adds nothing, and a series
//! no minute measured stays a `NaN` triple, never zeros. Rows of different layouts in the
//! same bucket stay separate rows, as the engine's accumulators keep them (one row per
//! bucket and layout).

use std::collections::HashMap;

use crate::blob::{NetHeader, OTHER_APPS, PackedNetApp};

/// Running min/max/mean per series for one 15-minute bucket and layout.
pub(crate) struct StatsFold {
    min: Vec<f32>,
    max: Vec<f32>,
    sum: Vec<f64>,
    n: Vec<u32>,
}

impl StatsFold {
    /// A fold for rows of `series` series (`3 * series` values each).
    pub(crate) fn new(series: usize) -> Self {
        Self {
            min: vec![f32::INFINITY; series],
            max: vec![f32::NEG_INFINITY; series],
            sum: vec![0.0; series],
            n: vec![0; series],
        }
    }

    /// Adds one minute row of `(min, max, avg)` triples. False, and nothing added, when
    /// its length does not match the fold's layout.
    pub(crate) fn add(&mut self, stats: &[f32]) -> bool {
        if stats.len() != self.n.len() * 3 {
            return false;
        }
        let triples = stats.as_chunks::<3>().0;
        for (i, &[mn, mx, avg]) in triples.iter().enumerate() {
            if !avg.is_finite() {
                continue;
            }
            if let (Some(a), Some(b), Some(s), Some(c)) = (
                self.min.get_mut(i),
                self.max.get_mut(i),
                self.sum.get_mut(i),
                self.n.get_mut(i),
            ) {
                *a = a.min(mn);
                *b = b.max(mx);
                *s += f64::from(avg);
                *c += 1;
            }
        }
        true
    }

    /// The folded row: `(min, max, avg)` per series, a `NaN` triple where no minute had a
    /// value.
    pub(crate) fn finish(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.n.len() * 3);
        for i in 0..self.n.len() {
            match (
                self.n.get(i),
                self.min.get(i),
                self.max.get(i),
                self.sum.get(i),
            ) {
                (Some(&n), Some(&mn), Some(&mx), Some(&s)) if n > 0 => {
                    out.extend([mn, mx, (s / f64::from(n)) as f32]);
                }
                _ => out.extend([f32::NAN; 3]),
            }
        }
        out
    }
}

/// Apps kept per stored network bucket (10 s, 1 m and 15 m alike), by rx + tx. The rest
/// are folded into "other apps" (D-089).
pub const NET_TOP_APPS: usize = 20;

/// Exact sums of network rows: the 10 s rows of a minute, or the minute rows of a quarter.
/// Header counters and each app's bytes add up; [`NetFold::finish`] keeps the top
/// [`NET_TOP_APPS`] and folds the rest into "other apps", so every total survives the
/// fold. An app inside the top 20 of the sum that was folded in one of the parts keeps
/// those bytes in "other apps": per-app sums can understate, never overstate.
#[derive(Default)]
pub(crate) struct NetFold {
    header: NetHeader,
    apps: HashMap<u32, (u64, u64)>,
}

impl NetFold {
    pub(crate) fn add(&mut self, header: &NetHeader, rows: &[PackedNetApp]) {
        self.header.add(header);
        for r in rows {
            self.add_app(r.name_id, r.rx, r.tx);
        }
    }

    pub(crate) fn add_header(&mut self, header: &NetHeader) {
        self.header.add(header);
    }

    pub(crate) fn add_app(&mut self, name_id: u32, rx: u64, tx: u64) {
        let e = self.apps.entry(name_id).or_default();
        e.0 = e.0.saturating_add(rx);
        e.1 = e.1.saturating_add(tx);
    }

    /// The header and the top apps by rx + tx (ties by `name_id`), then "other apps"
    /// last. Apps with no bytes are left out.
    pub(crate) fn finish(self) -> (NetHeader, Vec<PackedNetApp>) {
        let mut other = self.apps.get(&OTHER_APPS).copied().unwrap_or_default();
        let mut rows: Vec<PackedNetApp> = self
            .apps
            .into_iter()
            .filter(|&(id, (rx, tx))| id != OTHER_APPS && (rx | tx) != 0)
            .map(|(name_id, (rx, tx))| PackedNetApp { name_id, rx, tx })
            .collect();
        rows.sort_by(|a, b| {
            b.rx.saturating_add(b.tx)
                .cmp(&a.rx.saturating_add(a.tx))
                .then(a.name_id.cmp(&b.name_id))
        });
        for r in rows.iter().skip(NET_TOP_APPS) {
            other.0 = other.0.saturating_add(r.rx);
            other.1 = other.1.saturating_add(r.tx);
        }
        rows.truncate(NET_TOP_APPS);
        if (other.0 | other.1) != 0 {
            rows.push(PackedNetApp {
                name_id: OTHER_APPS,
                rx: other.0,
                tx: other.1,
            });
        }
        (self.header, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAN: f32 = f32::NAN;

    fn app(name_id: u32, rx: u64, tx: u64) -> PackedNetApp {
        PackedNetApp { name_id, rx, tx }
    }

    #[test]
    fn the_net_fold_keeps_the_top_20_and_folds_the_rest_exactly() {
        let mut f = NetFold::default();
        // 30 apps, app i moving 100 * i bytes down and i up, plus some "other" already.
        let rows: Vec<PackedNetApp> = (1..=30)
            .map(|i| app(i, 100 * u64::from(i), u64::from(i)))
            .collect();
        let header = NetHeader {
            measured_ms: 10_000,
            rx_bytes: 1_000_000,
            tx_bytes: 2_000,
            rx_pkts: 700,
            tx_pkts: 300,
        };
        f.add(&header, &rows);
        f.add(
            &header,
            &[app(OTHER_APPS, 7, 3), app(30, 1, 1), app(31, 0, 0)],
        );
        let (h, out) = f.finish();
        assert_eq!(h.measured_ms, 20_000);
        assert_eq!(h.rx_bytes, 2_000_000);
        assert_eq!(h.tx_pkts, 600);
        assert_eq!(out.len(), NET_TOP_APPS + 1);
        let ids: Vec<u32> = out.iter().take(NET_TOP_APPS).map(|r| r.name_id).collect();
        assert_eq!(ids, (11..=30).rev().collect::<Vec<_>>(), "largest first");
        assert_eq!(out[0], app(30, 3_001, 31), "repeated ids are summed");
        // Apps 1..=10 fold into "other", on top of what was already there.
        let folded_rx: u64 = (1..=10).map(|i| 100 * i).sum();
        let folded_tx: u64 = (1..=10).sum();
        assert_eq!(
            out[NET_TOP_APPS],
            app(OTHER_APPS, folded_rx + 7, folded_tx + 3)
        );
        let rx: u64 = out.iter().map(|r| r.rx).sum();
        let expect_rx: u64 = rows.iter().map(|r| r.rx).sum::<u64>() + 7 + 1;
        assert_eq!(rx, expect_rx, "nothing lost in the fold");
    }

    #[test]
    fn the_net_fold_leaves_out_empty_apps_and_an_empty_other() {
        let mut f = NetFold::default();
        f.add(&NetHeader::default(), &[app(1, 5, 0), app(2, 0, 0)]);
        assert_eq!(f.finish().1, vec![app(1, 5, 0)]);
    }

    #[test]
    fn folds_min_of_mins_max_of_maxes_mean_of_averages() {
        let mut f = StatsFold::new(2);
        assert!(f.add(&[1.0, 5.0, 3.0, 10.0, 20.0, 15.0]));
        assert!(f.add(&[0.5, 4.0, 2.0, 12.0, 30.0, 25.0]));
        assert!(f.add(&[2.0, 9.0, 7.0, NAN, NAN, NAN]));
        let out = f.finish();
        assert_eq!(&out[..3], &[0.5, 9.0, 4.0], "mean of 3, 2 and 7");
        assert_eq!(
            &out[3..],
            &[10.0, 30.0, 20.0],
            "the unsampled minute does not count"
        );
    }

    #[test]
    fn a_series_no_minute_measured_stays_nan() {
        let mut f = StatsFold::new(2);
        assert!(f.add(&[1.0, 1.0, 1.0, NAN, NAN, NAN]));
        assert!(f.add(&[2.0, 2.0, 2.0, NAN, NAN, NAN]));
        let out = f.finish();
        assert_eq!(&out[..3], &[1.0, 2.0, 1.5]);
        assert!(out[3..].iter().all(|v| v.is_nan()), "{out:?}");
    }

    #[test]
    fn a_row_of_another_width_is_refused() {
        let mut f = StatsFold::new(1);
        assert!(!f.add(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
        assert!(f.finish().iter().all(|v| v.is_nan()));
    }
}
