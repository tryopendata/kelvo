import { USAGE_BUCKET_MS } from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { usageSeriesFinal, useUsageSeries } from "./use-usage-series";

const NOW = 1_800_000_000_000;
const HOUR = 3_600_000;

function Probe() {
  const q = useUsageSeries({ fromMs: NOW - HOUR, toMs: NOW }, "energy", 5);
  if (!q.data) return <p>loading</p>;
  const first = q.data.apps[0];
  const lead = first?.values.findIndex((v) => v !== null) ?? -1;
  const measured = first?.values.slice(lead).every((v) => v !== null && v >= 0);
  return (
    <p>
      {`apps ${q.data.apps.length} buckets ${first?.values.length} lead ${lead} measured ${measured}`}
    </p>
  );
}

describe("useUsageSeries", () => {
  it("reads the top apps per 10 s bucket, unmeasured before counting began", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    renderWithProviders(<Probe />, { transport });
    // The mock has counted for its 600 s ring: the first 50 minutes are gaps.
    await screen.findByText(
      `apps 5 buckets ${HOUR / USAGE_BUCKET_MS} lead 300 measured true`
    );
    const calls = transport.calls.filter(
      (c) => c.command === "query_usage_series"
    );
    expect(calls).toEqual([
      {
        command: "query_usage_series",
        args: [expect.any(String), NOW - HOUR, NOW, "energy", 5],
      },
    ]);
  });

  it("is final once the buckets through the range's end are complete", () => {
    const answer = {
      from_ms: 0,
      bucket_ms: USAGE_BUCKET_MS,
      since_ms: 0,
      complete_to_ms: 20_000,
      latest_ms: 25_000,
      apps: [],
    };
    expect(usageSeriesFinal(answer, 15_000)).toBe(true);
    expect(usageSeriesFinal(answer, 25_000)).toBe(false);
    expect(usageSeriesFinal(undefined, 0)).toBe(false);
  });
});
