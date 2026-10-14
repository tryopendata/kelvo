import type { DockerSettings, DockerSnapshot } from "@core/generated/bindings";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "~/components/ui/dialog";
import { EngineChooser } from "./engine-chooser";

export function MonitoringDialog({
  snapshot,
  onClose,
  onChange,
  configure,
}: {
  snapshot: DockerSnapshot;
  onClose: () => void;
  onChange: (snapshot: DockerSnapshot) => void;
  configure: (patch: Partial<DockerSettings>) => Promise<boolean>;
}) {
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogTitle>Monitoring</DialogTitle>
        <DialogDescription>
          Follow containers in one local Docker engine.
        </DialogDescription>
        <EngineChooser
          settings={snapshot.settings}
          onChange={onChange}
          onClose={() => {}}
          embedded
        />
        <section className="space-y-2 text-[12px]">
          <h3 className="text-[13px]">Automatic collection</h3>
          <p className="text-muted-foreground">
            Docker is the source of truth. Kelvo follows this engine’s container
            inventory and starts collecting when containers run, up to{" "}
            {snapshot.aggregate.monitoring_limit} at a time. Recorded history
            remains available after a container stops or is removed.
          </p>
        </section>
        <div className="space-y-3 border-border-subtle border-t pt-3 text-[12px]">
          <label htmlFor="docker-popover" className="flex items-center gap-2">
            <Checkbox
              id="docker-popover"
              checked={snapshot.settings.show_popover ?? false}
              onCheckedChange={(value) =>
                void configure({ show_popover: value === true })
              }
            />
            Show in popover
          </label>
          <label htmlFor="docker-tray" className="flex items-center gap-2">
            <Checkbox
              id="docker-tray"
              checked={snapshot.settings.show_tray_count ?? false}
              onCheckedChange={(value) =>
                void configure({ show_tray_count: value === true })
              }
            />
            Show count in menu bar
          </label>
        </div>
        <div className="flex flex-wrap justify-between gap-2">
          <Button
            variant="ghost"
            onClick={async () => {
              if (await configure({ enabled: false })) onClose();
            }}
          >
            Disable monitoring
          </Button>
          <Button variant="outline" onClick={onClose}>
            Done
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
