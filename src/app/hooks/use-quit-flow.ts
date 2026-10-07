import type { LiveProcess } from "@core/generated/bindings";
import {
  type ProcessSignalResult,
  refusalReason,
  type SignalKind,
  type SignalTarget,
  signalOutcome,
  signalTarget,
} from "@core/process-signal";
import { useState } from "react";
import { toast } from "sonner";
import { useMarkProcessSignalUnavailable } from "~/hooks/use-edition";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/**
 * What Quit needs of a process: its identity, the name the dialog shows and
 * Rust's refusal. A live row (`LiveProcess`) and an energy row
 * (`ProcessEnergy`, D-093) both carry it.
 */
export type Quittable = Pick<
  LiveProcess,
  "pid" | "start_time_us" | "name" | "refusal"
>;

export interface QuitRequest {
  target: SignalTarget;
  kind: SignalKind;
}

export interface QuitFlow {
  /** The request waiting for confirmation, or null when no dialog is open. */
  pending: QuitRequest | null;
  /** The command is in flight. */
  busy: boolean;
  /** Open the confirm dialog. Does nothing for a refused process. */
  request: (p: Quittable, kind: SignalKind) => void;
  cancel: () => void;
  /** Send `process_signal` for the pending request and report the outcome. */
  confirm: () => Promise<void>;
}

/**
 * Quit and Force Quit (D-029): nothing is sent until the user confirms, and
 * the outcome is a toast. `EPERM` reads "Kelvo can't quit processes owned by
 * another user" and offers nothing more; there is no escalation.
 */
export function useQuitFlow(): QuitFlow {
  const transport = useTransport();
  const hostId = useHostId();
  const [pending, setPending] = useState<QuitRequest | null>(null);
  const [busy, setBusy] = useState(false);
  const markUnavailable = useMarkProcessSignalUnavailable();

  const confirm = async () => {
    if (!pending || busy) return;
    const { target, kind } = pending;
    setBusy(true);
    let result: ProcessSignalResult;
    try {
      result = await transport.processSignal(
        hostId,
        target.pid,
        target.startTimeUs,
        kind
      );
    } catch (err) {
      // Typed errors resolve; only an IPC failure (an `Error`) throws.
      console.error("[processes] process_signal failed", {
        hostId,
        window: transport.windowLabel(),
        pid: target.pid,
        kind,
        err,
      });
      toast.error(err instanceof Error ? err.message : String(err));
      setBusy(false);
      setPending(null);
      return;
    }
    if (result.status === "error" && result.error.kind === "unavailable") {
      markUnavailable();
    }
    const outcome = signalOutcome(result, target, kind);
    toast[outcome.tone](outcome.message);
    setBusy(false);
    setPending(null);
  };

  return {
    pending,
    busy,
    request: (p, kind) => {
      if (refusalReason(p) !== null) return;
      setPending({ target: signalTarget(p), kind });
    },
    cancel: () => {
      if (!busy) setPending(null);
    },
    confirm,
  };
}
