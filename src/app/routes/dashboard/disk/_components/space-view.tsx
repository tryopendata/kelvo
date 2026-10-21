import { formatBytes } from "@core/format";
import type { SpaceVolume } from "@core/generated/bindings";
import { remoteQueryText } from "@core/remote-errors";
import type { ComponentProps, ReactNode } from "react";
import { useState } from "react";
import { PageHeader } from "~/components/page-header";
import { useHostNoun } from "~/hooks/use-host-record";
import { useHostId } from "~/stores/host-store";
import {
  useSpaceDropped,
  useSpaceReport,
  useSpaceScan,
  useSpaceStatus,
  useSpaceVolumes,
} from "../_hooks/use-disk-space";
import { spaceScreen } from "../_lib/space";
import { SpaceReportView } from "./space-report";
import { SpaceVolumes } from "./space-volumes";

/**
 * Disk › Space (plan/disk-space-report.md): pick a volume, scan it, and read
 * what uses it. `vol` is the volume in the URL; without one, the volume list.
 * A remote host scans its own volumes over its session (D-117); each host
 * keeps its own scan, so switching hosts starts this view over.
 */
export function SpaceView(props: {
  vol: string | null;
  onVol: (mount: string | null) => void;
  switcher: ReactNode;
}) {
  return <HostSpace key={useHostId()} {...props} />;
}

function SpaceShell({
  subtitle,
  switcher,
  children,
}: {
  subtitle: string;
  switcher: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <PageHeader title="Disk" subtitle={subtitle} actions={switcher} />
      {children}
    </div>
  );
}

function HostSpace({
  vol,
  onVol,
  switcher,
}: {
  vol: string | null;
  onVol: (mount: string | null) => void;
  switcher: ReactNode;
}) {
  const volumes = useSpaceVolumes(true);
  const status = useSpaceStatus(true);
  const scanning = status.data?.phase === "scanning";
  const report = useSpaceReport(
    status.isSuccess,
    scanning ? null : (status.data?.report_id ?? null)
  );
  const scan = useSpaceScan();
  const dropped = useSpaceDropped(status.data);
  const [include, setInclude] = useState<ReadonlySet<string>>(new Set());
  const kept = report.data ?? null;
  const { screen, ended } = spaceScreen(status.data, kept, vol);
  const busy = scan.starting || scanning;
  const start = (mount: string) => {
    onVol(mount);
    void scan.start(mount, [...include]);
  };
  const volume = volumes.data?.find((v) => v.mount === vol);
  return (
    <SpaceShell subtitle={volumeSubtitle(volume)} switcher={switcher}>
      <SpaceMessage
        error={scan.error}
        // A host that stops answering mid-scan keeps its last status on
        // screen; the volume list says why on its own.
        lost={screen.mode === "volumes" ? null : status.error}
        ended={ended ?? dropped}
        kept={screen.mode === "report"}
      />
      {screen.mode === "volumes" ? (
        <VolumeList
          query={volumes}
          include={include}
          onInclude={(key, on) => setInclude((prev) => toggled(prev, key, on))}
          onScan={start}
          onOpen={onVol}
          reportVolume={kept?.volume.mount ?? null}
          scanningVolume={scanning ? (status.data?.volume ?? null) : null}
          busy={busy}
        />
      ) : (
        <SpaceReportView
          key={screen.reportId}
          reportId={screen.reportId}
          status={screen.mode === "scan" ? screen.status : null}
          report={screen.mode === "report" ? screen.report : null}
          busy={busy}
          onRescan={() => vol && start(vol)}
          onCancel={() => void scan.cancel()}
          onVolumes={() => onVol(null)}
        />
      )}
    </SpaceShell>
  );
}

function toggled(prev: ReadonlySet<string>, key: string, on: boolean) {
  const next = new Set(prev);
  if (on) next.add(key);
  else next.delete(key);
  return next;
}

function volumeSubtitle(volume: SpaceVolume | undefined): string {
  if (!volume) return "Choose a volume to scan";
  return `${volume.name} · ${formatBytes(volume.used_bytes)} of ${formatBytes(volume.total_bytes)} used`;
}

function SpaceMessage({
  error,
  lost,
  ended,
  kept,
}: {
  error: string | null;
  /** Why the scan's status stopped coming, if it did. */
  lost: Error | null;
  ended: string | null;
  kept: boolean;
}) {
  const alert = error ?? (lost && remoteQueryText(lost, "space reports"));
  if (alert)
    return (
      <p role="alert" className="text-[12px] text-destructive">
        {alert}
      </p>
    );
  if (!ended) return null;
  return (
    <p role="status" className="text-[12px] text-muted-foreground">
      {ended}
      {kept ? " The previous report is shown." : ""}
    </p>
  );
}

function VolumeList({
  query,
  ...props
}: {
  query: ReturnType<typeof useSpaceVolumes>;
} & Omit<ComponentProps<typeof SpaceVolumes>, "volumes">) {
  const noun = useHostNoun();
  if (query.data) return <SpaceVolumes volumes={query.data} {...props} />;
  return (
    <p role="status" className="text-[12px] text-muted-foreground">
      {!query.isError
        ? "Loading volumes…"
        : query.error.message.includes("edition")
          ? "Space reports are not available in this edition."
          : (remoteQueryText(query.error, "space reports") ??
            `Couldn't list the volumes on this ${noun}.`)}
    </p>
  );
}
