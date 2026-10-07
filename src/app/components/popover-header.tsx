import { formatDuration } from "@core/format";
import type { PerformanceReason } from "@core/generated/bindings";
import { Pause, Play, SlidersHorizontal } from "lucide-react";
import { KelvoMark } from "~/components/kelvo-mark";
import { PerformanceExplainer } from "~/components/performance-explainer";
import { Button } from "~/components/ui/button";
import { cn } from "~/lib/utils";
import { StatusPill, type StatusPillProps } from "~/widgets/status-pill";

export interface PopoverHeaderProps {
  /** `HostRecord.display_name`, "MacBook Pro". */
  hostName: string;
  /** Now minus `HostInfo.boot_time_ms`. */
  uptimeMs: number;
  /** Interval pill: live "1s", "Paused" or "Stale". */
  status: StatusPillProps;
  paused: boolean;
  /** Performance mode and why: the pill explains itself on hover and focus (D-088). */
  performance?: PerformanceReason;
  /** The card column has scrolled: the header gains a bottom border. */
  scrolled?: boolean;
  onPause: () => void;
  onSettings: () => void;
}

/** Popover header: app title, host and uptime, interval pill, pause, settings. */
export function PopoverHeader({
  hostName,
  uptimeMs,
  status,
  paused,
  performance = "off",
  scrolled = false,
  onPause,
  onSettings,
}: PopoverHeaderProps) {
  return (
    <header
      className={cn(
        "flex items-center gap-2 border-b py-2.5 pr-3 pl-4 transition-colors duration-(--motion-fast)",
        scrolled ? "border-border" : "border-transparent"
      )}
    >
      <KelvoMark className="size-6" />
      <div className="flex min-w-0 flex-1 flex-col gap-px">
        <h1 className="m-0 font-[590] text-[13px] tracking-[-0.01em]">Kelvo</h1>
        <span className="truncate font-normal text-[11px] text-muted-foreground">
          {hostName} · up{" "}
          <span className="data-mono">{formatDuration(uptimeMs)}</span>
        </span>
      </div>
      {performance === "off" ? (
        <StatusPill {...status} />
      ) : (
        <PerformanceExplainer
          reason={performance}
          side="bottom"
          onOpenSettings={onSettings}
          label={status.label}
          className="rounded-full"
        >
          <StatusPill {...status} />
        </PerformanceExplainer>
      )}
      <Button
        variant="outline"
        size="icon"
        aria-label={paused ? "Resume sampling" : "Pause sampling"}
        aria-pressed={paused}
        onClick={onPause}
      >
        {paused ? <Play strokeWidth={1.5} /> : <Pause strokeWidth={1.5} />}
      </Button>
      <Button
        variant="outline"
        size="icon"
        aria-label="Settings"
        onClick={onSettings}
      >
        <SlidersHorizontal strokeWidth={1.5} />
      </Button>
    </header>
  );
}
