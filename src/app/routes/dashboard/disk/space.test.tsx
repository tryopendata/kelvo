import type { HostRecord } from "@core/generated/bindings";
import { MOCK_SCAN_MS } from "@core/mock/disk-space";
import type { ScenarioName } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Profiler, useState } from "react";
import { describe, expect, it } from "vitest";
import { HostSelectionContext } from "~/lib/host-selection";
import { HostStoreProvider } from "~/stores/host-store";
import DiskRoute from "./route";

// A scan polls once a second; give a poll room to land.
const POLL = { timeout: 3000 };

function setup(route = "/?view=space", scenarios: ScenarioName[] = []) {
  let t = 1_700_000_000_000;
  const transport = createMockTransport({
    now: () => t,
    autoTick: false,
    scenarios,
  });
  const result = renderWithProviders(<DiskRoute />, { transport, route });
  return {
    ...result,
    /** Moves the mock scan's clock; the next poll sees it. */
    advance: (ms: number) => {
      t += ms;
    },
  };
}

const contents = () => screen.getByRole("table", { name: "Folder contents" });
const row = (name: string | RegExp) =>
  within(contents()).getByRole("row", { name });
const search = (r: ReturnType<typeof setup>) =>
  new URLSearchParams(r.router.state.location.search);

async function scanBoot(r: ReturnType<typeof setup>) {
  await r.user.click(
    await screen.findByRole("button", { name: "Scan Macintosh HD" })
  );
  await screen.findByText(/Scanning · about/);
}

async function finish(r: ReturnType<typeof setup>) {
  r.advance(MOCK_SCAN_MS);
  await screen.findByRole("button", { name: "Scan again" }, POLL);
}

describe("Disk › Space", () => {
  it("switches from Activity to Space and back, keeping Activity's params", async () => {
    const r = setup("/?device=disk0&response=ops");
    await r.user.click(await screen.findByRole("radio", { name: "Space" }));
    expect(
      await screen.findByRole("region", { name: "Volumes to scan" })
    ).toBeVisible();
    expect(search(r).get("view")).toBe("space");
    await r.user.click(screen.getByRole("radio", { name: "Activity" }));
    await screen.findByRole("region", { name: "Volume capacity" });
    expect(search(r).get("view")).toBeNull();
    expect(search(r).get("device")).toBe("disk0");
    expect(search(r).get("response")).toBe("ops");
  });

  it("opens Space for a volume from Activity's Analyze link", async () => {
    const r = setup("/");
    const volumes = await screen.findByRole("region", {
      name: "Volume capacity",
    });
    // Only scannable volumes get the link: "/" yes, the Data mount no.
    const links = await within(volumes).findAllByRole("button", {
      name: "Analyze space",
    });
    expect(links).toHaveLength(1);
    await r.user.click(links[0] as HTMLElement);
    expect(search(r).get("view")).toBe("space");
    expect(search(r).get("vol")).toBe("/");
    expect(
      await screen.findByText("Macintosh HD · 436 GB of 995 GB used")
    ).toBeVisible();
  });

  it("fills the report in while scanning, then shows the finished breakdown", async () => {
    const r = setup();
    await scanBoot(r);
    expect(search(r).get("vol")).toBe("/");
    // Top-level folders are listed at once; unfinished ones say so.
    expect(row(/^Users, scanning/)).toHaveTextContent("Scanning…");
    expect(screen.getByRole("radio", { name: "Largest files" })).toBeDisabled();
    await finish(r);
    // The finished tree replaces the last in-progress poll.
    await waitFor(() =>
      expect(within(contents()).queryByText("Scanning…")).toBeNull()
    );
    expect(row(/^Other/)).toBeVisible();
    expect(row(/^Users, 234 GB, folder/)).toBeVisible();
    expect(
      screen.getByRole("img", { name: /^Treemap of Macintosh HD/ })
    ).toBeVisible();
    expect(screen.queryByText(/Scanning · about/)).toBeNull();
    expect(
      screen.getByText(/Skipped, protected by macOS: Desktop/)
    ).toBeVisible();
  });

  it("does not open a folder that is still scanning", async () => {
    const r = setup();
    await scanBoot(r);
    act(() => row(/^Users, scanning/).focus());
    await r.user.keyboard("{Enter}");
    expect(
      screen.getByRole("navigation", { name: "Folder path" })
    ).not.toHaveTextContent("Users");
  });

  it("drills down and back up with Finder's keys and the breadcrumb", async () => {
    const r = setup();
    await scanBoot(r);
    await finish(r);
    const path = () => screen.getByRole("navigation", { name: "Folder path" });
    act(() => row(/^Users, /).focus());
    await r.user.keyboard("{Enter}");
    await waitFor(() => expect(path()).toHaveTextContent(/Macintosh HDUsers$/));
    act(() => row(/^riley, /).focus());
    await r.user.keyboard("{Meta>}{ArrowDown}{/Meta}");
    await waitFor(() => expect(path()).toHaveTextContent(/Usersriley$/));
    expect(row(/^Projects, /)).toBeVisible();
    act(() => row(/^Projects, /).focus());
    await r.user.keyboard("{Meta>}{ArrowUp}{/Meta}");
    await waitFor(() => expect(path()).not.toHaveTextContent("riley"));
    act(() => row(/^riley, /).focus());
    await r.user.keyboard("{Escape}");
    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: "In Macintosh HD" })
      ).toBeVisible()
    );
    await r.user.click(within(path()).getByRole("button", { name: "Volumes" }));
    expect(
      await screen.findByRole("button", { name: "Open report" })
    ).toBeVisible();
    expect(search(r).get("vol")).toBeNull();
    await r.user.click(screen.getByRole("button", { name: "Open report" }));
    expect(await screen.findByText("Used")).toBeVisible();
  });

  it("enters protected locations only when included", async () => {
    const r = setup();
    await r.user.click(
      await screen.findByText("Protected locations · 7 of 7 skipped")
    );
    await r.user.click(screen.getByRole("checkbox", { name: "Downloads" }));
    expect(
      screen.getByText("Protected locations · 6 of 7 skipped")
    ).toBeVisible();
    await scanBoot(r);
    await finish(r);
    const note = screen.getByText(/Skipped, protected by macOS/);
    expect(note).not.toHaveTextContent("Downloads");
    expect(note).toHaveTextContent("Documents");
  });

  it("lists the largest files and copies a path as Finder shows it", async () => {
    const r = setup();
    await scanBoot(r);
    await finish(r);
    await r.user.click(screen.getByRole("radio", { name: "Largest files" }));
    const table = await screen.findByRole("table", { name: "Largest files" });
    const first = within(table).getAllByRole("row")[1] as HTMLElement;
    expect(first).toHaveTextContent("census.parquet");
    expect(first).toHaveTextContent("Users/riley/Projects/opendata/data");
    await r.user.click(
      within(first).getByRole("button", {
        name: "Copy the path of census.parquet",
      })
    );
    await waitFor(async () =>
      expect(await navigator.clipboard.readText()).toBe(
        "/Users/riley/Projects/opendata/data/census.parquet"
      )
    );
    expect(
      screen.getByText(
        "Copied /Users/riley/Projects/opendata/data/census.parquet"
      )
    ).toBeVisible();
  });

  it("marks the volume being scanned and leads back to its scan", async () => {
    const r = setup();
    await scanBoot(r);
    const path = screen.getByRole("navigation", { name: "Folder path" });
    await r.user.click(within(path).getByRole("button", { name: "Volumes" }));
    // Another volume can't start while this one runs.
    expect(
      await screen.findByRole("button", { name: "Scan SanDisk Extreme" })
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "Scan Macintosh HD" })
    ).toBeNull();
    await r.user.click(
      screen.getByRole("button", { name: "Show the scan of Macintosh HD" })
    );
    expect(await screen.findByText(/Scanning · about/)).toBeVisible();
    expect(search(r).get("vol")).toBe("/");
  });

  it("keeps the previous report when a rescan is cancelled", async () => {
    const r = setup();
    await scanBoot(r);
    await finish(r);
    await r.user.click(screen.getByRole("button", { name: "Scan again" }));
    await screen.findByText(/Scanning · about/);
    await r.user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(
      await screen.findByText(
        "The last scan was cancelled before it finished. The previous report is shown."
      )
    ).toBeVisible();
    expect(screen.getByText("Used")).toBeVisible();
  });

  it("says why a scan failed when its volume disconnects", async () => {
    const r = setup("/?view=space", ["disk-space-eject"]);
    await r.user.click(
      await screen.findByRole("button", { name: "Scan SanDisk Extreme" })
    );
    await screen.findByText(/Scanning · about/);
    r.advance(MOCK_SCAN_MS);
    expect(
      await screen.findByText(
        "Couldn't finish scanning SanDisk Extreme: the volume was disconnected.",
        {},
        POLL
      )
    ).toBeVisible();
    expect(
      screen.getByRole("region", { name: "Volumes to scan" })
    ).toBeVisible();
  });
});

describe("Disk › Space outside a local scan", () => {
  it("has no Space view in the App Store edition", async () => {
    setup("/?view=space", ["appstore"]);
    expect(
      await screen.findByRole("region", { name: "Volume capacity" })
    ).toBeVisible();
    expect(screen.queryByRole("radio", { name: "Space" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Analyze space" })).toBeNull();
  });

  it("offers This Mac when a remote host is selected", async () => {
    const transport = createMockTransport({
      scenarios: ["multi-host"],
      autoTick: false,
    });
    const hosts = await transport.listHosts();
    const remote = hosts.find((h) => !h.is_local) as HostRecord;
    function Harness() {
      const [selected, select] = useState(remote.id);
      return (
        <HostSelectionContext value={{ hosts, selected, select }}>
          <HostStoreProvider hostId={selected}>
            <DiskRoute />
          </HostStoreProvider>
        </HostSelectionContext>
      );
    }
    const r = renderWithProviders(<Harness />, {
      transport,
      route: "/?view=space",
    });
    expect(
      await screen.findByText("Space reports are available for This Mac only.")
    ).toBeVisible();
    await r.user.click(screen.getByRole("button", { name: "Show This Mac" }));
    expect(
      await screen.findByRole("region", { name: "Volumes to scan" })
    ).toBeVisible();
  });

  it("does not re-render a finished report on the 1 Hz tick", async () => {
    let commits = 0;
    let t = 1_700_000_000_000;
    const transport = createMockTransport({ now: () => t, autoTick: false });
    const r = renderWithProviders(
      <Profiler id="space" onRender={() => commits++}>
        <DiskRoute />
      </Profiler>,
      { transport, route: "/?view=space" }
    );
    await r.user.click(
      await screen.findByRole("button", { name: "Scan Macintosh HD" })
    );
    t += MOCK_SCAN_MS;
    await screen.findByRole("button", { name: "Scan again" }, POLL);
    await waitFor(() =>
      expect(within(contents()).queryByText("Scanning…")).toBeNull()
    );
    const before = commits;
    act(() => {
      for (let i = 0; i < 3; i++) transport.tick();
    });
    expect(commits).toBe(before);
  });
});
