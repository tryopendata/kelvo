import type {
  CommandError,
  HistoricalUsageRequest,
  RemoteHostRecord,
  UsageProcessRequest,
} from "@core/generated/bindings";
import type { MockCommands } from "./commands";
import type { MockTransport, MockTransportOptions } from "./contracts";
import { validateUsageRequest } from "./durable-usage";
import { MOCK_HOST_ID } from "./fixtures";
import { err, ok } from "./protocol";

export type UsageQueries = Pick<
  MockTransport,
  "queryUsageByApp" | "queryHistoricalUsage" | "queryUsageProcesses"
>;

function refusal(
  request: HistoricalUsageRequest | UsageProcessRequest,
  record: RemoteHostRecord,
  supported: boolean | undefined
): CommandError | null {
  const bad = validateUsageRequest(request);
  if (bad) return { kind: "invalid_argument", message: bad };
  if (record.status.state !== "connected")
    return { kind: "remote_unreachable", message: "the host is not connected" };
  if (supported === false)
    return {
      kind: "remote_unsupported",
      message:
        "This host does not support usage history; update Kelvo on that host",
    };
  return null;
}

/** Durable attribution goes to the sampled peer, without an offline mirror fallback. */
export function remoteUsage<T extends UsageQueries>(
  remotes: ReadonlyMap<
    string,
    { record: RemoteHostRecord; peer: MockTransport }
  >,
  commands: MockCommands,
  options: Pick<MockTransportOptions, "remoteUsageHistorySupported">,
  local: T
): T {
  return {
    ...local,
    queryHistoricalUsage: async (request) => {
      const remote = remotes.get(request.host);
      if (!remote) return local.queryHistoricalUsage(request);
      commands.record("query_historical_usage", request);
      const error = refusal(
        request,
        remote.record,
        options.remoteUsageHistorySupported
      );
      if (error) return err(error);
      return remote.peer.queryHistoricalUsage({
        ...request,
        host: MOCK_HOST_ID,
      });
    },
    queryUsageProcesses: async (request) => {
      const remote = remotes.get(request.host);
      if (!remote) return local.queryUsageProcesses(request);
      commands.record("query_usage_processes", request);
      const error = refusal(
        request,
        remote.record,
        options.remoteUsageHistorySupported
      );
      if (error) return err(error);
      return remote.peer.queryUsageProcesses({
        ...request,
        host: MOCK_HOST_ID,
      });
    },
    queryUsageByApp: async (host, fromMs, toMs, by, limit) => {
      const remote = remotes.get(host);
      if (!remote) return local.queryUsageByApp(host, fromMs, toMs, by, limit);
      commands.record("query_usage_by_app", host, fromMs, toMs, by, limit);
      if (remote.record.status.state !== "connected")
        return err({
          kind: "remote_unreachable",
          message: "the host is not connected",
        });
      const r = await remote.peer.queryUsageByApp(
        MOCK_HOST_ID,
        fromMs,
        toMs,
        by,
        limit
      );
      if (r.status === "error") return r;
      return ok({
        ...r.data,
        apps: r.data.apps.map((app) => ({
          ...app,
          quit_pid: null,
          processes: app.processes.map((p) => ({ ...p, refusal: null })),
        })),
      });
    },
  };
}
