import type { CommandError, HistoryHealth } from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { CommandFailure, unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/**
 * The store's health (`history_health`, D-057, D-059), kept current by
 * `history-health-changed`: the event replaces the cached value, so no
 * polling. `error` is the command error when history is unavailable.
 */
export function useHistoryHealth(): {
  health: HistoryHealth | undefined;
  error: CommandError | null;
} {
  const transport = useTransport();
  const hostId = useHostId();
  const queryClient = useQueryClient();
  const queryKey = hostKeys.historyHealth(hostId);
  const { data, error } = useQuery({
    queryKey,
    queryFn: () => unwrap(transport.historyHealth(hostId)),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });

  useEffect(
    () =>
      transport.onHistoryHealthChanged((e) => {
        const key = hostKeys.historyHealth(hostId);
        // The event carries health, not availability. Over a cached
        // `history_unavailable` it would hide the banner, so ask the command
        // again instead: it clears the error only when history is back.
        if (queryClient.getQueryState(key)?.status === "error") {
          void queryClient.invalidateQueries({ queryKey: key });
          return;
        }
        queryClient.setQueryData(key, e.health);
      }),
    [transport, queryClient, hostId]
  );

  return {
    health: data,
    error: error instanceof CommandFailure ? error.error : null,
  };
}
