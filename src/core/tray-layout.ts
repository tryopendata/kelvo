/**
 * What the menu bar draws for a set of menu bar settings (D-102), mirrored
 * from the Rust tray model (`src-tauri/src/tray/model.rs`, `build`) so the
 * Settings and onboarding previews show what the real status items will.
 * Rust draws the menu bar; this only lays out a preview of it.
 */
import type {
  ItemMode,
  MenuBarSettings,
  NetworkUnit,
  Readout,
  TemperatureUnit,
} from "@core/generated/bindings";
import { READOUTS } from "@core/generated/bindings";
import { SETTINGS_MODULES, type SettingsModule } from "@core/settings-patch";

/** Current values; `null` is a gap (a dash, an empty track). */
export interface TrayReadings {
  /** `cpu.total`, percent. */
  cpu: number | null;
  /** `gpu.util`, percent. */
  gpu: number | null;
  /** `mem.pressure`, percent. */
  mem: number | null;
  /** `thermal.hottest`, °C. */
  temp: number | null;
  /** `power.system`, W. */
  power: number | null;
  /** `net.tx_total` and `net.rx_total`, bytes/s. */
  netUp: number | null;
  netDown: number | null;
  /** `disk.read_total` + `disk.write_total`, bytes/s (the own Disk item). */
  diskRate: number | null;
  /** % of the boot volume used. */
  diskUsed: number | null;
  /** `battery.charge`, percent. */
  battery: number | null;
  /** Recent `cpu.total` and `gpu.util`, oldest first, for the own graphs. */
  cpuHistory?: readonly (number | null)[];
  gpuHistory?: readonly (number | null)[];
  /** `cpu.load` per core, P cores then E cores. */
  cores?: readonly (readonly (number | null)[])[];
}

export interface TrayUnits {
  temperature: TemperatureUnit;
  network: NetworkUnit;
}

/** What says which value a readout is. */
export type TrayMarker =
  | { kind: "label"; text: string }
  | { kind: "glyph"; glyph: "bolt" | "drive" }
  | { kind: "none" };

export type TrayReadout =
  | { readout: Readout; kind: "marked"; marker: TrayMarker; text: string }
  | { readout: Readout; kind: "rates"; up: string; down: string };

export type TrayOwnItem =
  | { module: SettingsModule; kind: "value"; label: string; text: string }
  | {
      module: SettingsModule;
      kind: "spark" | "hist";
      label: string;
      samples: readonly (number | null)[];
    }
  | {
      module: SettingsModule;
      kind: "gauge";
      label: string;
      pct: number | null;
      text: string;
    }
  | { module: SettingsModule; kind: "rates"; up: string; down: string }
  | {
      module: SettingsModule;
      kind: "cores";
      label: string;
      clusters: readonly (readonly (number | null)[])[];
    };

export interface TrayLayout {
  /** The combined item; null when it has nothing and an own item is there to click. */
  combined: { bars: (number | null)[]; readouts: TrayReadout[] } | null;
  /** Own items, in module order. */
  items: TrayOwnItem[];
}

const DASH = "–";

/** The module whose settings switch turns a readout on and off. */
export function readoutModule(r: Readout): SettingsModule {
  return r === "temperature" ? "power" : r;
}

export function pctText(v: number | null): string {
  return v === null ? DASH : `${Math.round(Math.min(999, Math.max(0, v)))}%`;
}

export function tempText(c: number | null, unit: TemperatureUnit): string {
  if (c === null) return DASH;
  return `${Math.round(unit === "fahrenheit" ? (c * 9) / 5 + 32 : c)}°`;
}

export function wattsText(w: number | null): string {
  if (w === null) return DASH;
  return w < 100 ? `${w.toFixed(1)}W` : `${w.toFixed(0)}W`;
}

/** Rust's `rate_line`: "38.4 MB/s", "512 KB/s", bits with a lowercase b. */
export function rateLine(bps: number | null, unit: NetworkUnit): string {
  if (bps === null) return DASH;
  const bits = unit === "bits_per_sec";
  const v = Math.max(0, bits ? bps * 8 : bps);
  const suffix = bits ? "b" : "B";
  const scaled = (x: number, prefix: string) =>
    Math.round(x * 10) >= 1000
      ? `${x.toFixed(0)} ${prefix}${suffix}/s`
      : `${x.toFixed(1)} ${prefix}${suffix}/s`;
  if (v >= 1e9) return scaled(v / 1e9, "G");
  if (v >= 1e6) return scaled(v / 1e6, "M");
  return `${Math.floor(v / 1e3)} K${suffix}/s`;
}

/** Rust's `rate_text`: "38.4MB", "512KB". */
export function rateText(bps: number | null, unit: NetworkUnit): string {
  if (bps === null) return DASH;
  const bits = unit === "bits_per_sec";
  const v = Math.max(0, bits ? bps * 8 : bps);
  const suffix = bits ? "b" : "B";
  if (v >= 1e9) return `${(v / 1e9).toFixed(1)}G${suffix}`;
  if (v >= 1e6) return `${(v / 1e6).toFixed(1)}M${suffix}`;
  return `${Math.floor(v / 1e3)}K${suffix}`;
}

function rates(r: TrayReadings, unit: NetworkUnit) {
  return {
    up: `${rateLine(r.netUp, unit)} ↑`,
    down: `${rateLine(r.netDown, unit)} ↓`,
  };
}

function readout(r: Readout, v: TrayReadings, units: TrayUnits): TrayReadout {
  switch (r) {
    case "cpu":
      return marked(r, label("CPU"), pctText(v.cpu));
    case "gpu":
      return marked(r, label("GPU"), pctText(v.gpu));
    case "memory":
      return marked(r, label("MEM"), pctText(v.mem));
    case "battery":
      return marked(r, label("BAT"), pctText(v.battery));
    case "temperature":
      return marked(r, { kind: "none" }, tempText(v.temp, units.temperature));
    case "power":
      return marked(r, { kind: "glyph", glyph: "bolt" }, wattsText(v.power));
    case "disk":
      return marked(r, { kind: "glyph", glyph: "drive" }, pctText(v.diskUsed));
    case "network":
      return { readout: r, kind: "rates", ...rates(v, units.network) };
  }
}

const label = (text: string): TrayMarker => ({ kind: "label", text });
const marked = (
  readout: Readout,
  marker: TrayMarker,
  text: string
): TrayReadout => ({ readout, kind: "marked", marker, text });

const PCT_LABEL = { cpu: "CPU", gpu: "GPU", memory: "MEM" } as const;

function ownItem(
  m: SettingsModule,
  mode: ItemMode,
  v: TrayReadings,
  units: TrayUnits
): TrayOwnItem {
  if (m === "cpu" || m === "gpu" || m === "memory") {
    const pct = m === "cpu" ? v.cpu : m === "gpu" ? v.gpu : v.mem;
    const lbl = PCT_LABEL[m];
    if (mode === "graph" && m === "cpu") {
      return {
        module: m,
        kind: "spark",
        label: lbl,
        samples: v.cpuHistory ?? [pct],
      };
    }
    if (mode === "graph" && m === "gpu") {
      return {
        module: m,
        kind: "hist",
        label: lbl,
        samples: v.gpuHistory ?? [pct],
      };
    }
    if (mode === "graph") {
      return { module: m, kind: "gauge", label: lbl, pct, text: pctText(pct) };
    }
    if (mode === "cores" && v.cores && v.cores.length > 0) {
      return { module: m, kind: "cores", label: lbl, clusters: v.cores };
    }
    return { module: m, kind: "value", label: lbl, text: pctText(pct) };
  }
  if (m === "network" && mode === "graph") {
    return { module: m, kind: "rates", ...rates(v, units.network) };
  }
  switch (m) {
    case "power":
      return {
        module: m,
        kind: "value",
        label: "PWR",
        text: wattsText(v.power),
      };
    case "network": {
      const total =
        v.netUp === null || v.netDown === null ? null : v.netUp + v.netDown;
      return {
        module: m,
        kind: "value",
        label: "NET",
        text: rateText(total, units.network),
      };
    }
    case "disk":
      return {
        module: m,
        kind: "value",
        label: "DSK",
        text: rateText(v.diskRate, units.network),
      };
    default:
      return {
        module: m,
        kind: "value",
        label: "BAT",
        text: pctText(v.battery),
      };
  }
}

const BAR_MODULES = ["cpu", "gpu", "memory"] as const;

/**
 * The menu bar for `menuBar`, with `enabled` saying which modules are on and
 * present. The same rules as Rust's `build`: bars, then readouts in
 * `READOUTS` order; own items in module order; the combined item goes away
 * when it is empty and an own item exists, and shows three empty tracks when
 * nothing at all is on.
 */
export function trayLayout(
  menuBar: MenuBarSettings,
  enabled: (m: SettingsModule) => boolean,
  v: TrayReadings,
  units: TrayUnits
): TrayLayout {
  const bars = BAR_MODULES.filter((m) => menuBar.bars[m] && enabled(m)).map(
    (m) => (m === "cpu" ? v.cpu : m === "gpu" ? v.gpu : v.mem)
  );
  const readouts = READOUTS.filter(
    (r) => menuBar.readouts[r] && enabled(readoutModule(r))
  ).map((r) => readout(r, v, units));
  const items = SETTINGS_MODULES.filter(
    (m) => enabled(m) && menuBar.items[m] !== "off"
  ).map((m) => ownItem(m, menuBar.items[m], v, units));

  const empty = bars.length === 0 && readouts.length === 0;
  if (empty && items.length > 0) return { combined: null, items };
  return {
    combined: { bars: empty ? [null, null, null] : bars, readouts },
    items,
  };
}

// Geometry in points, from the Rust renderer (design-system.md, Tray icon spec).
const MONO_ADVANCE = 0.6;
const VALUE_PT = 12;
const RATE_PT = 8;
const BAR_PITCH = 7;
const BAR_WIDTH = 3;
const LABELLED = 10;
const GLYPH_WIDTH = { bolt: 7, drive: 11 } as const;
const GLYPH_GAP = 3;
const GROUP_GAP = 10;
const RATE_MIN_CHARS = 11;

/** Characters a value reserves after `marker`, so the item keeps its width. */
function minChars(marker: TrayMarker): number {
  if (marker.kind === "glyph") return marker.glyph === "bolt" ? 5 : 3;
  if (marker.kind === "label" && marker.text === "PWR") return 5;
  return 3;
}

function readoutWidth(r: TrayReadout): number {
  if (r.kind === "rates") {
    const chars = Math.max(RATE_MIN_CHARS, r.up.length, r.down.length);
    return chars * RATE_PT * MONO_ADVANCE;
  }
  const marker =
    r.marker.kind === "label"
      ? LABELLED
      : r.marker.kind === "glyph"
        ? GLYPH_WIDTH[r.marker.glyph] + GLYPH_GAP
        : 0;
  const chars = Math.max(minChars(r.marker), r.text.length);
  return marker + chars * VALUE_PT * MONO_ADVANCE;
}

/**
 * The combined item's width in points, as the Rust renderer lays it out
 * (within a point or two: text widths are taken as mono advances).
 */
export function combinedWidthPt(combined: TrayLayout["combined"]): number {
  if (!combined) return 0;
  const n = combined.bars.length;
  let w = n > 0 ? n * BAR_WIDTH + (n - 1) * (BAR_PITCH - BAR_WIDTH) : 0;
  combined.readouts.forEach((r, i) => {
    if (i > 0 || n > 0) {
      const afterBars = i === 0 && n > 0;
      const bare = r.kind === "marked" && r.marker.kind === "none";
      w += afterBars ? (bare ? 4 : 6) : GROUP_GAP;
    }
    w += readoutWidth(r);
  });
  return w;
}

/**
 * Past this the combined item risks sitting behind the camera notch on a
 * MacBook, where macOS hides status items that do not fit (D-102).
 */
export const WIDE_ITEM_PT = 150;
