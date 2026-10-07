//! The per-tick path: clock checks, sampling, the frame, rollups, events and process
//! rows.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use kelvo_collect::{Cadence, Interest, Interests, ProcessSample, Tick};
use kelvo_schema::{Gap, GapReason, Tier};
use kelvo_store::ProcRow;

use super::cadence::{
    PERFORMANCE_IDLE_PROCESS_MS, PERFORMANCE_VISIBLE_MS, performance_period, performance_slowdown,
};
use super::control::NO_PROCESS_INTEREST;
use super::{Engine, hold_ms, stale_ms};
use crate::bus::{BusMsg, FrameLayout, LiveFrame, ProcessBatch};
use crate::usage::USAGE_BUCKET_MS;

/// Ticks of continuous time without a tick, and without a sleep event, after which the
/// engine records the hole as a `sleep` gap (the process was suspended, or a sleep
/// notification was missed). Fewer missing ticks are just skipped ticks.
const STALL_TICKS: i64 = 5;

/// A wall clock that moved this many base ticks more (or less) than the continuous
/// clock between two ticks was stepped (NTP, a manual change, a time zone bug), not
/// merely late (D-064). At least [`CLOCK_STEP_MIN_MS`], so ordinary NTP slews at a fast
/// interval are not steps.
const CLOCK_STEP_TICKS: i64 = 2;

/// The smallest wall-clock jump that counts as a step, whatever the interval.
const CLOCK_STEP_MIN_MS: i64 = 2_000;

/// How far the wall clock may move beyond the continuous clock before it was stepped.
pub(super) fn clock_step_threshold(interval_ms: i64) -> i64 {
    (CLOCK_STEP_TICKS * interval_ms).max(CLOCK_STEP_MIN_MS)
}

/// The longest the engine holds persistence after the wall clock stepped back (D-070).
/// Rows the old clock wrote beyond it are discarded, so a clock that ran months ahead
/// does not stop history for months.
pub(super) const MAX_PERSIST_HOLD_MS: i64 = 3_600_000;

const MINUTE_MS: i64 = 60_000;

/// Top processes kept per snapshot (architecture.md, Store).
const PROC_SNAPSHOT_TOP: usize = 30;

impl Engine {
    pub(super) fn on_tick(&mut self, t: Tick) {
        let ts = t.wall_ms;
        self.ticks += 1;
        self.update_power();

        if let Some(prev) = self.last_tick {
            let held_ns = t.continuous_ns.saturating_sub(prev.continuous_ns);
            let held_ms = i64::try_from(held_ns / 1_000_000).unwrap_or(i64::MAX);
            let interval = i64::from(self.interval_ms);
            // How far the wall clock moved beyond what actually elapsed.
            let step = (ts - prev.wall_ms).saturating_sub(held_ms);
            if step.abs() > clock_step_threshold(interval) || ts <= prev.wall_ms {
                self.on_clock_step(ts, step);
            } else if held_ms > STALL_TICKS * interval {
                // No tick and no sleep event for several intervals: nothing was measured.
                let start = prev.wall_ms + interval;
                if start < ts {
                    match Gap::host(start, Some(ts), GapReason::Sleep) {
                        Ok(gap) => self.write_gap(gap),
                        Err(e) => tracing::warn!("stall gap rejected: {e}"),
                    }
                }
                tracing::warn!(held_ms, "ticks stalled without a sleep event");
            }
        }
        self.last_tick = Some(t);

        if !self.reprobe.is_empty() {
            let which = std::mem::take(&mut self.reprobe);
            self.do_reprobe(&which);
        }

        // Sample. Process interest counts on the ticks its period is due, so a window
        // that wants rows every 5 s does not make the collector run every tick.
        let interval = self.interval_ms;
        let t = Tick {
            interval_ms: interval,
            ..t
        };
        let performance = self.performance().is_on();
        let detail = self.shared.detail.load(Ordering::Acquire) > 0;
        // Performance mode's slow idle periods also apply in the background (D-094).
        let slowed = performance || !detail;
        self.detect.set_process_idle_ms(if slowed {
            PERFORMANCE_IDLE_PROCESS_MS
        } else {
            kelvo_collect::IDLE_MS
        });
        let mut process_period = self.shared.process_period.load(Ordering::Acquire);
        if performance && process_period != NO_PROCESS_INTEREST {
            // Visible windows get process rows at most every 2 s (D-088).
            process_period = process_period.max(PERFORMANCE_VISIBLE_MS);
        }
        let wants_processes = process_period != NO_PROCESS_INTEREST;
        // Network rates join the process rows, so they are sampled on the same ticks and
        // cover the same span.
        let wants_network = wants_processes && self.shared.network.load(Ordering::Acquire);
        let wants_gpu = wants_processes && self.shared.gpu.load(Ordering::Acquire);
        let wants_ports = wants_processes && self.shared.ports.load(Ordering::Acquire);
        // Network history samples per-app bytes on every tick the processes collector
        // samples, tray-only included, and keeps the session open meanwhile (D-089). Not
        // through `wants_network`: on demand means every tick or never.
        let history = self.shared.net_history.load(Ordering::Acquire);
        // Per-app GPU over a range (D-099) needs GPU time through every usage bucket,
        // tray-only included, so outside Performance mode the GPU collector is held like
        // network history and joins a process sample at most once per 10 s bucket (each
        // 30 s background sample; every tenth 1 s one). A share covers the time since the
        // previous GPU sample, so averages over the samples stay right. A GPU view still
        // gets it on every process tick. Without the entitlement (App Store) its slot is
        // inactive and this costs nothing.
        let gpu_always = !performance;
        let gpu_always_due = gpu_always && t.continuous_ns >= self.gpu_always_next_ns;
        let processes_due = wants_processes && self.proc_every.due_with(process_period, &t);
        let interests = Interests {
            processes: processes_due,
            detail,
            // A visible window shows everything; otherwise per collector, below.
            live: detail,
            network_processes: wants_network && processes_due,
            gpu_processes: wants_gpu && processes_due,
            port_processes: wants_ports && processes_due,
        };
        // Who holds each interest, due this tick or not: an on-demand collector is
        // released only when its interest is gone.
        let held = Interests {
            processes: wants_processes,
            network_processes: wants_network || history,
            gpu_processes: wants_gpu || gpu_always,
            port_processes: wants_ports,
            ..interests
        };
        // Set once the processes collector sampled this tick; the per-process network
        // collector comes after it in the slot order.
        let mut processes_sampled = false;
        let menu_bar = &self.settings.menu_bar;
        let n = self.layout.frame.series.len();
        self.scratch.clear();
        self.scratch.resize(n, f32::NAN);
        self.span_ms.resize(n, 0);
        self.buf.clear();
        self.slot_period.resize(self.slots.len(), 0);
        self.slot_sampled_at.resize(self.slots.len(), i64::MIN);
        for ((slot, slot_period), slot_at) in self
            .slots
            .iter_mut()
            .zip(self.slot_period.iter_mut())
            .zip(self.slot_sampled_at.iter_mut())
        {
            let interests = Interests {
                live: interests.live
                    || slot
                        .collector
                        .modules()
                        .iter()
                        .any(|m| menu_bar.contains(m)),
                network_processes: interests.network_processes || (history && processes_sampled),
                gpu_processes: interests.gpu_processes || (gpu_always_due && processes_sampled),
                ..interests
            };
            let cadence = slot.collector.cadence();
            if slot.demanded
                && let Cadence::OnDemand(i) = cadence
                && !(slot.active && held.has(i))
            {
                slot.collector.release();
                slot.demanded = false;
                slot.every.reset();
                *slot_at = i64::MIN;
            }
            let mut period = cadence.period_ms(interests);
            if slowed {
                let slowdown = performance_slowdown(&*slot.collector, cadence, interests, !detail);
                period = period.map(|p| performance_period(slowdown, p));
            }
            *slot_period = period.unwrap_or(0);
            let Some(period) = period.filter(|_| slot.active) else {
                // Not sampling: its next read starts a new baseline.
                *slot_at = i64::MIN;
                continue;
            };
            if !slot.every.due_with(period, &t) {
                continue;
            }
            if matches!(cadence, Cadence::OnDemand(_)) {
                slot.demanded = true;
            }
            let start = self.buf.values().len();
            processes_sampled |= matches!(
                cadence,
                Cadence::Adaptive {
                    interest: Interest::Processes,
                    ..
                }
            );
            if let Err(e) = slot.collector.sample(&t, &mut self.buf)
                && let Some(suppressed) = slot.errors.check(ts)
            {
                tracing::warn!(
                    collector = %slot.collector.id(),
                    suppressed,
                    "sample failed: {e}"
                );
            }
            // A value from this read covers the time since the collector's previous read,
            // even when that read produced nothing for it (a counter reset, a dropped
            // rate): the collector's baseline moved then.
            let read_span = if *slot_at == i64::MIN {
                0
            } else {
                ts.saturating_sub(*slot_at).max(1)
            };
            *slot_at = ts;
            for s in self.buf.values().get(start..).unwrap_or_default() {
                let Some(&i) = self.layout.index.get(&s.key) else {
                    continue;
                };
                if let (Some(v), Some(l), Some(at), Some(span)) = (
                    self.scratch.get_mut(i),
                    self.latest.get_mut(i),
                    self.sampled_at.get_mut(i),
                    self.span_ms.get_mut(i),
                ) {
                    *v = s.value;
                    *l = s.value;
                    *span = read_span;
                    *at = ts;
                }
            }
        }
        if gpu_always_due && processes_sampled {
            // Half a tick of slack, so timer jitter does not push it a tick later.
            let wait_ms = u64::try_from(USAGE_BUCKET_MS)
                .unwrap_or(0)
                .saturating_sub(u64::from(interval) / 2);
            self.gpu_always_next_ns = t.continuous_ns.saturating_add(wait_ms * 1_000_000);
        }
        if let Some(primary) = self.buf.primary_iface()
            && *primary != self.primary_iface
        {
            self.primary_iface = primary.clone();
            self.publish_status();
        }
        let procs = if self.buf.processes().is_empty() {
            Vec::new()
        } else {
            let mut procs = self.buf.take_processes();
            if self.buf.process_net_measured() {
                self.buf.sort_process_net();
                for p in &mut procs {
                    // Absent from the network batch: no traffic over the interval.
                    let (rx, tx) = self
                        .buf
                        .net_of(p.pid)
                        .map_or((0.0, 0.0), |n| (n.rx_bps, n.tx_bps));
                    p.net_rx_bps = Some(rx);
                    p.net_tx_bps = Some(tx);
                }
            }
            if self.buf.process_gpu_measured() {
                self.buf.sort_process_gpu();
                for p in &mut procs {
                    // Absent from the GPU batch: no GPU time over the interval.
                    p.gpu_pct = Some(self.buf.gpu_of(p.pid).map_or(0.0, |g| g.pct));
                }
            }
            procs
        };

        // One allocation each for the raw values, the held values and the frame: the
        // frame is shared with the hub's ring, the bus and every subscriber.
        let values: Arc<[f32]> = Arc::from(self.scratch.as_slice());
        // A held value stays current for 2.5 times its sampling period as it is now (the
        // larger of the catalog period, its collector's current period and the base
        // tick), so a collector that drops to its idle rate does not blink to a gap.
        let slot_period = &self.slot_period;
        self.series_period.clear();
        self.series_period.extend(self.layout.meta.iter().map(|m| {
            let own = m
                .owner
                .and_then(|o| slot_period.get(o).copied())
                .unwrap_or(0);
            m.period_ms.max(own).max(interval)
        }));
        if self.holds.len() != self.series_period.len()
            || self
                .holds
                .iter()
                .zip(&self.series_period)
                .any(|(&h, &p)| h != hold_ms(p))
        {
            self.holds = self.series_period.iter().map(|&p| hold_ms(p)).collect();
        }
        let held: Arc<[f32]> = self
            .latest
            .iter()
            .zip(&self.sampled_at)
            .zip(&self.series_period)
            .map(|((&v, &at), &period)| {
                if at.saturating_add(stale_ms(period)) >= ts {
                    v
                } else {
                    f32::NAN
                }
            })
            .collect();
        let frame = Arc::new(LiveFrame {
            ts_ms: ts,
            interval_ms: interval,
            layout: Arc::clone(&self.layout.frame),
            timeline: self.timeline,
            values,
            held,
            holds: Arc::clone(&self.holds),
        });
        self.sink.live.publish(BusMsg::Frame(Arc::clone(&frame)));

        // Rollups, unless the wall clock stepped back over buckets already written.
        if let Some(from) = self.persist_from {
            // A discard the store has not confirmed keeps the hold: persisting now would
            // upsert into the old clock's rows.
            if ts < from || !self.discard_ahead(ts) {
                self.account_net(&t, false);
                if !procs.is_empty() {
                    self.publish_processes(ts, procs);
                }
                return;
            }
            self.persist_from = None;
            self.publish_status();
            tracing::info!("wall clock passed the held span; persisting again");
        }
        self.account_net(&t, true);
        let persisted = &self.layout.persisted;
        let (meta, spans, periods) = (&self.layout.meta, &self.span_ms, &self.series_period);
        // Each value with its rollup weight (D-092): for a span average, the span it
        // covers (`span_ms`; its current period when the read was a new baseline); for a
        // gauge, 1.
        let vals = self.layout.persisted_idx.iter().map(|&i| {
            let v = frame.values.get(i).copied().unwrap_or(f32::NAN);
            let weight = match (meta.get(i), spans.get(i), periods.get(i)) {
                (Some(m), Some(&span), _) if m.span_weighted && span > 0 => span as f64,
                (Some(m), _, Some(&period)) if m.span_weighted => f64::from(period.max(1)),
                _ => 1.0,
            };
            (v, weight)
        });
        let minute_closed =
            self.sink
                .live
                .rollups()
                .add(self.host, ts, persisted, vals, &mut self.rows);
        self.write_rows(ts);
        if minute_closed {
            tracing::info!(
                ticks = self.ticks,
                series = n,
                persisted = self.layout.persisted.len(),
                layout_no = self.layout.frame.layout_no,
                interval_ms = self.interval_ms,
                "engine minute"
            );
        }

        let batch = (!procs.is_empty()).then_some(procs.as_slice());
        self.detect_events(ts, &frame.values, batch);

        if !procs.is_empty() {
            self.on_processes(ts, procs);
        }
    }

    /// Runs the detectors and alert rules on a persisted tick. Each event is queued to
    /// the store with a commit request behind it, so readers see it within one writer
    /// round trip rather than at the next batch commit (D-083), and published on the bus
    /// right away, before that commit lands.
    fn detect_events(&mut self, ts: i64, values: &[f32], batch: Option<&[ProcessSample]>) {
        let mut events = std::mem::take(&mut self.events);
        self.detect
            .on_tick(ts, values, &self.series_period, batch, &mut events);
        if !events.is_empty() {
            if let Some(store) = &self.sink.store {
                let r = events
                    .iter()
                    .try_for_each(|e| store.record_event(self.host, e))
                    .and_then(|()| store.commit_soon());
                self.store_result("event", r, ts);
            }
            for e in events.drain(..) {
                tracing::info!(kind = e.detail.kind(), processes = ?e.processes, "event");
                self.sink.live.publish(BusMsg::Event(Arc::new(e)));
            }
        }
        self.events = events;
    }

    /// The wall clock moved `step` ms more than the continuous clock since the last tick
    /// (negative: it went back), and now reads `ts`. Treated like a sleep: the open buckets
    /// are flushed and dropped, and the hole gets a `clock_changed` gap. Going back,
    /// nothing is persisted until the clock passes the newest bucket already written
    /// (D-064). Frames from here on carry the next `timeline`, which tells subscribers to
    /// drop what they hold from the stepped time on; the hub's ring does the same when the
    /// stepped frame arrives.
    pub(super) fn on_clock_step(&mut self, ts: i64, step: i64) {
        tracing::warn!(step_ms = step, "wall clock stepped");
        self.flush_buckets(ts - step);
        let written_to = {
            let mut rollups = self.sink.live.rollups();
            let end = rollups.bucket_end();
            rollups.reset();
            if step < 0 {
                rollups.drop_after(ts);
            }
            end.into_iter().chain(self.persist_from).max()
        };
        self.detect.reset();
        self.last_proc_bucket = None;
        // `flush_buckets` wrote the open per-app buckets; a sample reaching back before
        // the step would put time that was not lived on the new timeline.
        self.close_net_as_is(self.ticker.now());
        if step < 0 {
            self.sink.live.net_drop_after(ts);
            self.sink.live.usage_drop_after(ts);
        }
        for at in self.sampled_at.iter_mut().chain(&mut self.slot_sampled_at) {
            if *at != i64::MIN {
                *at = at.saturating_add(step);
            }
        }
        let gap = if step > 0 {
            // Forward: the minutes the clock skipped were never lived.
            Gap::host(ts - step, Some(ts), GapReason::ClockChanged)
        } else {
            let written_to = written_to.unwrap_or(ts).max(ts);
            // Hold for at most an hour, ending on a minute boundary so the first minute
            // written again is whole (D-070).
            let cap = ts
                .saturating_add(MAX_PERSIST_HOLD_MS - 1)
                .div_euclid(MINUTE_MS)
                .saturating_mul(MINUTE_MS)
                .saturating_add(MINUTE_MS);
            let until = written_to.min(cap);
            if written_to > until {
                // The old clock wrote past the hold: drop those rows so the new timeline
                // never upserts into them.
                tracing::warn!(
                    from_ms = until,
                    to_ms = written_to,
                    "discarding history the wall clock wrote ahead of itself"
                );
                self.discard_pending = Some(self.discard_pending.map_or(until, |p| p.min(until)));
            }
            // A pending discard keeps the hold even when this step needs none: the next
            // tick past it retries the discard before anything is persisted again.
            self.persist_from = (until > ts || self.discard_pending.is_some()).then_some(until);
            Gap::host(ts, Some(until), GapReason::ClockChanged)
        };
        match gap {
            Ok(gap) if gap.end_ms.is_some_and(|e| e > gap.start_ms) => {
                let end = gap.end_ms.unwrap_or(gap.start_ms);
                // While a discard is pending nothing is persisted, so an earlier step's
                // gap that began before this one widens to cover both instead of being
                // forgotten.
                self.clock_gap = self.discard_pending.map(|_| match self.clock_gap {
                    Some((s, e)) if s < gap.start_ms => (s, e.max(end)),
                    _ => (gap.start_ms, end),
                });
                self.write_gap(gap);
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("clock step gap rejected: {e}"),
        }
        // After the gap: the discard commits what was queued before it, the gap with it.
        self.discard_ahead(ts);
        self.timeline = self.timeline.wrapping_add(1);
        self.bump_layout();
        self.publish_status();
    }

    /// Republishes the current series under a new layout number after the wall clock
    /// stepped. Subscribers reset on the frame's `timeline`, not on this.
    fn bump_layout(&mut self) {
        let frame = Arc::new(FrameLayout {
            layout_no: self.layout.frame.layout_no + 1,
            series: Arc::clone(&self.layout.frame.series),
        });
        self.layout.frame = Arc::clone(&frame);
        self.sink.live.publish(BusMsg::Layout(frame));
    }

    fn on_processes(&mut self, ts: i64, procs: Vec<ProcessSample>) {
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

    fn publish_processes(&self, ts: i64, procs: Vec<ProcessSample>) {
        self.sink
            .live
            .publish(BusMsg::Processes(Arc::new(ProcessBatch {
                ts_ms: ts,
                rows: procs,
            })));
    }
}
