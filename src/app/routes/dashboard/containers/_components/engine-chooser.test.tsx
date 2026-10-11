import { createMockTransport } from "@core/mock-transport";
import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it } from "vitest";
import { EngineChooser } from "./engine-chooser";

it("shows the reason a selected engine could not be configured", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const snapshot = await transport.dockerSnapshot();
  if (snapshot.status !== "ok") throw new Error("Missing fixture");
  transport.dockerConfigure = async () => ({
    status: "error",
    error: {
      kind: "docker",
      message: "The selected Unix socket is no longer accessible.",
    },
  });
  const result = renderWithProviders(
    <EngineChooser
      settings={snapshot.data.settings}
      onChange={() => {}}
      onClose={() => {}}
    />,
    { transport }
  );
  await result.user.click(
    await screen.findByRole("button", { name: "Use selected engine" })
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The selected Unix socket is no longer accessible."
  );
  result.unmount();
  transport.dispose();
});
