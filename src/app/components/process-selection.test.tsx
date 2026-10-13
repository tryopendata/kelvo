import { createMockTransport } from "@core/mock-transport";
import type { ProcessTarget } from "@core/process-selection";
import { targetKey } from "@core/process-selection";
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import {
  ProcessSelectionHead,
  ProcessSelectionRow,
  ProcessSelectionSurface,
} from "./process-selection";

const targets: ProcessTarget[] = [1, 2, 3].map((pid) => ({
  pid,
  name: `python ${pid}`,
  start_time_us: pid,
  refusal: null,
}));
function Harness({
  scope = "live",
  members = targets,
}: {
  scope?: string;
  members?: ProcessTarget[];
}) {
  const [query, setQuery] = useState("");
  const shown = members.filter((p) => p.name.includes(query));
  const rows = shown.map((p) => ({
    key: targetKey(p),
    label: p.name,
    targets: [p],
  }));
  return (
    <>
      <input
        aria-label="Search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <ProcessSelectionSurface rows={rows} allTargets={shown} scopeKey={scope}>
        <table aria-label="Test processes">
          <thead>
            <tr>
              <ProcessSelectionHead />
              <th>Process</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((p) => (
              <ProcessSelectionRow
                key={targetKey(p)}
                rowKey={targetKey(p)}
                label={p.name}
                targets={[p]}
              >
                <td>{p.name}</td>
              </ProcessSelectionRow>
            ))}
          </tbody>
        </table>
      </ProcessSelectionSurface>
    </>
  );
}
const row = (name: string) =>
  screen.getByRole("cell", { name }).closest("tr") as HTMLTableRowElement;

describe("shared process interaction", () => {
  it("supports click, Command toggles, Shift range, hidden review and no automatic newcomers", async () => {
    const { user } = renderWithProviders(<Harness />);
    await user.click(screen.getByRole("cell", { name: "python 1" }));
    fireEvent.click(row("python 3"), { metaKey: true });
    expect(screen.getByText("2 selected")).toBeVisible();
    fireEvent.click(row("python 2"), { shiftKey: true });
    expect(
      screen.getByRole("checkbox", { name: "Select python 1" })
    ).not.toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "Select python 2" })
    ).toBeChecked();
    await user.type(
      screen.getByRole("textbox", { name: "Search" }),
      "python 1"
    );
    expect(screen.getByText("2 selected · 2 hidden")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Selected only" }));
    const review = screen.getByRole("table", { name: "Selected processes" });
    expect(within(review).getByText("python 2")).toBeVisible();
    expect(within(review).getByText("python 3")).toBeVisible();
    expect(within(review).queryByText("python 1")).toBeNull();
  });

  it("keyboard range, focus-only movement, toggle, select-all and clear", async () => {
    renderWithProviders(<Harness />);
    fireEvent.click(row("python 1"));
    fireEvent.keyDown(row("python 1"), { key: "ArrowDown", shiftKey: true });
    expect(screen.getByText("2 selected")).toBeVisible();
    fireEvent.keyDown(row("python 2"), { key: "End", metaKey: true });
    expect(row("python 3")).toHaveFocus();
    expect(screen.getByText("2 selected")).toBeVisible();
    fireEvent.keyDown(row("python 3"), { key: " " });
    expect(screen.getByText("3 selected")).toBeVisible();
    fireEvent.keyDown(row("python 3"), { key: "Escape" });
    expect(screen.queryByText("3 selected")).toBeNull();
    fireEvent.keyDown(row("python 3"), { key: "a", metaKey: true });
    expect(screen.getByText("3 selected")).toBeVisible();
    fireEvent.click(
      screen.getByRole("checkbox", { name: "Select all matching processes" })
    );
    expect(screen.queryByText("3 selected")).toBeNull();
  });

  it("gutter drag selects a range without a release click reducing it", () => {
    renderWithProviders(<Harness />);
    fireEvent.pointerDown(
      row("python 1").querySelector("[data-selection-gutter]") as HTMLElement,
      { button: 0 }
    );
    fireEvent.pointerEnter(row("python 3"));
    expect(screen.getByText("3 selected")).toBeVisible();
    fireEvent.pointerUp(window);
    fireEvent.click(
      row("python 3").querySelector("[data-selection-gutter]") as HTMLElement
    );
    expect(screen.getByText("3 selected")).toBeVisible();
  });

  it("confirms once for an immutable group and reports partial results", async () => {
    const transport = createMockTransport({ autoTick: false });
    const sent: number[] = [];
    transport.processSignal = async (_host, pid) => {
      sent.push(pid);
      return pid === 2
        ? { status: "error", error: { kind: "permission_denied" } }
        : { status: "ok", data: null };
    };
    const { user } = renderWithProviders(<Harness />, { transport });
    await waitFor(() =>
      expect(transport.calls.some((c) => c.command === "get_edition")).toBe(
        true
      )
    );
    await user.click(
      screen.getByRole("checkbox", { name: "Select all matching processes" })
    );
    await user.click(screen.getByRole("button", { name: "Quit 3…" }));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Quit 3 processes?",
    });
    expect(sent).toEqual([]);
    await user.click(
      within(dialog).getByRole("button", { name: "Quit 3 processes" })
    );
    await waitFor(() => expect(sent).toEqual([1, 2, 3]));
    await user.click(
      screen.getByText("Batch results: 2 of 3 requests accepted")
    );
    expect(screen.getByText(/another user/)).toBeVisible();
    expect(screen.getByText("3 selected")).toBeVisible();
  });

  it("stops after an unknown transport result and does not retry", async () => {
    const transport = createMockTransport({ autoTick: false });
    const sent: number[] = [];
    transport.processSignal = async (_host, pid) => {
      sent.push(pid);
      throw new Error("connection lost");
    };
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    const { user } = renderWithProviders(<Harness />, { transport });
    await user.click(
      screen.getByRole("checkbox", { name: "Select all matching processes" })
    );
    await user.click(screen.getByRole("button", { name: "Force Quit 3…" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "Force Quit 3 processes" })
    );
    await user.click(
      await screen.findByText("Batch results: 0 of 3 requests accepted")
    );
    expect(sent).toEqual([1]);
    expect(
      screen.getByText(/whether the request arrived is unknown/)
    ).toBeVisible();
    expect(screen.getAllByText(/Not sent/)).toHaveLength(2);
    log.mockRestore();
  });

  it("stops the unsent tail when its view is unmounted", async () => {
    const transport = createMockTransport({ autoTick: false });
    let finish: (() => void) | undefined;
    const sent: number[] = [];
    transport.processSignal = (_host, pid) => {
      sent.push(pid);
      return new Promise((resolve) => {
        finish = () => resolve({ status: "ok", data: null });
      });
    };
    const { user, unmount } = renderWithProviders(<Harness />, { transport });
    await user.click(
      screen.getByRole("checkbox", { name: "Select all matching processes" })
    );
    await user.click(screen.getByRole("button", { name: "Quit 3…" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(
      within(dialog).getByRole("button", { name: "Quit 3 processes" })
    );
    expect(sent).toEqual([1]);
    unmount();
    await act(async () => finish?.());
    expect(sent).toEqual([1]);
  });
});
