import type {
  DockerContainer,
  DockerSnapshot,
  DockerStorageOperation,
} from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import {
  ContainerActionButton,
  ContainerActionProvider,
  useContainerActions,
} from "./container-actions";

afterEach(() => vi.useRealTimers());

async function fixture() {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
    autoTick: false,
  });
  const response = await transport.dockerSnapshot();
  if (response.status !== "ok") throw new Error("Missing Docker fixture");
  const first = response.data.containers[0];
  if (!first) throw new Error("Missing container fixture");
  const row: DockerContainer = {
    ...first,
    lifecycle: "running",
    auto_remove: false,
  };
  const snapshot: DockerSnapshot = { ...response.data, containers: [row] };
  const operation: DockerStorageOperation = {
    id: "container-operation",
    engine_id: row.engine_id,
    generation: snapshot.generation,
    started_ms: 1000,
    finished_ms: null,
    results: [],
    total: 1,
  };
  const dispatch = vi
    .fn<typeof transport.dockerContainerAction>()
    .mockResolvedValue({
      status: "ok",
      data: operation,
    });
  const poll = vi
    .fn<typeof transport.dockerStorageOperation>()
    .mockResolvedValue({
      status: "ok",
      data: operation,
    });
  transport.dockerContainerAction = dispatch;
  transport.dockerStorageOperation = poll;
  return { transport, row, snapshot, operation, dispatch, poll };
}

function Failure({ id }: { id: string }) {
  const { failures } = useContainerActions();
  return failures[id] ? <p role="alert">{failures[id]}</p> : null;
}

it("requires confirmation for Stop and holds the action until its result", async () => {
  const { transport, row, snapshot, dispatch } = await fixture();
  const result = renderWithProviders(
    <ContainerActionProvider snapshot={snapshot}>
      <ContainerActionButton row={row} />
    </ContainerActionProvider>,
    { transport }
  );
  await result.user.click(
    screen.getByRole("button", { name: `Stop ${row.name}` })
  );
  expect(dispatch).not.toHaveBeenCalled();
  expect(screen.getByRole("alertdialog")).toHaveTextContent(
    "Docker sends SIGTERM and waits 30 seconds, then kills the container."
  );
  await result.user.click(
    within(screen.getByRole("alertdialog")).getByRole("button", {
      name: "Cancel",
    })
  );
  expect(dispatch).not.toHaveBeenCalled();
  await result.user.click(
    screen.getByRole("button", { name: `Stop ${row.name}` })
  );
  await result.user.click(
    within(screen.getByRole("alertdialog")).getByRole("button", {
      name: "Stop",
    })
  );
  expect(dispatch).toHaveBeenCalledExactlyOnceWith(
    row.engine_id,
    row.id,
    "stop"
  );
  expect(screen.getByText("Stopping…")).toBeVisible();
  result.unmount();
  transport.dispose();
});

it("starts an exited container directly and prevents a second concurrent action", async () => {
  const { transport, row, snapshot, dispatch } = await fixture();
  const stopped: DockerContainer = { ...row, lifecycle: "exited" };
  const other: DockerContainer = { ...stopped, id: "other-id", name: "other" };
  const result = renderWithProviders(
    <ContainerActionProvider
      snapshot={{ ...snapshot, containers: [stopped, other] }}
    >
      <ContainerActionButton row={stopped} />
      <ContainerActionButton row={other} />
    </ContainerActionProvider>,
    { transport }
  );
  await result.user.click(
    screen.getByRole("button", { name: `Start ${row.name}` })
  );
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(dispatch).toHaveBeenCalledExactlyOnceWith(
    row.engine_id,
    row.id,
    "start"
  );
  expect(screen.getByText("Starting…")).toBeVisible();
  expect(screen.getByRole("button", { name: "Start other" })).toHaveAttribute(
    "aria-disabled",
    "true"
  );
  await result.user.click(screen.getByRole("button", { name: "Start other" }));
  expect(dispatch).toHaveBeenCalledTimes(1);
  result.unmount();
  transport.dispose();
});

it.each([true, null])("blocks Stop with AutoRemove=%s", async (autoRemove) => {
  const { transport, row, snapshot, dispatch } = await fixture();
  const unsafe = { ...row, auto_remove: autoRemove };
  const result = renderWithProviders(
    <ContainerActionProvider snapshot={{ ...snapshot, containers: [unsafe] }}>
      <ContainerActionButton row={unsafe} />
    </ContainerActionProvider>,
    { transport }
  );
  expect(
    screen.getByRole("button", { name: `Stop ${row.name}` })
  ).toHaveAttribute("aria-disabled", "true");
  await result.user.click(
    screen.getByRole("button", { name: `Stop ${row.name}` })
  );
  expect(dispatch).not.toHaveBeenCalled();
  expect(screen.queryByRole("alertdialog")).toBeNull();
  result.unmount();
  transport.dispose();
});

it("rechecks connection when confirming and never dispatches a stale engine action", async () => {
  const { transport, row, snapshot, dispatch } = await fixture();
  let disconnect: (() => void) | undefined;
  function Harness() {
    const [current, setCurrent] = useState(snapshot);
    disconnect = () => setCurrent({ ...snapshot, status: "unavailable" });
    return (
      <ContainerActionProvider snapshot={current}>
        <ContainerActionButton row={row} />
      </ContainerActionProvider>
    );
  }
  const result = renderWithProviders(<Harness />, { transport });
  await result.user.click(
    screen.getByRole("button", { name: `Stop ${row.name}` })
  );
  act(() => disconnect?.());
  await result.user.click(
    within(screen.getByRole("alertdialog")).getByRole("button", {
      name: "Stop",
    })
  );
  expect(dispatch).not.toHaveBeenCalled();
  result.unmount();
  transport.dispose();
});

it("shows a failed poll result for ten seconds and never retries the mutation", async () => {
  vi.useFakeTimers();
  const { transport, row, snapshot, operation, dispatch, poll } =
    await fixture();
  const stopped: DockerContainer = { ...row, lifecycle: "exited" };
  poll.mockResolvedValue({
    status: "ok",
    data: {
      ...operation,
      finished_ms: 1500,
      results: [
        {
          selection: { kind: "container", id: row.id, action: "start" },
          status: "failed",
          message: "Port 8080 is already in use",
          reclaimed_bytes: null,
        },
      ],
    },
  });
  const result = renderWithProviders(
    <ContainerActionProvider snapshot={{ ...snapshot, containers: [stopped] }}>
      <ContainerActionButton row={stopped} />
      <Failure id={row.id} />
    </ContainerActionProvider>,
    { transport }
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: `Start ${row.name}` }));
  });
  expect(screen.getByText("Starting…")).toBeVisible();
  await act(async () => vi.advanceTimersByTimeAsync(250));
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Port 8080 is already in use"
  );
  expect(screen.queryByText("Starting…")).toBeNull();
  await act(async () => vi.advanceTimersByTimeAsync(9999));
  expect(screen.getByRole("alert")).toBeVisible();
  await act(async () => vi.advanceTimersByTimeAsync(1));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(dispatch).toHaveBeenCalledTimes(1);
  expect(poll).toHaveBeenCalledTimes(1);
  result.unmount();
  transport.dispose();
});
