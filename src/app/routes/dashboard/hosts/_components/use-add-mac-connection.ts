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
  | "cancelling";
export function useAddMacConnection({
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
  const add = async () => {
    if (!prepared) return;
    const attempt = crypto.randomUUID();
    setupId.current = attempt;
    setPhase("adding");
    setError(null);
    const config: RemoteHostConfig = {
      id: prepared.host_id ?? "",
      name: name.trim() || prepared.name,
      ...endpoints,
      enabled: true,
      mirror_budget_mb: 100,
    };
    try {
      const result = await transport.addRemoteHost(
        config,
        !prepared.installed || !prepared.compatible,
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

  return { add, cancel };
}
