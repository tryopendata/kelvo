import {
  holdProcessRows,
  type ProcessSelection,
  type ProcessTarget,
  type SelectableProcessRow,
  selectProcessRow,
  targetKey,
} from "./process-selection";

const p = (pid: number, extra: Partial<ProcessTarget> = {}): ProcessTarget => ({
  pid,
  name: "python",
  start_time_us: pid,
  refusal: null,
  ...extra,
});
const processes = [p(1), p(2), p(3), p(4)];
const rows: SelectableProcessRow[] = processes.map((p) => ({
  key: targetKey(p),
  label: p.name,
  targets: [p],
}));
const empty: ProcessSelection = { targets: [], anchor: null };
const ids = (s: ProcessSelection) => s.targets.map((p) => p.pid);

describe("process selection", () => {
  it("replaces, toggles, extends in both directions and contracts around an anchor", () => {
    const first = selectProcessRow(empty, rows, "2:2");
    expect(ids(selectProcessRow(first, rows, "4:4", { range: true }))).toEqual([
      2, 3, 4,
    ]);
    const extended = selectProcessRow(first, rows, "4:4", { range: true });
    expect(
      ids(selectProcessRow(extended, rows, "3:3", { range: true }))
    ).toEqual([2, 3]);
    expect(ids(selectProcessRow(first, rows, "1:1", { range: true }))).toEqual([
      1, 2,
    ]);
    const toggled = selectProcessRow(first, rows, "4:4", { toggle: true });
    expect(ids(toggled)).toEqual([2, 4]);
    expect(
      ids(selectProcessRow(toggled, rows, "2:2", { toggle: true }))
    ).toEqual([4]);
    expect(ids(selectProcessRow(toggled, rows, "3:3"))).toEqual([3]);
  });

  it("adds ranges without losing a disjoint selection", () => {
    const first = selectProcessRow(empty, rows, "1:1");
    const anchored = selectProcessRow(first, rows, "3:3", { toggle: true });
    expect(
      ids(
        selectProcessRow(anchored, rows, "4:4", { range: true, additive: true })
      )
    ).toEqual([1, 3, 4]);
  });

  it("deduplicates group members, supports exclusions and never adopts new members", () => {
    const group = {
      key: "app",
      label: "Python",
      targets: [p(1), p(2), p(3, { running: false })],
    };
    const grouped = [group, ...rows];
    const selected = selectProcessRow(empty, grouped, "app");
    expect(ids(selected)).toEqual([1, 2]);
    const excluded = selectProcessRow(selected, grouped, "2:2", {
      toggle: true,
    });
    expect(ids(excluded)).toEqual([1]);
    expect(
      ids(selectProcessRow(excluded, grouped, "app", { toggle: true }))
    ).toEqual([1, 2]);
    group.targets.push(p(4));
    expect(ids(selected)).toEqual([1, 2]);
  });

  it("keeps old identities through filtering and never selects a reused PID", () => {
    const selected = selectProcessRow(empty, rows, "2:2");
    const newProcess = p(2, { start_time_us: 99 });
    const newRows = [
      { key: targetKey(newProcess), label: "python", targets: [newProcess] },
    ];
    expect(selectProcessRow(selected, newRows, "missing")).toBe(selected);
    expect(
      selectProcessRow(selected, newRows, "2:99", { toggle: true }).targets.map(
        targetKey
      )
    ).toEqual(["2:2", "2:99"]);
  });

  it("holds sibling order, appends newcomers and keeps expanded children under their group", () => {
    const grouped = [
      { key: "b", label: "b", targets: [] },
      { key: "a", label: "a", targets: [] },
      { key: "child", parentKey: "a", label: "child", targets: [] },
      { key: "new", label: "new", targets: [] },
    ];
    expect(
      holdProcessRows(grouped, ["a", "gone", "b"]).map((r) => r.key)
    ).toEqual(["a", "child", "b", "new"]);
  });
});
