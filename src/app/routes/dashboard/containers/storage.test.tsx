import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it, vi } from "vitest";
import ContainersRoute from "./route";

it("does not scan storage on Activity or live ticks; storage entry fetches once and leaves nothing selected", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const scan = vi.spyOn(transport, "dockerStorageScan");
  const result = renderWithProviders(<ContainersRoute />, { transport });
  await screen.findByRole("table", { name: "Containers now" });
  await act(async () => transport.tick());
  expect(scan).not.toHaveBeenCalled();
  await result.user.click(
    screen.getByRole("radio", { name: "Storage & cleanup" })
  );
  await screen.findByRole("table", { name: "Storage resources" });
  expect(scan).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Review cleanup" })).toBeDisabled();
  await act(async () => transport.tick());
  expect(scan).toHaveBeenCalledTimes(1);
  result.unmount();
  transport.dispose();
});

it("requires a separate volume data-loss acknowledgement and supports cancelling without executing", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const execute = vi.spyOn(transport, "dockerStorageExecute");
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  await result.user.click(
    await screen.findByRole("checkbox", { name: "Select old-database-data" })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Review cleanup" })
  );
  await screen.findByRole("dialog", { name: "Review cleanup" });
  await result.user.click(
    screen.getByRole("button", { name: "Review volume deletion" })
  );
  expect(screen.getByText(/Docker volume names can be reused/)).toBeVisible();
  expect(
    screen.getByRole("button", { name: "Delete selected resources" })
  ).toBeDisabled();
  await result.user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(execute).not.toHaveBeenCalled();
  await result.user.click(
    screen.getByRole("button", { name: "Review cleanup" })
  );
  await result.user.click(
    await screen.findByRole("button", { name: "Review volume deletion" })
  );
  await result.user.click(
    screen.getByRole("checkbox", { name: /I understand the permanent/ })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Delete selected resources" })
  );
  await waitFor(() => expect(execute).toHaveBeenCalledTimes(1));
  expect(execute.mock.calls[0]?.[1]).toBe(true);
  result.unmount();
  transport.dispose();
});

it("disables AutoRemove Stop while ordinary Stop displays the grace period", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const result = renderWithProviders(<ContainersRoute />, { transport });
  expect(
    await screen.findByRole("button", { name: "Stop web" })
  ).toHaveAttribute("aria-disabled", "true");
  await result.user.click(screen.getByRole("button", { name: "Stop worker" }));
  expect(
    await screen.findByText(
      "Docker sends SIGTERM and waits 30 seconds, then kills the container."
    )
  ).toBeVisible();
  expect(
    screen.getByRole("alertdialog", { name: "Stop worker?" })
  ).toBeVisible();
  result.unmount();
  transport.dispose();
});

it("shows failed refresh with retained data and disables cleanup until a good scan", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const scan = transport.dockerStorageScan;
  transport.dockerStorageScan = async (force) => {
    const response = await scan(force);
    return force && response.status === "ok"
      ? {
          status: "ok",
          data: { ...response.data, error: "Storage scan timed out" },
        }
      : response;
  };
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  await result.user.click(
    await screen.findByRole("checkbox", { name: "Select old-database-data" })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Refresh storage" })
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Storage scan timed out"
  );
  expect(screen.getByRole("button", { name: "Review cleanup" })).toBeDisabled();
  expect(
    screen.getByRole("checkbox", { name: "Select old-database-data" })
  ).toBeDisabled();
  result.unmount();
  transport.dispose();
});

it("keeps incomplete reference inventories non-actionable", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const scan = transport.dockerStorageScan;
  transport.dockerStorageScan = async (force) => {
    const response = await scan(force);
    if (response.status === "ok" && response.data.snapshot)
      response.data.snapshot.complete = false;
    return response;
  };
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  expect(
    await screen.findByRole("checkbox", { name: "Select old-database-data" })
  ).toBeDisabled();
  expect(screen.getByText(/Incomplete inventory/)).toBeVisible();
  result.unmount();
  transport.dispose();
});

it("recovers retained operation results after reopening with a fresh UI cache", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const review = await transport.dockerStoragePrepare([
    { kind: "image", id: "sha256:old-image", action: "delete" },
  ]);
  if (review.status !== "ok") throw new Error("Missing review fixture");
  await transport.dockerStorageExecute(review.data.token, false);
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  expect(
    await screen.findByRole("heading", { name: "Operation complete" })
  ).toBeVisible();
  expect(
    screen.getByRole("region", { name: "Docker operation results" })
  ).toHaveTextContent("Succeeded");
  expect(
    screen.queryByRole("checkbox", { name: "Select old-api:latest" })
  ).toBeNull();
  result.unmount();
  transport.dispose();
});

it("discards a prepared review from a changed engine generation", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const prepare = transport.dockerStoragePrepare;
  transport.dockerStoragePrepare = async (selections) => {
    const result = await prepare(selections);
    return result.status === "ok"
      ? {
          status: "ok",
          data: { ...result.data, generation: result.data.generation + 1 },
        }
      : result;
  };
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  await result.user.click(
    await screen.findByRole("checkbox", { name: "Select old-database-data" })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Review cleanup" })
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Engine changed during review"
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  result.unmount();
  transport.dispose();
});

it("keeps unknown mutation outcomes visible without retrying the mutation", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const execute = vi
    .spyOn(transport, "dockerStorageExecute")
    .mockRejectedValue(new Error("Connection lost"));
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  await result.user.click(
    await screen.findByRole("checkbox", { name: "Select old-api:latest" })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Review cleanup" })
  );
  await result.user.click(
    await screen.findByRole("button", { name: "Delete selected resources" })
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Execution could not be confirmed"
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  await act(async () => transport.tick());
  expect(execute).toHaveBeenCalledTimes(1);
  result.unmount();
  transport.dispose();
});

it("keeps large reference lists compact and exposes every name on expansion", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  const summary = await screen.findByText("32 container references");
  expect(
    screen.getByRole("list", { name: "Container references", hidden: true })
  ).not.toBeVisible();
  await result.user.click(summary);
  expect(
    screen.getByRole("list", { name: "Container references" })
  ).toBeVisible();
  expect(
    within(
      screen.getByRole("list", { name: "Container references" })
    ).getByText("service-31")
  ).toBeVisible();
  result.unmount();
  transport.dispose();
});

it("keeps a mixed cleanup batch visible through the final volume acknowledgement", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const execute = vi.spyOn(transport, "dockerStorageExecute");
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  await result.user.click(
    await screen.findByRole("checkbox", { name: "Select old-database-data" })
  );
  await result.user.click(
    screen.getByRole("checkbox", { name: "Select old-api:latest" })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Review cleanup" })
  );
  await result.user.click(
    await screen.findByRole("button", { name: "Review volume deletion" })
  );
  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getByText("old-api:latest")).toBeVisible();
  expect(
    within(dialog).getByText("old-database-data", { exact: true })
  ).toBeVisible();
  expect(
    within(dialog).getByText(/other selected resources will also be removed/)
  ).toBeVisible();
  expect(
    within(dialog).getByRole("button", { name: "Delete selected resources" })
  ).toBeDisabled();
  await result.user.click(
    within(dialog).getByRole("checkbox", {
      name: /risk for the listed volumes/,
    })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Delete selected resources" })
  );
  expect(execute).toHaveBeenCalledTimes(1);
  const scan = await transport.dockerStorageScan(true);
  if (scan.status !== "ok" || !scan.data.latest_operation_id)
    throw new Error("Missing operation fixture");
  const operation = await transport.dockerStorageOperation(
    scan.data.latest_operation_id
  );
  if (operation.status !== "ok") throw new Error("Missing operation fixture");
  expect(
    operation.data.results.map((item) => item.selection.id).sort()
  ).toEqual(["old-database-data", "sha256:old-image"]);
  result.unmount();
  transport.dispose();
});

it("recovers a newer daemon operation after a lost execute response despite an older cached result", async () => {
  let now = Date.now();
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
    now: () => now,
  });
  const review = await transport.dockerStoragePrepare([
    { kind: "image", id: "sha256:old-image", action: "delete" },
  ]);
  if (review.status !== "ok") throw new Error("Missing review fixture");
  await transport.dockerStorageExecute(review.data.token, false);
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  await screen.findByRole("heading", { name: "Operation complete" });
  expect(
    screen.getByRole("region", { name: "Docker operation results" })
  ).toHaveTextContent("sha256:old-image");
  now += 1000;
  const execute = transport.dockerStorageExecute;
  transport.dockerStorageExecute = async (token, volumes) => {
    await execute(token, volumes);
    throw new Error("Response lost after dispatch");
  };
  await result.user.click(
    await screen.findByRole("checkbox", { name: "Select old-database-data" })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Review cleanup" })
  );
  await result.user.click(
    await screen.findByRole("button", { name: "Review volume deletion" })
  );
  await result.user.click(
    screen.getByRole("checkbox", { name: /I understand the permanent/ })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Delete selected resources" })
  );
  await screen.findByText(/Execution could not be confirmed/);
  await result.user.click(
    screen.getByRole("button", { name: "Refresh storage" })
  );
  await waitFor(() =>
    expect(
      screen.getByRole("region", { name: "Docker operation results" })
    ).toHaveTextContent("old-database-data")
  );
  expect(
    screen.getByRole("region", { name: "Docker operation results" })
  ).not.toHaveTextContent("sha256:old-image");
  result.unmount();
  transport.dispose();
});

it("keeps maximum-length volume batches out of the final acknowledgement label", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const prepare = transport.dockerStoragePrepare;
  transport.dockerStoragePrepare = async (selections) => {
    const response = await prepare(selections);
    if (response.status !== "ok") return response;
    const volume = response.data.resources[0];
    if (!volume) throw new Error("Missing volume fixture");
    const resources = Array.from({ length: 128 }, (_, index) => ({
      ...volume,
      id: `volume-${index}`,
      name: `${"database-data-".repeat(15)}${index}`,
    }));
    return {
      status: "ok",
      data: {
        ...response.data,
        resources,
        selections: resources.map((row) => ({
          kind: row.kind,
          id: row.id,
          action: "delete",
        })),
      },
    };
  };
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?view=storage",
  });
  await result.user.click(
    await screen.findByRole("checkbox", { name: "Select old-database-data" })
  );
  await result.user.click(
    screen.getByRole("button", { name: "Review cleanup" })
  );
  await result.user.click(
    await screen.findByRole("button", { name: "Review volume deletion" })
  );
  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getAllByRole("listitem")).toHaveLength(128);
  const acknowledgement = within(dialog).getByRole("checkbox", {
    name: "I understand the permanent data loss and volume name reuse risk for the listed volumes.",
  });
  expect(acknowledgement).toBeVisible();
  expect(within(dialog).getByRole("list")).toHaveClass(
    "max-h-48",
    "overflow-y-auto",
    "break-words"
  );
  expect(
    within(dialog).getByRole("button", { name: "Delete selected resources" })
  ).toBeDisabled();
  result.unmount();
  transport.dispose();
});
