import type { MockCommands } from "./commands";
import type { MockTransport } from "./contracts";
import type { MockEmitters } from "./emitters";
import type { MockPower } from "./power";
import { ok } from "./protocol";
import type { MockRing } from "./ring";
import type { MockSettings } from "./settings";
export class MockShell {
  constructor(
    private readonly ring: MockRing,
    private readonly power: MockPower,
    private readonly settings: MockSettings,
    private readonly commands: MockCommands,
    private readonly emitters: MockEmitters
  ) {}
  readonly requestNavigate: MockTransport["requestNavigate"] = (route) => {
    this.emitters.navigate.emit({ route });
  };
  readonly openDashboard: MockTransport["openDashboard"] = async (route) => {
    this.commands.record("open_dashboard", route);
    return ok(null);
  };
  readonly getEdition: MockTransport["getEdition"] = async () => {
    this.commands.record("get_edition");
    return { process_signal: !this.ring.flags.appstore };
  };
  readonly checkForUpdates: MockTransport["checkForUpdates"] = async () => {
    this.commands.record("check_for_updates");
    return this.settings.snapshot.settings.general.check_updates
      ? { kind: "not_configured" }
      : { kind: "disabled" };
  };
  readonly getWindowAppearance: MockTransport["getWindowAppearance"] =
    async () => {
      this.commands.record("get_window_appearance");
      return {
        performance: this.power.status.performance,
        reduce_transparency: false,
        theme: this.settings.snapshot.settings.general.appearance,
      };
    };
  readonly openUrl: MockTransport["openUrl"] = async (url) => {
    this.commands.record("open_url", url);
  };
  readonly closeWindow: MockTransport["closeWindow"] = async () => {
    this.commands.record("close_window");
  };

  get methods() {
    return {
      requestNavigate: this.requestNavigate,
      openDashboard: this.openDashboard,
      getEdition: this.getEdition,
      checkForUpdates: this.checkForUpdates,
      getWindowAppearance: this.getWindowAppearance,
      openUrl: this.openUrl,
      closeWindow: this.closeWindow,
    };
  }
}
