import { refusalReason, type SignalKind } from "@core/process-signal";
import { useRef } from "react";
import { ConfirmDialog } from "~/components/confirm-dialog";
import { Button } from "~/components/ui/button";
import {
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
} from "~/components/ui/context-menu";
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
  // The last request, so the dialog keeps its text while it fades out.
  const last = useRef(req);
  if (req) last.current = req;
  const shown = req ?? last.current;
  if (!shown) return null;
  const force = shown.kind === "force_quit";
  return (
    <ConfirmDialog
      open={req !== null}
      onOpenChange={(open) => {
        if (!open) flow.cancel();
      }}
      className="sm:max-w-md"
      title={`${force ? "Force quit" : "Quit"} ${shown.target.name}?`}
      description={
        force ? (
          <>
            {shown.target.name} (PID{" "}
            <span className="data-mono">{shown.target.pid}</span>) stops
            immediately. Unsaved data in this process will be lost.
          </>
        ) : (
          <>
            Kelvo asks {shown.target.name} (PID{" "}
            <span className="data-mono">{shown.target.pid}</span>) to quit. An
            app with unsaved changes may ask you to save first.
          </>
        )
      }
      confirmLabel={force ? "Force Quit" : "Quit"}
      confirmVariant={force ? "destructive" : "secondary"}
      onConfirm={() => void flow.confirm()}
      busy={flow.busy}
      lockWhileBusy
    />
  );
}
