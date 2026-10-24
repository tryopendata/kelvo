import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import DiskRoute from "./route";

const NOW = 1_791_000_000_000;
const HOUR = 3_600_000;
const END = NOW - HOUR;
const fixed = (end: number) => ({
  absolute: { from_ms: end - HOUR, to_ms: end },
});

it("keeps fixed Disk chart bounds when brushing and returns to a rolling range with Live", async () => {
  let now = NOW;
  const { transport, user } = renderWithProviders(<DiskRoute />, {
    transportOptions: {
      now: () => now,
      autoTick: false,
      chartWindow: fixed(END),
    },
  });
  const [brush] = await screen.findAllByRole("slider", {
    name: "Select a time range",
  });
  expect(screen.getByRole("button", { name: "Live" })).toHaveAttribute(
    "aria-pressed",
    "false"
  );
  const requests = () =>
    transport.calls
      .filter((c) => c.command === "query_disk_io")
      .map(
        (c) =>
          c.args[0] as { from_ms: number; to_ms: number; max_points: number }
      );
  const chartsBefore = requests().filter((r) => r.max_points > 1).length;
  act(() => brush?.focus());
  await user.keyboard("{ArrowLeft}{Enter}");
  await screen.findByTestId("selection-summary");
  expect(requests().filter((r) => r.max_points > 1)).toHaveLength(chartsBefore);
  expect(
    requests()
      .filter((r) => r.max_points === 1)
      .at(-1)?.to_ms
  ).toBeLessThan(END);
  await user.click(screen.getByRole("button", { name: "Live" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Live" })).toHaveAttribute(
      "aria-pressed",
      "true"
    )
  );
  await waitFor(() => expect(requests().at(-1)?.to_ms).toBe(NOW));
  expect(screen.queryByTestId("selection-summary")).toBeNull();
  now += 20_000;
  await act(() => transport.tick());
  await waitFor(() => expect(requests().at(-1)?.to_ms).toBe(now));
});

it.each(["fixed", "rolling"])(
  "does not show the previous fixed Disk answer while a %s range loads",
  async (mode) => {
    const transport = createMockTransport({
      now: () => NOW,
      autoTick: false,
      chartWindow: fixed(END),
    });
    const query = transport.queryDiskIo.bind(transport);
    let release: (() => void) | undefined;
    transport.queryDiskIo = async (request) => {
      if (
        request.to_ms === (mode === "fixed" ? END - HOUR : NOW) &&
        request.max_points > 1
      ) {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      }
      return query(request);
    };
    renderWithProviders(<DiskRoute />, { transport });
    await screen.findAllByRole("slider", { name: "Select a time range" });
    try {
      await act(() =>
        transport.updateSettings({
          general: {
            chart_window: mode === "fixed" ? fixed(END - HOUR) : "1h",
          },
        })
      );
      await waitFor(() => expect(release).toBeDefined());
      expect(screen.getByText("Loading disk I/O…")).toBeVisible();
      expect(
        screen.queryByRole("slider", { name: "Select a time range" })
      ).toBeNull();
    } finally {
      await act(async () => release?.());
    }
    expect(
      await screen.findAllByRole("slider", { name: "Select a time range" })
    ).toHaveLength(2);
  }
);
