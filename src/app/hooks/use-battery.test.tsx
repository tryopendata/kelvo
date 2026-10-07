import type { BatteryHour } from "@core/battery-hours";
import { createMockTransport } from "@core/mock-transport";
import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useBatteryHours } from "./use-battery";

let hours: BatteryHour[] | undefined;

function Probe() {
  hours = useBatteryHours().hours;
  return <p>{hours ? "loaded" : "loading"}</p>;
}

describe("useBatteryHours", () => {
  it("draws Rust's rows for the 24 local hours ending with the current one", async () => {
    const transport = createMockTransport({ autoTick: false });
    let asked: number[] = [];
    transport.batteryHours = async (_host, starts) => {
      asked = starts;
      return {
        status: "ok",
        data: starts.slice(0, -1).map((start_ms, i) => ({
          start_ms,
          charge: i === 23 ? 81 : null,
          charging: i === 23,
        })),
      };
    };
    renderWithProviders(<Probe />, { transport });
    await screen.findByText("loaded");
    expect(asked).toHaveLength(25);
    const now = Date.now();
    expect(asked[23]).toBeLessThanOrEqual(now);
    expect(asked[24]).toBeGreaterThan(now);
    expect(hours?.[23]).toEqual({
      tsMs: asked[23],
      charge: 81,
      charging: true,
    });
    // Hours without samples stay gaps, never 0.
    expect(hours?.[0]?.charge).toBeNull();
  });
});
