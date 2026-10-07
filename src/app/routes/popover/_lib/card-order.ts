import type { ModuleState, UiModule } from "@core/module-state";

export type PopoverCard =
  | "cpu"
  | "cores"
  | "memory"
  | "gpu"
  | "power"
  | "network"
  | "battery";

/** Plan 4.3 module order. Disk has no popover card in v1. */
const ORDER: readonly [PopoverCard, UiModule][] = [
  ["cpu", "cpu"],
  ["cores", "cpu"],
  ["memory", "memory"],
  ["gpu", "gpu"],
  ["power", "power"],
  ["network", "network"],
  ["battery", "battery"],
];

export type PopoverSlot =
  | { kind: "card"; card: PopoverCard }
  | { kind: "unavailable"; module: UiModule; state: ModuleState };

/**
 * The popover's cards in module order: enabled modules the host has. A
 * module the build cannot run gets one "not available" card in its place;
 * switched-off, absent and unknown-chip modules get nothing.
 */
export function popoverSlots(
  states: Record<UiModule, ModuleState>
): PopoverSlot[] {
  const out: PopoverSlot[] = [];
  const seen = new Set<UiModule>();
  for (const [card, module] of ORDER) {
    const state = states[module];
    if (state === "on") out.push({ kind: "card", card });
    else if (
      (state === "edition" || state === "unavailable") &&
      !seen.has(module)
    ) {
      out.push({ kind: "unavailable", module, state });
    }
    seen.add(module);
  }
  return out;
}
