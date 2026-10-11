import type { DockerLogBatch } from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { describe, expect, it, vi } from "vitest";
import { ContainerLogs } from "./container-logs";

describe("container log lifecycle", () => {
  it("closes a late subscription after unmount and rejects previous generations", async () => {
    const transport = createMockTransport({
      scenarios: ["docker-32"],
      historyRows: 1,
    });
    const stop = vi.fn();
    let deliver: ((batch: DockerLogBatch) => void) | undefined;
    let resolve:
      | ((value: Awaited<ReturnType<typeof transport.dockerLogs>>) => void)
      | undefined;
    transport.dockerLogs = (_request, callback) => {
      deliver = callback;
      return new Promise((done) => {
        resolve = done;
      });
    };
    const result = renderWithProviders(
      <ContainerLogs
        engine="mock-orbstack"
        container="container-0"
        generation={2}
      />,
      { transport }
    );
    await waitFor(() => expect(deliver).toBeDefined());
    act(() =>
      deliver?.({
        generation: 1,
        lines: ["old private output"],
        truncated: false,
        complete: false,
        error: null,
      })
    );
    expect(screen.queryByText("old private output")).toBeNull();
    act(() =>
      deliver?.({
        generation: 2,
        lines: ["<b>literal</b>"],
        truncated: false,
        complete: false,
        error: null,
      })
    );
    expect(screen.getByText("<b>literal</b>").tagName).toBe("P");
    result.unmount();
    await act(async () =>
      resolve?.({ info: { status: "ok", data: 3 }, unsubscribe: stop })
    );
    expect(stop).toHaveBeenCalledOnce();
    transport.dispose();
  });
});

it("preserves actionable rejected-admission messages", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  transport.dockerLogs = async () => ({
    info: {
      status: "error",
      error: {
        kind: "docker",
        message: "Two log streams are open; close one to continue.",
      },
    },
    unsubscribe: () => {},
  });
  const result = renderWithProviders(
    <ContainerLogs
      engine="mock-orbstack"
      container="container-0"
      generation={1}
    />,
    { transport }
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Two log streams are open; close one to continue."
  );
  expect(screen.getByRole("button", { name: "Retry logs" })).toBeVisible();
  result.unmount();
  transport.dispose();
});

it("marks a hidden-window Follow cancellation stopped and resumes explicitly", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  let deliver: ((batch: DockerLogBatch) => void) | undefined;
  const requests: boolean[] = [];
  transport.dockerLogs = async (request, callback) => {
    requests.push(request.follow);
    deliver = callback;
    callback({
      generation: 1,
      lines: ["line"],
      truncated: false,
      complete: !request.follow,
      error: null,
    });
    return {
      info: { status: "ok", data: requests.length },
      unsubscribe: () => {},
    };
  };
  const result = renderWithProviders(
    <ContainerLogs
      engine="mock-orbstack"
      container="container-0"
      generation={1}
    />,
    { transport }
  );
  await screen.findByText("line");
  await result.user.click(screen.getByRole("checkbox", { name: "Follow" }));
  await screen.findByText(/Following/);
  act(() =>
    deliver?.({
      generation: 1,
      lines: [],
      truncated: false,
      complete: true,
      error:
        "Logs paused because this window was hidden; reopen logs to continue",
    })
  );
  expect(screen.queryByText(/Following/)).toBeNull();
  expect(screen.getByText(/Latest 200 lines/)).toHaveTextContent("Stopped");
  await result.user.click(screen.getByRole("button", { name: "Resume logs" }));
  await screen.findByText(/Following/);
  expect(requests).toEqual([false, true, true]);
  result.unmount();
  transport.dispose();
});
