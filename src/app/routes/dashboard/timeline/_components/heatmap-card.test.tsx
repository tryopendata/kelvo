import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { Profiler } from "react";
import { renderWithProviders } from "../../../../../../tests/test-utils";
import { HeatmapCard } from "./heatmap-card";

const UNITS = { rate: "MBps", temperature: "C" } as const;
const DAY = 86_400_000;

function setup(onOpen = vi.fn(), scenarios: string[] = []) {
  let commits = 0;
  const transport = createMockTransport({
    autoTick: false,
    scenarios: scenarios as never,
  });
  const result = renderWithProviders(
    <Profiler id="heatmap" onRender={() => (commits += 1)}>
      <HeatmapCard units={UNITS} onOpen={onOpen} />
    </Profiler>,
    { transport }
  );
  const heatmapCalls = () =>
    transport.calls.filter((c) => c.command === "query_heatmap");
  return { ...result, onOpen, heatmapCalls, commits: () => commits };
}

const grid = () => screen.getByRole("grid");
const cells = () => within(grid()).getAllByRole("gridcell");

describe("HeatmapCard", () => {
  it("draws 30 local days by 24 hours with a name on every cell", async () => {
    const { heatmapCalls } = setup();
    await waitFor(() =>
      expect(
        cells().some((c) =>
          /average CPU \d+%$/.test(c.getAttribute("aria-label") ?? "")
        )
      ).toBe(true)
    );
    expect(within(grid()).getAllByRole("row")).toHaveLength(30);
    expect(cells()).toHaveLength(720);
    // Nights are empty: named "no samples", not drawn as 0%.
    expect(
      cells().some((c) => c.getAttribute("aria-label")?.endsWith("no samples"))
    ).toBe(true);
    const req = heatmapCalls()[0]?.args[0] as {
      metric: string;
      days: { hour_starts: number[] }[];
    };
    expect(req.metric).toBe("cpu");
    expect(req.days).toHaveLength(30);
    for (const d of req.days) expect(d.hour_starts).toHaveLength(25);
    expect(screen.getByText("0%")).toBeInTheDocument();
    expect(screen.getByText("80%+")).toBeInTheDocument();
  });

  it("switches to temperature on its own scale and reads it once", async () => {
    const { user, heatmapCalls } = setup();
    await waitFor(() => expect(heatmapCalls()).toHaveLength(1));
    await user.click(screen.getByRole("radio", { name: "Temperature" }));
    await waitFor(() =>
      expect(
        cells().some((c) =>
          /average temperature \d+ °C$/.test(c.getAttribute("aria-label") ?? "")
        )
      ).toBe(true)
    );
    expect(screen.getByText("40 °C")).toBeInTheDocument();
    expect(screen.getByText("90 °C+")).toBeInTheDocument();
    expect(heatmapCalls()[1]?.args[0]).toMatchObject({ metric: "temp" });
    // Back to CPU within the hour is a cache hit.
    await user.click(screen.getByRole("radio", { name: "Avg CPU" }));
    expect(heatmapCalls()).toHaveLength(2);
  });

  it("moves with the arrow keys and opens a cell with Enter or a click", async () => {
    const onOpen = vi.fn();
    const { user } = setup(onOpen);
    await waitFor(() => expect(cells()).toHaveLength(720));
    // One tab stop, on the current hour.
    const stops = cells().filter((c) => c.tabIndex === 0);
    expect(stops).toHaveLength(1);
    const hour = new Date().getHours();
    expect(stops[0]?.getAttribute("data-cell")).toBe(`29-${hour}`);

    stops[0]?.focus();
    await user.keyboard("{ArrowUp}{ArrowUp}{Home}");
    expect(document.activeElement?.getAttribute("data-cell")).toBe("27-0");
    expect(cells().filter((c) => c.tabIndex === 0)[0]).toBe(
      document.activeElement
    );
    await user.keyboard("{Enter}");
    const recent = onOpen.mock.calls[0]?.[0];
    expect(recent.span).toBe("1h");
    expect(recent.endMs).toBeLessThan(Date.now());

    // Row 0 is 29 days back: only quarters are left, so 6 hours.
    await user.click(grid().querySelector('[data-cell="0-14"]') as HTMLElement);
    const old = onOpen.mock.calls[1]?.[0];
    expect(old.span).toBe("6h");
    expect(Date.now() - old.endMs).toBeGreaterThan(28 * DAY);
  });

  it("does not re-render or re-read on live ticks", async () => {
    const { transport, heatmapCalls, commits } = setup();
    await waitFor(() => expect(cells()).toHaveLength(720));
    await waitFor(() =>
      expect(
        cells().some((c) =>
          /average CPU/.test(c.getAttribute("aria-label") ?? "")
        )
      ).toBe(true)
    );
    const before = commits();
    await act(async () => {
      for (let i = 0; i < 120; i++) transport.tick();
    });
    expect(commits()).toBe(before);
    expect(heatmapCalls()).toHaveLength(1);
  });

  it("says so when there is no history instead of drawing empty days", async () => {
    setup(vi.fn(), ["history-unavailable"]);
    expect(
      await screen.findByText(/No history is being kept/)
    ).toBeInTheDocument();
    expect(screen.queryByRole("grid")).toBeNull();
  });
});

describe("HeatmapCard over the day", () => {
  // 14:20 local: hours 15 to 23 of today are still ahead.
  const NOW = new Date(2026, 9, 5, 14, 20);
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(NOW);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  const cell = (r: number, h: number) =>
    grid().querySelector(`[data-cell="${r}-${h}"]`) as HTMLElement;

  it("names today's later hours 'not yet' and does not open them", async () => {
    const { onOpen, user } = setup();
    await waitFor(() =>
      expect(cell(29, 9).getAttribute("aria-label")).toMatch(/average CPU/)
    );
    const later = cell(29, 23);
    expect(later.getAttribute("aria-label")).toBe("Oct 5, 23:00, not yet");
    expect(later).toHaveAttribute("aria-disabled", "true");
    expect(cell(29, 14)).not.toHaveAttribute("aria-disabled");
    // Not "no samples": nothing was missed, the hour has not come.
    for (let h = 15; h < 24; h++) {
      expect(cell(29, h).getAttribute("aria-label")).toMatch(/not yet$/);
    }
    await user.click(later);
    later.focus();
    await user.keyboard("{Enter}");
    expect(onOpen).not.toHaveBeenCalled();
    await user.click(cell(29, 9));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("reads the hour again every 5 minutes, as the writer commits", async () => {
    const { heatmapCalls } = setup();
    await waitFor(() => expect(heatmapCalls()).toHaveLength(1));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5 * 60_000);
    });
    await waitFor(() => expect(heatmapCalls()).toHaveLength(2));
    const [a, b] = heatmapCalls().map(
      (c) => (c.args[0] as { days: { date: string }[] }).days
    );
    // The same 30 days: a re-read within the hour, not a new hour.
    expect(b).toEqual(a);
  });
});

describe("HeatmapCard just after the hour turns", () => {
  // 14:02: the writer has not committed any of hour 14 yet (D-070).
  const NOW = new Date(2026, 9, 5, 14, 2);
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(NOW);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("names the current hour 'no data yet' and still opens it, at Live", async () => {
    const transport = createMockTransport({ autoTick: false });
    const real = transport.queryHeatmap.bind(transport);
    transport.queryHeatmap = async (req) => {
      const res = await real(req);
      if (res.status !== "ok") return res;
      return {
        ...res,
        data: res.data.map((d) =>
          d.date === "2026-10-05"
            ? { ...d, hours: d.hours.map((v, h) => (h === 14 ? null : v)) }
            : d
        ),
      };
    };
    const onOpen = vi.fn();
    const { user } = renderWithProviders(
      <HeatmapCard units={UNITS} onOpen={onOpen} />,
      { transport }
    );
    const cell = () =>
      grid().querySelector('[data-cell="29-14"]') as HTMLElement;
    await waitFor(() => expect(grid()).toHaveAttribute("aria-busy", "false"));
    expect(cell().getAttribute("aria-label")).toBe(
      "Oct 5, 14:00, this hour, no data yet"
    );
    expect(cell()).not.toHaveAttribute("aria-disabled");
    // Earlier hours that day did sample, so this is commit lag, not a gap.
    expect(
      (grid().querySelector('[data-cell="29-13"]') as HTMLElement).getAttribute(
        "aria-label"
      )
    ).toMatch(/average CPU/);
    await user.click(cell());
    expect(onOpen).toHaveBeenCalledWith({ span: "1h", endMs: null });
  });
});

describe("HeatmapCard while loading", () => {
  it("draws neither data nor gaps until the cells arrive, and says it is busy", async () => {
    const transport = createMockTransport({ autoTick: false });
    const real = transport.queryHeatmap.bind(transport);
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => {
      release = r;
    });
    transport.queryHeatmap = async (req) => {
      await gate;
      return real(req);
    };
    renderWithProviders(<HeatmapCard units={UNITS} onOpen={vi.fn()} />, {
      transport,
    });
    await waitFor(() => expect(cells()).toHaveLength(720));
    expect(grid()).toHaveAttribute("aria-busy", "true");
    const names = cells().map((c) => c.getAttribute("aria-label") ?? "");
    expect(names.some((n) => n.endsWith("no samples"))).toBe(false);
    expect(names.filter((n) => n.endsWith(", loading")).length).toBeGreaterThan(
      600
    );

    await act(async () => release());
    await waitFor(() => expect(grid()).toHaveAttribute("aria-busy", "false"));
    expect(
      cells().some((c) => c.getAttribute("aria-label")?.endsWith("no samples"))
    ).toBe(true);
  });
});
