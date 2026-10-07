//! Sleep and wake, pause, settings, attaching another store, and shutdown.

use kelvo_schema::{GapReason, Module, Settings};
use kelvo_store::Writer;

use super::tick::clock_step_threshold;
use super::{Engine, EngineSettings};
use crate::clock::ClockReading;
use crate::inbox::SleepAck;
use crate::power::PowerEvent;

impl Engine {
    pub(super) fn on_power(&mut self, event: PowerEvent, at: ClockReading, ack: Option<SleepAck>) {
        match event {
            PowerEvent::WillSleep => {
                if self.asleep.is_none() && self.started && !self.stopped {
                    if self.running() {
                        self.flush_buckets(at.wall_ms);
                        self.ticker.stop();
                        self.release_on_demand();
                        self.open_gap(at.wall_ms, None, GapReason::Sleep);
                        self.sleep_gap_start = Some(at.wall_ms);
                    }
                    self.flush_store(at.wall_ms);
                    self.asleep = Some(at);
                    self.publish_status();
                    tracing::info!("sleeping");
                }
            }
            PowerEvent::DidWake => {
                if let Some(since) = self.asleep.take() {
                    // The wall clock may have been stepped during sleep; the continuous
                    // clock measures what actually elapsed.
                    let end = since.wall_ms + at.continuous_ms_since(&since);
                    // The wall clock stepped during sleep: handled like a step between
                    // two ticks once the sleep gap is closed.
                    let step = at.wall_ms - end;
                    let stepped = step.abs() > clock_step_threshold(i64::from(self.interval_ms));
                    if let Some(start) = self.sleep_gap_start.take() {
                        self.close_gap(GapReason::Sleep, None, Some(start), end);
                        if self.paused {
                            self.open_gap(end, None, GapReason::Paused);
                            self.paused_gap_start = Some(end);
                        }
                        // Commit the closed gap, and the pause that follows it, now: with
                        // a 5-minute commit interval, "last woke" would otherwise lag the
                        // wake and the pause would not show (D-070).
                        self.flush_store(end);
                    }
                    tracing::info!(slept_ms = end - since.wall_ms, "woke");
                    if stepped {
                        self.on_clock_step(at.wall_ms, step);
                    }
                    self.resume_sampling();
                    self.publish_status();
                } else {
                    // A wake without a sleep we saw: devices may still have changed.
                    self.reprobe.all = true;
                }
            }
        }
        if let Some(ack) = ack {
            ack.done();
        }
    }

    /// Releases every on-demand collector that holds something open (the NetworkStatistics
    /// manager, the GPU client counters). Ticks release them when their interest ends,
    /// but none come while paused or asleep, and a view left open must not keep them
    /// held then (D-082). The next sample after resuming is a fresh baseline.
    fn release_on_demand(&mut self) {
        for (slot, at) in self.slots.iter_mut().zip(self.slot_sampled_at.iter_mut()) {
            if slot.demanded {
                slot.collector.release();
                slot.demanded = false;
                slot.every.reset();
                *at = i64::MIN;
            }
        }
    }

    /// After sleep or pause: reset rate state through a re-probe, restart the ticker.
    fn resume_sampling(&mut self) {
        if !self.running() {
            return;
        }
        self.reprobe.all = true;
        self.last_tick = None;
        self.detect.reset();
        for slot in &mut self.slots {
            slot.every.reset();
        }
        // The span asleep or paused was not measured; the open buckets keep what was.
        let now = self.ticker.now();
        self.net.restart_at(now.wall_ms, now.continuous_ns);
        self.start_ticker();
    }

    pub(super) fn set_paused(&mut self, paused: bool) {
        if paused == self.paused || !self.started || self.stopped {
            return;
        }
        let now = self.ticker.now();
        if paused {
            if self.asleep.is_none() {
                self.flush_buckets(now.wall_ms);
                self.ticker.stop();
                self.release_on_demand();
                self.open_gap(now.wall_ms, None, GapReason::Paused);
                self.paused_gap_start = Some(now.wall_ms);
                // Commit the open now: with a 5-minute commit interval, history would
                // otherwise show no pause for up to that long (D-070).
                self.flush_store(now.wall_ms);
            }
            self.paused = true;
        } else {
            self.paused = false;
            let closed = self.paused_gap_start.take();
            if let Some(start) = closed {
                self.close_gap(GapReason::Paused, None, Some(start), now.wall_ms);
            }
            if self.asleep.is_some() {
                self.open_gap(now.wall_ms, None, GapReason::Sleep);
                self.sleep_gap_start = Some(now.wall_ms);
            }
            // Readers would otherwise draw the open pause over new samples until the
            // next commit, up to 5 minutes away (D-070).
            if closed.is_some() || self.asleep.is_some() {
                self.flush_store(now.wall_ms);
            }
            self.resume_sampling();
        }
        self.publish_status();
    }

    pub(super) fn sync_disabled_gaps(&mut self, now_ms: i64) {
        let want = self.settings.disabled.clone();
        let opened: Vec<Module> = want.difference(&self.disabled_gaps).copied().collect();
        let closed: Vec<Module> = self.disabled_gaps.difference(&want).copied().collect();
        // Commit the change at once: an open `module_disabled` gap left in the batch
        // would read as still open over new samples until the next commit (D-070).
        let changed = !opened.is_empty() || !closed.is_empty();
        for m in opened {
            self.open_gap(now_ms, Some(m), GapReason::ModuleDisabled);
        }
        for m in closed {
            self.close_gap(GapReason::ModuleDisabled, Some(m), None, now_ms);
            self.reprobe.modules.insert(m);
        }
        self.disabled_gaps = want;
        if changed {
            self.flush_store(now_ms);
        }
    }

    pub(super) fn apply_settings(&mut self, settings: &Settings) {
        let next = EngineSettings::from_settings(settings);
        if next == self.settings {
            return;
        }
        if next.alerts != self.settings.alerts {
            self.detect.set_alerts(next.alerts);
        }
        let was_network_off = self.settings.disabled.contains(&Module::Network);
        let cadence_only = EngineSettings {
            menu_bar: next.menu_bar.clone(),
            alerts: next.alerts,
            performance_mode: next.performance_mode,
            ..self.settings.clone()
        } == next;
        let performance_changed = next.performance_mode != self.settings.performance_mode;
        self.settings = next;
        if cadence_only {
            // The menu bar and Performance mode change cadences from the next tick and
            // the alert rules are swapped above. Performance mode can also change the
            // battery back-off and is in the status.
            if performance_changed {
                self.apply_interval();
                self.publish_status();
            }
            return;
        }
        let now = self.ticker.now();
        if self.settings.disabled.contains(&Module::Network) != was_network_off {
            // The network collectors start or stop; buckets restart from here.
            self.reset_net(now.wall_ms, now.continuous_ns);
        }
        self.sync_disabled_gaps(now.wall_ms);
        self.rebuild_layout();
        self.apply_interval();
        self.publish_status();
    }

    /// Closes, in the store, every gap the engine holds open, at `now_ms`. The flags
    /// that say which state the engine is in (paused, asleep, disabled modules) stay.
    fn close_open_gaps(&mut self, now_ms: i64) {
        if let Some(start) = self.paused_gap_start.take() {
            self.close_gap(GapReason::Paused, None, Some(start), now_ms);
        }
        if let Some(start) = self.sleep_gap_start.take() {
            self.close_gap(GapReason::Sleep, None, Some(start), now_ms);
        }
        for m in std::mem::take(&mut self.disabled_gaps) {
            self.close_gap(GapReason::ModuleDisabled, Some(m), None, now_ms);
        }
    }

    pub(super) fn set_store(&mut self, store: Option<Writer>) {
        if !self.started || self.stopped {
            self.sink.store = store;
            return;
        }
        let now = self.ticker.now().wall_ms;
        // Leave the old store consistent: what was measured, and no gap left open.
        if self.running() {
            self.flush_buckets(now);
        }
        self.close_open_gaps(now);
        // The old store has the open per-app buckets (flushed above, or at the pause or
        // sleep); the new one (history was reset) starts from here, and the ring forgets
        // what the old one had.
        self.net.clear();
        let at = self.ticker.now();
        self.net.restart_at(at.wall_ms, at.continuous_ns);
        self.sink.live.net_clear();
        self.sink.live.net_update(&[], &[], self.net.final_to());
        self.sink.live.rollups().clear();
        self.flush_store(now);
        self.sink.store = store;
        self.detect.reset();
        // The hold protected the old file's buckets; another file has none of them.
        self.discard_pending = None;
        self.clock_gap = None;
        if self.persist_from.take().is_some() {
            self.publish_status();
        }
        let Some(store) = &self.sink.store else {
            tracing::info!("store detached; running live-only");
            return;
        };
        match store.begin_session(self.host, now) {
            Ok(s) => {
                let written_to = s.net_written_to_ms;
                self.seed_net_edge(written_to, now);
            }
            Err(e) => tracing::error!("store session start failed: {e}"),
        }
        if self.paused {
            self.open_gap(now, None, GapReason::Paused);
            self.paused_gap_start = Some(now);
        } else if self.asleep.is_some() {
            self.open_gap(now, None, GapReason::Sleep);
            self.sleep_gap_start = Some(now);
        }
        let disabled = self.settings.disabled.clone();
        for &m in &disabled {
            self.open_gap(now, Some(m), GapReason::ModuleDisabled);
        }
        self.disabled_gaps = disabled;
        tracing::info!("store attached");
    }

    pub(super) fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        let now = self.ticker.now();
        if self.started {
            if self.running() {
                self.flush_buckets(now.wall_ms);
            }
            self.ticker.stop();
            self.close_open_gaps(now.wall_ms);
            self.flush_store(now.wall_ms);
        }
        self.stopped = true;
        self.force_publish_status();
        tracing::info!("engine stopped");
    }
}
