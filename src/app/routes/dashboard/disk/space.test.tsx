import { MOCK_SCAN_MS } from "@core/mock/disk-space";
import { MOCK_HOST_ID, type ScenarioName } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import { hostKeys } from "@core/query-keys";
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
    // Top-level folders are listed at once: a running one with what it has
    // so far, one the scan has not reached yet as waiting.
    expect(row(/^Users, scanning/)).toHaveTextContent(/MB so far/);
    expect(row(/^Applications, scanning, Waiting/)).toHaveTextContent(
      "Waiting"
    );
    expect(screen.getByRole("radio", { name: "Largest files" })).toBeDisabled();
    await finish(r);
    // The finished tree replaces the last in-progress poll.
    await waitFor(() =>
      expect(within(contents()).queryByText(/so far|Waiting/)).toBeNull()
    );
    expect(row(/^Other/)).toBeVisible();
    expect(row(/^Users, 234 GB, folder/)).toHaveTextContent("1,434");
    expect(
      screen.getByRole("img", { name: /^Treemap of Macintosh HD/ })
    ).toBeVisible();
    expect(screen.queryByText(/Scanning · about/)).toBeNull();
    expect(
      screen.getByText(/7 locations protected by macOS and 312 folders/)
    ).toBeVisible();
    await r.user.click(screen.getByText("Skipped folders"));
    expect(screen.getByText(/Protected by macOS: Desktop/)).toBeVisible();
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
    expect(screen.getByText(/6 locations protected by macOS/)).toBeVisible();
    await r.user.click(screen.getByText("Skipped folders"));
    const note = screen.getByText(/Protected by macOS:/);
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

/** Disk › Space on the `linux-host` peer, linux-tailscale (D-117). */
function remoteSetup() {
  let t = 1_700_000_000_000;
  const transport = createMockTransport({
    now: () => t,
    autoTick: false,
    scenarios: ["linux-host"],
  });
  const result = renderWithProviders(
    <HostStoreProvider hostId="remote-linux">
      <DiskRoute />
    </HostStoreProvider>,
    { transport, route: "/?view=space" }
  );
  return {
    ...result,
    advance: (ms: number) => {
      t += ms;
    },
  };
}

describe("Disk › Space on a remote Linux host (D-117)", () => {
  it("scans the host's mounts and copies its paths, with no Reveal in Finder", async () => {
    const r = remoteSetup();
    const volumes = await screen.findByRole("region", {
      name: "Volumes to scan",
    });
    for (const mount of ["/", "/boot", "/boot/efi"])
      expect(
        within(volumes).getByRole("button", { name: `Scan ${mount}` })
      ).toBeEnabled();
    // Nothing on Linux is protected the way macOS protects folders.
    expect(screen.queryByText(/Protected locations/)).toBeNull();
    expect(screen.queryByText(/macOS/)).toBeNull();
    await r.user.click(screen.getByRole("button", { name: "Scan /" }));
    await screen.findByText(/Scanning · about/);
    r.advance(MOCK_SCAN_MS);
    await screen.findByRole("button", { name: "Scan again" }, POLL);
    expect(screen.getByText("/ · 141 GB of 155 GB used")).toBeVisible();
    expect(
      screen.getByText(
        /no file accounts for: snapshots and filesystem metadata\./
      )
    ).toBeVisible();
    const path = () => screen.getByRole("navigation", { name: "Folder path" });
    act(() => row(/^var, /).focus());
    await r.user.keyboard("{Enter}");
    await waitFor(() => expect(path()).toHaveTextContent(/\/var$/));
    expect(screen.queryByRole("button", { name: /in Finder$/ })).toBeNull();
    await r.user.click(
      within(row(/^lib, /)).getByRole("button", {
        name: "Copy the path of lib",
      })
    );
    await waitFor(async () =>
      expect(await navigator.clipboard.readText()).toBe("/var/lib")
    );
    // The host's scan is its own: this Mac's stays idle.
    const local = await r.transport.diskSpaceStatus(MOCK_HOST_ID);
    expect(local.status === "ok" && local.data.phase).toBe("idle");
  });

  it("says when the host's Kelvo is too old to answer", async () => {
    const transport = createMockTransport({
      autoTick: false,
      scenarios: ["linux-host"],
    });
    const old = {
      status: "error",
      error: {
        kind: "remote_incompatible",
        message: "this host's Kelvo is too old to answer; update it",
      },
    } as const;
    transport.diskSpaceVolumes = async () => old;
    transport.diskSpaceStatus = async () => old;
    renderWithProviders(
      <HostStoreProvider hostId="remote-linux">
        <DiskRoute />
      </HostStoreProvider>,
      { transport, route: "/?view=space" }
    );
    expect(
      await screen.findByText(
        "This host's Kelvo is too old to share space reports. Update Kelvo on it, then try again."
      )
    ).toBeVisible();
    expect(
      screen.queryByRole("region", { name: "Volumes to scan" })
    ).toBeNull();
  });

  it("says when the host is not connected", async () => {
    const transport = createMockTransport({
      autoTick: false,
      scenarios: ["linux-host"],
    });
    const [linux] = await transport.listRemoteHosts();
    if (!linux) throw new Error("no remote host");
    await transport.updateRemoteHost({ ...linux.config, enabled: false });
    renderWithProviders(
      <HostStoreProvider hostId="remote-linux">
        <DiskRoute />
      </HostStoreProvider>,
      { transport, route: "/?view=space" }
    );
    expect(
      await screen.findByText(
        "This host isn't connected, so its space reports can't be read now."
      )
    ).toBeVisible();
  });

  it("says the report was dropped when the connection to the host ended", async () => {
    const r = remoteSetup();
    await r.user.click(await screen.findByRole("button", { name: "Scan /" }));
    r.advance(MOCK_SCAN_MS);
    await screen.findByRole("button", { name: "Scan again" }, POLL);
    const [linux] = await r.transport.listRemoteHosts();
    if (!linux) throw new Error("no remote host");
    // The session ends: the host drops its report, and the view hears of
    // the connection change on its next read of the host list.
    const reconnect = async (enabled: boolean) => {
      await r.transport.updateRemoteHost({ ...linux.config, enabled });
      await act(() =>
        r.queryClient.invalidateQueries({ queryKey: hostKeys.remote() })
      );
    };
    await reconnect(false);
    expect(
      await screen.findByText(
        "This host isn't connected, so its space reports can't be read now."
      )
    ).toBeVisible();
    await reconnect(true);
    expect(
      await screen.findByText(
        "The report was dropped when the connection to linux-tailscale ended."
      )
    ).toBeVisible();
    expect(
      screen.getByRole("region", { name: "Volumes to scan" })
    ).toBeVisible();
  });

  it("says the report was dropped when a folder of it no longer loads", async () => {
    const r = remoteSetup();
    await r.user.click(await screen.findByRole("button", { name: "Scan /" }));
    r.advance(MOCK_SCAN_MS);
    await screen.findByRole("button", { name: "Scan again" }, POLL);
    // A reconnect too quick for the host list to show it.
    const [linux] = await r.transport.listRemoteHosts();
    if (!linux) throw new Error("no remote host");
    await r.transport.updateRemoteHost({ ...linux.config, enabled: false });
    await r.transport.updateRemoteHost({ ...linux.config, enabled: true });
    act(() => row(/^var, /).focus());
    await r.user.keyboard("{Enter}");
    expect(
      await screen.findByText(
        "The report was dropped when the connection to linux-tailscale ended."
      )
    ).toBeVisible();
    expect(screen.queryByText(/Couldn't load this folder/)).toBeNull();
  });

  it("starts over on the other host's own volumes when switching hosts", async () => {
    const transport = createMockTransport({
      scenarios: ["linux-host"],
      autoTick: false,
    });
    const hosts = await transport.listHosts();
    function Harness() {
      const [selected, select] = useState<string>("remote-linux");
      return (
        <HostSelectionContext value={{ hosts, selected, select }}>
          <HostStoreProvider hostId={selected}>
            <DiskRoute />
          </HostStoreProvider>
          <button type="button" onClick={() => select(MOCK_HOST_ID)}>
            This Mac
          </button>
        </HostSelectionContext>
      );
    }
    const r = renderWithProviders(<Harness />, {
      transport,
      route: "/?view=space",
    });
    expect(
      await screen.findByRole("button", { name: "Scan /boot" })
    ).toBeVisible();
    await r.user.click(screen.getByRole("button", { name: "This Mac" }));
    expect(
      await screen.findByRole("button", { name: "Scan Macintosh HD" })
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Scan /boot" })).toBeNull();
  });
});
