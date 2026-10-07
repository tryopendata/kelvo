import { cn } from "~/lib/utils";

describe("cn", () => {
  it("drops falsy values and lets the later Tailwind class win", () => {
    const hidden = false;
    expect(cn("p-2", hidden && "hidden", "p-4")).toBe("p-4");
  });
});
