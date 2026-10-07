import { createMockTransport } from "@core/mock-transport";
import { render, waitFor } from "@testing-library/react";
import { App } from "./app";

/** The backfill the popover's first `subscribe_live` asked for. */
async function popoverBackfill(scenarios: ("default" | "low-power-mode")[]) {
  const transport = createMockTransport({
    windowLabel: "popover",
    scenarios,
    autoTick: false,
  });
  const r = render(
    <App transport={transport} router={{ initialEntry: "/popover" }} />
  );
  let backfill: unknown;
  await waitFor(() => {
    const call = transport.calls.find((c) => c.command === "subscribe_live");
    expect(call).toBeDefined();
    backfill = call?.args[1];
  });
  r.unmount();
  return backfill;
}

describe("popover backfill (D-088)", () => {
  it("covers 60 samples at the interval Low Power Mode doubles", async () => {
    expect(await popoverBackfill(["default"])).toBe(60_000);
    expect(await popoverBackfill(["low-power-mode"])).toBe(120_000);
  });
});
