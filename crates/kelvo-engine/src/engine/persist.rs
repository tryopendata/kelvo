//! Store writes: gaps, rollup rows, per-app network buckets, and the discard after the
//! wall clock stepped back (D-070).

use std::sync::atomic::Ordering;

use kelvo_collect::Tick;
use kelvo_schema::{Gap, GapReason, Module};

use super::Engine;
use crate::clock::ClockReading;
use crate::netacc::{Anchor, NET_BUCKET_MS, NetSlot};

/// A per-app network bucket closes once both byte streams have reported past its end, or
/// this many times the slower of the base tick and the idle period after its end,
/// whichever is first: a stream that stopped (module off, collector backing off) does
/// not hold buckets open.
const NET_CLOSE_PERIODS: i64 = 3;

fn net_close_grace(interval_ms: u32) -> i64 {
    NET_CLOSE_PERIODS * i64::from(interval_ms.max(kelvo_collect::IDLE_MS))
}

impl Engine {
    pub(super) fn store_result(&mut self, what: &str, r: kelvo_store::Result<()>, now_ms: i64) {
        if let Err(e) = r
            && let Some(suppressed) = self.store_errors.check(now_ms)
        {
            tracing::error!(suppressed, "store {what} failed: {e}");
        }
    }

    pub(super) fn open_gap(&mut self, start_ms: i64, module: Option<Module>, reason: GapReason) {
        if let Some(store) = &self.sink.store {
            let r = store.open_gap(self.host, start_ms, module, reason);
            self.store_result("open gap", r, start_ms);
        }
    }

    /// Closes the open gap of `reason` and `module` at `end_ms`; `start_ms`, when known,
    /// lets the store write the whole gap if its open never reached the file.
    pub(super) fn close_gap(
        &mut self,
        reason: GapReason,
        module: Option<Module>,
        start_ms: Option<i64>,
        end_ms: i64,
    ) {
        if let Some(store) = &self.sink.store {
            let r = store.close_gap(self.host, reason, module, start_ms, end_ms);
            self.store_result("close gap", r, end_ms);
        }
    }

    pub(super) fn write_gap(&mut self, gap: Gap) {
        if let Some(store) = &self.sink.store {
            let at = gap.start_ms;
            let r = store.write_gap(self.host, gap);
            self.store_result("write gap", r, at);
        }
    }

    pub(super) fn write_rows(&mut self, now_ms: i64) {
        let rows = std::mem::take(&mut self.rows);
        if let Some(store) = &self.sink.store {
            for row in rows {
                if let Err(e) = store.write_bucket(row)
                    && let Some(suppressed) = self.store_errors.check(now_ms)
                {
                    tracing::error!(suppressed, "store write bucket failed: {e}");
                }
            }
        }
    }

    /// Emits the open buckets of both tiers without resetting them (sleep, pause,
    /// shutdown).
    pub(super) fn flush_buckets(&mut self, now_ms: i64) {
        self.sink.live.rollups().flush(self.host, &mut self.rows);
        self.write_rows(now_ms);
        self.flush_net(now_ms);
    }

    /// Writes one per-app network bucket, unless nothing in it was measured or
    /// persistence is held after the clock stepped back (the store would upsert into the
    /// old timeline's rows, or a pending discard would delete it).
    fn write_net(&mut self, which: NetWrite, now_ms: i64) {
        if self.persist_from.is_some() {
            return;
        }
        let slot = match which {
            NetWrite::Open(i) => self.net.open().get(i),
            NetWrite::Closed(i) => self.net_closed.get(i),
        };
        let Some(slot) = slot.filter(|s| s.is_measured()) else {
            return;
        };
        if let Some(store) = &self.sink.store {
            let r = store.write_net_bucket(self.host, slot.start_ms, slot.to_net_bucket());
            self.store_result("network bucket", r, now_ms);
        }
    }

    /// Writes the open per-app network buckets as they are, keeping them open (sleep,
    /// pause, shutdown, and before a reset). A bucket written again later replaces this
    /// row with a superset.
    fn flush_net(&mut self, now_ms: i64) {
        for i in 0..self.net.open().len() {
            self.write_net(NetWrite::Open(i), now_ms);
        }
    }

    /// Starts per-app counting after the store's newest per-app row (the previous run's
    /// bucket, flushed at shutdown), so the bucket this run opens first does not replace
    /// that row with a part of it. A row ahead of the clock (it went back while the app
    /// was not running) is ignored rather than stall counting until the clock catches up.
    pub(super) fn seed_net_edge(&mut self, written_to_ms: Option<i64>, now_ms: i64) {
        if let Some(end) = written_to_ms.filter(|&e| e <= now_ms + NET_BUCKET_MS) {
            self.net.written_to(end);
        }
    }

    /// Writes the open per-app network buckets and closes them as they are, into the
    /// store and the hub's ring; nothing before continuous time `from_ns` counts from
    /// now on. The edge the written buckets reached stays: re-enabled within the same
    /// bucket, new samples start at its end rather than overwrite its row.
    pub(super) fn reset_net(&mut self, now_ms: i64, from_ns: u64) {
        let edge = self
            .net
            .open()
            .last()
            .map(NetSlot::end_ms)
            .max(self.net.final_to());
        self.sink.live.net_update(self.net.open(), &[], edge);
        self.flush_net(now_ms);
        self.net.close_all();
        self.net.restart_at(now_ms, from_ns);
    }

    /// Moves the open per-app buckets to the hub's ring as closed, already written, and
    /// starts a new timeline at `now` (a clock step).
    pub(super) fn close_net_as_is(&mut self, now: ClockReading) {
        self.sink.live.net_update(self.net.open(), &[], None);
        self.net.clear();
        self.net.restart_at(now.wall_ms, now.continuous_ns);
        self.sink.live.net_update(&[], &[], self.net.final_to());
    }

    /// Adds this tick's per-app and interface bytes to the open buckets, writes the ones
    /// that closed, and updates the hub's ring. `persist`: false while persistence is
    /// held after the clock stepped back (closed buckets then reach only the ring).
    pub(super) fn account_net(&mut self, t: &Tick, persist: bool) {
        let on = self.shared.net_history.load(Ordering::Acquire);
        if on != self.net_history {
            self.net_history = on;
            tracing::info!(on, "network history");
            if on {
                self.net.restart_at(t.wall_ms, t.continuous_ns);
            } else {
                self.reset_net(t.wall_ms, t.continuous_ns);
            }
        }
        if !on {
            return;
        }
        let anchor = Anchor {
            wall_ms: t.wall_ms,
            continuous_ns: t.continuous_ns,
        };
        let mut changed = false;
        if let Some(iv) = self.buf.process_net_interval() {
            self.net.add_apps(anchor, iv, self.buf.process_net());
            changed = true;
        }
        if let Some(totals) = self.buf.iface_net() {
            self.net.add_iface(anchor, &totals);
            changed = true;
        }
        self.net.close_due(
            t.wall_ms,
            net_close_grace(self.interval_ms),
            &mut self.net_closed,
        );
        if !changed && self.net_closed.is_empty() {
            return;
        }
        // The ring first: a query between the two then finds the closed bucket in the
        // ring, never a committed row next to the ring's older, open copy of it.
        self.sink
            .live
            .net_update(&self.net_closed, self.net.open(), self.net.final_to());
        if persist {
            for i in 0..self.net_closed.len() {
                self.write_net(NetWrite::Closed(i), t.wall_ms);
            }
        }
        self.net.recycle(&mut self.net_closed);
    }

    /// Sends the pending `discard_from` (D-070). Returns whether nothing is left pending.
    ///
    /// The discard deletes every gap that starts at or after its cut, the gaps this engine
    /// holds open among them (a module switched off, or a pause, while the clock was
    /// ahead). Those are opened again, at `now_ms` or the cut if that is earlier, so the
    /// span stays covered and a later close finds its row. Reopening a gap that began
    /// before the cut, and so is still open, is a no-op in the store. The step's
    /// `clock_changed` gap is written again too (see `clock_gap`).
    pub(super) fn discard_ahead(&mut self, now_ms: i64) -> bool {
        let Some(from) = self.discard_pending else {
            return true;
        };
        let Some(store) = &self.sink.store else {
            self.discard_pending = None;
            self.clock_gap = None;
            return true;
        };
        if let Err(e) = store.discard_from(self.host, from) {
            self.store_result("discard", Err(e), now_ms);
            return false;
        }
        self.discard_pending = None;
        let at = now_ms.min(from);
        let mut reopened = false;
        if let Some((start, end)) = self.clock_gap.take() {
            match Gap::host(start, Some(end.max(now_ms)), GapReason::ClockChanged) {
                Ok(gap) => {
                    self.write_gap(gap);
                    reopened = true;
                }
                Err(e) => tracing::warn!("clock step gap rejected: {e}"),
            }
        }
        if self.paused_gap_start.is_some_and(|s| s >= from) {
            self.open_gap(at, None, GapReason::Paused);
            self.paused_gap_start = Some(at);
            reopened = true;
        }
        for m in self.disabled_gaps.clone() {
            self.open_gap(at, Some(m), GapReason::ModuleDisabled);
            reopened = true;
        }
        // The discard is committed; the reopened gaps must be too, or readers see the
        // span with no gap until the next commit, and a crash loses them (D-070).
        if reopened {
            self.flush_store(now_ms);
        }
        true
    }

    pub(super) fn flush_store(&mut self, now_ms: i64) {
        if let Some(store) = &self.sink.store {
            let r = store.flush();
            self.store_result("flush", r, now_ms);
        }
    }
}

/// Which per-app network bucket `Engine::write_net` writes.
#[derive(Clone, Copy)]
enum NetWrite {
    Open(usize),
    Closed(usize),
}
