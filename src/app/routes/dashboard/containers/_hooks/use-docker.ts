import type { DockerSnapshot } from "@core/generated/bindings";
import { dockerKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";
import { dockerErrorMessage } from "../_lib/presentation";

export function useDocker() {
  const transport = useTransport();
  const local = useIsLocalHost();
  const [snapshot, setSnapshot] = useState<DockerSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!local) return;
    let cancelled = false;
    let stop: (() => void) | undefined;
    transport
      .dockerSubscribe((next) => {
        if (!cancelled)
          setSnapshot((previous) =>
            previous && previous.generation > next.generation ? previous : next
          );
      })
      .then((subscription) => {
        if (cancelled) subscription.unsubscribe();
        else {
          stop = subscription.unsubscribe;
          if (subscription.info.status === "error")
            setError(dockerErrorMessage(subscription.info.error));
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [transport, local]);
  return { snapshot: local ? snapshot : null, error, setSnapshot };
}

export function useDockerHistory(
  engine: string,
  container: string | null,
  from: number,
  to: number,
  generation = 0
) {
  const transport = useTransport();
  const local = useIsLocalHost();
  const host = useHostId();
  return useQuery({
    queryKey: dockerKeys.history(engine, container, from, to, host, generation),
    placeholderData: keepPreviousData,
    enabled: local && Boolean(engine),
    queryFn: () =>
      unwrap(
        transport.dockerHistory({
          engine_id: engine,
          container_id: container,
          from_ms: from,
          to_ms: to,
          max_points: 360,
        })
      ),
  });
}
export function useDockerEvents(
  engine: string,
  container: string | null,
  from: number,
  to: number,
  generation = 0,
  beforeId: string | null = null
) {
  const transport = useTransport();
  const local = useIsLocalHost();
  const host = useHostId();
  return useQuery({
    queryKey: dockerKeys.events(
      engine,
      container,
      from,
      to,
      host,
      generation,
      beforeId
    ),
    enabled: local && Boolean(engine),
    queryFn: () =>
      unwrap(
        transport.dockerEvents({
          engine_id: engine,
          container_id: container,
          from_ms: from,
          to_ms: to,
          limit: 200,
          before_id: beforeId,
        })
      ),
  });
}
