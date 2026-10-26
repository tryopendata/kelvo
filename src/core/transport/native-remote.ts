import { commands } from "@core/generated/bindings";
import type { Transport } from "./contracts";

export function nativeRemote(): Pick<
  Transport,
  | "listRemoteHosts"
  | "getRemoteUpdateStatus"
  | "setRemoteUpdatePolicy"
  | "inspectRemoteSshHost"
  | "remoteSshConfigStatus"
  | "acceptRemoteSshHost"
  | "prepareRemoteHost"
  | "addRemoteHost"
  | "cancelRemoteSetup"
  | "updateRemoteHost"
  | "removeRemoteHost"
  | "retryRemoteHost"
  | "resetRemoteHostIdentity"
  | "updateRemoteAgent"
  | "repairRemoteHost"
> {
  return {
    listRemoteHosts: () => commands.listRemoteHosts(),
    getRemoteUpdateStatus: (id) => commands.getRemoteUpdateStatus(id),
    setRemoteUpdatePolicy: (id, checkUpdates, autoInstallUpdates) =>
      commands.setRemoteUpdatePolicy(id, checkUpdates, autoInstallUpdates),
    inspectRemoteSshHost: (alias) => commands.inspectRemoteSshHost(alias),
    remoteSshConfigStatus: () => commands.remoteSshConfigStatus(),
    acceptRemoteSshHost: (token) => commands.acceptRemoteSshHost(token),
    prepareRemoteHost: (lan, tailscale) =>
      commands.prepareRemoteHost(lan, tailscale),
    addRemoteHost: (config, install, showMenuBar, setupId) =>
      commands.addRemoteHost(config, install, showMenuBar, setupId),
    cancelRemoteSetup: (setupId) => commands.cancelRemoteSetup(setupId),
    updateRemoteHost: (config) => commands.updateRemoteHost(config),
    removeRemoteHost: (
      id,
      deleteMirror,
      stopSharing,
      uninstallAgent,
      purgeAgent
    ) =>
      commands.removeRemoteHost(
        id,
        deleteMirror,
        stopSharing,
        uninstallAgent,
        purgeAgent
      ),
    retryRemoteHost: (id) => commands.retryRemoteHost(id),
    resetRemoteHostIdentity: (lan, tailscale) =>
      commands.resetRemoteHostIdentity(lan, tailscale),
    updateRemoteAgent: (id) => commands.updateRemoteAgent(id),
    repairRemoteHost: (id) => commands.repairRemoteHost(id),
  };
}
