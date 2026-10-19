import type {
  CommandError,
  SpaceScanStatus,
  SpaceTarget,
} from "@core/generated/bindings";
import { diskSpaceKeys } from "@core/query-keys";
import { CommandFailure, unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { useTransport } from "~/lib/transport-context";

/** How often a running scan is read: progress, and the treemap at most this often. */
export const SPACE_POLL_MS = 1000;

export function useSpaceVolumes(enabled: boolean) {
  const transport = useTransport();
  return useQuery({
    queryKey: diskSpaceKeys.volumes,
    queryFn: () => unwrap(transport.diskSpaceVolumes()),
    enabled,
    staleTime: 30_000,
    retry: false,
  });
}

/** The running scan, or how the last one ended. Polled only while one runs. */
export function useSpaceStatus(enabled: boolean) {
  const transport = useTransport();
  return useQuery({
    queryKey: diskSpaceKeys.status,
    queryFn: () => transport.diskSpaceStatus(),
    enabled,
    refetchInterval: (q) =>
      q.state.data?.phase === "scanning" ? SPACE_POLL_MS : false,
    refetchOnWindowFocus: false,
  });
}

/** The finished report; refetched when a scan ends. */
export function useSpaceReport(enabled: boolean, settledScan: number | null) {
  const transport = useTransport();
  return useQuery({
    queryKey: [...diskSpaceKeys.report, settledScan],
    queryFn: () => transport.diskSpaceReport(),
    enabled,
    refetchOnWindowFocus: false,
  });
}

export function useSpaceSubtree(
  report: number | null,
  node: number | null,
  scanning: boolean
) {
  const transport = useTransport();
  return useQuery({
    queryKey: diskSpaceKeys.subtree(report ?? -1, node, scanning),
    queryFn: () => unwrap(transport.diskSpaceSubtree(report ?? -1, node)),
    enabled: report !== null,
    refetchInterval: scanning ? SPACE_POLL_MS : false,
    // Keep the previous folder's map on screen while the next one loads.
    placeholderData: (prev, prevQuery) =>
      prevQuery?.queryKey[2] === report ? prev : undefined,
    staleTime: scanning ? 0 : Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    retry: false,
  });
}

export function useSpaceLargest(report: number | null, enabled: boolean) {
  const transport = useTransport();
  return useQuery({
    queryKey: diskSpaceKeys.largest(report ?? -1),
    queryFn: () => unwrap(transport.diskSpaceLargestFiles(report ?? -1)),
    enabled: enabled && report !== null,
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    retry: false,
  });
}

const message = (error: CommandError | unknown) =>
  error instanceof CommandFailure && "message" in error.error
    ? error.error.message
    : String(error);

/** Start and cancel, with the error of the last start. */
export function useSpaceScan() {
  const transport = useTransport();
  const client = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const start = useCallback(
    async (volume: string, include: readonly string[]) => {
      setStarting(true);
      setError(null);
      try {
        const status = await unwrap(
          transport.diskSpaceStart(volume, [...include])
        );
        client.setQueryData<SpaceScanStatus>(diskSpaceKeys.status, status);
      } catch (e) {
        setError(message(e));
        await client.invalidateQueries({ queryKey: diskSpaceKeys.status });
      } finally {
        setStarting(false);
      }
    },
    [client, transport]
  );
  const cancel = useCallback(async () => {
    await transport.diskSpaceCancel();
    await client.invalidateQueries({ queryKey: diskSpaceKeys.status });
  }, [client, transport]);
  return { start, cancel, starting, error };
}

/** Reveal in Finder and Copy path; the last failure, or the path just copied. */
export function useSpaceItemActions(report: number | null) {
  const transport = useTransport();
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
        const path = await unwrap(transport.diskSpacePath(report, target));
        await navigator.clipboard.writeText(path);
        setCopied(path);
      } catch (e) {
        setError(message(e));
      }
    },
    [report, transport]
  );
  return { reveal, copyPath, error, copied };
}
