import type { SeriesSelector } from "@core/generated/bindings";

/**
 * Every metric the popover reads (cards, header, footer), so its live
 * channel carries only these (D-066 projection): 50 series of the mock's
 * 170-series wide layout, and of the default mock's 110. A metric missing
 * here reads as a gap in the popover, so the popover route test renders
 * every card against this projection.
 */
export const POPOVER_METRICS = [
  // CPU card and cores card.
  "cpu.total",
  "cpu.user",
  "cpu.system",
  "cpu.load",
  "cpu.cluster.freq",
  // Memory card.
  "mem.used",
  "mem.app",
  "mem.wired",
  "mem.compressed",
  "mem.cached",
  "mem.free",
  "mem.pressure",
  "mem.pressure_level",
  "mem.swap_used",
  // GPU card.
  "gpu.util",
  "gpu.render",
  "gpu.tiler",
  "gpu.freq",
  // Power card, with the CPU power source label (D-065).
  "power.system",
  "power.cpu",
  "power.cpu_source",
  "power.gpu",
  "power.ane",
  "power.dram",
  "power.package",
  // Network card.
  "net.rx_total",
  "net.tx_total",
  "net.link_rate",
  // Battery card.
  "battery.charge",
  "battery.charging",
  "battery.external",
  "battery.time_remaining",
  "battery.health",
  "battery.cycles",
  // Footer: Kelvo's own CPU.
  "self.cpu",
] as const;

export const POPOVER_SERIES: readonly SeriesSelector[] = POPOVER_METRICS.map(
  (metric) => ({ metric, labels: [] })
);
