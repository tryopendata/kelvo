import type { DockerStorageOperation } from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { dockerKeys } from "@core/query-keys";
import type { CommandResult } from "@core/transport";
import { act, screen, waitFor } from "@testing-library/react";
import { createTestQueryClient, renderWithProviders } from "@tests/test-utils";
import { expect, it } from "vitest";
import { useDockerCleanup } from "./use-docker-cleanup";

const engine = "mock-orbstack";
const operation = (id: string, started: number): DockerStorageOperation => ({
  id,
  engine_id: engine,
  generation: 1,
  started_ms: started,
  finished_ms: started + 1,
  results: [],
  total: 0,
});
function Harness() {
  const flow = useDockerCleanup(engine, 1, false);
  return <p>{flow.operation?.id ?? "No operation"}</p>;
}
function deferred<T>() {
  let resolve: (value: T) => void = () => {
    throw new Error("Resolver not initialized");
  };
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

it("does not let a late poll overwrite a newer locally accepted operation", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const client = createTestQueryClient();
  client.setQueryData(dockerKeys.operation(engine), operation("older", 1));
  const pending = deferred<CommandResult<DockerStorageOperation>>();
  let requested = false;
  transport.dockerStorageOperation = async () => {
    requested = true;
    return pending.promise;
  };
  const result = renderWithProviders(<Harness />, {
    transport,
    queryClient: client,
  });
  await waitFor(() => expect(requested).toBe(true));
  await act(async () => {
    client.setQueryData(
      dockerKeys.operation(engine),
      operation("new-local", 3)
    );
    pending.resolve({ status: "ok", data: operation("older", 1) });
  });
  expect(await screen.findByText("new-local")).toBeVisible();
  expect(
    client.getQueryData<DockerStorageOperation>(dockerKeys.operation(engine))
      ?.id
  ).toBe("new-local");
  result.unmount();
  transport.dispose();
});

it("does not let a late recovery overwrite a newer local operation", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const client = createTestQueryClient();
  client.setQueryData(dockerKeys.operation(engine), operation("older", 1));
  client.setQueryData(dockerKeys.storage(engine, 1), {
    snapshot: null,
    error: null,
    latest_operation_id: "discovered",
  });
  const pending = deferred<CommandResult<DockerStorageOperation>>();
  let requested = false;
  transport.dockerStorageOperation = async (id) => {
    if (id === "discovered") {
      requested = true;
      return pending.promise;
    }
    return { status: "ok", data: operation(id, id === "older" ? 1 : 3) };
  };
  const result = renderWithProviders(<Harness />, {
    transport,
    queryClient: client,
  });
  await waitFor(() => expect(requested).toBe(true));
  await act(async () => {
    client.setQueryData(
      dockerKeys.operation(engine),
      operation("new-local", 3)
    );
    pending.resolve({ status: "ok", data: operation("discovered", 2) });
  });
  expect(await screen.findByText("new-local")).toBeVisible();
  expect(
    client.getQueryData<DockerStorageOperation>(dockerKeys.operation(engine))
      ?.id
  ).toBe("new-local");
  result.unmount();
  transport.dispose();
});

it.each([1, 3])(
  "rejects stale scan metadata without proven newer ordering (%s)",
  async (olderTime) => {
    const transport = createMockTransport({
      scenarios: ["docker-32"],
      historyRows: 1,
    });
    const client = createTestQueryClient();
    client.setQueryData(dockerKeys.operation(engine), operation("current", 3));
    client.setQueryData(dockerKeys.storage(engine, 1), {
      snapshot: null,
      error: null,
      latest_operation_id: "stale",
    });
    let recovered = false;
    transport.dockerStorageOperation = async (id) => {
      if (id === "stale") recovered = true;
      return {
        status: "ok",
        data: operation(id, id === "stale" ? olderTime : 3),
      };
    };
    const result = renderWithProviders(<Harness />, {
      transport,
      queryClient: client,
    });
    await waitFor(() => expect(recovered).toBe(true));
    expect(await screen.findByText("current")).toBeVisible();
    result.unmount();
    transport.dispose();
  }
);
