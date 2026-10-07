import { formatClockSeconds } from "@core/format";
import type {
  ChartWindow,
  NetworkByApp,
  ProcessView,
  SettingsSnapshot,
} from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { BrushProvider } from "~/stores/brush-store";
import { AppsCard } from "./_components/apps-card";
import NetworkRoute from "./route";

const T = 1_700_000_000_000; // a 10 s edge
const NOW = T + 5000;
const S = 1000;

const views = (calls: { command: string; args: unknown[] }[]) =>
  calls
    .filter((c) => c.command === "set_process_interest")
    .map((c) => c.args[2] as ProcessView | null);
const byAppCalls = (calls: { command: string; args: unknown[] }[]) =>
  calls
    .filter((c) => c.command === "query_network_by_app")
    .map((c) => [c.args[1], c.args[2]] as [number, number]);

const appsTable = () => screen.findByRole("table", { name: "Network by app" });
const names = (table: HTMLElement) =>
  within(table)
    .getAllByRole("row")
    .slice(1)
    .map((r) => within(r).queryAllByRole("cell")[0]?.textContent ?? "");

/** The page at a 5m chart window unless `chartWindow` says otherwise. */
function renderRoute(scenarios?: string[], chartWindow: ChartWindow = "5m") {
  return renderWithProviders(<NetworkRoute />, {
    backfillMs: 600_000,
    transportOptions: {
      now: () => NOW,
      chartWindow,
      ...(scenarios ? { scenarios: scenarios as never } : {}),
    },
  });
}

/** The Apps card alone over a pinned range, against the mock's history. */
function renderCard(fromMs: number, toMs: number, transport = mock()) {
  return renderWithProviders(
    <BrushProvider initial={{ fromMs, toMs }}>
      <AppsCard windowMs={300_000} />
    </BrushProvider>,
    { transport, backfillMs: 600_000 }
  );
}

const mock = () =>
  createMockTransport({ now: () => NOW, autoTick: false, chartWindow: "5m" });

describe("Network page Apps card (D-089)", () => {
  it("lists apps over the whole window with the remainder rows, by total", async () => {
    const { transport } = renderRoute();
    const table = await appsTable();
    expect(
      screen.getByRole("heading", { name: "Apps, last 5 minutes" })
    ).toBeVisible();
    const rows = names(table);
    // The mock's Docker burst ended 60 s ago, inside the window.
    expect(rows).toContain("DDocker Desktop");
    expect(rows.slice(-4)).toEqual([
      "Other apps",
      "Protocol overhead (est.)",
      "System and other",
      "macOS services (updates, backups, DNS). Kelvo can't see these without a helper.",
    ]);
    expect(
      within(table).getByRole("columnheader", { name: /Total/ })
    ).toHaveAttribute("aria-sort", "descending");
    // The window is the closed buckets before the open one; "now" is the
    // latest closed bucket of the same command.
    expect(byAppCalls(transport.calls)).toEqual(
      expect.arrayContaining([
        [T - 300 * S, T],
        [T - 10 * S, T],
      ])
    );
    expect(
      screen.getByText("Drag across the chart to see what used it.")
    ).toBeVisible();
    // Per-app history replaces the live per-process rates.
    expect(views(transport.calls)).toEqual([]);
  });

  it("sorts by now on request", async () => {
    const { user } = renderRoute();
    const table = await appsTable();
    await user.click(within(table).getByRole("button", { name: /Now/ }));
    expect(
      within(table).getByRole("columnheader", { name: /Now/ })
    ).toHaveAttribute("aria-sort", "descending");
    expect(
      within(table).getByRole("columnheader", { name: /Total/ })
    ).not.toHaveAttribute("aria-sort");
  });

  it("reads the whole window once per 10 s bucket, not every tick", async () => {
    let clock = T + 1000;
    const transport = createMockTransport({
      now: () => clock,
      autoTick: false,
      chartWindow: "5m",
    });
    renderWithProviders(<NetworkRoute />, { transport, backfillMs: 600_000 });
    await appsTable();
    // 1 s past T the per-app stream (4 s past each edge) has reported only
    // to T − 6 s, so the bucket before T is still open: the window ends at
    // T − 10 s, and "now" is the bucket before that.
    expect(byAppCalls(transport.calls)).toEqual(
      expect.arrayContaining([
        [T - 10 * S, T],
        [T - 310 * S, T - 10 * S],
        [T - 20 * S, T - 10 * S],
      ])
    );
    expect(byAppCalls(transport.calls)).not.toContainEqual([T - 300 * S, T]);
    const before = byAppCalls(transport.calls).length;
    for (let i = 0; i < 8; i++) {
      clock += 1000;
      act(() => transport.tick());
    }
    await act(async () => {});
    expect(byAppCalls(transport.calls).length).toBe(before);
    clock += 1000; // T + 10 s: the bucket before T has closed
    act(() => transport.tick());
    await vi.waitFor(() =>
      expect(byAppCalls(transport.calls)).toEqual(
        expect.arrayContaining([
          [T, T + 10 * S],
          [T - 300 * S, T],
          [T - 10 * S, T],
        ])
      )
    );
  });

  it("selecting the spike puts its app first; the selection stays pinned off the chart until cleared", async () => {
    // The 15m default, so a burst 5 to 15 minutes ago is on the chart.
    const { user } = renderRoute(undefined, "15m");
    expect(
      await screen.findByRole("heading", { name: "Apps, last 15 minutes" })
    ).toBeVisible();
    // An earlier Docker burst is [NOW − 390 s, NOW − 360 s) = [T − 385 s, T − 355 s).
    const brush = screen.getByRole("slider", { name: "Select a time range" });
    act(() => brush.focus());
    // Focus starts on [T − 10 s, T), the newest bucket that has happened.
    await user.keyboard("{ArrowLeft>37/}{Enter}{Shift>}{ArrowRight}{/Shift}");
    expect(
      await screen.findByRole("heading", { name: "Apps, selected 20 s" })
    ).toBeVisible();
    await vi.waitFor(async () =>
      expect(names(await appsTable())[0]).toBe("DDocker Desktop")
    );
    const from = formatClockSeconds(T - 380 * S);
    const to = formatClockSeconds(T - 360 * S);
    expect(screen.getByTestId("selection-summary")).toHaveTextContent(
      new RegExp(
        `^${from} to ${to} · .+ down · .+ up · Click the chart or press Esc to clear$`
      )
    );
    expect(screen.getByTestId("selection-summary")).not.toHaveTextContent(
      "earlier than this chart"
    );

    // 5m: the chart now starts after the selection.
    await user.click(screen.getByRole("radio", { name: "5m" }));
    await vi.waitFor(() =>
      expect(screen.getByTestId("selection-summary")).toHaveTextContent(
        "· earlier than this chart"
      )
    );
    expect(screen.queryByTestId("brush-band")).toBeNull();
    // Nothing on the chart is selected, so nothing on it dims.
    const bars = screen.getByRole("img", {
      name: /last 5 minutes\. Drag to select/,
    });
    expect(bars.querySelectorAll(".opacity-35")).toHaveLength(0);
    expect(
      screen.getByRole("heading", { name: "Apps, selected 20 s" })
    ).toBeVisible();
    expect(names(await appsTable())[0]).toBe("DDocker Desktop");
    // The chip, in the card header.
    expect(
      screen.getByRole("button", { name: "Clear selection" }).parentElement
    ).toHaveTextContent(`${from}to${to}`);

    await user.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(
      await screen.findByRole("heading", { name: "Apps, last 5 minutes" })
    ).toBeVisible();
    expect(
      screen.getByText("Drag across the chart to see what used it.")
    ).toBeVisible();
  });

  it("reads nothing until settings say which window to read", async () => {
    const transport = mock();
    let load: (s: SettingsSnapshot) => void = () => {};
    const loaded = transport.getSettings();
    transport.getSettings = () =>
      new Promise((resolve) => {
        load = resolve;
      });
    renderWithProviders(<NetworkRoute />, { transport, backfillMs: 600_000 });
    expect(
      await screen.findByRole("heading", { name: "Network" })
    ).toBeVisible();
    await act(async () => {});
    // The page waits for its window: no chart, no Apps card, no read.
    expect(screen.queryByRole("heading", { name: /^Apps/ })).toBeNull();
    expect(screen.queryByRole("img", { name: /last/ })).toBeNull();
    expect(byAppCalls(transport.calls)).toEqual([]);

    const snapshot = await loaded;
    await act(async () => load(snapshot));
    expect(
      await screen.findByRole("heading", { name: "Apps, last 5 minutes" })
    ).toBeVisible();
    await vi.waitFor(() =>
      expect(byAppCalls(transport.calls)).toContainEqual([T - 300 * S, T])
    );
  });

  it("partial coverage: says how much of the range was measured", async () => {
    // Per-app history starts 40 s after the ring's first row (NOW − 599 s).
    renderCard(T - 600 * S, T - 500 * S);
    expect(await screen.findByText(/^Measured for/)).toHaveTextContent(
      /^Measured for 54 of 100 s\. Network history wasn't recording for the rest, so these totals cover 54 s\.$/
    );
    expect(await appsTable()).toBeVisible();
  });

  it("a range before collection started names when it started", async () => {
    renderCard(T - 900 * S, T - 800 * S);
    await vi.waitFor(() =>
      expect(screen.getByText(/^No app data/)).toHaveTextContent(
        `No app data · Network history started ${formatClockSeconds(T - 560 * S)}`
      )
    );
    expect(screen.queryByRole("table", { name: "Network by app" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Apps, selected 100 s" })
    ).toBeVisible();
  });

  it("a selection reaching an open bucket says so, and no time reads as missing", async () => {
    // 1 s past T the per-app stream has reported only to T − 6 s.
    const transport = createMockTransport({
      now: () => T + 1000,
      autoTick: false,
    });
    renderCard(T - 60 * S, T, transport);
    await appsTable();
    expect(document.body).toHaveTextContent(
      "The last 10 s are still being measured. These totals update as they close."
    );
    expect(document.body).not.toHaveTextContent("Measured for");
  });

  it("clamped: says app totals exceed the interface, and why they can", async () => {
    const transport = mock();
    const clamped: NetworkByApp = {
      from_ms: T - 120 * S,
      to_ms: T,
      complete_to_ms: T,
      resolution_ms: 10_000,
      measured_ms: 120_000,
      coverage: [{ from_ms: T - 120 * S, to_ms: T, tier: "s10" }],
      apps: [
        { name: "Tailscale", rx_bytes: 118e6, tx_bytes: 8e6 },
        { name: "Google Chrome", rx_bytes: 114e6, tx_bytes: 7.4e6 },
      ],
      other_apps_rx_bytes: 0,
      other_apps_tx_bytes: 0,
      iface_rx_bytes: 120e6,
      iface_tx_bytes: 8e6,
      overhead_rx_bytes: 0,
      overhead_tx_bytes: 0,
      system_rx_bytes: 0,
      system_tx_bytes: 0,
      clamped: true,
    };
    transport.queryNetworkByApp = async () => ({ status: "ok", data: clamped });
    renderCard(T - 120 * S, T, transport);
    expect(
      await screen.findByText(/^App totals here exceed the interface/)
    ).toHaveTextContent(
      "App totals here exceed the interface (tunnelled traffic, or bytes counted when their app was identified), so System and other reads 0."
    );
    const table = await appsTable();
    expect(names(table).slice(0, 2)).toEqual(["TTailscale", "GGoogle Chrome"]);
    expect(within(table).getByText("98.4%")).toBeVisible();
  });

  it("a failed read says why instead of an empty table", async () => {
    const transport = mock();
    transport.queryNetworkByApp = async () => ({
      status: "error",
      error: { kind: "store", message: "disk I/O error" },
    });
    renderCard(T - 120 * S, T, transport);
    expect(
      await screen.findByText(
        "History is unavailable: disk I/O error. Live values still work."
      )
    ).toBeVisible();
    expect(screen.queryByRole("table", { name: "Network by app" })).toBeNull();
  });

  it("a failed read with nothing selected says why instead of loading forever", async () => {
    const transport = mock();
    transport.queryNetworkByApp = async (host) => ({
      status: "error",
      error: { kind: "remote_host", host },
    });
    renderWithProviders(<NetworkRoute />, { transport, backfillMs: 600_000 });
    expect(
      await screen.findByText(
        "Per-app network history is kept only for this Mac."
      )
    ).toBeVisible();
    expect(screen.queryByText("Loading app totals…")).toBeNull();
  });

  it("history unavailable: the Apps card answers from the engine's last hour", async () => {
    renderRoute(["history-unavailable"]);
    const table = await appsTable();
    expect(names(table)).toContain("GGoogle Chrome");
  });

  it("with Network history off: live rates only, no brush, a way to turn it on", async () => {
    const transport = mock();
    await transport.updateSettings({ history: { network_history: false } });
    renderWithProviders(<NetworkRoute />, { transport, backfillMs: 600_000 });
    expect(
      await screen.findByRole("heading", { name: "Apps, now" })
    ).toBeVisible();
    expect(
      screen.getByRole("link", { name: "Turn it on in Settings" })
    ).toHaveAttribute("href", "/dashboard/settings");
    const table = await screen.findByRole("table", {
      name: "Network by app, now",
    });
    await vi.waitFor(() =>
      expect(within(table).getAllByRole("row")[1]).toHaveTextContent(
        "Safari22.1 MB/s"
      )
    );
    expect(views(transport.calls).at(-1)).toMatchObject({ network: true });
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.queryByText(/Drag across the chart/)).toBeNull();
    expect(byAppCalls(transport.calls)).toEqual([]);
  });

  it("without per-app access: no Apps card, no brush, no hint", async () => {
    const { transport } = renderRoute(["no-process-network"]);
    await screen.findByRole("heading", { name: "Interfaces" });
    expect(screen.queryByRole("heading", { name: /^Apps/ })).toBeNull();
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.queryByText(/Drag across the chart/)).toBeNull();
    expect(views(transport.calls)).toEqual([]);
    expect(byAppCalls(transport.calls)).toEqual([]);
  });
});
