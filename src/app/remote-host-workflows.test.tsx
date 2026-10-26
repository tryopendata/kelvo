import type { CommandError } from "@core/generated/bindings";
import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport, type MockTransport } from "@core/mock-transport";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { createMemoryRouter } from "react-router";
import { useHostSelection } from "~/lib/host-selection";
import { QueryProvider } from "~/lib/query";
import { TransportProvider } from "~/lib/transport-context";
import { LaneTableDialog } from "~/routes/dashboard/timeline/_components/lane-table-dialog";
import { useTimelineData } from "~/routes/dashboard/timeline/_hooks/use-timeline-data";
import { LANES } from "~/routes/dashboard/timeline/_lib/lanes";
import { HostStoreProvider } from "~/stores/host-store";
import { SettingsProvider } from "~/stores/settings-store";
import { App, LocalHost } from "./app";

const unavailable = (message: string) => ({
  status: "error" as const,
  error: { kind: "invalid_argument", message } satisfies CommandError,
});
const transports: MockTransport[] = [];

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  for (const transport of transports.splice(0)) transport.dispose();
});

function transport() {
  const result = createMockTransport({
    scenarios: ["multi-host"],
    historyRows: 60,
    autoTick: false,
  });
  transports.push(result);
  return result;
}

function mount(source: MockTransport) {
  render(
    <App transport={source} router={{ initialEntry: "/dashboard/hosts" }} />
  );
}

function hostRow(name = "Studio Mac") {
  const row = screen.getByRole("heading", { name }).parentElement
    ?.parentElement;
  if (!row) throw new Error(`Missing host row: ${name}`);
  return within(row);
}

// biome-ignore lint/complexity/noExcessiveLinesPerFunction: Existing scenario grouping; keep individual scenarios focused (architecture refactor section 10).
describe("remote Mac recovery workflows", () => {
  it("offers interactive trust verification separately from the saved-host SSH test", async () => {
    const source = transport();
    mount(source);
    await screen.findByRole("heading", { name: "Studio Mac" });
    fireEvent.click(hostRow().getByRole("button", { name: "Settings" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "o'brien@studio.local" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Show SSH setup guide" })
    );
    expect(
      screen.getByText(/BatchMode=no.*o'\\''brien@studio.local/)
    ).toHaveTextContent("StrictHostKeyChecking=ask");
    expect(
      screen.getByText(/BatchMode=yes.*o'\\''brien@studio.local/)
    ).toHaveTextContent("StrictHostKeyChecking=yes");
    expect(
      screen.getByText(/for key in \/etc\/ssh\/ssh_host_/)
    ).toHaveTextContent("ssh-keygen -lf");
  });

  it("keeps installation failure diagnostics visible without diagnosing a network problem", async () => {
    const source = transport();
    source.addRemoteHost = async () => ({
      status: "error",
      error: {
        kind: "remote_unreachable",
        message: "A packaged Kelvo.app with bundled helper is required.",
      },
    });
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "office.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A packaged Kelvo.app with bundled helper is required."
    );
    expect(screen.getByRole("alert")).not.toHaveTextContent("could not reach");
  });

  it("guides missing-config setup without blocking a direct SSH connection", async () => {
    const source = transport();
    source.remoteSshConfigStatus = async () => ({
      status: "ok",
      data: "missing",
    });
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    expect(
      await screen.findByText(/No ~\/\.ssh\/config found/)
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Hide SSH setup guide" })
    ).toHaveAttribute("aria-expanded", "true");
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "user@office.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    expect(
      await screen.findByRole("button", { name: "Connect" })
    ).toBeEnabled();
  });

  it("reports a failed config check and lets the user recheck after fixing it", async () => {
    const source = transport();
    source.remoteSshConfigStatus = async () =>
      unavailable("Cannot read config metadata");
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    expect(
      await screen.findByText(/Could not check ~\/\.ssh\/config/)
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/No ~\/\.ssh\/config found/)
    ).not.toBeInTheDocument();
    source.remoteSshConfigStatus = async () => ({
      status: "ok",
      data: "present",
    });
    fireEvent.click(screen.getByRole("button", { name: "Recheck config" }));
    expect(
      await screen.findByText(/Found ~\/\.ssh\/config/)
    ).toBeInTheDocument();
  });

  it("explains authentication recovery, retains the diagnostic, and preserves entries", async () => {
    const source = transport();
    source.prepareRemoteHost = async () => ({
      status: "error",
      error: { kind: "remote_auth", message: "Permission denied (publickey)." },
    });
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "user@office.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "unlock your key in your SSH agent"
    );
    expect(
      screen.getByText("Permission denied (publickey).")
    ).toBeInTheDocument();
    expect(screen.getByLabelText("LAN SSH alias")).toHaveValue(
      "user@office.local"
    );
    expect(
      screen.getByRole("button", { name: "Check connection" })
    ).toBeEnabled();
  });

  it("clears obsolete host verification when the SSH address changes", async () => {
    const source = transport();
    source.prepareRemoteHost = async () => ({
      status: "error",
      error: { kind: "host_key_unknown", message: "Unknown key" },
    });
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "old.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    await screen.findByRole("button", { name: "Verify LAN host" });
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "new.local" },
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Verify LAN host" })
    ).not.toBeInTheDocument();
  });

  it("offers a fresh verification after the trust token expires", async () => {
    const source = transport();
    source.prepareRemoteHost = async () => ({
      status: "error",
      error: { kind: "host_key_unknown", message: "Unknown key" },
    });
    source.acceptRemoteSshHost = async () =>
      unavailable("SSH verification expired. Check the host again");
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "office.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Verify LAN host" })
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Trust this host" })
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "verification expired"
    );
    expect(
      screen.queryByRole("button", { name: "Trust this host" })
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Verify LAN host" })
    ).toBeEnabled();
  });

  it("explains pasted SSH commands before trying to connect", async () => {
    const source = transport();
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "ssh user@office.local" },
    });
    expect(screen.getByRole("alert")).toHaveTextContent("without “ssh”");
    expect(
      screen.getByRole("button", { name: "Check connection" })
    ).toBeDisabled();
    expect(
      source.calls.some((call) => call.command === "prepare_remote_host")
    ).toBe(false);
  });

  it.each(["remote_incompatible", "host_key_unknown"] as const)(
    "offers host verification only for a host-key error: %s",
    async (kind) => {
      const source = transport();
      source.prepareRemoteHost = async () => ({
        status: "error",
        error: {
          kind,
          message:
            kind === "remote_incompatible"
              ? "Requires an Apple Silicon Mac"
              : "Verify the host key",
        },
      });
      mount(source);
      fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
      fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
        target: { value: "mac.local" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
      await screen.findByRole("alert");
      if (kind === "host_key_unknown") {
        expect(
          screen.getByRole("button", { name: "Verify LAN host" })
        ).toBeInTheDocument();
      } else {
        expect(
          screen.queryByRole("button", { name: "Verify LAN host" })
        ).not.toBeInTheDocument();
      }
    }
  );

  it("does not re-render the route tree when a poll changes only last_seen_ms", async () => {
    const source = transport();
    const list = source.listRemoteHosts;
    let polls = 0;
    source.listRemoteHosts = async () =>
      (await list()).map((host) => ({
        ...host,
        status: { ...host.status, last_seen_ms: ++polls },
      }));
    let renders = 0;
    function RouteProbe() {
      renders++;
      const selection = useHostSelection();
      return <p>Selected {selection?.selected}</p>;
    }
    vi.useFakeTimers();
    const router = createMemoryRouter([{ path: "/", element: <RouteProbe /> }]);
    await act(async () => {
      render(
        <TransportProvider transport={source}>
          <QueryProvider>
            <SettingsProvider>
              <LocalHost router={router} />
            </SettingsProvider>
          </QueryProvider>
        </TransportProvider>
      );
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByText(/^Selected /)).toBeInTheDocument();
    expect(polls).toBeGreaterThan(0);
    const before = renders;
    const beforePolls = polls;
    await act(() => vi.advanceTimersByTimeAsync(5001));
    expect(polls).toBeGreaterThan(beforePolls);
    expect(renders).toBe(before);
    router.dispose();
  });

  it("rejects empty, fractional and out-of-range history limits before saving", async () => {
    const source = transport();
    mount(source);
    await screen.findByRole("heading", { name: "Studio Mac" });
    fireEvent.click(hostRow().getByRole("button", { name: "Settings" }));
    for (const value of ["", "9", "10001", "10.5"]) {
      fireEvent.change(screen.getByLabelText("History limit (MB)"), {
        target: { value },
      });
      expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
      expect(screen.getByRole("alert")).toHaveTextContent("10 to 10,000 MB");
    }
    fireEvent.change(screen.getByLabelText("History limit (MB)"), {
      target: { value: "10000" },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect((await source.listRemoteHosts())[0]?.config.mirror_budget_mb).toBe(
      10000
    );
  });

  it.each([true, false])(
    "defaults stop-sharing on and sends the chosen value (%s) while removing the Mac",
    async (stopSharing) => {
      const source = transport();
      mount(source);
      await screen.findByRole("heading", { name: "Studio Mac" });
      fireEvent.click(hostRow().getByRole("button", { name: "Settings" }));
      fireEvent.click(screen.getByRole("button", { name: "Remove host…" }));
      expect(
        screen.getByLabelText("Also stop sharing on that Mac")
      ).toBeChecked();
      if (!stopSharing)
        fireEvent.click(screen.getByLabelText("Also stop sharing on that Mac"));
      fireEvent.click(screen.getByRole("button", { name: "Remove host" }));
      await waitFor(() =>
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
      );
      expect(await source.listRemoteHosts()).toHaveLength(0);
      expect(
        source.calls.find((call) => call.command === "remove_remote_host")?.args
      ).toEqual(["remote-studio", false, stopSharing, false, false]);
    }
  );

  it("reports a stop-sharing warning after the host is successfully removed", async () => {
    const source = transport();
    const remove = source.removeRemoteHost;
    source.removeRemoteHost = async (...args) => {
      const result = await remove(...args);
      return result.status === "ok"
        ? {
            status: "ok",
            data: "The Mac was removed, but sharing could not be stopped.",
          }
        : result;
    };
    mount(source);
    await screen.findByRole("heading", { name: "Studio Mac" });
    fireEvent.click(hostRow().getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove host…" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove host" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(await source.listRemoteHosts()).toHaveLength(0);
    expect(
      await screen.findByText(
        "The Mac was removed, but sharing could not be stopped."
      )
    ).toBeInTheDocument();
  });

  it("keeps a failed setup editable and connects the corrected Mac on retry", async () => {
    const source = transport();
    const add = source.addRemoteHost;
    source.addRemoteHost = async (...args) =>
      args[0].lan === "unreachable.local"
        ? unavailable(
            "SSH connection timed out. Check the address and try again."
          )
        : add(...args);
    mount(source);
    fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "Office Mac" },
    });
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "unreachable.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "SSH connection timed out"
    );
    expect(
      screen.queryByRole("heading", { name: "Office Mac" })
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("LAN SSH alias")).toBeEnabled();
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "office.local" },
    });
    expect(
      screen.queryByRole("button", { name: "Connect" })
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await screen.findByRole("heading", { name: "Office Mac" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(
      hostRow("Office Mac").getByRole("button", { name: "View metrics" })
    );
    await screen.findByRole("heading", { name: "Overview" });
    expect(
      screen.getByRole("combobox", { name: "Selected machine" })
    ).toHaveTextContent("Office Mac");
    await waitFor(() =>
      expect(
        screen.queryByRole("link", { name: /^Battery/ })
      ).not.toBeInTheDocument()
    );
  });

  it("shows failed reconnect feedback and restores live summaries after a successful retry", async () => {
    const source = transport();
    const records = await source.listRemoteHosts();
    const remote = records[0];
    if (!remote) throw new Error("Missing fixture host");
    remote.status = {
      ...remote.status,
      state: "offline",
      error: "Both SSH routes are unavailable",
    };
    const retry = source.retryRemoteHost;
    let attempts = 0;
    source.retryRemoteHost = async (id) =>
      ++attempts === 1
        ? unavailable("The Mac is still unreachable")
        : retry(id);
    mount(source);
    await screen.findByText("Offline");
    expect(hostRow().getByText(/Last seen/)).toBeInTheDocument();
    expect(hostRow().queryByText("18%")).not.toBeInTheDocument();
    fireEvent.click(hostRow().getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The Mac is still unreachable"
    );
    expect(hostRow().getByText("Offline")).toBeInTheDocument();
    fireEvent.click(hostRow().getByRole("button", { name: "Retry" }));
    await screen.findByText("LAN · connected");
    expect(hostRow().getByText("18%")).toBeInTheDocument();
    expect(hostRow().getByText("42%")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      hostRow().queryByRole("button", { name: "Retry" })
    ).not.toBeInTheDocument();
  });

  it("preserves a Mac when removal fails and returns to local metrics after successful removal", async () => {
    const source = transport();
    const remove = source.removeRemoteHost;
    let attempts = 0;
    source.removeRemoteHost = async (...args) =>
      ++attempts === 1
        ? unavailable("Could not save host settings")
        : remove(...args);
    mount(source);
    await screen.findByRole("heading", { name: "Studio Mac" });
    fireEvent.click(hostRow().getByRole("button", { name: "View metrics" }));
    await screen.findByRole("heading", { name: "Overview" });
    expect(
      screen.getByRole("combobox", { name: "Selected machine" })
    ).toHaveTextContent("Studio Mac");
    fireEvent.click(screen.getByRole("link", { name: "Hosts" }));
    await screen.findByRole("heading", { name: "Hosts" });
    fireEvent.click(hostRow().getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove host…" }));
    expect(
      screen.getByLabelText("Delete mirrored history from this Mac")
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Remove host" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not save host settings"
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByLabelText("Name")).toHaveValue("Studio Mac");
    fireEvent.click(screen.getByRole("button", { name: "Remove host…" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove host" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(
      screen.queryByRole("heading", { name: "Studio Mac" })
    ).not.toBeInTheDocument();
    await screen.findByText(/Add a Linux server or another Mac over SSH/);
    fireEvent.click(screen.getByRole("link", { name: "Overview" }));
    await screen.findByRole("heading", { name: "Overview" });
    expect(
      screen.getByRole("combobox", { name: "Selected machine" })
    ).toHaveTextContent("This Mac");
    await screen.findByRole("link", { name: /^Battery/ });
  });

  it("reads the selected Mac's mirrored timeline even when its live connection fails", async () => {
    const source = transport();
    const records = await source.listRemoteHosts();
    const remote = records[0];
    if (!remote) throw new Error("Missing fixture host");
    remote.status = { ...remote.status, state: "offline" };
    const subscribe = source.subscribeLive;
    source.subscribeLive = async (host, callback, options) =>
      host === "remote-studio"
        ? { info: unavailable("Remote Mac is offline"), unsubscribe: () => {} }
        : subscribe(host, callback, options);
    const history = source.queryHistory;
    // biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing scenario grouping; keep individual scenarios focused (architecture refactor section 10).
    source.queryHistory = async (request) => {
      const result = await history(request);
      if (result.status === "ok") {
        for (const series of result.data.series) {
          if (series.key.metric === "cpu.total") {
            series.points = [
              {
                t: request.from_ms + 60_000,
                min: request.host === "remote-studio" ? 37 : 83,
                avg: request.host === "remote-studio" ? 37 : 83,
                max: request.host === "remote-studio" ? 37 : 83,
              },
            ];
          }
        }
      }
      return result;
    };
    // Render the production table and history hook; happy-dom has no canvas.
    // The IPC response distinguishes hosts so a leaked query cache is observable.
    function HistoryTable() {
      const data = useTimelineData(
        { windowMs: 86_400_000, endMs: null },
        LANES.filter((lane) => lane.id === "cpu")
      );
      const lane = data.lanes[0];
      return (
        <LaneTableDialog
          lane={lane ?? null}
          bands={[]}
          fromMs={data.fromMs}
          toMs={data.toMs}
          units={{ rate: "MBps", temperature: "C" }}
          onClose={() => {}}
        />
      );
    }
    const tree = (host: string) => (
      <TransportProvider transport={source}>
        <QueryProvider>
          <SettingsProvider>
            <HostStoreProvider key={host} hostId={host}>
              <HistoryTable />
            </HostStoreProvider>
          </SettingsProvider>
        </QueryProvider>
      </TransportProvider>
    );
    const view = render(tree("remote-studio"));
    await waitFor(() =>
      expect(screen.getAllByRole("cell", { name: "37%" })).toHaveLength(3)
    );
    expect(screen.queryByRole("cell", { name: "83%" })).not.toBeInTheDocument();
    view.rerender(tree(MOCK_HOST_ID));
    await waitFor(() =>
      expect(screen.getAllByRole("cell", { name: "83%" })).toHaveLength(3)
    );
    expect(screen.queryByRole("cell", { name: "37%" })).not.toBeInTheDocument();
    view.rerender(tree("remote-studio"));
    await waitFor(() =>
      expect(screen.getAllByRole("cell", { name: "37%" })).toHaveLength(3)
    );
    expect(screen.queryByRole("cell", { name: "83%" })).not.toBeInTheDocument();
  });
});
