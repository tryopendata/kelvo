import type { Settings } from "@core/generated/bindings";
import { defaultSettings, scenarioFlags } from "@core/mock/fixtures";
import { menuBarOf } from "@core/settings-patch";
import {
  appKeys,
  historyKeys,
  hostKeys,
  settingsInvalidation,
} from "./query-keys";

const H = "h1";
const base = defaultSettings(scenarioFlags(["default"]));
const RANGE = { span: "1h", endMs: null, bucketMs: 10_000 } as const;

const KEYS = {
  historyRange: historyKeys.range(H, "cpu", RANGE, "auto"),
  maxima: historyKeys.maxima(H, "gpu", "gpu.power"),
  historySize: hostKeys.historySize(H),
  historyHealth: hostKeys.historyHealth(H),
  hostDetail: hostKeys.detail(H),
  capabilities: hostKeys.capabilities(H),
  sensorDump: hostKeys.sensorDump(H),
  updates: appKeys.updates,
  appearance: appKeys.appearance,
};

/** The names of the keys a change from `prev` to `next` invalidates. */
function stale(next: Settings, prev: Settings | null = base): string[] {
  const match = settingsInvalidation(prev, next);
  return Object.entries(KEYS)
    .filter(([, key]) => match(key))
    .map(([name]) => name);
}

describe("settingsInvalidation", () => {
  it("retention or size limit: history, its size and its health", () => {
    const next = { ...base, history: { ...base.history, retention_days: 7 } };
    expect(stale(next)).toEqual([
      "historyRange",
      "maxima",
      "historySize",
      "historyHealth",
    ]);
  });

  it("a module switch: history (module_disabled gaps) and size", () => {
    const next: Settings = {
      ...base,
      modules: { ...base.modules, gpu: { enabled: false } },
    };
    expect(stale(next)).toEqual(["historyRange", "maxima", "historySize"]);
  });

  it("a menu bar change alone invalidates nothing", () => {
    const menu_bar = menuBarOf(base);
    const next: Settings = {
      ...base,
      menu_bar: {
        ...menu_bar,
        readouts: { ...menu_bar.readouts, power: !menu_bar.readouts.power },
      },
    };
    expect(stale(next)).toEqual([]);
  });

  it("the interval: the size projection only", () => {
    const next = { ...base, sampling: { ...base.sampling, interval_ms: 2000 } };
    expect(stale(next)).toEqual(["historySize"]);
  });

  it("the update switch: the update check only", () => {
    const next = {
      ...base,
      general: { ...base.general, check_updates: false },
    };
    expect(stale(next)).toEqual(["updates"]);
  });

  it("appearance, units and onboarding invalidate nothing", () => {
    expect(
      stale({ ...base, general: { ...base.general, appearance: "dark" } })
    ).toEqual([]);
    expect(
      stale({ ...base, units: { ...base.units, temperature: "fahrenheit" } })
    ).toEqual([]);
    expect(stale({ ...base, onboarding: { completed: false } })).toEqual([]);
    expect(stale(structuredClone(base))).toEqual([]);
  });

  it("the heatmap is history: retention drops it, the interval does not", () => {
    const hour = new Date(2026, 9, 4, 14).getTime();
    const key = historyKeys.heatmap(H, "cpu", hour, 30);
    expect(key.slice(0, 2)).toEqual(historyKeys.host(H));
    const retention = settingsInvalidation(base, {
      ...base,
      history: { ...base.history, retention_days: 7 },
    });
    const interval = settingsInvalidation(base, {
      ...base,
      sampling: { ...base.sampling, interval_ms: 2000 },
    });
    expect(retention(key)).toBe(true);
    expect(interval(key)).toBe(false);
    expect(key).not.toEqual(historyKeys.heatmap(H, "temp", hour, 30));
  });

  it("with no previous settings every dependent key is stale", () => {
    expect(stale(base, null)).toEqual([
      "historyRange",
      "maxima",
      "historySize",
      "historyHealth",
      "updates",
    ]);
  });
});
