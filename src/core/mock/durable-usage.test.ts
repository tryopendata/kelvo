import type {
  HistoricalUsageRequest,
  UsageProcessRequest,
} from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { afterEach, describe, expect, it } from "vitest";
import type { MockTransport } from "./contracts";
import { MOCK_HOST_ID } from "./fixtures";

const NOW = 1_790_000_000_000;
const transports: MockTransport[] = [];
function transport(options: Parameters<typeof createMockTransport>[0] = {}) {
  const value = createMockTransport({ now: () => NOW, ...options });
  transports.push(value);
  return value;
}
function request(host = MOCK_HOST_ID): HistoricalUsageRequest {
  return {
    host,
    from_ms: NOW - 86_400_000,
    to_ms: NOW,
    by: "cpu",
    app_names: [],
  };
}
function processes(host = MOCK_HOST_ID): UsageProcessRequest {
  return {
    host,
    from_ms: NOW - 86_400_000,
    to_ms: NOW,
    by: "cpu",
    app_name: "Compiler",
    page_size: 100,
    cursor: null,
  };
}
afterEach(() => {
  for (const value of transports.splice(0)) value.dispose();
});
describe("durable usage transport", () => {
  it("queries an interval beyond the recent ring and preserves comparison membership", async () => {
    const source = transport();
    const result = await source.queryHistoricalUsage(request());
    expect(result.status).toBe("ok");
    if (result.status !== "ok") throw new Error("expected interval history");
    expect(result.data.range.requested_from_ms).toBe(NOW - 86_400_000);
    expect(result.data.apps.map((app) => app.name)).toEqual([
      "Compiler",
      "Browser",
      "Editor",
    ]);
    const comparison = await source.queryHistoricalUsage({
      ...request(),
      app_names: ["Editor", "Missing"],
    });
    expect(
      comparison.status === "ok" && comparison.data.apps.map((app) => app.name)
    ).toEqual(["Editor", "Missing"]);
  });
  it("returns every process through bounded pages without duplicate PID/start identities", async () => {
    const source = transport();
    const first = await source.queryUsageProcesses(processes());
    if (first.status !== "ok") throw new Error("expected first page");
    expect(first.data.processes).toHaveLength(100);
    expect(first.data.next_cursor).toBeTypeOf("string");
    const second = await source.queryUsageProcesses({
      ...processes(),
      cursor: first.data.next_cursor,
    });
    if (second.status !== "ok") throw new Error("expected continuation");
    expect(second.data.processes).toHaveLength(25);
    expect(second.data.next_cursor).toBeNull();
    expect(
      new Set(
        [...first.data.processes, ...second.data.processes].map(
          (p) => `${p.pid}:${p.start_time_us}`
        )
      ).size
    ).toBe(125);
    expect(first.data.network_app_level_only).toBe(true);
    expect(first.data.processes[0]?.figures.network_rx_bytes).toBeNull();
  });
  it("rejects a cursor used for a different interval and permits expansion restart", async () => {
    const source = transport();
    const page = await source.queryUsageProcesses(processes());
    if (page.status !== "ok") throw new Error("expected cursor");
    const moved = {
      ...processes(),
      to_ms: NOW - 1000,
      cursor: page.data.next_cursor,
    };
    expect(await source.queryUsageProcesses(moved)).toMatchObject({
      status: "error",
      error: { kind: "invalid_argument" },
    });
    expect(
      await source.queryUsageProcesses({ ...moved, cursor: null })
    ).toMatchObject({ status: "ok" });
  });
  it("rejects invalid interval, comparison membership and oversized process pages", async () => {
    const source = transport();
    for (const query of [
      { ...request(), to_ms: request().from_ms },
      { ...request(), app_names: ["Editor", "Editor"] },
      { ...request(), from_ms: NOW - 31 * 86_400_000 },
    ])
      expect(await source.queryHistoricalUsage(query)).toMatchObject({
        status: "error",
        error: { kind: "invalid_argument" },
      });
    expect(
      await source.queryUsageProcesses({ ...processes(), page_size: 101 })
    ).toMatchObject({ status: "error", error: { kind: "invalid_argument" } });
  });
  it("keeps missing evidence explicit rather than reporting complete zero use", async () => {
    const source = transport();
    const result = await source.queryHistoricalUsage({
      ...request(),
      to_ms: NOW + 60_000,
    });
    if (result.status !== "ok") throw new Error("expected coverage");
    expect(result.data.ranking_complete).toBe(false);
    expect(result.data.range.coverage[0]?.coverage).toMatchObject({
      complete: false,
      causes: ["not_recorded"],
    });
  });
  it("preserves pruned spans and unsupported resource coverage", async () => {
    const source = transport({
      scenarios: ["history-trimmed", "no-process-gpu", "no-process-network"],
    });
    const result = await source.queryHistoricalUsage({
      ...request(),
      from_ms: NOW - 30 * 86_400_000,
    });
    if (result.status !== "ok")
      throw new Error("expected retained observations");
    expect(
      result.data.range.spans.every(
        (span) => span.from_ms >= NOW - 20 * 86_400_000
      )
    ).toBe(true);
    expect(
      result.data.range.coverage.find((resource) => resource.metric === "cpu")
        ?.coverage
    ).toMatchObject({ complete: false, causes: ["pruned"] });
    expect(
      result.data.range.coverage.find((resource) => resource.metric === "gpu")
        ?.coverage
    ).toMatchObject({ supported: false, measured_ms: 0, complete: false });
    expect(result.data.apps[0]?.figures.network_rx_bytes).toBeNull();
  });
  it("routes capable remote queries to their own source and fails offline without cached attribution", async () => {
    const source = transport({ scenarios: ["multi-host"] });
    expect(
      await source.queryHistoricalUsage(request("remote-studio"))
    ).toMatchObject({ status: "ok" });
    const records = await source.listRemoteHosts();
    if (!records[0]) throw new Error("expected remote");
    await source.updateRemoteHost({ ...records[0].config, enabled: false });
    expect(
      await source.queryHistoricalUsage(request("remote-studio"))
    ).toMatchObject({ status: "error", error: { kind: "remote_unreachable" } });
    expect(
      await source.queryUsageProcesses(processes("remote-studio"))
    ).toMatchObject({ status: "error", error: { kind: "remote_unreachable" } });
  });
  it("reports unavailable durable history without substituting the recent live ring", async () => {
    const source = transport({ scenarios: ["history-unavailable"] });
    expect(await source.queryHistoricalUsage(request())).toMatchObject({
      status: "error",
      error: { kind: "history_unavailable" },
    });
    expect(await source.queryUsageProcesses(processes())).toMatchObject({
      status: "error",
      error: { kind: "history_unavailable" },
    });
  });
  it("reports an older connected peer as unsupported while local history still works", async () => {
    const source = transport({
      scenarios: ["multi-host"],
      remoteUsageHistorySupported: false,
    });
    expect(
      await source.queryHistoricalUsage(request("remote-studio"))
    ).toMatchObject({ status: "error", error: { kind: "remote_unsupported" } });
    expect(
      await source.queryUsageProcesses(processes("remote-studio"))
    ).toMatchObject({ status: "error", error: { kind: "remote_unsupported" } });
    expect(await source.queryHistoricalUsage(request())).toMatchObject({
      status: "ok",
    });
  });
});
