import type { DockerSettings } from "@core/generated/bindings";

/** Undefined is omission; null deliberately clears the selected engine. */
export function mergeDockerSettings(
  current: DockerSettings,
  patch: { [K in keyof DockerSettings]?: DockerSettings[K] | undefined }
): DockerSettings {
  const defined = Object.fromEntries(
    Object.entries(patch).filter(([, value]) => value !== undefined)
  );
  return { ...current, ...defined };
}
