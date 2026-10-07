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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use kelvo_engine::{EngineStatus, FrameLayout};
use kelvo_schema::settings::{MenuBarMode, NetworkUnit, TemperatureUnit};
use kelvo_schema::{Module, Settings};

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

fn pct_text(v: f32) -> String {
    format!("{}%", v.clamp(0.0, 999.0).round() as i32)
}

fn temp_text(c: f32, unit: TemperatureUnit) -> String {
    let t = match unit {
        TemperatureUnit::Celsius => c,
        TemperatureUnit::Fahrenheit => c * 9.0 / 5.0 + 32.0,
    };
    format!("{}°", t.round() as i32)
}

fn watts_text(w: f32) -> String {
    if w < 100.0 {
        format!("{w:.1}W")
    } else {
        format!("{w:.0}W")
    }
}

/// Compact rate for the menu bar: "38.4MB", "512KB", "0KB"; bits use a lowercase "b".
fn rate_text(bytes_per_sec: f32, unit: NetworkUnit) -> String {
    let (v, suffix) = match unit {
        NetworkUnit::BytesPerSec => (bytes_per_sec, "B"),
        NetworkUnit::BitsPerSec => (bytes_per_sec * 8.0, "b"),
    };
    let v = v.max(0.0);
    if v >= 1e9 {
        format!("{:.1}G{suffix}", v / 1e9)
    } else if v >= 1e6 {
        format!("{:.1}M{suffix}", v / 1e6)
    } else {
        format!("{:.0}K{suffix}", (v / 1e3).floor())
    }
}

/// A rate in the stacked graph form: "38.4 MB/s", "512 KB/s", "9.6 Mb/s".
fn rate_line(bytes_per_sec: f32, unit: NetworkUnit) -> String {
    let (v, suffix) = match unit {
        NetworkUnit::BytesPerSec => (bytes_per_sec, "B"),
        NetworkUnit::BitsPerSec => (bytes_per_sec * 8.0, "b"),
    };
    let v = v.max(0.0);
    let scaled = |v: f32, prefix: &str| {
        // Decided after rounding: 99.96 would print "100.0" with one decimal.
        if (v * 10.0).round() >= 1000.0 {
            format!("{v:.0} {prefix}{suffix}/s")
        } else {
            format!("{v:.1} {prefix}{suffix}/s")
        }
    };
    if v >= 1e9 {
        scaled(v / 1e9, "G")
    } else if v >= 1e6 {
        scaled(v / 1e6, "M")
    } else {
        format!("{:.0} K{suffix}/s", (v / 1e3).floor())
    }
}

fn rate_words(bytes_per_sec: f32, unit: NetworkUnit) -> String {
    let (v, word) = match unit {
        NetworkUnit::BytesPerSec => (bytes_per_sec, "bytes"),
        NetworkUnit::BitsPerSec => (bytes_per_sec * 8.0, "bits"),
    };
    if v >= 1e6 {
        format!("{:.1} mega{word} per second", v / 1e6)
    } else {
        format!("{:.0} kilo{word} per second", (v / 1e3).floor())
    }
}

fn words_name(module: Module) -> &'static str {
    match module {
        Module::Cpu => "CPU",
        Module::Gpu => "GPU",
        Module::Memory => "memory",
        Module::Power | Module::Sensors => "power",
        Module::Network => "network",
        Module::Disk => "disk",
        Module::Battery => "battery",
        Module::Unknown => "unknown",
    }
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

/// Shortest time between two drawn frames while sampling at the base tick (D-077). Each
/// redraw costs AppKit 5 to 8 ms of main-thread CPU after `setImage:` (D-073), whatever
/// the image; sampling stays at the base tick.
pub const REDRAW_PERIOD: Duration = Duration::from_secs(2);
/// A changed frame this close to the end of the period draws on arrival, so frames on a
/// 1 s tick with some jitter draw every other tick instead of waiting for the timer.
const EARLY: Duration = Duration::from_millis(250);
/// How long past the period a held frame waits for a newer frame before the timer draws
/// it. Long enough that the next tick's frame normally comes first and the timer never
/// fires while frames flow.
const GRACE: Duration = Duration::from_millis(500);

/// Paces the tray (D-077, D-080): skips frames that would draw the same image, and draws
/// at most once per period for all status items together, so every item that changed is
/// drawn in the same main-thread call rather than each on its own clock. Between draws
/// it holds each item's newest changed frame, so the timer can draw them if no newer
/// frame arrives and no item is left showing a stale image. Counts every outcome, per
/// item, for the debug log.
#[derive(Debug, Default)]
pub struct Pacer {
    /// What each item shows. `None` when its last draw failed: it still shows an older
    /// image. An item with no entry has never been drawn.
    last: BTreeMap<ItemKey, Option<TrayFrame>>,
    last_draw: Option<Instant>,
    pending: BTreeMap<ItemKey, TrayContent>,
    pub drawn: u64,
    /// Equal to the item's last drawn frame.
    pub skipped: u64,
    /// Changed, but inside the period: held (and mostly replaced by a newer frame).
    pub held: u64,
}

impl Pacer {
    /// The redraw period: [`REDRAW_PERIOD`], doubled while the engine is backed off for
    /// battery or Low Power Mode (its tick is then doubled too), and doubled again when
    /// `slowed`, in Performance mode (D-088) or in the background with no window showing
    /// detail (D-094): 2, 4 or 8 s. With a window open the menu bar keeps pace with it.
    pub fn period(backed_off: bool, slowed: bool) -> Duration {
        REDRAW_PERIOD * (1 + u32::from(backed_off)) * (1 + u32::from(slowed))
    }

    /// [`Pacer::period`] for the engine's status. In the background it is 4 s whatever
    /// the back-off: the background tick replaces the back-off rather than doubling, and
    /// a slower tick spaces the frames out by itself (D-094).
    pub fn for_status(s: &EngineStatus) -> Duration {
        if s.backgrounded {
            return Self::period(false, true);
        }
        Self::period(s.backed_off, s.performance.is_on())
    }

    /// Offers the newest content of every item on screen. Returns the items to draw now,
    /// together: those that differ from what they show, when the period is up, or nothing
    /// was drawn yet, or an item has never been drawn (it was just created), or `urgent`
    /// (the paused state). Otherwise the changed items are held for [`Pacer::due`]. An
    /// item missing from `items` is no longer on screen: its held frame is dropped.
    pub fn offer(
        &mut self,
        items: Vec<TrayItem>,
        now: Instant,
        period: Duration,
        urgent: bool,
    ) -> Vec<TrayItem> {
        self.pending
            .retain(|k, _| items.iter().any(|item| item.key == *k));
        let mut changed = Vec::new();
        for item in items {
            match self.last.get(&item.key) {
                Some(Some(frame)) if *frame == item.content.frame => {
                    // What the item shows is current again: nothing is left to draw.
                    self.pending.remove(&item.key);
                    self.skipped += 1;
                }
                _ => changed.push(item),
            }
        }
        if changed.is_empty() {
            return changed;
        }
        let fresh = changed
            .iter()
            .any(|item| !self.last.contains_key(&item.key));
        let due = match self.last_draw {
            Some(t) => urgent || fresh || now + EARLY >= t + period,
            None => true,
        };
        if due {
            // Every held frame is either in `changed` (newer) or was dropped above.
            self.pending.clear();
            self.drawing(changed, now)
        } else {
            self.held += changed.len() as u64;
            for item in changed {
                self.pending.insert(item.key, item.content);
            }
            Vec::new()
        }
    }

    /// When the held frames are drawn if no newer frames come first; `None` when nothing
    /// is held.
    pub fn deadline(&self, period: Duration) -> Option<Instant> {
        if self.pending.is_empty() {
            return None;
        }
        self.last_draw.map(|t| t + period + GRACE)
    }

    /// Every held frame, once their deadline has passed.
    pub fn due(&mut self, now: Instant, period: Duration) -> Vec<TrayItem> {
        match self.deadline(period) {
            Some(d) if now >= d => {}
            _ => return Vec::new(),
        }
        let items = std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(key, content)| TrayItem { key, content })
            .collect();
        self.drawing(items, now)
    }

    /// Drops the held frames: the display went idle and nothing is drawn until it wakes.
    pub fn drop_pending(&mut self) {
        self.pending.clear();
    }

    /// Forgets `key`'s last drawn frame: drawing it failed (render or `set_icon`), so the
    /// item still shows an older image. Its next frame, even one equal to the failed
    /// frame, is drawn once the period is up (or by the held-frame timer), not skipped as
    /// shown. The draw time is kept, so a failure that persists retries at the paced
    /// rate (D-077) rather than on every frame.
    pub fn forget_last(&mut self, key: ItemKey) {
        self.last.insert(key, None);
    }

    /// Forgets `key` entirely: its status item was removed. If it comes back, its first
    /// frame draws on arrival.
    pub fn remove(&mut self, key: ItemKey) {
        self.last.remove(&key);
        self.pending.remove(&key);
    }

    fn drawing(&mut self, items: Vec<TrayItem>, now: Instant) -> Vec<TrayItem> {
        for item in &items {
            self.last.insert(item.key, Some(item.content.frame.clone()));
        }
        self.last_draw = Some(now);
        self.drawn += items.len() as u64;
        items
    }
}

/// How the status items on screen change to match the wanted ones.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ItemChange {
    /// Shown but no longer wanted.
    pub remove: Vec<ItemKey>,
    /// Wanted but not shown, in the order to create them.
    pub create: Vec<ItemKey>,
}

/// What to remove and create so the items on screen are `wanted`. macOS puts a new
/// status item to the left of Kelvo's existing ones, so new items are created last to
/// first and come out in `wanted`'s (module) order. Once a position is saved under the
/// item's autosave name, macOS uses that instead. A key in `waiting` could not be
/// created earlier and is not tried again yet ([`Retries`]).
pub fn item_change(
    shown: &BTreeSet<ItemKey>,
    wanted: &[TrayItem],
    waiting: &BTreeSet<ItemKey>,
) -> ItemChange {
    let remove = shown
        .iter()
        .copied()
        .filter(|k| !wanted.iter().any(|w| w.key == *k))
        .collect();
    let create = wanted
        .iter()
        .rev()
        .map(|w| w.key)
        .filter(|k| !shown.contains(k) && !waiting.contains(k))
        .collect();
    ItemChange { remove, create }
}

/// How long a status item that could not be created waits before it is tried again.
pub const RETRY_AFTER: Duration = Duration::from_secs(30);

/// Status items that could not be created, and when each is tried again. A failure
/// that persists retries every [`RETRY_AFTER`] and is logged once per streak, not on
/// every frame.
#[derive(Debug, Default)]
pub struct Retries {
    next: BTreeMap<ItemKey, Instant>,
}

impl Retries {
    /// Records that creating `key` failed at `now`. True for the first failure of a
    /// streak (since the item was last created, or was last unwanted): the one to log.
    pub fn failed(&mut self, key: ItemKey, now: Instant) -> bool {
        self.next.insert(key, now + RETRY_AFTER).is_none()
    }

    /// `key` was created: its streak is over.
    pub fn created(&mut self, key: ItemKey) {
        self.next.remove(&key);
    }

    /// Forgets the keys no longer wanted, and returns those still waiting at `now`.
    pub fn waiting(&mut self, wanted: &[TrayItem], now: Instant) -> BTreeSet<ItemKey> {
        self.next.retain(|k, _| wanted.iter().any(|w| w.key == *k));
        self.next
            .iter()
            .filter(|(_, at)| **at > now)
            .map(|(k, _)| *k)
            .collect()
    }

    /// When the next waiting key is due, if any.
    pub fn deadline(&self) -> Option<Instant> {
        self.next.values().min().copied()
    }

    /// Makes every waiting key due at `now`: the settings changed or the display woke.
    /// The streaks are kept, so a failure that persists is not logged again.
    pub fn retry_now(&mut self, now: Instant) {
        for at in self.next.values_mut() {
            *at = now;
        }
    }
}

/// What a drawn frame has to change on the status item besides the image itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemChanges {
    /// The image's pixel size changed: the status item must resize (the slow path through
    /// tray-icon, which also resizes its click target). Otherwise only the image is
    /// swapped, at the same size.
    pub resize: bool,
    /// The VoiceOver label, when its words changed.
    pub label: Option<String>,
}

/// What the status item shows now, so a drawn frame touches only what differs.
#[derive(Debug, Default)]
pub struct ItemState {
    size: Option<(u32, u32)>,
    label: Option<String>,
}

impl ItemState {
    /// Records an image of `width` x `height` px with `label` as shown, and returns what
    /// differs from the last one.
    pub fn changes(&mut self, width: u32, height: u32, label: &str) -> ItemChanges {
        let resize = self.size != Some((width, height));
        self.size = Some((width, height));
        let label = (self.label.as_deref() != Some(label)).then(|| {
            self.label = Some(label.to_owned());
            label.to_owned()
        });
        ItemChanges { resize, label }
    }

    /// Forgets what is shown (a set failed), so the next frame takes the full path.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kelvo_schema::{Labels, MetricId, SeriesKey};

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

    fn set(s: &mut Settings, module: Module, mode: MenuBarMode) {
        s.modules.get_mut(&module).unwrap().menu_bar = mode;
    }

    fn values_settings() -> Settings {
        let mut s = Settings::default();
        for m in [Module::Cpu, Module::Gpu, Module::Memory] {
            s.modules.get_mut(&m).unwrap().menu_bar = MenuBarMode::ValueLabel;
        }
        s.modules.get_mut(&Module::Power).unwrap().menu_bar = MenuBarMode::WattsValue;
        s
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
        let s = TraySeries::resolve(&layout);
        assert_eq!(s.layout_no, Some(3));
        let r = s.readings(&[18.0, 50.0, 7.0, 100.0, 20.0, f32::NAN]);
        assert_eq!(r.cpu, Reading::Value(18.0));
        assert_eq!(r.net_bps, Reading::Value(120.0));
        assert_eq!(r.net_rx_bps, Reading::Value(100.0), "the total, not en0");
        assert_eq!(r.temp_c, Reading::Gap, "a stale value is a gap");
        assert_eq!(r.gpu, Reading::Absent);
        assert_eq!(r.disk_bps, Reading::Absent);
        // One direction stale: the labelled value is a gap, never rx alone.
        let r = s.readings(&[18.0, 50.0, 7.0, 100.0, f32::NAN, 61.0]);
        assert_eq!(r.net_bps, Reading::Gap);
        assert_eq!(r.net_rx_bps, Reading::Value(100.0));
        assert_eq!(r.net_tx_bps, Reading::Gap);
    }

    #[test]
    fn combined_default_quantizes_bars_to_device_pixels() {
        let c = combined(&sample(), &Settings::default(), false, 2);
        // 14 pt at 2x is 28 px: 18.2% -> 5, 36% -> 10, 42.4% -> 12.
        assert_eq!(c.frame.bars, vec![Some(5), Some(10), Some(12)]);
        assert_eq!(c.frame.combined_text.as_deref(), Some("142°"));
        assert!(c.frame.values.is_empty());
        assert_eq!(
            c.accessibility,
            "CPU 18 percent, GPU 36 percent, memory 42 percent, 142 degrees"
        );
        let c1 = combined(&sample(), &Settings::default(), false, 1);
        assert_eq!(c1.frame.bars, vec![Some(3), Some(5), Some(6)]);
    }

    #[test]
    fn values_layout_and_units() {
        let mut s = values_settings();
        s.modules.get_mut(&Module::Network).unwrap().menu_bar = MenuBarMode::ValueLabel;
        s.modules.get_mut(&Module::Battery).unwrap().menu_bar = MenuBarMode::ValueLabel;
        let c = combined(&sample(), &s, false, 2);
        assert!(c.frame.bars.is_empty());
        let got: Vec<(&str, &str)> = c
            .frame
            .values
            .iter()
            .map(|l| (l.label, l.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("CPU", "18%"),
                ("GPU", "36%"),
                ("MEM", "42%"),
                ("PWR", "14.8W"),
                ("NET", "38.4MB"),
                ("BAT", "87%"),
            ]
        );
        s.units.network = NetworkUnit::BitsPerSec;
        s.units.temperature = TemperatureUnit::Fahrenheit;
        s.modules.get_mut(&Module::Power).unwrap().menu_bar = MenuBarMode::TempInCombined;
        let c = combined(&sample(), &s, false, 2);
        // No bars, so the temperature gets its own label.
        assert_eq!(c.frame.values[0].label, "SOC");
        assert_eq!(c.frame.values[0].text, "142°");
        assert!(c.frame.values.iter().any(|l| l.text == "307.2Mb"));
    }

    #[test]
    fn disabled_absent_and_hidden_modules_are_not_drawn() {
        let mut s = Settings::default();
        s.modules.get_mut(&Module::Gpu).unwrap().enabled = false;
        s.modules.get_mut(&Module::Battery).unwrap().menu_bar = MenuBarMode::ValueLabel;
        let r = Readings {
            battery: Reading::Absent,
            ..sample()
        };
        let c = combined(&r, &s, false, 2);
        assert_eq!(c.frame.bars.len(), 2);
        assert!(c.frame.values.is_empty());

        for m in [Module::Cpu, Module::Gpu, Module::Memory, Module::Power] {
            s.modules.get_mut(&m).unwrap().menu_bar = MenuBarMode::Hidden;
        }
        s.modules.get_mut(&Module::Battery).unwrap().menu_bar = MenuBarMode::Hidden;
        let c = combined(&r, &s, false, 2);
        assert_eq!(
            c.frame.bars,
            vec![None; 3],
            "never an empty, unclickable item"
        );
    }

    #[test]
    fn paused_and_gaps_drop_to_tracks_and_dashes() {
        let c = combined(&sample(), &Settings::default(), true, 2);
        assert_eq!(c.frame.bars, vec![None; 3]);
        assert_eq!(c.frame.combined_text.as_deref(), Some("\u{2013}"));
        assert_eq!(c.accessibility, "Kelvo, sampling paused");

        let r = Readings {
            gpu: Reading::Gap,
            ..sample()
        };
        let c = combined(&r, &Settings::default(), false, 2);
        assert_eq!(c.frame.bars[1], None);
        assert!(c.accessibility.contains("GPU no data"));
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
        let r = TraySeries::resolve(&layout).readings(&[10.0, 1.0, 2.0, 0.0, f32::NAN]);
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
        set(&mut s, Module::Cpu, MenuBarMode::OwnGraph);
        set(&mut s, Module::Memory, MenuBarMode::OwnGraph);
        set(&mut s, Module::Network, MenuBarMode::OwnGraph);
        set(&mut s, Module::Gpu, MenuBarMode::Hidden);
        set(&mut s, Module::Power, MenuBarMode::Hidden);
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
        set(&mut s, Module::Cpu, MenuBarMode::OwnCores);
        set(&mut s, Module::Gpu, MenuBarMode::OwnGraph);
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

    #[test]
    fn own_values_leave_the_rest_in_the_combined_item() {
        let mut s = Settings::default();
        set(&mut s, Module::Cpu, MenuBarMode::OwnValue);
        set(&mut s, Module::Power, MenuBarMode::OwnValue);
        set(&mut s, Module::Battery, MenuBarMode::OwnValue);
        let items = build(&sample(), &TrayHistory::default(), &s, false, 2);
        let c = &items[0].content;
        assert_eq!(items[0].key, ItemKey::Combined);
        assert_eq!(c.frame.bars, [Some(10), Some(12)], "GPU and memory");
        assert_eq!(
            c.frame.combined_text, None,
            "power shows watts, not the temperature"
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

        // Back to the combined item: the own items are gone from the build.
        set(&mut s, Module::Cpu, MenuBarMode::InCombined);
        set(&mut s, Module::Power, MenuBarMode::TempInCombined);
        set(&mut s, Module::Battery, MenuBarMode::Hidden);
        let items = build(&sample(), &TrayHistory::default(), &s, false, 2);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, ItemKey::Combined);
    }

    #[test]
    fn paused_own_items_drop_to_tracks() {
        let mut s = Settings::default();
        set(&mut s, Module::Cpu, MenuBarMode::OwnGraph);
        set(&mut s, Module::Memory, MenuBarMode::OwnGraph);
        set(&mut s, Module::Network, MenuBarMode::OwnGraph);
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
}
