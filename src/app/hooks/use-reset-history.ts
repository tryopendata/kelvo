import { resetHistoryFailure } from "@core/history-state";
import { historyKeys, hostKeys } from "@core/query-keys";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/**
 * `reset_history` (D-064): moves the history file aside and starts an empty
 * one. On success every history query, the size on disk and the health are
 * refetched, so the banner clears without waiting for the health event. A
 * failure is a toast; `store_busy` names the other Kelvo holding the file.
 * Resolves true when the store was reset.
 */
export function useResetHistory() {
  const transport = useTransport();
  const hostId = useHostId();
  const queryClient = useQueryClient();
  const [resetting, setResetting] = useState(false);

  const reset = async (): Promise<boolean> => {
    setResetting(true);
    const result = await transport.resetHistory();
    setResetting(false);
    if (result.status === "error") {
      console.error("[history] reset_history failed", {
        hostId,
        error: result.error,
      });
      toast.error(resetHistoryFailure(result.error));
      return false;
    }
    void queryClient.invalidateQueries({ queryKey: historyKeys.all });
    void queryClient.invalidateQueries({
      queryKey: hostKeys.historySize(hostId),
    });
    void queryClient.invalidateQueries({
      queryKey: hostKeys.historyHealth(hostId),
    });
    toast.success("History reset. The old file was kept aside.");
    return true;
  };

  return { reset, resetting };
}
