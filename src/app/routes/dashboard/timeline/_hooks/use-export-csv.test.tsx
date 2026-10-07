import { createMockTransport } from "@core/mock-transport";
import { screen } from "@testing-library/react";
import { toast } from "sonner";
import { renderWithProviders } from "../../../../../../tests/test-utils";
import { LANES } from "../_lib/lanes";
import { useExportCsv } from "./use-export-csv";

const FROM = new Date(2026, 9, 4, 8, 0).getTime();
const TO = FROM + 86_400_000;

function Probe() {
  const { exportCsv, exporting } = useExportCsv();
  return (
    <button
      type="button"
      disabled={exporting}
      onClick={() =>
        void exportCsv({ span: "24h", fromMs: FROM, toMs: TO, lanes: LANES })
      }
    >
      Export
    </button>
  );
}

function setup(options: Parameters<typeof createMockTransport>[0] = {}) {
  const success = vi.spyOn(toast, "success").mockImplementation(() => 0);
  const error = vi.spyOn(toast, "error").mockImplementation(() => 0);
  vi.spyOn(console, "error").mockImplementation(() => {});
  const transport = createMockTransport({ autoTick: false, ...options });
  const r = renderWithProviders(<Probe />, { transport });
  const exportCalls = () =>
    transport.calls.filter((c) => c.command === "export_csv");
  return { ...r, success, error, exportCalls };
}

afterEach(() => vi.restoreAllMocks());

describe("useExportCsv", () => {
  it("exports the visible range and every lane's series, then says where", async () => {
    const { user, success, exportCalls } = setup();
    await user.click(screen.getByRole("button", { name: "Export" }));
    await vi.waitFor(() => expect(success).toHaveBeenCalledTimes(1));
    const req = exportCalls()[0]?.args[0] as {
      selectors: { metric: string }[];
      from_ms: number;
      to_ms: number;
      tier: string;
      file_name: string;
    };
    expect(req.from_ms).toBe(FROM);
    expect(req.to_ms).toBe(TO);
    expect(req.tier).toBe("auto");
    expect(req.file_name).toBe("kelvo-2026-10-04-0800-24h.csv");
    expect(req.selectors.map((s) => s.metric)).toEqual(
      LANES.flatMap((l) => l.metrics.map((m) => m.metric))
    );
    expect(String(success.mock.calls[0]?.[0])).toMatch(
      /^Exported [\d,]+ rows.* to \/Users\/mock\/Downloads\/kelvo-2026-10-04-0800-24h\.csv$/
    );
  });

  it("says nothing when the save dialog is cancelled", async () => {
    const { user, success, error, exportCalls } = setup({
      exportCancels: true,
    });
    await user.click(screen.getByRole("button", { name: "Export" }));
    await vi.waitFor(() => expect(exportCalls()).toHaveLength(1));
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Export" })).toBeEnabled()
    );
    expect(success).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
  });

  it("re-enables the button and toasts when the invoke itself throws", async () => {
    const transport = createMockTransport({ autoTick: false });
    transport.exportCsv = () => Promise.reject(new Error("bridge gone"));
    const success = vi.spyOn(toast, "success").mockImplementation(() => 0);
    const error = vi.spyOn(toast, "error").mockImplementation(() => 0);
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    const { user } = renderWithProviders(<Probe />, { transport });
    await user.click(screen.getByRole("button", { name: "Export" }));
    await vi.waitFor(() => expect(error).toHaveBeenCalledTimes(1));
    expect(error.mock.calls[0]?.[0]).toBe("Export failed: bridge gone");
    expect(logged).toHaveBeenCalled();
    expect(success).not.toHaveBeenCalled();
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Export" })).toBeEnabled()
    );
  });

  it("toasts a failure", async () => {
    const { user, success, error } = setup({
      scenarios: ["history-unavailable"],
    });
    await user.click(screen.getByRole("button", { name: "Export" }));
    await vi.waitFor(() => expect(error).toHaveBeenCalledTimes(1));
    expect(error.mock.calls[0]?.[0]).toBe(
      "Export failed: no history is being kept."
    );
    expect(success).not.toHaveBeenCalled();
  });
});
