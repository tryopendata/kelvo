import type { HostRecord } from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { expect, it, vi } from "vitest";
import { HostSelectionContext } from "~/lib/host-selection";
import { HostStoreProvider } from "~/stores/host-store";
import ContainerDetailRoute from "../detail-route";
import ContainersRoute from "../route";
import { DockerTimelineLane } from "./docker-surfaces";

it.each([ContainersRoute, ContainerDetailRoute])(
  "unmounts local Docker surfaces and stops their reads when switching to a remote host (%s)",
  async (Route) => {
    const transport = createMockTransport({
      scenarios: ["multi-host", "docker-32"],
      autoTick: false,
      historyRows: 1,
    });
    const hosts = await transport.listHosts();
    const local = hosts.find((host) => host.is_local) as HostRecord;
    const remote = hosts.find((host) => !host.is_local) as HostRecord;
    const history = vi.spyOn(transport, "dockerHistory");
    const events = vi.spyOn(transport, "dockerEvents");
    const detail = vi.spyOn(transport, "dockerDetail");
    const subscribe = vi.spyOn(transport, "dockerSubscribe");
    function Harness() {
      const [selected, select] = useState(local.id);
      return (
        <HostSelectionContext value={{ hosts, selected, select }}>
          <HostStoreProvider hostId={selected}>
            <button type="button" onClick={() => select(remote.id)}>
              Select remote host
            </button>
            <Route />
            <DockerTimelineLane from={0} to={1000} />
          </HostStoreProvider>
        </HostSelectionContext>
      );
    }
    const result = renderWithProviders(<Harness />, { transport });
    await screen.findByRole("region", { name: "Docker timeline" });
    await waitFor(() => expect(history).toHaveBeenCalled());
    await result.user.click(
      screen.getByRole("button", { name: "Select remote host" })
    );
    await screen.findByText(/Container monitoring is available on This Mac/);
    expect(
      screen.queryByRole("region", { name: "Docker timeline" })
    ).toBeNull();
    history.mockClear();
    events.mockClear();
    detail.mockClear();
    subscribe.mockClear();
    transport.tick();
    expect(history).not.toHaveBeenCalled();
    expect(events).not.toHaveBeenCalled();
    expect(detail).not.toHaveBeenCalled();
    expect(subscribe).not.toHaveBeenCalled();
    await result.user.click(
      screen.getByRole("button", { name: "Show containers on This Mac" })
    );
    await screen.findByRole("region", { name: "Docker timeline" });
    expect(subscribe).toHaveBeenCalled();
    result.unmount();
    transport.dispose();
  }
);
