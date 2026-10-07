import { describe, expect, it } from "vitest";
import { batteryBars, batteryHourStarts } from "./battery-hours";
import { MOCK_HOST_ID } from "./mock/fixtures";
import { createMockTransport } from "./mock-transport";

const HOUR = 3_600_000;
const utc = (iso: string) => Date.parse(iso);

describe("battery hour boundaries", () => {
  // Node re-reads TZ when it is assigned, so `Date` follows it from here on.
  const previous = process.env.TZ;
  afterEach(() => {
    process.env.TZ = previous;
  });

  it("follows local hours in a half-hour zone, ending with the current one", () => {
    process.env.TZ = "Asia/Kolkata";
    // 14:40 IST.
    const starts = batteryHourStarts(new Date(utc("2026-10-05T09:10:00Z")));
    expect(starts).toHaveLength(25);
    // The current hour is 14:00 to 15:00 IST: 08:30 to 09:30 UTC.
    expect(starts[24]).toBe(utc("2026-10-05T09:30:00Z"));
    expect(starts[23]).toBe(utc("2026-10-05T08:30:00Z"));
    expect(starts[0]).toBe(utc("2026-10-04T09:30:00Z"));
  });

  it("steps over a spring forward in real hours, with no empty bar", () => {
    process.env.TZ = "America/New_York";
    // 2026-03-08 04:30 EDT; local 02:00 does not exist.
    const starts = batteryHourStarts(new Date(utc("2026-03-08T08:30:00Z")), 4);
    // 00:00 EST, 01:00 EST, 03:00 EDT, 04:00 EDT, 05:00 EDT.
    expect(starts).toEqual([
      utc("2026-03-08T05:00:00Z"),
      utc("2026-03-08T06:00:00Z"),
      utc("2026-03-08T07:00:00Z"),
      utc("2026-03-08T08:00:00Z"),
      utc("2026-03-08T09:00:00Z"),
    ]);
  });

  it("keeps both of a fall back's 1 a.m. hours, from the second", () => {
    process.env.TZ = "America/New_York";
    // 2026-11-01 01:30 EST, the second 1 a.m. (06:00 UTC is 02:00 EDT
    // turned back to 01:00 EST).
    const starts = batteryHourStarts(new Date(utc("2026-11-01T06:30:00Z")), 4);
    // 23:00 EDT, 00:00 EDT, 01:00 EDT, 01:00 EST, 02:00 EST.
    expect(starts).toEqual([
      utc("2026-11-01T03:00:00Z"),
      utc("2026-11-01T04:00:00Z"),
      utc("2026-11-01T05:00:00Z"),
      utc("2026-11-01T06:00:00Z"),
      utc("2026-11-01T07:00:00Z"),
    ]);
  });

  it("maps Rust's rows to the bars", () => {
    expect(
      batteryBars([{ start_ms: 5, charge: null, charging: true }])
    ).toEqual([{ tsMs: 5, charge: null, charging: true }]);
  });
});

describe("mock battery_hours", () => {
  it("answers one row per hour through the current one, and rejects bad boundaries", async () => {
    const t = createMockTransport({ autoTick: false });
    const now = Date.now();
    const end = Math.ceil(now / HOUR) * HOUR;
    const starts = Array.from({ length: 25 }, (_, i) => end - (24 - i) * HOUR);
    const res = await t.batteryHours(MOCK_HOST_ID, starts);
    if (res.status !== "ok") throw new Error("battery_hours failed");
    expect(res.data.map((h) => h.start_ms)).toEqual(starts.slice(0, -1));
    // The running hour has a bar from its minutes so far.
    expect(res.data[23]?.charge).not.toBeNull();

    const backwards = await t.batteryHours(MOCK_HOST_ID, [end, end - HOUR]);
    expect(backwards.status).toBe("error");
    const one = await t.batteryHours(MOCK_HOST_ID, [end]);
    expect(one.status).toBe("error");
  });
});
