import type { ScenarioName } from "@core/mock/fixtures";
import { createMockTransport, type MockTransport } from "@core/mock-transport";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { App } from "./app";

const transports: MockTransport[] = [];

afterEach(() => {
  cleanup();
  for (const transport of transports.splice(0)) transport.dispose();
});

function mount(scenarios: ScenarioName[] = []) {
  const source = createMockTransport({
    scenarios,
    historyRows: 60,
    autoTick: false,
  });
  transports.push(source);
  render(
    <App transport={source} router={{ initialEntry: "/dashboard/hosts" }} />
  );
  return source;
}

async function check(alias: string) {
  fireEvent.click(await screen.findByRole("button", { name: "Add host" }));
  fireEvent.change(screen.getByLabelText("Tailscale SSH alias"), {
    target: { value: alias },
  });
  fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
}

function hostRow(name: string) {
  const row = screen.getByRole("heading", { name }).parentElement
    ?.parentElement;
  if (!row) throw new Error(`Missing host row: ${name}`);
  return within(row);
}

describe("Linux hosts", () => {
  it("discloses the system service before installing, then adds the server", async () => {
    const source = mount();
    await check("linux-tailscale");
    expect(
      await screen.findByText(/Linux 6\.17 · aarch64 · systemd 256/)
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /at \/usr\/local\/bin\/kelvo-agent as a system service that runs as root/
      )
    ).toBeInTheDocument();
    expect(
      screen.getByText(/150 MB of history in \/var\/lib\/kelvo-agent/)
    ).toBeInTheDocument();
    expect(screen.getByText(/SELinux is enforcing/)).toBeInTheDocument();
    expect(
      screen.queryByLabelText("Show menu-bar icon on remote Mac")
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Install and connect" })
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    const [added] = await source.listRemoteHosts();
    expect(added?.os).toBe("linux");
    expect(
      source.calls.find((call) => call.command === "add_remote_host")?.args[1]
    ).toBe(true);
  });

  it("shows the enable-linger command for a user service", async () => {
    mount();
    await check("linux-user");
    expect(
      await screen.findByText(/as a user service for riley/)
    ).toBeInTheDocument();
    expect(
      screen.getByText("sudo loginctl enable-linger riley")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
    expect(
      screen.getByText(/can’t read the Docker socket/)
    ).toBeInTheDocument();
  });

  it("offers an update for an older agent", async () => {
    mount();
    await check("linux-update");
    expect(
      await screen.findByText(/Kelvo agent 0\.0\.9 is installed/)
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Update and connect" })
    ).toBeInTheDocument();
  });

  it("connects to a newer agent as it is, never downgrading it", async () => {
    const source = mount();
    await check("linux-newer");
    expect(
      await screen.findByText(/newer agent than this app/)
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Update and connect" })
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(
      source.calls.find((call) => call.command === "add_remote_host")?.args[1]
    ).toBe(false);
  });

  it("notes the Docker socket only when it is there but unusable", async () => {
    mount();
    await check("linux-nodocker");
    expect(
      await screen.findByText(/as a system service that runs as root/)
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/can’t read the Docker socket/)
    ).not.toBeInTheDocument();
  });

  it("resets a duplicate identity before adding", async () => {
    const source = mount(["linux-host"]);
    await screen.findByRole("heading", { name: "linux-tailscale" });
    await check("linux-dup");
    expect(
      await screen.findByText(/same identity as linux-tailscale/)
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Reset identity and connect" })
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    const commands = source.calls.map((call) => call.command);
    expect(commands).toContain("reset_remote_host_identity");
    expect(commands.indexOf("reset_remote_host_identity")).toBeLessThan(
      commands.indexOf("add_remote_host")
    );
    expect(await source.listRemoteHosts()).toHaveLength(2);
  });

  it("explains a low-disk refusal", async () => {
    mount();
    await check("linux-lowdisk");
    expect(
      await screen.findByText("The server is low on disk space.")
    ).toBeInTheDocument();
  });

  it("updates an older agent from the host list", async () => {
    const source = mount(["linux-host", "linux-agent-update"]);
    await screen.findByRole("heading", { name: "linux-tailscale" });
    fireEvent.click(
      hostRow("linux-tailscale").getByRole("button", { name: "Update agent" })
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Update agent" })
      ).not.toBeInTheDocument()
    );
    expect(
      source.calls.find((call) => call.command === "update_remote_agent")?.args
    ).toEqual(["remote-linux"]);
  });

  it("re-pairs a host with a new identity and keeps the old one offline", async () => {
    const source = mount(["linux-host", "linux-identity-changed"]);
    await screen.findByRole("heading", { name: "linux-tailscale" });
    const row = hostRow("linux-tailscale");
    expect(
      row.getByText("This host now answers with a different identity.")
    ).toBeInTheDocument();
    expect(
      row.queryByRole("button", { name: "Retry" })
    ).not.toBeInTheDocument();
    fireEvent.click(row.getByRole("button", { name: "Re-pair…" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Re-pair as a new host" })
    );
    expect(
      await screen.findByRole("heading", {
        name: "linux-tailscale (before re-pair)",
      })
    ).toBeInTheDocument();
    const hosts = await source.listRemoteHosts();
    expect(hosts.map((h) => [h.config.name, h.config.enabled])).toEqual([
      ["linux-tailscale (before re-pair)", false],
      ["linux-tailscale", true],
    ]);
  });

  it("uninstalls the agent by default when removing a Linux host", async () => {
    const source = mount(["linux-host"]);
    await screen.findByRole("heading", { name: "linux-tailscale" });
    fireEvent.click(
      hostRow("linux-tailscale").getByRole("button", { name: "Settings" })
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove host…" }));
    expect(
      screen.queryByLabelText("Also stop sharing on that Mac")
    ).not.toBeInTheDocument();
    expect(
      screen.getByLabelText("Also uninstall the agent from that server")
    ).toBeChecked();
    const purge = screen.getByLabelText(
      "Delete the agent’s history on that server"
    );
    expect(purge).not.toBeChecked();
    fireEvent.click(purge);
    fireEvent.click(screen.getByRole("button", { name: "Remove host" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(
      source.calls.find((call) => call.command === "remove_remote_host")?.args
    ).toEqual(["remote-linux", false, false, true, true]);
  });
});
