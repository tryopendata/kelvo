import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import {
  DOCKER_RANGES,
  type DockerRange,
  HistoryControls,
} from "./history-controls";

it("offers the same direct range selection and past/live navigation as Timeline", async () => {
  function History() {
    const [range, setRange] = useState<DockerRange>("15m");
    const [end, setEnd] = useState<number | null>(null);
    return (
      <>
        <HistoryControls
          range={range}
          end={end}
          now={DOCKER_RANGES["30d"]}
          onRange={setRange}
          onEnd={setEnd}
        />
        <output>{end === null ? "Current history" : `Ends at ${end}`}</output>
      </>
    );
  }
  render(<History />);
  const user = userEvent.setup();
  const ranges = screen.getByRole("radiogroup", {
    name: "Container history range",
  });
  await user.click(within(ranges).getByRole("radio", { name: "24h" }));
  expect(within(ranges).getByRole("radio", { name: "24h" })).toHaveAttribute(
    "aria-checked",
    "true"
  );
  await user.click(
    screen.getByRole("button", { name: "Previous container range" })
  );
  expect(screen.getByRole("status")).toHaveTextContent(
    `Ends at ${DOCKER_RANGES["30d"] - DOCKER_RANGES["24h"]}`
  );
  await user.click(
    screen.getByRole("button", { name: "Next container range" })
  );
  expect(screen.getByRole("status")).toHaveTextContent("Current history");
  await user.click(
    screen.getByRole("button", { name: "Previous container range" })
  );
  await user.click(screen.getByRole("button", { name: "Live" }));
  expect(screen.getByRole("status")).toHaveTextContent("Current history");
  expect(
    screen.queryByRole("button", { name: "Next container range" })
  ).toBeNull();
});
