import { requestTime } from "./disk-io-charts";

describe("completed request time labels", () => {
  it("does not round a positive duration down to a real zero", () => {
    expect(requestTime(0.001, 12)).toBe("<0.01 ms");
    expect(requestTime(0, 12)).toBe("0 ms");
    expect(requestTime(0.095, 12)).toBe("0.1 ms");
  });
  it("distinguishes idle completion counts from unsupported counters", () => {
    expect(requestTime(null, 0)).toBe("— · No completed operations");
    expect(requestTime(null, null)).toBe("— · Unavailable");
    expect(requestTime(null, 12)).toBe("— · Unavailable");
  });
});
