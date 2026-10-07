import type {
  AppEnergy,
  EnergyByApp,
  ProcessEnergy,
} from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { EnergyCard } from "./energy-card";

const NOW = 1_800_000_000_000;
const WINDOW_MS = 600_000;

function proc(
  pid: number,
  name: string,
  j: number,
  over: Partial<ProcessEnergy> = {}
): ProcessEnergy {
  return {
    pid,
    start_time_us: pid * 1000,
    name,
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
  processes: ProcessEnergy[]
): AppEnergy {
  const j = processes.reduce((s, p) => s + (p.energy_j ?? 0), 0);
  return { name, energy_j: j, avg_w: j / 600, quit_pid: quit, processes };
}

const APPS: AppEnergy[] = [
  app("Google Chrome", 10, [
    proc(11, "Google Chrome Helper (Renderer)", 300),
    proc(10, "Google Chrome", 100),
    proc(12, "Google Chrome Helper", 50, { running: false }),
  ]),
  app("WindowServer", 391, [
    proc(391, "WindowServer", 80, { refusal: "window_server" }),
  ]),
];

function render(answer: (fromMs: number, toMs: number) => EnergyByApp | null) {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  transport.queryEnergyByApp = async (_host, fromMs, toMs) => {
    const data = answer(fromMs, toMs);
    return data === null
      ? {
          status: "error",
          error: { kind: "remote_host", host: "other" },
        }
      : { status: "ok", data };
  };
  const r = renderWithProviders(<EnergyCard windowMs={WINDOW_MS} />, {
    transport,
  });
  act(() => transport.tick());
  return r;
}

const full = (fromMs: number, toMs: number): EnergyByApp => ({
  from_ms: fromMs,
  to_ms: toMs,
  since_ms: fromMs - 60_000,
  measured_ms: toMs - fromMs,
  total_j: 530,
  apps: APPS,
});

const table = () => screen.findByRole("table", { name: "Energy by app" });

describe("Energy by app (D-093)", () => {
  it("quits an app through its main process, after the dialog", async () => {
    const { user, transport } = render(full);
    await table();
    await user.click(
      screen.getByRole("button", { name: "Quit Google Chrome (10)" })
    );
    const dialog = await screen.findByRole("dialog", {
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
        name: "Search energy by app, process or PID",
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
      measured_ms: 120_000,
    }));
    expect(
      await screen.findByText(/Kelvo started counting at/)
    ).toHaveTextContent("so these totals cover");
  });

  it("explains a host whose energy is not kept here", async () => {
    render(() => null);
    expect(
      await screen.findByText(
        "Energy by app is only kept on the Mac it describes."
      )
    ).toBeVisible();
  });
});
