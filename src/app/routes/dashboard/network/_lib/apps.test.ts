import { formatSpan } from "@core/format";
import type { NetworkByApp } from "@core/generated/bindings";
import {
  appRows,
  appsTitle,
  firstRecordedMs,
  nowRates,
  openTailS,
  partialCoverage,
  remainderRows,
  sortApps,
  unrecorded,
  windowRange,
} from "./apps";

const T = 1_700_000_000_000;

function answer(over: Partial<NetworkByApp> = {}): NetworkByApp {
  return {
    from_ms: T,
    to_ms: T + 90_000,
    complete_to_ms: T + 90_000,
    resolution_ms: 10_000,
    measured_ms: 90_000,
    coverage: [{ from_ms: T, to_ms: T + 90_000, tier: "s10" }],
    apps: [
      { name: "Docker Desktop", rx_bytes: 286e6, tx_bytes: 1.4e6 },
      { name: "Google Chrome", rx_bytes: 64e6, tx_bytes: 3.6e6 },
      { name: "curl", rx_bytes: 3.8e6, tx_bytes: 10e3 },
    ],
    other_apps_rx_bytes: 0,
    other_apps_tx_bytes: 0,
    iface_rx_bytes: 380e6,
    iface_tx_bytes: 50e6,
    overhead_rx_bytes: 18e6,
    overhead_tx_bytes: 9e6,
    system_rx_bytes: 8.2e6,
    system_tx_bytes: 35.99e6,
    clamped: false,
    ...over,
  };
}

describe("Apps title", () => {
  it("names the window with nothing selected", () => {
    expect(appsTitle(null, 300_000)).toBe("Apps, last 5 minutes");
    expect(appsTitle(null, 900_000)).toBe("Apps, last 15 minutes");
    expect(appsTitle(null, 1_800_000)).toBe("Apps, last 30 minutes");
    expect(appsTitle(null, 3_600_000)).toBe("Apps, last hour");
  });

  it("names the selection's length", () => {
    expect(appsTitle({ fromMs: T, toMs: T + 90_000 }, 300_000)).toBe(
      "Apps, selected 90 s"
    );
    expect(appsTitle({ fromMs: T, toMs: T + 120_000 }, 300_000)).toBe(
      "Apps, selected 2 min"
    );
  });

  it("formats spans", () => {
    expect(formatSpan(10_000)).toBe("10 s");
    expect(formatSpan(150_000)).toBe("2 min 30 s");
    expect(formatSpan(3_900_000)).toBe("1 h 5 min");
  });
});

describe("Apps rows", () => {
  it("shares are of the interface total; now comes from the latest bucket", () => {
    const now = nowRates(
      answer({
        measured_ms: 10_000,
        apps: [{ name: "Google Chrome", rx_bytes: 11e6, tx_bytes: 0 }],
      })
    );
    const rows = appRows(answer(), now);
    expect(rows.map((r) => [r.name, r.totalBytes, r.nowBps])).toEqual([
      ["Docker Desktop", 287.4e6, 0],
      ["Google Chrome", 67.6e6, 1.1e6],
      ["curl", 3.81e6, 0],
    ]);
    expect(rows[0]?.share).toBeCloseTo((287.4e6 / 430e6) * 100);
  });

  it("an unmeasured latest bucket leaves now unknown, not zero", () => {
    expect(nowRates(answer({ measured_ms: 0, apps: [] }))).toBeNull();
    expect(appRows(answer(), null).every((r) => r.nowBps === null)).toBe(true);
  });

  it("no share when nothing moved", () => {
    const rows = appRows(
      answer({
        apps: [{ name: "curl", rx_bytes: 0, tx_bytes: 0 }],
        iface_rx_bytes: 0,
        iface_tx_bytes: 0,
        overhead_rx_bytes: 0,
        overhead_tx_bytes: 0,
        system_rx_bytes: 0,
        system_tx_bytes: 0,
      }),
      null
    );
    expect(rows[0]?.share).toBeNull();
  });

  // Regression: apps 30x the interface showed node at 1556.4%.
  it("shares add up to 100% even when the apps exceed the interface", () => {
    const a = answer({
      apps: [
        { name: "node", rx_bytes: 1.8e6, tx_bytes: 195e6 },
        { name: "chrome-headless-shell", rx_bytes: 149e6, tx_bytes: 1.4e6 },
        { name: "Ghostty", rx_bytes: 27.1e6, tx_bytes: 0.3e6 },
      ],
      iface_rx_bytes: 9e6,
      iface_tx_bytes: 3.7e6,
      overhead_rx_bytes: 0,
      overhead_tx_bytes: 0,
      system_rx_bytes: 0,
      system_tx_bytes: 0,
      clamped: true,
    });
    const shares = [...appRows(a, null), ...remainderRows(a)].map(
      (r) => r.share ?? 0
    );
    expect(Math.max(...shares)).toBeLessThanOrEqual(100);
    expect(shares.reduce((n, s) => n + s, 0)).toBeCloseTo(100);
  });

  it("remainder: other apps only when non-zero, then overhead, then System and other", () => {
    expect(remainderRows(answer()).map((r) => r.name)).toEqual([
      "Protocol overhead (est.)",
      "System and other",
    ]);
    const withOther = remainderRows(
      answer({ other_apps_rx_bytes: 5e6, other_apps_tx_bytes: 1e6 })
    );
    expect(withOther.map((r) => [r.name, r.totalBytes])).toEqual([
      ["Other apps", 6e6],
      ["Protocol overhead (est.)", 27e6],
      ["System and other", 44.19e6],
    ]);
    expect(withOther.every((r) => r.nowBps === null)).toBe(true);
  });

  it("the parts add up to the interface", () => {
    const a = answer();
    const all = [...appRows(a, null), ...remainderRows(a)];
    expect(all.reduce((n, r) => n + r.totalBytes, 0)).toBeCloseTo(
      a.iface_rx_bytes + a.iface_tx_bytes
    );
  });

  it("sorts by total or by now, unknown now last", () => {
    const now = new Map([
      ["Google Chrome", 1.1e6],
      ["Docker Desktop", 0],
    ]);
    const rows = appRows(answer(), now);
    const names = (r: { name: string }[]) => r.map((x) => x.name);
    expect(names(sortApps(rows, { by: "total", dir: "asc" }))).toEqual([
      "curl",
      "Google Chrome",
      "Docker Desktop",
    ]);
    expect(names(sortApps(rows, { by: "now", dir: "desc" }))).toEqual([
      "Google Chrome",
      "curl",
      "Docker Desktop",
    ]);
    const unknown = appRows(answer(), null);
    expect(names(sortApps(unknown, { by: "now", dir: "desc" }))).toEqual([
      "curl",
      "Docker Desktop",
      "Google Chrome",
    ]);
  });
});

describe("coverage", () => {
  it("partial: measured for 40 of 90 s", () => {
    expect(partialCoverage(answer({ measured_ms: 40_000 }))).toEqual({
      measuredS: 40,
      spanS: 90,
    });
  });

  it("full coverage, within jitter, is not partial", () => {
    expect(partialCoverage(answer())).toBeNull();
    expect(partialCoverage(answer({ measured_ms: 89_400 }))).toBeNull();
  });

  it("an open bucket still filling is not missing time", () => {
    // The last bucket is open: the interface has its 10 s, the per-app
    // stream (on its own 10 s phase) only 4 s so far.
    const open = answer({ complete_to_ms: T + 80_000, measured_ms: 84_000 });
    expect(partialCoverage(open)).toBeNull();
    expect(openTailS(open)).toBe(10);
    expect(openTailS(answer())).toBe(0);
  });

  it("time missing before the open part is reported once it closes", () => {
    // 50 s of the complete 80 s measured, and 4 s so far of the open bucket:
    // how much of the 54 s is the complete part's is not known yet.
    const open = answer({ complete_to_ms: T + 80_000, measured_ms: 54_000 });
    expect(partialCoverage(open)).toBeNull();
    const closed = answer({ measured_ms: 60_000 });
    expect(partialCoverage(closed)).toEqual({ measuredS: 60, spanS: 90 });
  });

  it("a range with nothing complete yet is not partial", () => {
    expect(
      partialCoverage(
        answer({ complete_to_ms: T - 10_000, measured_ms: 2_000 })
      )
    ).toBeNull();
    expect(openTailS(answer({ complete_to_ms: T - 10_000 }))).toBe(90);
  });

  it("nothing recorded, and where recording starts", () => {
    const empty = answer({
      measured_ms: 0,
      apps: [],
      coverage: [{ from_ms: T, to_ms: T + 90_000, tier: null }],
    });
    expect(unrecorded(empty)).toBe(true);
    expect(firstRecordedMs(empty)).toBeNull();
    expect(unrecorded(answer())).toBe(false);
    expect(
      firstRecordedMs(
        answer({
          coverage: [
            { from_ms: T, to_ms: T + 40_000, tier: null },
            { from_ms: T + 40_000, to_ms: T + 90_000, tier: "s10" },
          ],
        })
      )
    ).toBe(T + 40_000);
  });

  it("the whole window is the complete buckets before the complete edge", () => {
    expect(windowRange(T, 300_000)).toEqual({ fromMs: T - 300_000, toMs: T });
  });
});
