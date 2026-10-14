import {
  commands,
  type DockerLogBatch,
  type DockerSnapshot,
} from "@core/generated/bindings";
import { Channel } from "@tauri-apps/api/core";
import type { Transport } from "./contracts";

export function nativeDocker(): Pick<
  Transport,
  | "dockerComparison"
  | "dockerContainerAction"
  | "dockerStorageScan"
  | "dockerStoragePrepare"
  | "dockerStorageExecute"
  | "dockerStorageOperation"
  | "dockerRetained"
  | "dockerDiscover"
  | "dockerConfigure"
  | "dockerSnapshot"
  | "dockerSubscribe"
  | "dockerDetail"
  | "dockerHistory"
  | "dockerEvents"
  | "dockerLogs"
  | "dockerResume"
> {
  return {
    dockerComparison: (request) => commands.dockerComparison(request),
    dockerContainerAction: (engineId, containerId, action) =>
      commands.dockerContainerAction(engineId, containerId, action),
    dockerStorageScan: (force) => commands.dockerStorageScan(force),
    dockerStoragePrepare: (selections) =>
      commands.dockerStoragePrepare(selections),
    dockerStorageExecute: (token, confirmVolumes) =>
      commands.dockerStorageExecute(token, confirmVolumes),
    dockerStorageOperation: (operationId) =>
      commands.dockerStorageOperation(operationId),
    dockerRetained: (request) => commands.dockerRetained(request),
    dockerDiscover: () => commands.dockerDiscover(),
    dockerConfigure: (settings) => commands.dockerConfigure(settings),
    dockerSnapshot: () => commands.dockerSnapshot(),
    async dockerSubscribe(onSnapshot) {
      let active = true;
      const channel = new Channel<DockerSnapshot>();
      channel.onmessage = (snapshot) => {
        if (active) onSnapshot(snapshot);
      };
      const info = await commands.dockerSubscribe(channel, true);
      return {
        info,
        unsubscribe: () => {
          active = false;
          if (info.status === "ok") void commands.dockerUnsubscribe(info.data);
        },
      };
    },
    dockerDetail: (engineId, containerId) =>
      commands.dockerDetail(engineId, containerId),
    dockerHistory: (request) => commands.dockerHistory(request),
    dockerEvents: (request) => commands.dockerEvents(request),
    async dockerLogs(request, onBatch) {
      let active = true;
      const channel = new Channel<DockerLogBatch>();
      channel.onmessage = (batch) => {
        if (active) onBatch(batch);
      };
      const info = await commands.dockerLogs(request, channel);
      return {
        info,
        unsubscribe: () => {
          active = false;
          if (info.status === "ok") void commands.dockerCloseLogs(info.data);
        },
      };
    },
    dockerResume: () => commands.dockerResume(),
  };
}
