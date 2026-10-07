import { act, render } from "@testing-library/react";
import { type CSSProperties, Profiler } from "react";
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

/** What the element measures on mount, before the observer reports. */
function layoutBox(width: number, height: number) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width,
    height,
  } as DOMRect);
}

function Probe({
  axis,
  style,
}: {
  axis: "width" | "height";
  style?: CSSProperties;
}) {
  const [ref, size] = useElementSize(axis, 480);
  return (
    <div ref={ref} style={style}>
      {size}
    </div>
  );
}

describe("useElementSize", () => {
  const original = globalThis.ResizeObserver;
  beforeEach(() => {
    globalThis.ResizeObserver =
      FakeResizeObserver as unknown as typeof ResizeObserver;
    layoutBox(0, 0);
  });
  afterEach(() => {
    globalThis.ResizeObserver = original;
    observed = null;
    vi.restoreAllMocks();
  });

  it("starts at the initial size and follows the named axis", () => {
    const { container } = render(<Probe axis="height" />);
    expect(container.textContent).toBe("480");
    resize(900, 312.5);
    expect(container.textContent).toBe("312.5");
  });

  it("seeds from the element's content box before the observer reports", () => {
    layoutBox(700, 300);
    const { container } = render(
      <Probe axis="width" style={{ padding: "0 8px" }} />
    );
    expect(container.textContent).toBe("684");
  });

  it("a change on the other axis commits nothing", () => {
    let commits = 0;
    render(
      <Profiler id="probe" onRender={() => commits++}>
        <Probe axis="width" />
      </Profiler>
    );
    resize(640, 200);
    // React may render once more to confirm a same-value update right after
    // a real one; it bails out eagerly from then on.
    resize(640, 300);
    const before = commits;
    resize(640, 900);
    resize(640, 120);
    expect(commits).toBe(before);
  });

  it("keeps the last size when the element measures zero", () => {
    const { container } = render(<Probe axis="width" />);
    resize(640, 200);
    resize(0, 0);
    expect(container.textContent).toBe("640");
  });
});
