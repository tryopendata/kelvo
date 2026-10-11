import { appendLogLines, emptyLogBuffer } from "@core/docker-log-buffer";
import { useEffect, useRef, useState } from "react";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import { useTransport } from "~/lib/transport-context";
import { dockerErrorMessage } from "../_lib/presentation";

export function ContainerLogs({
  engine,
  container,
  generation,
}: {
  engine: string;
  container: string;
  generation: number;
}) {
  const transport = useTransport();
  const [follow, setFollow] = useState(false);
  const [retry, setRetry] = useState(0);
  const [buffer, setBuffer] = useState(emptyLogBuffer);
  const [error, setError] = useState<string | null>(null);
  const [complete, setComplete] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const [newLines, setNewLines] = useState(0);
  const [paused, setPaused] = useState(false);
  const pausedRef = useRef(false);
  const scroll = useRef<HTMLElement>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: retry explicitly starts a fresh request after a failed subscription
  useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | undefined;
    setBuffer(emptyLogBuffer());
    setError(null);
    setComplete(false);
    setTruncated(false);
    setNewLines(0);
    transport
      .dockerLogs(
        { engine_id: engine, container_id: container, follow },
        (batch) => {
          if (cancelled || batch.generation !== generation) return;
          setBuffer((current) => appendLogLines(current, batch.lines));
          if (pausedRef.current) setNewLines((n) => n + batch.lines.length);
          if (batch.error) setError(batch.error);
          if (batch.truncated) setTruncated(true);
          setComplete(batch.complete);
        }
      )
      .then((subscription) => {
        if (cancelled) subscription.unsubscribe();
        else {
          stop = subscription.unsubscribe;
          if (subscription.info.status === "error")
            setError(dockerErrorMessage(subscription.info.error));
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [transport, engine, container, follow, retry, generation]);
  useEffect(() => {
    if (!paused && scroll.current && buffer.lines.length)
      scroll.current.scrollTop = scroll.current.scrollHeight;
  }, [buffer, paused]);
  return (
    <section aria-label="Container logs" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-[12px] text-muted-foreground">
          Latest 200 lines · kept in memory only ·{" "}
          {error
            ? "Stopped"
            : complete
              ? "Complete"
              : follow
                ? "Following"
                : "Loading"}
        </p>
        <label
          htmlFor="docker-log-follow"
          className="flex items-center gap-2 text-[12px]"
        >
          <Checkbox
            id="docker-log-follow"
            checked={follow}
            onCheckedChange={(checked) => setFollow(checked === true)}
          />
          Follow
        </label>
      </div>
      {error && (
        <p role="alert" className="text-[12px] text-destructive">
          {error}{" "}
          <Button
            variant="outline"
            size="sm"
            onClick={() => setRetry((n) => n + 1)}
          >
            {follow ? "Resume logs" : "Retry logs"}
          </Button>
        </p>
      )}
      {paused && (
        <Button
          variant="outline"
          size="sm"
          className="self-start"
          onClick={() => {
            pausedRef.current = false;
            setPaused(false);
            setNewLines(0);
          }}
        >
          Autoscroll paused · {newLines} new lines · Jump to latest
        </Button>
      )}
      <section
        ref={scroll}
        aria-label="Log output"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: keyboard scrolling needs a focusable log viewport
        tabIndex={0}
        className="max-h-80 overflow-auto rounded-control border border-border bg-deep p-3 text-[11px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onScroll={(e) => {
          const node = e.currentTarget;
          const up =
            node.scrollHeight - node.scrollTop - node.clientHeight > 24;
          pausedRef.current = up;
          setPaused(up);
          if (!up) setNewLines(0);
        }}
      >
        {buffer.lines.map((line) => (
          <p key={line.id} className="whitespace-pre-wrap break-all">
            {line.text}
            {line.truncated && (
              <span className="text-warning"> [line truncated at 64 KiB]</span>
            )}
          </p>
        ))}
        {!buffer.lines.length && (
          <p className="text-muted-foreground">
            {complete ? "No log output available." : "Waiting for log output…"}
          </p>
        )}
      </section>
      <p className="text-[11px] text-muted-foreground">
        Buffer capped at 2,000 lines or 2 MiB.
        {truncated
          ? " Docker log output was truncated; some lines are unavailable."
          : ""}
        {buffer.discarded > 0
          ? ` ${buffer.discarded} older lines discarded.`
          : ""}{" "}
        Closing this pane cancels the log request.
      </p>
    </section>
  );
}
