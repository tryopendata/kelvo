import { heldFor } from "./use-range-scope";

const WINDOW_MS = 900_000;
const answer = (span: number) => ({ from_ms: 0, to_ms: span });

describe("heldFor (D-099)", () => {
  it("shows a fresh answer whatever its span", () => {
    const data = answer(90_000);
    expect(heldFor({ data, isPlaceholderData: false }, WINDOW_MS)).toBe(data);
  });

  it("holds the window's previous answer while its key moves", () => {
    const data = answer(WINDOW_MS);
    expect(heldFor({ data, isPlaceholderData: true }, WINDOW_MS)).toBe(data);
  });

  it("never lets a cleared selection's answer stand in for the window", () => {
    expect(
      heldFor({ data: answer(90_000), isPlaceholderData: true }, WINDOW_MS)
    ).toBeUndefined();
  });
});
