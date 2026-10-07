import { matchProcessQuery } from "@core/app-search";
import { countNoun } from "@core/format";

describe("matchProcessQuery", () => {
  const xcode = { name: "Xcode", pid: 1840, ports: [5432, 8080] };

  it("is null for a blank query, so every row stays", () => {
    expect(matchProcessQuery("   ")).toBeNull();
  });

  it("matches a case-insensitive substring of the name", () => {
    expect(matchProcessQuery(" XCO ")?.(xcode)).toBe(true);
    expect(matchProcessQuery("safari")?.(xcode)).toBe(false);
  });

  it("matches the leading digits of a PID or a listening port", () => {
    expect(matchProcessQuery("18")?.(xcode)).toBe(true);
    expect(matchProcessQuery("80")?.(xcode)).toBe(true);
    expect(matchProcessQuery("84")?.(xcode)).toBe(false);
    expect(matchProcessQuery("18")?.({ name: "Xcode" })).toBe(false);
  });
});

describe("countNoun", () => {
  it("agrees with the count and groups thousands", () => {
    expect(countNoun(1, "process", "processes")).toBe("1 process");
    expect(countNoun(0, "row", "rows")).toBe("0 rows");
    expect(countNoun(1440, "row", "rows")).toBe("1,440 rows");
  });
});
