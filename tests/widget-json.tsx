import { cleanup, render } from "@testing-library/react";
import type { ComponentType } from "react";
import { expect } from "vitest";

/**
 * React's client `useId` values come from a global counter, so two mounts of
 * the same tree get different ids. Normalise them before comparing markup.
 */
function normaliseIds(html: string): string {
  // React 19.3 emits `_r_1a_` (client) / `_R_…_` (server); older 19.x `«r0»`.
  return html.replace(/«[^»]*»|:r[0-9a-z]+:|_[rR]_[0-9a-z]+_/g, "«id»");
}

function markup<P extends object>(Component: ComponentType<P>, props: P) {
  const { container } = render(<Component {...props} />);
  const html = normaliseIds(container.innerHTML);
  cleanup();
  return html;
}

/**
 * The widget contract (design-system.md "Component inventory", infra 10):
 * data props survive `JSON.stringify` unchanged and render the same markup
 * after a round trip. Catches `Date`, `Map`, typed arrays, class instances
 * and `undefined`-vs-missing differences that would break the widget feed.
 */
export function expectJsonRoundTrip<P extends object>(
  Component: ComponentType<P>,
  props: P
): void {
  const json = JSON.parse(JSON.stringify(props)) as P;
  expect(json).toStrictEqual(props);
  const original = markup(Component, props);
  expect(original.length).toBeGreaterThan(0);
  expect(markup(Component, json)).toBe(original);
}
