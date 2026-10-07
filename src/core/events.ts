/**
 * Detector and alert events (v1.2, D-083): labels for the Timeline's pills
 * and the Power chart, and merging a pushed `event-recorded` into a list
 * already fetched.
 */
import type { Event, HostId, ThermalState } from "@core/generated/bindings";

const THERMAL: Record<ThermalState, string> = {
  nominal: "Nominal",
  fair: "Fair",
  serious: "Serious",
  critical: "Critical",
};

const pct = (v: number | null) => (v === null ? "?" : `${Math.round(v)}%`);
const watts = (v: number | null) => (v === null ? "?" : `${v.toFixed(1)} W`);

function duration(secs: number): string {
  return secs < 120 ? `${secs} s` : `${Math.round(secs / 60)} min`;
}

/** Process names joined with " + ": "kernel_task + Xcode build". */
function blame(processes: readonly string[]): string {
  return processes.length > 0 ? ` · ${processes.join(" + ")}` : "";
}

/**
 * What happened, without the time: "Fans ramped up · kernel_task + Xcode
 * build", "ANE 1.4 W · Photos face analysis".
 */
export function eventLabel(e: Event): string {
  const d = e.detail;
  switch (d.kind) {
    case "fans_ramped":
      return `Fans ramped up${blame(e.processes)}`;
    case "thermal_state":
      return `Thermal state ${THERMAL[d.to]}`;
    case "sustained_process":
      return `${d.process} at ${pct(d.cpu_pct)} CPU for ${duration(d.secs)}`;
    case "power_spike":
      return `${d.component === "ane" ? "ANE" : "Package"} ${watts(d.watts)}${blame(e.processes.slice(0, 1))}`;
    case "alert":
      return d.cause.type === "process_cpu"
        ? `Alert · ${d.cause.process} at ${pct(d.cause.cpu_pct)} CPU`
        : `Alert · thermal state ${THERMAL[d.cause.state]}`;
  }
}

/** Identity of a stored event: the store keeps one row per time and kind. */
export function eventId(e: Event): string {
  return `${e.detail.kind}@${e.ts_ms}`;
}

/**
 * `list` with `e` added in time order, or `list` itself when it already
 * holds it (a fetch that raced the push, or a second listener).
 */
export function mergeEvent(list: readonly Event[], e: Event): Event[] {
  const id = eventId(e);
  if (list.some((x) => eventId(x) === id)) return list as Event[];
  return [...list, e].sort((a, b) => a.ts_ms - b.ts_ms);
}

/**
 * Events pushed in the last `ttlMs`, per host. `event-recorded` goes out
 * once the commit is queued, not once it lands (D-083), so a read in that
 * window can answer without the event; merging these into every answer
 * covers it. `ttlMs` sits well above the writer's commit round trip, and
 * at most `max` events are kept per host.
 */
export class RecentEvents {
  readonly #byHost = new Map<HostId, { at: number; event: Event }[]>();
  readonly #ttlMs: number;
  readonly #max: number;
  readonly #now: () => number;

  constructor(ttlMs = 10_000, max = 64, now: () => number = Date.now) {
    this.#ttlMs = ttlMs;
    this.#max = max;
    this.#now = now;
  }

  add(host: HostId, event: Event): void {
    const list = this.#live(host);
    list.push({ at: this.#now(), event });
    if (list.length > this.#max) list.shift();
  }

  /** `answer` with every recent event in `[fromMs, toMs)` merged in. */
  mergeInto(
    host: HostId,
    answer: readonly Event[],
    fromMs: number,
    toMs: number
  ): Event[] {
    let out = answer as Event[];
    for (const { event } of this.#live(host)) {
      if (event.ts_ms >= fromMs && event.ts_ms < toMs) {
        out = mergeEvent(out, event);
      }
    }
    return out;
  }

  #live(host: HostId): { at: number; event: Event }[] {
    const since = this.#now() - this.#ttlMs;
    const list = (this.#byHost.get(host) ?? []).filter((r) => r.at >= since);
    this.#byHost.set(host, list);
    return list;
  }
}
