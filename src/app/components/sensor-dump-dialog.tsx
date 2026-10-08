import { hostKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "~/components/ui/dialog";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/**
 * The issue form asks the user to paste the JSON: a full dump is too long
 * for a URL (plan 4.17). The repository path is an assumption until the
 * project is public; no doc names it yet.
 */
export const SENSOR_ISSUE_URL =
  "https://github.com/tryopendata/kelvo/issues/new?labels=sensors&title=Sensor+map+for+";

/** Issue link with the model in the title, so reports group by chip. */
export function sensorIssueUrl(model: string | null): string {
  return `${SENSOR_ISSUE_URL}${encodeURIComponent(model ?? "unknown Mac")}`;
}

/**
 * Share sensor dump sheet (plan 4.17): the JSON Kelvo collected, for the
 * user to review before sharing, with Copy, Save… and Open GitHub issue.
 * `sensor_dump` leaves out serials, user, host and network names.
 */
export function SensorDumpDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const transport = useTransport();
  const hostId = useHostId();
  const { data, error } = useQuery({
    queryKey: hostKeys.sensorDump(hostId),
    queryFn: () => unwrap(transport.sensorDump(hostId)),
    enabled: open,
    staleTime: 0,
  });
  const json = data ? JSON.stringify(data, null, 2) : "";

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(json);
      toast("Sensor dump copied");
    } catch (err) {
      console.error("[sensor-dump] copy failed", { hostId, err });
      toast("Couldn't copy. Select the text and copy it instead.");
    }
  };

  const save = () => {
    const url = URL.createObjectURL(
      new Blob([json], { type: "application/json" })
    );
    const a = document.createElement("a");
    a.href = url;
    a.download = `kelvo-sensor-dump-${data?.model ?? "mac"}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[560px]">
        <DialogHeader>
          <DialogTitle>Share sensor dump</DialogTitle>
          <DialogDescription>
            What Kelvo read from this Mac's sensors. It has no serial numbers,
            user, host or network names. Review it, then paste it into the
            GitHub issue.
          </DialogDescription>
        </DialogHeader>
        <pre
          data-testid="sensor-dump-json"
          className="figures m-0 max-h-[320px] overflow-auto rounded-tile border border-border bg-deep p-3 text-[11px] text-fg-subtle"
        >
          {error
            ? "The sensor dump could not be read."
            : json || "Reading sensors…"}
        </pre>
        <DialogFooter>
          <Button
            variant="outline"
            disabled={!data}
            onClick={() => void copy()}
          >
            Copy
          </Button>
          <Button variant="outline" disabled={!data} onClick={save}>
            Save…
          </Button>
          <Button
            disabled={!data}
            onClick={() =>
              void transport.openUrl(sensorIssueUrl(data?.model ?? null))
            }
          >
            Open GitHub issue
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Open state for the sheet, so a notice can own its trigger. */
export function useSensorDumpDialog() {
  const [open, setOpen] = useState(false);
  return { open, setOpen, show: () => setOpen(true) };
}
