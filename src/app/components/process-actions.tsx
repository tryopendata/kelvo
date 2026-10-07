import { refusalReason, type SignalKind } from "@core/process-signal";
import { Button } from "~/components/ui/button";
import {
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
} from "~/components/ui/context-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "~/components/ui/dialog";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "~/components/ui/tooltip";
import type { QuitFlow, Quittable } from "~/hooks/use-quit-flow";

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

/** The row's context menu: the same two actions, with the reason when refused. */
export function ProcessContextMenu({
  p,
  onRequest,
}: {
  p: Quittable;
  onRequest: QuitFlow["request"];
}) {
  const reason = refusalReason(p);
  return (
    <ContextMenuContent className="w-60">
      <ContextMenuLabel className="truncate font-[590] text-[12px]">
        {p.name}
        <span className="data-mono ml-1.5 font-normal text-muted-foreground">
          {p.pid}
        </span>
      </ContextMenuLabel>
      <ContextMenuSeparator />
      <ContextMenuItem
        disabled={reason !== null}
        onSelect={() => onRequest(p, "quit")}
      >
        Quit…
      </ContextMenuItem>
      <ContextMenuItem
        variant="destructive"
        disabled={reason !== null}
        onSelect={() => onRequest(p, "force_quit")}
      >
        Force Quit…
      </ContextMenuItem>
      {reason && (
        <p className="px-2 pt-1 pb-1.5 font-normal text-[11px] text-muted-foreground">
          {reason}
        </p>
      )}
    </ContextMenuContent>
  );
}

/**
 * Confirm dialog naming the process and PID. Force Quit says that unsaved
 * data is lost (D-029). Nothing is sent before the confirm button.
 */
export function QuitDialog({ flow }: { flow: QuitFlow }) {
  const req = flow.pending;
  const force = req?.kind === "force_quit";
  return (
    <Dialog
      open={req !== null}
      onOpenChange={(open) => {
        if (!open) flow.cancel();
      }}
    >
      <DialogContent showCloseButton={false} className="sm:max-w-md">
        {req && (
          <>
            <DialogHeader>
              <DialogTitle>
                {force ? "Force quit" : "Quit"} {req.target.name}?
              </DialogTitle>
              <DialogDescription>
                {force ? (
                  <>
                    {req.target.name} (PID{" "}
                    <span className="data-mono">{req.target.pid}</span>) stops
                    immediately. Unsaved data in this process will be lost.
                  </>
                ) : (
                  <>
                    Kelvo asks {req.target.name} (PID{" "}
                    <span className="data-mono">{req.target.pid}</span>) to
                    quit. An app with unsaved changes may ask you to save first.
                  </>
                )}
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button
                variant="outline"
                onClick={flow.cancel}
                disabled={flow.busy}
              >
                Cancel
              </Button>
              <Button
                variant={force ? "destructive" : "secondary"}
                onClick={() => void flow.confirm()}
                disabled={flow.busy}
              >
                {force ? "Force Quit" : "Quit"}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
