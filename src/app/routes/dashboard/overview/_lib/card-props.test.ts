import type { LiveProcess } from "@core/generated/bindings";
import {
  cpuCard,
  diskCard,
  memoryCard,
  networkCard,
  powerCard,
  ratio,
  topProcesses,
} from "./card-props";

const proc = (p: Partial<LiveProcess> & { pid: number }): LiveProcess => ({
  start_time_us: 1,
  name: `p${p.pid}`,
  cpu_pct: null,
  mem_bytes: 0,
  compressed_bytes: null,
  threads: 1,
  idle_wakeups_per_s: null,
  energy: null,
  disk_read_bps: null,
  disk_write_bps: null,
  net_rx_bps: null,
  net_tx_bps: null,
  gpu_pct: null,
  ports: null,
  user: "me",
  refusal: null,
  ...p,
});

const rowsOf = (card: ReturnType<typeof cpuCard>) =>
  card.body.kind === "list" ? card.body.rows : [];

describe("ratio", () => {
  it("is null when either side is missing, so the bar draws no fill", () => {
    expect(ratio(null, 10)).toBeNull();
    expect(ratio(5, null)).toBeNull();
    expect(ratio(5, 0)).toBeNull();
    expect(ratio(5, 10)).toBe(0.5);
  });
});

describe("topProcesses", () => {
  it("sorts by value, drops rows without one and keys by pid and start time", () => {
    const rows = topProcesses(
      [
        proc({ pid: 1, name: "kernel_task", energy: 18 }),
        proc({ pid: 2, name: "Xcode", energy: 41, start_time_us: 99 }),
        proc({ pid: 3, name: "launchd" }),
      ],
      (p) => p.energy,
      String,
      5
    );
    expect(rows).toEqual([
      { id: "2:99", initial: "X", name: "Xcode", value: "41" },
      { id: "1:1", initial: "k", name: "kernel_task", value: "18" },
    ]);
  });
});

describe("cpuCard", () => {
  it("shows process CPU as a share of the whole machine", () => {
    const card = cpuCard({
      total: 18,
      user: 12.4,
      system: 5.6,
      coresLabel: "10P + 4E",
      coreCount: 14,
      clusters: [
        { label: "P-cluster", freqHz: 3.2e9, maxHz: 4.5e9 },
        { label: "E-cluster", freqHz: null, maxHz: 2.9e9 },
      ],
      processes: [proc({ pid: 1, name: "Xcode", cpu_pct: 86.8 })],
    });
    expect(rowsOf(card)[0]?.value).toBe("6.2%");
    expect(card.bars[1]).toEqual({
      label: "E-cluster",
      value: "—",
      fraction: null,
    });
  });
});

describe("powerCard", () => {
  const input = {
    system: 14.8,
    cpu: 6.4,
    gpu: 3.1,
    dram: 0.9,
    hottestC: 61,
    fanRpm: 1860,
    fanMax: 5000,
    passive: false,
    powerSource: "battery" as const,
    temperature: "C" as const,
    processes: [],
  };

  it("sums GPU and DRAM when both rails report", () => {
    const card = powerCard(input);
    expect(card.legend[1]).toEqual({
      label: "GPU + DRAM",
      value: "4.0 W",
      step: 2,
    });
    expect(card.ring.fractions[1]).toBeCloseTo(4 / 14.8);
  });

  it("shows no GPU + DRAM total when either rail is missing", () => {
    for (const missing of [{ gpu: null }, { dram: null }]) {
      const card = powerCard({ ...input, ...missing });
      expect(card.legend[1]?.value).toBe("—");
      expect(card.ring.fractions[1]).toBeNull();
    }
  });

  it("says Passive cooling on a Mac without fans", () => {
    const card = powerCard({ ...input, passive: true, fanRpm: null });
    expect(card.bars[1]).toEqual({
      label: "Fans",
      value: "Passive cooling",
      fraction: "none",
    });
  });

  it("names the power source Rust reports", () => {
    expect(powerCard(input).subtitle).toBe("on battery");
    for (const powerSource of ["adapter", "charging"] as const) {
      expect(powerCard({ ...input, powerSource }).subtitle).toBe(
        "on power adapter"
      );
    }
    expect(powerCard({ ...input, powerSource: null }).subtitle).toBe("");
  });
});

describe("memoryCard", () => {
  const input = {
    used: 17.6e9,
    app: 11.6e9,
    wired: 4.5e9,
    compressed: 1.5e9,
    pressure: 42,
    swapUsed: 512e6,
    totalBytes: 24e9,
    binary: false,
    processes: [],
  };

  it("sums wired and compressed when both report", () => {
    const card = memoryCard(input);
    expect(card.legend[1]?.value).toBe("6.0 GB");
    expect(card.ring.fractions[1]).toBeCloseTo(6 / 24);
  });

  it("shows no wired + compressed total when either part is missing", () => {
    for (const missing of [{ wired: null }, { compressed: null }]) {
      const card = memoryCard({ ...input, ...missing });
      expect(card.legend[1]?.value).toBe("—");
      expect(card.ring.fractions[1]).toBeNull();
    }
  });
});

describe("networkCard", () => {
  it("rings the primary interface's rate against its link speed", () => {
    const en0 = { iface: "en0", rx: 38.4e6, tx: 1.2e6 };
    const en1 = { iface: "en1", rx: 0.2e6, tx: 0.05e6 };
    const card = networkCard({
      primary: en0,
      linkBps: 1.2e9,
      total: { rx: 38.6e6, tx: 1.25e6 },
      interfaces: [en1, en0],
      rate: "MBps",
      processes: null,
    });
    expect(card.ring.value).toBe("26%");
    expect(card.subtitle).toBe("All interfaces · en0");
    // The figures are every interface's.
    expect(card.bars.map((b) => b.value)).toEqual(["38.6 MB/s", "1.3 MB/s"]);
    expect(
      card.body.kind === "list" && card.body.rows.map((r) => r.name)
    ).toEqual(["en0", "en1"]);
  });

  it("leaves out an interface whose rate is only half known", () => {
    const card = networkCard({
      primary: null,
      linkBps: null,
      total: { rx: 3e6, tx: null },
      interfaces: [
        { iface: "en0", rx: 2e6, tx: null },
        { iface: "en1", rx: 1e6, tx: 0.5e6 },
      ],
      rate: "MBps",
      processes: null,
    });
    expect(rowsOf(card).map((r) => r.name)).toEqual(["en1"]);
  });

  it("shows no ring value when the link speed is unknown", () => {
    const card = networkCard({
      primary: { iface: "en0", rx: 1e6, tx: 1e6 },
      linkBps: null,
      total: { rx: 1e6, tx: 1e6 },
      interfaces: [],
      rate: "MBps",
      processes: null,
    });
    expect(card.ring.value).toBe("—");
    expect(card.ring.fractions).toEqual([null, null]);
    expect(card.bars.map((b) => b.fraction)).toEqual([null, null]);
  });

  it("shows the totals with no primary interface (a full-tunnel VPN)", () => {
    const card = networkCard({
      primary: null,
      linkBps: null,
      total: { rx: 4e6, tx: 0.5e6 },
      interfaces: [{ iface: "utun4", rx: 4e6, tx: 0.5e6 }],
      rate: "MBps",
      processes: null,
    });
    expect(card.subtitle).toBe("All interfaces");
    expect(card.bars.map((b) => b.value)).toEqual(["4.0 MB/s", "500 KB/s"]);
    expect(card.legend.map((l) => l.value)).not.toContain("—");
    expect(card.ring.value).toBe("—");
    expect(card.ring.fractions).toEqual([null, null]);
  });
});

describe("networkCard with per-process network (D-081)", () => {
  const base = {
    primary: { iface: "en0", rx: 25e6, tx: 1e6 },
    linkBps: 1.2e9,
    total: { rx: 25e6, tx: 1e6 },
    interfaces: [{ iface: "en0", rx: 25e6, tx: 1e6 }],
    rate: "MBps" as const,
  };

  it("lists processes by total rate instead of interfaces, with its coverage", () => {
    const card = networkCard({
      ...base,
      processes: [
        proc({ pid: 1, name: "node", net_rx_bps: 3e6, net_tx_bps: 1.2e6 }),
        proc({ pid: 2, name: "Safari", net_rx_bps: 21.4e6, net_tx_bps: 0.7e6 }),
        proc({ pid: 3, name: "idle", net_rx_bps: 0, net_tx_bps: 0 }),
      ],
    });
    expect(card.body).toMatchObject({
      kind: "list",
      ariaLabel: "Top processes by network rate",
      note: "Your processes only",
    });
    expect(rowsOf(card).map((r) => [r.name, r.value])).toEqual([
      ["Safari", "22.1 MB/s"],
      ["node", "4.2 MB/s"],
    ]);
  });

  it("shows nothing rather than zeros before the first measured sample", () => {
    const card = networkCard({
      ...base,
      processes: [proc({ pid: 1, name: "Safari" })],
    });
    expect(rowsOf(card)).toEqual([]);
    expect(card.body.kind === "list" && card.body.note).toBe(
      "Your processes only"
    );
  });
});

describe("diskCard", () => {
  it("reads used space from disk.used", () => {
    const card = diskCard({
      read: 220e6,
      write: 48e6,
      totalBytes: 1000e9,
      freeBytes: 612e9,
      usedBytes: 388e9,
      readScale: 500e6,
      writeScale: 500e6,
      rate: "MBps",
      processes: [],
    });
    expect(card.ring.value).toBe("39%");
    expect(card.legend?.[0]).toMatchObject({ label: "Used", value: "388 GB" });
  });
});
