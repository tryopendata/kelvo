//! Process view shaping: which rows of a [`ProcessBatch`](crate::ProcessBatch) a consumer
//! asked for (how many, ranked by what, how often). Lives next to the [`LiveHub`]
//! (crate::LiveHub) so the app's live channels and a headless agent (v4) cut the process
//! table the same way; the app shell maps [`ProcessView`] from its IPC type and the
//! selected rows to its own.

use kelvo_collect::ProcessSample;

/// How a consumer wants process rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessView {
    /// Rows per sort key: the batch is the union of the top `limit` processes by each
    /// key in `sort`. `None` is every readable process.
    pub limit: Option<u16>,
    /// The keys to rank by. Empty means CPU.
    pub sort: Vec<ProcessSort>,
    /// At most one batch per this many ms (`None`: every sample).
    pub period_ms: Option<u32>,
    /// The consumer shows per-process network rates (D-081).
    pub network: bool,
    /// The consumer shows per-process GPU time.
    pub gpu: bool,
}

/// A descending sort key for [`ProcessView`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessSort {
    Cpu,
    Memory,
    Threads,
    Wakeups,
    Energy,
    DiskRead,
    DiskWrite,
    /// Read plus write.
    DiskTotal,
    NetRx,
    NetTx,
    /// Receive plus send.
    NetTotal,
    /// Share of the GPU.
    Gpu,
}

/// A process's value for `s`; `NaN` ranks last.
fn sort_value(s: ProcessSort, p: &ProcessSample) -> f64 {
    let v = match s {
        ProcessSort::Cpu => f64::from(p.cpu_pct),
        ProcessSort::Memory => p.mem_bytes as f64,
        ProcessSort::Threads => f64::from(p.threads),
        ProcessSort::Wakeups => f64::from(p.idle_wakeups_per_s),
        ProcessSort::Energy => f64::from(p.energy),
        ProcessSort::DiskRead => f64::from(p.disk_read_bps),
        ProcessSort::DiskWrite => f64::from(p.disk_write_bps),
        ProcessSort::DiskTotal => f64::from(p.disk_read_bps) + f64::from(p.disk_write_bps),
        ProcessSort::NetRx => p.net_rx_bps.map_or(f64::NAN, f64::from),
        ProcessSort::NetTx => p.net_tx_bps.map_or(f64::NAN, f64::from),
        ProcessSort::NetTotal => match (p.net_rx_bps, p.net_tx_bps) {
            (Some(rx), Some(tx)) => f64::from(rx) + f64::from(tx),
            _ => f64::NAN,
        },
        ProcessSort::Gpu => p.gpu_pct.map_or(f64::NAN, f64::from),
    };
    if v.is_nan() { f64::NEG_INFINITY } else { v }
}

/// The rows `view` asks for, each passed through `out`: every row in collector order, or
/// the union of the top `limit` by each sort key, ordered by the first. Only the rows
/// selected are converted, so a top-5 view costs a few conversions per batch, not the
/// whole table.
pub fn select_processes<T>(
    rows: &[ProcessSample],
    view: &ProcessView,
    out: impl FnMut(&ProcessSample) -> T,
) -> Vec<T> {
    let Some(limit) = view.limit else {
        return rows.iter().map(out).collect();
    };
    let limit = usize::from(limit);
    let sorts: &[ProcessSort] = if view.sort.is_empty() {
        &[ProcessSort::Cpu]
    } else {
        &view.sort
    };
    let desc = |s: ProcessSort| {
        move |a: &&ProcessSample, b: &&ProcessSample| sort_value(s, b).total_cmp(&sort_value(s, a))
    };
    let mut all: Vec<&ProcessSample> = rows.iter().collect();
    let mut picked: Vec<&ProcessSample> = Vec::with_capacity(limit * sorts.len());
    for &s in sorts {
        if limit < all.len() {
            all.select_nth_unstable_by(limit, desc(s));
        }
        picked.extend(all.iter().take(limit));
    }
    // A process in the top rows of two keys is sent once.
    let mut seen = std::collections::HashSet::with_capacity(picked.len());
    picked.retain(|p| seen.insert((p.pid, p.start_time_us)));
    if let Some(&first) = sorts.first() {
        picked.sort_by(desc(first));
    }
    picked.into_iter().map(out).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(limit: Option<u16>, sort: &[ProcessSort]) -> ProcessView {
        ProcessView {
            limit,
            sort: sort.to_vec(),
            period_ms: None,
            network: false,
            gpu: false,
        }
    }

    fn proc(pid: i32, cpu: f32, mem: u64) -> ProcessSample {
        ProcessSample {
            pid,
            start_time_us: i64::from(pid) * 10,
            name: format!("p{pid}").into(),
            cpu_pct: cpu,
            mem_bytes: mem,
            compressed_bytes: None,
            threads: 1,
            idle_wakeups_per_s: 0.0,
            energy: 0.0,
            energy_j: 0.0,
            app: None,
            app_main: false,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            net_rx_bps: None,
            net_tx_bps: None,
            gpu_pct: None,
            user: "me".into(),
        }
    }

    #[test]
    fn top_rows_are_the_union_of_each_sort_key_ordered_by_the_first() {
        // CPU order: 4, 3, 2, 1, 0; memory order: 0, 1, 2, 3, 4.
        let rows: Vec<_> = (0..5).map(|i| proc(i, i as f32, (10 - i) as u64)).collect();
        let pids = |v: &ProcessView| -> Vec<i32> { select_processes(&rows, v, |p| p.pid) };
        assert_eq!(pids(&view(Some(2), &[])), [4, 3], "CPU by default");
        assert_eq!(
            pids(&view(Some(2), &[ProcessSort::Cpu, ProcessSort::Memory])),
            [4, 3, 1, 0]
        );
        assert_eq!(
            pids(&view(Some(3), &[ProcessSort::Memory, ProcessSort::Cpu])),
            [0, 1, 2, 3, 4],
            "a process in both tops is sent once"
        );
        assert_eq!(pids(&view(None, &[ProcessSort::Memory])), [0, 1, 2, 3, 4]);
        assert_eq!(pids(&view(Some(9), &[])), [4, 3, 2, 1, 0]);
    }

    /// #21: a NaN value ranks last instead of making the comparator inconsistent.
    #[test]
    fn a_nan_sort_value_ranks_last() {
        let rows: Vec<_> = [3.0, f32::NAN, 5.0, f32::NAN, 1.0, 4.0]
            .into_iter()
            .enumerate()
            .map(|(i, cpu)| proc(i as i32, cpu, 0))
            .collect();
        let pids = |limit| -> Vec<i32> {
            select_processes(&rows, &view(limit, &[ProcessSort::Cpu]), |p| p.pid)
        };
        assert_eq!(pids(Some(3)), [2, 5, 0]);
        assert_eq!(&pids(Some(6))[..4], [2, 5, 0, 4]);
    }
}
