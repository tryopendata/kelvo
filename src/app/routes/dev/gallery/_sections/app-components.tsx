import type { LiveProcess } from "@core/generated/bindings";
import {
  DEFAULT_MENU_BAR,
  type TrayStyle,
  trayStyleMenuBar,
} from "@core/settings-patch";
import {
  type TrayReadings,
  type TrayUnits,
  trayLayout,
} from "@core/tray-layout";
import { type CSSProperties, useState } from "react";
import { CardGrid } from "~/components/card-grid";
import { CollectingOverlay } from "~/components/collecting-overlay";
import { InterfaceTable } from "~/components/interface-table";
import { MachineHeader } from "~/components/machine-header";
import {
  type ModuleToggleItem,
  ModuleToggleList,
} from "~/components/module-toggle-list";
import { PopoverFooter } from "~/components/popover-footer";
import { PopoverHeader } from "~/components/popover-header";
import { type ProcessSort, ProcessTable } from "~/components/process-table";
import { SegmentedControl } from "~/components/segmented-control";
import { SettingsRow } from "~/components/settings-row";
import { Sidebar, type SidebarProps } from "~/components/sidebar";
import { TrayPreview } from "~/components/tray-preview";
import {
  TrayStyleGroup,
  TrayStyleOption,
} from "~/components/tray-style-option";
import { Button } from "~/components/ui/button";
import { Switch } from "~/components/ui/switch";
import { UnsupportedNotice } from "~/components/unsupported-notice";
import { VolumeTable } from "~/components/volume-table";
import { type ZoneRow, ZoneTable } from "~/components/zone-table";
import type { Accent } from "~/widgets/lib/accent";
import { GalleryItem } from "../_components/gallery-item";

const noop = () => {};

// Sidebar values.
const SIDEBAR: SidebarProps = {
  groups: [
    {
      id: "nav",
      entries: [
        {
          id: "overview",
          href: "/dashboard/overview",
          label: "Overview",
          icon: "overview",
        },
        {
          id: "timeline",
          href: "/dashboard/timeline",
          label: "Timeline",
          icon: "timeline",
        },
      ],
    },
    {
      id: "modules",
      entries: [
        {
          id: "cpu",
          href: "/dashboard/cpu",
          label: "CPU",
          icon: "cpu",
          value: "18%",
        },
        {
          id: "gpu",
          href: "/dashboard/gpu",
          label: "GPU",
          icon: "gpu",
          value: "36%",
        },
        {
          id: "memory",
          href: "/dashboard/memory",
          label: "Memory",
          icon: "memory",
          value: "42%",
        },
        {
          id: "power",
          href: "/dashboard/power",
          label: "Power & Sensors",
          icon: "power",
          value: "14.8W",
        },
        {
          id: "network",
          href: "/dashboard/network",
          label: "Network",
          icon: "network",
          value: "38.4M",
        },
        {
          id: "disk",
          href: "/dashboard/disk",
          label: "Disk",
          icon: "disk",
          value: "220M",
        },
        {
          id: "battery",
          href: "/dashboard/battery",
          label: "Battery",
          icon: "battery",
          value: "87%",
        },
      ],
    },
    {
      id: "tools",
      entries: [
        {
          id: "processes",
          href: "/dashboard/processes",
          label: "Processes",
          icon: "processes",
        },
        {
          id: "settings",
          href: "/dashboard/settings",
          label: "Settings",
          icon: "settings",
        },
      ],
    },
  ],
  active: "overview",
  status: { intervalMs: 1000, paused: false, onBattery: false },
  version: "0.4.0",
};

const SIDEBAR_DISABLED_DISK: SidebarProps = {
  ...SIDEBAR,
  active: "cpu",
  groups: SIDEBAR.groups.map((g) => ({
    ...g,
    entries: g.entries.map((e) =>
      e.id === "disk" ? { ...e, disabled: true } : e
    ),
  })),
  status: { intervalMs: 2000, paused: false, onBattery: true },
};

// Overview machine header.
const SPECS = [
  {
    label: "Chip",
    value: "Apple M4 Pro · 14-core CPU (10P + 4E) · 20-core GPU",
  },
  { label: "Memory", value: "24 GB unified · LPDDR5X" },
  { label: "Storage", value: "1 TB SSD · 612 GB available" },
  { label: "Battery", value: "87% · health 94% · 212 cycles" },
  { label: "Model", value: "Mac16,8 · Nov 2024" },
  { label: "Uptime", value: "3d 4h 12m · last wake 06:55" },
];

// Power & Sensors zones.
const ZONE_VALUES = [61, 59, 58, 56, 55, 53, 52, 50, 49, 48];
const ZONE_KEYS = [
  "PMU tdie4",
  "PMU tdie1",
  "PMU tdie6",
  "PMU2 tdie2",
  "PMU tdie3",
  "PMU2 tdie1",
  "PMU tdie5",
  "PMU2 tdie4",
  "PMU tdie2",
  "PMU2 tdie3",
];
const ZONES: ZoneRow[] = ZONE_VALUES.map((v, i) => ({
  key: ZONE_KEYS[i] as string,
  name: `Zone ${String(i + 1).padStart(2, "0")}`,
  now: v,
  min: v - 9,
  max: v + 4,
}));

// CPU page top processes.
const PROCS: [string, number, number, number, number, number, string][] = [
  ["Xcode", 2214, 86.8, 74, 312, 41.2, "me"],
  ["kernel_task", 0, 43.4, 612, 1904, 18.8, "root"],
  ["WindowServer", 391, 33.6, 22, 840, 6.1, "_windowserver"],
  ["Safari", 1840, 25.2, 41, 226, 9.6, "me"],
  ["node", 5531, 15.4, 12, 64, 2.4, "me"],
  ["Figma", 1712, 12.6, 38, 118, 7.4, "me"],
  ["com.docker.backend", 988, 9.8, 29, 402, 3.1, "me"],
  ["mds_stores", 466, 4.2, 7, 12, 0.8, "root"],
];
const PROCESS_ROWS: LiveProcess[] = PROCS.map(
  ([name, pid, cpu, threads, wakeups, energy, user]) => ({
    pid,
    start_time_us: 1_759_000_000_000_000 + pid,
    name,
    cpu_pct: cpu,
    mem_bytes: 0,
    compressed_bytes: null,
    threads,
    idle_wakeups_per_s: wakeups,
    energy,
    disk_read_bps: null,
    disk_write_bps: null,
    net_rx_bps: null,
    net_tx_bps: null,
    gpu_pct: null,
    ports: null,
    user,
    refusal:
      pid === 0
        ? "kernel_task"
        : name === "WindowServer"
          ? "window_server"
          : null,
  })
);

// Onboarding modules (Battery shown as absent to exercise that state).
const MODULES: ModuleToggleItem[] = [
  {
    id: "cpu",
    label: "CPU",
    description: "Load, clusters, per-core",
    accent: "cpu",
    enabled: true,
    available: true,
  },
  {
    id: "gpu",
    label: "GPU",
    description: "Utilization and frequency",
    accent: "gpu",
    enabled: true,
    available: true,
  },
  {
    id: "memory",
    label: "Memory",
    description: "Pressure, composition, swap",
    accent: "mem",
    enabled: true,
    available: true,
  },
  {
    id: "power",
    label: "Power & Sensors",
    description: "Watts, SoC zones, fans",
    accent: "power",
    enabled: true,
    available: true,
  },
  {
    id: "network",
    label: "Network",
    description: "Rates per interface",
    accent: "net",
    enabled: true,
    available: true,
  },
  {
    id: "disk",
    label: "Disk",
    description: "Throughput and capacity",
    accent: "disk",
    enabled: false,
    available: true,
  },
  {
    id: "battery",
    label: "Battery",
    description: "Charge, health, cycles",
    accent: "battery",
    enabled: true,
    available: false,
  },
];

const TRAY: TrayReadings = {
  cpu: 18,
  gpu: 36,
  mem: 42,
  temp: 61,
  power: 14.8,
  netUp: 1.2e6,
  netDown: 38.4e6,
  diskRate: 4.1e6,
  diskUsed: 62,
  battery: 87,
  cpuHistory: [12, 18, 30, 22, 15, 40, 55, 32, 18, 20, 26, 18],
};
const TRAY_GAPS: TrayReadings = {
  cpu: null,
  gpu: null,
  mem: null,
  temp: null,
  power: null,
  netUp: null,
  netDown: null,
  diskRate: null,
  diskUsed: null,
  battery: null,
};
const TRAY_UNITS: TrayUnits = {
  temperature: "celsius",
  network: "bytes_per_sec",
};
const ALL_ON = () => true;
const trayPreset = (style: TrayStyle, v = TRAY) =>
  trayLayout(trayStyleMenuBar(style), ALL_ON, v, TRAY_UNITS);
const TRAY_ALL_READOUTS = trayLayout(
  {
    ...DEFAULT_MENU_BAR,
    readouts: {
      cpu: true,
      gpu: true,
      memory: true,
      temperature: true,
      power: true,
      network: true,
      disk: true,
      battery: true,
    },
  },
  ALL_ON,
  TRAY,
  TRAY_UNITS
);
const TRAY_POWER_DISK = trayLayout(
  {
    ...DEFAULT_MENU_BAR,
    readouts: { ...DEFAULT_MENU_BAR.readouts, power: true, disk: true },
  },
  ALL_ON,
  TRAY,
  TRAY_UNITS
);

const GRID_ITEMS: { id: string; label: string; accent: Accent }[] = [
  { id: "cpu", label: "CPU", accent: "cpu" },
  { id: "gpu", label: "GPU", accent: "gpu" },
  { id: "mem", label: "Memory", accent: "mem" },
  { id: "power", label: "Power & Sensors", accent: "power" },
  { id: "net", label: "Network", accent: "net" },
  { id: "disk", label: "Disk", accent: "disk" },
];

const CORNER_POS = {
  tl: "0% 0%",
  tr: "100% 0%",
  bl: "0% 100%",
  br: "100% 100%",
};

const panel = "overflow-hidden rounded-card border border-border bg-card";

/** App components outside widgets/: tables, navigation, settings, states. */
export function AppComponentsSection() {
  const [sort, setSort] = useState<ProcessSort>({ by: "cpu", dir: "desc" });
  const [intervalMs, setIntervalMs] = useState("1000");
  const [temp, setTemp] = useState<"C" | "F">("C");
  const [style, setStyle] = useState<TrayStyle>("combined");
  const [modules, setModules] = useState(MODULES);

  return (
    <div className="flex flex-col gap-8">
      <div className="flex flex-wrap items-start gap-8">
        <GalleryItem name="Sidebar" usedIn="Dashboard sidebar" width={220}>
          <div className="h-[560px]">
            <Sidebar {...SIDEBAR} onNavigate={noop} />
          </div>
        </GalleryItem>
        <GalleryItem
          name="Sidebar · disabled Disk, on battery"
          usedIn="Dashboard sidebar"
          width={220}
        >
          <div className="h-[560px]">
            <Sidebar {...SIDEBAR_DISABLED_DISK} onNavigate={noop} />
          </div>
        </GalleryItem>
        <div className="flex flex-col gap-8">
          <GalleryItem
            name="PopoverHeader · PopoverFooter"
            usedIn="Popover"
            width={360}
            vibrant
          >
            <PopoverHeader
              hostName="MacBook Pro"
              uptimeMs={(3 * 24 + 4) * 3_600_000 + 12 * 60_000}
              status={{ state: "live", label: "1s" }}
              paused={false}
              onPause={noop}
              onSettings={noop}
            />
            <div className="h-10" />
            <PopoverFooter
              selfCpuPct={0.4}
              onOpenDashboard={noop}
              onActivity={noop}
            />
          </GalleryItem>
          <GalleryItem
            name="PopoverHeader · paused, scrolled"
            usedIn="Popover, scrolled"
            width={360}
            vibrant
          >
            <PopoverHeader
              hostName="MacBook Pro"
              uptimeMs={(3 * 24 + 4) * 3_600_000}
              status={{ state: "paused", label: "Paused" }}
              paused
              scrolled
              onPause={noop}
              onSettings={noop}
            />
          </GalleryItem>
          <GalleryItem name="SegmentedControl" usedIn="Settings">
            <div className="flex items-center gap-4">
              <SegmentedControl
                ariaLabel="Sample interval"
                options={[
                  { value: "500", label: "0.5s" },
                  { value: "1000", label: "1s" },
                  { value: "2000", label: "2s" },
                  { value: "5000", label: "5s" },
                ]}
                value={intervalMs}
                onChange={setIntervalMs}
              />
              <SegmentedControl
                ariaLabel="Temperature unit"
                options={[
                  { value: "C", label: "°C" },
                  { value: "F", label: "°F" },
                ]}
                value={temp}
                onChange={setTemp}
              />
            </div>
          </GalleryItem>
        </div>
      </div>

      <GalleryItem name="MachineHeader" usedIn="Overview">
        <MachineHeader
          title="MacBook Pro 14-inch (M4 Pro, 2024)"
          osVersion="macOS 27.0.1"
          osBuild="27A5320"
          specs={SPECS}
        />
      </GalleryItem>

      <GalleryItem name="CardGrid" usedIn="Overview">
        <CardGrid items={GRID_ITEMS} getKey={(m) => m.id}>
          {(m, origin) => (
            <section
              aria-label={m.label}
              className="vt-card flex h-20 items-start justify-between p-4"
              style={
                {
                  "--a": `var(--color-${m.accent})`,
                  "--o": CORNER_POS[origin],
                } as CSSProperties
              }
            >
              <span className="font-[590] text-[13px]">{m.label}</span>
              <span className="figures text-[11px] text-muted-foreground">
                {origin}
              </span>
            </section>
          )}
        </CardGrid>
      </GalleryItem>

      <div className="grid grid-cols-2 items-start gap-8">
        <GalleryItem name="ZoneTable" usedIn="Power & Sensors">
          <div
            className="vt-card p-4"
            style={{ "--a": "var(--color-power)" } as CSSProperties}
          >
            <ZoneTable
              rows={ZONES}
              units="C"
              rangeLabel="15m"
              extras={[
                { label: "Battery", value: 31 },
                { label: "SSD (NAND)", value: 39 },
                { label: "Wi‑Fi module", value: 44 },
              ]}
            />
          </div>
        </GalleryItem>
        <div className="flex flex-col gap-8">
          <GalleryItem name="InterfaceTable" usedIn="Network">
            <div className={panel}>
              <InterfaceTable
                units="MBps"
                rows={[
                  {
                    id: "en0",
                    kind: "Wi‑Fi",
                    rxBps: 38_400_000,
                    txBps: 1_200_000,
                    rxBytes: 182_000_000_000,
                    txBytes: 9_400_000_000,
                  },
                  {
                    id: "utun4",
                    kind: "VPN",
                    rxBps: 21_000,
                    txBps: 4_000,
                    rxBytes: 640_000_000,
                    txBytes: 120_000_000,
                  },
                  {
                    id: "en5",
                    kind: "Ethernet",
                    rxBps: null,
                    txBps: null,
                    rxBytes: 0,
                    txBytes: 0,
                    disconnected: true,
                  },
                ]}
              />
            </div>
          </GalleryItem>
          <GalleryItem name="VolumeTable" usedIn="Disk">
            <div className={panel}>
              <VolumeTable
                units="GB"
                rows={[
                  {
                    id: "disk3s5",
                    name: "Data",
                    usedBytes: 330e9,
                    freeBytes: 612e9,
                    totalBytes: 1000e9,
                  },
                  {
                    id: "disk3s1",
                    name: "Macintosh HD",
                    usedBytes: 58e9,
                    freeBytes: 612e9,
                    totalBytes: 1000e9,
                  },
                  {
                    id: "disk5s1",
                    name: "Backup",
                    usedBytes: null,
                    freeBytes: null,
                    totalBytes: null,
                    disconnected: true,
                  },
                ]}
              />
            </div>
          </GalleryItem>
        </div>
      </div>

      <GalleryItem name="ProcessTable" usedIn="CPU">
        <div
          className="vt-card flex flex-col"
          style={{ "--a": "var(--color-cpu)" } as CSSProperties}
        >
          <div className="flex items-center gap-3 px-4 pt-3.5 pb-1.5">
            <h3 className="m-0 flex-1 font-[590] text-[14px]">Top processes</h3>
            <span className="font-normal text-[11px] text-muted-foreground">
              % CPU is of one core; 14 cores = 1400%
            </span>
          </div>
          <ProcessTable
            ariaLabel="Top processes"
            rows={PROCESS_ROWS}
            columns={[
              "name",
              "pid",
              "cpu",
              "threads",
              "wakeups",
              "energy",
              "user",
            ]}
            sort={sort}
            onSort={setSort}
            cpuBarMaxPct={140}
            height={8 * 29}
          />
        </div>
      </GalleryItem>

      <div className="grid grid-cols-2 items-start gap-8">
        <GalleryItem name="ModuleToggleList" usedIn="Onboarding">
          <ModuleToggleList
            modules={modules}
            onToggle={(id, enabled) =>
              setModules((ms) =>
                ms.map((m) => (m.id === id ? { ...m, enabled } : m))
              )
            }
          />
        </GalleryItem>
        <GalleryItem
          name="TrayStyleOption · TrayPreview"
          usedIn="Onboarding · menu bar"
        >
          <TrayStyleGroup
            value={style}
            onValueChange={setStyle}
            aria-label="Menu bar style"
            className="flex flex-col gap-2"
          >
            <TrayStyleOption
              style="combined"
              title="Combined"
              description="One item. Smallest footprint."
              recommended
              layout={trayPreset("combined")}
            />
            <TrayStyleOption
              style="graphs"
              title="Graph per module"
              description="Separate items you can reorder with ⌘-drag."
              layout={trayPreset("graphs")}
            />
            <TrayStyleOption
              style="values"
              title="Values only"
              description="Numbers with stacked labels."
              layout={trayPreset("values")}
            />
          </TrayStyleGroup>
          <TrayPreview layout={TRAY_POWER_DISK} />
          <TrayPreview layout={TRAY_ALL_READOUTS} />
          <TrayPreview layout={trayPreset("combined", TRAY_GAPS)} />
        </GalleryItem>
      </div>

      <div className="grid grid-cols-2 items-start gap-8">
        <GalleryItem name="SettingsRow" usedIn="Settings">
          <div className={panel}>
            <SettingsRow
              label="Sample interval"
              sub="Kelvo uses about 0.4% CPU at 1s"
            >
              <SegmentedControl
                ariaLabel="Sample interval (row)"
                options={[
                  { value: "500", label: "0.5s" },
                  { value: "1000", label: "1s" },
                  { value: "2000", label: "2s" },
                  { value: "5000", label: "5s" },
                ]}
                value={intervalMs}
                onChange={setIntervalMs}
              />
            </SettingsRow>
            <SettingsRow label="Slow down on battery">
              <span className="font-normal text-[12px] text-muted-foreground">
                to 2s
              </span>
              <Switch defaultChecked aria-label="Slow down on battery" />
            </SettingsRow>
            <SettingsRow label="History on disk">
              <span className="figures text-[12px] text-fg-subtle">148 MB</span>
              <Button variant="outline" size="sm">
                Clear
              </Button>
            </SettingsRow>
          </div>
        </GalleryItem>
        <div className="flex flex-col gap-8">
          <GalleryItem name="CollectingOverlay" usedIn="Empty and gap states">
            <div className="relative h-36 rounded-card border border-border bg-card">
              <div className="absolute inset-x-4 bottom-6 border-axis border-t border-dashed" />
              <CollectingOverlay
                recordedMs={4 * 60_000}
                retentionDays={30}
                className="absolute inset-0"
              />
            </div>
          </GalleryItem>
          <GalleryItem name="UnsupportedNotice" usedIn="Empty and gap states">
            <UnsupportedNotice modelId="Mac17,4" onShareDump={noop} />
          </GalleryItem>
        </div>
      </div>
    </div>
  );
}
