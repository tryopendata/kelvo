import type { SettingsPatch } from "@core/generated/bindings";
import { settingsErrorText } from "@core/settings-patch";
import { useCallback } from "react";
import { toast } from "sonner";
import { useUpdateSettings } from "~/stores/settings-store";

/**
 * Send one `update_settings` patch (D-050). The mirror takes the returned
 * snapshot, so the control shows the saved value; a rejected patch leaves
 * the control where it was and says why in a toast.
 */
export function useWriteSettings(): (patch: SettingsPatch) => void {
  const update = useUpdateSettings();
  return useCallback(
    (patch) => {
      void update(patch).then((result) => {
        if (result.status === "error") {
          console.error("[settings] update_settings failed", {
            patch,
            error: result.error,
          });
          toast.error(settingsErrorText(result.error));
        }
      });
    },
    [update]
  );
}
