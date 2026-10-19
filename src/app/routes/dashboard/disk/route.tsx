import { formatRate } from "@core/format";
import { labelValues, sk } from "@core/series-key";
import { type ReactNode, useEffect, useMemo } from "react";
import { Link, useSearchParams } from "react-router";
import { brushScopeProps, useDismissOutside } from "~/components/brush-overlay";
import { PageHeader } from "~/components/page-header";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { SelectionSummary } from "~/components/selection-summary";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { VolumeTable } from "~/components/volume-table";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useEdition } from "~/hooks/use-edition";
import { useElementSize } from "~/hooks/use-element-size";
import { useHostRecord } from "~/hooks/use-host-record";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useOpenEdge } from "~/hooks/use-range-scope";
import { useHeld, useLayout } from "~/hooks/use-ring";
import {
  BrushProvider,
  useBrushRange,
  useBrushStore,
} from "~/stores/brush-store";
import { useMemory } from "~/stores/live-selectors";
import { StatStrip } from "~/widgets/stat-strip";
import { DiskIoCharts } from "./_components/disk-io-charts";
import { DiskProcesses } from "./_components/disk-processes";
import { DiskRangeTotals } from "./_components/disk-range-totals";
import { SpaceView } from "./_components/space-view";
import { useDiskDevices, useDiskIo } from "./_hooks/use-disk-io";
import { useSpaceVolumes } from "./_hooks/use-disk-space";
import { volumeRows } from "./_lib/disk";
import { diskRange } from "./_lib/disk-range";

export default function DiskRoute() {
  const [params, setParams] = useSearchParams();
  const edition = useEdition();
  const space = params.get("view") === "space";
  const set = (patch: Record<string, string | null>) =>
    setParams(
      (old) => {
        const next = new URLSearchParams(old);
        for (const [k, v] of Object.entries(patch))
          if (v === null) next.delete(k);
          else next.set(k, v);
        return next;
      },
      { replace: true }
    );
  // Activity keeps its own params while in Space, so switching back restores it.
  const switcher = edition?.disk_space ? (
    <SegmentedControl
      options={[
        { value: "activity", label: "Activity" },
        { value: "space", label: "Space" },
      ]}
      value={space ? "space" : "activity"}
      onChange={(value) => set({ view: value === "space" ? value : null })}
      ariaLabel="Disk view"
    />
  ) : null;
  if (space && edition?.disk_space)
    return (
      <SpaceView
        vol={params.get("vol")}
        onVol={(vol) => set({ vol })}
        switcher={switcher}
      />
    );
  return (
    <BrushProvider initial={diskRange(params)}>
      <DiskActivity
        switcher={switcher}
        onAnalyze={
          edition?.disk_space ? (vol) => set({ view: "space", vol }) : undefined
        }
      />
    </BrushProvider>
  );
}

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
function DiskActivity({
  switcher,
  onAnalyze,
}: {
  switcher: ReactNode;
  /** Opens Space for a volume; absent when this build has no Space view. */
  onAnalyze: ((vol: string) => void) | undefined;
}) {
  const windowMs = useChartWindow()?.windowMs ?? null;
  const [params, setParams] = useSearchParams();
  const hostRecord = useHostRecord();
  const local = useIsLocalHost();
  const inventory = useDiskDevices(local && hostRecord !== undefined);
  const chosen = params.get("device");
  const device =
    !local || chosen === "all"
      ? null
      : (chosen ?? inventory.data?.default_device ?? null);
  const secondary = params.get("response") === "ops" ? "ops" : "time";
  const selection = useBrushRange();
  const brush = useBrushStore();
  // Keep dismissal available while changing range unmounts the loading plots.
  useDismissOutside(brush.getState().clear, selection !== null);
  const edge = useOpenEdge();
  // A pinned interval keeps its surrounding chart window when live time advances.
  const end = selection?.toMs ?? edge;
  const canReadHistory = !local || inventory.data !== undefined;
  const range =
    windowMs !== null && end !== null && canReadHistory
      ? { fromMs: end - windowMs, toMs: end }
      : null;
  const [chartRef, chartWidth] = useElementSize("width", 1000);
  const maxPoints = Math.max(
    12,
    Math.min(120, Math.floor((chartWidth - 128) / 5))
  );
  const chart = useDiskIo(device, range, maxPoints, selection === null);
  const selectedTotals = useDiskIo(
    device,
    canReadHistory ? selection : null,
    1
  );
  const totals = selection
    ? selectedTotals.isError
      ? undefined
      : selectedTotals.data
    : chart.data;
  const layout = useLayout();
  const vols = useMemo(
    () => labelValues(layout?.series ?? [], "disk.total", "vol"),
    [layout]
  );
  const volKeys = useMemo(
    () =>
      ["disk.used", "disk.free", "disk.total"].flatMap(
        (m) => layout?.byMetric.get(m) ?? []
      ),
    [layout]
  );
  const held = useHeld(volKeys);
  const mounts = hostRecord?.info.boot_mounts ?? [];
  const devices = local ? (inventory.data?.devices ?? []) : [];
  const selected = devices.find((d) => d.dev === device);
  const missing =
    device !== null && inventory.data !== undefined && selected === undefined;
  useEffect(() => {
    const next = new URLSearchParams(params);
    if (local && chosen === null && inventory.data)
      next.set("device", inventory.data.default_device ?? "all");
    if (selection) {
      next.set("from", String(selection.fromMs));
      next.set("to", String(selection.toMs));
    } else {
      next.delete("from");
      next.delete("to");
    }
    if (next.toString() !== params.toString())
      setParams(next, { replace: true });
  }, [chosen, inventory.data, local, params, selection, setParams]);
  return (
    <div className="flex min-w-0 flex-col gap-4 [&_[data-slot=toggle-group-item][data-state=on]]:text-foreground">
      <PageHeader
        title="Disk"
        subtitle={
          selected
            ? `${selected.name} · ${selected.dev}${selected.internal === true ? " · Internal" : selected.internal === false ? " · External" : ""}`
            : missing
              ? `${device} · Disconnected or unavailable`
              : "All devices"
        }
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <WindowControl />
            {switcher}
          </div>
        }
      />
      <div {...brushScopeProps} ref={chartRef} className="min-w-0">
        <SectionCard
          accent="disk"
          title="Disk I/O"
          compactHeader
          headerAlign="center"
          className="gap-3"
          aside={
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <Select
                value={device ?? "all"}
                onValueChange={(value) => {
                  const next = new URLSearchParams(params);
                  next.set("device", value);
                  setParams(next, { replace: true });
                }}
              >
                <SelectTrigger
                  aria-label="Disk device"
                  size="sm"
                  className="max-w-56"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All devices</SelectItem>
                  {devices.map((d) => (
                    <SelectItem key={d.dev} value={d.dev}>
                      {d.name} · {d.dev}
                    </SelectItem>
                  ))}
                  {missing && (
                    <SelectItem value={device}>
                      {device} · Unavailable
                    </SelectItem>
                  )}
                </SelectContent>
              </Select>
              <button
                type="button"
                onClick={() => brush.getState().clear()}
                aria-pressed={selection === null}
                className="h-7 rounded-control border border-border bg-btn px-3 text-[12px] outline-none hover:bg-selected focus-visible:ring-2 focus-visible:ring-ring"
              >
                Live
              </button>
            </div>
          }
        >
          <div className="flex flex-wrap items-end justify-between gap-x-6 gap-y-2">
            <CurrentRates device={device} />
            <div className="flex min-w-0 flex-col gap-1">
              <DiskRangeTotals
                page={totals}
                requested={selection ?? range}
                selected={selection !== null}
              />
              {selection && selectedTotals.isError && (
                <p role="status" className="text-[12px] text-muted-foreground">
                  Couldn't load byte totals for the selected interval.
                </p>
              )}
            </div>
          </div>
          {local && inventory.isError ? (
            <p role="status" className="text-[12px] text-muted-foreground">
              Couldn't load disk devices.
            </p>
          ) : chart.isError ? (
            <p role="status" className="text-[12px] text-muted-foreground">
              Couldn't load disk I/O history.
            </p>
          ) : chart.data && range ? (
            <DiskIoCharts
              page={chart.data}
              range={range}
              secondary={secondary}
              onSecondary={(value) => {
                const next = new URLSearchParams(params);
                next.set("response", value);
                setParams(next, { replace: true });
              }}
            />
          ) : (
            <p className="text-[12px] text-muted-foreground">
              Loading disk I/O…
            </p>
          )}
          {device === null && (
            <p className="text-[11px] text-muted-foreground">
              Request times are weighted by completed operations. A busy fast
              device can hide a slower device; select a drive to inspect it.
            </p>
          )}
          {missing && (
            <p role="status" className="text-[12px] text-muted-foreground">
              This device is unavailable. Recorded history remains selected.
            </p>
          )}
          {windowMs !== null && (
            <SelectionSummary windowMs={windowMs} inset={0} />
          )}
        </SectionCard>
      </div>
      {windowMs !== null && <DiskProcesses windowMs={windowMs} />}
      <MemoryContext />
      <SectionCard
        accent="disk"
        origin="tr"
        title="Volumes"
        variant="default"
        className="overflow-hidden"
      >
        <section
          aria-label="Volume capacity"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: the horizontally scrollable volume table needs keyboard access
          tabIndex={0}
          className="min-w-0 max-w-full overflow-x-auto rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <VolumeTable
            rows={volumeRows(vols, held, mounts)}
            units="GB"
            action={
              local && onAnalyze
                ? (row) => <AnalyzeLink vol={row.id} onAnalyze={onAnalyze} />
                : undefined
            }
          />
        </section>
      </SectionCard>
    </div>
  );
}
function CurrentRates({ device }: { device: string | null }) {
  const read =
    device === null ? "disk.read_total" : sk("disk.read", { dev: device });
  const write =
    device === null ? "disk.write_total" : sk("disk.write", { dev: device });
  const values = useHeld([read, write]);
  return (
    <StatStrip
      hero={{ label: "Read/s now", value: formatRate(values[read] ?? null) }}
      items={[
        { label: "Write/s now", value: formatRate(values[write] ?? null) },
      ]}
    />
  );
}
/** Analyze, for the volumes a space report can scan. */
function AnalyzeLink({
  vol,
  onAnalyze,
}: {
  vol: string;
  onAnalyze: (vol: string) => void;
}) {
  const volumes = useSpaceVolumes(true);
  if (!volumes.data?.some((v) => v.mount === vol)) return null;
  return (
    <button
      type="button"
      onClick={() => onAnalyze(vol)}
      className="rounded-sm text-[12px] text-fg-subtle underline underline-offset-2 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
    >
      Analyze space
    </button>
  );
}

function MemoryContext() {
  const memory = useMemory();
  if ((memory.pressureLevel ?? 0) === 0 && (memory.swapUsed ?? 0) === 0)
    return null;
  return (
    <p className="text-[12px] text-muted-foreground">
      This Mac is using swap or reporting memory pressure.{" "}
      <Link
        className="rounded-sm text-fg-subtle underline underline-offset-2 outline-none focus-visible:ring-2 focus-visible:ring-ring"
        to="/dashboard/memory"
      >
        Inspect Memory
      </Link>
    </p>
  );
}
