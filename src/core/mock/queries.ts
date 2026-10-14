import type { ExportOutcome, HistoryRequest } from "@core/generated/bindings";
import { HISTORY_TIERS, METRIC_MODULES } from "@core/generated/bindings";
import type { MockCommands } from "./commands";
import type { MockTransport, MockTransportOptions } from "./contracts";
import { mockDiskDevices, mockDiskIo } from "./disk-io";
import type { createMockDocker } from "./docker";
import { hostRecord, MOCK_HOST_ID } from "./fixtures";
import { mockHeatmap } from "./heatmap";
import {
  mockBatteryHours,
  mockGaps,
  mockHistory,
  mockRecentHistory,
  mockSeriesStats,
} from "./history";
import { NET_MAX_SPAN_MS } from "./net-apps";
import type { MockPower } from "./power";
import { err, ok, STORE } from "./protocol";
import type { MockRemotes } from "./remote";
import type { MockRing } from "./ring";
import type { MockStore } from "./store";
export class MockQueries {
  constructor(
    private readonly options: MockTransportOptions,
    private readonly ring: MockRing,
    private readonly power: MockPower,
    private readonly store: MockStore,
    private readonly remote: MockRemotes,
    private readonly commands: MockCommands,
    private readonly docker: ReturnType<typeof createMockDocker>
  ) {}
  readonly historyPage = (req: HistoryRequest) =>
    this.store.unavailable
      ? mockRecentHistory(
          req,
          this.ring.gen.specs,
          this.ring.now(),
          this.power.status.interval_ms
        )
      : mockHistory(
          req,
          this.ring.gen.specs,
          mockGaps(this.ring.flags, this.ring.startMs),
          this.ring.now(),
          this.power.status.interval_ms
        );
  readonly listDiskIoDevices: MockTransport["listDiskIoDevices"] = async (
    host
  ) => {
    if (this.remote.remotes.has(host)) {
      this.commands.record("list_disk_io_devices", host);
      return err({ kind: "remote_host", host });
    }
    return this.commands.hostCommand(
      "list_disk_io_devices",
      host,
      [host],
      {},
      () => ok(mockDiskDevices(this.options.scenarios ?? []))
    );
  };
  readonly queryDiskIo: MockTransport["queryDiskIo"] = async (request) => {
    const remote = this.remote.remotes.get(request.host);
    if (remote) {
      this.commands.record("query_disk_io", request);
      return remote.peer.queryDiskIo({ ...request, host: MOCK_HOST_ID });
    }
    return this.commands.hostCommand(
      "query_disk_io",
      request.host,
      [request],
      { range: [request.from_ms, request.to_ms] },
      () =>
        ok(
          mockDiskIo(
            request,
            this.options.scenarios ?? [],
            mockGaps(this.ring.flags, this.ring.startMs)
          )
        )
    );
  };
  readonly queryHistory: MockTransport["queryHistory"] = async (request) => {
    const remote = this.remote.remotes.get(request.host);
    if (remote)
      return remote.peer.queryHistory({ ...request, host: MOCK_HOST_ID });
    return this.commands.hostCommand(
      "query_history",
      request.host,
      [request],
      {},
      () => {
        if (request.selectors.length === 0 || request.to_ms < request.from_ms) {
          return err({
            kind: "invalid_argument",
            message: "empty selectors or negative span",
          });
        }
        // With history unavailable Rust answers from the engine's recent
        // rows (`HISTORY_RECENT_MS`). Gaps sit where the ring has its holes:
        // fixed at start, not sliding.
        return ok(this.historyPage(request));
      }
    );
  };
  readonly querySeriesStats: MockTransport["querySeriesStats"] = async (
    host,
    metrics,
    fromMs,
    toMs
  ) => {
    const remote = this.remote.remotes.get(host);
    if (remote)
      return remote.peer.querySeriesStats(MOCK_HOST_ID, metrics, fromMs, toMs);
    const args = [host, metrics, fromMs, toMs];
    const checks = {
      range: [fromMs, toMs],
      maxSpanMs: NET_MAX_SPAN_MS,
    } as const;
    return this.commands.hostCommand(
      "query_series_stats",
      host,
      args,
      checks,
      () => {
        // As Rust: series stats take unlabelled catalog metrics only.
        for (const m of metrics) {
          if (!(m in METRIC_MODULES)) {
            return err({
              kind: "invalid_argument",
              message: `${m} is not in the catalog`,
            });
          }
          if (
            this.ring.gen.keys.some(
              (k) => k.metric === m && k.labels.length > 0
            )
          ) {
            return err({
              kind: "invalid_argument",
              message: `${m} has labels; series stats take unlabelled metrics`,
            });
          }
        }
        return ok(
          mockSeriesStats(
            host,
            metrics,
            fromMs,
            toMs,
            this.ring.now(),
            this.historyPage
          )
        );
      }
    );
  };
  readonly queryEvents: MockTransport["queryEvents"] = async (
    host,
    fromMs,
    toMs
  ) => {
    const remote = this.remote.remotes.get(host);
    if (remote) return remote.peer.queryEvents(MOCK_HOST_ID, fromMs, toMs);
    const args = [host, fromMs, toMs];
    return this.commands.hostCommand("query_events", host, args, STORE, () =>
      ok(this.store.events.filter((e) => e.ts_ms >= fromMs && e.ts_ms < toMs))
    );
  };
  readonly queryHeatmap: MockTransport["queryHeatmap"] = async (request) => {
    const remote = this.remote.remotes.get(request.host);
    if (remote)
      return remote.peer.queryHeatmap({ ...request, host: MOCK_HOST_ID });
    return this.commands.hostCommand(
      "query_heatmap",
      request.host,
      [request],
      STORE,
      () =>
        request.days.some((d) => d.hour_starts.length !== 25)
          ? err({
              kind: "invalid_argument",
              message: "every day needs 25 hour boundaries",
            })
          : ok(mockHeatmap(request, this.ring.now()))
    );
  };
  readonly batteryHours: MockTransport["batteryHours"] = async (
    host,
    hourStartsMs
  ) => {
    const remote = this.remote.remotes.get(host);
    if (remote) return remote.peer.batteryHours(MOCK_HOST_ID, hourStartsMs);
    const args = [host, hourStartsMs];
    return this.commands.hostCommand("battery_hours", host, args, {}, () => {
      const hours = mockBatteryHours(host, hourStartsMs, this.historyPage);
      return typeof hours === "string"
        ? err({ kind: "invalid_argument", message: hours })
        : ok(hours);
    });
  };
  readonly exportCsv: MockTransport["exportCsv"] = async (request) => {
    return this.commands.hostCommand<ExportOutcome>(
      "export_csv",
      request.host,
      [request],
      STORE,
      () => {
        if (
          request.selectors.length === 0 ||
          request.to_ms <= request.from_ms
        ) {
          return err({
            kind: "invalid_argument",
            message: "empty selectors or empty range",
          });
        }
        if (this.options.exportCancels) return ok({ kind: "cancelled" });
        // One row per bucket of the tier `auto` would read (D-076), gaps apart.
        const span = request.to_ms - request.from_ms;
        const tierMs = exportTier(request.tier, span);
        const gapRows = mockGaps(this.ring.flags, this.ring.startMs).filter(
          (g) =>
            g.start_ms < request.to_ms &&
            (g.end_ms ?? Infinity) > request.from_ms
        ).length;
        const rows = Math.ceil(span / tierMs);
        return ok({
          kind: "saved",
          path: `/Users/mock/Downloads/${request.file_name ?? "kelvo-history.csv"}`,
          rows,
          gap_rows: gapRows,
          bytes: 64 + rows * 24 * (1 + request.selectors.length * 3),
        });
      }
    );
  };
  readonly historySize: MockTransport["historySize"] = async (host) => {
    const remote = this.remote.remotes.get(host);
    if (remote) return remote.peer.historySize(MOCK_HOST_ID);
    return this.commands.hostCommand("history_size", host, [host], STORE, () =>
      ok(this.store.historyBytes)
    );
  };
  readonly historyGrowth: MockTransport["historyGrowth"] = async (host) => {
    const remote = this.remote.remotes.get(host);
    if (remote) return remote.peer.historyGrowth(MOCK_HOST_ID);
    return this.commands.hostCommand(
      "history_growth",
      host,
      [host],
      STORE,
      () => ok(this.store.growth)
    );
  };
  readonly clearHistory: MockTransport["clearHistory"] = async (host) => {
    return this.commands.hostCommand(
      "clear_history",
      host,
      [host],
      STORE,
      () => {
        this.store.historyBytes = 0;
        this.store.growth = null;
        if (this.store.health.trimmed_before_ms !== null) {
          this.store.setHistoryHealth({
            ...this.store.health,
            trimmed_before_ms: null,
            trimmed_limit_bytes: null,
            cap_met: true,
          });
        }
        return ok(this.store.historyBytes);
      }
    );
  };
  readonly historyHealth: MockTransport["historyHealth"] = async (host) => {
    const remote = this.remote.remotes.get(host);
    if (remote) return remote.peer.historyHealth(MOCK_HOST_ID);
    return this.commands.hostCommand(
      "history_health",
      host,
      [host],
      STORE,
      () => ok(this.store.health)
    );
  };
  readonly resetHistory: MockTransport["resetHistory"] = async () => {
    this.commands.record("reset_history");
    // Another Kelvo process holds the lock: a reset cannot take it.
    if (
      this.store.unavailable?.kind === "history_unavailable" &&
      this.store.unavailable.reason?.kind === "locked"
    ) {
      return err({
        kind: "store_busy",
        message: "history.sqlite is locked by another Kelvo process",
      });
    }
    this.store.unavailable = null;
    this.docker.resetHistory();
    this.store.historyBytes = 4096;
    this.store.growth = null;
    this.store.setHistoryHealth({
      low_disk_paused: this.store.health.low_disk_paused,
      trimmed_before_ms: null,
      trimmed_limit_bytes: null,
      cap_met: true,
    });
    return ok(this.store.historyBytes);
  };
  readonly sensorDump: MockTransport["sensorDump"] = async (host) => {
    return this.commands.hostCommand("sensor_dump", host, [host], {}, () => {
      const info = hostRecord(this.ring.flags, this.ring.startMs).info;
      const last = this.ring.rows[this.ring.rows.length - 1]?.values ?? [];
      return ok({
        model: info.model,
        chip: info.chip,
        os_version: info.os_version,
        chip_known: info.chip_known,
        capabilities: this.ring.caps,
        sensors: this.ring.gen.specs
          .map((s, i) => ({ s, value: last[i] ?? null }))
          .filter(({ s }) => s.module === "power" || s.module === "sensors")
          .map(({ s, value }) => ({ key: s.key, value })),
      });
    });
  };

  get methods() {
    return {
      listDiskIoDevices: this.listDiskIoDevices,
      queryDiskIo: this.queryDiskIo,
      queryHistory: this.queryHistory,
      querySeriesStats: this.querySeriesStats,
      queryEvents: this.queryEvents,
      queryHeatmap: this.queryHeatmap,
      batteryHours: this.batteryHours,
      exportCsv: this.exportCsv,
      historySize: this.historySize,
      historyGrowth: this.historyGrowth,
      clearHistory: this.clearHistory,
      historyHealth: this.historyHealth,
      resetHistory: this.resetHistory,
      sensorDump: this.sensorDump,
    };
  }
}

function exportTier(
  tier: Parameters<MockTransport["exportCsv"]>[0]["tier"],
  span: number
): number {
  const [s10, m1, m15] = HISTORY_TIERS;
  if (tier === "s10") return s10.bucket_ms;
  if (tier === "m1") return m1.bucket_ms;
  if (span <= s10.kept_ms) return s10.bucket_ms;
  if (span <= m1.kept_ms) return m1.bucket_ms;
  return m15.bucket_ms;
}
