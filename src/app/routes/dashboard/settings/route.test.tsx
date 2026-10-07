import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "../../../../../tests/test-utils";
import SettingsRoute from "./route";

const LIMITED = /Limited to about \d+ days by the 150 MB limit/;

/** The innermost element whose whole text matches, for text split by spans. */
const byText = (pattern: RegExp) =>
  screen.queryByText(
    (_, el) =>
      !!el?.textContent &&
      pattern.test(el.textContent) &&
      ![...el.children].some((c) => pattern.test(c.textContent ?? ""))
  );

const OVERHEAD =
  /Kelvo uses about ([\d.]+)% CPU at (\S+), this window included/;

/** The quoted overhead and the interval it names, or null while measuring. */
const overhead = () => {
  const m = byText(OVERHEAD)?.textContent?.match(OVERHEAD);
  return m ? { pct: Number(m[1]), at: m[2] } : null;
};

describe("Settings modules", () => {
  it("offers each module its own-item modes and writes the choice", async () => {
    const { transport, user } = renderWithProviders(<SettingsRoute />);
    await user.click(
      await screen.findByRole("combobox", { name: "CPU menu bar style" })
    );
    const cpu = (await screen.findAllByRole("option")).map(
      (o) => o.textContent
    );
    expect(cpu).toEqual([
      "In combined item",
      "Value + label",
      "Own item: graph",
      "Own item: cores",
      "Own item: value",
      "Hidden",
    ]);
    await user.click(screen.getByRole("option", { name: "Own item: cores" }));
    await waitFor(() =>
      expect(transport.calls).toContainEqual({
        command: "update_settings",
        args: [{ modules: { cpu: { menu_bar: "own_cores" } } }],
      })
    );

    // Cores is CPU's alone; Disk has no graph.
    await user.click(
      screen.getByRole("combobox", { name: "GPU menu bar style" })
    );
    expect(
      (await screen.findAllByRole("option")).map((o) => o.textContent)
    ).not.toContain("Own item: cores");
    await user.keyboard("{Escape}");
    await user.click(
      screen.getByRole("combobox", { name: "Battery menu bar style" })
    );
    expect(
      (await screen.findAllByRole("option")).map((o) => o.textContent)
    ).toEqual(["Value + label", "Own item: value", "Hidden"]);
  });
});

describe("Settings Performance mode (D-088)", () => {
  it("lists what it changes, writes the switch and holds the battery row", async () => {
    const { transport, user } = renderWithProviders(<SettingsRoute />);
    const toggle = await screen.findByRole("switch", {
      name: "Performance mode",
    });
    expect(toggle).not.toBeChecked();
    expect(toggle).toHaveAccessibleDescription(/Animations off/);
    // The mock's menu bar shows a temperature and slow-down is off.
    expect(screen.getByText("Interval doubles on battery")).toBeInTheDocument();
    expect(screen.queryByText(/Temperatures sampled/)).toBeNull();
    expect(
      screen.getByText(
        "A longer sample interval saves more while a window is open."
      )
    ).toBeInTheDocument();

    await user.click(toggle);
    await waitFor(() =>
      expect(transport.calls).toContainEqual({
        command: "update_settings",
        args: [{ sampling: { performance_mode: true } }],
      })
    );
    const battery = screen.getByRole("switch", {
      name: "Slow down on battery",
    });
    await waitFor(() => expect(battery).toBeDisabled());
    expect(battery).toBeChecked();
    expect(screen.getByText("Set by Performance mode")).toBeInTheDocument();
    // The stored value is kept, so turning the mode off restores it.
    expect(
      (await transport.getSettings()).settings.sampling.slow_on_battery
    ).toBe(false);
  });

  it("names the next lever: own menu bar items before the interval", async () => {
    const { transport } = renderWithProviders(<SettingsRoute />);
    await screen.findByRole("switch", { name: "Performance mode" });
    await transport.updateSettings({
      modules: { cpu: { menu_bar: "own_graph" } },
    });
    await waitFor(() =>
      expect(
        screen.getByText(/Separate menu bar items cost the most/)
      ).toBeInTheDocument()
    );
    await transport.updateSettings({
      modules: { cpu: { menu_bar: "in_combined" } },
      sampling: { interval_ms: 5000, slow_on_battery: true },
    });
    await waitFor(() => expect(screen.queryByText(/saves more/)).toBeNull());
    expect(screen.queryByText("Open windows update every 2 s")).toBeNull();
    expect(screen.queryByText("Interval doubles on battery")).toBeNull();
  });

  it("is on and locked while Low Power Mode holds it", async () => {
    const { transport } = renderWithProviders(<SettingsRoute />, {
      transportOptions: { scenarios: ["low-power-mode"] },
    });
    const toggle = await screen.findByRole("switch", {
      name: "Performance mode",
    });
    await waitFor(() => expect(toggle).toBeDisabled());
    expect(toggle).toBeChecked();
    expect(
      screen.getByText("On while Low Power Mode is on")
    ).toBeInTheDocument();
    expect(
      (await transport.getSettings()).settings.sampling.performance_mode
    ).toBe(false);
  });
});

describe("Settings sampling and history", () => {
  it("offers every interval and follows the battery slow-down to it", async () => {
    const { transport, user } = renderWithProviders(<SettingsRoute />);
    await screen.findByText("Sample interval");
    for (const label of ["0.5s", "1s", "2s", "5s", "10s", "30s", "60s"]) {
      expect(screen.getByRole("radio", { name: label })).toBeInTheDocument();
    }
    expect(screen.getByText("to 2s")).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: "30s" }));
    await waitFor(() => expect(screen.getByText("to 60s")).toBeInTheDocument());
    expect((await transport.getSettings()).settings.sampling.interval_ms).toBe(
      30_000
    );
  });

  it("fits 90 days of the mock's series under the default limit", async () => {
    const { transport } = renderWithProviders(<SettingsRoute />);
    await screen.findByText("Keep history");
    expect(screen.queryByText(LIMITED)).toBeNull();

    // Past 7 days history is 15-minute buckets (D-076): 90 days of a
    // typical host's series is well under 150 MB.
    await transport.updateSettings({ history: { retention_days: 90 } });
    await waitFor(() =>
      expect(screen.getByText("90 days")).toBeInTheDocument()
    );
    expect(screen.queryByText(LIMITED)).toBeNull();
    expect(screen.getByText(/^about [1-9]0 MB$/)).toBeInTheDocument();
  });

  it("projects history from what this Mac's history measured", async () => {
    renderWithProviders(<SettingsRoute />, {
      transportOptions: {
        historyGrowth: {
          measured_ms: 12 * 3_600_000,
          fixed_bytes: 36_000_000,
          minute_day_bytes: 2_050_000,
        },
      },
    });
    // 30 days: 36 MB, 7 minute days and 23 quarter days (D-076), about 54 MB.
    expect(await screen.findByText("about 50 MB")).toBeInTheDocument();
  });

  it("falls back to the fill-test model before an hour is recorded", async () => {
    const { transport } = renderWithProviders(<SettingsRoute />, {
      transportOptions: { historyGrowth: null },
    });
    await waitFor(() =>
      expect(transport.calls.map((c) => c.command)).toContain("history_growth")
    );
    // 30 days of the mock's series at the fill tests' rates.
    expect(await screen.findByText("about 60 MB")).toBeInTheDocument();
  });

  it("measures Kelvo's CPU again after the interval changes", async () => {
    let t = 1_800_000_000_000;
    const { transport, user } = renderWithProviders(<SettingsRoute />, {
      transportOptions: { now: () => t },
    });
    const advance = (ms: number, ticks: number) =>
      act(() => {
        for (let i = 0; i < ticks; i++) {
          t += ms;
          transport.tick();
        }
      });
    // Backfilled readings predate this window's first frame: not quoted.
    expect(
      await screen.findByText("Measuring Kelvo's CPU at 1s…")
    ).toBeInTheDocument();

    advance(1000, 40);
    await waitFor(() => expect(overhead()?.at).toBe("1s"));
    const atOne = overhead()?.pct ?? Number.NaN;

    await user.click(screen.getByRole("radio", { name: "30s" }));
    expect(
      await screen.findByText("Measuring Kelvo's CPU at 30s…")
    ).toBeInTheDocument();
    // The first reading spans the change; two more are needed.
    advance(30_000, 2);
    expect(overhead()).toBeNull();
    advance(30_000, 1);
    await waitFor(() => expect(overhead()?.at).toBe("30s"));
    expect(overhead()?.pct).toBeLessThan(atOne);
  });

  it("quotes no CPU figure while sampling is paused", async () => {
    let t = 1_800_000_000_000;
    const { transport } = renderWithProviders(<SettingsRoute />, {
      transportOptions: { now: () => t },
    });
    await screen.findByText("Measuring Kelvo's CPU at 1s…");
    act(() => {
      for (let i = 0; i < 40; i++) {
        t += 1000;
        transport.tick();
      }
    });
    await waitFor(() => expect(overhead()?.at).toBe("1s"));

    await act(() => transport.setPaused(true));
    expect(overhead()).toBeNull();
    expect(screen.queryByText(/Measuring Kelvo's CPU/)).toBeNull();
  });

  it("Network history is on by default and turns off through update_settings (D-089)", async () => {
    const { transport, user } = renderWithProviders(<SettingsRoute />);
    const toggle = await screen.findByRole("switch", {
      name: "Network history",
    });
    expect(toggle).toBeChecked();
    expect(
      screen.getByText(/Keeps which apps used the network, in 10 s steps/)
    ).toBeVisible();
    await user.click(toggle);
    await waitFor(() =>
      expect(transport.calls).toContainEqual({
        command: "update_settings",
        args: [{ history: { network_history: false } }],
      })
    );
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "Network history" })
      ).not.toBeChecked()
    );
  });

  it("has no Network history row without per-app network access", async () => {
    renderWithProviders(<SettingsRoute />, {
      transportOptions: { scenarios: ["no-process-network"] },
    });
    await screen.findByRole("switch", { name: "Slow down on battery" });
    expect(
      screen.queryByRole("switch", { name: "Network history" })
    ).toBeNull();
  });

  it("writes the size limit through update_settings", async () => {
    const { transport, user } = renderWithProviders(<SettingsRoute />);
    await user.click(
      await screen.findByRole("combobox", { name: "History size limit" })
    );
    await user.click(await screen.findByRole("option", { name: "500 MB" }));
    await waitFor(() =>
      expect(transport.calls).toContainEqual({
        command: "update_settings",
        args: [{ history: { size_limit_mb: 500 } }],
      })
    );
  });

  it("shows the low-disk warning and the trim note, and drops them on the event", async () => {
    const { transport } = renderWithProviders(<SettingsRoute />, {
      transportOptions: { scenarios: ["low-disk", "history-trimmed"] },
    });
    const warning = await screen.findByText(/History paused: disk almost full/);
    expect(warning.closest("[role=alert]")).not.toBeNull();
    const note = screen.getByText(/History trimmed to stay under 150 MB/);
    expect(note.closest("[role=status]")).not.toBeNull();

    transport.setHistoryHealth({
      low_disk_paused: false,
      trimmed_before_ms: null,
      trimmed_limit_bytes: null,
      cap_met: true,
    });
    await waitFor(() =>
      expect(screen.queryByText(/History paused/)).toBeNull()
    );
    expect(screen.queryByText(/History trimmed/)).toBeNull();
    expect(transport.calls.map((c) => c.command)).toContain("history_health");
    expect(
      transport.calls.find((c) => c.command === "history_health")?.args
    ).toEqual([MOCK_HOST_ID]);
  });

  it("shows the unavailable banner when there is no store", async () => {
    renderWithProviders(<SettingsRoute />, {
      transportOptions: { scenarios: ["history-unavailable"] },
    });
    expect(
      await screen.findByText("History is unavailable. Live values still work.")
    ).toBeInTheDocument();
  });
});
