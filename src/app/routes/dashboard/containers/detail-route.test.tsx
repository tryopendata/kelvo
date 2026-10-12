import { mockContainer } from "@core/mock/docker";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Route, Routes } from "react-router";
import { expect, it } from "vitest";
import { useResetHistory } from "~/hooks/use-reset-history";
import ContainerDetailRoute from "./detail-route";

function ResetButton() {
  const { reset } = useResetHistory();
  return (
    <button type="button" onClick={() => void reset()}>
      Reset history
    </button>
  );
}
const detailView = (
  <Routes>
    <Route
      path="/dashboard/containers/:engineId/:containerId"
      element={
        <>
          <ContainerDetailRoute />
          <ResetButton />
        </>
      }
    />
  </Routes>
);
const route = "/dashboard/containers/mock-orbstack/container-0";

it.each([false, true])(
  "uses successful inspection metadata while retaining current inventory metrics, and reports refresh failure (inventory inspected: %s)",
  async (metadataAvailable) => {
    const transport = createMockTransport({
      scenarios: ["docker-32"],
      historyRows: 1,
    });
    const inventory = {
      ...mockContainer(0, 1000),
      metadata_available: metadataAvailable,
      monitored: false,
      explicit_limit_bytes: null,
      ports: [],
      mounts: [],
      restart_count: null,
    };
    inventory.metrics = { ...inventory.metrics, cpu_pct: 3 };
    let metricsTick = false;
    let inspected = false;
    let publish: (() => void) | undefined;
    const subscribe = transport.dockerSubscribe;
    transport.dockerSubscribe = (cb) =>
      subscribe((snapshot) => {
        publish = () =>
          cb({
            ...snapshot,
            observed_ms: metricsTick
              ? Date.now() + 10_000
              : snapshot.observed_ms,
            containers: [
              {
                ...inventory,
                ...(inspected
                  ? {
                      metadata_available: true,
                      explicit_limit_bytes: 4_000_000,
                      ports: ["8080 → 80/tcp"],
                    }
                  : {}),
                observed_ms: metricsTick
                  ? Date.now() + 10_000
                  : inventory.observed_ms,
                metrics: { ...inventory.metrics, cpu_pct: metricsTick ? 7 : 3 },
              },
            ],
          });
        publish();
      });
    let failed = false;
    transport.dockerDetail = async () => {
      if (failed)
        return {
          status: "error",
          error: { kind: "docker", message: "Inspection socket disconnected" },
        };
      inspected = true;
      publish?.();
      return {
        status: "ok",
        data: {
          ...inventory,
          metadata_available: true,
          explicit_limit_bytes: 4_000_000,
          ports: ["8080 → 80/tcp"],
        },
      };
    };
    const result = renderWithProviders(detailView, { transport, route });
    await screen.findByText(/Charged memory/, { selector: "span" });
    await result.user.click(
      screen.getByText("Container metadata", { exact: true })
    );
    expect(await screen.findByText("8080 → 80/tcp")).toBeVisible();
    expect(screen.getByText(/Now · CPU/)).toHaveTextContent("3%");
    expect(
      screen.getByText(/Charged memory/, { selector: "span" })
    ).toHaveTextContent("4.0 MB");
    metricsTick = true;
    await act(async () => transport.tick());
    expect(screen.getByText(/Now · CPU/)).toHaveTextContent("7%");
    expect(
      screen.getByText(/Charged memory/, { selector: "span" })
    ).toHaveTextContent("4.0 MB");
    expect(screen.getByText("8080 → 80/tcp")).toBeVisible();
    failed = true;
    await result.user.click(
      screen.getByRole("button", { name: "Refresh metadata" })
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Inspection socket disconnected"
    );
    expect(
      screen.getByText(/Charged memory/, { selector: "span" })
    ).toHaveTextContent("4.0 MB");
    result.unmount();
    transport.dispose();
  }
);

it("labels a retained running container from a previous engine as last observed", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const subscribe = transport.dockerSubscribe;
  transport.dockerSubscribe = (cb) =>
    subscribe((snapshot) =>
      cb({
        ...snapshot,
        engine: snapshot.engine && { ...snapshot.engine, id: "another-engine" },
        containers: [],
      })
    );
  transport.dockerDetail = async () => ({
    status: "ok",
    data: mockContainer(0, 1000),
  });
  const result = renderWithProviders(detailView, { transport, route });
  expect(await screen.findByText(/Last observed · CPU/)).toBeVisible();
  expect(screen.getByText(/Retained engine · not selected/)).toHaveTextContent(
    "Retained observation"
  );
  expect(screen.queryByText(/Now · CPU/)).toBeNull();
  result.unmount();
  transport.dispose();
});

it("invalidates retained detail with history reset and removes erased metadata", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  let erased = false;
  let reads = 0;
  transport.dockerDetail = async () => {
    reads++;
    return erased
      ? {
          status: "error",
          error: {
            kind: "docker",
            message: "Container metadata no longer retained",
          },
        }
      : {
          status: "ok",
          data: {
            ...mockContainer(0, 1000),
            id: "removed",
            lifecycle: "removed",
            name: "Retained worker",
          },
        };
  };
  transport.resetHistory = async () => {
    erased = true;
    return { status: "ok", data: 0 };
  };
  const result = renderWithProviders(detailView, {
    transport,
    route: route.replace("container-0", "removed"),
  });
  await screen.findByRole("heading", { name: "Retained worker" });
  await result.user.click(
    screen.getByRole("button", { name: "Reset history" })
  );
  await waitFor(() => expect(reads).toBeGreaterThan(1));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Container metadata no longer retained"
  );
  expect(screen.queryByRole("heading", { name: "Retained worker" })).toBeNull();
  result.unmount();
  transport.dispose();
});

it("drops retained detail when the Docker generation changes", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  let erased = false;
  transport.dockerDetail = async () =>
    erased
      ? {
          status: "error",
          error: {
            kind: "docker",
            message: "Metadata belongs to the previous generation",
          },
        }
      : {
          status: "ok",
          data: {
            ...mockContainer(0, 1000),
            id: "removed",
            lifecycle: "removed",
            name: "Old generation worker",
          },
        };
  const result = renderWithProviders(detailView, {
    transport,
    route: route.replace("container-0", "removed"),
  });
  await screen.findByRole("heading", { name: "Old generation worker" });
  const snapshot = await transport.dockerSnapshot();
  if (snapshot.status !== "ok") throw new Error("Missing fixture");
  erased = true;
  await act(async () => {
    await transport.dockerConfigure({
      ...snapshot.data.settings,
      enabled: false,
    });
  });
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Metadata belongs to the previous generation"
  );
  expect(
    screen.queryByRole("heading", { name: "Old generation worker" })
  ).toBeNull();
  result.unmount();
  transport.dispose();
});

it("renders Rust-reconciled removal evidence from the stream", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const inventory = mockContainer(0, 1000);
  const subscribe = transport.dockerSubscribe;
  let publish: (() => void) | undefined;
  const removed = {
    ...inventory,
    lifecycle: "removed",
    metadata_available: false,
    metadata_observed_ms: 2000,
    observed_ms: null,
    interval_ms: null,
    metrics: {
      cpu_pct: null,
      working_set_bytes: null,
      raw_memory_bytes: null,
      effective_limit_bytes: null,
      network_rx_bps: null,
      network_tx_bps: null,
      block_read_bps: null,
      block_write_bps: null,
    },
  } as const;
  let inspected = false;
  transport.dockerSubscribe = (cb) =>
    subscribe((snapshot) => {
      publish = () =>
        cb({ ...snapshot, containers: [inspected ? removed : inventory] });
      publish();
    });
  transport.dockerDetail = async () => {
    inspected = true;
    publish?.();
    return { status: "ok", data: removed };
  };
  const result = renderWithProviders(detailView, { transport, route });
  expect(
    await screen.findByText(/Removed container · retained history/)
  ).toBeVisible();
  expect(screen.getByText(/Last observed · CPU/)).toHaveTextContent("CPU —");
  expect(screen.queryByText(/Now · CPU/)).toBeNull();
  result.unmount();
  transport.dispose();
});
