import { containerRouteState, restoredScroll } from "./route-state";

it("normalizes unknown filters and caps custom chart membership without duplicate slots", () => {
  const state = containerRouteState(
    new URLSearchParams(
      "metric=constructor&sort=wrong&state=toString&range=constructor&end=Infinity&columns=ports,unknown,ports,pids&chart=a,a,b,c,d,e,f,g"
    ),
    125_999,
    null
  );
  expect(state).toMatchObject({
    metric: "cpu",
    sort: "cpu",
    state: "all",
    range: "15m",
    end: null,
    to: 120_000,
    from: -780_000,
    columns: ["ports", "pids"],
    custom: ["a", "b", "c", "d", "e", "f"],
  });
});

it("preserves explicit empty chart selection and valid saved range/navigation choices", () => {
  const state = containerRouteState(
    new URLSearchParams(
      "metric=memory&sort=name&view=storage&group=grouped&project=api&q=worker&range=1h&end=600000&chart=&collapsed=api&collapsed=db"
    ),
    900_000,
    "cpu"
  );
  expect(state).toMatchObject({
    metric: "memory",
    sort: "name",
    storageView: true,
    grouped: true,
    project: "api",
    query: "worker",
    to: 600_000,
    from: -3_000_000,
    custom: [],
    collapsed: ["api", "db"],
  });
});

it("uses persisted sort only when the URL has no supported sort", () => {
  expect(containerRouteState(new URLSearchParams(), 0, "restarts").sort).toBe(
    "restarts"
  );
  expect(
    containerRouteState(new URLSearchParams("sort=memory"), 0, "restarts").sort
  ).toBe("memory");
});

it.each([
  null,
  {},
  { scroll: "5" },
  { scroll: -1 },
  { scroll: Infinity },
  { scroll: NaN },
])("ignores invalid browser-back scroll state %j", (state) => {
  expect(restoredScroll(state)).toBeUndefined();
});
it.each([0, 125.5])("restores finite scroll position %s", (scroll) => {
  expect(restoredScroll({ scroll })).toBe(scroll);
});
