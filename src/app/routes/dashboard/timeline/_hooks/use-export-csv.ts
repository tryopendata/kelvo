import { useState } from "react";
import { toast } from "sonner";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";
import {
  exportFailure,
  exportFileName,
  exportSaved,
  exportThrown,
} from "../_lib/export";
import type { LaneDef } from "../_lib/lanes";
import type { Span } from "../_lib/time";

export interface ExportRange {
  span: Span;
  fromMs: number;
  toMs: number;
  lanes: readonly LaneDef[];
}

/**
 * `export_csv` for the visible range and lanes. Rust asks where with a save
 * dialog; a save or a failure is a toast, a cancel says nothing.
 */
export function useExportCsv() {
  const transport = useTransport();
  const hostId = useHostId();
  const [exporting, setExporting] = useState(false);

  const exportCsv = async ({ span, fromMs, toMs, lanes }: ExportRange) => {
    setExporting(true);
    try {
      const result = await transport.exportCsv({
        host: hostId,
        selectors: lanes.flatMap((l) =>
          l.metrics.map((m) => ({ metric: m.metric, labels: [] }))
        ),
        from_ms: fromMs,
        to_ms: toMs,
        tier: "auto",
        file_name: exportFileName(span, fromMs),
      });
      if (result.status === "error") {
        console.error("[timeline] export_csv failed", {
          hostId,
          error: result.error,
        });
        toast.error(exportFailure(result.error));
        return;
      }
      const saved = exportSaved(result.data);
      if (saved) toast.success(saved);
    } catch (thrown) {
      // The invoke itself failed (the bridge rethrows what is not a typed
      // command error): say so rather than leave an unhandled rejection.
      console.error("[timeline] export_csv threw", { hostId, error: thrown });
      toast.error(exportThrown(thrown));
    } finally {
      setExporting(false);
    }
  };

  return { exportCsv, exporting };
}
