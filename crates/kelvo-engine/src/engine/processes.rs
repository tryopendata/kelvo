//! The existing live process publication and diagnostic top-process snapshots.
use super::Engine;
use crate::bus::{BusMsg, ProcessBatch};
use kelvo_collect::ProcessSample;
use kelvo_schema::Tier;
use kelvo_store::ProcRow;
use std::sync::Arc;
const PROC_SNAPSHOT_TOP: usize = 30;
impl Engine {
    pub(super) fn on_processes(&mut self, ts: i64, procs: Vec<ProcessSample>) {
        let bucket = Tier::S10.bucket_start(ts);
        if bucket.is_some() && bucket != self.last_proc_bucket {
            self.last_proc_bucket = bucket;
            let mut top: Vec<&ProcessSample> = procs.iter().collect();
            top.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
            let rows: Vec<ProcRow> = top
                .into_iter()
                .take(PROC_SNAPSHOT_TOP)
                .map(|p| ProcRow {
                    name: p.name.to_string(),
                    pid: p.pid,
                    cpu_pct: p.cpu_pct,
                    mem_bytes: p.mem_bytes,
                    threads: p.threads,
                    idle_wakeups_per_s: p.idle_wakeups_per_s,
                    energy: p.energy,
                })
                .collect();
            if let Some(store) = &self.sink.store {
                let r = store.write_proc_snapshot(self.host, ts, rows);
                self.store_result("process snapshot", r, ts);
            }
        }
        self.publish_processes(ts, procs);
    }

    pub(super) fn publish_processes(&self, ts: i64, procs: Vec<ProcessSample>) {
        self.sink
            .live
            .publish(BusMsg::Processes(Arc::new(ProcessBatch {
                ts_ms: ts,
                rows: procs,
                gpu_span_ms: self.proc_gpu_span_ms,
            })));
    }
}
