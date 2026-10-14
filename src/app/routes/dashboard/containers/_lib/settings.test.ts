import type { DockerSettings } from "@core/generated/bindings";
import { expect, it } from "vitest";
import { mergeDockerSettings } from "./settings";

it("keeps settings when a partial patch explicitly supplies undefined", () => {
  const current: DockerSettings = {
    enabled: true,
    selected_engine: "engine",
    show_popover: true,
    show_tray_count: true,
  };
  expect(
    mergeDockerSettings(current, {
      enabled: undefined,
      selected_engine: undefined,
      show_popover: undefined,
      show_tray_count: undefined,
    })
  ).toEqual(current);
  expect(
    mergeDockerSettings(current, { enabled: false, selected_engine: null })
  ).toEqual({ ...current, enabled: false, selected_engine: null });
});
