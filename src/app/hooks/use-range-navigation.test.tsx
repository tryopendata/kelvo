import { createMockTransport } from "@core/mock-transport";
import { act, screen } from "@testing-library/react";
import { useState } from "react";
import { Toaster } from "sonner";
import { renderWithProviders } from "../../../tests/test-utils";
import {
  RangeNavigationProvider,
  useRangeNavigation,
} from "./use-range-navigation";

const FIXED = { absolute: { from_ms: 1_000_000, to_ms: 1_120_000 } };
function Probe() {
  const navigation = useRangeNavigation();
  if (!navigation.range) return <p>Loading</p>;
  return (
    <>
      <output>{JSON.stringify(navigation.range.window)}</output>
      <button type="button" onClick={() => void navigation.apply(FIXED)}>
        Zoom
      </button>
      <button
        type="button"
        disabled={!navigation.canBack}
        onClick={navigation.back}
      >
        Back
      </button>
      <button type="button" onClick={navigation.live}>
        Live
      </button>
    </>
  );
}
function Session() {
  const [shown, setShown] = useState(true);
  return (
    <RangeNavigationProvider>
      <button type="button" onClick={() => setShown(!shown)}>
        Tab
      </button>
      {shown && <Probe />}
    </RangeNavigationProvider>
  );
}

describe("range navigation", () => {
  afterEach(() => vi.restoreAllMocks());
  it("shares committed settings and keeps Back across tab unmounts", async () => {
    const { user, transport } = renderWithProviders(<Session />);
    await screen.findByText('"15m"');
    await user.click(screen.getByRole("button", { name: "Zoom" }));
    await screen.findByText(JSON.stringify(FIXED));
    await user.click(screen.getByRole("button", { name: "Tab" }));
    await user.click(screen.getByRole("button", { name: "Tab" }));
    await user.click(screen.getByRole("button", { name: "Back" }));
    await screen.findByText('"15m"');
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
    expect((await transport.getSettings()).settings.general.chart_window).toBe(
      "15m"
    );
  });
  it("does not add settings events from another window to Back", async () => {
    const { transport } = renderWithProviders(<Session />);
    await screen.findByText('"15m"');
    await act(() =>
      transport.updateSettings({ general: { chart_window: FIXED } })
    );
    await screen.findByText(JSON.stringify(FIXED));
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
  });
  it("retains the old range and empty Back after a rejected save", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const transport = createMockTransport({
      autoTick: false,
      settingsNotSaved: true,
    });
    const { user } = renderWithProviders(
      <>
        <Toaster />
        <Session />
      </>,
      { transport }
    );
    await screen.findByText('"15m"');
    await user.click(screen.getByRole("button", { name: "Zoom" }));
    await screen.findByRole("button", { name: "Retry" });
    expect(screen.getByText('"15m"')).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
  });
  it("resumes 24h Live after reload into an absolute selection", async () => {
    const transport = createMockTransport({
      autoTick: false,
      chartWindow: FIXED,
    });
    const { user } = renderWithProviders(<Session />, { transport });
    await screen.findByText(JSON.stringify(FIXED));
    await user.click(screen.getByRole("button", { name: "Live" }));
    await screen.findByText('{"relative":86400000}');
  });
});
