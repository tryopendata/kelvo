//! What the tray items show, reduced to what the icons can actually display (architecture.md,
//! Tray rendering pipeline). Pure: no Tauri, no AppKit, so the quantizing, the frame skip
//! and pacing, the accessibility label and what a drawn frame changes on the status item
//! are tested without a menu bar.
//!
//! The menu bar settings (D-102) decide the elements: CPU, GPU and Memory bars in the
//! combined item, then the readouts after them in [`Readout::ALL`] order, each a marker
//! and a value (a stacked label, a glyph, nothing for "61°", or the two-line network
//! rates). A module's own item (D-080) is its labelled value or its graph (CPU sparkline
//! or per-core strip, GPU history bars, memory fill gauge, network rates). The sparkline
//! and history bars read [`TrayHistory`], a bounded ring fed by every frame, never a
//! history query.

use std::collections::VecDeque;

use kelvo_engine::FrameLayout;
use kelvo_schema::settings::{BarSettings, ItemMode, NetworkUnit, Readout};
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
    /// `disk.used` and `disk.total` of the boot volume (`HostInfo.boot_mounts[0]`).
    disk_used: Option<usize>,
    disk_total: Option<usize>,
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
    /// `boot_mount` is the volume the Disk readout shows (`HostInfo.boot_mounts[0]`);
    /// without one the readout is absent.
    pub fn resolve(layout: &FrameLayout, boot_mount: Option<&str>) -> Self {
        let mut s = Self {
            layout_no: Some(layout.layout_no),
            ..Self::default()
        };
        let mut cores: Vec<((u8, char, u32), usize)> = Vec::new();
        for (i, key) in layout.series.iter().enumerate() {
            let bare = key.labels.is_empty();
            let boot = boot_mount.is_some() && key.labels.get("vol") == boot_mount;
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
                "disk.used" if boot => s.disk_used = Some(i),
                "disk.total" if boot => s.disk_total = Some(i),
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
        let disk_used_pct = match (one(self.disk_used), one(self.disk_total)) {
            (Reading::Value(used), Reading::Value(total)) if total > 0.0 => {
                Reading::Value(used / total * 100.0)
            }
            (Reading::Absent, Reading::Absent) => Reading::Absent,
            _ => Reading::Gap,
        };
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
            disk_used_pct,
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
    /// % of the boot volume used.
    pub disk_used_pct: Reading,
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

/// A labelled value in an own item: a stacked three-letter label and the value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Labeled {
    pub label: &'static str,
    pub text: String,
}

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

/// A glyph drawn before a readout's value (design-system.md, Tray icon spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    /// Power: a filled bolt.
    Bolt,
    /// Disk used: an outlined drive with an activity dot.
    Drive,
}

/// What says which value a readout is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Marker {
    /// A stacked three-letter label ("CPU").
    Label(&'static str),
    Glyph(Glyph),
    /// The value says it itself ("61°").
    None,
}

impl Marker {
    /// Characters of width the value after this marker always reserves.
    pub fn min_chars(self) -> usize {
        match self {
            Marker::Label(l) => min_chars(l),
            Marker::Glyph(Glyph::Bolt) => 5, // "14.8W"
            Marker::Glyph(Glyph::Drive) | Marker::None => 3,
        }
    }
}

/// One readout in the combined item, after the bars.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReadoutFrame {
    Marked {
        marker: Marker,
        text: String,
    },
    /// Network up over down, drawn as the own item's rates.
    Graph(Graph),
}

impl ReadoutFrame {
    fn marked(marker: Marker, text: String) -> Self {
        ReadoutFrame::Marked { marker, text }
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
    /// The readouts after the bars, in [`Readout::ALL`] order.
    pub readouts: Vec<ReadoutFrame>,
    /// An own item's labelled value.
    pub values: Vec<Labeled>,
    /// Graphs, after the values (an own item has one).
    pub graphs: Vec<Graph>,
}

impl TrayFrame {
    pub fn empty(scale: u32) -> Self {
        Self {
            scale,
            bars: Vec::new(),
            readouts: Vec::new(),
            values: Vec::new(),
            graphs: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.bars.is_empty()
            && self.readouts.is_empty()
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

fn enabled(settings: &Settings, module: Module) -> bool {
    settings.module(module).is_some_and(|m| m.enabled)
}

/// `module`'s own item, `Off` when the module is switched off.
fn item_mode(settings: &Settings, module: Module) -> ItemMode {
    if enabled(settings, module) {
        settings.menu_bar.items.get(module)
    } else {
        ItemMode::Off
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

/// Collects the combined item's elements and the own items as modules are visited.
struct Layout {
    scale: u32,
    paused: bool,
    combined: TrayFrame,
    bar_words: Vec<String>,
    readout_words: Vec<String>,
    own: Vec<TrayItem>,
}

impl Layout {
    fn own_value(&mut self, module: Module, value: Labeled, words: String) {
        let mut frame = TrayFrame::empty(self.scale);
        frame.values.push(value);
        self.own_item(module, frame, words);
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

/// A percentage reading's text and words ("CPU 18 percent").
fn pct_parts(r: Reading, word: &str) -> (String, String) {
    match r.value() {
        Some(v) => (
            pct_text(v),
            format!("{word} {} percent", v.clamp(0.0, 999.0).round() as i32),
        ),
        None => (DASH.into(), format!("{word} no data")),
    }
}

/// Network up over down, in the stacked two-line form, and its words.
fn rates_parts(tx: Reading, rx: Reading, unit: NetworkUnit) -> (Graph, Option<String>) {
    let line = |r: Reading| {
        r.value()
            .map_or_else(|| DASH.into(), |v| rate_line(v, unit))
    };
    let graph = Graph::Rates {
        up: format!("{} \u{2191}", line(tx)),
        down: format!("{} \u{2193}", line(rx)),
    };
    let words = match (tx, rx) {
        (Reading::Value(tx), Reading::Value(rx)) => Some(format!(
            "network up {}, down {}",
            rate_words(tx, unit),
            rate_words(rx, unit)
        )),
        _ => None,
    };
    (graph, words)
}

/// Builds every status item for `readings` under `settings` (D-102): the combined item
/// first (bars, then the readouts in [`Readout::ALL`] order), if it has anything to show
/// or nothing else is shown, then the own items in module order. `paused` drops bars and
/// graphs to their tracks and shows a dash for every text, as a gap does.
pub fn build(
    readings: &Readings,
    history: &TrayHistory,
    settings: &Settings,
    paused: bool,
    scale: u32,
) -> Vec<TrayItem> {
    let unit_t = settings.units.temperature;
    let unit_n = settings.units.network;
    let mb = &settings.menu_bar;
    let shown = |r: Reading| if paused { Reading::Gap } else { r };
    let mut l = Layout {
        scale,
        paused,
        combined: TrayFrame::empty(scale),
        bar_words: Vec::new(),
        readout_words: Vec::new(),
        own: Vec::new(),
    };

    let pct_of = |module: Module| match module {
        Module::Cpu => readings.cpu,
        Module::Gpu => readings.gpu,
        _ => readings.mem,
    };
    for module in BarSettings::MODULES {
        let reading = pct_of(module);
        if !mb.bars.get(module) || !enabled(settings, module) || reading == Reading::Absent {
            continue;
        }
        let r = shown(reading);
        l.combined.bars.push(r.value().map(|v| bar_px(v, scale)));
        l.bar_words.push(pct_parts(r, words_name(module)).1);
    }

    for readout in Readout::ALL {
        if !mb.readouts.get(readout) || !enabled(settings, readout.module()) {
            continue;
        }
        let (frame, words) = match readout {
            Readout::Cpu | Readout::Gpu | Readout::Memory | Readout::Battery => {
                let (reading, label) = match readout {
                    Readout::Cpu => (readings.cpu, "CPU"),
                    Readout::Gpu => (readings.gpu, "GPU"),
                    Readout::Memory => (readings.mem, "MEM"),
                    _ => (readings.battery, "BAT"),
                };
                if reading == Reading::Absent {
                    continue;
                }
                let (text, words) = pct_parts(shown(reading), words_name(readout.module()));
                (ReadoutFrame::marked(Marker::Label(label), text), words)
            }
            Readout::Temperature => {
                if readings.temp_c == Reading::Absent {
                    continue;
                }
                let r = shown(readings.temp_c);
                let text = r
                    .value()
                    .map_or_else(|| DASH.into(), |c| temp_text(c, unit_t));
                let words = match r.value() {
                    Some(_) => format!("temperature {} degrees", text.trim_end_matches('°')),
                    None => "temperature no data".into(),
                };
                // The degree sign says what it is: no marker (D-102).
                (ReadoutFrame::marked(Marker::None, text), words)
            }
            Readout::Power => {
                if readings.watts == Reading::Absent {
                    continue;
                }
                let r = shown(readings.watts);
                let text = r.value().map_or_else(|| DASH.into(), watts_text);
                let words = match r.value() {
                    Some(w) => format!("power {w:.1} watts"),
                    None => "power no data".into(),
                };
                (
                    ReadoutFrame::marked(Marker::Glyph(Glyph::Bolt), text),
                    words,
                )
            }
            Readout::Network => {
                if readings.net_bps == Reading::Absent {
                    continue;
                }
                let (graph, words) = rates_parts(
                    shown(readings.net_tx_bps),
                    shown(readings.net_rx_bps),
                    unit_n,
                );
                let words = words.unwrap_or_else(|| "network no data".into());
                (ReadoutFrame::Graph(graph), words)
            }
            Readout::Disk => {
                if readings.disk_used_pct == Reading::Absent {
                    continue;
                }
                let (text, words) = pct_parts(shown(readings.disk_used_pct), "disk");
                let words = if text == DASH {
                    words
                } else {
                    format!("{words} used")
                };
                (
                    ReadoutFrame::marked(Marker::Glyph(Glyph::Drive), text),
                    words,
                )
            }
        };
        l.combined.readouts.push(frame);
        l.readout_words.push(words);
    }

    // Own items, in module order.
    let pct_modules = [
        (Module::Cpu, "CPU"),
        (Module::Gpu, "GPU"),
        (Module::Memory, "MEM"),
    ];
    for (module, label) in pct_modules {
        let reading = pct_of(module);
        let mode = item_mode(settings, module);
        if reading == Reading::Absent || mode == ItemMode::Off {
            continue;
        }
        let r = shown(reading);
        let (text, words) = pct_parts(r, words_name(module));
        match (mode, module) {
            (ItemMode::Graph, Module::Cpu) => {
                let points = ring_px(&history.cpu, SPARK_RANGE_PT, scale, paused);
                l.graph(module, Graph::Spark { label, points }, words);
            }
            (ItemMode::Graph, Module::Gpu) => {
                let bars = ring_px(&history.gpu, BOX_FILL_PT, scale, paused);
                l.graph(module, Graph::Hist { label, bars }, words);
            }
            (ItemMode::Graph, _) => {
                let fill = r.value().map(|v| px(v, BOX_FILL_PT, scale));
                l.graph(module, Graph::Gauge { label, fill, text }, words);
            }
            (ItemMode::Cores, _) if !readings.cores.is_empty() => {
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
            // Value, or Cores on a host without per-core series: the value is still
            // worth an item.
            _ => l.own_value(module, Labeled { label, text }, words),
        }
    }

    if item_mode(settings, Module::Power) == ItemMode::Value && readings.watts != Reading::Absent {
        let r = shown(readings.watts);
        let value = Labeled {
            label: "PWR",
            text: r.value().map_or_else(|| DASH.into(), watts_text),
        };
        let words = match r.value() {
            Some(w) => format!("power {w:.1} watts"),
            None => "power no data".into(),
        };
        l.own_value(Module::Power, value, words);
    }

    let rate_modules = [
        (Module::Network, readings.net_bps, "NET", "network"),
        (Module::Disk, readings.disk_bps, "DSK", "disk"),
    ];
    for (module, reading, label, word) in rate_modules {
        let mode = item_mode(settings, module);
        if reading == Reading::Absent || mode == ItemMode::Off {
            continue;
        }
        let r = shown(reading);
        let words = match r.value() {
            Some(v) => format!("{word} {}", rate_words(v, unit_n)),
            None => format!("{word} no data"),
        };
        if mode == ItemMode::Graph {
            let (graph, rate_words) = rates_parts(
                shown(readings.net_tx_bps),
                shown(readings.net_rx_bps),
                unit_n,
            );
            l.graph(module, graph, rate_words.unwrap_or(words));
        } else {
            let value = Labeled {
                label,
                text: r
                    .value()
                    .map_or_else(|| DASH.into(), |v| rate_text(v, unit_n)),
            };
            l.own_value(module, value, words);
        }
    }

    if item_mode(settings, Module::Battery) == ItemMode::Value
        && readings.battery != Reading::Absent
    {
        let (text, words) = pct_parts(shown(readings.battery), "battery");
        l.own_value(Module::Battery, Labeled { label: "BAT", text }, words);
    }

    // The combined item goes away when it has nothing to show and an own item is there to
    // click; with neither, it shows three empty tracks, never an empty, unclickable item.
    if l.combined.is_empty() && l.own.is_empty() {
        l.combined.bars = vec![None; 3];
    }
    let mut items = Vec::with_capacity(l.own.len() + 1);
    if !l.combined.is_empty() {
        let mut words: Vec<String> = l.bar_words;
        words.extend(l.readout_words);
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
