import {
  FULL_TABLE,
  ProcessInterest,
  type ProcessView,
  unionViews,
} from "./process-interest";

const overview: ProcessView = {
  limit: 5,
  sort: ["cpu", "memory"],
  period_ms: 5000,
};

describe("unionViews", () => {
  it("is null with no consumers", () => {
    expect(unionViews([])).toBeNull();
  });

  it("takes the largest limit, every sort key and the shortest period", () => {
    expect(
      unionViews([
        overview,
        { limit: 8, sort: ["energy", "cpu"], period_ms: 2000 },
      ])
    ).toEqual({ limit: 8, sort: ["cpu", "memory", "energy"], period_ms: 2000 });
  });

  it("reads an empty sort as CPU", () => {
    expect(
      unionViews([
        { limit: 3, sort: [], period_ms: null },
        { limit: 3, sort: ["memory"], period_ms: 1000 },
      ])
    ).toEqual({ limit: 3, sort: ["cpu", "memory"], period_ms: null });
  });

  it("asks for network rates when any consumer shows them", () => {
    const net: ProcessView = {
      limit: 5,
      sort: ["net_total"],
      period_ms: 5000,
      network: true,
    };
    expect(unionViews([overview, net])).toEqual({
      limit: 5,
      sort: ["cpu", "memory", "net_total"],
      period_ms: 5000,
      network: true,
    });
    expect(unionViews([FULL_TABLE, net])).toEqual({
      ...FULL_TABLE,
      network: true,
    });
    expect(unionViews([overview])?.network).toBeUndefined();
  });

  it("asks for GPU time when any consumer shows it, apart from network", () => {
    const gpu: ProcessView = {
      limit: 5,
      sort: ["gpu"],
      period_ms: 5000,
      gpu: true,
    };
    expect(unionViews([overview, gpu])).toEqual({
      limit: 5,
      sort: ["cpu", "memory", "gpu"],
      period_ms: 5000,
      gpu: true,
    });
    expect(unionViews([overview])?.gpu).toBeUndefined();
  });

  it("asks for listening ports when any consumer shows them", () => {
    const ports: ProcessView = { ...FULL_TABLE, ports: true };
    expect(unionViews([overview, ports])).toEqual(ports);
    expect(unionViews([overview])?.ports).toBeUndefined();
  });

  it("is the full table at every sample when any consumer wants it", () => {
    expect(unionViews([overview, FULL_TABLE])).toEqual(FULL_TABLE);
  });
});

describe("ProcessInterest", () => {
  const flush = () => new Promise<void>((r) => queueMicrotask(r));

  function setup() {
    const sent: { view: ProcessView | null; stream: number }[] = [];
    const interest = new ProcessInterest((view, stream) =>
      sent.push({ view, stream })
    );
    return { interest, sent };
  }

  it("waits for the stream, then sends one union for consumers that mount together", async () => {
    const { interest, sent } = setup();
    const a = Symbol("a");
    const b = Symbol("b");
    interest.set(a, overview);
    interest.set(b, { limit: 8, sort: ["cpu"], period_ms: null });
    await flush();
    expect(sent).toEqual([]);

    interest.setStream(7);
    await flush();
    expect(sent).toEqual([
      {
        view: { limit: 8, sort: ["cpu", "memory"], period_ms: null },
        stream: 7,
      },
    ]);
  });

  it("sends the narrower view when a consumer leaves, and none when all have", async () => {
    const { interest, sent } = setup();
    const a = Symbol("a");
    const b = Symbol("b");
    interest.setStream(1);
    interest.set(a, overview);
    interest.set(b, FULL_TABLE);
    await flush();
    interest.remove(b);
    await flush();
    interest.remove(a);
    await flush();
    expect(sent.map((s) => s.view)).toEqual([FULL_TABLE, overview, null]);
  });

  it("does not resend an unchanged union", async () => {
    const { interest, sent } = setup();
    const a = Symbol("a");
    interest.setStream(1);
    interest.set(a, overview);
    await flush();
    // A route remount: remove then set again in one turn.
    interest.remove(a);
    interest.set(a, { ...overview, sort: [...overview.sort] });
    await flush();
    expect(sent).toHaveLength(1);
  });

  it("resends when only the network flag changes", async () => {
    const { interest, sent } = setup();
    const a = Symbol("a");
    interest.setStream(1);
    interest.set(a, FULL_TABLE);
    await flush();
    interest.set(a, { ...FULL_TABLE, network: true });
    await flush();
    expect(sent.map((s) => s.view?.network)).toEqual([undefined, true]);
  });

  it("resends when only the GPU flag changes", async () => {
    const { interest, sent } = setup();
    const a = Symbol("a");
    interest.setStream(1);
    interest.set(a, FULL_TABLE);
    await flush();
    interest.set(a, { ...FULL_TABLE, gpu: true });
    await flush();
    expect(sent.map((s) => s.view?.gpu)).toEqual([undefined, true]);
  });

  it("resends the union for a new stream after a resubscribe", async () => {
    const { interest, sent } = setup();
    interest.set(Symbol("a"), overview);
    interest.setStream(1);
    await flush();
    interest.setStream(null);
    interest.setStream(2);
    await flush();
    expect(sent).toEqual([
      { view: overview, stream: 1 },
      { view: overview, stream: 2 },
    ]);
  });

  it("says nothing to a new stream when there is no interest", async () => {
    const { interest, sent } = setup();
    interest.setStream(1);
    await flush();
    expect(sent).toEqual([]);
  });
});
