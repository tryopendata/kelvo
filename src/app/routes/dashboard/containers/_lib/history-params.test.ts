import { describe, expect, it } from "vitest";
import { DOCKER_RANGES, dockerRange, historyEnd } from "./history-params";

describe("Docker history navigation", () => {
  it.each(["toString", "constructor", "__proto__", "unknown", "", null])(
    "falls back for an unknown range %s without producing a NaN window",
    (raw) => {
      expect(dockerRange(raw, "15m")).toBe("15m");
      expect(dockerRange(raw, "1h")).toBe("1h");
      expect(
        Number.isFinite(
          1_791_900_000_000 - DOCKER_RANGES[dockerRange(raw, "15m")]
        )
      ).toBe(true);
    }
  );
  it.each(["15m", "1h", "24h", "7d", "30d"])(
    "keeps supported range %s",
    (raw) => {
      expect(dockerRange(raw, "15m")).toBe(raw);
    }
  );
  it.each([null, "", " ", "NaN", "Infinity", "-Infinity", "not-a-date"])(
    "returns to live history for invalid endpoint %s",
    (raw) => {
      expect(historyEnd(raw)).toBeNull();
      expect(historyEnd(raw, 60_000)).toBeNull();
    }
  );
  it("preserves pinned timestamps and adds the detail event window", () => {
    expect(historyEnd("1791900000000")).toBe(1_791_900_000_000);
    expect(historyEnd("1791900000000", 60_000)).toBe(1_791_900_060_000);
    expect(historyEnd("0")).toBe(0);
  });
});
