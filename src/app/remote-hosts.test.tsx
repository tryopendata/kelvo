import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { App } from "./app";

function setup(remoteSetupDelayMs?: number) {
  const transport = createMockTransport({
    scenarios: ["multi-host"],
    historyRows: 60,
    remoteSetupDelayMs,
  });
  const view = render(
    <App transport={transport} router={{ initialEntry: "/dashboard/hosts" }} />
  );
  return { transport, ...view };
}

describe("remote host selection", () => {
  it("scopes subscriptions and capabilities to the selected Mac and keeps Settings local", async () => {
    const { transport, unmount } = setup();
    const title = await screen.findByRole("heading", { name: "Studio Mac" });
    const row = title.parentElement?.parentElement;
    if (!row) throw new Error("Missing host row");
    fireEvent.click(
      within(row).getByRole("button", {
        name: "View metrics",
      })
    );
    await waitFor(() =>
      expect(
        transport.calls.some(
          (call) =>
            call.command === "subscribe_live" &&
            call.args[0] === "remote-studio"
        )
      ).toBe(true)
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("link", { name: /^Battery/ })
      ).not.toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole("link", { name: "Processes" }));
    await screen.findByRole("heading", { name: "Processes" });
    expect(
      screen.queryByRole("button", { name: /Quit/ })
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "Settings" }));
    await screen.findByRole("heading", { name: "Settings" });
    await waitFor(() =>
      expect(
        transport.calls
          .filter((call) => call.command === "subscribe_live")
          .at(-1)?.args[0]
      ).toBe(MOCK_HOST_ID)
    );
    fireEvent.click(screen.getByRole("link", { name: "Overview" }));
    await waitFor(() =>
      expect(
        transport.calls
          .filter((call) => call.command === "subscribe_live")
          .at(-1)?.args[0]
      ).toBe("remote-studio")
    );
    unmount();
    transport.dispose();
  });

  it("checks endpoints before connecting and saves a new selectable host", async () => {
    const { transport, unmount } = setup();
    fireEvent.click(await screen.findByRole("button", { name: "Add Mac" }));
    expect(
      screen.getByRole("button", { name: "Check connection" })
    ).toBeDisabled();
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "office.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await screen.findByRole("heading", { name: "office.local" });
    expect(
      transport.calls.some((call) => call.command === "prepare_remote_host")
    ).toBe(true);
    expect(
      transport.calls.some((call) => call.command === "add_remote_host")
    ).toBe(true);
    unmount();
    transport.dispose();
  });
  it("shows a scanned fingerprint and only saves trust after explicit confirmation", async () => {
    const { transport, unmount } = setup();
    let trusted = false;
    const prepare = transport.prepareRemoteHost;
    const accept = transport.acceptRemoteSshHost;
    transport.prepareRemoteHost = async (lan, tailscale) =>
      trusted
        ? prepare(lan, tailscale)
        : {
            status: "error",
            error: {
              kind: "invalid_argument",
              message: "Host key verification failed",
            },
          };
    transport.acceptRemoteSshHost = async (token) => {
      trusted = true;
      return accept(token);
    };
    fireEvent.click(await screen.findByRole("button", { name: "Add Mac" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "office.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Verify LAN host" })
    );
    await screen.findByText(/SHA256:exampleMockHostFingerprint/);
    expect(trusted).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Trust this host" }));
    await screen.findByRole("button", { name: "Connect" });
    expect(trusted).toBe(true);
    unmount();
    transport.dispose();
  });
  it("cancels one setup attempt and waits for rollback before allowing another action", async () => {
    const { transport, unmount } = setup(60000);
    let completeCleanup = () => {};
    const cleanup = new Promise<void>((resolve) => {
      completeCleanup = resolve;
    });
    const cancel = transport.cancelRemoteSetup;
    let cancelCalls = 0;
    transport.cancelRemoteSetup = async (id) => {
      cancelCalls++;
      await cleanup;
      return cancel(id);
    };
    fireEvent.click(await screen.findByRole("button", { name: "Add Mac" }));
    fireEvent.change(screen.getByLabelText("LAN SSH alias"), {
      target: { value: "office.local" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
    fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
    await screen.findByText("Setting up Kelvo and connecting…");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    const cancelling = await screen.findByRole("button", {
      name: "Cancelling…",
    });
    expect(cancelling).toBeDisabled();
    fireEvent.click(cancelling);
    expect(cancelCalls).toBe(1);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await act(async () => completeCleanup());
    await screen.findByText("Setup cancelled. Remote settings were restored.");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();
    const addCall = transport.calls.find(
      (call) => call.command === "add_remote_host"
    );
    const cancelCall = transport.calls.find(
      (call) => call.command === "cancel_remote_setup"
    );
    expect(addCall?.args[3]).toEqual(expect.any(String));
    expect(cancelCall?.args[0]).toBe(addCall?.args[3]);
    expect(await transport.listRemoteHosts()).toHaveLength(1);
    unmount();
    transport.dispose();
  });
});
