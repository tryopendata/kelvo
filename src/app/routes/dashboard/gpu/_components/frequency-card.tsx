import { fixed, MISSING } from "@core/format";
import { windowWords } from "@core/live-window";
import { residencyRows } from "@core/residency";
import { labelOf } from "@core/series-key";
import { SectionCard } from "~/components/section-card";
import { useGpuMaxMhz } from "~/hooks/use-host-record";
import { useHeld, useLayout, useRingStats } from "~/hooks/use-ring";
import { ResidencyBar } from "~/widgets/residency-bar";
import { RingGauge } from "~/widgets/ring-gauge";

/**
 * GPU frequency as a ring of the top DVFS state, and time per state over the
 * chart window (D-091). The maximum is the top of the GPU's DVFS table
 * (`HostInfo.gpu_dvfs_mhz`, D-092); the `gpu.residency` state labels are
 * MHz, as for CPU clusters.
 */
export function FrequencyCard({ windowMs }: { windowMs: number }) {
  const span = `last ${windowWords(windowMs)}`;
  const layout = useLayout();
  const states: { key: string; state: string }[] = [];
  layout?.series.forEach((s, i) => {
    if (s.metric !== "gpu.residency") return;
    const state = labelOf(s, "state");
    if (state !== undefined)
      states.push({ key: layout.keys[i] as string, state });
  });
  const maxMhz = useGpuMaxMhz();
  const hz = useHeld(["gpu.freq"])["gpu.freq"] ?? null;
  const stats = useRingStats(
    states.map((s) => s.key),
    windowMs
  );
  const pct: Record<string, number | null> = {};
  for (const s of states) pct[s.state] = stats[s.key]?.avg ?? null;
  const rows = residencyRows(pct);
  const idle = rows?.find((r) => r.label === "idle")?.pct;
  const ghz = hz === null ? null : hz / 1e9;
  const maxGhz = maxMhz === null ? null : maxMhz / 1000;

  return (
    <SectionCard
      accent="gpu"
      variant="default"
      title="Frequency"
      compactHeader
      className="gap-4"
    >
      <div className="flex items-start gap-5">
        <figure className="m-0 flex flex-col items-center gap-2.5">
          <RingGauge
            fractions={[ghz === null || !maxGhz ? null : ghz / maxGhz]}
            value={ghz === null ? MISSING : fixed(ghz, 2)}
            label="GHz"
            accent="gpu"
            size={112}
          />
          <figcaption className="figures text-[11px] text-muted-foreground">
            max {maxGhz === null ? MISSING : fixed(maxGhz, 2)}
          </figcaption>
        </figure>
        <div className="min-w-0 flex-1">
          {rows ? (
            <ResidencyBar
              cluster={`Last ${windowWords(windowMs)}`}
              activePct={idle === undefined ? 100 : 100 - idle}
              states={rows}
              accent="gpu"
            />
          ) : (
            <p className="m-0 font-normal text-[12px] text-muted-foreground">
              No frequency residency samples in the {span}
            </p>
          )}
        </div>
      </div>
    </SectionCard>
  );
}
