import capability from "../../../src-tauri/capabilities/default.json";
import { sensorIssueUrl } from "./sensor-dump-dialog";

/**
 * The webview may open only the URLs it has a reason to (plan 4.17's sensor
 * dump issue). tauri-plugin-opener matches each `url` entry as a
 * `glob::Pattern` against the whole URL; with default match options `*` is
 * any run of characters (slashes included) and `?` any one character. This
 * translation covers those two; a pattern with a `[` class fails the test
 * rather than being matched loosely.
 */
function globToRegExp(pattern: string): RegExp {
  if (pattern.includes("[")) {
    throw new Error(`character classes are not translated: ${pattern}`);
  }
  const body = pattern
    .split("")
    .map((c) =>
      c === "*" ? ".*" : c === "?" ? "." : c.replace(/[.+^${}()|\\/]/g, "\\$&")
    )
    .join("");
  return new RegExp(`^${body}$`);
}

type Permission = (typeof capability.permissions)[number];

const openerEntries = capability.permissions.filter((p: Permission) =>
  (typeof p === "string" ? p : p.identifier).startsWith("opener:")
);

const allowed = (url: string) =>
  openerEntries.some(
    (p) =>
      typeof p !== "string" &&
      p.identifier === "opener:allow-open-url" &&
      p.allow.some((a) => globToRegExp(a.url).test(url))
  );

describe("opener capability scope", () => {
  it("grants no blanket opener permission", () => {
    const names = openerEntries.map((p) =>
      typeof p === "string" ? p : p.identifier
    );
    expect(names).toEqual(["opener:allow-open-url"]);
  });

  it("allows the sensor dump issue link for any model", () => {
    expect(allowed(sensorIssueUrl("Mac15,9"))).toBe(true);
    expect(allowed(sensorIssueUrl(null))).toBe(true);
  });

  it("refuses every other URL", () => {
    for (const url of [
      "https://example.com/",
      "http://github.com/tryopendata/kelvo/issues/new?labels=sensors&title=x",
      "https://github.com/someone/else/issues/new?labels=sensors&title=x",
      "https://github.com/tryopendata/kelvo/settings",
      "file:///etc/passwd",
      "mailto:someone@example.com",
    ]) {
      expect(allowed(url), url).toBe(false);
    }
  });
});
