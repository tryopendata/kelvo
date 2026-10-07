//! `query_heatmap`: one series averaged over time cells the caller chooses.
//!
//! The store knows nothing about time zones. The frontend turns "the last 30 local days"
//! into one `[start, end)` cell per local hour, in UTC milliseconds, with DST already
//! applied: a spring-forward hour that does not exist is an empty cell, a fall-back hour
//! that happens twice is one two-hour cell. Every zone in use is offset from UTC by a
//! multiple of 15 minutes, so a 15-minute or 1-minute bucket never straddles a local hour
//! and each bucket falls in exactly one cell by its start.

use std::collections::HashMap;

use kelvo_schema::{HostId, SeriesKey};
use rusqlite::params;

use super::{Reader, f32_at, load_layout};
use crate::error::{Result, StoreError};

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
        let mut stmt = self.conn.prepare_cached(
            "SELECT bucket_ts, layout_id, blob, 15 FROM tier_15m
               WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3
             UNION ALL
             SELECT bucket_ts, layout_id, blob, 1 FROM tier_1m
               WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3",
        )?;
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

/// The index of the cell containing `ts`, by binary search over starts.
fn cell_of(cells: &[(i64, i64)], ts: i64) -> Option<usize> {
    let i = cells.partition_point(|c| c.0 <= ts).checked_sub(1)?;
    let (start, end) = *cells.get(i)?;
    (start <= ts && ts < end).then_some(i)
}

#[cfg(test)]
mod tests {
    use super::cell_of;

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
