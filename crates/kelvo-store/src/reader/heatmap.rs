//! `query_heatmap`: one series averaged over time cells the caller chooses.
//!
//! The store knows nothing about time zones. The frontend turns "the last 30 local days"
//! into one `[start, end)` cell per local hour, in UTC milliseconds, with DST already
//! applied: a spring-forward hour that does not exist is an empty cell, a fall-back hour
//! that happens twice is one two-hour cell. Every zone in use is offset from UTC by a
//! multiple of 15 minutes, so a 15-minute or 1-minute bucket never straddles a local hour
//! and each bucket falls in exactly one cell by its start.

use std::collections::HashMap;

use kelvo_schema::{HostId, SeriesKey, Tier};
use rusqlite::params;

use super::{Reader, f32_at, load_layout};
use crate::db::bucket_rows_sql;
use crate::error::{Result, StoreError};
use crate::types::HistoryResult;

impl Reader {
    /// The average of `series` over each of `cells` (`[start_ms, end_ms)`), from
    /// `tier_15m` and the minutes still in `tier_1m`, each bucket weighed by its width in
    /// minutes, as an M15 history read weighs them (D-076). `None` for a cell with no
    /// bucket that sampled the series (asleep, off, an empty cell, the future), never
    /// zero.
    ///
    /// `cells` must be in order of start and must not overlap; empty cells are allowed.
    /// A bucket outside every cell is skipped.
    pub fn heatmap(
        &mut self,
        host: HostId,
        series: &SeriesKey,
        cells: &[(i64, i64)],
    ) -> Result<Vec<Option<f32>>> {
        let host_ref = self.host_ref(host)?;
        let mut acc = vec![(0.0_f64, 0_u32); cells.len()];
        let from = cells.iter().map(|c| c.0).min();
        let to = cells.iter().map(|c| c.1).max();
        let (Some(from), Some(to)) = (from, to) else {
            return Ok(Vec::new());
        };

        // Where `series` sits in each layout, `None` when the layout lacks it.
        let mut positions: HashMap<u32, Option<usize>> = HashMap::new();
        let mut stmt = self.conn.prepare_cached(&bucket_rows_sql(Tier::M15)?)?;
        let mut rows = stmt.query(params![host_ref, from, to])?;
        while let Some(row) = rows.next()? {
            let ts: i64 = row.get(0)?;
            let layout_id: u32 = row.get(1)?;
            let pos = match positions.get(&layout_id) {
                Some(&p) => p,
                None => {
                    let layout = load_layout(&self.conn, &mut self.layouts, layout_id)?;
                    let p = layout.iter().position(|k| k == series);
                    positions.insert(layout_id, p);
                    p
                }
            };
            let (Some(pos), Some(cell)) = (pos, cell_of(cells, ts)) else {
                continue;
            };
            let blob = row
                .get_ref(2)?
                .as_blob()
                .map_err(|e| StoreError::Corrupt(format!("bucket at {ts}: {e}")))?;
            let avg = f32_at(blob, pos * 3 + 2).ok_or_else(|| {
                StoreError::Corrupt(format!("bucket at {ts} is shorter than layout {layout_id}"))
            })?;
            // NaN: in the layout but not sampled in this bucket.
            if avg.is_nan() {
                continue;
            }
            let weight: u32 = row.get(3)?;
            if let Some(a) = acc.get_mut(cell) {
                a.0 += f64::from(avg) * f64::from(weight);
                a.1 += weight;
            }
        }
        Ok(acc
            .into_iter()
            .map(|(sum, w)| (w > 0).then(|| (sum / f64::from(w)) as f32))
            .collect())
    }
}

/// The series [`fill_battery_hours`] reads.
pub const BATTERY_CHARGE: &str = "battery.charge";
pub const BATTERY_CHARGING: &str = "battery.charging";

/// One hour cell's battery reading (`battery_hours`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatteryCell {
    /// The latest `battery.charge` in the cell; `None` when none was read, never 0.
    pub charge: Option<f32>,
    /// Whether any bucket in the cell charged (`battery.charging` max at least 0.5).
    pub charging: bool,
    /// When `charge` was read, `i64::MIN` while the cell has none.
    charge_t: i64,
}

impl Default for BatteryCell {
    fn default() -> Self {
        Self {
            charge: None,
            charging: false,
            charge_t: i64::MIN,
        }
    }
}

impl BatteryCell {
    /// A charge reading landed in the cell.
    pub fn has_charge(&self) -> bool {
        self.charge_t != i64::MIN
    }
}

/// Each battery point of `result` into the cell of `cells` holding it: the latest charge
/// and whether any bucket charged. `cells` are as [`Reader::heatmap`] takes them; `out`
/// has one entry per cell.
pub fn fill_battery_hours(cells: &[(i64, i64)], result: &HistoryResult, out: &mut [BatteryCell]) {
    for s in &result.series {
        for p in &s.points {
            let Some(hour) = cell_of(cells, p.t).and_then(|i| out.get_mut(i)) else {
                continue;
            };
            match s.key.metric.as_str() {
                BATTERY_CHARGE if p.t > hour.charge_t => {
                    hour.charge = Some(p.avg);
                    hour.charge_t = p.t;
                }
                BATTERY_CHARGING if p.max >= 0.5 => hour.charging = true,
                _ => {}
            }
        }
    }
}

/// The index of the cell containing `ts`, by binary search over starts.
fn cell_of(cells: &[(i64, i64)], ts: i64) -> Option<usize> {
    let i = cells.partition_point(|c| c.0 <= ts).checked_sub(1)?;
    let (start, end) = *cells.get(i)?;
    (start <= ts && ts < end).then_some(i)
}

#[cfg(test)]
mod tests {
    use kelvo_schema::{Labels, MetricId, SeriesKey, Tier};

    use super::{BATTERY_CHARGE, BATTERY_CHARGING, BatteryCell, cell_of, fill_battery_hours};
    use crate::types::{HistoryResult, Point, SeriesPoints};

    #[test]
    fn a_battery_hour_takes_its_latest_charge_and_any_charging() {
        let series = |metric: &'static str, points: &[(i64, f32)]| SeriesPoints {
            key: SeriesKey::new(MetricId::from_static(metric), Labels::new()),
            points: points
                .iter()
                .map(|&(t, v)| Point {
                    t,
                    min: v,
                    max: v,
                    avg: v,
                })
                .collect(),
        };
        let result = HistoryResult {
            tier: Tier::M1,
            bucket_ms: 1,
            series: vec![
                // Out of order: the later reading wins, not the last one seen.
                series(BATTERY_CHARGE, &[(5, 80.0), (2, 70.0), (12, 60.0)]),
                series(BATTERY_CHARGING, &[(3, 0.4), (15, 1.0)]),
            ],
            gaps: Vec::new(),
        };
        let cells = [(0, 10), (10, 20), (20, 30)];
        let mut out = [BatteryCell::default(); 3];
        fill_battery_hours(&cells, &result, &mut out);
        assert_eq!((out[0].charge, out[0].charging), (Some(80.0), false));
        assert_eq!((out[1].charge, out[1].charging), (Some(60.0), true));
        assert!(out[0].has_charge() && out[1].has_charge());
        assert_eq!(out[2], BatteryCell::default());
        assert!(!out[2].has_charge());
    }

    #[test]
    fn a_bucket_lands_in_the_cell_that_holds_its_start() {
        // Hour 2 does not exist (spring forward): an empty cell before hour 3.
        let cells = [(0, 10), (10, 10), (10, 20), (30, 40)];
        assert_eq!(cell_of(&cells, 0), Some(0));
        assert_eq!(cell_of(&cells, 9), Some(0));
        assert_eq!(cell_of(&cells, 10), Some(2), "never the empty cell");
        assert_eq!(cell_of(&cells, 25), None, "between cells");
        assert_eq!(cell_of(&cells, 40), None, "end is exclusive");
        assert_eq!(cell_of(&cells, -1), None);
    }
}
