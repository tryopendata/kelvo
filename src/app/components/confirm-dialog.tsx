import { AlertDialog } from "radix-ui";
import type { ComponentProps, ReactNode } from "react";
import { Button } from "~/components/ui/button";
import {
  DialogFooter,
  DialogHeader,
  dialogContentClass,
  dialogDescriptionClass,
  dialogOverlayClass,
  dialogTitleClass,
} from "~/components/ui/dialog";
import { cn } from "~/lib/utils";

export interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description: ReactNode;
  confirmLabel: ReactNode;
  confirmVariant?: ComponentProps<typeof Button>["variant"];
  /**
   * Runs the action. The dialog stays open until the caller closes it, so
   * an async action can keep it up (with `busy`) until it is done.
   */
  onConfirm: () => void;
  /** Disables the confirm button while the action runs. */
  busy?: boolean;
  /** Disables Cancel too while busy (a signal already sent cannot be taken back). */
  lockWhileBusy?: boolean;
  /** Width override for the content ("sm:max-w-sm"). */
  className?: string;
}

/**
 * A yes/no confirmation before an action: an `alertdialog`, so a click
 * outside does not dismiss it and Cancel takes the initial focus. Escape
 * and Cancel close it. Dialog look (ui/dialog.tsx) with no close button.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  confirmVariant = "default",
  onConfirm,
  busy = false,
  lockWhileBusy = false,
  className,
}: ConfirmDialogProps) {
  return (
    <AlertDialog.Root open={open} onOpenChange={onOpenChange}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className={dialogOverlayClass} />
        <AlertDialog.Content className={cn(dialogContentClass, className)}>
          <DialogHeader>
            <AlertDialog.Title className={dialogTitleClass}>
              {title}
            </AlertDialog.Title>
            <AlertDialog.Description className={dialogDescriptionClass}>
              {description}
            </AlertDialog.Description>
          </DialogHeader>
          <DialogFooter>
            <AlertDialog.Cancel asChild>
              <Button variant="outline" disabled={lockWhileBusy && busy}>
                Cancel
              </Button>
            </AlertDialog.Cancel>
            <Button
              variant={confirmVariant}
              disabled={busy}
              onClick={onConfirm}
            >
              {confirmLabel}
            </Button>
          </DialogFooter>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
