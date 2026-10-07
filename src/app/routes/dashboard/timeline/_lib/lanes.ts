/**
 * The six Timeline lanes (plan 4.6 table), top to bottom, and the metrics
 * each one queries. A lane's `module` decides its query key and which
 * `module_disabled` gaps cover it.
 */
import type { Module } from "@core/generated/bindings";
import type { Accent } from "~/widgets/lib/accent";
import type { Combine } from "./buckets";

export type LaneId = "cpu" | "gpu" | "memory" | "power" | "temp" | "network";

export interface LaneMetric {
  metric: string;
  combine: Combine;
  /** Drawn in the plot; otherwise only used for the sub line or tooltip. */
  plotted: boolean;
}

export interface LaneDef {
  id: LaneId;
  label: string;
  /** Capability and gap module. Temperature lives under Sensors. */
  module: Module;
  accent: Accent;
  metrics: LaneMetric[];
  /** Accessible name of the plot. */
  ariaLabel: string;
}

const m = (metric: string, plotted = true, combine: Combine = "sum") => ({
  metric,
  combine,
  plotted,
});

export const LANES: readonly LaneDef[] = [
  {
    id: "cpu",
    label: "CPU",
    module: "cpu",
    accent: "cpu",
    metrics: [m("cpu.total")],
    ariaLabel: "CPU usage",
  },
  {
    id: "gpu",
    label: "GPU",
    module: "gpu",
    accent: "gpu",
    metrics: [m("gpu.util"), m("gpu.freq", false)],
    ariaLabel: "GPU usage",
  },
  {
    id: "memory",
    label: "Memory",
    module: "memory",
    accent: "mem",
    metrics: [m("mem.pressure")],
    ariaLabel: "Memory pressure",
  },
  {
    id: "power",
    label: "Power",
    module: "power",
    accent: "power",
    // System first (faint), CPU on top (solid).
    metrics: [m("power.system"), m("power.cpu")],
    ariaLabel: "Power draw: CPU solid, system faint",
  },
  {
    id: "temp",
    label: "Temperature",
    module: "sensors",
    accent: "temp",
    metrics: [m("thermal.hottest"), m("fan.rpm", false, "max")],
    ariaLabel: "Hottest SoC zone temperature",
  },
  {
    id: "network",
    label: "Network",
    module: "network",
    accent: "net",
    // Upload above the baseline, download below: the totals over interfaces (D-092).
    metrics: [m("net.tx_total"), m("net.rx_total")],
    ariaLabel: "Network: upload above the line, download below",
  },
];

/** Lane plot height and the space between lanes. */
export const LANE_HEIGHT = 64;
export const LANE_GAP = 12;
