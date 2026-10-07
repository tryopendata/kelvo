import { initialRoute } from "./initial-route";

describe("initialRoute", () => {
  it("maps each window label to its entry route", () => {
    expect(initialRoute("popover")).toBe("/popover");
    expect(initialRoute("dashboard")).toBe("/dashboard/overview");
    expect(initialRoute("onboarding")).toBe("/onboarding");
  });

  it("falls back to the dashboard for an unknown label", () => {
    expect(initialRoute("board-1")).toBe("/dashboard/overview");
  });

  it("honours ?route= only when overrides are allowed", () => {
    const search = "?window=popover&route=/dev/gallery";
    expect(initialRoute("popover", search, true)).toBe("/dev/gallery");
    expect(initialRoute("popover", search, false)).toBe("/popover");
  });

  it("starts a dashboard at the route Rust loaded as its URL path", () => {
    expect(initialRoute("dashboard", "", false, "/dashboard/settings")).toBe(
      "/dashboard/settings"
    );
    expect(initialRoute("dashboard", "", false, "/")).toBe(
      "/dashboard/overview"
    );
    // The popover's URL path never moves it off its own route.
    expect(initialRoute("popover", "", false, "/dashboard/cpu")).toBe(
      "/popover"
    );
  });

  it("ignores a route override that is not a path", () => {
    expect(initialRoute("popover", "?route=javascript:x", true)).toBe(
      "/popover"
    );
  });
});
