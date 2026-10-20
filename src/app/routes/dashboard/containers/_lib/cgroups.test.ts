import { layoutInfo } from "@core/live-state";
import { seriesKey } from "@core/series-key";
import {
  type CgroupRow,
  cgroupLayout,
  cgroupRow,
  inFilter,
  othersLabel,
  sortCgroups,
} from "./cgroups";

const g = (metric: string, kind: string, name: string) =>
  seriesKey(metric, { kind, name });

describe("cgroupLayout", () => {
  it("groups each kind/name's series and splits off the remainder", () => {
    const layout = cgroupLayout(
      layoutInfo(1, [
        g("cgroup.cpu", "docker", "web"),
        g("cgroup.mem", "docker", "web"),
        g("cgroup.mem_max", "docker", "web"),
        g("cgroup.cpu", "service", "sshd.service"),
        g("cgroup.cpu", "other", "_other"),
        seriesKey("cgroup.other_count"),
        seriesKey("cpu.total"),
      ])
    );
    expect(layout.groups.map((x) => x.id)).toEqual([
      "docker/web",
      "service/sshd.service",
    ]);
    expect(layout.groups[0]?.keys).toEqual({
      cpu: "cgroup.cpu{kind=docker,name=web}",
      mem: "cgroup.mem{kind=docker,name=web}",
      memMax: "cgroup.mem_max{kind=docker,name=web}",
    });
    expect(layout.other?.id).toBe("other/_other");
    expect(layout.keys).toContain("cgroup.other_count");
    expect(layout.keys).not.toContain("cpu.total");
  });

  it("hides disk and pressure columns when no group has those series", () => {
    const bare = cgroupLayout(
      layoutInfo(1, [
        g("cgroup.cpu", "docker", "web"),
        g("cgroup.mem", "docker", "web"),
      ])
    );
    expect(bare.io).toBe(false);
    expect(bare.psi).toBe(false);
    expect(bare.other).toBeNull();
    const full = cgroupLayout(
      layoutInfo(1, [
        g("cgroup.io_read", "docker", "web"),
        g("cgroup.mem_psi", "docker", "web"),
      ])
    );
    expect(full.io).toBe(true);
    expect(full.psi).toBe(true);
  });

  it("is empty before a layout", () => {
    expect(cgroupLayout(null).groups).toEqual([]);
  });
});

describe("rows", () => {
  const row = (name: string, cpu: number | null): CgroupRow => ({
    id: `docker/${name}`,
    kind: "docker",
    name,
    cpu,
    mem: null,
    memMax: null,
    ioRead: null,
    ioWrite: null,
    memPsi: null,
  });

  it("reads a missing value as a gap, never 0", () => {
    const [group] = cgroupLayout(
      layoutInfo(1, [g("cgroup.cpu", "docker", "web")])
    ).groups;
    if (!group) throw new Error("no group");
    expect(cgroupRow(group, {}).cpu).toBeNull();
    expect(cgroupRow(group, {}).mem).toBeNull();
  });

  it("sorts gaps last in either direction", () => {
    const rows = [row("a", null), row("b", 1), row("c", 5)];
    const names = (dir: "asc" | "desc") =>
      sortCgroups(rows, { by: "cpu", dir }).map((r) => r.name);
    expect(names("desc")).toEqual(["c", "b", "a"]);
    expect(names("asc")).toEqual(["b", "c", "a"]);
  });

  it("files user services with services and Podman with containers", () => {
    expect(inFilter("user", "services")).toBe(true);
    expect(inFilter("podman", "containers")).toBe(true);
    expect(inFilter("service", "containers")).toBe(false);
    expect(inFilter("docker", "services")).toBe(false);
  });

  it("names the remainder by its count", () => {
    expect(othersLabel(25)).toBe("25 others");
    expect(othersLabel(1)).toBe("1 other");
    expect(othersLabel(null)).toBe("Others");
  });
});
