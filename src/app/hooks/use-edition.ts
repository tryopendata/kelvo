import type { Edition } from "@core/generated/bindings";
import { appKeys } from "@core/query-keys";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { useTransport } from "~/lib/transport-context";

/**
 * What this build can do (`get_edition`, D-065). It never changes while the
 * app runs, so it is fetched once. `null` until it answers: callers hide
 * an optional action until they know it is there, rather than show it and
 * take it away.
 */
export function useEdition(): Edition | null {
  const transport = useTransport();
  const { data } = useQuery({
    queryKey: appKeys.edition,
    queryFn: () => transport.getEdition(),
    staleTime: Number.POSITIVE_INFINITY,
  });
  return data ?? null;
}

/**
 * Records that `process_signal` is not in this build: the command answered
 * `unavailable`, so whatever `get_edition` said, the actions go away.
 */
export function useMarkProcessSignalUnavailable(): () => void {
  const queryClient = useQueryClient();
  return useCallback(
    () =>
      queryClient.setQueryData<Edition>(appKeys.edition, (e) => ({
        ...e,
        process_signal: false,
      })),
    [queryClient]
  );
}
