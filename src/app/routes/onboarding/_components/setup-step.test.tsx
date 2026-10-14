import { act, screen } from "@testing-library/react";
import { Profiler } from "react";
import { vi } from "vitest";
import { type HostStore, useHostStore } from "~/stores/host-store";
import { renderWithProviders } from "../../../../../tests/test-utils";
import { SetupStep } from "./setup-step";

// Observe commits to the real static subtree, without invoking components as functions.
const renders = vi.hoisted(() => ({ toggleList: 0, trayCards: 0 }));
vi.mock("~/components/module-toggle-list", async (importOriginal) => {
  const real =
    await importOriginal<typeof import("~/components/module-toggle-list")>();
  return {
    ...real,
    ModuleToggleList: (props: Parameters<typeof real.ModuleToggleList>[0]) => {
      return (
        <Profiler
          id="module-controls"
          onRender={() => {
            renders.toggleList += 1;
          }}
        >
          <real.ModuleToggleList {...props} />
        </Profiler>
      );
    },
  };
});

vi.mock("./tray-style-cards", async (importOriginal) => {
  const real = await importOriginal<typeof import("./tray-style-cards")>();
  return {
    ...real,
    TrayStyleCards: (props: Parameters<typeof real.TrayStyleCards>[0]) => (
      <Profiler
        id="live-tray-previews"
        onRender={() => {
          renders.trayCards += 1;
        }}
      >
        <real.TrayStyleCards {...props} />
      </Profiler>
    ),
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
    const previewsBefore = renders.trayCards;
    expect(before).toBeGreaterThan(0);
    const start = store?.getState().lastTsMs ?? 0;

    // New ticks whose values are unchanged: only the sparkline takes a sample.
    for (let i = 1; i <= 3; i++) {
      act(() => store?.setState({ lastTsMs: start + i * 1000 }));
    }
    expect(renders.toggleList).toBe(before);
    expect(renders.trayCards).toBe(previewsBefore + 3);
  });
});
