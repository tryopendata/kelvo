#![allow(clippy::too_many_lines)] // Integration scenarios keep setup and assertions together.
//! Layout changes a collector drives at runtime (`series_revision`), cardinality caps on
//! unbounded labels, and the headless agent's settings (D-116).

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use kelvo_collect::{
    Cadence, CollectError, Collector, CollectorId, LIVE_OR_IDLE, Probe, SampleBuf, Tick,
};
use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey, Settings};

#[derive(Default)]
struct RateState {
    keys: Vec<SeriesKey>,
    revision: u64,
    /// The previous counter: a rate needs one, and a hint probe forgets it.
    prev: Option<u64>,
    probes: u32,
}

/// Emits a rate for each key from a counter (the tick number), once it has a previous
/// counter. A probe after its own revision moved keeps the counter, as the contract
/// allows; any other probe resets it, as a hint probe does in a real collector.
struct RateFake {
    id: &'static str,
    probed_revision: u64,
    s: Arc<Mutex<RateState>>,
}

fn rate_fake(id: &'static str, keys: &[&str]) -> (Box<dyn Collector>, Arc<Mutex<RateState>>) {
    let s = Arc::new(Mutex::new(RateState {
        keys: keys.iter().map(|k| key(k)).collect(),
        ..RateState::default()
    }));
    let c = RateFake {
        id,
        probed_revision: 0,
        s: Arc::clone(&s),
    };
    (Box::new(c), s)
}

impl Collector for RateFake {
    fn id(&self) -> CollectorId {
        CollectorId(self.id)
    }
    fn cadence(&self) -> Cadence {
        Cadence::EveryTick
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }
    fn modules(&self) -> &'static [Module] {
        &[Module::Network]
    }
    fn probe(&mut self) -> Probe {
        let mut s = self.s.lock().unwrap();
        s.probes += 1;
        if s.revision == self.probed_revision {
            s.prev = None;
        }
        self.probed_revision = s.revision;
        Probe::Supported(s.keys.clone())
    }
    fn series_revision(&self) -> u64 {
        self.s.lock().unwrap().revision
    }
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let mut s = self.s.lock().unwrap();
        if let Some(prev) = s.prev {
            for k in &s.keys {
                out.push(k, (tick.n - prev) as f32);
            }
        }
        s.prev = Some(tick.n);
        Ok(())
    }
}

fn in_layout(h: &Harness, k: &str) -> bool {
    h.live.layout().unwrap().series.contains(&key(k))
}

/// A collector whose series set changes is re-probed alone: its sibling in the same
/// module keeps its baseline and emits a rate on the very next tick, and the new series
/// appear in the next layout.
#[test]
fn series_revision_reprobes_only_that_collector() {
    let (a, a_s) = rate_fake("fake.a", &["net.rx{iface=en0}"]);
    let (b, b_s) = rate_fake("fake.b", &["net.tx{iface=en0}"]);
    let mut h = Harness::new("revision", vec![a, b], &settings_all_on());
    h.ticks(3);
    let frames = h.frames();
    let last = frames.last().unwrap();
    assert!(value(last, "net.rx{iface=en0}", false).is_finite());
    assert!(value(last, "net.tx{iface=en0}", false).is_finite());
    let layout_before = last.layout.layout_no;
    let (a_probes, b_probes) = (a_s.lock().unwrap().probes, b_s.lock().unwrap().probes);

    // An interface appears: the collector lists it and bumps its revision.
    {
        let mut s = a_s.lock().unwrap();
        s.keys.push(key("net.rx{iface=en1}"));
        s.revision += 1;
    }
    // The tick that sees the bump still has the old layout.
    h.tick();
    assert!(!in_layout(&h, "net.rx{iface=en1}"));
    // The next one re-probes before sampling.
    h.tick();
    assert_eq!(a_s.lock().unwrap().probes, a_probes + 1);
    assert_eq!(
        b_s.lock().unwrap().probes,
        b_probes,
        "the sibling was re-probed"
    );
    let frames = h.frames();
    let last = frames.last().unwrap();
    assert!(last.layout.layout_no > layout_before);
    assert!(in_layout(&h, "net.rx{iface=en1}"));
    assert!(
        value(last, "net.tx{iface=en0}", false).is_finite(),
        "the sibling's rate was blanked"
    );
    assert!(value(last, "net.rx{iface=en0}", false).is_finite());
    assert!(value(last, "net.rx{iface=en1}", false).is_finite());

    // Steady again: no further probes.
    h.ticks(3);
    assert_eq!(a_s.lock().unwrap().probes, a_probes + 1);
    assert_eq!(b_s.lock().unwrap().probes, b_probes);
}

fn cgroup(metric: &'static str, name: &str) -> SeriesKey {
    SeriesKey::new(
        MetricId::from_static(metric),
        Labels::from_pairs([("kind", "docker"), ("name", name)]).unwrap(),
    )
}

/// `cgroup.*` is capped at 32 series per metric (D-116). The series beyond it are left
/// out of the layout, keeping the first 32 in the collector's probe order, whatever
/// their names sort to.
#[test]
fn layout_keeps_the_first_capped_series_in_probe_order() {
    // Names in reverse order, so probe order and sorted order disagree.
    let names: Vec<String> = (0..40).rev().map(|i| format!("c{i:02}")).collect();
    let mut keys: Vec<String> = names
        .iter()
        .map(|n| cgroup("cgroup.cpu", n).to_string())
        .collect();
    keys.push("cgroup.other_count".into());
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let (c, _h) = fake(
        "fake.cgroup",
        Cadence::EveryTick,
        &[Module::Containers],
        &refs,
    );
    let mut h = Harness::new("cap", vec![c], &settings_all_on());
    h.tick();
    let layout = h.live.layout().unwrap();
    let cpu: Vec<&SeriesKey> = layout
        .series
        .iter()
        .filter(|k| k.metric.as_str() == "cgroup.cpu")
        .collect();
    assert_eq!(cpu.len(), 32);
    for n in &names[..32] {
        assert!(cpu.contains(&&cgroup("cgroup.cpu", n)), "{n} was dropped");
    }
    for n in &names[32..] {
        assert!(
            !cpu.contains(&&cgroup("cgroup.cpu", n)),
            "{n} is over the cap"
        );
    }
    assert!(in_layout(&h, "cgroup.other_count"));
    // The kept series still get their values.
    let frames = h.frames();
    let last = frames.last().unwrap();
    let kept = cgroup("cgroup.cpu", &names[0]).to_string();
    assert!(value(last, &kept, false).is_finite());
}

type PerModule = (Vec<Box<dyn Collector>>, Vec<(Module, FakeHandle)>);

/// One `LIVE_OR_IDLE` collector per module, the way the per-tick collectors run. The
/// menu bar is the only thing that would pin one of them live with no window and no
/// remote viewer.
fn live_or_idle_per_module() -> PerModule {
    const IDS: [&str; 9] = [
        "fake.0", "fake.1", "fake.2", "fake.3", "fake.4", "fake.5", "fake.6", "fake.7", "fake.8",
    ];
    let mut collectors = Vec::new();
    let mut handles = Vec::new();
    for (i, m) in Module::ALL.into_iter().enumerate() {
        let modules: &'static [Module] = Box::leak(Box::new([m]));
        let (c, h) = fake(IDS[i], LIVE_OR_IDLE, modules, &[]);
        collectors.push(c);
        handles.push((m, h));
    }
    (collectors, handles)
}

/// With the agent's settings no collector is live until someone asks: every
/// `LIVE_OR_IDLE` collector stays on its idle period. The Mac's default menu bar pins
/// CPU live, which shows the check measures what it claims.
#[test]
fn agent_default_pins_no_collector_live() {
    let ticks = 15;
    let (collectors, handles) = live_or_idle_per_module();
    let mut h = Harness::new("agent-idle", collectors, &Settings::agent_default());
    h.ticks(ticks);
    for (m, fh) in &handles {
        assert!(
            fh.samples() < 6,
            "{m:?} sampled {} times in {ticks} ticks: pinned live",
            fh.samples()
        );
    }

    let (collectors, handles) = live_or_idle_per_module();
    let mut h = Harness::new("mac-live", collectors, &Settings::default());
    h.ticks(ticks);
    let cpu = &handles.iter().find(|(m, _)| *m == Module::Cpu).unwrap().1;
    assert_eq!(cpu.samples(), ticks as u32, "the menu bar pins CPU live");
}
