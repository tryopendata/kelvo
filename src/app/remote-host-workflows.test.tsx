import type { CommandError } from "@core/generated/bindings";
import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport, type MockTransport } from "@core/mock-transport";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryProvider } from "~/lib/query";
import { TransportProvider } from "~/lib/transport-context";
import { LaneTableDialog } from "~/routes/dashboard/timeline/_components/lane-table-dialog";
import { useTimelineData } from "~/routes/dashboard/timeline/_hooks/use-timeline-data";
import { LANES } from "~/routes/dashboard/timeline/_lib/lanes";
import { HostStoreProvider } from "~/stores/host-store";
import { SettingsProvider } from "~/stores/settings-store";
import { App } from "./app";

const unavailable = (message: string) => ({
  status: "error" as const,
  error: { kind: "invalid_argument", message } satisfies CommandError,
});
const transports: MockTransport[] = [];

afterEach(() => {
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

describe("remote Mac recovery workflows", () => {
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
    fireEvent.click(await screen.findByRole("button", { name: "Add Mac" }));
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
    fireEvent.click(screen.getByRole("button", { name: "Remove Mac…" }));
    expect(
      screen.getByLabelText("Delete mirrored history from this Mac")
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Remove Mac" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not save host settings"
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByLabelText("Name")).toHaveValue("Studio Mac");
    fireEvent.click(screen.getByRole("button", { name: "Remove Mac…" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove Mac" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(
      screen.queryByRole("heading", { name: "Studio Mac" })
    ).not.toBeInTheDocument();
    await screen.findByText(/Add a Mac to view its live metrics/);
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
        { span: "24h", endMs: null },
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
