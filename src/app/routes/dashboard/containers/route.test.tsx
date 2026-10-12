import { createMockTransport } from "@core/mock-transport";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it } from "vitest";
import ContainersRoute from "./route";

it.each([null, "No running containers. Monitoring paused to save power."])(
  "offers Resume for a Rust-shaped idle snapshot with message %s",
  async (message) => {
    const transport = createMockTransport({
      scenarios: ["docker-idle"],
      historyRows: 1,
    });
    const subscribe = transport.dockerSubscribe;
    transport.dockerSubscribe = (cb) =>
      subscribe((snapshot) => cb({ ...snapshot, message }));
    const result = renderWithProviders(<ContainersRoute />, { transport });
    expect(
      await screen.findByRole("button", { name: "Resume monitoring" })
    ).toBeVisible();
    expect(screen.getByText("0 running / 0 total")).toBeVisible();
    result.unmount();
    transport.dispose();
  }
);

it("keeps live announcements stable across ticks and ranges follow publication time", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-1"],
    historyRows: 1,
  });
  const subscribe = transport.dockerSubscribe;
  let now = 12_000_000;
  let observed = 1000;
  transport.dockerSubscribe = (cb) =>
    subscribe((snapshot) =>
      cb({
        ...snapshot,
        observed_ms: observed,
        now_ms: now,
        interval_ms: 0,
      })
    );
  const result = renderWithProviders(<ContainersRoute />, { transport });
  await screen.findByRole("table", { name: "Containers now" });
  await screen.findByText(new Date(now).toLocaleTimeString());
  const announcements = () =>
    screen.getAllByRole("status").map((node) => node.textContent);
  const before = announcements();
  expect(screen.queryByText(/every 0s/)).toBeNull();
  now += 10_000;
  observed += 1000;
  await act(async () => transport.tick());
  expect(announcements()).toEqual(before);
  expect(
    await screen.findByText(new Date(now).toLocaleTimeString())
  ).toBeVisible();
  result.unmount();
  transport.dispose();
});
