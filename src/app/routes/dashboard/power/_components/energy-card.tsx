import {
  formatClockSeconds,
  formatEnergy,
  formatSpan,
  formatWattsFine,
} from "@core/format";
import type { EnergyByApp, ProcessEnergy } from "@core/generated/bindings";
import { windowWords } from "@core/live-window";
import { CommandFailure } from "@core/transport";
import { ChevronRight } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import {
  APP_CELL,
  AppInitial,
  AppsNote,
  AppsShell,
  ShareCell,
} from "~/components/app-table";
import { ProcessRowActions, QuitDialog } from "~/components/process-actions";
import { SearchField } from "~/components/search-field";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "~/components/ui/table";
import { useEdition } from "~/hooks/use-edition";
import { type QuitFlow, useQuitFlow } from "~/hooks/use-quit-flow";
import { cn } from "~/lib/utils";
import { useEnergyByApp } from "../_hooks/use-energy-by-app";
import {
  type EnergyRow,
  energyRows,
  partialSince,
  processCount,
  processShare,
} from "../_lib/energy";

/** Apps listed before "Show all": enough to find the drain, short enough to scan. */
const COLLAPSED_APPS = 10;

/**
 * Energy by app over the chart window (D-093): what used the battery, not
 * what is busy now (that is the Processes page). Each app row sums its
 * processes (Chrome with its helpers) and expands to list them. Quit acts on
 * the app's main process; each running process has its own. The table
 * follows the Network Apps card.
 */
export function EnergyCard({ windowMs }: { windowMs: number }) {
  const q = useEnergyByApp(windowMs);
  const [query, setQuery] = useState("");
  const flow = useQuitFlow();
  // The App Store edition has no Quit or Force Quit (D-065).
  const canSignal = useEdition()?.process_signal === true;
  // The answer held over from another window would be mislabelled.
  const data =
    q.data && q.data.to_ms - q.data.from_ms === windowMs ? q.data : undefined;

  let body: ReactNode;
  if (q.isError) {
    body = (
      <AppsNote>
        {q.error instanceof CommandFailure &&
        q.error.error.kind === "remote_host"
          ? "Energy by app is only kept on the Mac it describes."
          : "Couldn't load energy by app."}
      </AppsNote>
    );
  } else if (!data) {
    body = <AppsNote>Loading energy by app…</AppsNote>;
  } else {
    body = (
      <EnergyBody
        data={data}
        windowMs={windowMs}
        query={query}
        flow={canSignal ? flow : null}
      />
    );
  }

  return (
    <AppsShell
      accent="power"
      title={`Energy by app, last ${windowWords(windowMs)}`}
      aside={
        <SearchField
          value={query}
          onChange={setQuery}
          placeholder="App, process or PID"
          label="Search energy by app, process or PID"
        />
      }
    >
      {body}
      <QuitDialog flow={flow} />
    </AppsShell>
  );
}

function EnergyBody({
  data,
  windowMs,
  query,
  flow,
}: {
  data: EnergyByApp;
  windowMs: number;
  query: string;
  /** Null when this edition cannot quit processes. */
  flow: QuitFlow | null;
}) {
  const rows = useMemo(() => energyRows(data, query), [data, query]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState(false);
  const searching = query.trim() !== "";
  const shown = showAll || searching ? rows : rows.slice(0, COLLAPSED_APPS);
  const partial = partialSince(data, windowMs);
  const toggle = (name: string) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (!next.delete(name)) next.add(name);
      return next;
    });

  if (data.apps.length === 0) {
    return (
      <AppsNote>
        {data.since_ms === null
          ? "Waiting for the first process sample…"
          : "No app has used measurable energy in this window yet."}
      </AppsNote>
    );
  }

  return (
    <>
      {partial && (
        <AppsNote>
          Kelvo started counting at{" "}
          <span className="data-mono">
            {formatClockSeconds(partial.sinceMs)}
          </span>
          , so these totals cover{" "}
          <span className="data-mono">{formatSpan(partial.measuredMs)}</span>.
        </AppsNote>
      )}
      <div className="overflow-x-auto">
        <Table aria-label="Energy by app">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>App</TableHead>
              <TableHead aria-sort="descending" className="text-right">
                Energy <span aria-hidden>↓</span>
              </TableHead>
              <TableHead className="text-right">Average</TableHead>
              <TableHead className="text-right">Share</TableHead>
              <TableHead className="w-[132px]">
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {shown.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell
                  colSpan={5}
                  className="px-3 py-3 font-normal text-[12px] text-muted-foreground"
                >
                  No app or process matches “{query.trim()}”.
                </TableCell>
              </TableRow>
            )}
            {shown.map((r) => (
              <AppRows
                key={r.app.name}
                row={r}
                data={data}
                open={r.matchedInside || expanded.has(r.app.name)}
                onToggle={() => toggle(r.app.name)}
                flow={flow}
              />
            ))}
            {!searching && rows.length > COLLAPSED_APPS && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={5} className="px-3 py-1.5">
                  <button
                    type="button"
                    onClick={() => setShowAll((v) => !v)}
                    className="rounded-sm font-[510] text-[12px] text-fg-subtle outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {showAll
                      ? `Show top ${COLLAPSED_APPS}`
                      : `Show all ${rows.length} apps`}
                  </button>
                </TableCell>
              </TableRow>
            )}
            <TableRow className="hover:bg-transparent">
              <TableCell
                colSpan={5}
                className="whitespace-normal px-3 pt-1 pb-2.5 font-normal text-[11px] text-muted-foreground leading-[1.45]"
              >
                CPU energy per process, as macOS estimates it. GPU, display and
                other users' processes (system daemons) aren't attributed, so
                these add up to less than system draw.
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </>
  );
}

function AppRows({
  row,
  data,
  open,
  onToggle,
  flow,
}: {
  row: EnergyRow;
  data: EnergyByApp;
  open: boolean;
  onToggle: () => void;
  flow: QuitFlow | null;
}) {
  const { app, quit } = row;
  const many = app.processes.length > 1;
  return (
    <>
      <TableRow className="group text-fg-subtle">
        <TableCell className={APP_CELL}>
          <span className="inline-flex min-w-0 items-center gap-2 text-foreground">
            {many ? (
              <button
                type="button"
                onClick={onToggle}
                aria-expanded={open}
                aria-label={`${open ? "Hide" : "Show"} ${app.name} processes`}
                className="-ml-1 grid size-4 place-items-center rounded-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              >
                <ChevronRight
                  aria-hidden
                  className={cn(
                    "size-3.5 transition-transform duration-(--motion-fast)",
                    open && "rotate-90"
                  )}
                />
              </button>
            ) : (
              <span aria-hidden className="-ml-1 inline-block size-4" />
            )}
            <AppInitial name={app.name} />
            <span className="truncate">{app.name}</span>
            {many && (
              <span className="font-normal text-[11px] text-muted-foreground">
                {processCount(app.processes.length)}
              </span>
            )}
          </span>
        </TableCell>
        <TableCell
          className={cn(APP_CELL, "data-mono text-right text-foreground")}
        >
          {formatEnergy(app.energy_j)}
        </TableCell>
        <TableCell className={cn(APP_CELL, "data-mono text-right")}>
          {formatWattsFine(app.avg_w)}
        </TableCell>
        <ShareCell share={row.share} muted={false} />
        <ActionsCell process={quit} flow={flow} />
      </TableRow>
      {open &&
        row.processes.map((p) => (
          <ProcessRow
            key={`${p.pid}:${p.start_time_us}`}
            p={p}
            share={processShare(p, data)}
            flow={flow}
          />
        ))}
    </>
  );
}

function ProcessRow({
  p,
  share,
  flow,
}: {
  p: ProcessEnergy;
  share: number | null;
  flow: QuitFlow | null;
}) {
  return (
    <TableRow
      className={cn(
        "group",
        p.running ? "text-fg-subtle" : "text-muted-foreground"
      )}
    >
      <TableCell className={cn(APP_CELL, "pl-[52px]")}>
        <span className="inline-flex min-w-0 items-baseline gap-2">
          <span className="truncate">{p.name}</span>
          <span className="data-mono text-[11px] text-muted-foreground">
            {p.pid}
          </span>
        </span>
      </TableCell>
      <TableCell className={cn(APP_CELL, "data-mono text-right")}>
        {formatEnergy(p.energy_j)}
      </TableCell>
      <TableCell className={cn(APP_CELL, "data-mono text-right")}>
        {formatWattsFine(p.avg_w)}
      </TableCell>
      <ShareCell share={share} muted />
      {p.running ? (
        <ActionsCell process={p} flow={flow} />
      ) : (
        <TableCell
          className={cn(APP_CELL, "text-right text-[11px] text-fg-faint")}
        >
          exited
        </TableCell>
      )}
    </TableRow>
  );
}

/** Quit and Force Quit, shown while the row is hovered or focused. */
function ActionsCell({
  process,
  flow,
}: {
  process: ProcessEnergy | null;
  flow: QuitFlow | null;
}) {
  return (
    <TableCell className={cn(APP_CELL, "py-0 text-right")}>
      {process && flow && (
        <span className="inline-flex h-6 items-center align-middle opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">
          <ProcessRowActions p={process} onRequest={flow.request} />
        </span>
      )}
    </TableCell>
  );
}
