import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { OVERVIEW_CARDS } from "./lib/sample-props";
import { ProcessList } from "./process-list";

const body = OVERVIEW_CARDS[0]?.body;
if (body?.kind !== "list") throw new Error("the CPU sample has a list body");
const { kind: _kind, ...props } = body;

describe("ProcessList", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(ProcessList, props);
  });

  it("lists rows in order under its name", () => {
    render(<ProcessList {...props} />);
    const items = screen
      .getByRole("list", { name: "Top processes by CPU" })
      .querySelectorAll("li");
    expect(items).toHaveLength(5);
    expect(items[0]).toHaveTextContent("Xcode6.2%");
  });
});
