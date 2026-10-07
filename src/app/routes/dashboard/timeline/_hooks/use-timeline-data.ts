import type {
  Gap,
  HistoryPage,
  ProcessesAt,
  Tier,
  TierRequest,
} from "@core/generated/bindings";
import { HISTORY_COMMIT_MS } from "@core/history-state";
import { historyKeys, type RangeSpec } from "@core/query-keys";
import { type CommandFailure, unwrap } from "@core/transport";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTransport } from "~/lib/transport-context";
import { useHost, useHostId } from "~/stores/host-store";
import { type Bucket, combineSeries } from "../_lib/buckets";
import { mergeTail, tailRange } from "../_lib/history-tail";
import type { LaneDef } from "../_lib/lanes";
import {
  historyWindow,
  requestBucketMs,
  resolutionLabel,
  SPAN_MS,
  type Span,
} from "../_lib/time";

export interface TimelineView {
  span: Span;
  /** `null` follows now (Live); a number is a window the user stepped back to. */
  endMs: number | null;
}

export interface LaneData {
  def: LaneDef;
  /** Buckets per metric id, its label sets combined. */
  series: Record<string, Bucket[]>;
  /** `hold_ms` per metric id: the most any of its label sets has. */
  holds: Record<string, number>;
}

export interface TimelineData {
  fromMs: number;
  toMs: number;
  bucketMs: number;
  /** The tier the history came from, once a page has arrived. */
  tier: Tier | null;
  /** The tooltip's "1 min avg": the bucket width actually drawn. */
  resolution: string;
  lanes: LaneData[];
  gaps: Gap[];
  loading: boolean;
  error: CommandFailure | null;
}

/**
 * Every span asks for Auto and draws at the tier the store picked by the
 * range's start (D-041, D-076). 1h reads 10 s buckets while they cover it.
 * 24h is longer than S10 retention, so it reads 1 min buckets within the
 * last 7 days and 15 min buckets for a day older than that, where the
 * minutes have been rolled down and an `m1` read comes back empty. 7d
 * ending now reads minutes; 30d, or 7d stepped back, reads quarters with
 * the newest minutes folded in. Both are merged server-side to about 2
 * points per plot pixel.
 */
const TIER_REQUEST: TierRequest = "auto";

/**
 * History for every lane over the window the view asks for. Rust answers
 * through now (D-092). While following Live the window ends at the start of
 * the open bucket, and the newest slots are read again each time a bucket
 * closes, so this re-renders once per closed bucket, not per tick.
 */
export function useTimelineData(
  view: TimelineView,
  lanes: readonly LaneDef[],
  /** Width of the plot column; picks the bucket for 7d and 30d. */
  plotPx = 0
): TimelineData {
  const transport = useTransport();
  const hostId = useHostId();
  const queryClient = useQueryClient();
  const live = view.endMs === null;
  const spanMs = SPAN_MS[view.span];
  const long = view.span === "7d" || view.span === "30d";
  const requestBucket = requestBucketMs(view.span, plotPx);
  // The bucket is part of the range: a window resized across a step of the
  // bucket ladder reads again at the new width. Not the pixel width itself,
  // so resizing within a step does not.
  const keys = useMemo(() => {
    const range: RangeSpec = {
      span: view.span,
      endMs: view.endMs,
      bucketMs: requestBucket,
    };
    return lanes.map((lane) =>
      historyKeys.range(hostId, lane.module, range, TIER_REQUEST)
    );
  }, [lanes, hostId, view.span, view.endMs, requestBucket]);

  const queries = useQueries({
    queries: lanes.map((lane, i) => ({
      queryKey: keys[i] as (typeof keys)[number],
      queryFn: () => {
        const w = historyWindow(
          view.span,
          view.endMs ?? Date.now(),
          requestBucket
        );
        return unwrap(
          transport.queryHistory({
            host: hostId,
            selectors: lane.metrics.map((m) => ({
              metric: m.metric,
              labels: [],
            })),
            from_ms: w.fromMs,
            to_ms: w.toMs,
            tier: TIER_REQUEST,
            max_points: w.maxPoints,
          })
        );
      },
      // 7d and 30d wait for the plot's width so they are read once, at it.
      enabled: !long || plotPx > 0,
    })),
  });

  const pages = queries.map((q) => q.data);
  const first = pages.find((p) => p !== undefined);
  const tier = first?.tier ?? null;
  // The width the store merged the points to (`bucket_ms`), not the tier's:
  // 7d of minutes comes back as 10-minute points. A zero width (a page
  // from a peer that did not fill it) falls back to the width asked for.
  const bucketMs = first?.bucket_ms || requestBucket;

  // The start of the open bucket: changes once per bucket, not per frame.
  const liveEdge = useHost((s) =>
    s.lastTsMs === null ? null : Math.floor(s.lastTsMs / bucketMs) * bucketMs
  );
  const [mountedAt] = useState(() => Date.now());
  const toMs =
    view.endMs ?? liveEdge ?? Math.floor(mountedAt / bucketMs) * bucketMs;
  const fromMs = toMs - spanMs;

  // Following Live, a bucket closing reads the newest slots again (Rust has
  // the closed one now) and merges them into the page; a page that cannot
  // be extended that way (`tailRange`) is read again whole. The edge is seen
  // with its width: a first page at another width than asked moves the edge
  // without closing a bucket.
  const seen = useRef({ edge: liveEdge, bucketMs });
  useEffect(() => {
    if (!live || liveEdge === null) return;
    const prev = seen.current;
    seen.current = { edge: liveEdge, bucketMs };
    // The first frame lands in the bucket the page was read in.
    if (prev.edge === null || prev.bucketMs !== bucketMs) return;
    if (prev.edge === liveEdge) return;
    const nowMs = Date.now();
    const keepFromMs = historyWindow(view.span, nowMs, requestBucket).fromMs;
    lanes.forEach((lane, i) => {
      const queryKey = keys[i];
      if (!queryKey) return;
      const state = queryClient.getQueryState<HistoryPage>(queryKey);
      // Not read yet, or being read: that read is through now.
      if (!state?.data || state.fetchStatus === "fetching") return;
      const tail = tailRange(state.data, state.dataUpdatedAt, nowMs);
      if (!tail) {
        void queryClient.refetchQueries({ queryKey, exact: true });
        return;
      }
      void transport
        .queryHistory({
          host: hostId,
          selectors: lane.metrics.map((m) => ({
            metric: m.metric,
            labels: [],
          })),
          from_ms: tail.fromMs,
          to_ms: tail.toMs,
          tier: tail.tier,
          max_points: tail.maxPoints,
        })
        .then((res) => {
          if (res.status !== "ok") {
            void queryClient.refetchQueries({ queryKey, exact: true });
            return;
          }
          const now = queryClient.getQueryState<HistoryPage>(queryKey);
          // A newer read (a whole refetch) landed meanwhile: it wins.
          if (!now?.data || now.dataUpdatedAt > nowMs) return;
          queryClient.setQueryData<HistoryPage>(
            queryKey,
            mergeTail(now.data, res.data, tail.fromMs, keepFromMs),
            { updatedAt: nowMs }
          );
        });
    });
  }, [
    live,
    liveEdge,
    bucketMs,
    keys,
    lanes,
    hostId,
    transport,
    queryClient,
    view.span,
    requestBucket,
  ]);

  // `pages` is a fresh array each render; the stamp changes when any page does.
  const readAt = queries.map((q) => q.dataUpdatedAt);
  const dataStamp = readAt.join(",");
  // biome-ignore lint/correctness/useExhaustiveDependencies: keyed on dataStamp instead of the per-render pages array
  const data = useMemo(() => {
    const laneData = lanes.map((def, i) => {
      const page = pages[i];
      // A bucket still open when the page was read is partial: it is drawn
      // once a read after it closed has it, even when the window's end has
      // already moved past it.
      const readAtMs = readAt[i] ?? 0;
      const series: Record<string, Bucket[]> = {};
      const holds: Record<string, number> = {};
      for (const m of def.metrics) {
        const parts = (page?.series ?? []).filter(
          (s) => s.key.metric === m.metric
        );
        // The open bucket is past the window's end until it closes.
        series[m.metric] = combineSeries(parts, m.combine).filter(
          (b) => b.t >= fromMs && b.t < toMs && b.t + bucketMs <= readAtMs
        );
        holds[m.metric] = parts.reduce(
          (h, s) => Math.max(h, s.hold_ms),
          bucketMs
        );
      }
      return { def, series, holds };
    });
    const gaps = pages.flatMap((p: HistoryPage | undefined) => p?.gaps ?? []);
    return { laneData, gaps };
  }, [lanes, bucketMs, fromMs, toMs, dataStamp]);

  const failed = queries.find((q) => q.error);
  return {
    fromMs,
    toMs,
    bucketMs,
    tier,
    resolution: resolutionLabel(bucketMs),
    lanes: data.laneData,
    gaps: data.gaps,
    loading: queries.some((q) => q.isPending),
    error: (failed?.error as CommandFailure | undefined) ?? null,
  };
}

/** Value of `t`, settled for `ms` (cursor rest before a process query). */
function useDebounced<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(id);
  }, [value, ms]);
  return settled;
}

/** Slack after a commit is due before an answer counts as final. */
const COMMIT_MARGIN_MS = 30_000;

/**
 * The stored processes nearest the cursor's bucket (plan 4.6): asked after
 * 50 ms of cursor rest and cached per bucket. An answer fetched before the
 * bucket can have been committed (D-070) is not final: often `null`, because
 * the snapshots are still in the writer's open batch. It goes stale at once,
 * so the cursor's next visit asks again.
 */
export function useProcessesAt(
  bucketT: number | null,
  bucketMs: number
): ProcessesAt | null | undefined {
  const transport = useTransport();
  const hostId = useHostId();
  // Start and width settle together: a range switch changes both, and the
  // old start at the new width is a bucket nobody asked for.
  const bucket = bucketT === null ? null : `${bucketT}:${bucketMs}`;
  const settledBucket = useDebounced(bucket, 50);
  const [t, ms] = (settledBucket ?? "-1:0").split(":").map(Number) as [
    number,
    number,
  ];
  const { data } = useQuery({
    queryKey: historyKeys.processesAt(hostId, t, ms),
    queryFn: () => unwrap(transport.queryProcessesAt(hostId, t + ms / 2)),
    enabled: settledBucket !== null,
    staleTime: (query) =>
      query.state.dataUpdatedAt >= t + ms + HISTORY_COMMIT_MS + COMMIT_MARGIN_MS
        ? Number.POSITIVE_INFINITY
        : 0,
  });
  return settledBucket === bucket ? data : undefined;
}
