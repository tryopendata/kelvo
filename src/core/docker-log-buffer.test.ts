import { describe, expect, it } from "vitest";
import {
  appendLogLines,
  emptyLogBuffer,
  LOG_LINE_BYTES,
  LOG_MAX_BYTES,
  LOG_MAX_LINES,
} from "./docker-log-buffer";

describe("Docker log display buffer", () => {
  it("bounds many short lines while preserving stable identity", () => {
    const result = appendLogLines(
      emptyLogBuffer(),
      Array.from({ length: 2_101 }, (_, i) => `line ${i}`)
    );
    expect(result.lines).toHaveLength(LOG_MAX_LINES);
    expect(result.lines[0]?.id).toBe(101);
    expect(result.discarded).toBe(101);
  });
  it("caps multibyte lines and the total UTF-8 buffer", () => {
    const result = appendLogLines(
      emptyLogBuffer(),
      Array.from({ length: 50 }, () => "🙂".repeat(30_000))
    );
    expect(result.bytes).toBeLessThanOrEqual(LOG_MAX_BYTES);
    expect(
      result.lines.every(
        (line) => line.truncated && line.bytes <= LOG_LINE_BYTES
      )
    ).toBe(true);
    expect(result.discarded).toBeGreaterThan(0);
  });
  it("keeps text literal and does not mutate the previous buffer", () => {
    const first = appendLogLines(emptyLogBuffer(), [
      "<script>alert(1)</script>",
    ]);
    const next = appendLogLines(first, ["second"]);
    expect(first.lines).toHaveLength(1);
    expect(next.lines[0]?.text).toBe("<script>alert(1)</script>");
  });
});
