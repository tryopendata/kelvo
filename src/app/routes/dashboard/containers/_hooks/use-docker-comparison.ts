import type { DockerComparisonRequest } from "@core/generated/bindings";
import { dockerKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";
export function useDockerComparison(
  request: DockerComparisonRequest,
  generation: number
) {
  const transport = useTransport();
  const local = useIsLocalHost();
  const host = useHostId();
  return useQuery({
    queryKey: dockerKeys.comparison(
      request.engine_id,
      request.from_ms,
      request.to_ms,
      request.metric,
      request.project,
      request.selected,
      host,
      generation
    ),
    placeholderData: keepPreviousData,
    enabled: local && Boolean(request.engine_id),
    queryFn: () => unwrap(transport.dockerComparison(request)),
  });
}
