import type {
  CommandError,
  RemoteHostConfig,
  RemoteHostPreparation,
} from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { isHostKeyError } from "@core/remote-errors";
import { useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";
import { useTransport } from "~/lib/transport-context";

export type SetupPhase =
  | "idle"
  | "checking"
  | "adding"
  | "verifying"
  | "resetting"
  | "cancelling";
export function useAddHostConnection({
  prepared,
  name,
  endpoints,
  showMenuBar,
  phase,
  setPhase,
  setError,
  setPrepared,
  setHostKeyError,
  onOpenChange,
  lan,
  tailscale,
}: {
  prepared: RemoteHostPreparation | null;
  name: string;
  endpoints: { lan: string | null; tailscale: string | null };
  showMenuBar: boolean;
  phase: SetupPhase;
  setPhase: (phase: SetupPhase) => void;
  setError: (error: CommandError | string | null) => void;
  setPrepared: (prepared: RemoteHostPreparation | null) => void;
  setHostKeyError: (error: boolean) => void;
  onOpenChange: (open: boolean) => void;
  lan: string;
  tailscale: string;
}) {
  const transport = useTransport();
  const cache = useQueryClient();
  const setupId = useRef<string | null>(null);
  const add = async (next: RemoteHostPreparation | null = prepared) => {
    if (!next) return;
    const attempt = crypto.randomUUID();
    setupId.current = attempt;
    setPhase("adding");
    setError(null);
    const config: RemoteHostConfig = {
      id: next.host_id ?? "",
      name: name.trim() || next.name,
      ...endpoints,
      enabled: true,
      mirror_budget_mb: 100,
    };
    try {
      const result = await transport.addRemoteHost(
        config,
        !next.installed || !next.compatible,
        showMenuBar,
        attempt
      );
      if (result.status === "error") {
        setError(result.error);
        if (isHostKeyError(result.error)) {
          setPrepared(null);
          setHostKeyError(true);
        }
      } else {
        await cache.invalidateQueries({ queryKey: hostKeys.all });
        onOpenChange(false);
      }
    } catch (e) {
      console.error("[remote setup] add failed", { lan, tailscale, error: e });
      setError(String(e));
    } finally {
      setupId.current = null;
      setPhase("idle");
    }
  };
  /** A Linux agent that reports a saved host's id gets a new one, then is added. */
  const resetAndAdd = async () => {
    setPhase("resetting");
    setError(null);
    try {
      const result = await transport.resetRemoteHostIdentity(
        endpoints.lan,
        endpoints.tailscale
      );
      if (result.status === "error") {
        setError(result.error);
        setPhase("idle");
        return;
      }
      setPrepared(result.data);
      await add(result.data);
    } catch (e) {
      console.error("[remote setup] identity reset failed", {
        lan,
        tailscale,
        error: e,
      });
      setError(String(e));
      setPhase("idle");
    }
  };
  const cancel = async () => {
    if (phase === "idle") {
      onOpenChange(false);
      return;
    }
    const attempt = setupId.current;
    if (phase !== "adding" || attempt === null) return;
    setPhase("cancelling");
    try {
      const result = await transport.cancelRemoteSetup(attempt);
      if (setupId.current === attempt && result.status === "error")
        setError(result.error);
    } catch (e) {
      console.error("[remote setup] cancel failed", {
        lan,
        tailscale,
        error: e,
      });
      if (setupId.current === attempt) setError(String(e));
    }
    // The original add response owns completion: its rollback must finish first.
  };

  return { add, resetAndAdd, cancel };
}
