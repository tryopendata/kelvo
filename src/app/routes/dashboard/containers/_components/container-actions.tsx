import type {
  DockerContainer,
  DockerContainerAction,
  DockerSnapshot,
  DockerStorageOperation,
} from "@core/generated/bindings";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "~/components/confirm-dialog";
import { Button } from "~/components/ui/button";
import { ContextMenuItem } from "~/components/ui/context-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "~/components/ui/tooltip";
import { useTransport } from "~/lib/transport-context";
import { cn } from "~/lib/utils";
import { dockerErrorMessage } from "../_lib/presentation";

export function containerAction(
  row: DockerContainer
): DockerContainerAction | null {
  if (["running", "restarting", "paused"].includes(row.lifecycle))
    return "stop";
  if (["exited", "created"].includes(row.lifecycle)) return "start";
  return null;
}

export function containerActionReason(
  row: DockerContainer,
  snapshot: DockerSnapshot | null
): string | null {
  if (snapshot?.status !== "connected" || snapshot.engine?.id !== row.engine_id)
    return "Connect to this engine before changing a container.";
  if (!snapshot.containers.some((current) => current.id === row.id))
    return "This container no longer exists.";
  if (containerAction(row) === "stop") {
    if (row.auto_remove === true)
      return "AutoRemove can delete the container and anonymous volumes on Stop.";
    if (row.auto_remove !== false)
      return "Host configuration is unknown. Refresh the container before stopping it.";
  }
  return null;
}

type Flow = {
  pendingIds: ReadonlySet<string>;
  failures: Readonly<Record<string, string>>;
  request: (row: DockerContainer) => void;
  reason: (row: DockerContainer) => string | null;
};
const idleFlow: Flow = {
  pendingIds: new Set(),
  failures: {},
  request: () => {},
  reason: () => "Actions unavailable",
};
const Context = createContext<Flow>(idleFlow);
export const useContainerActions = () => useContext(Context);

export function ContainerActionProvider({
  snapshot,
  children,
}: {
  snapshot: DockerSnapshot | null;
  children: ReactNode;
}) {
  const transport = useTransport();
  const [confirm, setConfirm] = useState<DockerContainer | null>(null);
  const [pending, setPending] = useState<{
    row: DockerContainer;
    action: DockerContainerAction;
  } | null>(null);
  const [operation, setOperation] = useState<DockerStorageOperation | null>(
    null
  );
  const [failures, setFailures] = useState<Record<string, string>>({});
  const busy = useRef(false);
  const mounted = useRef(true);
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());
  const currentSnapshot = useRef(snapshot);
  currentSnapshot.current = snapshot;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      for (const timer of timers.current) clearTimeout(timer);
    };
  }, []);
  const fail = useCallback((id: string, message: string) => {
    if (!mounted.current) return;
    setFailures((previous) => ({ ...previous, [id]: message }));
    toast.error(message);
    const timer = setTimeout(() => {
      timers.current.delete(timer);
      setFailures((previous) => {
        const next = { ...previous };
        delete next[id];
        return next;
      });
    }, 10_000);
    timers.current.add(timer);
  }, []);
  const finish = useCallback(
    (result: DockerStorageOperation) => {
      if (!mounted.current) return;
      for (const item of result.results)
        if (item.status !== "succeeded") fail(item.selection.id, item.message);
      busy.current = false;
      setPending(null);
      setOperation(null);
    },
    [fail]
  );
  const dispatch = async (row: DockerContainer) => {
    const action = containerAction(row);
    if (
      !action ||
      busy.current ||
      containerActionReason(row, currentSnapshot.current)
    )
      return;
    busy.current = true;
    setConfirm(null);
    setPending({ row, action });
    try {
      const result = await transport.dockerContainerAction(
        row.engine_id,
        row.id,
        action
      );
      if (!mounted.current) return;
      if (result.status === "error") {
        fail(row.id, dockerErrorMessage(result.error));
        busy.current = false;
        setPending(null);
        return;
      }
      if (result.data.finished_ms !== null) finish(result.data);
      else setOperation(result.data);
    } catch (error) {
      fail(row.id, `Action outcome unknown: ${String(error)}`);
      busy.current = false;
      setPending(null);
    }
  };
  useEffect(() => {
    if (!operation || operation.finished_ms !== null) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const result = await transport.dockerStorageOperation(operation.id);
        if (cancelled) return;
        if (result.status === "error") {
          fail(pending?.row.id ?? "", dockerErrorMessage(result.error));
          busy.current = false;
          setPending(null);
          setOperation(null);
          return;
        }
        if (result.data.finished_ms !== null) finish(result.data);
        else timer = setTimeout(() => void poll(), 250);
      } catch (error) {
        if (!cancelled) {
          fail(
            pending?.row.id ?? "",
            `Action outcome unknown: ${String(error)}`
          );
          busy.current = false;
          setPending(null);
          setOperation(null);
        }
      }
    };
    timer = setTimeout(() => void poll(), 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [operation, transport, fail, finish, pending?.row.id]);
  const reachedState =
    pending &&
    snapshot?.containers.find(
      (row) =>
        row.id === pending.row.id && row.engine_id === pending.row.engine_id
    )?.lifecycle === (pending.action === "stop" ? "exited" : "running");
  const pendingIds = new Set(pending && !reachedState ? [pending.row.id] : []);
  const reason = (row: DockerContainer) =>
    containerActionReason(row, snapshot) ??
    (busy.current ? "An operation is already running." : null);
  return (
    <Context.Provider
      value={{
        pendingIds,
        failures,
        reason,
        request: (row) => {
          if (reason(row)) return;
          if (containerAction(row) === "stop") setConfirm(row);
          else void dispatch(row);
        },
      }}
    >
      {children}
      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(open) => {
          if (!open) setConfirm(null);
        }}
        title={`Stop ${confirm?.name ?? "container"}?`}
        description="Docker sends SIGTERM and waits 30 seconds, then kills the container."
        confirmDisabled={
          confirm ? containerActionReason(confirm, snapshot) !== null : true
        }
        confirmLabel="Stop"
        confirmVariant="destructive"
        onConfirm={() => {
          if (confirm) void dispatch(confirm);
        }}
      />
    </Context.Provider>
  );
}

export function ContainerActionButton({
  row,
  reveal = false,
}: {
  row: DockerContainer;
  reveal?: boolean;
}) {
  const flow = useContainerActions();
  const action = containerAction(row);
  if (!action) return null;
  if (flow.pendingIds.has(row.id))
    return (
      <span className="text-[11px] text-muted-foreground">
        {action === "stop" ? "Stopping…" : "Starting…"}
      </span>
    );
  const reason = flow.reason(row);
  const button = (
    <Button
      variant="ghost"
      size="sm"
      aria-label={`${action === "stop" ? "Stop" : "Start"} ${row.name}`}
      aria-disabled={reason !== null || undefined}
      className={cn(
        "h-6 px-2 text-[11px] aria-disabled:opacity-50",
        reveal &&
          "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
      )}
      onClick={(event) => {
        event.stopPropagation();
        if (!reason) flow.request(row);
      }}
    >
      {action === "stop" ? "Stop" : "Start"}
    </Button>
  );
  return reason ? (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent>{reason}</TooltipContent>
    </Tooltip>
  ) : (
    button
  );
}

export function ContainerActionMenuItem({ row }: { row: DockerContainer }) {
  const flow = useContainerActions();
  const action = containerAction(row);
  if (!action) return null;
  const reason = flow.reason(row);
  return (
    <ContextMenuItem
      disabled={reason !== null}
      title={reason ?? undefined}
      onSelect={() => flow.request(row)}
    >
      {action === "stop" ? "Stop" : "Start"}
    </ContextMenuItem>
  );
}

export function ContainerActionFailure({ id }: { id: string }) {
  const message = useContainerActions().failures[id];
  return message ? (
    <p role="alert" className="text-[11px] text-destructive">
      {message}
    </p>
  ) : null;
}
