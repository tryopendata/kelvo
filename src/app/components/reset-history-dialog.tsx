import { ConfirmDialog } from "~/components/confirm-dialog";
import { useResetHistory } from "~/hooks/use-reset-history";

export interface ResetHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * Confirm for "Reset history". Nothing is deleted: the old file stays next
 * to the new one as `history-reset-<time>.sqlite`, so the dialog says so
 * and the action is not styled as destructive.
 */
export function ResetHistoryDialog({
  open,
  onOpenChange,
}: ResetHistoryDialogProps) {
  const { reset, resetting } = useResetHistory();
  const confirm = async () => {
    await reset();
    onOpenChange(false);
  };
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      className="sm:max-w-sm"
      title="Reset history?"
      description={
        <>
          Kelvo starts a new, empty history file. The current one is kept aside
          next to it as history-reset-&lt;time&gt;.sqlite, not deleted. Live
          values keep updating, and the Timeline starts again from now.
        </>
      }
      confirmLabel="Reset history"
      onConfirm={confirm}
      busy={resetting}
    />
  );
}
