import type { ReactNode } from "react";
import { PageHeader } from "~/components/page-header";
import { Button } from "~/components/ui/button";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useHostSelection } from "~/lib/host-selection";

/** Docker v1 reads only this Mac's selected local engine. */
export function DockerLocalOnly({ children }: { children: ReactNode }) {
  const local = useIsLocalHost();
  const selection = useHostSelection();
  if (local) return children;
  const localHost = selection?.hosts.find((host) => host.is_local);
  return (
    <div className="flex flex-col items-start gap-3">
      <PageHeader title="Containers" />
      <p className="text-[13px] text-muted-foreground">
        Container monitoring is available on This Mac. Select This Mac to view
        local containers.
      </p>
      {localHost && (
        <Button onClick={() => selection?.select(localHost.id)}>
          Show containers on This Mac
        </Button>
      )}
    </div>
  );
}
