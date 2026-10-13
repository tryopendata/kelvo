import type {
  DockerEngine,
  DockerSettings,
  DockerSnapshot,
} from "@core/generated/bindings";
import { useEffect, useState } from "react";
import { Button } from "~/components/ui/button";
import { useTransport } from "~/lib/transport-context";
import { dockerErrorMessage } from "../_lib/presentation";

export function EngineChooser({
  settings,
  onChange,
  onClose,
  embedded = false,
}: {
  embedded?: boolean;
  settings: DockerSettings;
  onChange: (snapshot: DockerSnapshot) => void;
  onClose: () => void;
}) {
  const transport = useTransport();
  const [engines, setEngines] = useState<DockerEngine[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    transport
      .dockerDiscover()
      .then((r) => {
        if (active) {
          if (r.status === "ok") setEngines(r.data);
          else setError(dockerErrorMessage(r.error));
        }
      })
      .catch((e: unknown) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [transport]);
  const choose = async (engine: DockerEngine) => {
    setBusy(true);
    setError(null);
    try {
      const result = await transport.dockerConfigure({
        ...settings,
        enabled: true,
        selected_engine: engine.socket,
      });
      if (result.status === "ok") {
        onChange(result.data);
        onClose();
      } else setError(dockerErrorMessage(result.error));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section
      aria-label="Choose Docker engine"
      className={
        embedded ? "" : "rounded-card border border-border bg-card p-4"
      }
    >
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-[14px]">Local Docker engines</h2>
        {!embedded && (
          <Button variant="ghost" size="sm" onClick={onClose}>
            Close
          </Button>
        )}
      </div>
      <p className="mb-3 text-[12px] text-muted-foreground">
        Selection is independent of the Docker CLI context. Switching preserves
        the previous engine's history.
      </p>
      {error && (
        <p role="alert" className="text-[12px] text-destructive">
          {error}
        </p>
      )}
      {engines === null && !error && (
        <p role="status">Discovering local Unix sockets…</p>
      )}
      {engines?.length === 0 && (
        <p>
          No local engine found. Start Docker Desktop, OrbStack or Colima, then
          reopen the chooser.
        </p>
      )}
      {engines?.map((engine) => (
        <div
          key={engine.socket}
          className="flex flex-wrap items-center justify-between gap-3 border-border-subtle border-t py-3"
        >
          <div className="min-w-0">
            <div className="text-[13px]">
              {engine.runtime} · {engine.context ?? "Default socket"}
              {engine.preferred ? " · Current context" : ""}
            </div>
            <div className="break-all text-[11px] text-muted-foreground">
              {engine.socket}
            </div>
            <p className="text-[11px] text-muted-foreground">
              {engine.reachable ? "Reachable" : (engine.error ?? "Unavailable")}
            </p>
          </div>
          <Button
            size="sm"
            variant="outline"
            disabled={busy || !engine.reachable}
            onClick={() => void choose(engine)}
          >
            {settings.selected_engine === engine.socket
              ? "Use selected engine"
              : "Use this engine"}
          </Button>
        </div>
      ))}
    </section>
  );
}
