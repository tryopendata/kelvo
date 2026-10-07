import { cn } from "~/lib/utils";
import { type Accent, accentVars } from "./lib/accent";

export interface CoreTilesCluster {
  /** Cluster label value ("P0"); stable key. */
  id: string;
  /** "P-cluster", "E-cluster". */
  name: string;
  /** Formatted cluster frequency ("3.2 GHz"), or "—" when not sampled. */
  freq: string;
  /** `load` is percent 0 to 100, `null` when the core was not sampled. */
  cores: { id: string; load: number | null }[];
}

export interface CoreTilesProps {
  clusters: CoreTilesCluster[];
  accent?: Accent;
}

/** Above this load the tile is light enough to need dark text. */
const DARK_TEXT_FROM = 55;

/**
 * Per-core load tiles grouped by cluster. Tile background
 * alpha follows load (10% to 95% of the accent); the number is always printed,
 * so the reading never rests on color.
 */
export function CoreTiles({ clusters, accent = "cpu" }: CoreTilesProps) {
  return (
    <div className="flex flex-col gap-2" style={accentVars(accent)}>
      {clusters.map((cluster) => (
        <div
          key={cluster.id}
          className="grid grid-cols-[76px_minmax(0,1fr)] items-center gap-2"
        >
          <div className="flex flex-col">
            <span className="font-normal text-[11px] text-fg-subtle">
              {cluster.name}
            </span>
            <span className="data-mono text-[11px]">{cluster.freq}</span>
          </div>
          <ul
            aria-label={`${cluster.name} core load`}
            className="grid grid-cols-10 gap-0.5"
          >
            {cluster.cores.map((core) => {
              const load =
                core.load === null || !Number.isFinite(core.load)
                  ? null
                  : Math.round(Math.min(100, Math.max(0, core.load)));
              return (
                <li
                  key={core.id}
                  title={`${core.id} ${load === null ? "no sample" : `${load}%`}`}
                  aria-label={`${core.id} ${load === null ? "no sample" : `${load}%`}`}
                  className={cn(
                    "data-mono flex h-5 items-center justify-center rounded-mark text-[9px] transition-[background-color] duration-(--motion-tick) ease-tick",
                    load === null
                      ? "bg-track text-muted-foreground"
                      : load > DARK_TEXT_FROM
                        ? "text-primary-foreground"
                        : "text-foreground"
                  )}
                  style={
                    load === null
                      ? undefined
                      : {
                          background: `color-mix(in srgb, var(--a) ${(10 + load * 0.85).toFixed(1)}%, transparent)`,
                        }
                  }
                >
                  {load === null ? "–" : load}
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </div>
  );
}
