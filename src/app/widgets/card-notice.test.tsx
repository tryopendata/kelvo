import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { CardNotice } from "./card-notice";
import { ModuleCard } from "./module-card";

describe("CardNotice", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(CardNotice, { text: "Sensor read failed" });
    expectJsonRoundTrip(ModuleCard, {
      accent: "cpu",
      title: "CPU",
      value: "–",
      notice: "Sensor read failed · last value 11:02",
    });
  });

  it("puts the state in words inside the card", () => {
    render(
      <ModuleCard
        accent="cpu"
        title="CPU"
        value="–"
        notice="Sensor read failed · last value 11:02"
      />
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Sensor read failed · last value 11:02"
    );
  });
});
