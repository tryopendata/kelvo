import { act, screen } from "@testing-library/react";
import { vi } from "vitest";
import { type HostStore, useHostStore } from "~/stores/host-store";
import { renderWithProviders } from "../../../../../tests/test-utils";
import { SetupStep } from "./setup-step";

// ModuleToggleList is not memoized, so it renders exactly when SetupStep does.
const renders = vi.hoisted(() => ({ toggleList: 0 }));
vi.mock("~/components/module-toggle-list", async (importOriginal) => {
  const real =
    await importOriginal<typeof import("~/components/module-toggle-list")>();
  return {
    ...real,
    ModuleToggleList: (props: Parameters<typeof real.ModuleToggleList>[0]) => {
      renders.toggleList += 1;
      return real.ModuleToggleList(props);
    },
  };
});

describe("SetupStep at 1 Hz", () => {
  it("a tick re-renders the Graph per module card, not the whole step", async () => {
    let store: HostStore | undefined;
    function GrabStore() {
      store = useHostStore();
      return null;
    }
    renderWithProviders(
      <>
        <GrabStore />
        <SetupStep
          hostInfo={undefined}
          busy={false}
          onSkip={() => {}}
          onContinue={() => {}}
        />
      </>
    );
    await screen.findByRole("radio", { name: /Graph per module/ });
    const before = renders.toggleList;
    const start = store?.getState().lastTsMs ?? 0;

    // New ticks whose values are unchanged: only the sparkline takes a sample.
    for (let i = 1; i <= 3; i++) {
      act(() => store?.setState({ lastTsMs: start + i * 1000 }));
    }
    expect(renders.toggleList).toBe(before);
  });
});
