import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// Data enters the frontend only through @core/transport. Each test builds its
// own mock transport (renderWithProviders in tests/test-utils.tsx), so no
// frame or subscription leaks between tests and there is nothing global to
// reset. Do not vi.mock @tauri-apps/* here: tests go through the transport
// seam, not around it.

// Mock localStorage for tests (happy-dom sometimes has issues)
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
    get length() {
      return Object.keys(store).length;
    },
    key: (index: number) => Object.keys(store)[index] ?? null,
  };
})();

Object.defineProperty(globalThis, "localStorage", {
  value: localStorageMock,
  writable: true,
});

// happy-dom returns all zeros from getBoundingClientRect, so chart primitives
// that size their scales from the container would render at 0x0. Give every
// element a plausible card-sized box.
Element.prototype.getBoundingClientRect = () => ({
  x: 0,
  y: 0,
  width: 320,
  height: 160,
  top: 0,
  right: 320,
  bottom: 160,
  left: 0,
  toJSON() {
    return this;
  },
});

afterEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
});
