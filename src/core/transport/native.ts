import { commands, events, type LiveMsg } from "@core/generated/bindings";
import { Channel } from "@tauri-apps/api/core";
import type { Transport, Unsubscribe } from "./contracts";
import { nativeDiskSpace } from "./native-disk-space";
import { nativeDocker } from "./native-docker";
import { nativeHistory } from "./native-history";
import { nativeRemote } from "./native-remote";

/** Turn Tauri's async unlisten into a synchronous one for effect cleanups. */
function syncUnlisten(pending: Promise<() => void>): Unsubscribe {
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  pending.then(
    (fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    },
    (err: unknown) => console.error("[transport] listen failed", err)
  );
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/**
 * The app transport: generated commands and events, one Channel per live
 * subscription. Rust keys the channel by window label and host and owns when
 * it stops (hidden, occluded, display asleep); a resubscribe from the same
 * window replaces it. There is no unsubscribe command: dropping the callback
 * here only stops delivery, and closing the window drops the channel.
 */
export function createTauriTransport(label: string): Transport {
  return {
    kind: "tauri",
    windowLabel: () => label,
    ...nativeDocker(),
    ...nativeDiskSpace(),
    listHosts: () => commands.listHosts(),
    ...nativeRemote(),
    getHost: (host) => commands.getHost(host),
    getCapabilities: (host) => commands.getCapabilities(host),
    async subscribeLive(host, onMsg, options = {}) {
      let active = true;
      const channel = new Channel<LiveMsg>();
      channel.onmessage = (msg) => {
        if (active) onMsg(msg);
      };
      const info = await commands.subscribeLive(
        host,
        channel,
        options.backfillMs ?? null,
        options.series ? [...options.series] : null,
        options.minPeriodMs ?? null
      );
      return {
        info,
        unsubscribe: () => {
          active = false;
        },
      };
    },
    setProcessInterest: (host, interested, view, stream) =>
      commands.setProcessInterest(host, interested, view, stream),
    ...nativeHistory(),
    getSettings: () => commands.getSettings(),
    updateSettings: (patch) => commands.updateSettings(patch),
    historySize: (host) => commands.historySize(host),
    historyGrowth: (host) => commands.historyGrowth(host),
    clearHistory: (host) => commands.clearHistory(host),
    historyHealth: (host) => commands.historyHealth(host),
    resetHistory: () => commands.resetHistory(),
    setPaused: (paused) => commands.setPaused(paused),
    reportPopoverContentHeight: (height) =>
      commands.reportPopoverContentHeight(height),
    openDashboard: (route) => commands.openDashboard(route),
    sensorDump: (host) => commands.sensorDump(host),
    processSignal: (host, pid, startTimeUs, kind) =>
      commands.processSignal(host, pid, startTimeUs, kind),
    getEdition: () => commands.getEdition(),
    getUpdateStatus: () => commands.getUpdateStatus(),
    checkForUpdates: () => commands.checkForUpdates(),
    installUpdate: (version) => commands.installUpdate(version),
    onUpdateStatusChanged: (cb) =>
      syncUnlisten(
        events.updateStatusChanged.listen((e) => cb(e.payload.status))
      ),
    getWindowAppearance: () => commands.getWindowAppearance(),
    async openUrl(url) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    },
    async closeWindow() {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    },
    onNavigateRequested: (cb) =>
      syncUnlisten(events.navigateRequested.listen((e) => cb(e.payload))),
    onSettingsChanged: (cb) =>
      syncUnlisten(events.settingsChanged.listen((e) => cb(e.payload))),
    onCapabilitiesChanged: (cb) =>
      syncUnlisten(events.capabilitiesChanged.listen((e) => cb(e.payload))),
    onHostsChanged: (cb) =>
      syncUnlisten(events.hostsChanged.listen((e) => cb(e.payload))),
    onHistoryHealthChanged: (cb) =>
      syncUnlisten(events.historyHealthChanged.listen((e) => cb(e.payload))),
    onEventRecorded: (cb) =>
      syncUnlisten(events.eventRecorded.listen((e) => cb(e.payload))),
    onWindowAppearanceChanged: (cb) =>
      syncUnlisten(events.windowAppearanceChanged.listen((e) => cb(e.payload))),
  };
}
