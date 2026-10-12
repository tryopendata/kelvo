import { createMockTransport } from "@core/mock-transport";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { expect, it } from "vitest";
import { useDockerHistory } from "./use-docker";

it("keeps history defined while the next range is pending", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-1"],
    historyRows: 1,
  });
  const history = transport.dockerHistory;
  let resolve:
    | ((value: Awaited<ReturnType<typeof history>>) => void)
    | undefined;
  transport.dockerHistory = (request) =>
    request.to_ms === 20_000
      ? new Promise((done) => {
          resolve = done;
        })
      : history(request);
  let change: (() => void) | undefined;
  function Harness() {
    const [to, setTo] = useState(10_000);
    change = () => setTo(20_000);
    const query = useDockerHistory("mock-orbstack", null, 0, to);
    return (
      <p>
        {query.data
          ? `${query.data.points.length} points`
          : "Undefined history"}
      </p>
    );
  }
  const result = renderWithProviders(<Harness />, { transport });
  expect(await screen.findByText("180 points")).toBeVisible();
  act(() => change?.());
  expect(screen.queryByText("Undefined history")).toBeNull();
  expect(screen.getByText("180 points")).toBeVisible();
  const page = await history({
    engine_id: "mock-orbstack",
    container_id: null,
    from_ms: 0,
    to_ms: 20_000,
    max_points: 360,
  });
  if (page.status !== "ok") throw new Error("History fixture failed");
  await act(async () =>
    resolve?.({
      status: "ok",
      data: { ...page.data, points: page.data.points.slice(0, 2) },
    })
  );
  expect(await screen.findByText("2 points")).toBeVisible();
  result.unmount();
  transport.dispose();
});
