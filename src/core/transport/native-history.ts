import { commands } from "@core/generated/bindings";
import type { Transport } from "./contracts";

export function nativeHistory(): Pick<
  Transport,
  | "queryDiskIo"
  | "listDiskIoDevices"
  | "queryHistory"
  | "queryProcessesAt"
  | "queryNetworkByApp"
  | "queryUsageByApp"
  | "queryUsageSeries"
  | "querySeriesStats"
  | "getNetworkAddresses"
  | "getPublicIp"
  | "queryEvents"
  | "queryHeatmap"
  | "batteryHours"
  | "exportCsv"
> {
  return {
    queryDiskIo: (request) => commands.queryDiskIo(request),
    listDiskIoDevices: (host) => commands.listDiskIoDevices(host),
    queryHistory: (request) => commands.queryHistory(request),
    queryProcessesAt: (host, tMs) => commands.queryProcessesAt(host, tMs),
    queryNetworkByApp: (host, fromMs, toMs) =>
      commands.queryNetworkByApp(host, fromMs, toMs),
    queryUsageByApp: (host, fromMs, toMs, by, limit) =>
      commands.queryUsageByApp(host, fromMs, toMs, by, limit),
    queryUsageSeries: (host, fromMs, toMs, by, limit) =>
      commands.queryUsageSeries(host, fromMs, toMs, by, limit),
    querySeriesStats: (host, metrics, fromMs, toMs) =>
      commands.querySeriesStats(host, metrics, fromMs, toMs),
    getNetworkAddresses: (host) => commands.getNetworkAddresses(host),
    getPublicIp: () => commands.getPublicIp(),
    queryEvents: (host, fromMs, toMs) =>
      commands.queryEvents(host, fromMs, toMs),
    queryHeatmap: (request) => commands.queryHeatmap(request),
    batteryHours: (host, hourStartsMs) =>
      commands.batteryHours(host, hourStartsMs),
    exportCsv: (request) => commands.exportCsv(request),
  };
}
