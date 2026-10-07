import type { ClusterInfo } from "@core/generated/bindings";
import { clusterViews } from "./clusters";

const e0: ClusterInfo = {
  name: "E0",
  kind: "efficiency",
  cores: ["E0", "E1"],
  dvfs_mhz: [1020, 2890],
};
const p0: ClusterInfo = {
  name: "P0",
  kind: "performance",
  cores: ["P0"],
  dvfs_mhz: [1260, 4510],
};

describe("clusterViews", () => {
  it("puts performance clusters first", () => {
    const p1 = { ...p0, name: "P1", cores: ["P1"] };
    expect(clusterViews([e0, p0, p1])).toEqual([
      { letter: "P", cores: ["P0"] },
      { letter: "P", cores: ["P1"] },
      { letter: "E", cores: ["E0", "E1"] },
    ]);
  });
});
