import type {
  CommandError,
  RemoteHostRecord,
  SpaceScanStatus,
  SpaceTarget,
} from "@core/generated/bindings";
import { diskSpaceKeys } from "@core/query-keys";
import { CommandFailure, unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { useRemoteHosts } from "~/hooks/use-remote-hosts";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/** How often a running scan is read: progress, and the treemap at most this often. */
export const SPACE_POLL_MS = 1000;

export function useSpaceVolumes(enabled: boolean) {
  const transport = useTransport();
  const host = useHostId();
  return useQuery({
    queryKey: diskSpaceKeys.volumes(host),
    queryFn: () => unwrap(transport.diskSpaceVolumes(host)),
    enabled,
    staleTime: 30_000,
    retry: false,
  });
}

/** The running scan, or how the last one ended. Polled only while one runs. */
export function useSpaceStatus(enabled: boolean) {
  const transport = useTransport();
  const host = useHostId();
  return useQuery({
    queryKey: diskSpaceKeys.status(host),
    queryFn: () => unwrap(transport.diskSpaceStatus(host)),
    enabled,
    refetchInterval: (q) =>
      q.state.data?.phase === "scanning" ? SPACE_POLL_MS : false,
    refetchOnWindowFocus: false,
    retry: false,
  });
}

/** The finished report; refetched when a scan ends. */
export function useSpaceReport(enabled: boolean, settledScan: number | null) {
  const transport = useTransport();
  const host = useHostId();
  return useQuery({
    queryKey: diskSpaceKeys.report(host, settledScan),
    queryFn: () => unwrap(transport.diskSpaceReport(host)),
    enabled,
    refetchOnWindowFocus: false,
    retry: false,
  });
}

/** Each remote host's name and the connection its session runs over. */
function selectLinks(hosts: RemoteHostRecord[]) {
  return hosts.map((h) => ({
    id: h.config.id,
    name: h.config.name,
    link: `${h.status.state}:${h.status.route}`,
  }));
}

/**
 * A remote host drops the scan and report of a session that ends (D-117): a
 * reconnect, a sleep, a move from Tailscale to the LAN. When this host's
 * connection changes its space queries start over, and when a report the
 * view held is gone, this says why. `null` on this Mac.
 */
export function useSpaceDropped(
  status: SpaceScanStatus | undefined
): string | null {
  const host = useHostId();
  const client = useQueryClient();
  const remote = useRemoteHosts(selectLinks).data?.find((h) => h.id === host);
  const link = remote?.link ?? null;
  const seen = useRef(link);
  useEffect(() => {
    const was = seen.current;
    seen.current = link;
    if (was !== null && was !== link)
      void client.resetQueries({ queryKey: diskSpaceKeys.host(host) });
  }, [client, host, link]);
  const held = useRef<number | null>(null);
  const [dropped, setDropped] = useState(false);
  useEffect(() => {
    if (!status) return;
    if (status.report_id !== null) {
      held.current = status.report_id;
      setDropped(false);
    } else if (held.current !== null && status.phase === "idle") {
      held.current = null;
      setDropped(true);
    }
  }, [status]);
  return remote && dropped
    ? `The report was dropped when the connection to ${remote.name} ended.`
    : null;
}

/**
 * A report read that fails may mean the host dropped the report: reading the
 * status again shows whether it did.
 */
function useRecheckOnError(failed: boolean) {
  const host = useHostId();
  const client = useQueryClient();
  useEffect(() => {
    if (failed)
      void client.invalidateQueries({ queryKey: diskSpaceKeys.status(host) });
  }, [client, failed, host]);
}

export function useSpaceSubtree(
  report: number | null,
  node: number | null,
  scanning: boolean
) {
  const transport = useTransport();
  const host = useHostId();
  const query = useQuery({
    queryKey: diskSpaceKeys.subtree(host, report ?? -1, node, scanning),
    queryFn: () => unwrap(transport.diskSpaceSubtree(host, report ?? -1, node)),
    enabled: report !== null,
    refetchInterval: scanning ? SPACE_POLL_MS : false,
    // Keep the previous folder's map on screen while the next one loads.
    placeholderData: (prev, prevQuery) =>
      prevQuery?.queryKey[1] === host && prevQuery.queryKey[3] === report
        ? prev
        : undefined,
    staleTime: scanning ? 0 : Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    retry: false,
  });
  useRecheckOnError(query.isError);
  return query;
}

export function useSpaceLargest(report: number | null, enabled: boolean) {
  const transport = useTransport();
  const host = useHostId();
  const query = useQuery({
    queryKey: diskSpaceKeys.largest(host, report ?? -1),
    queryFn: () => unwrap(transport.diskSpaceLargestFiles(host, report ?? -1)),
    enabled: enabled && report !== null,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    retry: false,
  });
  useRecheckOnError(query.isError);
  return query;
}

const message = (error: CommandError | unknown) =>
  error instanceof CommandFailure && "message" in error.error
    ? error.error.message
    : String(error);

/** Start and cancel on the host shown, with the error of the last start. */
export function useSpaceScan() {
  const transport = useTransport();
  const host = useHostId();
  const client = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const start = useCallback(
    async (volume: string, include: readonly string[]) => {
      setStarting(true);
      setError(null);
      try {
        const status = await unwrap(
          transport.diskSpaceStart(host, volume, [...include])
        );
        client.setQueryData<SpaceScanStatus>(
          diskSpaceKeys.status(host),
          status
        );
      } catch (e) {
        setError(message(e));
        await client.invalidateQueries({
          queryKey: diskSpaceKeys.status(host),
        });
      } finally {
        setStarting(false);
      }
    },
    [client, host, transport]
  );
  const cancel = useCallback(async () => {
    const r = await transport.diskSpaceCancel(host);
    if (r.status === "error") setError(message(new CommandFailure(r.error)));
    await client.invalidateQueries({ queryKey: diskSpaceKeys.status(host) });
  }, [client, host, transport]);
  return { start, cancel, starting, error };
}

/**
 * Reveal in Finder (this Mac only: `null` on a remote host) and Copy path,
 * which copies the path as the host shows it; the last failure, or the path
 * just copied.
 */
export function useSpaceItemActions(report: number | null, local: boolean) {
  const transport = useTransport();
  const host = useHostId();
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const reveal = useCallback(
    async (target: SpaceTarget) => {
      if (report === null) return;
      setError(null);
      setCopied(null);
      const r = await transport.diskSpaceReveal(report, target);
      if (r.status === "error") setError(message(new CommandFailure(r.error)));
    },
    [report, transport]
  );
  const copyPath = useCallback(
    async (target: SpaceTarget) => {
      if (report === null) return;
      setError(null);
      setCopied(null);
      try {
        const path = await unwrap(
          transport.diskSpacePath(host, report, target)
        );
        await navigator.clipboard.writeText(path);
        setCopied(path);
      } catch (e) {
        setError(message(e));
      }
    },
    [host, report, transport]
  );
  return { reveal: local ? reveal : null, copyPath, error, copied };
}
