import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { BrushProvider } from "~/stores/brush-store";
import { DiskProcesses } from "./disk-processes";

const NOW = 1_700_000_005_000;
const RANGE = { fromMs: NOW - 15_000, toMs: NOW - 5000 };

it.each(["selected", "live"])(
  "process links preserve the current %s brush before its URL mirror catches up",
  async (mode) => {
    const selected = mode === "selected";
    renderWithProviders(
      <BrushProvider initial={selected ? RANGE : null}>
        <DiskProcesses windowMs={300_000} />
      </BrushProvider>,
      {
        transportOptions: { now: () => NOW, autoTick: false },
        route: `/dashboard/disk?device=disk7&response=ops${selected ? "" : "&from=1000&to=2000"}`,
      }
    );
    const link = await screen.findByRole("link", {
      name: selected ? "Inspect clang, PID 6120" : "Inspect Xcode, PID 2214",
    });
    const destination = new URL(
      link.getAttribute("href") ?? "",
      "http://localhost"
    );
    const back = destination.searchParams.get("back");
    expect(back).not.toBeNull();
    const returnTo = new URL(back ?? "", "http://localhost");
    expect(returnTo.pathname).toBe("/dashboard/disk");
    expect(returnTo.searchParams.get("device")).toBe("disk7");
    expect(returnTo.searchParams.get("response")).toBe("ops");
    for (const url of [destination, returnTo]) {
      expect(url.searchParams.get("from")).toBe(
        selected ? String(RANGE.fromMs) : null
      );
      expect(url.searchParams.get("to")).toBe(
        selected ? String(RANGE.toMs) : null
      );
    }
  }
);
