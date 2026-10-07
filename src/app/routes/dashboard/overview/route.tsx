import { isUnknownChip, type UiModule } from "@core/module-state";
import type { ProcessView } from "@core/process-interest";
import type { ReactNode } from "react";
import { CardGrid } from "~/components/card-grid";
import {
  SensorDumpDialog,
  useSensorDumpDialog,
} from "~/components/sensor-dump-dialog";
import { UnavailableCard } from "~/components/unavailable-card";
import { UnsupportedNotice } from "~/components/unsupported-notice";
import { useHostRecord } from "~/hooks/use-host-record";
import { useModuleStates } from "~/hooks/use-module-states";
import {
  useProcessGpu,
  useProcessInterest,
  useProcessNetwork,
} from "~/hooks/use-process-interest";
import { useHost } from "~/stores/host-store";
import { useSampling } from "~/stores/live-selectors";
import type { Corner } from "~/widgets/lib/accent";
import { StatusPill } from "~/widgets/status-pill";
import { samplingStatus } from "../_lib/sampling-status";
import { LiveMachineHeader } from "./_components/live-machine-header";
import {
  OverviewCpuCard,
  OverviewDiskCard,
  OverviewGpuCard,
  OverviewMemoryCard,
  OverviewNetworkCard,
  OverviewPowerCard,
} from "./_components/overview-cards";

/**
 * The cards' top-5 lists rank by CPU, memory, energy impact and disk I/O.
 * Rust sends the union of the top 5 by each, every 5 s (D-066), so the
 * Overview no longer makes the collector read every process each tick.
 */
const OVERVIEW_PROCESS_VIEW: ProcessView = {
  limit: 5,
  sort: ["cpu", "memory", "energy", "disk_total"],
  period_ms: 5000,
};

/**
 * With per-process network (D-081) the Network card's list ranks processes
 * by total rate too, and with per-process GPU time (D-085) the GPU card's by
 * GPU share; the host samples each on the same 5 s process ticks while the
 * Overview is visible. `useProcessInterest` compares views by value.
 */
function overviewView(network: boolean, gpu: boolean): ProcessView {
  return {
    ...OVERVIEW_PROCESS_VIEW,
    sort: [
      ...OVERVIEW_PROCESS_VIEW.sort,
      ...(network ? (["net_total"] as const) : []),
      ...(gpu ? (["gpu"] as const) : []),
    ],
    ...(network ? { network } : {}),
    ...(gpu ? { gpu } : {}),
  };
}

/** The six Overview modules in display order. Battery has no card. */
const CARDS: readonly [UiModule, (p: { origin: Corner }) => ReactNode][] = [
  ["cpu", OverviewCpuCard],
  ["gpu", OverviewGpuCard],
  ["memory", OverviewMemoryCard],
  ["power", OverviewPowerCard],
  ["network", OverviewNetworkCard],
  ["disk", OverviewDiskCard],
];

/**
 * Overview (plan 4.5): page header with the live status pill,
 * the machine header, and a card per module the host has and the user has
 * on. A module the build cannot run keeps its slot with a "not available"
 * card; an absent one is left out and the grid reflows. On an unknown chip
 * Power & Sensors is hidden and the notice says why.
 */
export default function OverviewRoute() {
  // Process rows feed the CPU, Memory, Power and Disk cards' top-5 lists,
  // the Network card's when the host can attribute traffic, and the GPU
  // card's when it can attribute GPU time.
  const perProcessNetwork = useProcessNetwork();
  const perProcessGpu = useProcessGpu();
  useProcessInterest(overviewView(perProcessNetwork, perProcessGpu));
  const states = useModuleStates();
  const sampling = useSampling();
  const unknownChip = useHost((s) => isUnknownChip(s.capabilities));
  const host = useHostRecord();
  const dump = useSensorDumpDialog();
  const items = CARDS.filter(([m]) => {
    const s = states[m];
    return s === "on" || s === "edition" || s === "unavailable";
  });

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-center justify-between gap-3">
        <h1 className="m-0 font-[590] text-[22px] tracking-[-0.022em]">
          Overview
        </h1>
        <StatusPill {...samplingStatus(sampling)} />
      </header>
      <LiveMachineHeader />
      <CardGrid items={items} getKey={([m]) => m}>
        {([m, Card], origin) =>
          states[m] === "on" ? (
            <Card origin={origin} />
          ) : (
            <UnavailableCard module={m} state={states[m]} />
          )
        }
      </CardGrid>
      {unknownChip && (
        <UnsupportedNotice
          modelId={host?.info.model ?? "this Mac"}
          onShareDump={dump.show}
        />
      )}
      <SensorDumpDialog open={dump.open} onOpenChange={dump.setOpen} />
    </div>
  );
}
