/**
 * Quit and Force Quit (D-029, plan 4.14). `SignalKind`,
 * `ProcessSignalError` and `SignalRefusal` are generated from the Rust
 * `process_signal` command.
 *
 * Rust decides what is refused: each live row carries `refusal`, from the
 * rule `process_signal` checks again regardless of what the UI sends
 * (`src-tauri/src/process_signal/mod.rs`, D-092). This module words it.
 */
import type {
  LiveProcess,
  ProcessSignalError,
  SignalKind,
  SignalRefusal,
} from "@core/generated/bindings";

export type { ProcessSignalError, SignalKind, SignalRefusal };

/** What `commands.processSignal` resolves to. */
export type ProcessSignalResult =
  | { status: "ok"; data: null }
  | { status: "error"; error: ProcessSignalError };

/** The process the user picked: pid plus start time is its identity. */
export interface SignalTarget {
  pid: number;
  startTimeUs: number;
  name: string;
}

export function signalTarget(
  p: Pick<LiveProcess, "pid" | "start_time_us" | "name">
): SignalTarget {
  return { pid: p.pid, startTimeUs: p.start_time_us, name: p.name };
}

const REFUSAL_TEXT: Record<SignalRefusal, string> = {
  kernel_task: "kernel_task is the macOS kernel and can't be quit.",
  launchd: "launchd starts every other process; quitting it would stop macOS.",
  window_server: "Quitting WindowServer logs you out and closes every app.",
  login_window: "Quitting loginwindow logs you out and closes every app.",
  kelvo: "This is Kelvo. Quit it from the menu bar instead.",
};

/**
 * Why Kelvo will not signal this process, or `null` when it may. The text is
 * the tooltip on the disabled action and the toast of a refused signal.
 */
export function refusalReason(p: Pick<LiveProcess, "refusal">): string | null {
  return p.refusal === null ? null : REFUSAL_TEXT[p.refusal];
}

export type SignalOutcome =
  | { tone: "success"; message: string }
  | { tone: "info"; message: string }
  | { tone: "error"; message: string };

/** What to tell the user after `process_signal` answers. */
export function signalOutcome(
  result: ProcessSignalResult,
  target: SignalTarget,
  kind: SignalKind
): SignalOutcome {
  const who = `${target.name} (${target.pid})`;
  if (result.status === "ok") {
    return {
      tone: "success",
      message: kind === "quit" ? `Asked ${who} to quit` : `Force quit ${who}`,
    };
  }
  const e = result.error;
  switch (e.kind) {
    case "permission_denied":
      return {
        tone: "error",
        message: "Kelvo can't quit processes owned by another user",
      };
    case "not_found":
      return { tone: "info", message: `${who} has already exited` };
    case "pid_reused":
      return {
        tone: "info",
        message: `PID ${target.pid} now belongs to a different process, so nothing was sent`,
      };
    case "refused":
      return { tone: "error", message: REFUSAL_TEXT[e.refusal] };
    case "remote_host":
      return {
        tone: "error",
        message: "Kelvo can only quit processes on this Mac",
      };
    case "unknown_host":
      return { tone: "error", message: `Couldn't quit ${who} (unknown host)` };
    case "failed":
      return { tone: "error", message: `Couldn't quit ${who}: ${e.message}` };
    case "unavailable":
      return {
        tone: "error",
        message: "Quitting processes isn't available in this edition of Kelvo",
      };
    default: {
      const unhandled: never = e;
      return {
        tone: "error",
        message: `Couldn't quit ${who} (${JSON.stringify(unhandled)})`,
      };
    }
  }
}
