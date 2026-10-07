import type {
  AppUsage,
  ProcessUsage,
  UsageByApp,
} from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { UsageAppsCard, type UsageTableConfig } from "./usage-apps-card";

const NOW = 1_800_000_000_000;
const WINDOW_MS = 600_000;

const CONFIG: UsageTableConfig = {
  by: "energy",
  noun: "Energy",
  accent: "power",
  columns: [
    { label: "Energy", format: (u) => `${u.energy_j} J`, ranked: true },
  ],
  footnote: "CPU energy per process.",
};

function proc(
  pid: number,
  name: string,
  j: number,
  over: Partial<ProcessUsage> = {}
): ProcessUsage {
  return {
    pid,
    start_time_us: pid * 1000,
    name,
    cpu_avg_pct: null,
    gpu_avg_pct: null,
    mem_peak_bytes: 0,
    read_bytes: null,
    write_bytes: null,
    energy_j: j,
    avg_w: j / 600,
    running: true,
    refusal: null,
    ...over,
  };
}

function app(
  name: string,
  quit: number | null,
  processes: ProcessUsage[]
): AppUsage {
  const j = processes.reduce((s, p) => s + (p.energy_j ?? 0), 0);
  return {
    name,
    cpu_avg_pct: null,
    gpu_avg_pct: null,
    mem_peak_bytes: 0,
    mem_avg_bytes: 0,
    read_bytes: null,
    write_bytes: null,
    energy_j: j,
    avg_w: j / 600,
    quit_pid: quit,
    processes,
  };
}

const APPS: AppUsage[] = [
  app("Google Chrome", 10, [
    proc(11, "Google Chrome Helper (Renderer)", 300),
    proc(10, "Google Chrome", 100),
    proc(12, "Google Chrome Helper", 50, { running: false }),
  ]),
  app("WindowServer", 391, [
    proc(391, "WindowServer", 80, { refusal: "window_server" }),
  ]),
];

function render(answer: (fromMs: number, toMs: number) => UsageByApp | null) {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  transport.queryUsageByApp = async (_host, fromMs, toMs) => {
    const data = answer(fromMs, toMs);
    return data === null
      ? {
          status: "error",
          error: { kind: "remote_host", host: "other" },
        }
      : { status: "ok", data };
  };
  const r = renderWithProviders(
    <UsageAppsCard windowMs={WINDOW_MS} config={CONFIG} />,
    { transport }
  );
  act(() => transport.tick());
  return r;
}

const full = (fromMs: number, toMs: number): UsageByApp => ({
  from_ms: fromMs,
  to_ms: toMs,
  since_ms: fromMs - 60_000,
  complete_to_ms: toMs,
  covered_ms: toMs - fromMs,
  gpu_covered_ms: 0,
  total: {
    cpu_avg_pct: null,
    gpu_avg_pct: null,
    read_bytes: null,
    write_bytes: null,
    energy_j: 530,
    avg_w: 530 / 600,
  },
  other: {
    cpu_avg_pct: null,
    gpu_avg_pct: null,
    read_bytes: null,
    write_bytes: null,
    clamped: [],
  },
  apps: APPS,
});

const table = () => screen.findByRole("table", { name: "Energy by app" });

describe("UsageAppsCard (D-093, D-099)", () => {
  it("quits an app through its main process, after the dialog", async () => {
    const { user, transport } = render(full);
    await table();
    await user.click(
      screen.getByRole("button", { name: "Quit Google Chrome (10)" })
    );
    const dialog = await screen.findByRole("alertdialog", {
      name: "Quit Google Chrome?",
    });
    expect(dialog).toHaveTextContent("PID 10");
    await user.click(within(dialog).getByRole("button", { name: "Quit" }));
    await waitFor(() =>
      expect(
        transport.calls.filter((c) => c.command === "process_signal")
      ).toEqual([
        {
          command: "process_signal",
          args: [expect.any(String), 10, 10_000, "quit"],
        },
      ])
    );
  });

  it("expands an app to its processes; exited ones have no actions", async () => {
    const { user } = render(full);
    await table();
    expect(screen.queryByText("Google Chrome Helper")).toBeNull();
    await user.click(
      screen.getByRole("button", { name: "Show Google Chrome processes" })
    );
    const exited = screen.getByText("Google Chrome Helper").closest("tr");
    if (!exited) throw new Error("no row");
    expect(exited).toHaveTextContent("exited");
    expect(within(exited).queryByRole("button")).toBeNull();
    expect(
      screen.getByRole("button", {
        name: "Quit Google Chrome Helper (Renderer) (11)",
      })
    ).toBeInTheDocument();
  });

  it("disables Quit on a process macOS needs", async () => {
    render(full);
    await table();
    expect(
      screen.getByRole("button", { name: "Quit WindowServer (391)" })
    ).toHaveAttribute("aria-disabled", "true");
  });

  it("search opens the app holding a matched process", async () => {
    const { user } = render(full);
    await table();
    await user.type(
      screen.getByRole("searchbox", {
        name: "Search Energy by app, process or PID",
      }),
      "renderer"
    );
    const rows = within(await table()).getAllByRole("row");
    const text = rows.map((r) => r.textContent ?? "");
    expect(
      text.some((t) => t.includes("Google Chrome Helper (Renderer)"))
    ).toBe(true);
    expect(text.some((t) => t.includes("WindowServer"))).toBe(false);
  });

  it("says when counting started inside the window", async () => {
    render((fromMs, toMs) => ({
      ...full(fromMs, toMs),
      since_ms: toMs - 120_000,
      covered_ms: 120_000,
    }));
    expect(
      await screen.findByText(/Kelvo started counting at/)
    ).toHaveTextContent("so these figures cover");
  });

  it("explains a host whose use is not kept here", async () => {
    render(() => null);
    expect(
      await screen.findByText(
        "Use by app is only kept on the Mac it describes."
      )
    ).toBeVisible();
  });
});
