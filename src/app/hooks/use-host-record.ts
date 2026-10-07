import type { HostRecord } from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/** The current host's record (`get_host`); static for the app run. */
export function useHostRecord(): HostRecord | undefined {
  const transport = useTransport();
  const hostId = useHostId();
  const { data } = useQuery({
    queryKey: hostKeys.detail(hostId),
    queryFn: () => unwrap(transport.getHost(hostId)),
    staleTime: Infinity,
  });
  return data;
}

/**
 * The top of the GPU's DVFS table (`HostInfo.gpu_dvfs_mhz`, D-092), MHz:
 * the GPU's maximum frequency. Null while the record loads or on a host
 * without the table.
 */
export function useGpuMaxMhz(): number | null {
  const dvfs = useHostRecord()?.info.gpu_dvfs_mhz ?? [];
  return dvfs.length === 0 ? null : Math.max(...dvfs);
}
