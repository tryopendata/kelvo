import { act, render } from "@testing-library/react";
import { useElementSize } from "./use-element-size";

type Callback = (entries: { contentRect: DOMRectReadOnly }[]) => void;

let observed: Callback | null = null;

class FakeResizeObserver {
  constructor(cb: Callback) {
    observed = cb;
  }
  observe() {}
  disconnect() {}
}

function resize(width: number, height: number) {
  act(() =>
    observed?.([{ contentRect: { width, height } as DOMRectReadOnly }])
  );
}

function Probe({ axis }: { axis: "width" | "height" }) {
  const [ref, size] = useElementSize(axis, 480);
  return <div ref={ref}>{size}</div>;
}

describe("useElementSize", () => {
  const original = globalThis.ResizeObserver;
  beforeEach(() => {
    globalThis.ResizeObserver =
      FakeResizeObserver as unknown as typeof ResizeObserver;
  });
  afterEach(() => {
    globalThis.ResizeObserver = original;
    observed = null;
  });

  it("starts at the initial size and follows the named axis", () => {
    const { container } = render(<Probe axis="height" />);
    expect(container.textContent).toBe("480");
    resize(900, 312.5);
    expect(container.textContent).toBe("312.5");
  });

  it("keeps the last size when the element measures zero", () => {
    const { container } = render(<Probe axis="width" />);
    resize(640, 200);
    resize(0, 0);
    expect(container.textContent).toBe("640");
  });
});
