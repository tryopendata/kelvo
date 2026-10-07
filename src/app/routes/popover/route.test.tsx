import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { POPOVER_SERIES } from "./_lib/series";
import PopoverRoute from "./route";

const NOW = 1_800_000_000_000;

/** The popover's text after one frame, with or without the projection. */
async function popoverText(projected: boolean) {
  const r = renderWithProviders(<PopoverRoute />, {
    transportOptions: { now: () => NOW, scenarios: ["cpu-power-seeded"] },
    series: projected ? POPOVER_SERIES : undefined,
  });
  await screen.findByText("Open dashboard");
  act(() => r.transport.tick());
  const text = document.body.textContent ?? "";
  r.unmount();
  return text;
}

describe("popover series projection (D-066)", () => {
  it("reads the same on the projected channel as on every series", async () => {
    const full = await popoverText(false);
    const projected = await popoverText(true);
    // A metric the projection left out would read as a gap here.
    expect(projected).toBe(full);
    expect(full).toContain("estimated from last calibration");
  });
});

describe("popover cards", () => {
  it("open their module page in the dashboard", async () => {
    const { transport, user } = renderWithProviders(<PopoverRoute />, {
      transportOptions: { now: () => NOW },
    });
    await screen.findByText("Open dashboard");
    act(() => transport.tick());
    await user.click(screen.getByRole("link", { name: "CPU" }));
    await user.click(screen.getByRole("link", { name: "Cores" }));
    await user.click(screen.getByRole("link", { name: "Memory" }));
    const opened = transport.calls
      .filter((c) => c.command === "open_dashboard")
      .map((c) => c.args[0]);
    expect(opened).toEqual([
      "/dashboard/cpu",
      "/dashboard/cpu",
      "/dashboard/memory",
    ]);
  });
});
