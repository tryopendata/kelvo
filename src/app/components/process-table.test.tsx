import type { LiveProcess } from "@core/generated/bindings";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import {
  type ProcessSort,
  ProcessTable,
  processKey,
  sortProcesses,
} from "./process-table";

function proc(
  over: Partial<LiveProcess> & Pick<LiveProcess, "pid" | "name">
): LiveProcess {
  return {
    start_time_us: 1_700_000_000_000_000 + over.pid,
    cpu_pct: 0,
    mem_bytes: 0,
    compressed_bytes: null,
    threads: 1,
    idle_wakeups_per_s: null,
    energy: null,
    disk_read_bps: null,
    disk_write_bps: null,
    net_rx_bps: null,
    net_tx_bps: null,
    gpu_pct: null,
    ports: null,
    user: "me",
    refusal: null,
    ...over,
  };
}

const ROWS: LiveProcess[] = [
  proc({ pid: 391, name: "WindowServer", cpu_pct: 33.6, threads: 22 }),
  proc({ pid: 2214, name: "Xcode", cpu_pct: 86.8, threads: 74 }),
  proc({ pid: 0, name: "kernel_task", cpu_pct: 43.4, threads: 612 }),
  proc({ pid: 5531, name: "node", cpu_pct: null, threads: 12 }),
];

function Harness({ initial }: { initial: ProcessSort }) {
  const [sort, setSort] = useState(initial);
  return (
    <ProcessTable
      rows={ROWS}
      columns={["name", "pid", "cpu", "threads"]}
      sort={sort}
      onSort={setSort}
      height={400}
      ariaLabel="Top processes"
    />
  );
}

function names(): string[] {
  const table = screen.getByRole("table", { name: "Top processes" });
  return within(table)
    .getAllByRole("row")
    .slice(1)
    .map((r) => within(r).getAllByRole("cell")[0]?.textContent ?? "")
    .filter(Boolean)
    .map((t) => t.slice(1)); // drop the initial chip
}

describe("ProcessTable", () => {
  it("sorts by % CPU descending with missing values last", () => {
    render(<Harness initial={{ by: "cpu", dir: "desc" }} />);
    expect(names()).toEqual(["Xcode", "kernel_task", "WindowServer", "node"]);
    expect(screen.getByRole("columnheader", { name: /% CPU/ })).toHaveAttribute(
      "aria-sort",
      "descending"
    );
  });

  it("flips direction on the sorted column and starts text columns ascending", async () => {
    const user = userEvent.setup();
    render(<Harness initial={{ by: "cpu", dir: "desc" }} />);

    await user.click(screen.getByRole("button", { name: /% CPU/ }));
    expect(names()).toEqual(["WindowServer", "kernel_task", "Xcode", "node"]);

    await user.click(screen.getByRole("button", { name: /Process/ }));
    expect(names()).toEqual(["kernel_task", "node", "WindowServer", "Xcode"]);
    expect(
      screen.getByRole("columnheader", { name: /Process/ })
    ).toHaveAttribute("aria-sort", "ascending");
  });

  it("keys rows by pid and start time, so a reused pid is a new row", () => {
    const a = proc({ pid: 42, name: "a", start_time_us: 1 });
    const b = proc({ pid: 42, name: "b", start_time_us: 2 });
    expect(processKey(a)).not.toBe(processKey(b));

    const { container, rerender } = render(
      <ProcessTable
        rows={[a]}
        columns={["name"]}
        sort={{ by: "name", dir: "asc" }}
        onSort={() => {}}
        height={200}
        ariaLabel="p"
      />
    );
    const before = container.querySelector("tr[data-key]");
    expect(before).toHaveAttribute("data-key", "42:1");
    rerender(
      <ProcessTable
        rows={[b]}
        columns={["name"]}
        sort={{ by: "name", dir: "asc" }}
        onSort={() => {}}
        height={200}
        ariaLabel="p"
      />
    );
    expect(container.querySelector("tr[data-key]")).toHaveAttribute(
      "data-key",
      "42:2"
    );
  });

  it("sortProcesses does not mutate its input", () => {
    const copy = [...ROWS];
    sortProcesses(ROWS, { by: "threads", dir: "asc" });
    expect(ROWS).toEqual(copy);
  });
});
