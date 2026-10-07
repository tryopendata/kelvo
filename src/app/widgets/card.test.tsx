import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { Card, type CardProps } from "./card";

describe("Card", () => {
  it("round-trips its data props through JSON", () => {
    const props: CardProps = {
      accent: "cpu",
      origin: "br",
      variant: "chart",
      labelledBy: "t",
    };
    expectJsonRoundTrip(Card, props);
  });

  it("puts the accent and glow origin in scope", () => {
    render(
      <Card accent="gpu" origin="tr" labelledBy="t">
        <h3 id="t">GPU</h3>
      </Card>
    );
    const card = screen.getByRole("region", { name: "GPU" });
    expect(card.style.getPropertyValue("--a")).toBe("var(--color-gpu)");
    expect(card.style.getPropertyValue("--o")).toBe("100% 0%");
  });
});
