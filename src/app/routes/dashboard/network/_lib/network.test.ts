import { PROCESSES, withNetRates } from "@core/mock/fixtures";
import { describe, expect, it } from "vitest";
import {
  interfaceRows,
  measuredTraffic,
  networkSubtitle,
  ofLink,
} from "./network";

describe("measuredTraffic", () => {
  it("keeps rows with a measured, non-zero rate", () => {
    const measured = withNetRates(PROCESSES);
    expect(measuredTraffic(PROCESSES)).toEqual([]);
    expect(measuredTraffic(measured).map((p) => p.name)).toEqual([
      "Xcode",
      "Safari",
      "node",
      "Figma",
      "com.docker.backend",
    ]);
  });
});

describe("network page helpers", () => {
  it("builds rows in layout order and leaves unknown fields missing", () => {
    const rows = interfaceRows(["en0", "en1"], {
      "net.rx{iface=en0}": 38.4e6,
      "net.tx{iface=en0}": 1.2e6,
    });
    expect(rows.map((r) => r.id)).toEqual(["en0", "en1"]);
    expect(rows[0]).toMatchObject({
      rxBps: 38.4e6,
      txBps: 1.2e6,
      rxBytes: null,
    });
    expect(rows[1]).toMatchObject({ rxBps: null, txBps: null });
  });

  it("computes the download share of the link in bits", () => {
    expect(ofLink(38.4e6, 1.2e9)).toBeCloseTo(25.6);
    expect(ofLink(38.4e6, null)).toBeNull();
    expect(ofLink(null, 1.2e9)).toBeNull();
  });

  it("writes the subtitle from what is known", () => {
    expect(networkSubtitle("en0", 1.2e9)).toBe("en0 · 1.2 Gb/s link");
    expect(networkSubtitle("en0", null)).toBe("en0");
    expect(networkSubtitle(null, null)).toBe("No active interface");
  });
});
