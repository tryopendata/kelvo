import type { HostId } from "@core/generated/bindings";
import { type ProcessTarget, uniqueTargets } from "@core/process-selection";
import {
  type ProcessSignalResult,
  refusalReason,
  type SignalKind,
  type SignalOutcome,
  type SignalTarget,
  signalOutcome,
  signalTarget,
} from "@core/process-signal";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  useEdition,
  useMarkProcessSignalUnavailable,
} from "~/hooks/use-edition";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

export type Quittable = ProcessTarget;

export interface QuitRequest {
  host: HostId;
  /** Kept for single-process dialog wording. */
  target: SignalTarget;
  targets: readonly Quittable[];
  kind: SignalKind;
}

export interface QuitResult {
  target: Quittable;
  state: "accepted" | "excluded" | "failed" | "unknown" | "unsent";
  tone: SignalOutcome["tone"];
  message: string;
}

export const quitExclusion = (p: Quittable): string | null =>
  p.running === false ? "Already exited" : refusalReason(p);

export interface QuitFlow {
  pending: QuitRequest | null;
  busy: boolean;
  results: readonly QuitResult[];
  progress: number;
  request: (p: Quittable, kind: SignalKind) => void;
  requestMany: (targets: readonly Quittable[], kind: SignalKind) => void;
  cancel: () => void;
  confirm: () => Promise<void>;
}

/** One immutable, host-bound request; every target still passes Rust's guards. */
// biome-ignore lint/complexity/noExcessiveLinesPerFunction: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
export function useQuitFlow(): QuitFlow {
  const local = useIsLocalHost();
  const edition = useEdition();
  const transport = useTransport();
  const hostId = useHostId();
  const [pending, setPending] = useState<QuitRequest | null>(null);
  const pendingRef = useRef<QuitRequest | null>(null);
  const [busy, setBusy] = useState(false);
  const locked = useRef(false);
  const [results, setResults] = useState<readonly QuitResult[]>([]);
  const [progress, setProgress] = useState(0);
  const lifetime = useRef(0);
  const currentHost = useRef(hostId);
  currentHost.current = hostId;
  const allowed = useRef(false);
  allowed.current = local && edition?.process_signal === true;
  const markUnavailable = useMarkProcessSignalUnavailable();

  useEffect(() => {
    currentHost.current = hostId;
    setPending(null);
    pendingRef.current = null;
    setResults([]);
    setBusy(false);
    locked.current = false;
    return () => {
      lifetime.current += 1;
    };
  }, [hostId]);

  const requestMany = (targets: readonly Quittable[], kind: SignalKind) => {
    if (!allowed.current || locked.current) return;
    const captured = uniqueTargets(targets).map((p) => ({ ...p }));
    const first = captured[0];
    if (!first) return;
    const req = {
      host: hostId,
      target: signalTarget(first),
      targets: captured,
      kind,
    };
    pendingRef.current = req;
    setPending(req);
    setProgress(0);
  };

  // biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
  const confirm = async () => {
    const req = pendingRef.current;
    if (
      !allowed.current ||
      !req ||
      locked.current ||
      req.host !== currentHost.current
    )
      return;
    if (req.targets.every((p) => quitExclusion(p) !== null)) return;
    // A ref closes the same-render double-click race before React commits busy.
    locked.current = true;
    setBusy(true);
    const token = lifetime.current;
    const out: QuitResult[] = [];
    let stopped = false;
    for (const p of req.targets) {
      const exclusion = quitExclusion(p);
      if (exclusion) {
        out.push({
          target: p,
          state: "excluded",
          tone: "info",
          message: exclusion,
        });
      } else if (
        stopped ||
        lifetime.current !== token ||
        currentHost.current !== req.host ||
        !allowed.current
      ) {
        out.push({
          target: p,
          state: "unsent",
          tone: "info",
          message: "Not sent — batch interrupted",
        });
      } else {
        try {
          const result: ProcessSignalResult = await transport.processSignal(
            req.host,
            p.pid,
            p.start_time_us,
            req.kind
          );
          const outcome = signalOutcome(result, signalTarget(p), req.kind);
          out.push({
            target: p,
            state: result.status === "ok" ? "accepted" : "failed",
            tone: outcome.tone,
            message: outcome.message,
          });
          if (
            result.status === "error" &&
            ["unavailable", "remote_host", "unknown_host"].includes(
              result.error.kind
            )
          ) {
            stopped = true;
            if (result.error.kind === "unavailable") markUnavailable();
          }
        } catch (err) {
          console.error("[processes] process_signal failed", {
            hostId: req.host,
            pid: p.pid,
            kind: req.kind,
            err,
          });
          out.push({
            target: p,
            state: "unknown",
            tone: "error",
            message:
              "Connection failed; whether the request arrived is unknown",
          });
          stopped = true;
        }
      }
      if (lifetime.current === token) setProgress(out.length);
    }
    const accepted = out.filter((r) => r.state === "accepted").length;
    if (out.length === 1 && out[0]) {
      toast[out[0].tone](out[0].message);
    } else {
      toast.info(
        `${accepted} of ${out.length} ${req.kind === "quit" ? "quit" : "force quit"} requests accepted`
      );
    }
    if (lifetime.current !== token) return;
    setResults(out);
    setBusy(false);
    locked.current = false;
    pendingRef.current = null;
    setPending(null);
  };

  return {
    pending,
    busy,
    results,
    progress,
    requestMany,
    request: (p, kind) => {
      if (quitExclusion(p) === null) requestMany([p], kind);
    },
    cancel: () => {
      if (!locked.current) {
        pendingRef.current = null;
        setPending(null);
      }
    },
    confirm,
  };
}
