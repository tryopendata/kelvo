import type { ModuleState, UiModule } from "@core/module-state";
import { popoverSlots } from "./card-order";

const all = (state: ModuleState = "on"): Record<UiModule, ModuleState> => ({
  cpu: state,
  gpu: state,
  memory: state,
  power: state,
  network: state,
  disk: state,
  battery: state,
});

const cards = (states: Record<UiModule, ModuleState>) =>
  popoverSlots(states).map((s) =>
    s.kind === "card" ? s.card : `${s.module}:${s.state}`
  );

describe("popoverSlots", () => {
  it("lists the plan 4.3 order, with no Disk card", () => {
    expect(cards(all())).toEqual([
      "cpu",
      "cores",
      "memory",
      "gpu",
      "power",
      "network",
      "battery",
    ]);
  });

  it("drops switched-off, absent and unknown-chip modules", () => {
    expect(
      cards({
        ...all(),
        cpu: "disabled",
        battery: "absent",
        power: "unknown_chip",
      })
    ).toEqual(["memory", "gpu", "network"]);
  });

  it("gives a module the build cannot run one not-available card", () => {
    expect(cards({ ...all(), cpu: "edition", power: "unavailable" })).toEqual([
      "cpu:edition",
      "memory",
      "gpu",
      "power:unavailable",
      "network",
      "battery",
    ]);
  });
});
