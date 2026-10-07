import type {
  AppEnergy,
  EnergyByApp,
  ProcessEnergy,
} from "@core/generated/bindings";
import { describe, expect, it } from "vitest";
import { energyRows, partialSince } from "./energy";

function proc(pid: number, name: string, j: number): ProcessEnergy {
  return {
    pid,
    start_time_us: pid,
    name,
    energy_j: j,
    avg_w: j / 600,
    running: true,
    refusal: null,
  };
}

function app(
  name: string,
  processes: ProcessEnergy[],
  quit: number | null
): AppEnergy {
  const j = processes.reduce((s, p) => s + (p.energy_j ?? 0), 0);
  return { name, energy_j: j, avg_w: j / 600, quit_pid: quit, processes };
}

const data: EnergyByApp = {
  from_ms: 0,
  to_ms: 600_000,
  since_ms: 0,
  measured_ms: 600_000,
  total_j: 400,
  apps: [
    app(
      "Google Chrome",
      [
        proc(11, "Google Chrome Helper (Renderer)", 200),
        proc(10, "Google Chrome", 100),
      ],
      10
    ),
    app("node", [proc(5531, "node", 100)], 5531),
  ],
};

describe("energyRows", () => {
  it("shares each app of the total and resolves its Quit target", () => {
    const rows = energyRows(data, "");
    expect(rows.map((r) => [r.app.name, r.share])).toEqual([
      ["Google Chrome", 75],
      ["node", 25],
    ]);
    expect(rows[0]?.quit?.name).toBe("Google Chrome");
    expect(rows[0]?.processes).toHaveLength(2);
  });

  it("quits the running process when an exited one had the same pid", () => {
    const old = { ...proc(10, "Google Chrome", 300), running: false };
    const fresh = { ...proc(10, "Google Chrome", 50), start_time_us: 99 };
    const rows = energyRows(
      { ...data, apps: [app("Google Chrome", [old, fresh], 10)] },
      ""
    );
    expect(rows[0]?.quit?.start_time_us).toBe(99);
  });

  it("keeps the processes a search matched inside an app", () => {
    const rows = energyRows(data, "renderer");
    expect(rows).toHaveLength(1);
    expect(rows[0]?.matchedInside).toBe(true);
    expect(rows[0]?.processes.map((p) => p.pid)).toEqual([11]);
  });

  it("matches an app by name with all its processes, and PIDs by prefix", () => {
    expect(energyRows(data, "chrome")[0]?.processes).toHaveLength(2);
    expect(energyRows(data, "chrome")[0]?.matchedInside).toBe(false);
    expect(energyRows(data, "553").map((r) => r.app.name)).toEqual(["node"]);
    expect(energyRows(data, "safari")).toEqual([]);
  });
});

describe("partialSince", () => {
  it("names when counting started inside the window", () => {
    expect(partialSince(data, 600_000)).toBeNull();
    expect(
      partialSince(
        { ...data, since_ms: 420_000, measured_ms: 180_000 },
        600_000
      )
    ).toEqual({ sinceMs: 420_000, measuredMs: 180_000 });
  });
});
