import type { HostInfo } from "@core/generated/bindings";
import { chipStatus, defaultEnabled } from "./setup";

const info = (over: Partial<HostInfo>): HostInfo => ({
  os: "mac_os",
  os_version: "26.0",
  model: "Mac16,8",
  chip: "Apple M4 Pro",
  chip_known: true,
  cpu_topology: [],
  mem_total_bytes: 24e9,
  boot_time_ms: 0,
  ...over,
});

describe("chipStatus", () => {
  it("names a mapped chip without the Apple prefix", () => {
    expect(chipStatus(info({}))).toBe("M4 Pro detected · all sensors mapped");
  });

  it("names the model when the sensor map does not know the chip", () => {
    expect(
      chipStatus(
        info({ model: "Mac17,4", chip: "Apple M5 Pro", chip_known: false })
      )
    ).toBe("Mac17,4 detected · sensors not mapped yet");
  });
});

describe("defaultEnabled", () => {
  it("switches present modules on, Disk off, absent ones off", () => {
    const on = defaultEnabled({
      revision: 1,
      modules: {
        cpu: { available: { series: 10 } },
        disk: { available: { series: 4 } },
        battery: "not_present",
      },
    });
    expect(on.cpu).toBe(true);
    expect(on.disk).toBe(false);
    expect(on.battery).toBe(false);
    expect(on.gpu).toBe(false);
  });
});
