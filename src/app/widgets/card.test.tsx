import { fireEvent, render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { Card, type CardProps, LinkCard } from "./card";

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

  it("takes its name from ariaLabel when nothing in it names it", () => {
    render(<Card accent="cpu" ariaLabel="This Mac" />);
    expect(screen.getByRole("region", { name: "This Mac" })).toHaveClass(
      "vt-card"
    );
  });

  it("is the link itself when given an href, calling onOpen instead", () => {
    const onOpen = vi.fn();
    render(
      <LinkCard
        accent="mem"
        labelledBy="m"
        href="/dashboard/memory"
        onOpen={onOpen}
      >
        <h3 id="m">Memory</h3>
      </LinkCard>
    );
    const link = screen.getByRole("link", { name: "Memory" });
    expect(link).toHaveClass("vt-card");
    expect(link.style.getPropertyValue("--a")).toBe("var(--color-mem)");
    fireEvent.click(link);
    expect(onOpen).toHaveBeenCalledWith("/dashboard/memory");
  });
});
