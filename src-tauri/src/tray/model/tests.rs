use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kelvo_engine::EngineStatus;
use kelvo_schema::settings::{ItemSettings, ReadoutSettings, TemperatureUnit};
use kelvo_schema::{Labels, MetricId, SeriesKey};

use super::pacer::{EARLY, GRACE};
use super::*;

fn key(metric: &str, labels: &[(&str, &str)]) -> SeriesKey {
    SeriesKey::new(
        MetricId::parse(metric).unwrap(),
        Labels::from_pairs(labels.iter().copied()).unwrap(),
    )
}

fn sample() -> Readings {
    Readings {
        cpu: Reading::Value(18.2),
        gpu: Reading::Value(36.0),
        mem: Reading::Value(42.4),
        temp_c: Reading::Value(61.3),
        watts: Reading::Value(14.84),
        battery: Reading::Value(87.0),
        net_bps: Reading::Value(38_400_000.0),
        net_rx_bps: Reading::Value(37_200_000.0),
        net_tx_bps: Reading::Value(1_200_000.0),
        disk_bps: Reading::Value(220_000_000.0),
        disk_used_pct: Reading::Value(61.6),
        // 10 P cores and 4 E cores.
        cores: vec![
            [34.0, 22.0, 41.0, 18.0, 12.0, 9.0, 27.0, 15.0, 8.0, 6.0]
                .map(Reading::Value)
                .to_vec(),
            [52.0, 38.0, 44.0, 29.0].map(Reading::Value).to_vec(),
        ],
    }
}

/// The combined item's content (these settings always have one).
fn combined(r: &Readings, s: &Settings, paused: bool, scale: u32) -> TrayContent {
    let items = build(r, &TrayHistory::default(), s, paused, scale);
    items
        .into_iter()
        .find(|i| i.key == ItemKey::Combined)
        .expect("a combined item")
        .content
}

fn own(items: &[TrayItem], module: Module) -> &TrayContent {
    &items
        .iter()
        .find(|i| i.key == ItemKey::Own(module))
        .unwrap_or_else(|| panic!("no own item for {module:?}"))
        .content
}

fn set(s: &mut Settings, module: Module, mode: ItemMode) {
    *s.menu_bar.items.get_mut(module).unwrap() = mode;
}

fn no_bars(s: &mut Settings) {
    s.menu_bar.bars = BarSettings {
        cpu: false,
        gpu: false,
        memory: false,
    };
}

/// The readouts' markers and texts, in order.
fn readouts(c: &TrayContent) -> Vec<(Marker, String)> {
    c.frame
        .readouts
        .iter()
        .map(|r| match r {
            ReadoutFrame::Marked { marker, text } => (*marker, text.clone()),
            ReadoutFrame::Graph(Graph::Rates { up, down }) => {
                (Marker::Label("NET"), format!("{up}|{down}"))
            }
            ReadoutFrame::Graph(g) => panic!("unexpected readout graph {g:?}"),
        })
        .collect()
}

#[test]
fn resolves_series_and_reads_the_totals() {
    let layout = FrameLayout {
        layout_no: 3,
        series: Arc::from(vec![
            key("cpu.total", &[]),
            key("cpu.load", &[("core", "P0")]),
            key("net.rx", &[("iface", "en0")]),
            key("net.rx_total", &[]),
            key("net.tx_total", &[]),
            key("thermal.hottest", &[]),
        ]),
    };
    let s = TraySeries::resolve(&layout, Some("/"));
    assert_eq!(s.layout_no, Some(3));
    let r = s.readings(&[18.0, 50.0, 7.0, 100.0, 20.0, f32::NAN]);
    assert_eq!(r.cpu, Reading::Value(18.0));
    assert_eq!(r.net_bps, Reading::Value(120.0));
    assert_eq!(r.net_rx_bps, Reading::Value(100.0), "the total, not en0");
    assert_eq!(r.temp_c, Reading::Gap, "a stale value is a gap");
    assert_eq!(r.gpu, Reading::Absent);
    assert_eq!(r.disk_bps, Reading::Absent);
    assert_eq!(r.disk_used_pct, Reading::Absent);
    // One direction stale: the labelled value is a gap, never rx alone.
    let r = s.readings(&[18.0, 50.0, 7.0, 100.0, f32::NAN, 61.0]);
    assert_eq!(r.net_bps, Reading::Gap);
    assert_eq!(r.net_rx_bps, Reading::Value(100.0));
    assert_eq!(r.net_tx_bps, Reading::Gap);
}

/// The Disk readout is the boot volume's used share; another volume, or no known boot
/// volume, does not stand in for it.
#[test]
fn disk_used_reads_the_boot_volume() {
    let layout = FrameLayout {
        layout_no: 4,
        series: Arc::from(vec![
            key("disk.used", &[("vol", "/Volumes/Ext")]),
            key("disk.total", &[("vol", "/Volumes/Ext")]),
            key("disk.used", &[("vol", "/")]),
            key("disk.total", &[("vol", "/")]),
        ]),
    };
    let held = [1.0, 2.0, 250e9, 1000e9];
    let r = TraySeries::resolve(&layout, Some("/")).readings(&held);
    assert_eq!(r.disk_used_pct, Reading::Value(25.0));
    let r = TraySeries::resolve(&layout, None).readings(&held);
    assert_eq!(r.disk_used_pct, Reading::Absent);
    // A stale total, or a zero one, is a gap, never a division by zero.
    let s = TraySeries::resolve(&layout, Some("/"));
    assert_eq!(
        s.readings(&[1.0, 2.0, 250e9, f32::NAN]).disk_used_pct,
        Reading::Gap
    );
    assert_eq!(
        s.readings(&[1.0, 2.0, 250e9, 0.0]).disk_used_pct,
        Reading::Gap
    );
}

#[test]
fn combined_default_quantizes_bars_to_device_pixels() {
    let c = combined(&sample(), &Settings::default(), false, 2);
    // 14 pt at 2x is 28 px: 18.2% -> 5, 36% -> 10, 42.4% -> 12.
    assert_eq!(c.frame.bars, vec![Some(5), Some(10), Some(12)]);
    assert_eq!(readouts(&c), [(Marker::None, "142°".into())]);
    assert!(c.frame.values.is_empty());
    assert_eq!(
        c.accessibility,
        "CPU 18 percent, GPU 36 percent, memory 42 percent, temperature 142 degrees"
    );
    let c1 = combined(&sample(), &Settings::default(), false, 1);
    assert_eq!(c1.frame.bars, vec![Some(3), Some(5), Some(6)]);
}

/// Every readout, in `Readout::ALL` order, with its marker (D-102).
#[test]
fn readouts_follow_the_bars_with_their_markers() {
    let mut s = Settings::default();
    s.modules.get_mut(&Module::Disk).unwrap().enabled = true;
    s.menu_bar.readouts = ReadoutSettings {
        cpu: true,
        gpu: true,
        memory: true,
        temperature: true,
        power: true,
        network: true,
        disk: true,
        battery: true,
    };
    s.units.temperature = TemperatureUnit::Celsius;
    let c = combined(&sample(), &s, false, 2);
    assert_eq!(c.frame.bars.len(), 3);
    assert_eq!(
        readouts(&c),
        [
            (Marker::Label("CPU"), "18%".into()),
            (Marker::Label("GPU"), "36%".into()),
            (Marker::Label("MEM"), "42%".into()),
            (Marker::None, "61°".into()),
            (Marker::Glyph(Glyph::Bolt), "14.8W".into()),
            (
                Marker::Label("NET"),
                "1.2 MB/s \u{2191}|37.2 MB/s \u{2193}".into()
            ),
            (Marker::Glyph(Glyph::Drive), "62%".into()),
            (Marker::Label("BAT"), "87%".into()),
        ]
    );
    assert_eq!(
        c.accessibility,
        "CPU 18 percent, GPU 36 percent, memory 42 percent, CPU 18 percent, \
         GPU 36 percent, memory 42 percent, temperature 61 degrees, power 14.8 watts, \
         network up 1.2 megabytes per second, down 37.2 megabytes per second, \
         disk 62 percent used, battery 87 percent"
    );
}

/// The old "Values" style: no bars, and "61°" needs no label of its own.
#[test]
fn values_without_bars() {
    let mut s = Settings::default();
    no_bars(&mut s);
    s.menu_bar.readouts.cpu = true;
    s.menu_bar.readouts.memory = true;
    let c = combined(&sample(), &s, false, 2);
    assert!(c.frame.bars.is_empty());
    assert_eq!(
        readouts(&c),
        [
            (Marker::Label("CPU"), "18%".into()),
            (Marker::Label("MEM"), "42%".into()),
            (Marker::None, "142°".into()),
        ]
    );
}

#[test]
fn disabled_absent_and_hidden_modules_are_not_drawn() {
    let mut s = Settings::default();
    s.modules.get_mut(&Module::Gpu).unwrap().enabled = false;
    s.menu_bar.readouts.gpu = true;
    s.menu_bar.readouts.battery = true;
    // Disk is off by default: its readout draws nothing.
    s.menu_bar.readouts.disk = true;
    let r = Readings {
        battery: Reading::Absent,
        ..sample()
    };
    let c = combined(&r, &s, false, 2);
    assert_eq!(c.frame.bars.len(), 2);
    assert_eq!(readouts(&c), [(Marker::None, "142°".into())]);

    // Power & Sensors off takes the temperature with it.
    s.modules.get_mut(&Module::Power).unwrap().enabled = false;
    no_bars(&mut s);
    let c = combined(&r, &s, false, 2);
    assert_eq!(
        c.frame.bars,
        vec![None; 3],
        "never an empty, unclickable item"
    );
    assert!(c.frame.readouts.is_empty());
}

#[test]
fn paused_and_gaps_drop_to_tracks_and_dashes() {
    let mut s = Settings::default();
    s.menu_bar.readouts.power = true;
    let c = combined(&sample(), &s, true, 2);
    assert_eq!(c.frame.bars, vec![None; 3]);
    assert_eq!(
        readouts(&c),
        [
            (Marker::None, "\u{2013}".into()),
            (Marker::Glyph(Glyph::Bolt), "\u{2013}".into())
        ],
        "markers stay, values dash"
    );
    assert_eq!(c.accessibility, "Kelvo, sampling paused");

    let r = Readings {
        gpu: Reading::Gap,
        disk_used_pct: Reading::Gap,
        ..sample()
    };
    s.modules.get_mut(&Module::Disk).unwrap().enabled = true;
    s.menu_bar.readouts.disk = true;
    let c = combined(&r, &s, false, 2);
    assert_eq!(c.frame.bars[1], None);
    assert!(c.accessibility.contains("GPU no data"));
    assert!(c.accessibility.ends_with("disk no data"));
}

/// Offers `content` as the combined item, the only item on screen.
fn offer1(
    pacer: &mut Pacer,
    content: TrayContent,
    now: Instant,
    period: Duration,
    urgent: bool,
) -> Option<TrayContent> {
    let item = TrayItem {
        key: ItemKey::Combined,
        content,
    };
    pacer
        .offer(vec![item], now, period, urgent)
        .pop()
        .map(|i| i.content)
}

fn due1(pacer: &mut Pacer, now: Instant, period: Duration) -> Option<TrayContent> {
    pacer.due(now, period).pop().map(|i| i.content)
}

fn cpu(v: f32) -> TrayContent {
    let r = Readings {
        cpu: Reading::Value(v),
        ..sample()
    };
    combined(&r, &Settings::default(), false, 2)
}

#[test]
fn pacer_skips_equal_frames() {
    let t0 = Instant::now();
    let p = Pacer::period(false, false);
    let mut pacer = Pacer::default();
    assert!(
        offer1(&mut pacer, cpu(18.2), t0, p, false).is_some(),
        "first frame"
    );
    // 18.2% -> 18.9% is still 5 px and the label text did not change.
    let later = t0 + Duration::from_secs(5);
    assert!(offer1(&mut pacer, cpu(18.9), later, p, false).is_none());
    assert!(offer1(&mut pacer, cpu(30.0), later, p, false).is_some());
    assert_eq!((pacer.drawn, pacer.skipped, pacer.held), (2, 1, 0));
}

/// D-077: a frame that changes every 1 s tick draws every other tick, never twice
/// inside the period, and the timer never fires while frames flow.
#[test]
fn pacer_draws_at_most_once_per_period() {
    let t0 = Instant::now();
    let p = Pacer::period(false, false);
    let mut pacer = Pacer::default();
    let mut draws = Vec::new();
    for i in 0..20u32 {
        // A few ms of jitter either way, as the GCD tick has.
        let jitter = if i % 3 == 0 { 0 } else { 4 };
        let now = t0 + Duration::from_secs(u64::from(i)) - Duration::from_millis(jitter);
        if offer1(&mut pacer, cpu(10.0 + 5.0 * i as f32), now, p, false).is_some() {
            draws.push(now);
        }
        if let Some(d) = pacer.deadline(p) {
            assert!(
                d > now + Duration::from_secs(1),
                "the next frame comes first"
            );
        }
    }
    assert_eq!(draws.len(), 10, "{draws:?}");
    for w in draws.windows(2) {
        assert!(w[1] - w[0] >= p - EARLY, "{:?}", w[1] - w[0]);
    }
    assert_eq!((pacer.drawn, pacer.held + pacer.skipped), (10, 10));
}

/// The last change before the frames stop is drawn by the timer, not left stale.
#[test]
fn pacer_draws_a_held_frame_at_its_deadline() {
    let t0 = Instant::now();
    let p = Pacer::period(false, false);
    let mut pacer = Pacer::default();
    offer1(&mut pacer, cpu(10.0), t0, p, false).unwrap();
    let t1 = t0 + Duration::from_secs(1);
    assert!(
        offer1(&mut pacer, cpu(50.0), t1, p, false).is_none(),
        "held"
    );
    let deadline = pacer.deadline(p).unwrap();
    assert_eq!(deadline, t0 + p + GRACE);
    assert!(due1(&mut pacer, deadline - Duration::from_millis(1), p).is_none());
    assert_eq!(due1(&mut pacer, deadline, p), Some(cpu(50.0)));
    assert_eq!(pacer.deadline(p), None, "nothing left");
    assert_eq!(due1(&mut pacer, deadline + p, p), None);

    // A frame back to what is shown cancels the held one.
    let t2 = deadline + Duration::from_millis(100);
    assert!(offer1(&mut pacer, cpu(90.0), t2, p, false).is_none());
    assert!(offer1(&mut pacer, cpu(50.0), t2, p, false).is_none());
    assert_eq!(pacer.deadline(p), None);
    // So does the display going idle.
    assert!(offer1(&mut pacer, cpu(90.0), t2, p, false).is_none());
    pacer.drop_pending();
    assert_eq!(pacer.deadline(p), None);
}

/// Every sampling plan's menu bar figure is the spacing this pacer draws at in the
/// background when the frame changes on every tick: never closer, and late by at most
/// the held frame's grace when the tick does not divide the period (D-092, D-094).
#[test]
fn sampling_plans_quote_the_pacer() {
    for plan in crate::facts::sampling_plans() {
        for f in [plan.ac, plan.battery] {
            let tick = Duration::from_millis(u64::from(f.background_tick_ms));
            let status = EngineStatus {
                backed_off: f.tick_ms != plan.interval_ms,
                backgrounded: true,
                ..EngineStatus::default()
            };
            let p = Pacer::for_status(&status);
            let t0 = Instant::now();
            let mut pacer = Pacer::default();
            let mut draws = Vec::new();
            for i in 0..40u32 {
                let now = t0 + tick * i;
                if let Some(d) = pacer.deadline(p).filter(|d| *d < now) {
                    due1(&mut pacer, d, p).unwrap();
                    draws.push(d);
                }
                // 9% steps, each a different bar, none repeating within 11 ticks.
                let v = ((i * 3) % 11) as f32 * 9.0;
                if offer1(&mut pacer, cpu(v), now, p, false).is_some() {
                    draws.push(now);
                }
            }
            let menu_bar = Duration::from_millis(u64::from(f.menu_bar_ms));
            for w in draws.windows(2) {
                let gap = w[1] - w[0];
                assert!(
                    gap + EARLY >= menu_bar && gap <= menu_bar + GRACE,
                    "{plan:?}: drew {gap:?} apart, plan says {menu_bar:?}"
                );
            }
        }
    }
}

/// A frame whose draw failed is not counted as shown: the same frame offered again
/// draws once the period is up instead of being skipped as equal, which left the icon
/// stale. A failure that persists still retries at the paced rate, not every frame.
#[test]
fn pacer_redraws_after_a_failed_draw() {
    let t0 = Instant::now();
    let p = Pacer::period(false, false);
    let mut pacer = Pacer::default();
    offer1(&mut pacer, cpu(10.0), t0, p, false).unwrap();
    let t1 = t0 + Duration::from_millis(100);
    let failed = offer1(&mut pacer, cpu(50.0), t1, p, true).unwrap();
    pacer.forget_last(ItemKey::Combined);

    // Inside the period it is held, not drawn again.
    let t2 = t1 + Duration::from_secs(1);
    assert!(offer1(&mut pacer, cpu(50.0), t2, p, false).is_none());
    assert_eq!(
        pacer.deadline(p),
        Some(t1 + p + GRACE),
        "the timer would draw it"
    );
    assert_eq!(
        offer1(&mut pacer, cpu(50.0), t1 + p, p, false),
        Some(failed),
        "the same frame draws once the period is up"
    );
    // Once drawn, pacing resumes as usual.
    assert!(offer1(&mut pacer, cpu(50.0), t1 + p, p, false).is_none());
}

#[test]
fn pacer_draws_urgent_frames_now_and_doubles_when_backed_off() {
    let t0 = Instant::now();
    let mut pacer = Pacer::default();
    let p = Pacer::period(false, false);
    offer1(&mut pacer, cpu(10.0), t0, p, false).unwrap();
    let paused = combined(&sample(), &Settings::default(), true, 2);
    assert!(
        offer1(&mut pacer, paused, t0 + Duration::from_millis(10), p, true).is_some(),
        "pausing shows at once"
    );

    let slow = Pacer::period(true, false);
    assert_eq!(slow, 2 * REDRAW_PERIOD);
    let t1 = t0 + Duration::from_secs(10);
    offer1(&mut pacer, cpu(20.0), t1, slow, false).unwrap();
    // Backed off, frames come every 2 s: every other one draws.
    assert!(offer1(&mut pacer, cpu(30.0), t1 + REDRAW_PERIOD, slow, false).is_none());
    assert!(offer1(&mut pacer, cpu(40.0), t1 + 2 * REDRAW_PERIOD, slow, false).is_some());
}

/// Performance mode and the background double the period again: 4 s, and 8 s when
/// backed off (D-088, D-094). They do not stack.
#[test]
fn pacer_period_follows_performance_mode_and_the_background() {
    let s = |n: u64| Duration::from_secs(n);
    let status = |backed_off, performance, backgrounded| EngineStatus {
        backed_off,
        backgrounded,
        performance: if performance {
            kelvo_schema::PerformanceReason::Setting
        } else {
            kelvo_schema::PerformanceReason::Off
        },
        ..EngineStatus::default()
    };
    assert_eq!(
        Pacer::for_status(&status(false, false, false)),
        s(2),
        "a window open"
    );
    assert_eq!(Pacer::for_status(&status(true, false, false)), s(4));
    assert_eq!(Pacer::for_status(&status(false, true, false)), s(4));
    assert_eq!(Pacer::for_status(&status(true, true, false)), s(8));
    assert_eq!(
        Pacer::for_status(&status(false, false, true)),
        s(4),
        "the menu bar only"
    );
    assert_eq!(
        Pacer::for_status(&status(true, false, true)),
        s(4),
        "on battery the background tick stays 2 s, so the redraw does not double"
    );
    assert_eq!(
        Pacer::for_status(&status(false, true, true)),
        s(4),
        "no stacking"
    );
}

#[test]
fn item_state_resizes_and_relabels_only_on_change() {
    let mut item = ItemState::default();
    let first = item.changes(34, 36, "CPU 18 percent");
    assert_eq!(
        first,
        ItemChanges {
            resize: true,
            label: Some("CPU 18 percent".into())
        }
    );
    // A new image at the same size with the same words: swap the image only.
    assert_eq!(
        item.changes(34, 36, "CPU 18 percent"),
        ItemChanges::default()
    );
    // New words, same size: relabel without resizing.
    assert_eq!(
        item.changes(34, 36, "CPU 30 percent"),
        ItemChanges {
            resize: false,
            label: Some("CPU 30 percent".into())
        }
    );
    // A wider image ("100%") resizes; the unchanged label is not set again.
    assert_eq!(
        item.changes(40, 36, "CPU 30 percent"),
        ItemChanges {
            resize: true,
            label: None
        }
    );
    // After a failed set, everything is set again.
    item.forget();
    assert_eq!(
        item.changes(40, 36, "CPU 30 percent"),
        ItemChanges {
            resize: true,
            label: Some("CPU 30 percent".into())
        }
    );
}

#[test]
fn rate_text_is_compact() {
    assert_eq!(rate_text(0.0, NetworkUnit::BytesPerSec), "0KB");
    assert_eq!(rate_text(512_300.0, NetworkUnit::BytesPerSec), "512KB");
    assert_eq!(rate_text(1_250_000.0, NetworkUnit::BytesPerSec), "1.2MB");
    assert_eq!(rate_text(2.5e9, NetworkUnit::BytesPerSec), "2.5GB");
}

#[test]
fn rate_lines_match_the_spec() {
    assert_eq!(rate_line(1_200_000.0, NetworkUnit::BytesPerSec), "1.2 MB/s");
    assert_eq!(
        rate_line(38_400_000.0, NetworkUnit::BytesPerSec),
        "38.4 MB/s"
    );
    assert_eq!(rate_line(512_300.0, NetworkUnit::BytesPerSec), "512 KB/s");
    assert_eq!(rate_line(250e6, NetworkUnit::BytesPerSec), "250 MB/s");
    // Rounds up to 100: no decimal, so the line is no wider than "250 MB/s".
    assert_eq!(rate_line(99.96e6, NetworkUnit::BytesPerSec), "100 MB/s");
    assert_eq!(rate_line(99.94e6, NetworkUnit::BytesPerSec), "99.9 MB/s");
    assert_eq!(rate_line(1_200_000.0, NetworkUnit::BitsPerSec), "9.6 Mb/s");
}

#[test]
fn ring_keeps_the_newest_samples_right_aligned() {
    let mut ring = Ring::new(3);
    assert!(ring.is_empty());
    ring.push(Some(1.0));
    assert_eq!(ring.slots().collect::<Vec<_>>(), [None, None, Some(1.0)]);
    for v in [Some(2.0), None, Some(4.0)] {
        ring.push(v);
    }
    assert_eq!(ring.len(), 3, "bounded");
    assert_eq!(
        ring.slots().collect::<Vec<_>>(),
        [Some(2.0), None, Some(4.0)]
    );
    ring.clear();
    assert_eq!(ring.slots().collect::<Vec<_>>(), [None; 3]);
}

#[test]
fn history_records_gaps_and_holds_graph_lengths() {
    let mut h = TrayHistory::default();
    for i in 0..30 {
        let r = Readings {
            cpu: if i == 29 {
                Reading::Gap
            } else {
                Reading::Value(i as f32)
            },
            ..sample()
        };
        h.record(&r);
    }
    assert_eq!((h.cpu.len(), h.gpu.len()), (SPARK_SAMPLES, HIST_SAMPLES));
    let cpu: Vec<_> = h.cpu.slots().collect();
    assert_eq!(cpu[0], Some(10.0), "the oldest kept sample");
    assert_eq!(cpu[19], None, "a gap stays a gap");
    h.clear();
    assert!(h.cpu.is_empty() && h.gpu.is_empty());
}

#[test]
fn resolves_cores_p_then_e_in_core_order() {
    let layout = FrameLayout {
        layout_no: 1,
        series: Arc::from(vec![
            key("cpu.load", &[("core", "P10")]),
            key("cpu.load", &[("core", "E1")]),
            key("cpu.load", &[("core", "P2")]),
            key("cpu.load", &[("core", "E0")]),
            key("cpu.load", &[("core", "P0")]),
        ]),
    };
    let r = TraySeries::resolve(&layout, None).readings(&[10.0, 1.0, 2.0, 0.0, f32::NAN]);
    assert_eq!(
        r.cores,
        vec![
            vec![Reading::Gap, Reading::Value(2.0), Reading::Value(10.0)],
            vec![Reading::Value(0.0), Reading::Value(1.0)],
        ]
    );
}

/// Onboarding's "Graph per module" (the "Graphs" row): three own items and no
/// combined item, since nothing is left in it.
#[test]
fn graph_per_module_builds_own_items_only() {
    let mut s = Settings::default();
    no_bars(&mut s);
    s.menu_bar.readouts.temperature = false;
    set(&mut s, Module::Cpu, ItemMode::Graph);
    set(&mut s, Module::Memory, ItemMode::Graph);
    set(&mut s, Module::Network, ItemMode::Graph);
    let mut history = TrayHistory::default();
    for _ in 0..3 {
        history.record(&Readings {
            cpu: Reading::Value(50.0),
            ..sample()
        });
    }
    let items = build(&sample(), &history, &s, false, 2);
    let keys: Vec<ItemKey> = items.iter().map(|i| i.key).collect();
    assert_eq!(
        keys,
        [
            ItemKey::Own(Module::Cpu),
            ItemKey::Own(Module::Memory),
            ItemKey::Own(Module::Network)
        ]
    );
    let ids: Vec<&str> = keys.iter().map(|k| k.id()).collect();
    assert_eq!(ids, ["kelvo-cpu", "kelvo-memory", "kelvo-network"]);

    // 50% of the 12 pt travel at 2x is 12 px; the three samples sit at the right.
    let mut points = vec![None; SPARK_SAMPLES - 3];
    points.extend([Some(12); 3]);
    assert_eq!(
        own(&items, Module::Cpu).frame.graphs,
        [Graph::Spark {
            label: "CPU",
            points
        }]
    );
    assert_eq!(own(&items, Module::Cpu).accessibility, "CPU 18 percent");
    // 42.4% of 13 pt at 2x.
    assert_eq!(
        own(&items, Module::Memory).frame.graphs,
        [Graph::Gauge {
            label: "MEM",
            fill: Some(11),
            text: "42%".into()
        }]
    );
    let net = own(&items, Module::Network);
    assert_eq!(
        net.frame.graphs,
        [Graph::Rates {
            up: "1.2 MB/s \u{2191}".into(),
            down: "37.2 MB/s \u{2193}".into()
        }]
    );
    assert_eq!(
        net.accessibility,
        "network up 1.2 megabytes per second, down 37.2 megabytes per second"
    );
}

#[test]
fn cores_and_gpu_history_build_the_cores_row() {
    let mut s = Settings::default();
    s.menu_bar.bars.cpu = false;
    s.menu_bar.bars.gpu = false;
    s.menu_bar.readouts.temperature = false;
    set(&mut s, Module::Cpu, ItemMode::Cores);
    set(&mut s, Module::Gpu, ItemMode::Graph);
    let mut r = sample();
    r.cores[1][3] = Reading::Value(0.4);
    r.cores[0][9] = Reading::Gap;
    let mut history = TrayHistory::default();
    history.record(&Readings {
        gpu: Reading::Value(100.0),
        ..sample()
    });
    history.record(&Readings {
        gpu: Reading::Gap,
        ..sample()
    });
    let items = build(&r, &history, &s, false, 2);
    // Memory stays in the combined item, first.
    assert_eq!(items[0].key, ItemKey::Combined);
    assert_eq!(items[0].content.frame.bars, [Some(12)]);
    let Graph::Cores { label, clusters } = &own(&items, Module::Cpu).frame.graphs[0] else {
        panic!("not a core strip");
    };
    assert_eq!(*label, "CPU");
    assert_eq!(clusters.iter().map(Vec::len).collect::<Vec<_>>(), [10, 4]);
    // 34% of 16 pt at 2x; a gap is no bar; any load is at least 1 pt.
    assert_eq!(clusters[0][0], Some(11));
    assert_eq!(clusters[0][9], None);
    assert_eq!(clusters[1][3], Some(2));
    let mut bars = vec![None; HIST_SAMPLES - 2];
    bars.extend([Some(26), None]);
    assert_eq!(
        own(&items, Module::Gpu).frame.graphs,
        [Graph::Hist { label: "GPU", bars }]
    );
}

/// Own items are independent of the combined item: CPU can be a bar and its own value.
#[test]
fn own_values_sit_beside_the_combined_item() {
    let mut s = Settings::default();
    set(&mut s, Module::Cpu, ItemMode::Value);
    set(&mut s, Module::Power, ItemMode::Value);
    set(&mut s, Module::Battery, ItemMode::Value);
    set(&mut s, Module::Disk, ItemMode::Value);
    let items = build(&sample(), &TrayHistory::default(), &s, false, 2);
    let c = &items[0].content;
    assert_eq!(items[0].key, ItemKey::Combined);
    assert_eq!(c.frame.bars, [Some(5), Some(10), Some(12)]);
    assert_eq!(
        readouts(c),
        [(Marker::None, "142°".into())],
        "Power's own item is watts; the temperature stays"
    );
    let value = |m| own(&items, m).frame.values.clone();
    assert_eq!(
        value(Module::Cpu),
        [Labeled {
            label: "CPU",
            text: "18%".into()
        }]
    );
    assert_eq!(
        value(Module::Power),
        [Labeled {
            label: "PWR",
            text: "14.8W".into()
        }]
    );
    assert_eq!(
        own(&items, Module::Battery).accessibility,
        "battery 87 percent"
    );
    // Disk is off by default: no own item for it.
    assert!(items.iter().all(|i| i.key != ItemKey::Own(Module::Disk)));
    s.modules.get_mut(&Module::Disk).unwrap().enabled = true;
    let items = build(&sample(), &TrayHistory::default(), &s, false, 2);
    assert_eq!(
        own(&items, Module::Disk).frame.values,
        [Labeled {
            label: "DSK",
            text: "220.0MB".into()
        }],
        "the own Disk item is the read + write rate"
    );

    // Back off: the own items are gone from the build.
    s.menu_bar.items = ItemSettings::default();
    let items = build(&sample(), &TrayHistory::default(), &s, false, 2);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].key, ItemKey::Combined);
}

#[test]
fn paused_own_items_drop_to_tracks() {
    let mut s = Settings::default();
    set(&mut s, Module::Cpu, ItemMode::Graph);
    set(&mut s, Module::Memory, ItemMode::Graph);
    set(&mut s, Module::Network, ItemMode::Graph);
    let mut history = TrayHistory::default();
    history.record(&sample());
    let items = build(&sample(), &history, &s, true, 2);
    let cpu = own(&items, Module::Cpu);
    assert_eq!(
        cpu.frame.graphs,
        [Graph::Spark {
            label: "CPU",
            points: vec![None; SPARK_SAMPLES]
        }]
    );
    assert_eq!(cpu.accessibility, "Kelvo CPU, sampling paused");
    assert_eq!(
        own(&items, Module::Memory).frame.graphs,
        [Graph::Gauge {
            label: "MEM",
            fill: None,
            text: DASH.into()
        }]
    );
    assert_eq!(
        own(&items, Module::Network).frame.graphs,
        [Graph::Rates {
            up: format!("{DASH} \u{2191}"),
            down: format!("{DASH} \u{2193}")
        }]
    );
}

fn own_item(module: Module, v: f32) -> TrayItem {
    TrayItem {
        key: ItemKey::Own(module),
        content: cpu(v),
    }
}

fn keys(items: &[TrayItem]) -> Vec<ItemKey> {
    items.iter().map(|i| i.key).collect()
}

const GRAPHS: [Module; 3] = [Module::Cpu, Module::Memory, Module::Network];

/// D-080: with the Graph per module preset every item changes on every tick. They are
/// drawn together, once per period, never each on its own clock.
#[test]
fn pacer_draws_changed_items_together_once_per_period() {
    let t0 = Instant::now();
    let p = Pacer::period(false, false);
    let mut pacer = Pacer::default();
    let mut batches = Vec::new();
    for i in 0..20u32 {
        let jitter = if i % 3 == 0 { 0 } else { 4 };
        let now = t0 + Duration::from_secs(u64::from(i)) - Duration::from_millis(jitter);
        let items = GRAPHS
            .iter()
            .enumerate()
            .map(|(n, &m)| own_item(m, 5.0 + ((5 * i + 20 * n as u32) % 90) as f32))
            .collect();
        let drawn = pacer.offer(items, now, p, false);
        if !drawn.is_empty() {
            assert_eq!(keys(&drawn).len(), 3, "all three in one call at {i}");
            batches.push(now);
        }
    }
    assert_eq!(batches.len(), 10, "{batches:?}");
    for w in batches.windows(2) {
        assert!(w[1] - w[0] >= p - EARLY, "{:?}", w[1] - w[0]);
    }
    assert_eq!((pacer.drawn, pacer.held), (30, 30));
}

/// Items that change on different ticks share one deadline: a change held inside the
/// period draws with the next item's change, and the timer draws every held item at
/// once.
#[test]
fn pacer_shares_one_deadline_across_items() {
    let t0 = Instant::now();
    let p = Pacer::period(false, false);
    let mut pacer = Pacer::default();
    let first = pacer.offer(
        vec![own_item(Module::Cpu, 10.0), own_item(Module::Memory, 10.0)],
        t0,
        p,
        false,
    );
    assert_eq!(keys(&first).len(), 2);

    // Only CPU changes: held, since the period is shared.
    let t1 = t0 + Duration::from_secs(1);
    let held = pacer.offer(
        vec![own_item(Module::Cpu, 50.0), own_item(Module::Memory, 10.0)],
        t1,
        p,
        false,
    );
    assert!(held.is_empty());
    // Only memory changes at the period: the held CPU frame draws with it.
    let t2 = t0 + p;
    let both = pacer.offer(
        vec![own_item(Module::Cpu, 50.0), own_item(Module::Memory, 50.0)],
        t2,
        p,
        false,
    );
    assert_eq!(
        keys(&both),
        [ItemKey::Own(Module::Cpu), ItemKey::Own(Module::Memory)]
    );

    // Both change inside the period, then frames stop: one timer draws both.
    let t3 = t2 + Duration::from_secs(1);
    let none = pacer.offer(
        vec![own_item(Module::Cpu, 90.0), own_item(Module::Memory, 90.0)],
        t3,
        p,
        false,
    );
    assert!(none.is_empty());
    let deadline = pacer.deadline(p).unwrap();
    assert_eq!(deadline, t2 + p + GRACE);
    assert!(pacer.due(deadline - Duration::from_millis(1), p).is_empty());
    assert_eq!(keys(&pacer.due(deadline, p)).len(), 2);
    assert_eq!(pacer.deadline(p), None);

    // CPU draws alone a period later. Memory changing a second after that waits for
    // the next shared draw, though its own last draw is more than a period ago.
    let t4 = deadline + p;
    let cpu_only = pacer.offer(
        vec![own_item(Module::Cpu, 10.0), own_item(Module::Memory, 90.0)],
        t4,
        p,
        false,
    );
    assert_eq!(keys(&cpu_only), [ItemKey::Own(Module::Cpu)]);
    let t5 = t4 + Duration::from_secs(1);
    let held = pacer.offer(
        vec![own_item(Module::Cpu, 10.0), own_item(Module::Memory, 10.0)],
        t5,
        p,
        false,
    );
    assert!(
        held.is_empty(),
        "no second main-thread call inside the period"
    );
    assert_eq!(pacer.deadline(p), Some(t4 + p + GRACE));
}

/// A newly created item draws on arrival; a removed item's held frame is dropped and,
/// if the item comes back, its first frame draws at once.
#[test]
fn pacer_draws_new_items_at_once_and_drops_removed_ones() {
    let t0 = Instant::now();
    let p = Pacer::period(false, false);
    let mut pacer = Pacer::default();
    pacer.offer(vec![own_item(Module::Cpu, 10.0)], t0, p, false);
    let t1 = t0 + Duration::from_millis(300);
    let drawn = pacer.offer(
        vec![own_item(Module::Cpu, 50.0), own_item(Module::Memory, 10.0)],
        t1,
        p,
        false,
    );
    assert_eq!(
        keys(&drawn),
        [ItemKey::Own(Module::Cpu), ItemKey::Own(Module::Memory)],
        "the new item and the changed one draw together"
    );

    let t2 = t1 + Duration::from_millis(300);
    assert!(
        pacer
            .offer(
                vec![own_item(Module::Cpu, 50.0), own_item(Module::Memory, 60.0)],
                t2,
                p,
                false
            )
            .is_empty()
    );
    // Memory's item goes away: its held frame with it.
    pacer.remove(ItemKey::Own(Module::Memory));
    assert!(
        pacer
            .offer(vec![own_item(Module::Cpu, 50.0)], t2, p, false)
            .is_empty()
    );
    assert_eq!(pacer.deadline(p), None);
    // Back again: drawn at once, though inside the period.
    let back = pacer.offer(
        vec![own_item(Module::Cpu, 50.0), own_item(Module::Memory, 60.0)],
        t2,
        p,
        false,
    );
    assert_eq!(keys(&back), [ItemKey::Own(Module::Memory)]);
}

#[test]
fn item_change_creates_last_to_first_and_removes_unwanted() {
    let shown = BTreeSet::from([ItemKey::Combined, ItemKey::Own(Module::Cpu)]);
    let wanted: Vec<TrayItem> = GRAPHS.iter().map(|&m| own_item(m, 10.0)).collect();
    assert_eq!(
        item_change(&shown, &wanted, &BTreeSet::new()),
        ItemChange {
            remove: vec![ItemKey::Combined],
            create: vec![ItemKey::Own(Module::Network), ItemKey::Own(Module::Memory)],
        }
    );
    // A key that failed to create is skipped until the caller clears it.
    let failed = BTreeSet::from([ItemKey::Own(Module::Network)]);
    assert_eq!(
        item_change(&shown, &wanted, &failed).create,
        [ItemKey::Own(Module::Memory)]
    );
    // Nothing to do when the set already matches.
    let all: BTreeSet<ItemKey> = wanted.iter().map(|w| w.key).collect();
    assert_eq!(item_change(&all, &wanted, &failed), ItemChange::default());
}

/// A status item that fails to build is tried again every 30 s, logged once per
/// streak, retried at once when asked (settings change, display wake), and
/// forgotten when it is no longer wanted.
#[test]
fn retries_wait_then_try_again_and_log_once_per_streak() {
    let t0 = Instant::now();
    let combined = TrayItem {
        key: ItemKey::Combined,
        content: cpu(10.0),
    };
    let wanted = vec![combined.clone()];
    let mut retries = Retries::default();
    assert!(
        retries.failed(ItemKey::Combined, t0),
        "first failure is logged"
    );
    assert_eq!(retries.deadline(), Some(t0 + RETRY_AFTER));

    let soon = t0 + RETRY_AFTER - Duration::from_millis(1);
    assert_eq!(
        retries.waiting(&wanted, soon),
        BTreeSet::from([ItemKey::Combined])
    );
    let shown = BTreeSet::new();
    assert!(
        item_change(&shown, &wanted, &retries.waiting(&wanted, soon))
            .create
            .is_empty(),
        "not tried inside the wait"
    );
    let later = t0 + RETRY_AFTER;
    assert_eq!(
        item_change(&shown, &wanted, &retries.waiting(&wanted, later)).create,
        [ItemKey::Combined],
        "tried again once the wait is over"
    );
    assert!(
        !retries.failed(ItemKey::Combined, later),
        "the same streak is not logged again"
    );
    assert_eq!(retries.deadline(), Some(later + RETRY_AFTER));

    // Asked to retry now: due at once, the streak kept.
    let t1 = later + Duration::from_secs(1);
    retries.retry_now(t1);
    assert!(retries.waiting(&wanted, t1).is_empty());
    assert!(!retries.failed(ItemKey::Combined, t1));

    // Created: the streak ends, and a new failure is logged again.
    retries.created(ItemKey::Combined);
    assert_eq!(retries.deadline(), None);
    assert!(retries.failed(ItemKey::Combined, t1));

    // No longer wanted: forgotten, so its deadline no longer wakes the thread.
    assert!(retries.waiting(&[], t1).is_empty());
    assert_eq!(retries.deadline(), None);
}
