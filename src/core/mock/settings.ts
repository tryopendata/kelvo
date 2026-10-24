import type {
  Settings,
  SettingsPatch,
  SettingsSnapshot,
} from "@core/generated/bindings";
import { validChartRange } from "../chart-range";
import { INTERVALS_MS, menuBarOf, SIZE_LIMITS_MB } from "../settings-patch";
import type { MockCommands } from "./commands";
import type { MockTransport, MockTransportOptions } from "./contracts";
import type { MockEmitters } from "./emitters";
import { defaultSettings, type ScenarioFlags } from "./fixtures";
import { err, ok } from "./protocol";
export class MockSettings {
  snapshot: SettingsSnapshot;
  onChanged: (before: Settings, after: Settings) => void = () => {};
  constructor(
    private readonly options: MockTransportOptions,
    flags: ScenarioFlags,
    private readonly commands: MockCommands,
    private readonly emitters: MockEmitters
  ) {
    const baseSettings = defaultSettings(flags);
    if (baseSettings.docker)
      baseSettings.docker.enabled =
        (options.scenarios ?? []).some((s) => s.startsWith("docker-")) &&
        !(options.scenarios ?? []).includes("docker-retained");

    const startInterval = options.intervalMs ?? 1000;
    this.snapshot = {
      revision: 1,
      settings: {
        ...baseSettings,
        sampling: { ...baseSettings.sampling, interval_ms: startInterval },
        general: {
          ...baseSettings.general,
          ...(options.chartWindow === undefined
            ? {}
            : { chart_window: options.chartWindow }),
        },
      },
    };
  }
  readonly getSettings: MockTransport["getSettings"] = async () => {
    this.commands.record("get_settings");
    return this.snapshot;
  };
  readonly updateSettings: MockTransport["updateSettings"] = async (patch) => {
    this.commands.record("update_settings", patch);
    if (this.options.settingsNotSaved) {
      return err({ kind: "settings_not_saved", message: "disk full" });
    }
    const next = applyPatch(this.snapshot.settings, patch);
    const chartWindow = next.general.chart_window;
    if (chartWindow !== undefined && !validChartRange(chartWindow)) {
      return err({
        kind: "invalid_settings",
        message: `chart window ${chartWindow}`,
      });
    }
    if (
      !(INTERVALS_MS as readonly number[]).includes(next.sampling.interval_ms)
    ) {
      return err({
        kind: "invalid_settings",
        message: `interval ${next.sampling.interval_ms} ms`,
      });
    }
    if (
      !(SIZE_LIMITS_MB as readonly number[]).includes(
        next.history.size_limit_mb
      )
    ) {
      return err({
        kind: "invalid_settings",
        message: `size limit ${next.history.size_limit_mb} MB`,
      });
    }
    if (JSON.stringify(next) === JSON.stringify(this.snapshot.settings)) {
      return ok(this.snapshot);
    }
    const previous = this.snapshot.settings;
    this.snapshot = { revision: this.snapshot.revision + 1, settings: next };
    this.onChanged(previous, next);
    this.emitters.settings.emit(this.snapshot);
    return ok(this.snapshot);
  };

  setDocker(next: NonNullable<Settings["docker"]>) {
    this.snapshot = {
      revision: this.snapshot.revision + 1,
      settings: { ...this.snapshot.settings, docker: next },
    };
    this.emitters.settings.emit(this.snapshot);
  }
  get methods() {
    return {
      getSettings: this.getSettings,
      updateSettings: this.updateSettings,
    };
  }
}

export function applyPatch(s: Settings, patch: SettingsPatch): Settings {
  const pick = <T extends object>(base: T, p: object | null | undefined) => {
    if (!p) return base;
    const out = { ...base };
    for (const [k, v] of Object.entries(p)) {
      if (v !== null && v !== undefined)
        (out as Record<string, unknown>)[k] = v;
    }
    return out;
  };
  const modules = { ...s.modules };
  for (const [id, mp] of Object.entries(patch.modules ?? {})) {
    const cur = modules[id as keyof typeof modules];
    if (cur && mp) modules[id as keyof typeof modules] = pick(cur, mp);
  }
  const mb = patch.menu_bar;
  const base = menuBarOf(s);
  return {
    modules,
    menu_bar: {
      bars: pick(base.bars, mb?.bars),
      readouts: pick(base.readouts, mb?.readouts),
      items: pick(base.items, mb?.items),
    },
    sampling: pick(s.sampling, patch.sampling),
    history: pick(s.history, patch.history),
    units: pick(s.units, patch.units),
    general: pick(s.general, patch.general),
    remote_access: pick(
      s.remote_access ?? { enabled: false },
      patch.remote_access
    ),
    onboarding: pick(s.onboarding, patch.onboarding),
    alerts: pick(s.alerts ?? NO_ALERTS, patch.alerts),
  };
}
const NO_ALERTS = { hot_process: false, thermal_serious: false };
