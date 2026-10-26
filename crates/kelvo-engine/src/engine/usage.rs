//! Recorder lifecycle and writes; collector cadences and interests stay unchanged.
use super::Engine;
use kelvo_collect::{ProcessSample, Tick};
use kelvo_schema::Module;
use kelvo_store::usage::{USAGE_BUCKET_MS, UsageBucket};

pub(super) fn next_bucket(at: i64) -> i64 {
    at.saturating_add(USAGE_BUCKET_MS - 1)
        .div_euclid(USAGE_BUCKET_MS)
        * USAGE_BUCKET_MS
}
impl Engine {
    pub(super) fn account_usage(&mut self, tick: Tick, rows: &[ProcessSample], persist: bool) {
        if !persist {
            return;
        }
        let disabled = &self.settings.disabled;
        let mut enabled = [
            Module::Cpu,
            Module::Gpu,
            Module::Memory,
            Module::Power,
            Module::Network,
        ]
        .map(|m| !disabled.contains(&m));
        enabled[4] &= self
            .shared
            .net_history
            .load(std::sync::atomic::Ordering::Acquire);
        let caps = self.shared.caps.lock_ok();
        let processes = self
            .slots
            .iter()
            .any(|s| s.active && s.collector.id().0 == "processes");
        let supported = [
            processes,
            caps.process_gpu,
            processes,
            caps.process_energy,
            caps.process_network,
        ];
        drop(caps);
        let live = self.sink.live.clone();
        let mut usage = live.usage_recording();
        if usage.generation != self.usage_generation {
            return;
        }
        usage
            .recorder
            .record(tick, rows, &self.buf, enabled, supported);
        let grace = i64::from(self.interval_ms.max(kelvo_collect::IDLE_MS)) * 3;
        let closed = usage
            .recorder
            .close_before(tick.wall_ms.saturating_sub(grace));
        // Retain before enqueueing and hold the source lock through the queue write.
        // Clear-history takes this lock before its delete, so old writes precede it.
        usage.update_owned(closed.iter().cloned(), tick.wall_ms);
        self.write_usage(closed, tick.wall_ms);
    }
    pub(super) fn flush_usage(&mut self, now: i64) {
        self.sync_usage_generation(now);
        let live = self.sink.live.clone();
        let usage = live.usage_recording();
        if usage.generation != self.usage_generation {
            return;
        }
        if self.persist_from.is_none() {
            self.write_usage(usage.recorder.snapshots(), now);
        }
    }
    pub(super) fn sync_usage_generation(&mut self, now: i64) {
        let generation = self.sink.live.usage_generation();
        if generation != self.usage_generation {
            self.sink
                .live
                .usage_recording()
                .recorder
                .clear(next_bucket(now));
            self.usage_generation = generation;
            self.reprobe.all = true;
            for slot in &mut self.slots {
                if slot.collector.id().0 == "net_per_process"
                    || slot.collector.id().0 == "gpu_per_process"
                {
                    slot.collector.release();
                    slot.demanded = false;
                }
            }
        }
    }
    fn write_usage(&mut self, buckets: Vec<UsageBucket>, now: i64) {
        if let Some(store) = self.sink.store.clone() {
            for bucket in buckets {
                let result = store.write_usage_bucket(self.host, bucket);
                self.store_result("usage bucket", result, now);
            }
        }
    }
}
use kelvo_schema::lock::LockExt;
