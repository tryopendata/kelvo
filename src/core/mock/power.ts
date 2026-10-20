import type { LiveStatus, PerformanceReason } from "@core/generated/bindings";
import { samplingPlan } from "../sampling-plans";
import type { MockTransportOptions } from "./contracts";
import type { MockEmitters } from "./emitters";
import type { ScenarioFlags } from "./fixtures";
import type { MockSettings } from "./settings";
export class MockPower {
  status: LiveStatus;
  lowPowerMode: boolean;
  windowVisible = true;
  onChanged: (retimed: boolean) => void = () => {};
  constructor(
    private readonly settings: MockSettings,
    private readonly emitters: MockEmitters,
    flags: ScenarioFlags,
    options: MockTransportOptions
  ) {
    const startInterval = options.intervalMs ?? 1000;
    this.status = {
      interval_ms: startInterval,
      frame_period_ms: startInterval,
      paused: flags.paused,
      display_idle: false,
      on_battery: !flags.noBattery && !flags.linux,
      // As the engine resolves it: on battery wins; a Mac without one is on the
      // adapter (D-092). The mock never charges.
      power_source: flags.noBattery || flags.linux ? "adapter" : "battery",
      // The default route is on en0, the reported Wi-Fi interface; on a
      // full-tunnel VPN it is on the tunnel, which is not reported.
      primary_iface: flags.vpn ? null : flags.linux ? "end0" : "en0",
      performance: "off",
    };
    this.lowPowerMode = flags.lowPowerMode;
    this.status = { ...this.status, performance: this.performanceReason() };
    const startTick = this.effectiveInterval();
    if (startTick !== startInterval)
      this.status = {
        ...this.status,
        interval_ms: startTick,
        frame_period_ms: startTick,
      };
  }
  readonly performanceReason = (): PerformanceReason =>
    this.settings.snapshot.settings.sampling.performance_mode
      ? "setting"
      : this.lowPowerMode
        ? "low_power_mode"
        : "off";
  readonly effectiveInterval = () => {
    const s = this.settings.snapshot.settings.sampling;
    const plan = samplingPlan({
      interval_ms: s.interval_ms,
      slow_on_battery: s.slow_on_battery,
      performance: !!s.performance_mode || this.lowPowerMode,
      low_power_mode: this.lowPowerMode,
    });
    if (!plan) return s.interval_ms;
    const f = this.status.on_battery ? plan.battery : plan.ac;
    return this.windowVisible ? f.tick_ms : f.background_tick_ms;
  };
  readonly receiving = () => this.windowVisible && !this.status.display_idle;
  readonly updatePower = () => {
    const performance = this.performanceReason();
    const interval_ms = this.effectiveInterval();
    if (
      performance === this.status.performance &&
      interval_ms === this.status.interval_ms
    ) {
      return;
    }
    const retimed = interval_ms !== this.status.interval_ms;
    const changed = performance !== this.status.performance;
    this.status = { ...this.status, performance, interval_ms };
    this.onChanged(retimed);
    if (!changed) return;
    const appearance = {
      performance,
      reduce_transparency: false,
      theme: this.settings.snapshot.settings.general.appearance,
    };
    this.emitters.appearance.emit({ appearance });
  };
}
