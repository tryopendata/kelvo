import type { HostId, UsageKey } from "@core/generated/bindings";
import { USAGE_KEEP_MS } from "@core/generated/bindings";
import type { MockCommands } from "./commands";
import type { MockTransport } from "./contracts";
import { TOPOLOGY, withGpuPct } from "./fixtures";
import { mockSeriesStats } from "./history";
import {
  appsReportedTo,
  mockNetworkByApp,
  NET_COLLECTION_DELAY_MS,
  NET_MAX_SPAN_MS,
} from "./net-apps";
import type { MockPower } from "./power";
import type { MockProcesses } from "./processes";
import {
  coverRates,
  MOCK_LOCAL_IPV4,
  MOCK_PUBLIC_IP,
  MOCK_USER,
  ok,
} from "./protocol";
import type { MockQueries } from "./queries";
import type { MockRing } from "./ring";
import type { MockSettings } from "./settings";
import type { MockStore } from "./store";
import { mockUsageByApp } from "./usage";
import { mockUsageSeries } from "./usage-series";
export class MockUsageQueries {
  constructor(
    private readonly ring: MockRing,
    private readonly power: MockPower,
    private readonly store: MockStore,
    private readonly settings: MockSettings,
    private readonly processes: MockProcesses,
    private readonly queries: MockQueries,
    private readonly commands: MockCommands
  ) {}
  readonly queryNetworkByApp: MockTransport["queryNetworkByApp"] = async (
    host,
    fromMs,
    toMs
  ) => {
    // With history unavailable Rust answers from the engine's ring alone,
    // which is what the mock's rows are, so `unavailable` changes nothing.
    const checks = {
      range: [fromMs, toMs],
      maxSpanMs: NET_MAX_SPAN_MS,
    } as const;
    const args = [host, fromMs, toMs];
    return this.commands.hostCommand(
      "query_network_by_app",
      host,
      args,
      checks,
      () => {
        const lastTs = this.ring.rows[this.ring.rows.length - 1]?.ts;
        const historyOn =
          this.store.netHistoryOff[this.store.netHistoryOff.length - 1]?.to !==
          null;
        return ok(
          mockNetworkByApp({
            // The engine's open buckets: none with history off or no
            // NetworkStatistics, else from the per-app stream's last report.
            appsToMs:
              this.ring.caps.process_network &&
              historyOn &&
              lastTs !== undefined
                ? appsReportedTo(lastTs)
                : null,
            rows: coverRates(this.ring.rows, this.ring.rxIdx, this.ring.txIdx),
            intervalMs: this.power.status.interval_ms,
            startMs: this.ring.startMs,
            // No NetworkStatistics: nothing is ever recorded (appstore).
            collectingFromMs: this.ring.caps.process_network
              ? this.ring.ringStartMs + NET_COLLECTION_DELAY_MS
              : Number.POSITIVE_INFINITY,
            offSpans: this.store.netHistoryOff,
            fromMs,
            toMs,
          })
        );
      }
    );
  };
  /** The by-app answer and the span it was counted over, shared by both usage queries. */
  private usageAnswer(
    host: HostId,
    fromMs: number,
    toMs: number,
    by: UsageKey,
    limit: number
  ) {
    const latestMs =
      this.ring.rows[this.ring.rows.length - 1]?.ts ?? this.ring.startMs;
    // The engine keeps an hour; the mock's ring is its whole run.
    const sinceMs = Math.max(this.ring.ringStartMs, latestMs - 3_600_000);
    // GPU per process is measured always, except in Performance mode, where
    // only a view asking for it measures it (D-099). The mock has no record
    // of past interest: a view asking now stands for the whole range.
    const sampling = this.settings.snapshot.settings.sampling;
    const performance = !!sampling.performance_mode || this.power.lowPowerMode;
    const gpu =
      this.ring.caps.process_gpu === true &&
      (!performance || this.processes.interest?.view.gpu === true);
    const from = Math.max(fromMs, sinceMs);
    // Cut at the newest process sample, where the apps' sums stop.
    const stats =
      from < toMs
        ? mockSeriesStats(
            host,
            ["cpu.total", "gpu.util", "disk.read_total", "disk.write_total"],
            from,
            toMs,
            Math.min(this.ring.now(), latestMs),
            this.queries.historyPage
          )
        : null;
    const byApp = mockUsageByApp({
      processes: gpu
        ? withGpuPct(this.processes.processRows())
        : this.processes.processRows(),
      user: MOCK_USER,
      fromMs,
      toMs,
      sinceMs,
      latestMs,
      by,
      limit,
      gpu,
      stats,
      cores: TOPOLOGY.reduce((n, c) => n + c.cores.length, 0),
    });
    return { byApp, sinceMs, latestMs };
  }
  readonly queryUsageByApp: MockTransport["queryUsageByApp"] = async (
    host,
    fromMs,
    toMs,
    by,
    limit
  ) => {
    const args = [host, fromMs, toMs, by, limit];
    const checks = { range: [fromMs, toMs] } as const;
    return this.commands.hostCommand(
      "query_usage_by_app",
      host,
      args,
      checks,
      () => ok(this.usageAnswer(host, fromMs, toMs, by, limit).byApp)
    );
  };
  readonly queryUsageSeries: MockTransport["queryUsageSeries"] = async (
    host,
    fromMs,
    toMs,
    by,
    limit
  ) => {
    const args = [host, fromMs, toMs, by, limit];
    const checks = { range: [fromMs, toMs], maxSpanMs: USAGE_KEEP_MS } as const;
    return this.commands.hostCommand(
      "query_usage_series",
      host,
      args,
      checks,
      () =>
        ok(
          mockUsageSeries({
            ...this.usageAnswer(host, fromMs, toMs, by, limit),
            by,
          })
        )
    );
  };
  readonly getNetworkAddresses: MockTransport["getNetworkAddresses"] = async (
    host
  ) => {
    return this.commands.hostCommand(
      "get_network_addresses",
      host,
      [host],
      {},
      () =>
        ok({
          iface: this.power.status.primary_iface,
          ipv4:
            this.power.status.primary_iface === null ? [] : [MOCK_LOCAL_IPV4],
          ipv6: [],
          egress: this.power.status.primary_iface,
        })
    );
  };
  readonly getPublicIp: MockTransport["getPublicIp"] = async () => {
    this.commands.record("get_public_ip");
    return ok(MOCK_PUBLIC_IP);
  };

  get methods() {
    return {
      queryNetworkByApp: this.queryNetworkByApp,
      queryUsageByApp: this.queryUsageByApp,
      queryUsageSeries: this.queryUsageSeries,
      getNetworkAddresses: this.getNetworkAddresses,
      getPublicIp: this.getPublicIp,
    };
  }
}
