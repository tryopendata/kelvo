import type { UpdateStatus } from "@core/generated/bindings";
import type { MockCommands } from "./commands";
import type { MockTransport, MockTransportOptions } from "./contracts";
import type { MockEmitters } from "./emitters";
import type { MockPower } from "./power";
import { ok } from "./protocol";
import type { MockRing } from "./ring";
import type { MockSettings } from "./settings";
export class MockShell {
  private updateStatus: UpdateStatus;
  constructor(
    private readonly ring: MockRing,
    private readonly power: MockPower,
    private readonly settings: MockSettings,
    private readonly commands: MockCommands,
    private readonly emitters: MockEmitters,
    private readonly options: MockTransportOptions
  ) {
    this.updateStatus = options.updateStatus ?? { kind: "idle" };
  }
  readonly requestNavigate: MockTransport["requestNavigate"] = (route) => {
    this.emitters.navigate.emit({ route });
  };
  readonly openDashboard: MockTransport["openDashboard"] = async (route) => {
    this.commands.record("open_dashboard", route);
    return ok(null);
  };
  readonly getEdition: MockTransport["getEdition"] = async () => {
    this.commands.record("get_edition");
    return {
      process_signal: !this.ring.flags.appstore,
      disk_space: !this.ring.flags.appstore,
    };
  };
  readonly getUpdateStatus: MockTransport["getUpdateStatus"] = async () =>
    this.updateStatus;
  readonly onUpdateStatusChanged: MockTransport["onUpdateStatusChanged"] = (
    cb
  ) => this.emitters.updates.on(cb);
  readonly checkForUpdates: MockTransport["checkForUpdates"] = async () => {
    this.commands.record("check_for_updates");
    this.updateStatus = this.options.updateStatus ?? { kind: "up_to_date" };
    this.emitters.updates.emit(this.updateStatus);
    return ok(this.updateStatus);
  };
  readonly installUpdate: MockTransport["installUpdate"] = async (version) => {
    this.commands.record("install_update", version);
    if (this.options.updateFails) {
      this.updateStatus = {
        kind: "failed",
        message: "The update signature could not be verified.",
      };
      this.emitters.updates.emit(this.updateStatus);
      return {
        status: "error",
        error: { kind: "update", message: this.updateStatus.message },
      };
    }
    this.updateStatus = { kind: "ready", version };
    this.emitters.updates.emit(this.updateStatus);
    return ok(null);
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
      getUpdateStatus: this.getUpdateStatus,
      installUpdate: this.installUpdate,
      onUpdateStatusChanged: this.onUpdateStatusChanged,
      getWindowAppearance: this.getWindowAppearance,
      openUrl: this.openUrl,
      closeWindow: this.closeWindow,
    };
  }
}
