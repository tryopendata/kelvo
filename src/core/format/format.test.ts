import {
  bytesParts,
  formatBytes,
  formatDuration,
  formatEnergy,
  formatGhz,
  formatHoursMinutes,
  formatMarketingMemory,
  formatPercent,
  formatRate,
  formatRpm,
  formatTemperature,
  formatWatts,
  formatWattsFine,
  MISSING,
  marketingGb,
  rateParts,
  rpmParts,
  temperatureParts,
  wattsParts,
} from "@core/format";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const GiB = 2 ** 30;
const MiB = 2 ** 20;

describe("missing input", () => {
  it.each([null, undefined, Number.NaN, Number.POSITIVE_INFINITY])(
    "renders %s as a dash, never 0",
    (v) => {
      expect(formatPercent(v)).toBe(MISSING);
      expect(formatBytes(v)).toBe(MISSING);
      expect(formatRate(v)).toBe(MISSING);
      expect(formatTemperature(v)).toBe(MISSING);
      expect(formatWatts(v)).toBe(MISSING);
      expect(formatDuration(v)).toBe(MISSING);
      expect(formatHoursMinutes(v)).toBe(MISSING);
    }
  );
});

describe("formatPercent", () => {
  it("matches the mock figures", () => {
    expect(formatPercent(18)).toBe("18%");
    expect(formatPercent(12.4, { decimals: 1 })).toBe("12.4%");
    expect(formatPercent(82, { decimals: 1 })).toBe("82.0%");
    expect(formatPercent(0.4, { decimals: 1 })).toBe("0.4%");
    expect(formatPercent(412)).toBe("412%");
  });

  it("rounds rather than truncates", () => {
    expect(formatPercent(17.6)).toBe("18%");
  });

  it("keeps a real zero", () => {
    expect(formatPercent(0)).toBe("0%");
  });
});

describe("formatBytes", () => {
  it("uses decimal GB by default", () => {
    expect(formatBytes(17.6e9)).toBe("17.6 GB");
    expect(formatBytes(6e9)).toBe("6.0 GB");
    expect(formatBytes(840e6)).toBe("840 MB");
    expect(formatBytes(512e6)).toBe("512 MB");
    expect(formatBytes(330e9)).toBe("330 GB");
    expect(formatBytes(1e12)).toBe("1.0 TB");
  });

  it("formats binary units when the GiB setting is on", () => {
    expect(formatBytes(16.4 * GiB, { units: "GiB" })).toBe("16.4 GiB");
    expect(formatBytes(512 * MiB, { units: "GiB" })).toBe("512 MiB");
  });

  it("promotes rather than showing four integer digits", () => {
    expect(formatBytes(999.4e6)).toBe("999 MB");
    expect(formatBytes(999.6e6)).toBe("1.0 GB");
    // 1000 MiB is under 1 GiB but would print four digits.
    expect(formatBytes(1000 * MiB, { units: "GiB" })).toBe("1.0 GiB");
  });

  it("drops the decimal once rounding reaches 100", () => {
    expect(formatBytes(99.94e9)).toBe("99.9 GB");
    expect(formatBytes(99.96e9)).toBe("100 GB");
  });

  it("never puts decimals on plain bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
  });

  it("can pin a unit and precision", () => {
    expect(formatBytes(24 * GiB, { units: "GiB", decimals: 0 })).toBe("24 GiB");
    expect(bytesParts(2.1e9, { unit: "MB" })).toEqual({
      value: "2100",
      unit: "MB",
    });
  });
});

describe("formatRate", () => {
  it("formats bytes per second as MB/s by default", () => {
    expect(formatRate(38.4e6)).toBe("38.4 MB/s");
    expect(formatRate(1.2e6)).toBe("1.2 MB/s");
    expect(formatRate(220e6)).toBe("220 MB/s");
    expect(formatRate(640e3)).toBe("640 KB/s");
  });

  it("converts to bits for the Mb/s setting", () => {
    expect(formatRate(38.4e6, { units: "Mbps" })).toBe("307 Mb/s");
    expect(formatRate(1.2e6, { units: "Mbps" })).toBe("9.6 Mb/s");
    expect(formatRate(300e6, { units: "Mbps" })).toBe("2.4 Gb/s");
  });

  it("pins a unit so a column lines up", () => {
    expect(formatRate(0.7e6, { unit: "MB/s" })).toBe("0.7 MB/s");
    expect(rateParts(0.7e6, { units: "Mbps", unit: "Mb/s" })).toEqual({
      value: "5.6",
      unit: "Mb/s",
    });
  });
});

describe("formatTemperature", () => {
  it("formats °C with a space before the degree sign", () => {
    expect(formatTemperature(61.3)).toBe("61 °C");
    expect(formatTemperature(61.3, { compact: true })).toBe("61°");
    expect(temperatureParts(54)).toEqual({ value: "54", unit: "°C" });
  });

  it("converts to °F", () => {
    expect(formatTemperature(61, { units: "F" })).toBe("142 °F");
    expect(formatTemperature(100, { units: "F" })).toBe("212 °F");
    expect(formatTemperature(-40, { units: "F" })).toBe("−40 °F");
  });
});

describe("formatWattsFine and formatEnergy", () => {
  it("drops to milliwatts and milliwatt-hours below one unit", () => {
    expect(formatWattsFine(2.46)).toBe("2.5 W");
    expect(formatWattsFine(0.34)).toBe("340 mW");
    expect(formatWattsFine(0.0004)).toBe("<1 mW");
    expect(formatWattsFine(0)).toBe("0 mW");
    expect(formatWattsFine(null)).toBe(MISSING);
    expect(formatEnergy(3600 * 1.237)).toBe("1.24 Wh");
    expect(formatEnergy(3600 * 0.086)).toBe("86 mWh");
    expect(formatEnergy(1)).toBe("<1 mWh");
    expect(formatEnergy(3600 * 140)).toBe("140 Wh");
    expect(formatEnergy(Number.NaN)).toBe(MISSING);
  });
});

describe("formatWatts", () => {
  it("matches the mock figures", () => {
    expect(formatWatts(14.8)).toBe("14.8 W");
    expect(formatWatts(0)).toBe("0.0 W");
    expect(formatWatts(14.8, { compact: true })).toBe("14.8W");
    expect(formatWatts(20, { compact: true, decimals: 0 })).toBe("20W");
    expect(wattsParts(10.44)).toEqual({ value: "10.4", unit: "W" });
  });

  it("drops the decimal at 100 W and above", () => {
    expect(formatWatts(140.2)).toBe("140 W");
  });

  it("uses a typographic minus for discharge and no negative zero", () => {
    expect(formatWatts(-12.3)).toBe("−12.3 W");
    expect(formatWatts(-0.01)).toBe("0.0 W");
  });
});

describe("formatDuration", () => {
  it("shows the two largest units, truncated", () => {
    expect(formatDuration(3 * DAY + 4 * HOUR + 12 * MIN)).toBe("3d 4h");
    expect(formatDuration(3 * DAY + 4 * HOUR + 59 * MIN)).toBe("3d 4h");
    expect(formatDuration(5 * HOUR + 15 * MIN + 59_000)).toBe("5h 15m");
  });

  it("shows three units for the machine header", () => {
    expect(formatDuration(3 * DAY + 4 * HOUR + 12 * MIN, { parts: 3 })).toBe(
      "3d 4h 12m"
    );
  });

  it("drops zero units after the first", () => {
    expect(formatDuration(2 * DAY + 30 * MIN)).toBe("2d");
    expect(formatDuration(2 * HOUR)).toBe("2h");
  });

  it("reads as prose under an hour", () => {
    expect(formatDuration(4 * MIN + 30_000)).toBe("4 min");
    expect(formatDuration(59 * MIN)).toBe("59 min");
    expect(formatDuration(20_000)).toBe("<1 min");
  });

  it("rejects negative spans", () => {
    expect(formatDuration(-1)).toBe(MISSING);
  });
});

describe("formatHoursMinutes", () => {
  it("formats battery time remaining as h:mm", () => {
    expect(formatHoursMinutes(6 * HOUR + 12 * MIN)).toBe("6:12");
    expect(formatHoursMinutes(5 * MIN)).toBe("0:05");
    expect(formatHoursMinutes(26 * HOUR)).toBe("26:00");
    expect(formatHoursMinutes(12 * MIN + 59_999)).toBe("0:12");
  });
});

describe("formatRpm", () => {
  it("rounds and groups fan speed with an upper-case unit", () => {
    expect(formatRpm(1849.6)).toBe("1,850 RPM");
    expect(formatRpm(0)).toBe("0 RPM");
    expect(rpmParts(5210)).toEqual({ value: "5,210", unit: "RPM" });
    expect(formatRpm(null)).toBe(MISSING);
  });
});

describe("marketing memory", () => {
  it("rounds the byte total to whole GiB and labels it GB", () => {
    expect(marketingGb(24 * 2 ** 30 - 5e6)).toBe(24);
    expect(formatMarketingMemory(16 * 2 ** 30)).toBe("16 GB");
  });
});

describe("formatGhz", () => {
  it("shows Hz as GHz and missing input as a dash", () => {
    expect(formatGhz(3.204e9)).toBe("3.2 GHz");
    expect(formatGhz(4.512e9, { decimals: 2 })).toBe("4.51 GHz");
    expect(formatGhz(null)).toBe(MISSING);
  });
});
