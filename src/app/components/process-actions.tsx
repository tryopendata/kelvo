import { refusalReason, type SignalKind } from "@core/process-signal";
import { useRef } from "react";
import { ConfirmDialog } from "~/components/confirm-dialog";
import { Button } from "~/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "~/components/ui/tooltip";
import {
  type QuitFlow,
  type Quittable,
  quitExclusion,
} from "~/hooks/use-quit-flow";

const LABEL: Record<SignalKind, string> = {
  quit: "Quit",
  force_quit: "Force Quit",
};

function ActionButton({
  p,
  kind,
  reason,
  onRequest,
}: {
  p: Quittable;
  kind: SignalKind;
  reason: string | null;
  onRequest: QuitFlow["request"];
}) {
  const button = (
    <Button
      variant="ghost"
      size="sm"
      className="h-6 px-2 text-[11px] aria-disabled:cursor-default aria-disabled:opacity-50 aria-disabled:hover:bg-transparent"
      aria-label={`${LABEL[kind]} ${p.name} (${p.pid})`}
      aria-disabled={reason !== null || undefined}
      onClick={(e) => {
        e.stopPropagation();
        if (reason === null) onRequest(p, kind);
      }}
    >
      {LABEL[kind]}
    </Button>
  );
  if (reason === null) return button;
  // aria-disabled, not disabled: a disabled button gets no pointer events, so
  // the tooltip saying why would never show.
  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent>{reason}</TooltipContent>
    </Tooltip>
  );
}

/**
 * Row action: Quit and Force Quit, shown on hover or focus (plan 4.14), on
 * the Processes table and the Power page's energy table (D-093).
 */
export function ProcessRowActions({
  p,
  onRequest,
}: {
  p: Quittable;
  onRequest: QuitFlow["request"];
}) {
  const reason = refusalReason(p);
  return (
    <span className="inline-flex gap-0.5">
      <ActionButton p={p} kind="quit" reason={reason} onRequest={onRequest} />
      <ActionButton
        p={p}
        kind="force_quit"
        reason={reason}
        onRequest={onRequest}
      />
    </span>
  );
}

/**
 * Confirm dialog naming the process and PID. Force Quit says that unsaved
 * data is lost (D-029). Nothing is sent before the confirm button.
 */
export function QuitDialog({ flow }: { flow: QuitFlow }) {
  const req = flow.pending;
  // The last request, so the dialog keeps its text while it fades out.
  const last = useRef(req);
  if (req) last.current = req;
  const shown = req ?? last.current;
  if (!shown) return null;
  const force = shown.kind === "force_quit";
  const many = shown.targets.length > 1;
  const eligible = shown.targets.filter(
    (p) => quitExclusion(p) === null
  ).length;
  return (
    <ConfirmDialog
      open={req !== null}
      onOpenChange={(open) => {
        if (!open) flow.cancel();
      }}
      className="sm:max-w-md"
      title={`${force ? "Force quit" : "Quit"} ${many ? `${shown.targets.length} processes` : shown.target.name}?`}
      description={
        many ? (
          <>
            {eligible} requests will be sent. {shown.targets.length - eligible}{" "}
            excluded.
            {force
              ? " Processes stop immediately. Unsaved data will be lost."
              : " Kelvo asks each process to quit; apps may ask you to save first."}
          </>
        ) : force ? (
          <>
            {shown.target.name} (PID{" "}
            <span className="figures">{shown.target.pid}</span>) stops
            immediately. Unsaved data in this process will be lost.
          </>
        ) : (
          <>
            Kelvo asks {shown.target.name} (PID{" "}
            <span className="figures">{shown.target.pid}</span>) to quit. An app
            with unsaved changes may ask you to save first.
          </>
        )
      }
      confirmLabel={
        flow.busy
          ? `Sending ${flow.progress} of ${shown.targets.length}…`
          : many
            ? `${force ? "Force Quit" : "Quit"} ${eligible} processes`
            : force
              ? "Force Quit"
              : "Quit"
      }
      confirmDisabled={eligible === 0}
      confirmVariant={force ? "destructive" : "secondary"}
      onConfirm={() => void flow.confirm()}
      busy={flow.busy}
      lockWhileBusy
    >
      {many && (
        <ul
          aria-label="Processes to review"
          className="max-h-64 divide-y divide-border-subtle overflow-auto text-[12px]"
        >
          {shown.targets.map((p) => (
            <li
              key={`${p.pid}:${p.start_time_us}`}
              className="flex justify-between gap-3 py-2"
            >
              <span className="min-w-0 break-words">
                {p.name}{" "}
                <span className="figures text-muted-foreground">
                  PID {p.pid}
                </span>
              </span>
              {quitExclusion(p) && (
                <span className="text-muted-foreground">
                  {quitExclusion(p)}
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
    </ConfirmDialog>
  );
}
