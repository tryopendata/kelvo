import { formatClockSeconds } from "@core/format";
import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import { unwrap } from "@core/transport";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { HostStoreProvider } from "~/stores/host-store";
import MemoryRoute from "../memory/route";
import DiskRoute from "./route";

const T = 1_700_000_000_000; // a 10 s edge
const NOW = T + 5000;
const S = 1000;

const calls = (c: { command: string; args: unknown[] }[], command: string) =>
  c.filter((x) => x.command === command).map((x) => x.args.slice(1));

function render(page: React.ReactElement) {
  const transport = createMockTransport({
    now: () => NOW,
    autoTick: false,
    chartWindow: "5m",
  });
  return renderWithProviders(page, { transport, backfillMs: 600_000 });
}

// biome-ignore lint/complexity/noExcessiveLinesPerFunction: Existing scenario grouping; keep individual scenarios focused (architecture refactor section 10).
describe("Disk and Memory pages over a range (D-099)", () => {
  it("Disk: current process rates become retained interval totals on either brush", async () => {
    const { transport, user } = render(<DiskRoute />);
    const table = await screen.findByRole("table", {
      name: "Disk processes · Live",
    });
    expect(
      within(table).getByRole("columnheader", { name: "Total/s" })
    ).toHaveAttribute("aria-sort", "descending");
    await vi.waitFor(() =>
      expect(
        calls(transport.calls, "set_process_interest").length
      ).toBeGreaterThan(0)
    );
    const brushes = await screen.findAllByRole("slider", {
      name: "Select a time range",
    });
    expect(brushes).toHaveLength(2);
    act(() => brushes[1]?.focus());
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", {
        name: "Processes on this Mac · selected 10 s",
      })
    ).toBeVisible();
    expect(calls(transport.calls, "query_usage_by_app")).toContainEqual([
      T - 10 * S,
      T,
      "disk",
      200,
    ]);
    expect(screen.getAllByTestId("brush-band")).toHaveLength(2);
    const historical = screen.getByRole("table", {
      name: "Disk processes · selected 10 s",
    });
    expect(
      within(historical).getByRole("columnheader", { name: "Total Avg/s" })
    ).toBeVisible();
    expect(within(historical).getByText("6120 · Exited")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Live" }));
    expect(
      await screen.findByRole("table", { name: "Disk processes · Live" })
    ).toBeVisible();
  });

  it("keeps current process selection and bulk actions available", async () => {
    const { user } = render(<DiskRoute />);
    await screen.findByRole("table", { name: "Disk processes · Live" });
    await user.click(
      await screen.findByRole("checkbox", {
        name: "Select Xcode (2214)",
      })
    );
    expect(screen.getByText("1 selected", { exact: true })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Force Quit 1…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("Xcode");
  });

  it("renders the shared sleep gap on both Disk plots", async () => {
    const transport = createMockTransport({
      now: () => NOW,
      autoTick: false,
      chartWindow: "1h",
      scenarios: ["sleep-gap"],
    });
    renderWithProviders(<DiskRoute />, { transport });
    expect(
      await screen.findAllByRole("note", {
        name: /^Asleep .* not interpolated$/,
      })
    ).toHaveLength(2);
    const result = await unwrap(
      transport.queryDiskIo({
        host: MOCK_HOST_ID,
        device: "disk3",
        from_ms: NOW - 3_600_000,
        to_ms: NOW,
        max_points: 120,
      })
    );
    const sleep = result.gaps.find(
      (g) => g.reason === "sleep" && g.start_ms > NOW - 3_600_000
    );
    if (!sleep || sleep.end_ms === null)
      throw new Error("Missing closed sleep gap");
    const endMs = sleep.end_ms;
    const absent = result.points.filter(
      (p) => p.t >= sleep.start_ms && p.t < endMs
    );
    expect(absent.length).toBeGreaterThan(0);
    expect(
      absent.every((p) => p.read_bps === null && p.read_request_ms === null)
    ).toBe(true);
  });

  it("Escape clears a selected interval while its chart query is still loading", async () => {
    const { transport, user } = render(<DiskRoute />);
    const [brush] = await screen.findAllByRole("slider", {
      name: "Select a time range",
    });
    const query = transport.queryDiskIo.bind(transport);
    let release: (() => void) | undefined;
    transport.queryDiskIo = async (request) => {
      if (request.max_points > 1 && request.to_ms === T - 10 * S) {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      }
      return query(request);
    };
    try {
      act(() => brush?.focus());
      await user.keyboard("{ArrowLeft}{Enter}");
      expect(await screen.findByText("Loading disk I/O…")).toBeVisible();
      await user.keyboard("{Escape}");
      expect(
        await screen.findByRole("heading", {
          name: "Processes on this Mac · Live",
        })
      ).toBeVisible();
    } finally {
      release?.();
    }
  });

  it("lists a remote host's devices from its capabilities, without the local inventory (D-116)", async () => {
    const transport = createMockTransport({
      now: () => NOW,
      autoTick: false,
      chartWindow: "5m",
    });
    const remote = "remote-office";
    await unwrap(
      transport.addRemoteHost(
        {
          id: remote,
          name: "Office Mac",
          lan: "office",
          tailscale: null,
          enabled: true,
          mirror_budget_mb: 128,
        },
        false,
        false,
        "disk-test"
      )
    );
    // This peer supplies established byte-rate history but no new completion counters.
    const query = transport.queryDiskIo.bind(transport);
    transport.queryDiskIo = async (request) => {
      const result = await query(request);
      return result.status === "ok"
        ? {
            ...result,
            data: {
              ...result.data,
              points: result.data.points.map((p) => ({
                ...p,
                read_ops_per_sec: null,
                write_ops_per_sec: null,
                read_request_ms: null,
                write_request_ms: null,
              })),
            },
          }
        : result;
    };
    const { user } = renderWithProviders(
      <HostStoreProvider hostId={remote}>
        <DiskRoute />
      </HostStoreProvider>,
      { transport, route: "/dashboard/disk?device=disk3" }
    );
    expect(
      await screen.findByRole("img", { name: /^Disk I\/O: read above/ })
    ).toBeVisible();
    expect(
      screen.getByRole("combobox", { name: "Disk device" })
    ).toHaveTextContent("Apple SSD · disk3");
    expect(screen.queryByText("Couldn't load disk devices.")).toBeNull();
    expect(
      transport.calls.some((c) => c.command === "list_disk_io_devices")
    ).toBe(false);
    expect(screen.getByTestId("disk-response-current")).toHaveTextContent(
      "Read — · Unavailable · Write — · Unavailable"
    );
    const read = within(
      screen.getByRole("region", { name: "Recorded device byte totals" })
    ).getByText(/Read ·/);
    expect(read).toHaveTextContent(/Read · 5 min [0-9]/);
    const brush = screen.getAllByRole("slider", {
      name: "Select a time range",
    })[0];
    act(() => brush?.focus());
    await user.keyboard("{Enter}");
    await vi.waitFor(() =>
      expect(
        transport.calls
          .filter((c) => c.command === "query_disk_io")
          .every(
            (c) => (c.args[0] as { device: string | null }).device === "disk3"
          )
      ).toBe(true)
    );
    expect(
      screen.getByText("Process accounting is available on the monitored Mac.")
    ).toBeVisible();
  });

  it("reports a selected totals failure while leaving the chart and brush usable", async () => {
    const transport = createMockTransport({
      now: () => NOW,
      autoTick: false,
      chartWindow: "5m",
    });
    const query = transport.queryDiskIo.bind(transport);
    transport.queryDiskIo = async (request) =>
      request.max_points === 1
        ? {
            status: "error",
            error: { kind: "invalid_argument", message: "summary read failed" },
          }
        : query(request);
    const { user } = renderWithProviders(<DiskRoute />, { transport });
    const brush = (
      await screen.findAllByRole("slider", { name: "Select a time range" })
    )[0];
    act(() => brush?.focus());
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Couldn't load byte totals for the selected interval."
    );
    expect(
      screen.getByRole("img", { name: /^Disk I\/O: read above/ })
    ).toBeVisible();
    expect(
      screen.getAllByRole("slider", { name: "Select a time range" })
    ).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "Live" }));
    expect(
      screen.queryByText("Couldn't load byte totals for the selected interval.")
    ).toBeNull();
  });

  it("Memory: apps by peak footprint, with their average while running; either chart brushes", async () => {
    const { user } = render(<MemoryRoute />);
    const table = await screen.findByRole("table", { name: "Memory by app" });
    expect(
      within(table).getByRole("columnheader", { name: /Peak/ })
    ).toHaveAttribute("aria-sort", "descending");
    expect(
      within(table).getByRole("columnheader", { name: "Avg while running" })
    ).toBeVisible();
    // Peaks don't add up: no share column and no remainder rows.
    expect(within(table).queryByRole("columnheader", { name: "Share" })).toBe(
      null
    );
    expect(within(table).queryByText("System and other")).toBeNull();
    await vi.waitFor(() =>
      expect(screen.getByTestId("memory-range-totals")).toHaveTextContent(
        /Peak used · 5 min.+BPeak swap · 5 min.+B/
      )
    );
    const [, swap] = screen.getAllByRole("slider", {
      name: "Select a time range",
    });
    act(() => swap?.focus());
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", {
        name: "Memory by app, selected 10 s",
      })
    ).toBeVisible();
    // Both charts show the one selection.
    expect(screen.getAllByTestId("brush-band")).toHaveLength(2);
  });
});

for (const [scenario, text] of [
  ["disk-io-no-ops", "No completed operations"],
  ["disk-io-unavailable", "Unavailable"],
] as const) {
  it(`keeps ${scenario} distinct from zero request time`, async () => {
    renderWithProviders(<DiskRoute />, {
      transportOptions: {
        scenarios: [scenario],
        now: () => NOW,
        chartWindow: "5m",
      },
    });
    expect(
      await screen.findByTestId("disk-response-current")
    ).toHaveTextContent(text);
    expect(screen.getByTestId("disk-response-current")).not.toHaveTextContent(
      "0 ms"
    );
  });
}

it("keeps an unavailable selected device instead of silently changing scope", async () => {
  const { transport } = renderWithProviders(<DiskRoute />, {
    route: "/dashboard/disk?device=disk7",
    transportOptions: { now: () => NOW, chartWindow: "5m" },
  });
  expect(
    await screen.findByText(
      "This device is unavailable. Recorded history remains selected."
    )
  ).toBeVisible();
  expect(
    screen.getByRole("combobox", { name: "Disk device" })
  ).toHaveTextContent("disk7");
  await vi.waitFor(() =>
    expect(
      transport.calls.some(
        (c) =>
          c.command === "query_disk_io" &&
          (c.args[0] as { device: string }).device === "disk7"
      )
    ).toBe(true)
  );
});

it("discloses the recorded totals interval and each direction's measured coverage without changing the brush", async () => {
  const transport = createMockTransport({
    now: () => NOW,
    autoTick: false,
    chartWindow: "5m",
  });
  const read = transport.queryDiskIo;
  transport.queryDiskIo = async (request) => {
    const response = await read(request);
    if (request.max_points !== 1 || response.status !== "ok") return response;
    return {
      status: "ok",
      data: {
        ...response.data,
        from_ms: T - 60 * S,
        to_ms: T,
        read_bytes: 42e6,
        write_bytes: null,
        read_covered_ms: 30 * S,
        write_covered_ms: 0,
      },
    };
  };
  renderWithProviders(<DiskRoute />, {
    transport,
    route: `/dashboard/disk?device=disk3&from=${T - 10 * S}&to=${T}`,
  });
  const scope = await screen.findByText(/Recorded totals cover/);
  expect(scope).toHaveTextContent(
    `Recorded totals cover ${formatClockSeconds(T - 60 * S)} to ${formatClockSeconds(T)}. Selected interval: ${formatClockSeconds(T - 10 * S)} to ${formatClockSeconds(T)}.`
  );
  expect(
    screen.getByText(
      "Read measured for 30 s of 60 s; written measured for 0 s of 60 s."
    )
  ).toBeVisible();
  const totals = screen.getByLabelText("Recorded device byte totals");
  expect(totals).toHaveTextContent("Read · 60 s 42.0 MB");
  expect(totals).toHaveTextContent("Written · 60 s —");
  expect(
    screen.getByRole("heading", {
      name: "Processes on this Mac · selected 10 s",
    })
  ).toBeVisible();
});

it("does not imply complete measurement when totals have a gap inside the exact selection", async () => {
  const transport = createMockTransport({
    now: () => NOW,
    autoTick: false,
    chartWindow: "5m",
  });
  const read = transport.queryDiskIo;
  transport.queryDiskIo = async (request) => {
    const response = await read(request);
    return request.max_points === 1 && response.status === "ok"
      ? {
          status: "ok",
          data: {
            ...response.data,
            read_covered_ms: 5 * S,
            write_covered_ms: 10 * S,
          },
        }
      : response;
  };
  renderWithProviders(<DiskRoute />, {
    transport,
    route: `/dashboard/disk?device=disk3&from=${T - 10 * S}&to=${T}`,
  });
  expect(
    await screen.findByText(
      "Read measured for 5 s of 10 s; written measured for 10 s of 10 s."
    )
  ).toBeVisible();
  expect(screen.queryByText(/Recorded totals cover/)).toBeNull();
});
