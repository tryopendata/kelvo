import type {
  CommandError,
  Gap,
  HistoryHealth,
} from "@core/generated/bindings";
import {
  collectingHeader,
  dedupeGaps,
  gapBands,
  gapLabel,
  historyHealthNotices,
  historyUnavailable,
  isCollecting,
  resetHistoryFailure,
} from "./history-state";

const at = (h: number, m: number) => new Date(2026, 9, 4, h, m).getTime();

const gap = (g: Partial<Gap>): Gap => ({
  start_ms: at(11, 2),
  end_ms: at(11, 31),
  module: null,
  reason: "sleep",
  ...g,
});

describe("gapLabel", () => {
  it("names a closed sleep gap with both times and says it is not interpolated", () => {
    expect(gapLabel(gap({}))).toBe("Asleep 11:02–11:31 · not interpolated");
  });

  it("names a closed sleep gap by its length in the duration style", () => {
    expect(gapLabel(gap({ end_ms: at(16, 17) }), "duration")).toBe(
      "Asleep 5h 15m · no samples"
    );
  });

  it("names an open sleep gap by its start in either style", () => {
    expect(gapLabel(gap({ end_ms: null }))).toBe("Asleep since 11:02");
    expect(gapLabel(gap({ end_ms: null }), "duration")).toBe(
      "Asleep since 11:02"
    );
  });

  it("labels each reason", () => {
    expect(gapLabel(gap({ reason: "app_not_running" }))).toBe(
      "Kelvo not running"
    );
    expect(gapLabel(gap({ reason: "paused" }))).toBe("Paused");
    expect(gapLabel(gap({ reason: "module_disabled", module: "gpu" }))).toBe(
      "GPU sampling off"
    );
    expect(gapLabel(gap({ reason: "truncated" }))).toBe("History pruned");
    expect(gapLabel(gap({ reason: "source_offline" }))).toBe("Host offline");
  });

  it("labels reasons newer than the bindings, and unknown ones generically", () => {
    const newer = (reason: string) => gap({ reason: reason as Gap["reason"] });
    expect(gapLabel(newer("clock_changed"))).toBe("Clock changed");
    expect(gapLabel(newer("write_failed"))).toBe("History write failed");
    expect(gapLabel(gap({ reason: "unknown" }))).toBe("No samples");
    expect(gapLabel(newer("from_a_future_build"))).toBe("No samples");
  });
});

describe("dedupeGaps", () => {
  it("merges the same gap returned by several pages, keeping the later end", () => {
    const merged = dedupeGaps([
      gap({}),
      gap({ start_ms: at(11, 2) + 3, end_ms: at(11, 40) }),
      gap({ reason: "paused" }),
    ]);
    expect(merged).toEqual([
      gap({ end_ms: at(11, 40) }),
      gap({ reason: "paused" }),
    ]);
  });

  it("lets an open gap win over a closed copy", () => {
    expect(dedupeGaps([gap({}), gap({ end_ms: null })])).toEqual([
      gap({ end_ms: null }),
    ]);
  });
});

describe("gapBands", () => {
  const gaps = [
    gap({ start_ms: at(10, 0), end_ms: at(10, 50) }),
    gap({ reason: "module_disabled", module: "gpu" }),
    gap({ reason: "module_disabled", module: "cpu" }),
    gap({ start_ms: at(11, 50), end_ms: null, reason: "paused" }),
  ];

  it("keeps host-wide gaps and this module's gaps, clipped to the range", () => {
    expect(gapBands(gaps, at(10, 40), at(12, 0), { module: "cpu" })).toEqual([
      {
        fromMs: at(10, 40),
        toMs: at(10, 50),
        label: "Asleep 10:00–10:50 · not interpolated",
        module: null,
      },
      {
        fromMs: at(11, 2),
        toMs: at(11, 31),
        label: "CPU sampling off",
        module: "cpu",
      },
      { fromMs: at(11, 50), toMs: at(12, 0), label: "Paused", module: null },
    ]);
  });

  it("keeps every module's gaps when no module is given", () => {
    const bands = gapBands(gaps, at(10, 40), at(12, 0));
    expect(bands.map((b) => b.label)).toEqual([
      "Asleep 10:00–10:50 · not interpolated",
      "GPU sampling off",
      "CPU sampling off",
      "Paused",
    ]);
  });

  it("drops gaps outside the range", () => {
    expect(gapBands([gap({})], at(12, 0), at(13, 0))).toEqual([]);
  });
});

describe("isCollecting", () => {
  const from = at(0, 0);
  const to = at(24, 0);

  it("is collecting with no recorded sample", () => {
    expect(isCollecting(null, from, to)).toBe(true);
  });

  it("is collecting while under a quarter of the range is recorded", () => {
    expect(isCollecting(at(23, 56), from, to)).toBe(true);
    expect(isCollecting(at(18, 1), from, to)).toBe(true);
  });

  it("stops collecting once a quarter of the range is recorded", () => {
    expect(isCollecting(at(18, 0), from, to)).toBe(false);
    expect(isCollecting(at(0, 0) - 1, from, to)).toBe(false);
  });
});

describe("collectingHeader", () => {
  it("states the start and the rate", () => {
    expect(collectingHeader(at(22, 36), 1000)).toBe(
      "started 22:36 · 1 sample/s"
    );
    expect(collectingHeader(at(22, 36), 2000)).toBe(
      "started 22:36 · 1 sample/2s"
    );
  });
});

describe("historyUnavailable", () => {
  it("covers a store that never opened and one that failed a query", () => {
    expect(historyUnavailable({ kind: "history_unavailable" })).toEqual({
      message: "History is unavailable. Live values still work.",
      canReset: true,
    });
    expect(
      historyUnavailable({ kind: "store", message: "disk I/O error" })
    ).toEqual({
      message:
        "History is unavailable: disk I/O error. Live values still work.",
      canReset: false,
    });
  });

  it("says why by reason, and offers a reset except when locked", () => {
    const locked = historyUnavailable({
      kind: "history_unavailable",
      reason: { kind: "locked" },
    });
    expect(locked?.canReset).toBe(false);
    expect(locked?.message).toMatch(/another copy of Kelvo/);

    const tooNew = historyUnavailable({
      kind: "history_unavailable",
      reason: { kind: "too_new", found: 4, supported: 2 },
    });
    expect(tooNew).toEqual({
      message:
        "History was written by a newer version of Kelvo (format 4; this version reads up to 2). Live values still work.",
      canReset: true,
    });

    const damaged = historyUnavailable({
      kind: "history_unavailable",
      reason: { kind: "corrupt", message: "malformed" },
    });
    expect(damaged).toEqual({
      message: "The history file is damaged. Live values still work.",
      canReset: true,
    });

    expect(
      historyUnavailable({
        kind: "history_unavailable",
        reason: { kind: "failed", message: "permission denied" },
      })
    ).toEqual({
      message:
        "History is unavailable: permission denied. Live values still work.",
      canReset: true,
    });
  });

  it("maps the store's own corrupt and too-new errors like the reasons", () => {
    expect(historyUnavailable({ kind: "store_corrupt", message: "x" })).toEqual(
      historyUnavailable({
        kind: "history_unavailable",
        reason: { kind: "corrupt", message: "y" },
      })
    );
    expect(
      historyUnavailable({ kind: "store_too_new", found: 4, supported: 2 })
    ).toEqual(
      historyUnavailable({
        kind: "history_unavailable",
        reason: { kind: "too_new", found: 4, supported: 2 },
      })
    );
  });

  it("reads a reason a newer engine added as unavailable, with a reset", () => {
    const error = {
      kind: "history_unavailable",
      reason: { kind: "quarantined" },
    } as unknown as CommandError;
    expect(historyUnavailable(error)).toEqual({
      message: "History is unavailable. Live values still work.",
      canReset: true,
    });
  });

  it("is null for errors that are not about history being unavailable", () => {
    expect(historyUnavailable({ kind: "unknown_host", host: "h" })).toBeNull();
    expect(
      historyUnavailable({ kind: "store_busy", message: "locked" })
    ).toBeNull();
  });
});

describe("resetHistoryFailure", () => {
  it("tells the user to quit the other Kelvo when the file is held", () => {
    expect(resetHistoryFailure({ kind: "store_busy", message: "busy" })).toBe(
      "Another copy of Kelvo has the history file open. Quit it and try again."
    );
    expect(
      resetHistoryFailure({
        kind: "history_unavailable",
        reason: { kind: "locked" },
      })
    ).toMatch(/Quit it/);
    expect(resetHistoryFailure({ kind: "internal", message: "panic" })).toBe(
      "Couldn't reset history."
    );
  });
});

describe("historyHealthNotices", () => {
  const healthy: HistoryHealth = {
    low_disk_paused: false,
    trimmed_before_ms: null,
    trimmed_limit_bytes: null,
    cap_met: true,
  };
  const trimmedAt = at(9, 30);
  const trimmed: HistoryHealth = {
    ...healthy,
    trimmed_before_ms: trimmedAt,
    trimmed_limit_bytes: 150_000_000,
  };

  it("says nothing when history is fine", () => {
    expect(historyHealthNotices(healthy)).toEqual([]);
    expect(historyHealthNotices(healthy, at(0, 0))).toEqual([]);
  });

  it("warns while the disk is almost full, on every view", () => {
    const notices = historyHealthNotices({ ...healthy, low_disk_paused: true });
    expect(notices).toHaveLength(1);
    expect(notices[0]?.kind).toBe("warning");
    expect(notices[0]?.text).toMatch(/^History paused: disk almost full\./);
    expect(
      historyHealthNotices({ ...healthy, low_disk_paused: true }, at(10, 0))
    ).toHaveLength(1);
  });

  it("explains a trim as info, with the limit it stayed under", () => {
    expect(historyHealthNotices(trimmed)).toEqual([
      {
        kind: "info",
        text: "History trimmed to stay under 150 MB. It starts Oct 4 09:30.",
      },
    ]);
    expect(
      historyHealthNotices({
        ...trimmed,
        trimmed_limit_bytes: 1_000_000_000,
      })[0]?.text
    ).toContain("under 1 GB");
  });

  it("mentions a trim only on a chart that reaches back past it", () => {
    // A range starting after the trim point shows all the history there is.
    expect(historyHealthNotices(trimmed, trimmedAt + 1)).toEqual([]);
    expect(historyHealthNotices(trimmed, trimmedAt)).toEqual([]);
    expect(historyHealthNotices(trimmed, trimmedAt - 1)).toHaveLength(1);
  });

  it("warns when even one day does not fit", () => {
    const notices = historyHealthNotices(
      { ...trimmed, cap_met: false },
      at(12, 0)
    );
    expect(notices).toEqual([
      {
        kind: "warning",
        text: "History is over 150 MB even with only the last day kept.",
      },
    ]);
  });
});
