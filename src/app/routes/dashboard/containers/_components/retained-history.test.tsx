import { createMockTransport } from "@core/mock-transport";
import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it, vi } from "vitest";
import ContainersRoute from "../route";

it("browses retained containers from a disabled startup without probing or enabling an engine", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-retained"],
    historyRows: 1,
  });
  const discover = vi.spyOn(transport, "dockerDiscover");
  const configure = vi.spyOn(transport, "dockerConfigure");
  const snapshot = await transport.dockerSnapshot();
  if (snapshot.status !== "ok") throw new Error("Missing fixture");
  expect(snapshot.data.engine).toBeNull();
  expect(snapshot.data.settings.enabled).toBe(false);
  const result = renderWithProviders(<ContainersRoute />, { transport });
  expect(
    await screen.findByRole("combobox", { name: "Retained engine" })
  ).toBeVisible();
  await result.user.type(
    screen.getByPlaceholderText("Find retained containers"),
    "Removed"
  );
  expect(
    await screen.findByRole("link", { name: "Removed worker" })
  ).toHaveAttribute("href", "/dashboard/containers/mock-orbstack/removed");
  expect(
    screen.getByRole("button", { name: "Enable monitoring" })
  ).toBeVisible();
  expect(configure).not.toHaveBeenCalled();
  expect(discover).not.toHaveBeenCalled();
  result.unmount();
  transport.dispose();
});
