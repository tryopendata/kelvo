//! What the tray items show, reduced to what the icons can actually display (architecture.md,
//! Tray rendering pipeline). Pure: no Tauri, no AppKit, so the quantizing, the frame skip
//! and pacing, the accessibility label and what a drawn frame changes on the status item
//! are tested without a menu bar.
//!
//! Each module's "Menu bar" setting decides its element (v1-local-monitor.md 4.2):
//! CPU, GPU and Memory are a bar in the combined group or a labelled value; Power &
//! Sensors adds the hottest temperature after the bars (`TempInCombined`) or system watts
//! as a labelled value (`WattsValue`); Network, Disk and Battery are labelled values.
//! The `Own*` modes give a module a status item of its own (D-080): its labelled value,
//! or its graph (CPU sparkline or per-core strip, GPU history bars, memory
//! fill gauge, network rates). The sparkline and history bars read [`TrayHistory`], a
//! bounded ring fed by every frame, never a history query.

use std::collections::VecDeque;

use kelvo_engine::FrameLayout;
use kelvo_schema::settings::MenuBarMode;
use kelvo_schema::{Module, Settings};

use self::format::{pct_text, rate_line, rate_text, rate_words, temp_text, watts_text, words_name};
pub use self::items::{ItemChange, ItemChanges, ItemState, RETRY_AFTER, Retries, item_change};
pub use self::pacer::{Pacer, REDRAW_PERIOD};

mod format;
mod items;
mod pacer;

/// Bar geometry in points (design-system.md, Tray icon spec).
pub const BAR_HEIGHT_PT: u32 = 14;
/// Samples in the CPU sparkline (design-system.md: 20 samples wide).
pub const SPARK_SAMPLES: usize = 20;
/// Samples in the GPU history bars (design-system.md: the last 9 samples).
pub const HIST_SAMPLES: usize = 9;
/// Vertical travel of the sparkline inside its 16 pt box (y 2 to 14).
pub const SPARK_RANGE_PT: u32 = 12;
/// Tallest history bar and gauge fill inside their 16 pt boxes (13 pt).
pub const BOX_FILL_PT: u32 = 13;
/// Per-core bars are 16 pt tall (design-system.md, Cores + histogram).
pub const CORE_HEIGHT_PT: u32 = 16;

/// Where the tray's series sit in one frame layout. Resolved once per layout, so a frame
/// costs a handful of index reads instead of a full `Snapshot`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraySeries {
    pub layout_no: Option<u32>,
    cpu: Option<usize>,
    gpu: Option<usize>,
    mem: Option<usize>,
    temp: Option<usize>,
    watts: Option<usize>,
    battery: Option<usize>,
    /// `net.rx_total` and `net.tx_total`: the collector's sums over the reported
    /// interfaces, a gap when any interface is (D-092).
    net_rx: Option<usize>,
    net_tx: Option<usize>,
    /// `disk.read_total` and `disk.write_total`, the same over devices.
    disk_read: Option<usize>,
    disk_write: Option<usize>,
    /// `cpu.load` per core, grouped by core kind: P cores, then E cores, then any other
    /// kind, each in core-number order.
    cores: Vec<Vec<usize>>,
}

/// Sort key of a `core` label ("P3", "E0"): kind rank, then number.
fn core_order(label: &str) -> (u8, char, u32) {
    let mut chars = label.chars();
    let kind = chars.next().unwrap_or('?');
    let rank = match kind {
        'P' => 0,
        'E' => 1,
        _ => 2,
    };
    (rank, kind, chars.as_str().parse().unwrap_or(u32::MAX))
}

impl TraySeries {
    pub fn resolve(layout: &FrameLayout) -> Self {
        let mut s = Self {
            layout_no: Some(layout.layout_no),
            ..Self::default()
        };
        let mut cores: Vec<((u8, char, u32), usize)> = Vec::new();
        for (i, key) in layout.series.iter().enumerate() {
            let bare = key.labels.is_empty();
            match key.metric.as_str() {
                "cpu.total" if bare => s.cpu = Some(i),
                "gpu.util" if bare => s.gpu = Some(i),
                "mem.pressure" if bare => s.mem = Some(i),
                "thermal.hottest" if bare => s.temp = Some(i),
                "power.system" if bare => s.watts = Some(i),
                "battery.charge" if bare => s.battery = Some(i),
                "net.rx_total" if bare => s.net_rx = Some(i),
                "net.tx_total" if bare => s.net_tx = Some(i),
                "disk.read_total" if bare => s.disk_read = Some(i),
                "disk.write_total" if bare => s.disk_write = Some(i),
                "cpu.load" => {
                    if let Some(core) = key.labels.get("core") {
                        cores.push((core_order(core), i));
                    }
                }
                _ => {}
            }
        }
        cores.sort_unstable();
        for group in cores.chunk_by(|a, b| a.0.1 == b.0.1) {
            s.cores.push(group.iter().map(|&(_, i)| i).collect());
        }
        s
    }

    /// Current values from a frame's `held` array (never `values`: a series sampled every
    /// 5 ticks would blink).
    pub fn readings(&self, held: &[f32]) -> Readings {
        let one = |i: Option<usize>| -> Reading {
            match i {
                None => Reading::Absent,
                Some(i) => held
                    .get(i)
                    .copied()
                    .filter(|v| v.is_finite())
                    .map_or(Reading::Gap, Reading::Value),
            }
        };
        // Both directions for one labelled value: a gap when either side is, never one
        // side passed off as the total.
        let both = |a: Reading, b: Reading| -> Reading {
            match (a, b) {
                (Reading::Value(a), Reading::Value(b)) => Reading::Value(a + b),
                (Reading::Absent, Reading::Absent) => Reading::Absent,
                _ => Reading::Gap,
            }
        };
        let net_rx = one(self.net_rx);
        let net_tx = one(self.net_tx);
        Readings {
            cpu: one(self.cpu),
            gpu: one(self.gpu),
            mem: one(self.mem),
            temp_c: one(self.temp),
            watts: one(self.watts),
            battery: one(self.battery),
            net_bps: both(net_rx, net_tx),
            net_rx_bps: net_rx,
            net_tx_bps: net_tx,
            disk_bps: both(one(self.disk_read), one(self.disk_write)),
            cores: self
                .cores
                .iter()
                .map(|g| g.iter().map(|&i| one(Some(i))).collect())
                .collect(),
        }
    }
}

/// One value as the tray sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Reading {
    /// The host does not have this series (no battery on a Mac mini): not shown.
    #[default]
    Absent,
    /// The series exists but has no current value: shown as a dash, never interpolated.
    Gap,
    Value(f32),
}

impl Reading {
    fn value(self) -> Option<f32> {
        match self {
            Reading::Value(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readings {
    pub cpu: Reading,
    pub gpu: Reading,
    pub mem: Reading,
    pub temp_c: Reading,
    pub watts: Reading,
    pub battery: Reading,
    /// Received plus sent, for the labelled value.
    pub net_bps: Reading,
    pub net_rx_bps: Reading,
    pub net_tx_bps: Reading,
    pub disk_bps: Reading,
    /// `cpu.load` per core, P cores then E cores (see [`TraySeries`]).
    pub cores: Vec<Vec<Reading>>,
}

/// A bounded ring of the newest samples, oldest first. `None` is a gap: the sparkline
/// breaks there and the history bar is empty, never interpolated.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    cap: usize,
    samples: VecDeque<Option<f32>>,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            samples: VecDeque::with_capacity(cap),
        }
    }

    pub fn push(&mut self, v: Option<f32>) {
        if self.samples.len() == self.cap {
            self.samples.pop_front();
        }
        self.samples.push_back(v);
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// `cap` slots, oldest first, padded with gaps on the left until the ring is full, so
    /// the newest sample is always at the right edge.
    fn slots(&self) -> impl Iterator<Item = Option<f32>> + '_ {
        std::iter::repeat_n(None, self.cap - self.samples.len()).chain(self.samples.iter().copied())
    }
}

/// The recent samples the graphs draw: one per frame, in the tray model rather than
/// re-queried from history.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayHistory {
    pub cpu: Ring,
    pub gpu: Ring,
}

impl Default for TrayHistory {
    fn default() -> Self {
        Self {
            cpu: Ring::new(SPARK_SAMPLES),
            gpu: Ring::new(HIST_SAMPLES),
        }
    }
}

impl TrayHistory {
    /// Adds one frame's readings.
    pub fn record(&mut self, r: &Readings) {
        self.cpu.push(r.cpu.value());
        self.gpu.push(r.gpu.value());
    }

    /// Forgets every sample: sampling paused or the display slept, and the next sample is
    /// not next to the last one in time. The graphs start again from the right edge.
    pub fn clear(&mut self) {
        self.cpu.clear();
        self.gpu.clear();
    }
}

/// A labelled value in the values layout: a stacked three-letter label and mono text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Labeled {
    pub label: &'static str,
    pub text: String,
}

/// Characters of width the combined temperature always reserves ("61°").
pub const TEMP_MIN_CHARS: usize = 3;

/// Characters of width a labelled value always reserves, so the status item keeps its
/// width as values change ("9%" vs "18%", a dash while paused). A wider status item
/// shifts every item to its left, which reads as jitter.
pub fn min_chars(label: &str) -> usize {
    match label {
        "PWR" => 5,         // "14.8W"
        "NET" | "DSK" => 6, // "38.4MB"
        _ => 3,             // "18%", "61°"
    }
}

/// Characters of width the stacked network rates reserve ("38.4 MB/s ↓").
pub const RATE_MIN_CHARS: usize = 11;

/// A module's graph in a status item of its own ("Graphs" and "Cores + histogram"),
/// quantized to device pixels. `None` is a gap or the paused state: no ink.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Graph {
    /// Line through the last [`SPARK_SAMPLES`] samples, oldest left; each point is its
    /// height above the bottom of the line's travel.
    Spark {
        label: &'static str,
        points: Vec<Option<u16>>,
    },
    /// The last [`HIST_SAMPLES`] samples as bars, oldest left.
    Hist {
        label: &'static str,
        bars: Vec<Option<u16>>,
    },
    /// A vertical fill gauge and the value beside it.
    Gauge {
        label: &'static str,
        fill: Option<u16>,
        text: String,
    },
    /// Up (sent) over down (received), right-aligned.
    Rates { up: String, down: String },
    /// One bar per core, a wider gap between core kinds (P | E).
    Cores {
        label: &'static str,
        clusters: Vec<Vec<Option<u16>>>,
    },
}

/// Everything an icon draws, quantized: bar fills in device pixels, temperature to
/// 1 degree, percentages to 1%. Two frames that compare equal render the same image, so
/// an equal frame is skipped without touching the status item.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrayFrame {
    /// Device pixels per point (1 or 2).
    pub scale: u32,
    /// The combined group's bars, left to right. `None` draws the track only (gap or
    /// paused).
    pub bars: Vec<Option<u16>>,
    /// Text after the bars ("61°").
    pub combined_text: Option<String>,
    pub values: Vec<Labeled>,
    /// Graphs, after the values (an own item has one).
    pub graphs: Vec<Graph>,
}

impl TrayFrame {
    pub fn empty(scale: u32) -> Self {
        Self {
            scale,
            bars: Vec::new(),
            combined_text: None,
            values: Vec::new(),
            graphs: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.bars.is_empty()
            && self.combined_text.is_none()
            && self.values.is_empty()
            && self.graphs.is_empty()
    }
}

/// The frame plus its words for VoiceOver.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayContent {
    pub frame: TrayFrame,
    pub accessibility: String,
}

/// Which status item a frame is for: the combined item, or a module's own item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemKey {
    Combined,
    Own(Module),
}

impl ItemKey {
    /// The tray id, which is also the status item's `autosaveName` (D-037), so macOS
    /// keeps each item's ⌘-drag position across launches.
    pub fn id(self) -> &'static str {
        match self {
            ItemKey::Combined => "kelvo",
            ItemKey::Own(m) => match m {
                Module::Cpu => "kelvo-cpu",
                Module::Gpu => "kelvo-gpu",
                Module::Memory => "kelvo-memory",
                Module::Power | Module::Sensors => "kelvo-power",
                Module::Network => "kelvo-network",
                Module::Disk => "kelvo-disk",
                Module::Battery => "kelvo-battery",
                Module::Unknown => "kelvo-unknown",
            },
        }
    }
}

/// One status item's content.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayItem {
    pub key: ItemKey,
    pub content: TrayContent,
}

const DASH: &str = "\u{2013}";

fn mode(settings: &Settings, module: Module) -> MenuBarMode {
    match settings.module(module) {
        Some(m) if m.enabled => m.menu_bar,
        _ => MenuBarMode::Hidden,
    }
}

/// `pct` of `range_pt` points, in device pixels.
fn px(pct: f32, range_pt: u32, scale: u32) -> u16 {
    let steps = (range_pt * scale) as f32;
    // Clamped to [0, steps], which fits in u16.
    (pct.clamp(0.0, 100.0) / 100.0 * steps).round() as u16
}

fn bar_px(pct: f32, scale: u32) -> u16 {
    px(pct, BAR_HEIGHT_PT, scale)
}

/// Where a module's labelled value goes.
#[derive(Clone, Copy)]
enum Place {
    Combined,
    Own,
}

/// Collects the combined item's elements and the own items as modules are visited.
struct Layout {
    scale: u32,
    paused: bool,
    combined: TrayFrame,
    bar_words: Vec<String>,
    value_words: Vec<String>,
    own: Vec<TrayItem>,
}

impl Layout {
    fn value(&mut self, place: Place, module: Module, value: Labeled, words: String) {
        match place {
            Place::Combined => {
                self.combined.values.push(value);
                self.value_words.push(words);
            }
            Place::Own => {
                let mut frame = TrayFrame::empty(self.scale);
                frame.values.push(value);
                self.own_item(module, frame, words);
            }
        }
    }

    fn graph(&mut self, module: Module, graph: Graph, words: String) {
        let mut frame = TrayFrame::empty(self.scale);
        frame.graphs.push(graph);
        self.own_item(module, frame, words);
    }

    fn own_item(&mut self, module: Module, frame: TrayFrame, words: String) {
        let accessibility = if self.paused {
            format!("Kelvo {}, sampling paused", words_name(module))
        } else {
            words
        };
        self.own.push(TrayItem {
            key: ItemKey::Own(module),
            content: TrayContent {
                frame,
                accessibility,
            },
        });
    }
}

/// `ring`'s slots in device pixels of `range_pt` points; all gaps while paused.
fn ring_px(ring: &Ring, range_pt: u32, scale: u32, paused: bool) -> Vec<Option<u16>> {
    ring.slots()
        .map(|v| v.filter(|_| !paused).map(|v| px(v, range_pt, scale)))
        .collect()
}

/// Builds every status item for `readings` under `settings`: the combined item first (if
/// it has anything to show, or nothing else is shown), then the own items in module
/// order. `paused` drops bars and graphs to their tracks and shows a dash for every
/// text, as a gap does.
pub fn build(
    readings: &Readings,
    history: &TrayHistory,
    settings: &Settings,
    paused: bool,
    scale: u32,
) -> Vec<TrayItem> {
    let unit_t = settings.units.temperature;
    let unit_n = settings.units.network;
    let shown = |r: Reading| if paused { Reading::Gap } else { r };
    let mut l = Layout {
        scale,
        paused,
        combined: TrayFrame::empty(scale),
        bar_words: Vec::new(),
        value_words: Vec::new(),
        own: Vec::new(),
    };

    let pct_modules = [
        (Module::Cpu, readings.cpu, "CPU", "CPU"),
        (Module::Gpu, readings.gpu, "GPU", "GPU"),
        (Module::Memory, readings.mem, "MEM", "memory"),
    ];
    for (module, reading, label, word) in pct_modules {
        if reading == Reading::Absent {
            continue;
        }
        let r = shown(reading);
        let words = match r.value() {
            Some(v) => format!("{word} {} percent", v.clamp(0.0, 999.0).round() as i32),
            None => format!("{word} no data"),
        };
        let text = r.value().map_or_else(|| DASH.into(), pct_text);
        match mode(settings, module) {
            MenuBarMode::InCombined => {
                l.combined.bars.push(r.value().map(|v| bar_px(v, scale)));
                l.bar_words.push(words);
            }
            MenuBarMode::ValueLabel => {
                l.value(Place::Combined, module, Labeled { label, text }, words);
            }
            MenuBarMode::OwnValue => {
                l.value(Place::Own, module, Labeled { label, text }, words);
            }
            MenuBarMode::OwnGraph => {
                let graph = match module {
                    Module::Cpu => Graph::Spark {
                        label,
                        points: ring_px(&history.cpu, SPARK_RANGE_PT, scale, paused),
                    },
                    Module::Gpu => Graph::Hist {
                        label,
                        bars: ring_px(&history.gpu, BOX_FILL_PT, scale, paused),
                    },
                    _ => Graph::Gauge {
                        label,
                        fill: r.value().map(|v| px(v, BOX_FILL_PT, scale)),
                        text,
                    },
                };
                l.graph(module, graph, words);
            }
            MenuBarMode::OwnCores if !readings.cores.is_empty() => {
                // Any load shows at least 1 pt.
                let min = u16::try_from(scale).unwrap_or(1);
                let clusters = readings
                    .cores
                    .iter()
                    .map(|group| {
                        group
                            .iter()
                            .map(|&c| {
                                shown(c)
                                    .value()
                                    .map(|v| px(v, CORE_HEIGHT_PT, scale).max(min))
                            })
                            .collect()
                    })
                    .collect();
                l.graph(module, Graph::Cores { label, clusters }, words);
            }
            MenuBarMode::OwnCores => {
                // No per-core series on this host: the value is still worth an item.
                l.value(Place::Own, module, Labeled { label, text }, words);
            }
            _ => {}
        }
    }

    let power_mode = mode(settings, Module::Power);
    if power_mode == MenuBarMode::TempInCombined && readings.temp_c != Reading::Absent {
        let r = shown(readings.temp_c);
        let text = r
            .value()
            .map_or_else(|| DASH.into(), |c| temp_text(c, unit_t));
        let words = match r.value() {
            Some(c) => {
                let t = temp_text(c, unit_t);
                format!("{} degrees", t.trim_end_matches('°'))
            }
            None => "temperature no data".into(),
        };
        if l.combined.bars.is_empty() {
            // Without bars a bare "61°" would not say what it is.
            l.combined.values.insert(0, Labeled { label: "SOC", text });
            l.value_words.insert(0, words);
        } else {
            l.combined.combined_text = Some(text);
            l.bar_words.push(words);
        }
    }
    let watts_place = match power_mode {
        MenuBarMode::WattsValue => Some(Place::Combined),
        MenuBarMode::OwnValue => Some(Place::Own),
        _ => None,
    };
    if let Some(place) = watts_place
        && readings.watts != Reading::Absent
    {
        let r = shown(readings.watts);
        let value = Labeled {
            label: "PWR",
            text: r.value().map_or_else(|| DASH.into(), watts_text),
        };
        let words = match r.value() {
            Some(w) => format!("power {w:.1} watts"),
            None => "power no data".into(),
        };
        l.value(place, Module::Power, value, words);
    }

    let rate_modules = [
        (Module::Network, readings.net_bps, "NET", "network"),
        (Module::Disk, readings.disk_bps, "DSK", "disk"),
    ];
    for (module, reading, label, word) in rate_modules {
        if reading == Reading::Absent {
            continue;
        }
        let r = shown(reading);
        let words = match r.value() {
            Some(v) => format!("{word} {}", rate_words(v, unit_n)),
            None => format!("{word} no data"),
        };
        let value = Labeled {
            label,
            text: r
                .value()
                .map_or_else(|| DASH.into(), |v| rate_text(v, unit_n)),
        };
        match mode(settings, module) {
            MenuBarMode::ValueLabel => l.value(Place::Combined, module, value, words),
            MenuBarMode::OwnValue => l.value(Place::Own, module, value, words),
            MenuBarMode::OwnGraph => {
                let line = |r: Reading| {
                    shown(r)
                        .value()
                        .map_or_else(|| DASH.into(), |v| rate_line(v, unit_n))
                };
                let words = match (shown(readings.net_tx_bps), shown(readings.net_rx_bps)) {
                    (Reading::Value(tx), Reading::Value(rx)) => format!(
                        "{word} up {}, down {}",
                        rate_words(tx, unit_n),
                        rate_words(rx, unit_n)
                    ),
                    _ => words,
                };
                let graph = Graph::Rates {
                    up: format!("{} \u{2191}", line(readings.net_tx_bps)),
                    down: format!("{} \u{2193}", line(readings.net_rx_bps)),
                };
                l.graph(module, graph, words);
            }
            _ => {}
        }
    }
    if readings.battery != Reading::Absent {
        let r = shown(readings.battery);
        let value = Labeled {
            label: "BAT",
            text: r.value().map_or_else(|| DASH.into(), pct_text),
        };
        let words = match r.value() {
            Some(v) => format!("battery {} percent", v.round() as i32),
            None => "battery no data".into(),
        };
        match mode(settings, Module::Battery) {
            MenuBarMode::ValueLabel => l.value(Place::Combined, Module::Battery, value, words),
            MenuBarMode::OwnValue => l.value(Place::Own, Module::Battery, value, words),
            _ => {}
        }
    }

    // The combined item goes away when it has nothing to show and an own item is there to
    // click; with neither, it shows three empty tracks, never an empty, unclickable item.
    if l.combined.is_empty() && l.own.is_empty() {
        l.combined.bars = vec![None; 3];
    }
    let mut items = Vec::with_capacity(l.own.len() + 1);
    if !l.combined.is_empty() {
        let mut words: Vec<String> = l.bar_words;
        words.extend(l.value_words);
        let accessibility = if paused {
            "Kelvo, sampling paused".to_owned()
        } else if words.is_empty() {
            "Kelvo".to_owned()
        } else {
            words.join(", ")
        };
        items.push(TrayItem {
            key: ItemKey::Combined,
            content: TrayContent {
                frame: l.combined,
                accessibility,
            },
        });
    }
    items.extend(l.own);
    items
}
#[cfg(test)]
mod tests;
