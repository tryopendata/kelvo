import { mergeEvent, RecentEvents } from "@core/events";
import type { Event } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { type Transport, unwrap } from "@core/transport";
import {
  keepPreviousData,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useEffect } from "react";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/** A live list is read again this often, for anything a push missed. */
const LIVE_REFETCH_MS = 5 * 60_000;
const NONE: readonly Event[] = [];

/**
 * Recent pushes per transport, fed from its first use for the app's life,
 * so a list that mounts just after a push still sees it. Keyed by
 * transport, so each test's mock gets its own.
 */
const recentByTransport = new WeakMap<Transport, RecentEvents>();

function recentEvents(transport: Transport): RecentEvents {
  let recent = recentByTransport.get(transport);
  if (!recent) {
    const created = new RecentEvents();
    transport.onEventRecorded((e) => created.add(e.host, e.event));
    recentByTransport.set(transport, created);
    recent = created;
  }
  return recent;
}

/**
 * Detector and alert events over `spanMs` ending at `endMs` (`null` follows
 * now), oldest first. A live list reads once, then grows from
 * `event-recorded` (D-083): every list for the host merges a pushed event,
 * so a new one shows without a refetch. Callers clip to their own window;
 * a live list keeps events that have scrolled out until its next read.
 * Every read also merges the events pushed in the last few seconds, since
 * a push can beat its commit to the store (`RecentEvents`).
 * History unavailable means no events.
 */
export function useEvents(
  spanMs: number,
  endMs: number | null
): readonly Event[] {
  const transport = useTransport();
  const hostId = useHostId();
  const queryClient = useQueryClient();
  const live = endMs === null;
  const { data } = useQuery({
    queryKey: historyKeys.events(hostId, spanMs, endMs),
    queryFn: async () => {
      const recent = recentEvents(transport);
      const from = (endMs ?? Date.now()) - spanMs;
      // Live reads have no end: whatever lands while the read is in flight
      // is in the answer, in `recent`, or arrives as a push.
      const to = endMs ?? Number.MAX_SAFE_INTEGER;
      const answer = await unwrap(transport.queryEvents(hostId, from, to));
      return recent.mergeInto(hostId, answer, from, to);
    },
    staleTime: live ? LIVE_REFETCH_MS : Number.POSITIVE_INFINITY,
    refetchInterval: live ? LIVE_REFETCH_MS : false,
    placeholderData: keepPreviousData,
    retry: false,
  });

  useEffect(() => {
    recentEvents(transport);
    return transport.onEventRecorded((e) => {
      if (e.host !== hostId) return;
      queryClient.setQueriesData<Event[]>(
        { queryKey: historyKeys.allEvents(hostId) },
        (old) => (old ? mergeEvent(old, e.event) : old)
      );
    });
  }, [transport, queryClient, hostId]);

  return data ?? NONE;
}
