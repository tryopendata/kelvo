import { fixed, formatInteger, formatPercent } from "@core/format";
import type {
  SpaceNode,
  SpaceReport,
  SpaceScanStatus,
} from "@core/generated/bindings";
import type { SpaceTileKind } from "@core/space-treemap";

/** "12.4%", "<0.1%" for a sliver that would otherwise read as nothing. */
export function shareLabel(share: number): string {
  if (share > 0 && share < 0.001) return "<0.1%";
  return formatPercent(share * 100, { decimals: 1 });
}

/** "312 items", "48.2 K items", "1.8 M items". */
export function itemsLabel(n: number | null): string {
  const v = n ?? 0;
  const noun = v === 1 ? "item" : "items";
  if (v >= 1e6) return `${fixed(v / 1e6, 1)} M ${noun}`;
  if (v >= 1e5) return `${fixed(v / 1e3, 0)} K ${noun}`;
  return `${formatInteger(v)} ${noun}`;
}

/** "just now", "4 min ago", "2 h ago". */
export function agoLabel(nowMs: number, thenMs: number | null): string {
  if (thenMs === null) return "";
  const min = Math.floor((nowMs - thenMs) / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  return `${Math.floor(min / 60)} h ago`;
}

/** Elapsed scan time on a clock: "0:14", "2:03". */
export function elapsedLabel(ms: number | null): string {
  const s = Math.floor((ms ?? 0) / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Keeps both ends of a long path: "Users/riley/Lib…/DerivedData". */
export function middleTruncate(text: string, max: number): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.ceil(keep / 2);
  return `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`;
}

/** What a tile or row is, for its tooltip and the table's kind column. */
export function kindLabel(kind: SpaceTileKind, node: SpaceNode | null): string {
  switch (kind) {
    case "folder":
      return node?.done === false ? "Folder · still scanning" : "Folder";
    case "file":
      return "File";
    case "smaller":
      return "Merged: each under 0.5% of this folder";
    case "volume":
      return "Another volume in this container";
    case "other":
      return "Not measured as files";
    case "unmeasured":
      return "Not yet measured";
  }
}

/** Whether a node opens onto its own breakdown now. */
export function canOpen(node: SpaceNode | null): node is SpaceNode {
  return (
    node !== null &&
    node.kind === "folder" &&
    node.openable &&
    node.done &&
    node.id !== null
  );
}

/** What the Space view shows for the volume in the URL. */
export type SpaceScreen =
  | { mode: "scan"; reportId: number; status: SpaceScanStatus }
  /** `report` is null for the moment between a scan finishing and its report loading. */
  | { mode: "report"; reportId: number; report: SpaceReport | null }
  | { mode: "volumes" };

/**
 * The running scan of `vol`, else its kept report, else the volume list.
 * `ended` explains a scan of `vol` that stopped without a report.
 */
export function spaceScreen(
  status: SpaceScanStatus | undefined,
  report: SpaceReport | null,
  vol: string | null
): { screen: SpaceScreen; ended: string | null } {
  const mine = status !== undefined && status.volume === vol;
  const ended =
    mine && (status.phase === "cancelled" || status.phase === "failed")
      ? (status.error ?? "The last scan was cancelled before it finished.")
      : null;
  if (mine && status.phase === "scanning" && status.report_id !== null)
    return {
      screen: { mode: "scan", reportId: status.report_id, status },
      ended,
    };
  if (
    mine &&
    status.phase === "done" &&
    status.report_id !== null &&
    report?.id !== status.report_id
  )
    return {
      screen: { mode: "report", reportId: status.report_id, report: null },
      ended,
    };
  if (report !== null && report.volume.mount === vol)
    return { screen: { mode: "report", reportId: report.id, report }, ended };
  return { screen: { mode: "volumes" }, ended };
}
