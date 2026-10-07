import { render, screen } from "@testing-library/react";
import { CardGrid } from "~/components/card-grid";
import { enter, stagger } from "./enter";
import { Swap } from "./swap";

describe("enter", () => {
  it("gives the item its stagger index and the variant's class", () => {
    const lift = enter(3);
    const fade = enter(3, "fade");
    expect(lift.style).toEqual({ "--i": 3 });
    expect(lift.className).not.toBe("");
    expect(fade.className).not.toBe(lift.className);
  });

  it("numbers a staggered container's children with --j", () => {
    expect(stagger(2)).toEqual({ "--j": 2 });
  });
});

describe("CardGrid", () => {
  it("staggers its cards in order", () => {
    const { container } = render(
      <CardGrid items={["a", "b", "c"]} getKey={(k) => k}>
        {(k) => <span>{k}</span>}
      </CardGrid>
    );
    const grid = container.firstElementChild as HTMLElement;
    expect(grid).toHaveAttribute("data-stagger");
    const cells = Array.from(grid.children) as HTMLElement[];
    expect(cells.map((c) => c.style.getPropertyValue("--j"))).toEqual([
      "0",
      "1",
      "2",
    ]);
  });
});

describe("Swap", () => {
  it("remounts its content when the state changes, and only then", () => {
    const { rerender } = render(
      <Swap k="live">
        <b>1s</b>
      </Swap>
    );
    const first = screen.getByText("1s").parentElement;

    rerender(
      <Swap k="live">
        <b>2s</b>
      </Swap>
    );
    expect(screen.getByText("2s").parentElement).toBe(first);

    rerender(
      <Swap k="paused">
        <b>Paused</b>
      </Swap>
    );
    expect(screen.getByText("Paused").parentElement).not.toBe(first);
  });
});
