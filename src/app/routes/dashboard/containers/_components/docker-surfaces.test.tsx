import { createMockTransport } from "@core/mock-transport";
import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it, vi } from "vitest";
import { DockerPopoverSummary, DockerTimelineLane } from "./docker-surfaces";

it("shows unavailable state independently of partial coverage and opens the monitored leader by identity", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-200", "docker-unavailable"],
    historyRows: 1,
  });
  const getSettings = transport.getSettings;
  transport.getSettings = async () => {
    const settings = await getSettings();
    if (!settings.settings.docker) throw new Error("Missing Docker fixture");
    return {
      ...settings,
      settings: {
        ...settings.settings,
        docker: { ...settings.settings.docker, show_popover: true },
      },
    };
  };
  const open = vi.spyOn(transport, "openDashboard");
  const result = renderWithProviders(<DockerPopoverSummary />, { transport });
  const glance = await screen.findByRole("region", {
    name: "Container glance",
  });
  expect(glance).toHaveTextContent("Unavailable · Partial coverage");
  expect(glance).toHaveTextContent("Observed");
  await result.user.click(
    screen.getByRole("button", {
      name: /Last observed highest CPU among monitored: worker/,
    })
  );
  expect(open).toHaveBeenCalledWith(
    "/dashboard/containers/mock-orbstack/container-0"
  );
  result.unmount();
  transport.dispose();
});

it("provides memory history and distinguishes failed event queries from absent events", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  transport.dockerEvents = async () => ({
    status: "error",
    error: {
      kind: "docker",
      message: "Events unavailable while storage is paused",
    },
  });
  const result = renderWithProviders(
    <DockerTimelineLane from={1000} to={1000000} />,
    { transport }
  );
  expect(
    await screen.findByRole("img", {
      name: /Container workload · Working-set memory/,
    })
  ).toBeVisible();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Events unavailable while storage is paused"
  );
  result.unmount();
  transport.dispose();
});

it("shows event coverage gaps beside Timeline annotations", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  transport.dockerEvents = async () => ({
    status: "ok",
    data: {
      events: [],
      next_id: null,
      coverage: [
        {
          start_ms: 1000,
          end_ms: 2000,
          container_id: null,
          reason: "runtime_disconnected",
        },
      ],
    },
  });
  const result = renderWithProviders(
    <DockerTimelineLane from={1000} to={1000000} />,
    { transport }
  );
  expect(
    await screen.findByText(
      /Container event history is incomplete in parts of this range/
    )
  ).toBeVisible();
  result.unmount();
  transport.dispose();
});
