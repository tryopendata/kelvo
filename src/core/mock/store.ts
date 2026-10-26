import type {
  CommandError,
  Event,
  HistoryGrowth,
  HistoryHealth,
  Settings,
} from "@core/generated/bindings";
import { HISTORY_PROJECTION } from "@core/generated/bindings";
import type { MockTransport, MockTransportOptions } from "./contracts";
import type { MockEmitters } from "./emitters";
import { mockEvents } from "./events";
import { MOCK_HOST_ID } from "./fixtures";
import type { MockRing } from "./ring";
import type { MockSettings } from "./settings";
export class MockStore {
  unavailable: CommandError | null;
  netHistoryOff: { from: number; to: number | null }[] = [];
  historyBytes = 148_000_000;
  growth: HistoryGrowth | null;
  health: HistoryHealth;
  events: Event[];
  constructor(
    readonly options: MockTransportOptions,
    private readonly ring: MockRing,
    settings: MockSettings,
    private readonly emitters: MockEmitters
  ) {
    const flags = ring.flags;
    const startMs = ring.startMs;
    this.unavailable = flags.historyUnavailable
      ? { kind: "history_unavailable", reason: flags.historyReason }
      : null;
    this.growth =
      options.historyGrowth === undefined
        ? {
            measured_ms: 7 * 86_400_000,
            fixed_bytes: 36_000_000,
            minute_day_bytes: 2_050_000,
            usage: { ...HISTORY_PROJECTION.usage_fill },
          }
        : options.historyGrowth;
    this.health = {
      low_disk_paused: flags.lowDisk,
      trimmed_before_ms: flags.historyTrimmed
        ? startMs - 20 * 86_400_000
        : null,
      trimmed_limit_bytes: flags.historyTrimmed ? 150_000_000 : null,
      cap_met: true,
    };
    // A Linux agent (D-116) has no ANE, so it never reports an ANE spike.
    this.events = mockEvents(startMs).filter(
      (e) =>
        !flags.linux ||
        e.detail.kind !== "power_spike" ||
        e.detail.component !== "ane"
    );
    if (settings.snapshot.settings.history.network_history === false)
      this.netHistoryOff.push({ from: Number.NEGATIVE_INFINITY, to: null });
  }
  readonly setHistoryHealth: MockTransport["setHistoryHealth"] = (next) => {
    this.health = next;
    this.emitters.health.emit({ health: this.health });
  };
  readonly recordEvent: MockTransport["recordEvent"] = (event) => {
    this.events.push(event);
    this.events.sort((a, b) => a.ts_ms - b.ts_ms);
    this.emitters.event.emit({ host: MOCK_HOST_ID, event });
  };
  recordNetworkSetting(before: Settings, after: Settings) {
    const wasOn = before.history.network_history !== false;
    const isOn = after.history.network_history !== false;
    if (wasOn === isOn) return;
    const t = this.ring.now() + this.ring.clockOffset;
    const open = this.netHistoryOff[this.netHistoryOff.length - 1];
    if (!isOn) this.netHistoryOff.push({ from: t, to: null });
    else if (open && open.to === null) open.to = t;
  }

  get methods() {
    return {
      setHistoryHealth: this.setHistoryHealth,
      recordEvent: this.recordEvent,
    };
  }
}
