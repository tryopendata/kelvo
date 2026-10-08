import {
  createMockTransport,
  MOCK_LOCAL_IPV4,
  MOCK_PUBLIC_IP,
} from "@core/mock-transport";
import { screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { AddressLine } from "./address-line";

const transport = () => createMockTransport({ autoTick: false });
const publicCalls = (t: ReturnType<typeof transport>) =>
  t.calls.filter((c) => c.command === "get_public_ip").length;

describe("AddressLine (D-093)", () => {
  it("shows the local and public addresses, each copyable", async () => {
    renderWithProviders(<AddressLine primary="en0" />, {
      transport: transport(),
    });
    expect(
      await screen.findByRole("button", {
        name: `Copy local IP address ${MOCK_LOCAL_IPV4}`,
      })
    ).toBeVisible();
    expect(
      await screen.findByRole("button", {
        name: `Copy public IP address ${MOCK_PUBLIC_IP}`,
      })
    ).toBeVisible();
  });

  it("asks nothing outside without a network", async () => {
    const t = transport();
    const { container } = renderWithProviders(<AddressLine primary={null} />, {
      transport: t,
    });
    await new Promise((r) => setTimeout(r, 20));
    expect(container).toBeEmptyDOMElement();
    expect(publicCalls(t)).toBe(0);
  });

  it("asks nothing outside for a host with no local addresses here", async () => {
    const t = transport();
    t.getNetworkAddresses = async (host) => ({
      status: "error",
      error: { kind: "remote_host", host },
    });
    renderWithProviders(<AddressLine primary="en0" />, { transport: t });
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByText(/Public/)).toBeNull();
    expect(publicCalls(t)).toBe(0);
  });

  it("asks again when a VPN takes the internet route", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const t = transport();
      let egress = "en0";
      const local = t.getNetworkAddresses;
      t.getNetworkAddresses = async (host) => {
        const r = await local(host);
        return r.status === "ok" ? { ...r, data: { ...r.data, egress } } : r;
      };
      let publicIp = MOCK_PUBLIC_IP;
      t.getPublicIp = async () => ({ status: "ok", data: publicIp });
      renderWithProviders(<AddressLine primary="en0" />, { transport: t });
      expect(
        await screen.findByRole("button", {
          name: `Copy public IP address ${MOCK_PUBLIC_IP}`,
        })
      ).toBeVisible();

      // Same interface, same LAN address; only the route moves.
      egress = "utun4";
      publicIp = "198.51.100.9";
      await vi.advanceTimersByTimeAsync(10_000);
      expect(
        await screen.findByRole("button", {
          name: "Copy public IP address 198.51.100.9",
        })
      ).toBeVisible();
    } finally {
      vi.useRealTimers();
    }
  });

  it("reads unavailable when the lookup fails, and does not retry", async () => {
    const t = transport();
    let asked = 0;
    t.getPublicIp = async () => {
      asked += 1;
      return {
        status: "error",
        error: { kind: "public_ip", message: "offline" },
      };
    };
    renderWithProviders(<AddressLine primary="en0" />, { transport: t });
    expect(await screen.findByText("unavailable")).toBeVisible();
    await waitFor(() => expect(asked).toBe(1));
  });
});
