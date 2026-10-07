import {
  formatBytes,
  formatClock,
  formatDuration,
  formatPercent,
} from "@core/format";
import { useShallow } from "zustand/react/shallow";
import { MachineHeader, type MachineSpec } from "~/components/machine-header";
import { useHostRecord } from "~/hooks/use-host-record";
import { useNow } from "~/hooks/use-now";
import { useHost } from "~/stores/host-store";
import { useLastWake } from "../_hooks/use-overview-history";
import { coresLabel, marketingMemory } from "../_lib/card-props";
import { volumeCapacity } from "../_lib/selectors";

/** "Apple M4 Pro" reads as "M4 Pro" inside the title's parentheses. */
function shortChip(chip: string): string {
  return chip.replace(/^Apple\s+/, "");
}

/**
 * The Overview machine header from what the host reports. `HostInfo` has no
 * marketing name, model year, GPU core count, memory type or OS build yet,
 * so the title is the Mac's name with its chip, and those facts are left
 * out rather than guessed. Battery and storage come from the live store
 * (they change every 10 and 60 samples); uptime refreshes every minute.
 */
export function LiveMachineHeader() {
  const host = useHostRecord();
  const now = useNow(60_000);
  const lastWake = useLastWake();
  const boot = host?.info.boot_mounts?.[0] ?? null;
  const vol = useHost(useShallow((s) => volumeCapacity(s, boot)));
  const battery = useHost(
    useShallow((s) => ({
      charge: s.held["battery.charge"] ?? null,
      health: s.held["battery.health"] ?? null,
      cycles: s.held["battery.cycles"] ?? null,
      present:
        s.capabilities !== null &&
        s.capabilities.modules.battery !== undefined &&
        s.capabilities.modules.battery !== "not_present",
    }))
  );
  if (!host) return null;
  const info = host.info;
  const topology = info.cpu_topology;
  const cores = topology.reduce((n, c) => n + c.cores.length, 0);

  const chipParts = [
    info.chip,
    cores > 0 ? `${cores}-core CPU (${coresLabel(topology)})` : null,
  ].filter((p): p is string => Boolean(p));
  const specs: MachineSpec[] = [
    { label: "Chip", value: chipParts.join(" · ") || "—" },
    {
      label: "Memory",
      value: `${marketingMemory(info.mem_total_bytes)} unified`,
    },
    {
      label: "Storage",
      value:
        vol.total === null
          ? "—"
          : `${formatBytes(vol.total)} · ${formatBytes(vol.free)} available`,
    },
  ];
  if (battery.present) {
    specs.push({
      label: "Battery",
      value: `${formatPercent(battery.charge)} · health ${formatPercent(battery.health)} · ${battery.cycles === null ? "—" : Math.round(battery.cycles)} cycles`,
    });
  }
  specs.push({ label: "Model", value: info.model ?? "—" });
  specs.push({
    label: "Uptime",
    value: [
      formatDuration(now - info.boot_time_ms, { parts: 3 }),
      lastWake === null ? null : `last wake ${formatClock(lastWake)}`,
    ]
      .filter(Boolean)
      .join(" · "),
  });

  return (
    <MachineHeader
      title={
        info.chip
          ? `${host.display_name} (${shortChip(info.chip)})`
          : host.display_name
      }
      osVersion={`macOS ${info.os_version}`}
      specs={specs}
    />
  );
}
