import { formatPercent, formatWatts } from "@core/format";
import type { UiModule } from "@core/module-state";
import { useCallback } from "react";
import { useLocation, useNavigate } from "react-router";
import { PerformanceExplainer } from "~/components/performance-explainer";
import { Sidebar, type SidebarEntry } from "~/components/sidebar";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useModuleStates } from "~/hooks/use-module-states";
import { HostSwitcher } from "~/routes/dashboard/hosts/_components/host-switcher";
import {
  useBattery,
  useCpu,
  useDisk,
  useGpu,
  useMemory,
  useNetwork,
  usePower,
  useSampling,
} from "~/stores/live-selectors";
import { useSettings } from "~/stores/settings-store";
import { version } from "../../../../../package.json";
import { batteryBackoff } from "../_lib/sampling-status";

const NAV_ENTRIES: readonly SidebarEntry[] = [
  {
    id: "overview",
    href: "/dashboard/overview",
    label: "Overview",
    icon: "overview" as const,
  },
  {
    id: "timeline",
    href: "/dashboard/timeline",
    label: "Timeline",
    icon: "timeline" as const,
  },
];
const LOCAL_TOOLS: readonly SidebarEntry[] = [
  {
    id: "hosts",
    href: "/dashboard/hosts",
    label: "Hosts",
    icon: "hosts" as const,
  },
  {
    id: "processes",
    href: "/dashboard/processes",
    label: "Processes",
    icon: "processes" as const,
  },
  {
    id: "containers",
    href: "/dashboard/containers",
    label: "Containers",
    icon: "containers" as const,
  },
  {
    id: "settings",
    href: "/dashboard/settings",
    label: "Settings",
    icon: "settings" as const,
  },
];
const REMOTE_TOOLS = LOCAL_TOOLS.filter((entry) => entry.id !== "containers");

/** MB/s, no space ("38.4M", "220M"). */
export function compactRate(bps: number | null): string | undefined {
  if (bps === null) return undefined;
  const mb = bps / 1e6;
  return `${mb >= 100 ? mb.toFixed(0) : mb.toFixed(1)}M`;
}

/** Both directions, or null when either is missing (a part is not the total). */
export const sum = (a: number | null, b: number | null) =>
  a === null || b === null ? null : a + b;

/**
 * Dashboard sidebar with live values (plan 4.4). Modules the host
 * lacks are omitted, and so is Power & Sensors on an unknown chip (4.17).
 * Modules switched off, or that this build cannot run, are dimmed without a
 * value. Network and Disk show the sum of both directions.
 */
export function LiveSidebar() {
  const local = useIsLocalHost();
  const navigate = useNavigate();
  const onNavigate = useCallback((href: string) => navigate(href), [navigate]);
  const { pathname } = useLocation();
  const states = useModuleStates();
  const configuredMs = useSettings((s) => s.sampling.interval_ms);
  const sampling = useSampling();
  const cpu = useCpu();
  const gpu = useGpu();
  const mem = useMemory();
  const power = usePower();
  const net = useNetwork();
  const disk = useDisk();
  const battery = useBattery();
  const performanceLabel =
    sampling.performance === "low_power_mode"
      ? "Performance mode · Low Power"
      : "Performance mode";

  const module = (
    id: UiModule,
    label: string,
    icon: SidebarEntry["icon"],
    value: string | undefined
  ): SidebarEntry[] => {
    const state = states[id];
    if (state === "absent" || state === "unknown_chip") return [];
    const on = state === "on";
    return [
      {
        id,
        href: `/dashboard/${id}`,
        label,
        icon,
        value: on ? value : undefined,
        disabled: !on,
      },
    ];
  };

  const groups = [
    {
      id: "nav",
      entries: NAV_ENTRIES,
    },
    {
      id: "modules",
      entries: [
        ...module("cpu", "CPU", "cpu", formatPercent(cpu.total)),
        ...module("gpu", "GPU", "gpu", formatPercent(gpu.util)),
        ...module("memory", "Memory", "memory", formatPercent(mem.pressure)),
        ...module(
          "power",
          "Power & Sensors",
          "power",
          formatWatts(power.system, { compact: true })
        ),
        ...module(
          "network",
          "Network",
          "network",
          compactRate(sum(net.rx, net.tx))
        ),
        ...module(
          "disk",
          "Disk",
          "disk",
          compactRate(sum(disk.read, disk.write))
        ),
        ...module(
          "battery",
          "Battery",
          "battery",
          formatPercent(battery.charge)
        ),
      ],
    },
    {
      id: "tools",
      entries: local ? LOCAL_TOOLS : REMOTE_TOOLS,
    },
  ];

  return (
    <Sidebar
      hostSwitcher={<HostSwitcher />}
      groups={groups}
      active={pathname.split("/")[2] ?? "overview"}
      status={{
        intervalMs: sampling.intervalMs ?? configuredMs ?? 1000,
        paused: sampling.paused,
        stale: sampling.stale,
        onBattery: batteryBackoff({
          onBattery: sampling.onBattery,
          intervalMs: sampling.intervalMs,
          configuredMs,
        }),
      }}
      version={version}
      onNavigate={onNavigate}
      footerNote={
        <PerformanceExplainer
          reason={sampling.performance}
          label={performanceLabel}
          side="right"
          onOpenSettings={() => navigate("/dashboard/settings")}
          className="ml-3.5 self-start text-left font-normal text-[11px] text-muted-foreground"
        >
          {performanceLabel}
        </PerformanceExplainer>
      }
    />
  );
}
