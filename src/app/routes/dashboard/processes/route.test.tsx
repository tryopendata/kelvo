import { PROCESSES } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Toaster } from "~/components/ui/sonner";
import { renderWithProviders } from "../../../../../tests/test-utils";
import ProcessesRoute from "./route";

function renderPage() {
  return renderWithProviders(
    <>
      <ProcessesRoute />
      <Toaster />
    </>
  );
}

const fixture = (name: string) => {
  const p = PROCESSES.find((row) => row.name === name);
  if (!p) throw new Error(`no fixture ${name}`);
  return p;
};

const signals = (calls: { command: string; args: unknown[] }[]) =>
  calls.filter((c) => c.command === "process_signal");

describe("Processes page", () => {
  it("lists every process the mock sends and counts them", async () => {
    renderPage();
    expect(
      await screen.findByRole("cell", { name: /Xcode/ })
    ).toBeInTheDocument();
    expect(screen.getByText(/^\d+ processes · [\d,]+ threads$/)).toBeVisible();
    expect(
      screen.getByText(/processes owned by other users are hidden/i)
    ).toBeVisible();
  });

  it("filters by name or PID", async () => {
    const { user } = renderPage();
    await screen.findByRole("cell", { name: /Xcode/ });
    const search = screen.getByRole("searchbox", { name: /search processes/i });

    await user.type(search, "safari");
    expect(screen.queryByRole("cell", { name: /Xcode/ })).toBeNull();
    expect(screen.getByRole("cell", { name: /^Safari$/ })).toBeVisible();
    expect(screen.getByText(/^2 of \d+ processes/)).toBeVisible();

    await user.clear(search);
    await user.type(search, "2214");
    expect(screen.getByRole("cell", { name: /Xcode/ })).toBeVisible();
    expect(screen.queryByRole("cell", { name: /^Safari$/ })).toBeNull();
  });

  it("switches columns and default sort with the column set", async () => {
    const { user } = renderPage();
    await screen.findByRole("cell", { name: /Xcode/ });
    await user.click(screen.getByRole("radio", { name: "Disk" }));
    expect(
      screen.getByRole("columnheader", { name: /Disk total/ })
    ).toHaveAttribute("aria-sort", "descending");
    expect(screen.queryByRole("columnheader", { name: /Threads/ })).toBeNull();
  });

  it("sends Quit only after the dialog is confirmed", async () => {
    const { user, transport } = renderPage();
    const xcode = fixture("Xcode");
    await user.click(
      await screen.findByRole("button", { name: "Quit Xcode (2214)" })
    );
    const dialog = await screen.findByRole("dialog", { name: "Quit Xcode?" });
    expect(dialog).toHaveTextContent("PID 2214");
    expect(signals(transport.calls)).toEqual([]);

    await user.click(within(dialog).getByRole("button", { name: "Quit" }));
    await waitFor(() =>
      expect(signals(transport.calls)).toEqual([
        {
          command: "process_signal",
          args: [expect.any(String), 2214, xcode.start_time_us, "quit"],
        },
      ])
    );
    expect(await screen.findByText("Asked Xcode (2214) to quit")).toBeVisible();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("sends nothing when the dialog is cancelled", async () => {
    const { user, transport } = renderPage();
    await user.click(
      await screen.findByRole("button", { name: "Force Quit Xcode (2214)" })
    );
    const dialog = await screen.findByRole("dialog", {
      name: "Force quit Xcode?",
    });
    expect(dialog).toHaveTextContent(
      "Unsaved data in this process will be lost."
    );
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(signals(transport.calls)).toEqual([]);
  });

  it("disables both actions for refused processes and explains why", async () => {
    const { user, transport } = renderPage();
    const quit = await screen.findByRole("button", {
      name: "Quit kernel_task (0)",
    });
    expect(quit).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByRole("button", { name: "Force Quit WindowServer (391)" })
    ).toHaveAttribute("aria-disabled", "true");

    await user.click(quit);
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => {
      quit.blur();
      quit.focus();
    });
    expect(
      (await screen.findAllByText(/kernel_task is the macOS kernel/))[0]
    ).toBeInTheDocument();
    expect(signals(transport.calls)).toEqual([]);
  });

  it("shows the EPERM toast for another user's process", async () => {
    const { user } = renderPage();
    await user.click(
      await screen.findByRole("button", { name: "Quit mds_stores (466)" })
    );
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Quit" }));
    expect(
      await screen.findByText(
        "Kelvo can't quit processes owned by another user"
      )
    ).toBeVisible();
  });

  it("offers the same actions in the row context menu", async () => {
    const { user, transport } = renderPage();
    const cell = await screen.findByRole("cell", { name: /Figma/ });
    fireEvent.contextMenu(cell);
    const menu = await screen.findByRole("menu");
    await user.click(
      within(menu).getByRole("menuitem", { name: "Force Quit…" })
    );
    const dialog = await screen.findByRole("dialog", {
      name: "Force quit Figma?",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "Force Quit" })
    );
    await waitFor(() =>
      expect(signals(transport.calls)[0]?.args.slice(1, 2)).toEqual([1712])
    );
  });
});

describe("Processes page, network columns (D-081)", () => {
  const views = (calls: { command: string; args: unknown[] }[]) =>
    calls
      .filter((c) => c.command === "set_process_interest")
      .map((c) => c.args[2] as { network?: boolean } | null);

  it("shows down, up and total by process and asks for rates while the set is on", async () => {
    const { user, transport } = renderPage();
    await screen.findByRole("cell", { name: /Xcode/ });
    expect(views(transport.calls).at(-1)?.network).toBeUndefined();
    await user.click(screen.getByRole("radio", { name: "Network" }));
    await waitFor(() =>
      expect(views(transport.calls).at(-1)?.network).toBe(true)
    );
    expect(
      screen.getByRole("columnheader", { name: /Net total/ })
    ).toHaveAttribute("aria-sort", "descending");
    // The mock's next batch carries rates now that the view asks for them.
    act(() => transport.tick());
    const table = screen.getByRole("table", { name: "Processes" });
    await waitFor(() =>
      expect(within(table).getAllByRole("row")[1]?.textContent).toContain(
        "Safari"
      )
    );
    expect(within(table).getAllByRole("row")[1]?.textContent).toContain(
      "21.4 MB/s"
    );
    expect(screen.getByText(/cover your processes only/)).toBeVisible();

    await user.click(screen.getByRole("radio", { name: "CPU" }));
    await waitFor(() =>
      expect(views(transport.calls).at(-1)?.network).toBeUndefined()
    );
  });

  it("has no Network set when the host cannot attribute traffic", async () => {
    renderWithProviders(<ProcessesRoute />, {
      transportOptions: { scenarios: ["no-process-network"] },
    });
    await screen.findByRole("cell", { name: /Xcode/ });
    expect(screen.getByRole("radio", { name: "Disk" })).toBeVisible();
    expect(screen.queryByRole("radio", { name: "Network" })).toBeNull();
  });
});

describe("Processes page, GPU column (D-085)", () => {
  const views = (calls: { command: string; args: unknown[] }[]) =>
    calls
      .filter((c) => c.command === "set_process_interest")
      .map((c) => c.args[2] as { gpu?: boolean } | null);

  it("shows GPU share by process and asks for it while the set is on", async () => {
    const { user, transport } = renderPage();
    await screen.findByRole("cell", { name: /Xcode/ });
    expect(views(transport.calls).at(-1)?.gpu).toBeUndefined();
    await user.click(screen.getByRole("radio", { name: "GPU" }));
    await waitFor(() => expect(views(transport.calls).at(-1)?.gpu).toBe(true));
    expect(screen.getByRole("columnheader", { name: /% GPU/ })).toHaveAttribute(
      "aria-sort",
      "descending"
    );
    // The mock answers a view that starts asking for GPU time at once.
    const table = screen.getByRole("table", { name: "Processes" });
    await waitFor(() =>
      expect(within(table).getAllByRole("row")[1]?.textContent).toContain(
        "WindowServer"
      )
    );
    expect(within(table).getAllByRole("row")[1]?.textContent).toContain("14.2");
    expect(screen.getByText(/long GPU compute job/)).toBeVisible();

    await user.click(screen.getByRole("radio", { name: "CPU" }));
    await waitFor(() =>
      expect(views(transport.calls).at(-1)?.gpu).toBeUndefined()
    );
  });

  it("has no GPU set when the host cannot attribute GPU time", async () => {
    renderWithProviders(<ProcessesRoute />, {
      transportOptions: { scenarios: ["no-process-gpu"] },
    });
    await screen.findByRole("cell", { name: /Xcode/ });
    expect(screen.getByRole("radio", { name: "Network" })).toBeVisible();
    expect(screen.queryByRole("radio", { name: "GPU" })).toBeNull();
  });
});

describe("Processes page, App Store edition (D-065)", () => {
  it("has no Quit or Force Quit when the edition cannot signal", async () => {
    renderWithProviders(<ProcessesRoute />, {
      transportOptions: { scenarios: ["appstore"] },
    });
    const cell = await screen.findByRole("cell", { name: /Xcode/ });
    expect(screen.queryByRole("button", { name: /Quit Xcode/ })).toBeNull();
    fireEvent.contextMenu(cell);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("drops the actions when process_signal answers unavailable", async () => {
    // An edition answer that disagrees with the command: the command wins.
    const transport = createMockTransport({
      scenarios: ["appstore"],
      autoTick: false,
    });
    transport.getEdition = async () => ({ process_signal: true });
    const { user } = renderWithProviders(
      <>
        <ProcessesRoute />
        <Toaster />
      </>,
      { transport }
    );
    await user.click(
      await screen.findByRole("button", { name: "Quit Xcode (2214)" })
    );
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Quit" }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /Quit Xcode/ })).toBeNull()
    );
    expect(screen.getByRole("cell", { name: /Xcode/ })).toBeVisible();
  });
});
