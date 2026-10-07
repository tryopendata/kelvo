//! The frame layout and capabilities, from the collectors' probes and the module
//! switches.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use kelvo_collect::{Cadence, Interest, Interests, Probe};
use kelvo_schema::{Capabilities, MetricKind, Module, ModuleCap, SeriesKey};

use super::Engine;
use super::cadence::{
    background_interval_ms, backoff_interval_ms, performance_period, performance_slowdown,
};
use super::control::lock;
use crate::bus::{BusMsg, FrameLayout};

/// Series not gated by a module switch (catalog: "nothing gates it on the CPU module").
const UNGATED: &[&str] = &["self.cpu"];

pub(super) struct SeriesMeta {
    /// Nominal minimum period from the catalog, in ms.
    pub(super) period_ms: u32,
    /// A `Mean` or `Rate` series: its rollup weighs each sample by its span (D-092).
    pub(super) span_weighted: bool,
    /// The slot whose collector produces the series.
    pub(super) owner: Option<usize>,
}

/// The current layout plus what the hot path needs to fill it.
pub(super) struct Layout {
    pub(super) frame: Arc<FrameLayout>,
    pub(super) index: HashMap<SeriesKey, usize>,
    pub(super) meta: Vec<SeriesMeta>,
    /// Persisted series, in layout order: the store layout the accumulators write.
    pub(super) persisted: Arc<[SeriesKey]>,
    pub(super) persisted_idx: Vec<usize>,
}

impl Layout {
    pub(super) fn empty() -> Self {
        Self {
            frame: Arc::new(FrameLayout {
                layout_no: 0,
                series: Arc::from(Vec::new()),
            }),
            index: HashMap::new(),
            meta: Vec::new(),
            persisted: Arc::from(Vec::new()),
            persisted_idx: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(super) struct Reprobe {
    pub(super) all: bool,
    pub(super) modules: BTreeSet<Module>,
}

impl Reprobe {
    pub(super) fn is_empty(&self) -> bool {
        !self.all && self.modules.is_empty()
    }
}

impl Engine {
    fn series_enabled(&self, key: &SeriesKey) -> Option<bool> {
        let def = self.catalog.validate(key).ok()?;
        let ungated = UNGATED.contains(&def.id.as_str());
        Some(ungated || !self.settings.disabled.contains(&def.module))
    }

    /// Recomputes the layout from the last probes and the module switches. Publishes a
    /// new layout (before any frame uses it) when the series set changed.
    pub(super) fn rebuild_layout(&mut self) {
        let mut series: Vec<SeriesKey> = self
            .slots
            .iter()
            .flat_map(|s| s.keys().iter())
            .filter(|k| match self.series_enabled(k) {
                Some(enabled) => enabled,
                None => {
                    tracing::debug!(key = %k, "series not in the catalog; skipped");
                    false
                }
            })
            .cloned()
            .collect();
        series.sort();
        series.dedup();

        let changed = *self.layout.frame.series != *series || self.layout.frame.layout_no == 0;
        if changed {
            let layout_no = self.layout.frame.layout_no + 1;
            let series: Arc<[SeriesKey]> = series.into();
            let index: HashMap<SeriesKey, usize> = series
                .iter()
                .enumerate()
                .map(|(i, k)| (k.clone(), i))
                .collect();
            let mut meta = Vec::with_capacity(series.len());
            let mut persisted = Vec::new();
            let mut persisted_idx = Vec::new();
            for (i, k) in series.iter().enumerate() {
                let def = self.catalog.validate(k).ok();
                meta.push(SeriesMeta {
                    period_ms: def.map_or(0, |d| u32::from(d.period_s) * 1_000),
                    span_weighted: def
                        .is_some_and(|d| matches!(d.kind, MetricKind::Mean | MetricKind::Rate)),
                    owner: self.slots.iter().position(|s| s.keys().contains(k)),
                });
                if def.is_some_and(|d| d.persisted) {
                    persisted.push(k.clone());
                    persisted_idx.push(i);
                }
            }
            // Carry the latest values over by key.
            let mut latest = vec![f32::NAN; series.len()];
            let mut sampled = vec![i64::MIN; series.len()];
            for (old_i, k) in self.layout.frame.series.iter().enumerate() {
                if let (Some(&new_i), Some(&v), Some(&f)) = (
                    index.get(k),
                    self.latest.get(old_i),
                    self.sampled_at.get(old_i),
                ) && let (Some(l), Some(fr)) = (latest.get_mut(new_i), sampled.get_mut(new_i))
                {
                    *l = v;
                    *fr = f;
                }
            }
            self.latest = latest;
            self.sampled_at = sampled;
            self.layout = Layout {
                frame: Arc::new(FrameLayout { layout_no, series }),
                index,
                meta,
                persisted: persisted.into(),
                persisted_idx,
            };
            self.detect.bind(&self.layout.frame.series);
            self.sink
                .live
                .publish(BusMsg::Layout(Arc::clone(&self.layout.frame)));
            tracing::info!(
                layout_no,
                series = self.layout.frame.series.len(),
                "layout changed"
            );
        }
        for slot in &mut self.slots {
            slot.active = match &slot.probe {
                Some(Probe::Supported(keys)) if keys.is_empty() => slot
                    .collector
                    .modules()
                    .iter()
                    .all(|m| !self.settings.disabled.contains(m)),
                _ => slot
                    .keys()
                    .iter()
                    .any(|k| self.layout.index.contains_key(k)),
            };
        }
        // With no network collector sampling, nobody keeps the primary interface current.
        if self.primary_iface.is_some()
            && !self
                .slots
                .iter()
                .any(|s| s.active && s.collector.modules().contains(&Module::Network))
        {
            self.primary_iface = None;
            self.publish_status();
        }
        self.publish_history_periods();
    }

    /// Each persisted series' slowest period under the current settings, for the hold
    /// `query_history` reports (D-092): the longest of its catalog period, its
    /// collector's period with nothing shown (slowed further, as in the background or
    /// Performance mode) and the slowest base tick: the background tick or the backed-off
    /// one (D-094).
    fn publish_history_periods(&self) {
        let floor = background_interval_ms(backoff_interval_ms(self.settings.interval_ms.max(100)));
        let idle = Interests::default();
        let periods = self.layout.persisted_idx.iter().filter_map(|&i| {
            let key = self.layout.frame.series.get(i)?;
            let meta = self.layout.meta.get(i)?;
            let own = meta
                .owner
                .and_then(|o| self.slots.get(o))
                .map_or(0, |slot| {
                    let cadence = slot.collector.cadence();
                    let period = cadence.period_ms(idle).unwrap_or(0);
                    performance_period(
                        performance_slowdown(&*slot.collector, cadence, idle, true),
                        period,
                    )
                });
            Some((key.clone(), meta.period_ms.max(own).max(floor)))
        });
        let now = self.ticker.now().wall_ms;
        self.sink.live.rollups().set_periods(periods, floor, now);
    }

    pub(super) fn do_reprobe(&mut self, which: &Reprobe) {
        for (slot, at) in self.slots.iter_mut().zip(self.slot_sampled_at.iter_mut()) {
            let modules = slot.collector.modules();
            let hit = which.all || modules.iter().any(|m| which.modules.contains(m));
            if hit {
                slot.probe = Some(slot.collector.probe());
                slot.every.reset();
                // A probe resets the collector's rate state: its next read is a baseline.
                *at = i64::MIN;
            }
        }
        self.rebuild_layout();
        self.update_caps();
    }

    pub(super) fn update_caps(&mut self) {
        let mut modules: BTreeMap<Module, ModuleCap> = BTreeMap::new();
        let mut counts: BTreeMap<Module, u32> = BTreeMap::new();
        for slot in &self.slots {
            let Some(probe) = &slot.probe else { continue };
            match probe {
                Probe::Supported(keys) => {
                    for m in slot.collector.modules() {
                        counts.entry(*m).or_insert(0);
                    }
                    for k in keys {
                        if let Ok(def) = self.catalog.validate(k) {
                            *counts.entry(def.module).or_insert(0) += 1;
                        }
                    }
                }
                Probe::Unsupported { reason } => {
                    for m in slot.collector.modules() {
                        match modules.get(m) {
                            Some(ModuleCap::Unsupported(_)) => {}
                            _ => {
                                modules.insert(*m, ModuleCap::Unsupported(*reason));
                            }
                        }
                    }
                }
                Probe::NotPresent => {
                    for m in slot.collector.modules() {
                        modules.entry(*m).or_insert(ModuleCap::NotPresent);
                    }
                }
            }
        }
        for (m, series) in counts {
            modules.insert(m, ModuleCap::Available { series });
        }
        // A collector that can supply network rates whenever a view asks (D-081).
        let process_network = self.slots.iter().any(|s| {
            s.collector.cadence() == Cadence::OnDemand(Interest::NetworkProcesses)
                && matches!(s.probe, Some(Probe::Supported(_)))
        });
        let process_gpu = self.slots.iter().any(|s| {
            s.collector.cadence() == Cadence::OnDemand(Interest::GpuProcesses)
                && matches!(s.probe, Some(Probe::Supported(_)))
        });
        let mut caps = lock(&self.shared.caps);
        if caps.modules != modules
            || caps.process_network != process_network
            || caps.process_gpu != process_gpu
        {
            let next = Arc::new(Capabilities {
                modules,
                revision: caps.revision + 1,
                process_network,
                process_gpu,
            });
            *caps = Arc::clone(&next);
            drop(caps);
            self.sink.live.publish(BusMsg::Caps(next));
        }
    }
}
