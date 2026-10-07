import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NumberTicker } from "./number-ticker";

const root = document.documentElement;

describe("NumberTicker", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "performance"] });
    root.style.setProperty("--motion-count", "400ms");
  });
  afterEach(() => {
    vi.useRealTimers();
    root.style.removeProperty("--motion-count");
  });

  it("counts down across units and lands on the exact text", () => {
    const { container, rerender } = render(<NumberTicker text="10.0 GB" />);
    const span = () => container.querySelector("span")?.textContent;
    expect(span()).toBe("10.0 GB");

    rerender(<NumberTicker text="100 MB" />);
    // Before the first frame the old value is still on screen.
    expect(span()).toBe("10.0 GB");
    const seen = new Set<string>();
    for (let i = 0; i < 30; i++) {
      vi.advanceTimersByTime(16);
      seen.add(span() ?? "");
    }
    expect([...seen].some((s) => s.endsWith(" GB") && s !== "10.0 GB")).toBe(
      true
    );
    expect([...seen].some((s) => s.endsWith(" MB") && s !== "100 MB")).toBe(
      true
    );
    expect(span()).toBe("100 MB");
  });

  it("retargets from the live value without jumping back", () => {
    const { container, rerender } = render(<NumberTicker text="20%" />);
    const value = () =>
      Number.parseFloat(container.querySelector("span")?.textContent ?? "");
    rerender(<NumberTicker text="80%" />);
    vi.advanceTimersByTime(100);
    const mid = value();
    expect(mid).toBeGreaterThan(20);
    rerender(<NumberTicker text="90%" />);
    vi.advanceTimersByTime(16);
    expect(value()).toBeGreaterThanOrEqual(mid);
    vi.advanceTimersByTime(500);
    expect(value()).toBe(90);
  });

  it("replaces small moves, other kinds and a unit change in place", () => {
    const { container, rerender } = render(
      <NumberTicker text="40" unit="GB USED" />
    );
    const span = () => container.querySelector("span")?.textContent;
    rerender(<NumberTicker text="42" unit="GB USED" />);
    expect(span()).toBe("42");
    rerender(<NumberTicker text="840" unit="MB USED" />);
    expect(span()).toBe("840");
    rerender(<NumberTicker text="—" unit="MB USED" />);
    expect(span()).toBe("—");
    vi.advanceTimersByTime(500);
    expect(span()).toBe("—");
  });

  it("lands at once when the token is 0 (reduced motion, Performance mode)", () => {
    root.style.setProperty("--motion-count", "0ms");
    const { container, rerender } = render(<NumberTicker text="10.0 GB" />);
    rerender(<NumberTicker text="100 MB" />);
    expect(container.querySelector("span")?.textContent).toBe("100 MB");
  });

  it("a small move during a count lands it on the new text", () => {
    const { container, rerender } = render(<NumberTicker text="20%" />);
    const span = () => container.querySelector("span")?.textContent;
    rerender(<NumberTicker text="80%" />);
    vi.advanceTimersByTime(100);
    rerender(<NumberTicker text="—" />);
    vi.advanceTimersByTime(500);
    expect(span()).toBe("—");
  });
});
