import { scopeWords, type TimeRange } from "@core/brush";
import { percentOf } from "@core/chart-math";
import { formatClockSeconds, formatSpan } from "@core/format";
import type {
  ProcessUsage,
  UsageByApp,
  UsageKey,
} from "@core/generated/bindings";
import { targetKey } from "@core/process-selection";
import { CommandFailure } from "@core/transport";
import {
  gpuShortfall,
  processCount,
  type RemainderRow,
  remainderRows,
  type UsageFigures,
  type UsageRow,
  usageCoverage,
  usageRows,
  usageValue,
  usageWhole,
} from "@core/usage-rows";
import { ChevronRight } from "lucide-react";
import { type ReactNode, useMemo, useRef, useState } from "react";
import {
  APP_CELL,
  AppsNote,
  AppsShell,
  OtherAppsRow,
  ShareCell,
  UnrecordedRange,
} from "~/components/app-table";
import { brushScopeProps } from "~/components/brush-overlay";
import {
  ProcessSelectionHead,
  ProcessSelectionRow,
  ProcessSelectionSurface,
  SharedProcessActions,
  useProcessSelection,
} from "~/components/process-selection";
import { SearchField } from "~/components/search-field";
import { SelectionChip } from "~/components/selection-chip";
import { SortHeader } from "~/components/sort-header";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "~/components/ui/table";
import { useEdition } from "~/hooks/use-edition";
import { useHostNoun } from "~/hooks/use-host-record";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { heldFor, useRangeScope } from "~/hooks/use-range-scope";
import { useUsageByApp } from "~/hooks/use-usage-by-app";
import { cn } from "~/lib/utils";
import { InitialChip } from "~/widgets/initial-chip";
import type { Accent } from "~/widgets/lib/accent";

/** Apps listed before "Show all": enough to find the culprit, short enough to scan. */
const COLLAPSED_APPS = 10;

/** What a cell can read: any row's figures, and an app's average footprint. */
export type UsageCells = UsageFigures & { mem_avg_bytes?: number };

export interface UsageColumn {
  /** Column head ("Avg CPU"). */
  label: string;
  format: (u: UsageCells) => string;
  /** The table is ranked by this column (Rust sorts by the page's key). */
  ranked?: boolean;
}

export interface UsageTableConfig {
  by: UsageKey;
  /** "CPU": the title reads "CPU by app, last 15 minutes". */
  noun: string;
  accent: Accent;
  columns: readonly UsageColumn[];
  /** Under the rows: what the figures are and what they leave out. */
  footnote: ReactNode;
}

/**
 * Per-app use over the chart window (D-091), or over the brushed range
 * when there is one (D-099): which apps used the resource, not what is busy
 * now (that is the Processes page). Rows are apps that sum their processes
 * and expand to list them; an app's actions target its main process, with
 * individual processes selectable too. After the apps come "Other apps" and
 * "System and other", so additive columns add up to the host's figure.
 */
export function UsageAppsCard(props: {
  windowMs: number;
  config: UsageTableConfig;
  viewControl?: ReactNode;
}) {
  const local = useIsLocalHost();
  // A Linux agent keeps no per-app history to open elsewhere (D-115).
  const mac = useHostNoun() === "Mac";
  return local ? (
    <LocalUsageAppsCard {...props} />
  ) : (
    <p className="text-[12px] text-muted-foreground">
      {mac
        ? "Historical use by app is available on the monitored Mac."
        : "Historical use by app isn't kept for this host."}
    </p>
  );
}

function LocalUsageAppsCard({
  windowMs,
  config,
  viewControl,
}: {
  windowMs: number;
  config: UsageTableConfig;
  viewControl?: ReactNode;
}) {
  const { selection, range, keepPrevious } = useRangeScope(windowMs);
  const q = useUsageByApp(range, config.by, { keepPrevious });
  const [query, setQuery] = useState("");
  // The App Store edition has no Quit or Force Quit (D-065).
  const canSignal = useEdition()?.process_signal === true;
  const data = heldFor(q, windowMs);
  const shown: TimeRange | null = data
    ? { fromMs: data.from_ms, toMs: data.to_ms }
    : selection;

  let body: ReactNode;
  if (q.isError) {
    body = (
      <AppsNote>
        {q.error instanceof CommandFailure &&
        q.error.error.kind === "remote_host"
          ? "Use by app is only kept on the Mac it describes."
          : "Couldn't load use by app."}
      </AppsNote>
    );
  } else if (!data) {
    body = <AppsNote>Loading use by app…</AppsNote>;
  } else {
    body = (
      <UsageBody
        data={data}
        config={config}
        query={query}
        canSignal={canSignal}
        scopeKey={
          selection
            ? `${selection.fromMs}:${selection.toMs}`
            : `live:${windowMs}`
        }
      />
    );
  }

  return (
    <div {...brushScopeProps} className="contents">
      <AppsShell
        accent={config.accent}
        title={`${config.noun} by app, ${scopeWords(selection ? (shown ?? selection) : null, windowMs)}`}
        aside={
          <>
            {viewControl}
            <SearchField
              value={query}
              onChange={setQuery}
              placeholder="App, process or PID"
              label={`Search ${config.noun} by app, process or PID`}
            />
            {selection && shown && <SelectionChip range={shown} />}
          </>
        }
      >
        {body}
      </AppsShell>
    </div>
  );
}

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
function UsageBody({
  data,
  config,
  query,
  canSignal,
  scopeKey,
}: {
  data: UsageByApp;
  config: UsageTableConfig;
  query: string;
  canSignal: boolean;
  scopeKey: string;
}) {
  const { by } = config;
  const ranked = useMemo(() => usageRows(data, by, query), [data, by, query]);
  const [held, setHeld] = useState(false);
  const prior = useRef<string[]>([]);
  const rows = useMemo(() => {
    const order = held ? prior.current : [];
    const rank = new Map(order.map((key, i) => [key, i]));
    const next = [...ranked].sort(
      (a, b) =>
        (rank.get(a.app.name) ?? order.length) -
        (rank.get(b.app.name) ?? order.length)
    );
    prior.current = next.map((r) => r.app.name);
    return next;
  }, [ranked, held]);
  const rest = useMemo(() => remainderRows(data, by), [data, by]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState(false);
  const searching = query.trim() !== "";
  const shown = showAll || searching ? rows : rows.slice(0, COLLAPSED_APPS);
  const coverage = usageCoverage(data);
  const share = usageWhole(data, by) !== null;
  const span =
    2 + config.columns.length + (share ? 1 : 0) + (canSignal ? 1 : 0);
  const toggle = (name: string) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (!next.delete(name)) next.add(name);
      return next;
    });

  if (coverage.kind === "waiting") {
    return <AppsNote>Waiting for the first process sample…</AppsNote>;
  }
  if (coverage.kind === "unrecorded") {
    return <Unrecorded sinceMs={coverage.sinceMs} />;
  }

  const content = (
    <>
      {coverage.kind === "partial" && (
        <AppsNote>
          Kelvo started counting at{" "}
          <span className="figures">
            {formatClockSeconds(coverage.sinceMs)}
          </span>
          , so these figures cover{" "}
          <span className="figures">{formatSpan(coverage.coveredMs)}</span>.
        </AppsNote>
      )}
      {coverage.kind === "gaps" && (
        <AppsNote>
          Processes were sampled for{" "}
          <span className="figures">{formatSpan(coverage.coveredMs)}</span> of{" "}
          <span className="figures">{formatSpan(coverage.spanMs)}</span> (the
          Mac slept or sampling paused). Averages are over the sampled time.
        </AppsNote>
      )}
      {by === "gpu" && <GpuShortfallNote data={data} />}
      {data.other.clamped.includes(by) && (
        <AppsNote>
          Apps measured above the system total somewhere in this range (the two
          are sampled differently), so System and other reads low there.
        </AppsNote>
      )}
      <div className="overflow-x-auto">
        <Table aria-label={`${config.noun} by app`}>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <ProcessSelectionHead />
              <TableHead>App</TableHead>
              {config.columns.map((c) =>
                c.ranked ? (
                  <SortHeader key={c.label} ranked label={c.label} />
                ) : (
                  <TableHead key={c.label} className="text-right">
                    {c.label}
                  </TableHead>
                )
              )}
              {share && <TableHead className="text-right">Share</TableHead>}
              <TableHead className="w-[132px]">
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {shown.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell
                  colSpan={span}
                  className="px-3 py-3 font-normal text-[12px] text-muted-foreground"
                >
                  {searching
                    ? `No app or process matches “${query.trim()}”.`
                    : "No app used a measurable amount in this range."}
                </TableCell>
              </TableRow>
            )}
            <UsageAppRows
              rows={shown}
              data={data}
              config={config}
              share={share}
              expanded={expanded}
              toggle={toggle}
            />
            {!searching && rows.length > COLLAPSED_APPS && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={span} className="px-3 py-1.5">
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
            {!searching &&
              rest.map((r, i) => (
                <RestRow key={r.kind} row={r} config={config} first={i === 0} />
              ))}
            <TableRow className="hover:bg-transparent">
              <TableCell
                colSpan={span}
                className="whitespace-normal px-3 pt-1 pb-2.5 font-normal text-[11px] text-muted-foreground leading-[1.45]"
              >
                {config.footnote}
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </>
  );
  const descriptors = shown.flatMap((r) => [
    {
      key: `app:${r.app.name}`,
      label: r.app.name,
      targets: appTargets(r),
    },
    ...(r.matchedInside || expanded.has(r.app.name)
      ? r.processes.map((p) => ({
          key: targetKey(p),
          parentKey: `app:${r.app.name}`,
          label: `${p.name} (${p.pid})`,
          targets: [p],
        }))
      : []),
  ]);
  return canSignal ? (
    <ProcessSelectionSurface
      rows={descriptors}
      allTargets={rows.flatMap((r) => r.processes)}
      scopeKey={scopeKey}
      onHoldChange={setHeld}
    >
      {content}
    </ProcessSelectionSurface>
  ) : (
    content
  );
}

/**
 * An app row acts on the process Rust picked to quit the app (`quit_pid`).
 * Its helpers exit with it; signalling them first would cut a graceful quit
 * short (an app asking to save loses its helpers). Expand to pick members.
 */
function appTargets(row: UsageRow): ProcessUsage[] {
  return row.quit ? [row.quit] : [];
}

function UsageAppRows({
  rows,
  data,
  config,
  share,
  expanded,
  toggle,
}: {
  rows: readonly UsageRow[];
  data: UsageByApp;
  config: UsageTableConfig;
  share: boolean;
  expanded: ReadonlySet<string>;
  toggle: (name: string) => void;
}) {
  const selection = useProcessSelection();
  const order = new Map(selection?.rows.map((r, i) => [r.key, i]));
  const ordered = selection
    ? [...rows].sort(
        (a, b) =>
          (order.get(`app:${a.app.name}`) ?? 0) -
          (order.get(`app:${b.app.name}`) ?? 0)
      )
    : rows;
  return ordered.map((r) => (
    <AppRows
      key={r.app.name}
      row={r}
      data={data}
      config={config}
      share={share}
      open={r.matchedInside || expanded.has(r.app.name)}
      onToggle={() => toggle(r.app.name)}
    />
  ));
}

function GpuShortfallNote({ data }: { data: UsageByApp }) {
  const short = gpuShortfall(data);
  if (short === null) return null;
  return (
    <AppsNote>
      GPU time was measured for{" "}
      <span className="figures">{formatSpan(short.gpuMs)}</span> of the{" "}
      <span className="figures">{formatSpan(short.coveredMs)}</span> sampled.
      Averages are over the measured time.
    </AppsNote>
  );
}

/** A range with no process sample in it: a hatched box, as on Network. */
function Unrecorded({ sinceMs }: { sinceMs: number | null }) {
  return (
    <UnrecordedRange>
      {sinceMs === null ? (
        "No process data for this range"
      ) : (
        <>
          No process data · Kelvo started counting at{" "}
          <span className="figures">{formatClockSeconds(sinceMs)}</span>
        </>
      )}
    </UnrecordedRange>
  );
}

function FigureCells({
  u,
  config,
  strong,
}: {
  u: UsageCells;
  config: UsageTableConfig;
  strong: boolean;
}) {
  return config.columns.map((c) => (
    <TableCell
      key={c.label}
      className={cn(
        APP_CELL,
        "figures text-right",
        strong && c.ranked && "text-foreground"
      )}
    >
      {c.format(u)}
    </TableCell>
  ));
}

function AppRows({
  row,
  data,
  config,
  share,
  open,
  onToggle,
}: {
  row: UsageRow;
  data: UsageByApp;
  config: UsageTableConfig;
  share: boolean;
  open: boolean;
  onToggle: () => void;
}) {
  const { app } = row;
  const many = app.processes.length > 1;
  const selection = useProcessSelection();
  const order = new Map(selection?.rows.map((r, i) => [r.key, i]));
  const processes = selection
    ? [...row.processes].sort(
        (a, b) =>
          (order.get(targetKey(a)) ?? 0) - (order.get(targetKey(b)) ?? 0)
      )
    : row.processes;
  return (
    <>
      <ProcessSelectionRow
        rowKey={`app:${app.name}`}
        label={app.name}
        targets={appTargets(row)}
        className="group text-fg-subtle"
        onKeyDown={(e) => {
          if (many && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
            e.preventDefault();
            if ((e.key === "ArrowRight") !== open) onToggle();
          }
        }}
      >
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
            <InitialChip text={app.name} />
            <span className="truncate">{app.name}</span>
            {many && (
              <span className="font-normal text-[11px] text-muted-foreground">
                {processCount(app.processes.length)}
              </span>
            )}
          </span>
        </TableCell>
        <FigureCells u={app} config={config} strong />
        {share && <ShareCell share={row.share} muted={false} />}
        <ActionsCell processes={appTargets(row)} />
      </ProcessSelectionRow>
      {open &&
        processes.map((p) => (
          <ProcessRow
            key={`${p.pid}:${p.start_time_us}`}
            p={p}
            config={config}
            share={
              share
                ? percentOf(
                    usageValue(config.by, p),
                    usageWhole(data, config.by)
                  )
                : undefined
            }
          />
        ))}
    </>
  );
}

function ProcessRow({
  p,
  config,
  share,
}: {
  p: ProcessUsage;
  config: UsageTableConfig;
  /** Undefined when the table has no share column. */
  share: number | null | undefined;
}) {
  return (
    <ProcessSelectionRow
      rowKey={targetKey(p)}
      label={`${p.name} (${p.pid})`}
      targets={[p]}
      className={cn(
        "group",
        p.running ? "text-fg-subtle" : "text-muted-foreground"
      )}
    >
      <TableCell className={cn(APP_CELL, "pl-[52px]")}>
        <span className="inline-flex min-w-0 items-baseline gap-2">
          <span className="truncate">{p.name}</span>
          <span className="figures text-[11px] text-muted-foreground">
            {p.pid}
          </span>
        </span>
      </TableCell>
      <FigureCells u={p} config={config} strong={false} />
      {share !== undefined && <ShareCell share={share} muted />}
      {p.running ? (
        <ActionsCell processes={[p]} />
      ) : (
        <TableCell
          className={cn(APP_CELL, "text-right text-[11px] text-fg-faint")}
        >
          exited
        </TableCell>
      )}
    </ProcessSelectionRow>
  );
}

function RestRow({
  row,
  config,
  first,
}: {
  row: RemainderRow;
  config: UsageTableConfig;
  first: boolean;
}) {
  return (
    <OtherAppsRow
      name={row.name}
      system={row.kind === "system"}
      first={first}
      indent
      selectionCell={<SelectionSpacer />}
    >
      <FigureCells u={row.figures} config={config} strong={false} />
      <ShareCell share={row.share} muted />
      <TableCell className={APP_CELL} />
    </OtherAppsRow>
  );
}

/** Shared actions use the same targets as the row's selection. */
function ActionsCell({ processes }: { processes: readonly ProcessUsage[] }) {
  return (
    <TableCell className={cn(APP_CELL, "py-0 text-right")}>
      <span className="inline-flex h-6 items-center align-middle opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">
        <SharedProcessActions targets={processes} />
      </span>
    </TableCell>
  );
}

function SelectionSpacer() {
  return useProcessSelection() ? <TableCell aria-label="Selection" /> : null;
}
