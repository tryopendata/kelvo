import { commands } from "@core/generated/bindings";
import type { Transport } from "./contracts";

export function nativeDiskSpace(): Pick<
  Transport,
  | "diskSpaceVolumes"
  | "diskSpaceStart"
  | "diskSpaceStatus"
  | "diskSpaceCancel"
  | "diskSpaceReport"
  | "diskSpaceSubtree"
  | "diskSpaceLargestFiles"
  | "diskSpaceReveal"
  | "diskSpacePath"
> {
  return {
    diskSpaceVolumes: (host) => commands.diskSpaceVolumes(host),
    diskSpaceStart: (host, volume, include) =>
      commands.diskSpaceStart(host, volume, include),
    diskSpaceStatus: (host) => commands.diskSpaceStatus(host),
    diskSpaceCancel: (host) => commands.diskSpaceCancel(host),
    diskSpaceReport: (host) => commands.diskSpaceReport(host),
    diskSpaceSubtree: (host, report, node) =>
      commands.diskSpaceSubtree(host, report, node),
    diskSpaceLargestFiles: (host, report) =>
      commands.diskSpaceLargestFiles(host, report),
    diskSpaceReveal: (report, target) =>
      commands.diskSpaceReveal(report, target),
    diskSpacePath: (host, report, target) =>
      commands.diskSpacePath(host, report, target),
  };
}
