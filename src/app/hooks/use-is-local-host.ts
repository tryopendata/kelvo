import { useHostSelection } from "~/lib/host-selection";
import { useHostId } from "~/stores/host-store";
import { useHostRecord } from "./use-host-record";

export function useIsLocalHost(): boolean {
  const selection = useHostSelection();
  const id = useHostId();
  const record = useHostRecord();
  return (
    (selection?.hosts.find((host) => host.id === id) ?? record)?.is_local !==
    false
  );
}
