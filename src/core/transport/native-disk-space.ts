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
    diskSpaceVolumes: () => commands.diskSpaceVolumes(),
    diskSpaceStart: (volume, include) =>
      commands.diskSpaceStart(volume, include),
    diskSpaceStatus: () => commands.diskSpaceStatus(),
    diskSpaceCancel: () => commands.diskSpaceCancel(),
    diskSpaceReport: () => commands.diskSpaceReport(),
    diskSpaceSubtree: (report, node) => commands.diskSpaceSubtree(report, node),
    diskSpaceLargestFiles: (report) => commands.diskSpaceLargestFiles(report),
    diskSpaceReveal: (report, target) =>
      commands.diskSpaceReveal(report, target),
    diskSpacePath: (report, target) => commands.diskSpacePath(report, target),
  };
}
