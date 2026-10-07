import { formatBytes, formatPercent, MISSING } from "@core/format";
import type { PerformanceReason, Settings } from "@core/generated/bindings";
import {
  approxSize,
  DEFAULT_SERIES,
  retentionProjection,
  sizeLimitLabel,
} from "@core/history-projection";
import { slowestIntervalMs } from "@core/live-window";
import { performanceChanges, performanceNextLever } from "@core/performance";
import { historyKeys, hostKeys } from "@core/query-keys";
import {
  INTERVALS_MS,
  RETENTION_DAYS,
  SIZE_LIMITS_MB,
} from "@core/settings-patch";
import { unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "~/components/confirm-dialog";
import { HistoryNotices } from "~/components/history-notices";
import { SegmentedControl } from "~/components/segmented-control";
import { SettingsPanel, SettingsRow } from "~/components/settings-row";
import { Button } from "~/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { Switch } from "~/components/ui/switch";
import { useHistoryHealth } from "~/hooks/use-history-health";
import { useProcessNetwork } from "~/hooks/use-process-interest";
import { useWriteSettings } from "~/hooks/use-write-settings";
import { useTransport } from "~/lib/transport-context";
import { useHost, useHostId } from "~/stores/host-store";
import { useSampling, useSeriesCount } from "~/stores/live-selectors";
import { useSettings } from "~/stores/settings-store";
import { intervalLabel, selfCpuAverage } from "../_lib/overhead";

const INTERVAL_OPTIONS = INTERVALS_MS.map((ms) => ({
  value: String(ms),
  label: intervalLabel(ms),
}));

const MB = (bytes: number) => formatBytes(bytes, { unit: "MB", decimals: 0 });

/**
 * "Sampling": Performance mode with what it changes (D-088),
 * interval, battery slow-down, retention with its projected size, the size
 * limit (D-059), Network history (D-089), history on disk, and the store's
 * low-disk and trim notices.
 */
export function SamplingPanel({
  sampling,
  history,
}: {
  sampling: Settings["sampling"];
  history: Settings["history"];
}) {
  const write = useWriteSettings();
  // Measured, not a constant: `self.cpu` under the current sampling setup.
  const overhead = useHost(selfCpuAverage);
  // The interval in effect, which battery back-off can lengthen.
  const liveInterval =
    useHost((s) => s.status?.interval_ms) ?? sampling.interval_ms;
  const series = useSeriesCount() ?? DEFAULT_SERIES;
  const limitBytes = history.size_limit_mb * 1e6;
  const limitText = sizeLimitLabel(history.size_limit_mb);
  const transport = useTransport();
  const hostId = useHostId();
  // Measured on this Mac; until an hour is recorded, the fill-test model. No
  // figure while the measurement loads, so the fill test's never flashes first.
  const growth = useQuery({
    queryKey: hostKeys.historyGrowth(hostId),
    queryFn: () => unwrap(transport.historyGrowth(hostId)),
  });
  const projection = (days: number) =>
    growth.isPending
      ? null
      : retentionProjection(days, limitBytes, series, growth.data);
  const current = projection(history.retention_days);
  const health = useHistoryHealth();
  // Per-app network history needs NetworkStatistics (D-089); hidden without it.
  const perAppNetwork = useProcessNetwork();
  const { performance } = useSampling();
  const modules = useSettings((s) => s.modules) ?? {};
  const byLowPower = performance === "low_power_mode";
  const performanceOn = !!sampling.performance_mode || byLowPower;

  return (
    <SettingsPanel
      title="Sampling"
      after={<HistoryNotices health={health.health} error={health.error} />}
    >
      <SettingsRow
        label="Performance mode"
        sub={
          byLowPower
            ? "On while Low Power Mode is on"
            : "Uses less CPU by updating less often and turning off animations"
        }
      >
        <Switch
          checked={performanceOn}
          disabled={byLowPower}
          aria-label="Performance mode"
          aria-describedby="settings-performance-changes"
          onCheckedChange={(on) =>
            write({ sampling: { performance_mode: on } })
          }
        />
        <PerformanceChanges
          sampling={sampling}
          modules={modules}
          reason={performance}
        />
      </SettingsRow>
      <SettingsRow
        label="Sample interval"
        sub={
          overhead === "measuring" ? (
            `Measuring Kelvo's CPU at ${intervalLabel(liveInterval)}…`
          ) : overhead !== null ? (
            <>
              Kelvo uses about{" "}
              <span className="data-mono">
                {formatPercent(overhead, { decimals: 1 })}
              </span>{" "}
              CPU at {intervalLabel(liveInterval)}, this window included
            </>
          ) : null
        }
      >
        <SegmentedControl
          ariaLabel="Sample interval"
          options={INTERVAL_OPTIONS}
          value={String(sampling.interval_ms)}
          onChange={(v) => write({ sampling: { interval_ms: Number(v) } })}
        />
      </SettingsRow>
      <SettingsRow
        label="Slow down on battery"
        sub={performanceOn && "Set by Performance mode"}
      >
        <span className="font-normal text-[12px] text-muted-foreground">
          to{" "}
          {intervalLabel(
            slowestIntervalMs({ ...sampling, slow_on_battery: true })
          )}
        </span>
        <Switch
          checked={sampling.slow_on_battery || performanceOn}
          disabled={performanceOn}
          aria-label="Slow down on battery"
          onCheckedChange={(on) => write({ sampling: { slow_on_battery: on } })}
        />
      </SettingsRow>
      <SettingsRow
        label="Keep history"
        htmlFor="settings-retention"
        sub={
          current?.limitedDays != null &&
          `Limited to about ${current.limitedDays} days by the ${limitText} limit`
        }
      >
        <span className="data-mono text-[12px] text-muted-foreground">
          {current ? `about ${approxSize(current.bytes)}` : MISSING}
        </span>
        <Select
          value={String(history.retention_days)}
          onValueChange={(v) =>
            write({ history: { retention_days: Number(v) } })
          }
        >
          <SelectTrigger
            id="settings-retention"
            size="sm"
            className="px-2 text-[12px]"
          >
            <SelectValue>{history.retention_days} days</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {RETENTION_DAYS.map((d) => {
              const p = projection(d);
              return (
                <SelectItem key={d} value={String(d)}>
                  {d} days
                  <span className="data-mono text-[11px] text-muted-foreground">
                    {p === null
                      ? MISSING
                      : p.limitedDays === null
                        ? `about ${approxSize(p.bytes)}`
                        : `${limitText}, about ${p.limitedDays} days`}
                  </span>
                </SelectItem>
              );
            })}
          </SelectContent>
        </Select>
      </SettingsRow>
      <SettingsRow label="History size limit" htmlFor="settings-size-limit">
        <Select
          value={String(history.size_limit_mb)}
          onValueChange={(v) =>
            write({ history: { size_limit_mb: Number(v) } })
          }
        >
          <SelectTrigger
            id="settings-size-limit"
            size="sm"
            className="px-2 text-[12px]"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SIZE_LIMITS_MB.map((mb) => (
              <SelectItem key={mb} value={String(mb)}>
                {sizeLimitLabel(mb)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingsRow>
      {perAppNetwork && (
        <SettingsRow
          label="Network history"
          sub="Keeps which apps used the network, in 10 s steps. Costs about 0.1% CPU when idle."
        >
          <Switch
            checked={history.network_history !== false}
            aria-label="Network history"
            onCheckedChange={(on) =>
              write({ history: { network_history: on } })
            }
          />
        </SettingsRow>
      )}
      <HistoryOnDisk />
    </SettingsPanel>
  );
}

/**
 * What Performance mode changes for this user, always shown so the cost reads
 * before the switch is turned on (D-088), then the next thing to change.
 */
function PerformanceChanges({
  sampling,
  modules,
  reason,
}: {
  sampling: Settings["sampling"];
  modules: Settings["modules"];
  reason: PerformanceReason;
}) {
  const changes = performanceChanges({ sampling, modules }, reason);
  const next = performanceNextLever({ sampling, modules });
  return (
    <div
      id="settings-performance-changes"
      className="-mt-2 basis-full pb-2.5 font-normal text-[12px] text-muted-foreground"
    >
      <ul className="m-0 list-none p-0">
        {changes.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ul>
      {next && <p className="m-0 mt-1.5">{next}</p>}
    </div>
  );
}

/** Size on disk (refreshed every 60 s while shown) and Clear with a confirm. */
function HistoryOnDisk() {
  const transport = useTransport();
  const hostId = useHostId();
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const [clearing, setClearing] = useState(false);
  const size = useQuery({
    queryKey: hostKeys.historySize(hostId),
    queryFn: () => unwrap(transport.historySize(hostId)),
    refetchInterval: 60_000,
  });

  const clear = async () => {
    setClearing(true);
    const result = await transport.clearHistory(hostId);
    setClearing(false);
    setConfirming(false);
    if (result.status === "error") {
      console.error("[settings] clear_history failed", {
        hostId,
        error: result.error,
      });
      toast.error("Couldn't clear history. Nothing was deleted.");
      return;
    }
    void queryClient.invalidateQueries({ queryKey: historyKeys.host(hostId) });
    void queryClient.invalidateQueries({
      queryKey: hostKeys.historySize(hostId),
    });
  };

  const sizeText = size.data === undefined ? MISSING : MB(size.data);
  return (
    <SettingsRow label="History on disk">
      <span className="data-mono text-[12px] text-fg-subtle">{sizeText}</span>
      <Button
        variant="outline"
        size="sm"
        disabled={size.data === 0}
        onClick={() => setConfirming(true)}
      >
        Clear
      </Button>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        className="sm:max-w-sm"
        title="Clear history?"
        description={
          <>
            Deletes the {sizeText} of history stored for this Mac. Live values
            keep updating, and the Timeline starts again from now. This can't be
            undone.
          </>
        }
        confirmLabel="Clear history"
        confirmVariant="destructive"
        onConfirm={clear}
        busy={clearing}
      />
    </SettingsRow>
  );
}
