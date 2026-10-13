import { seriesColor } from "@core/chart-math/series-lines";
import { formatBytes, formatPercent } from "@core/format";
import type { DockerContainer } from "@core/generated/bindings";
import { useState } from "react";
import { SeriesSwatch } from "~/components/charts/chart-tooltip";
import { SearchField } from "~/components/search-field";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "~/components/ui/dialog";
import { useUnits } from "~/hooks/use-units";

export function ChartChooser({
  open,
  onOpenChange,
  candidates,
  liveRows,
  selected,
  slots,
  metric,
  onChange,
  onAutomatic,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  candidates: readonly DockerContainer[];
  liveRows: readonly DockerContainer[];
  selected: readonly string[];
  slots: Record<string, number>;
  metric: "cpu" | "memory";
  onChange: (ids: string[]) => void;
  onAutomatic: () => void;
}) {
  const [query, setQuery] = useState("");
  const { bytes } = useUnits();
  // The live parent updates every tick; closed dialogs must not build hundreds of rows.
  if (!open) return null;
  const current = new Map(
    liveRows
      .filter((row) => row.lifecycle === "running")
      .map((row) => [`${row.engine_id}:${row.id}`, row.metrics])
  );
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogTitle>Show on chart</DialogTitle>
        <DialogDescription>
          Compare up to six containers. Recording settings stay the same.
        </DialogDescription>
        <SearchField
          value={query}
          onChange={setQuery}
          label="Search chart candidates"
          placeholder="Search containers…"
        />
        <div className="max-h-88 overflow-y-auto">
          {candidates
            .filter((row) =>
              `${row.name} ${row.project ?? ""}`
                .toLowerCase()
                .includes(query.toLowerCase())
            )
            .map((row) => (
              <label
                key={row.id}
                className="flex min-h-11 items-center gap-3 border-border-subtle border-b py-2 text-[12px]"
              >
                <input
                  type="checkbox"
                  aria-label={`Chart ${row.name}`}
                  checked={selected.includes(row.id)}
                  disabled={!selected.includes(row.id) && selected.length >= 6}
                  onChange={(event) =>
                    onChange(
                      event.target.checked
                        ? [...selected, row.id]
                        : selected.filter((id) => id !== row.id)
                    )
                  }
                />
                {slots[row.id] ? (
                  <SeriesSwatch color={seriesColor(slots[row.id] ?? 1)} />
                ) : (
                  <span className="size-2" />
                )}
                <span className="min-w-0 flex-1 truncate">
                  {row.name}
                  <span className="text-muted-foreground">
                    {" "}
                    · {row.project ?? "Standalone"}
                  </span>
                </span>
                <span className="figures">
                  {metric === "cpu"
                    ? formatPercent(
                        current.get(`${row.engine_id}:${row.id}`)?.cpu_pct ??
                          null
                      )
                    : formatBytes(
                        current.get(`${row.engine_id}:${row.id}`)
                          ?.working_set_bytes ?? null,
                        {
                          units: bytes,
                        }
                      )}
                </span>
              </label>
            ))}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2 text-[12px]">
          <span>{selected.length} of 6</span>
          {selected.length >= 6 && (
            <span className="text-muted-foreground">
              Remove a container to compare another.
            </span>
          )}
        </div>
        <div className="flex justify-between">
          <Button variant="outline" onClick={onAutomatic}>
            Use top 5
          </Button>
          <Button onClick={() => onOpenChange(false)}>Done</Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
