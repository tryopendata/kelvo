import type { DockerSettings, DockerSnapshot } from "@core/generated/bindings";
import { useCallback, useState } from "react";
import { useTransport } from "~/lib/transport-context";
import { dockerErrorMessage } from "../_lib/presentation";
import { mergeDockerSettings } from "../_lib/settings";

export function useDockerConfiguration(
  snapshot: DockerSnapshot | null,
  setSnapshot: (snapshot: DockerSnapshot) => void
) {
  const transport = useTransport();
  const [monitoring, setMonitoring] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const settings = snapshot?.settings;
  const configure = useCallback(
    async (patch: Partial<DockerSettings>): Promise<boolean> => {
      if (!settings) return false;
      try {
        const result = await transport.dockerConfigure(
          mergeDockerSettings(settings, patch)
        );
        if (result.status === "ok") {
          setSnapshot(result.data);
          if (!result.data.engine && patch.enabled) setMonitoring(true);
          return true;
        }
        setActionError(dockerErrorMessage(result.error));
      } catch (e) {
        setActionError(String(e));
      }
      return false;
    },
    [settings, transport, setSnapshot]
  );
  const resume = () => {
    void transport
      .dockerResume()
      .then((result) => {
        if (result.status === "ok") setSnapshot(result.data);
        else setActionError(dockerErrorMessage(result.error));
      })
      .catch((e: unknown) => setActionError(String(e)));
  };
  return { monitoring, setMonitoring, actionError, configure, resume };
}
