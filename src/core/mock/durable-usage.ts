/** Deterministic source-history fixture, separate from the recent usage ring. */

import type {
  HistoricalUsageFigures,
  HistoricalUsageMetric,
  HistoricalUsageRequest,
  UsageHistoryRange,
  UsageProcessRequest,
} from "@core/generated/bindings";
import {
  USAGE_BUCKET_MS,
  USAGE_HISTORY_MAX_RANGE_MS,
  USAGE_PROCESS_PAGE_MAX,
} from "@core/generated/bindings";
import type { MockCommands } from "./commands";
import type { MockTransport } from "./contracts";
import { mockGaps } from "./history";
import { err, ok } from "./protocol";
import type { MockRing } from "./ring";
import type { MockStore } from "./store";

const METRICS: HistoricalUsageMetric[] = [
  "cpu",
  "gpu",
  "memory",
  "energy",
  "network",
];
const APPS = ["Compiler", "Browser", "Editor"];
const MAX_RANGE = USAGE_HISTORY_MAX_RANGE_MS;
function invalid(message: string) {
  return err({ kind: "invalid_argument", message });
}
function validName(name: string) {
  return (
    name.length > 0 &&
    new TextEncoder().encode(name).length <= 1024 &&
    !name.includes("\0")
  );
}
export function validateUsageRequest(
  request: HistoricalUsageRequest | UsageProcessRequest
) {
  if (
    !Number.isSafeInteger(request.from_ms) ||
    !Number.isSafeInteger(request.to_ms) ||
    request.to_ms <= request.from_ms ||
    request.to_ms - request.from_ms > MAX_RANGE
  )
    return "usage interval must be nonempty, JS-safe and at most thirty days";
  if ("app_names" in request) {
    if (
      request.app_names.length > 10 ||
      request.app_names.some((name) => !validName(name)) ||
      new Set(request.app_names).size !== request.app_names.length
    )
      return "comparison membership must contain at most ten distinct valid app identities";
  } else if (
    !validName(request.app_name) ||
    !Number.isInteger(request.page_size) ||
    request.page_size < 1 ||
    request.page_size > USAGE_PROCESS_PAGE_MAX ||
    (request.cursor !== null &&
      (request.cursor.length === 0 ||
        new TextEncoder().encode(request.cursor).length > 4096))
  ) {
    return "process expansion needs a valid identity, bounded page and continuation";
  }
  return null;
}
export class MockDurableUsage {
  private serial = 0;
  private readonly cursors = new Map<string, { key: string; offset: number }>();
  constructor(
    private readonly ring: MockRing,
    private readonly commands: MockCommands,
    private readonly store: MockStore
  ) {}
  private range(
    request: HistoricalUsageRequest | UsageProcessRequest
  ): UsageHistoryRange {
    const from = Math.max(
      request.from_ms,
      this.store.health.trimmed_before_ms ?? this.ring.startMs - MAX_RANGE
    );
    const to = Math.min(request.to_ms, this.ring.now());
    let spans =
      to > from
        ? [{ from_ms: from, to_ms: to, resolution_ms: USAGE_BUCKET_MS }]
        : [];
    for (const gap of mockGaps(this.ring.flags, this.ring.startMs)) {
      const end = gap.end_ms ?? to;
      spans = spans.flatMap((span) => {
        if (gap.start_ms >= span.to_ms || end <= span.from_ms) return [span];
        return [
          { ...span, to_ms: Math.min(span.to_ms, gap.start_ms) },
          { ...span, from_ms: Math.max(span.from_ms, end) },
        ].filter((part) => part.to_ms > part.from_ms);
      });
    }
    const measured = spans.reduce(
      (sum, span) => sum + span.to_ms - span.from_ms,
      0
    );
    const complete = measured === request.to_ms - request.from_ms;
    return {
      requested_from_ms: request.from_ms,
      requested_to_ms: request.to_ms,
      spans,
      coverage: METRICS.map((metric) => ({
        metric,
        coverage: {
          supported: this.supported(metric),
          measured_ms: this.supported(metric) ? measured : 0,
          complete: complete && this.supported(metric),
          causes: complete
            ? []
            : [
                from > request.from_ms &&
                this.store.health.trimmed_before_ms !== null
                  ? "pruned"
                  : from > request.from_ms || to < request.to_ms
                    ? "not_recorded"
                    : "sampling_gap",
              ],
        },
      })),
    };
  }
  private supported(metric: HistoricalUsageMetric): boolean {
    if (metric === "gpu") return this.ring.caps.process_gpu === true;
    if (metric === "network") return this.ring.caps.process_network === true;
    if (metric === "energy") return this.ring.caps.process_energy === true;
    return true;
  }
  private figures(
    range: UsageHistoryRange,
    weight: number
  ): HistoricalUsageFigures {
    const ms = range.spans.reduce(
      (sum, span) => sum + span.to_ms - span.from_ms,
      0
    );
    const measured = ms > 0;
    return {
      cpu_avg_pct: measured ? weight : null,
      gpu_avg_pct: measured && this.supported("gpu") ? weight / 4 : null,
      mem_peak_bytes: measured ? Math.round(weight * 16_777_216) : null,
      mem_avg_bytes: measured ? Math.round(weight * 8_388_608) : null,
      energy_j:
        measured && this.supported("energy") ? (weight * ms) / 100_000 : null,
      cpu_power_w: measured && this.supported("energy") ? weight / 100 : null,
      network_rx_bytes:
        measured && this.supported("network") ? Math.floor(weight * ms) : null,
      network_tx_bytes:
        measured && this.supported("network")
          ? Math.floor((weight * ms) / 10)
          : null,
      coverage: range.coverage,
    };
  }
  private absentFigures(range: UsageHistoryRange): HistoricalUsageFigures {
    const zero = (metric: HistoricalUsageMetric) =>
      range.coverage.find((resource) => resource.metric === metric)?.coverage
        .complete
        ? 0
        : null;
    return {
      cpu_avg_pct: zero("cpu"),
      gpu_avg_pct: zero("gpu"),
      mem_peak_bytes: zero("memory"),
      mem_avg_bytes: zero("memory"),
      energy_j: zero("energy"),
      cpu_power_w: zero("energy"),
      network_rx_bytes: zero("network"),
      network_tx_bytes: zero("network"),
      coverage: range.coverage,
    };
  }
  readonly queryHistoricalUsage: MockTransport["queryHistoricalUsage"] = async (
    request
  ) => {
    const bad = validateUsageRequest(request);
    if (bad) {
      this.commands.record("query_historical_usage", request);
      return invalid(bad);
    }
    return this.commands.hostCommand(
      "query_historical_usage",
      request.host,
      [request],
      { needsHistory: true },
      () => {
        const range = this.range(request);
        const names = request.app_names.length
          ? request.app_names
          : range.spans.length > 0
            ? APPS
            : [];
        return ok({
          range,
          by: request.by,
          apps: names.map((name) => ({
            name,
            figures: APPS.includes(name)
              ? this.figures(range, (3 - APPS.indexOf(name)) * 10)
              : this.absentFigures(range),
            process_details_available:
              APPS.includes(name) && range.spans.length > 0,
          })),
          ranking_complete:
            range.coverage.find((c) => c.metric === request.by)?.coverage
              .complete ?? false,
          network_uncertainty: [],
        });
      }
    );
  };
  readonly queryUsageProcesses: MockTransport["queryUsageProcesses"] = async (
    request
  ) => {
    const bad = validateUsageRequest(request);
    if (bad) {
      this.commands.record("query_usage_processes", request);
      return invalid(bad);
    }
    return this.commands.hostCommand(
      "query_usage_processes",
      request.host,
      [request],
      { needsHistory: true },
      () => {
        const key = JSON.stringify([
          request.host,
          request.app_name,
          request.from_ms,
          request.to_ms,
          request.by,
          Math.min(
            request.to_ms,
            Math.floor(this.ring.now() / USAGE_BUCKET_MS) * USAGE_BUCKET_MS
          ),
        ]);
        const cursor =
          request.cursor === null ? null : this.cursors.get(request.cursor);
        if (request.cursor !== null && (!cursor || cursor.key !== key))
          return invalid("process history changed; restart expansion");
        const range = this.range(request);
        const count =
          range.spans.length === 0 || !APPS.includes(request.app_name)
            ? 0
            : request.app_name === "Compiler"
              ? 125
              : 2;
        const offset = cursor?.offset ?? 0;
        const end = Math.min(count, offset + request.page_size);
        const next = this.nextCursor(key, end, count);
        return ok({
          range,
          app_name: request.app_name,
          processes: Array.from(
            { length: Math.max(0, end - offset) },
            (_, i) => ({
              pid: 4000 + offset + i,
              start_time_us: (this.ring.startMs - 60_000) * 1000 + offset + i,
              name: request.app_name,
              figures: {
                ...this.figures(
                  range,
                  ((3 - APPS.indexOf(request.app_name)) *
                    10 *
                    (count - offset - i)) /
                    ((count * (count + 1)) / 2)
                ),
                network_rx_bytes: null,
                network_tx_bytes: null,
                coverage: range.coverage.map((resource) =>
                  resource.metric === "network"
                    ? {
                        metric: resource.metric,
                        coverage: {
                          supported: false,
                          measured_ms: 0,
                          complete: false,
                          causes: ["not_recorded"],
                        },
                      }
                    : resource
                ),
              },
            })
          ),
          network_app_level_only: true,
          next_cursor: next,
        });
      }
    );
  };
  private nextCursor(
    key: string,
    offset: number,
    count: number
  ): string | null {
    if (offset >= count) return null;
    const next = `usage-snapshot-${++this.serial}`;
    this.cursors.set(next, { key, offset });
    if (this.cursors.size > 100)
      this.cursors.delete(this.cursors.keys().next().value ?? "");
    return next;
  }
  get methods() {
    return {
      queryHistoricalUsage: this.queryHistoricalUsage,
      queryUsageProcesses: this.queryUsageProcesses,
    };
  }
}
