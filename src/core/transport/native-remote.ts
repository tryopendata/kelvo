import { commands } from "@core/generated/bindings";
import type { Transport } from "./contracts";

export function nativeRemote(): Pick<
  Transport,
  | "listRemoteHosts"
  | "inspectRemoteSshHost"
  | "remoteSshConfigStatus"
  | "acceptRemoteSshHost"
  | "prepareRemoteHost"
  | "addRemoteHost"
  | "cancelRemoteSetup"
  | "updateRemoteHost"
  | "removeRemoteHost"
  | "retryRemoteHost"
> {
  return {
    listRemoteHosts: () => commands.listRemoteHosts(),
    inspectRemoteSshHost: (alias) => commands.inspectRemoteSshHost(alias),
    remoteSshConfigStatus: () => commands.remoteSshConfigStatus(),
    acceptRemoteSshHost: (token) => commands.acceptRemoteSshHost(token),
    prepareRemoteHost: (lan, tailscale) =>
      commands.prepareRemoteHost(lan, tailscale),
    addRemoteHost: (config, install, showMenuBar, setupId) =>
      commands.addRemoteHost(config, install, showMenuBar, setupId),
    cancelRemoteSetup: (setupId) => commands.cancelRemoteSetup(setupId),
    updateRemoteHost: (config) => commands.updateRemoteHost(config),
    removeRemoteHost: (id, deleteMirror, stopSharing) =>
      commands.removeRemoteHost(id, deleteMirror, stopSharing),
    retryRemoteHost: (id) => commands.retryRemoteHost(id),
  };
}
