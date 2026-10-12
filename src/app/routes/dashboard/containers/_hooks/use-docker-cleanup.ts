import type {
  DockerStorageOperation,
  DockerStorageReview,
  DockerStorageSelection,
} from "@core/generated/bindings";
import { dockerKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTransport } from "~/lib/transport-context";

/** Operations live in Rust; retaining their ID in the query cache survives route navigation. */
export function useDockerCleanup(
  engine: string,
  generation: number,
  connected: boolean
) {
  const transport = useTransport();
  const client = useQueryClient();
  const [review, setReview] = useState<DockerStorageReview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const locked = useRef(false);
  const identity = `${engine}:${generation}`;
  const identityRef = useRef(identity);
  identityRef.current = identity;
  const operation = useQuery({
    queryKey: dockerKeys.operation(engine),
    enabled: Boolean(engine),
    queryFn: async () => {
      const previous = client.getQueryData<DockerStorageOperation | null>(
        dockerKeys.operation(engine)
      );
      if (!previous) return null;
      const result = await unwrap(
        transport.dockerStorageOperation(previous.id)
      );
      const current = client.getQueryData<DockerStorageOperation | null>(
        dockerKeys.operation(engine)
      );
      // A poll dispatched before Execute must not restore the preceding operation.
      if (current && current.id !== previous.id) return current;
      if (
        current &&
        (current.results.length > result.results.length ||
          (current.finished_ms !== null && result.finished_ms === null))
      )
        return current;
      return result;
    },
    refetchInterval: (query) =>
      query.state.data?.finished_ms === null ? 1000 : false,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: false,
    retry: false,
    staleTime: 0,
    gcTime: 3_600_000,
  });
  const storage = useQuery({
    queryKey: dockerKeys.storage(engine, generation),
    queryFn: () => unwrap(transport.dockerStorageScan(false)),
    enabled: false,
  });
  const latestOperationId = storage.data?.latest_operation_id;
  useEffect(() => {
    if (latestOperationId === operation.data?.id) return;
    const previous = client.getQueryData<DockerStorageOperation | null>(
      dockerKeys.operation(engine)
    );
    if (!latestOperationId || previous?.id === latestOperationId) return;
    const previousId = previous?.id;
    let cancelled = false;
    void unwrap(transport.dockerStorageOperation(latestOperationId))
      .then((result) => {
        if (cancelled || result.engine_id !== engine) return;
        const current = client.getQueryData<DockerStorageOperation | null>(
          dockerKeys.operation(engine)
        );
        // Ignore late discovery replies after a newer local Execute or another recovery.
        if (current?.id !== previousId) return;
        // A stale scan can name a preceding operation. Never move backwards; equal
        // timestamps with different IDs have no proven ordering, so retain the current result.
        if (current && result.started_ms <= current.started_ms) return;
        client.setQueryData(dockerKeys.operation(engine), result);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [latestOperationId, operation.data?.id, transport, client, engine]);
  useEffect(() => {
    const result = operation.data;
    if (
      !result ||
      result.finished_ms === null ||
      client.getQueryData(dockerKeys.operationRefresh(engine)) === result.id ||
      result.generation !== generation ||
      !connected
    )
      return;
    client.setQueryData(dockerKeys.operationRefresh(engine), result.id);
    void unwrap(transport.dockerStorageScan(true))
      .then((view) => {
        if (identityRef.current === identity)
          client.setQueryData(dockerKeys.storage(engine, generation), view);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [
    operation.data,
    connected,
    transport,
    client,
    engine,
    generation,
    identity,
  ]);
  useEffect(() => {
    setReview((previous) =>
      previous &&
      (previous.engine_id !== engine ||
        previous.generation !== generation ||
        !connected)
        ? null
        : previous
    );
  }, [engine, generation, connected]);
  const prepare = useCallback(
    async (selections: DockerStorageSelection[]) => {
      if (locked.current || !connected || !engine) return;
      locked.current = true;
      setBusy(true);
      setError(null);
      try {
        const next = await unwrap(transport.dockerStoragePrepare(selections));
        if (identityRef.current === identity) {
          if (next.engine_id === engine && next.generation === generation)
            setReview(next);
          else
            setError(
              "Engine changed during review. Refresh storage before selecting resources again."
            );
        }
      } catch (e) {
        setError(String(e));
      } finally {
        locked.current = false;
        setBusy(false);
      }
    },
    [transport, connected, engine, generation, identity]
  );
  const execute = useCallback(
    async (confirmVolumes: boolean) => {
      if (!review || locked.current || !connected) return;
      locked.current = true;
      setBusy(true);
      setError(null);
      // Consume the UI copy before sending. An ambiguous transport failure must never offer retry.
      setReview(null);
      try {
        const next = await unwrap(
          transport.dockerStorageExecute(review.token, confirmVolumes)
        );
        client.setQueryData(dockerKeys.operation(review.engine_id), next);
      } catch (e) {
        setError(
          `Execution could not be confirmed. Do not retry automatically; refresh storage to inspect current resources. ${String(e)}`
        );
      } finally {
        locked.current = false;
        setBusy(false);
      }
    },
    [review, transport, client, connected]
  );
  return {
    review,
    dismiss: () => setReview(null),
    prepare,
    execute,
    error,
    busy: busy || operation.data?.finished_ms === null,
    operation: operation.data,
    operationError: operation.error,
  };
}
