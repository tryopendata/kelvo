/**
 * The `linux` scenario: the mock speaks for a headless Linux agent (D-116),
 * modelled on linux-tailscale, a Mac mini (M1, 2020) running Fedora Asahi
 * Remix 41 Server. Its host facts, capabilities, series and processes are
 * what `kelvo-collect/src/linux/` and `host_info/linux.rs` publish there:
 * E/P clusters from cpufreq policies, PSI, hwmon sensors named by their
 * kernel label, three block devices, cgroup v2 services and containers, and
 * no GPU or battery.
 */
import type {
  Capabilities,
  ClusterInfo,
  HostInfo,
  LiveProcess,
  Module,
} from "@core/generated/bindings";

/** M1 cpufreq `scaling_available_frequencies`, MHz ascending. */
const E_DVFS = [600, 972, 1332, 1704, 2064];
const P_DVFS = [
  600, 828, 1056, 1296, 1524, 1752, 1980, 2208, 2448, 2676, 2904, 3036, 3132,
  3168, 3204,
];
const E_CORES = ["E0", "E1", "E2", "E3"];
const P_CORES = ["P0", "P1", "P2", "P3"];

/** One cluster per cpufreq policy, in policy order (policy0 is E). */
export const LINUX_TOPOLOGY: ClusterInfo[] = [
  { name: "E0", kind: "efficiency", cores: E_CORES, dvfs_mhz: E_DVFS },
  { name: "P0", kind: "performance", cores: P_CORES, dvfs_mhz: P_DVFS },
];

/** MemTotal on linux-tailscale: 16,065,952 KiB. */
export const LINUX_MEM_TOTAL_BYTES = 16_065_952 * 1024;

/** Uptime on the server: 12 days 6 hours. */
const LINUX_UPTIME_MS = (12 * 24 + 6) * 3_600_000;

export const LINUX_HOST_NAME = "linux-tailscale";

export function linuxHostInfo(nowMs: number): HostInfo {
  return {
    os: "linux",
    os_version: "Fedora Linux Asahi Remix 41 (Server Edition)",
    kernel: "6.17.12-400.asahi.fc41.aarch64+16k",
    model: "Apple Mac mini (M1, 2020)",
    chip: "Apple M1",
    chip_known: true,
    cpu_topology: LINUX_TOPOLOGY,
    mem_total_bytes: LINUX_MEM_TOTAL_BYTES,
    boot_time_ms: nowMs - LINUX_UPTIME_MS,
    gpu_dvfs_mhz: [],
    boot_mounts: ["/"],
  };
}

export function linuxCapabilities(
  seriesPerModule: Record<string, number>,
  revision: number
): Capabilities {
  const available = (m: Module) => ({
    available: { series: seriesPerModule[m] ?? 0 },
  });
  return {
    revision,
    modules: {
      cpu: available("cpu"),
      // The asahi driver exposes no utilisation; DRM fdinfo is a follow-up.
      gpu: { unsupported: "no_hardware" },
      memory: available("memory"),
      power: available("power"),
      sensors: available("sensors"),
      network: available("network"),
      disk: available("disk"),
      battery: "not_present",
      containers: available("containers"),
    },
    process_network: false,
    process_gpu: false,
    process_energy: false,
    process_wakeups: false,
    // The agent runs as root, so it reads every `/proc/<pid>/io`.
    process_disk_io: true,
    disk_devices: [
      {
        dev: "nvme0n1",
        name: "APPLE SSD AP0512Q",
        internal: true,
        default: true,
      },
      { dev: "sda", name: "Samsung PSSD T7", internal: false, default: false },
      { dev: "sdb", name: "WDC WD40EFRX-68N", internal: null, default: false },
    ],
  };
}

type Add = (
  module: Module,
  metric: string,
  labels: Record<string, string>,
  base: number,
  opts?: {
    noise?: number;
    min?: number;
    max?: number;
    cadence?: number;
    periodMs?: number;
    perTick?: number;
    silent?: boolean;
  }
) => void;
type Total = (module: Module, metric: string, part: string) => void;

const GHZ = 1e9;
const pct = { min: 0, max: 100 };

/** Per-core load, E then P: an idle server. */
const CORE_LOAD = [4, 3, 2, 3, 2, 1, 1, 2];

function cpu(add: Add) {
  add("cpu", "cpu.total", {}, 3.1, { noise: 1.5, min: 0.5, max: 100 });
  add("cpu", "cpu.user", {}, 2.0, { noise: 1, ...pct });
  add("cpu", "cpu.system", {}, 0.9, { noise: 0.5, ...pct });
  add("cpu", "cpu.iowait", {}, 0.4, { noise: 0.3, ...pct });
  add("cpu", "cpu.steal", {}, 0, pct);
  [...E_CORES, ...P_CORES].forEach((core, i) => {
    add("cpu", "cpu.load", { core }, CORE_LOAD[i] ?? 2, { noise: 3, ...pct });
  });
  for (const [window, v] of [
    ["1", 0.42],
    ["5", 0.36],
    ["15", 0.31],
  ] as const)
    add("cpu", "cpu.loadavg", { window }, v, { noise: 0.08, cadence: 5 });
  add("cpu", "cpu.cluster.freq", { cluster: "E0" }, 1.33 * GHZ, {
    noise: 0.3 * GHZ,
    min: 0.6 * GHZ,
    max: 2.064 * GHZ,
    cadence: 5,
  });
  add("cpu", "cpu.cluster.freq", { cluster: "P0" }, 1.06 * GHZ, {
    noise: 0.4 * GHZ,
    min: 0.6 * GHZ,
    max: 3.204 * GHZ,
    cadence: 5,
  });
  for (const [stall, v] of [
    ["some", 1.2],
    ["full", 0],
  ] as const)
    add("cpu", "cpu.psi", { stall }, v, { noise: 0.5, cadence: 5, ...pct });
}

function memory(add: Add) {
  const mem = { noise: 0.04e9 };
  // used + cached + free is MemTotal (`linux/memory.rs`).
  add("memory", "mem.used", {}, 7.0e9, mem);
  add("memory", "mem.cached", {}, LINUX_MEM_TOTAL_BYTES - 7.0e9 - 1.2e9, mem);
  add("memory", "mem.free", {}, 1.2e9, mem);
  add("memory", "mem.swap_used", {}, 2.05e9);
  add("memory", "mem.swap_in", {}, 0);
  add("memory", "mem.swap_out", {}, 0);
  for (const [stall, v] of [
    ["some", 0.4],
    ["full", 0.1],
  ] as const)
    add("memory", "mem.psi", { stall }, v, { noise: 0.2, cadence: 5, ...pct });
}

function sensors(add: Add) {
  // Power: SMC's "Total System Power" is `power.system`, not also a sensor (hwmon.rs).
  for (const [name, v] of [
    ["3.8 V Rail Power", 2.6],
    ["AC Input Power", 7.1],
  ] as const)
    add("power", "power.sensor", { name }, v, { noise: 0.4, cadence: 5 });
  add("power", "power.system", {}, 7.0, { noise: 0.4 });
  // No CPU-class sensor, so no `thermal.hottest`.
  for (const [name, v] of [
    ["NAND Flash Temperature", 40],
    ["WiFi/BT Module Temp", 42],
    ["nvme@nvme0/Composite", 38],
  ] as const)
    add("sensors", "thermal.sensor", { name }, v, { noise: 0.5, cadence: 5 });
  add("sensors", "fan.rpm", { fan: "macsmc_hwmon/0" }, 1702, {
    noise: 20,
    cadence: 2,
  });
  add("sensors", "fan.max", { fan: "macsmc_hwmon/0" }, 4499, { cadence: 60 });
}

function network(add: Add, total: Total) {
  add("network", "net.rx", { iface: "end0" }, 180e3, { noise: 90e3 });
  add("network", "net.tx", { iface: "end0" }, 42e3, { noise: 20e3 });
  add("network", "net.link_rate", { iface: "end0" }, 1e9, { cadence: 60 });
  add("network", "net.rx", { iface: "wlp1s0f0" }, 0.4e3, { noise: 0.3e3 });
  add("network", "net.tx", { iface: "wlp1s0f0" }, 0.2e3, { noise: 0.1e3 });
  add("network", "net.link_rate", { iface: "wlp1s0f0" }, 866.7e6, {
    cadence: 60,
  });
  // Totals count physical interfaces only, so they come before the tunnel.
  total("network", "net.rx_total", "net.rx");
  total("network", "net.tx_total", "net.tx");
  add("network", "net.rx", { iface: "tailscale0" }, 12e3, { noise: 8e3 });
  add("network", "net.tx", { iface: "tailscale0" }, 9e3, { noise: 6e3 });
}

const DISK_METRICS = [
  "disk.read_ops",
  "disk.write_ops",
  "disk.read_timed_ops",
  "disk.write_timed_ops",
  "disk.read_time",
  "disk.write_time",
];

function disk(add: Add, total: Total) {
  const devices = [
    // dev, read B/s, write B/s, read ops, write ops, ms per read, ms per write
    ["nvme0n1", 0.4e6, 2.1e6, 12, 160, 0.2, 0.4],
    ["sda", 3.2e6, 0.3e6, 26, 4, 2.4, 3.1],
    ["sdb", 0.6e6, 0.1e6, 6, 2, 9.5, 12],
  ] as const;
  for (const [dev, read, write] of devices) {
    add("disk", "disk.read", { dev }, read, { noise: read * 0.5 });
    add("disk", "disk.write", { dev }, write, { noise: write * 0.5 });
  }
  for (const [dev, , , readOps, writeOps, readMs, writeMs] of devices) {
    for (const [metric, value] of [
      ["disk.read_ops", readOps],
      ["disk.write_ops", writeOps],
      ["disk.read_timed_ops", readOps],
      ["disk.write_timed_ops", writeOps],
      ["disk.read_time", readOps * readMs],
      ["disk.write_time", writeOps * writeMs],
    ] as const)
      add("disk", metric, { dev }, value);
  }
  for (const metric of DISK_METRICS) total("disk", `${metric}_total`, metric);
  total("disk", "disk.read_total", "disk.read");
  total("disk", "disk.write_total", "disk.write");
  // Root btrfs is 91% full with 14 GB free; `/var` and `/home` share it.
  for (const [vol, size, free] of [
    ["/", 155e9, 14e9],
    ["/boot", 1.0e9, 0.62e9],
    ["/boot/efi", 0.5e9, 0.43e9],
  ] as const) {
    add("disk", "disk.used", { vol }, size - free, { cadence: 60 });
    add("disk", "disk.free", { vol }, free, { cadence: 60 });
    add("disk", "disk.total", { vol }, size, { cadence: 60 });
  }
  for (const [stall, v] of [
    ["some", 90],
    ["full", 86],
  ] as const)
    add("disk", "disk.psi", { stall }, v, { noise: 3, cadence: 5, ...pct });
}

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;

/**
 * cgroup groups on linux-tailscale, as `linux/cgroup.rs` labels them: kind,
 * name, CPU (% of one core), memory, `memory.max` when it is a number, disk
 * read and write (B/s), memory PSI `some` (%).
 */
const CGROUPS: [
  "service" | "docker",
  string,
  number,
  number,
  number | null,
  number,
  number,
  number,
][] = [
  ["docker", "opendata-worker", 2.6, 625 * MIB, 4 * GIB, 12e3, 180e3, 0.2],
  ["docker", "opendata-backend", 1.9, 879 * MIB, 2 * GIB, 4e3, 36e3, 0.4],
  ["service", "docker.service", 1.1, 1.69 * GIB, null, 0, 420e3, 0],
  ["docker", "cadvisor", 1.4, 96 * MIB, null, 0, 0, 0],
  ["docker", "prometheus-agent", 0.9, 210 * MIB, GIB, 0, 64e3, 0.1],
  ["docker", "promtail", 0.6, 71 * MIB, null, 8e3, 2e3, 0],
  ["docker", "traefik", 0.5, 54 * MIB, null, 0, 1e3, 0],
  ["docker", "opendata-redis", 0.4, 38 * MIB, 512 * MIB, 0, 22e3, 0],
  ["docker", "opendata-mcp", 0.3, 142 * MIB, null, 0, 0, 0],
  ["docker", "node-exporter", 0.2, 18 * MIB, null, 0, 0, 0],
  ["service", "sshd.service", 0, 6 * MIB, null, 0, 0, 0],
];

/** A one-shot migration that exited: its series are gaps until it leaves. */
const EXITED = "opendata-migrate";

function cgroups(add: Add) {
  const every = { cadence: 5 };
  for (const [kind, name, cpu, mem, max, read, write, psi] of CGROUPS) {
    const labels = { kind, name };
    add("containers", "cgroup.cpu", labels, cpu, {
      ...every,
      noise: cpu * 0.4,
      min: 0,
    });
    add("containers", "cgroup.mem", labels, mem, { ...every, noise: mem / 50 });
    if (max !== null) add("containers", "cgroup.mem_max", labels, max, every);
    add("containers", "cgroup.io_read", labels, read, {
      ...every,
      noise: read,
      min: 0,
    });
    add("containers", "cgroup.io_write", labels, write, {
      ...every,
      noise: write,
      min: 0,
    });
    add("containers", "cgroup.mem_psi", labels, psi, {
      ...every,
      noise: psi,
      ...pct,
    });
  }
  const exited = { kind: "docker", name: EXITED };
  for (const metric of [
    "cgroup.cpu",
    "cgroup.mem",
    "cgroup.io_read",
    "cgroup.io_write",
    "cgroup.mem_psi",
  ])
    add("containers", metric, exited, 0, { ...every, silent: true });
  // The remainder: no `mem_max` or `mem_psi`, neither adds up.
  const other = { kind: "other", name: "_other" };
  add("containers", "cgroup.cpu", other, 0.9, { ...every, noise: 0.3 });
  add("containers", "cgroup.mem", other, 310 * MIB, { ...every, noise: MIB });
  add("containers", "cgroup.io_read", other, 2e3, { ...every, noise: 2e3 });
  add("containers", "cgroup.io_write", other, 9e3, { ...every, noise: 6e3 });
  add("containers", "cgroup.other_count", {}, 25, every);
}

/** The series the agent publishes on linux-tailscale, in collector order. */
export function buildLinuxSpecs(add: Add, total: Total, selfPeriodMs: number) {
  cpu(add);
  memory(add);
  sensors(add);
  network(add, total);
  disk(add, total);
  cgroups(add);
  add("cpu", "self.cpu", {}, 0.1, { noise: 0.03, periodMs: selfPeriodMs });
}

/**
 * Process rows as `linux/processes.rs` fills them: no energy, wakeups,
 * compressed memory, network or GPU (energy and wakeups are zero, the
 * capabilities say so), disk rates from `/proc/<pid>/io`.
 */
function proc(
  pid: number,
  name: string,
  cpuPct: number,
  mem: number,
  threads: number,
  diskBps: number,
  user: string
): LiveProcess {
  return {
    pid,
    start_time_us: 1_788_630_414_000_000 + pid * 1_000_000,
    name,
    cpu_pct: cpuPct,
    mem_bytes: mem,
    compressed_bytes: null,
    threads,
    idle_wakeups_per_s: 0,
    energy: 0,
    disk_read_bps: Math.round(diskBps * 0.6),
    disk_write_bps: Math.round(diskBps * 0.4),
    net_rx_bps: null,
    net_tx_bps: null,
    gpu_pct: null,
    ports: null,
    user,
    refusal: pid === 1 ? "launchd" : null,
  };
}

const NAMED: LiveProcess[] = [
  proc(2841, "postgres", 6.2, 1.1e9, 12, 1.8e6, "postgres"),
  proc(1932, "dockerd", 3.4, 0.21e9, 38, 0.2e6, "root"),
  proc(3310, "node", 2.8, 0.46e9, 11, 0.1e6, "riley"),
  proc(1866, "containerd", 1.6, 0.09e9, 24, 0.05e6, "root"),
  proc(4102, "jellyfin", 1.2, 0.72e9, 31, 2.4e6, "riley"),
  proc(1201, "tailscaled", 0.9, 0.06e9, 14, 0, "root"),
  proc(1, "systemd", 0.3, 0.02e9, 1, 0.01e6, "root"),
  proc(712, "systemd-journald", 0.4, 0.05e9, 1, 0.08e6, "root"),
  proc(744, "systemd-udevd", 0.1, 0.01e9, 1, 0, "root"),
  proc(1023, "sshd", 0.1, 0.01e9, 1, 0, "root"),
  proc(1104, "firewalld", 0.2, 0.04e9, 2, 0, "root"),
  proc(1150, "NetworkManager", 0.1, 0.02e9, 3, 0, "root"),
  proc(1302, "kelvo-agent", 0.1, 0.018e9, 4, 0.01e6, "root"),
  proc(1488, "chronyd", 0, 0.004e9, 1, 0, "chrony"),
  proc(2290, "redis-server", 0.5, 0.03e9, 6, 0.02e6, "redis"),
];

/** About how many processes linux-tailscale runs. */
const LINUX_PROCESS_COUNT = 320;
const IDLE_NAMES = [
  "kworker/u16:2",
  "containerd-shim-runc-v2",
  "bash",
  "agetty",
  "dbus-broker",
  "polkitd",
  "rsyslogd",
  "crond",
];

export const LINUX_PROCESSES: LiveProcess[] = [
  ...NAMED,
  ...Array.from({ length: LINUX_PROCESS_COUNT - NAMED.length }, (_, i) =>
    proc(
      5000 + i * 3,
      IDLE_NAMES[i % IDLE_NAMES.length] as string,
      (i % 5) / 100,
      (1 + (i % 20)) * 1e6,
      1 + (i % 3),
      0,
      i % 3 === 0 ? "riley" : "root"
    )
  ),
];
