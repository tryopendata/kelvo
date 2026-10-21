import type {
  SpaceCrumb,
  SpaceFile,
  SpaceNode,
  SpaceReport,
  SpaceScanStatus,
} from "@core/generated/bindings";
import { ChevronRight } from "lucide-react";
import { type ReactNode, useCallback, useState } from "react";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { cn } from "~/lib/utils";
import {
  useSpaceItemActions,
  useSpaceLargest,
  useSpaceSubtree,
} from "../_hooks/use-disk-space";
import { canOpen } from "../_lib/space";
import { SpaceMap } from "./space-map";
import { SpaceProgress, SpaceSummary } from "./space-summary";
import {
  LargestFilesTable,
  type SpaceItemActions,
  SpaceTable,
} from "./space-table";

type Tab = "contents" | "largest";

/**
 * One volume's report, running or finished: figures, the drillable treemap
 * and the linked table. Remount it (`key`) for a new report so the open
 * folder and finish order start over.
 */
export function SpaceReportView({
  reportId,
  status,
  report,
  busy,
  onRescan,
  onCancel,
  onVolumes,
}: {
  reportId: number;
  /** The running scan's status; `null` once finished. */
  status: SpaceScanStatus | null;
  report: SpaceReport | null;
  busy: boolean;
  onRescan: () => void;
  onCancel: () => void;
  onVolumes: () => void;
}) {
  const scanning = status !== null;
  const [folder, setFolder] = useState<number | null>(null);
  const [tab, setTab] = useState<Tab>("contents");
  const [hovered, setHovered] = useState<string | null>(null);
  const subtree = useSpaceSubtree(reportId, folder, scanning);
  const largest = useSpaceLargest(reportId, tab === "largest" && !scanning);
  // Reveal in Finder is this Mac's; a remote host's paths can only be copied.
  const local = useIsLocalHost();
  const actions = useSpaceItemActions(reportId, local);
  const data = subtree.data;
  const open = useCallback((node: SpaceNode) => {
    if (!canOpen(node)) return;
    setHovered(null);
    setFolder(node.id);
    setTab("contents");
  }, []);
  const path = data?.path ?? [];
  const up = () => {
    const parent = path.at(-2);
    if (parent) setFolder(parent.id === path[0]?.id ? null : parent.id);
  };
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <SectionCard accent="disk" title="Space" hiddenTitle className="gap-4">
        {status ? (
          <SpaceProgress status={status} onCancel={onCancel} />
        ) : (
          report && (
            <SpaceSummary report={report} onRescan={onRescan} busy={busy} />
          )
        )}
        <Breadcrumb
          path={path}
          onVolumes={onVolumes}
          onCrumb={(c, i) => setFolder(i === 0 ? null : c.id)}
        />
        {subtree.isError ? (
          <p role="status" className="text-[12px] text-muted-foreground">
            Couldn't load this folder. Scan again to refresh the report.
          </p>
        ) : data ? (
          <SpaceMap
            subtree={data}
            hovered={hovered}
            onHover={setHovered}
            onOpen={open}
          />
        ) : (
          <div className="h-[220px]" aria-hidden />
        )}
        {actions.error ? (
          <p role="alert" className="text-[12px] text-destructive">
            {actions.error}
          </p>
        ) : (
          actions.copied && (
            <p role="status" className="truncate text-[12px] text-fg-subtle">
              Copied {actions.copied}
            </p>
          )
        )}
      </SectionCard>
      <TableCard
        tab={tab}
        onTab={setTab}
        scanning={scanning}
        folderName={data?.folder.name ?? "this folder"}
      >
        {tab === "largest" ? (
          <Largest
            files={largest.data}
            failed={largest.isError}
            actions={actions}
          />
        ) : (
          data && (
            <SpaceTable
              rows={data.children}
              total={data.total_bytes ?? 0}
              hovered={hovered}
              onHover={setHovered}
              onOpen={open}
              onUp={up}
              actions={actions}
            />
          )
        )}
      </TableCard>
    </div>
  );
}

function TableCard({
  tab,
  onTab,
  scanning,
  folderName,
  children,
}: {
  tab: Tab;
  onTab: (tab: Tab) => void;
  scanning: boolean;
  folderName: string;
  children: ReactNode;
}) {
  return (
    <SectionCard
      accent="disk"
      origin="tr"
      variant="default"
      title={
        tab === "largest" ? "Largest files on this volume" : `In ${folderName}`
      }
      headerAlign="center"
      aside={
        <SegmentedControl
          options={[
            { value: "contents", label: "Contents" },
            { value: "largest", label: "Largest files", disabled: scanning },
          ]}
          value={tab}
          onChange={onTab}
          ariaLabel="Table"
        />
      }
      className="overflow-hidden"
    >
      {children}
    </SectionCard>
  );
}

function Largest({
  files,
  failed,
  actions,
}: {
  files: readonly SpaceFile[] | undefined;
  failed: boolean;
  actions: SpaceItemActions;
}) {
  if (files) return <LargestFilesTable files={files} actions={actions} />;
  return (
    <p className="text-[12px] text-muted-foreground">
      {failed ? "Couldn't load the largest files." : "Loading largest files…"}
    </p>
  );
}

function Breadcrumb({
  path,
  onVolumes,
  onCrumb,
}: {
  path: readonly SpaceCrumb[];
  onVolumes: () => void;
  onCrumb: (crumb: SpaceCrumb, index: number) => void;
}) {
  const crumb =
    "rounded-sm px-1 py-0.5 outline-none hover:bg-selected hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring";
  return (
    <nav aria-label="Folder path" className="-mx-1 min-w-0 text-[12px]">
      <ol className="flex min-w-0 flex-wrap items-center gap-y-1 text-fg-subtle">
        <li className="flex items-center">
          <button type="button" className={crumb} onClick={onVolumes}>
            Volumes
          </button>
        </li>
        {path.map((c, i) => {
          const last = i === path.length - 1;
          return (
            <li key={c.id} className="flex min-w-0 items-center">
              <ChevronRight
                aria-hidden
                className="size-3 shrink-0 text-fg-faint"
              />
              {last ? (
                <span
                  aria-current="page"
                  className={cn(
                    "truncate px-1 py-0.5 font-[510] text-foreground"
                  )}
                >
                  {c.name}
                </span>
              ) : (
                <button
                  type="button"
                  className={cn(crumb, "truncate")}
                  onClick={() => onCrumb(c, i)}
                >
                  {c.name}
                </button>
              )}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
