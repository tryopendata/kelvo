import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { ModuleCard, type ModuleCardProps } from "./module-card";

const PROPS: ModuleCardProps = {
  accent: "mem",
  title: "Memory",
  value: "17.6",
  unit: " / 24 GB",
};

describe("ModuleCard", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(ModuleCard, PROPS);
    expectJsonRoundTrip(ModuleCard, {
      accent: "cpu",
      title: "Cores",
      subtitle: "% load",
      subtitleStyle: "label",
    });
  });

  it("is a region named by its title", () => {
    render(<ModuleCard {...PROPS} />);
    expect(screen.getByRole("region", { name: "Memory" })).toBeInTheDocument();
  });
});
