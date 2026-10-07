import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import {
  annotationFraction,
  ChartAnnotation,
  type ChartAnnotationProps,
} from "./chart-annotation";

const props: ChartAnnotationProps = {
  tsMs: 1_760_000_000_000,
  label: "ANE 1.4 W · Photos face analysis",
};

describe("ChartAnnotation", () => {
  it("renders the same after a JSON round trip", () => {
    expectJsonRoundTrip(ChartAnnotation, props);
    expectJsonRoundTrip(ChartAnnotation, { ...props, variant: "pill" });
  });

  it("shows its label", () => {
    const { getByText } = render(<ChartAnnotation {...props} />);
    expect(getByText(props.label)).toBeTruthy();
  });

  it("positions within a window and rejects moments outside it", () => {
    expect(annotationFraction(950, 1000, 100)).toBeCloseTo(0.5);
    expect(annotationFraction(1000, 1000, 100)).toBe(1);
    expect(annotationFraction(800, 1000, 100)).toBeNull();
    expect(annotationFraction(1100, 1000, 100)).toBeNull();
  });
});
