import { execFileSync } from "node:child_process";
import { expect, test } from "vitest";

interface ListedSuite {
  file: string;
  specs: { tests: { projectName: string }[] }[];
  suites?: ListedSuite[];
}

function list(performance: boolean) {
  const report = JSON.parse(
    execFileSync("bunx", ["playwright", "test", "--list", "--reporter=json"], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        PLAYWRIGHT_PERFORMANCE: performance ? "1" : "0",
        PLAYWRIGHT_REUSE_SERVER: "0",
      },
      encoding: "utf8",
    })
  ) as { suites: ListedSuite[]; config: { workers: number } };
  const cases: { file: string; project: string }[] = [];
  const visit = (suite: ListedSuite) => {
    for (const spec of suite.specs)
      for (const item of spec.tests)
        cases.push({ file: suite.file, project: item.projectName });
    for (const child of suite.suites ?? []) visit(child);
  };
  for (const suite of report.suites) visit(suite);
  return { cases, workers: report.config.workers };
}

test("default browser command collects functional cases in both engines", () => {
  const { cases } = list(false);
  expect(cases.length).toBeGreaterThan(0);
  expect(new Set(cases.map((item) => item.project))).toEqual(
    new Set(["chromium", "webkit"])
  );
  expect(cases.every((item) => !item.file.endsWith("perf-gate.spec.ts"))).toBe(
    true
  );
});

test("performance mode collects only Chromium performance cases", () => {
  const { cases, workers } = list(true);
  expect(workers).toBe(1);
  expect(cases.length).toBeGreaterThan(0);
  expect(new Set(cases.map((item) => item.project))).toEqual(
    new Set(["chromium"])
  );
  expect(cases.every((item) => item.file.endsWith("perf-gate.spec.ts"))).toBe(
    true
  );
});
